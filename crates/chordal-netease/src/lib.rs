//! 网易云音乐适配器 —— 对接自部署的
//! [api-enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced)
//! Node.js HTTP 服务。
//!
//! 契约：本 crate 只把"HTTP API 服务"适配成 [`MusicSource`]；用户自行部署
//! api-enhanced（默认 `http://127.0.0.1:3000`），本适配器不做协议加密逆向，
//! 不直连网易服务器。
//!
//! 运行时模型：每个进程一个共享 tokio Runtime（2 worker，[`shared_runtime`]）。
//! trait 的 async 方法克隆 `Arc<Inner>` 后把内部 future `spawn` 到该 runtime，
//! 再 await 其 JoinHandle —— future 持所有权而非借用，因此不需要 unsafe
//! 生命周期把戏，在 gpui executor / tokio / 任意 executor 下轮询均安全。
//!
//! 性能：
//! - 每 HTTP 请求带 `netease.request` 检查点、10s 超时、一次退避重试；
//! - 直链解析结果进固定容量 LRU（[`lru`]），重播同一首 O(1) 免网络往返。

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use chordal_cache::{CacheTags, ObjectCache, now_secs, object_key};
use chordal_core::error::{CoreError, CoreResult};
use chordal_core::model::{
    AccountProfile, AccountState, Capabilities, CollectionId, CollectionKind, CollectionPage,
    CollectionSummary, LoginChallenge, LoginMethod, LoginProgress, Lyrics, ResolvedStream,
    SearchKind, SearchQuery, SearchResults, SourceId, SourceKind, StreamHandle, Track, TrackId,
    TrackPage,
};
use chordal_core::perf::PerfScope;
use chordal_core::source::{AccountSource, CatalogSource, MusicSource, SourceInfo};

mod inner;
mod inner_account;
mod inner_collections;
mod inner_feeds;
pub mod lru;
mod traits;

pub mod client;
pub mod mapping;
pub mod service;

pub use client::NeteaseClient;
pub use mapping::SongQualityLevel;
pub use service::{Endpoint, ManagedConfig, NeteaseMode, NeteaseService};

/// 适配器配置（UI 配置文件反序列化用）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NeteaseConfig {
    /// api-enhanced 服务地址（External 模式 / Auto 回退）。
    pub base_url: String,
    /// 登录 Cookie（`MUSIC_U=...`），可空 —— 未登录只能搜到可试听曲目。
    pub cookie: Option<String>,
    /// 请求音质档位（standard/higher/exhigh/lossless/hires）。
    pub level: Option<SongQualityLevel>,
    /// 外部服务 / 托管 Node 进程 / 自动。
    #[serde(default)]
    pub mode: NeteaseMode,
    /// 托管模式配置。
    #[serde(default)]
    pub managed: ManagedConfig,
}

impl Default for NeteaseConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:3000".into(),
            cookie: None,
            level: None,
            mode: NeteaseMode::Auto,
            managed: ManagedConfig::default(),
        }
    }
}

/// 网易云直链在服务端通常 ~20 分钟过期；缓存取 10 分钟留安全余量。
const URL_TTL_SECS: u64 = 10 * 60;
/// 单曲详情/歌词的后台刷新阈值。
const TRACK_REFRESH_AFTER_SECS: u64 = 5 * 60;
const LYRIC_REFRESH_AFTER_SECS: u64 = 24 * 60 * 60;
/// 直链即将过期时提前后台刷新。
const URL_REFRESH_AHEAD_SECS: u64 = 2 * 60;
/// 用户歌单 / 集合页的后台刷新阈值。
const ACCOUNT_REFRESH_AFTER_SECS: u64 = 60;
const COLLECTION_REFRESH_AFTER_SECS: u64 = 5 * 60;

/// 解析后的直链 + 音质 + 过期时间（内存 LRU / 磁盘缓存共用）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CachedStream {
    resolved: ResolvedStream,
    expires_at: u64,
}

/// 共享内部状态：被 trait 方法 spawn 的 future 以 `Arc<Inner>` 持有。
struct Inner {
    source_id: SourceId,
    client: NeteaseClient,
    /// 托管 Node 进程（External/Auto 回退时为 None）。
    service: Option<Arc<NeteaseService>>,
    level: SongQualityLevel,
    /// 歌曲直链 LRU：capacity 256，get/put 均摊 O(1)。
    url_cache: std::sync::Mutex<lru::LruCache<String, CachedStream>>,
    /// 跨会话对象缓存（单曲详情 / 歌词 / 直链）。
    cache: Option<Arc<ObjectCache>>,
    /// 防止同一 key 并发后台刷新。
    refreshing: std::sync::Mutex<HashSet<String>>,
    /// 账号会话状态。Cookie 是会话本体，服务端无状态。
    session: std::sync::RwLock<AccountState>,
    /// 登录/登出代数：异步结果回写前检查，避免旧账号数据污染新会话。
    session_epoch: AtomicU64,
}

/// 网易云源。
pub struct NeteaseSource {
    info: SourceInfo,
    inner: Arc<Inner>,
}

impl NeteaseSource {
    pub fn new(id: SourceId, config: NeteaseConfig, display_name: impl Into<String>) -> Arc<Self> {
        Self::new_with_cache(id, config, display_name, None)
    }

    /// 带共享对象缓存的构造。缓存 key 以源 id 为 namespace，
    /// 卸载源时按 namespace 回收。
    pub fn new_with_cache(
        id: SourceId,
        config: NeteaseConfig,
        display_name: impl Into<String>,
        cache: Option<Arc<ObjectCache>>,
    ) -> Arc<Self> {
        let info = SourceInfo {
            description: match config.mode {
                NeteaseMode::Managed => "网易云 · 托管 api-enhanced（Node sidecar）".to_string(),
                _ => format!("网易云 · api-enhanced @ {}", config.base_url),
            },
            display_name: display_name.into(),
            id: id.clone(),
            kind: SourceKind::HttpApi,
            priority: 1,
        };
        let level = config.level.unwrap_or(SongQualityLevel::ExHigh);
        let session = if config.cookie.is_some() {
            AccountState::Unknown
        } else {
            AccountState::LoggedOut
        };
        let endpoint = Arc::new(Endpoint::new(config.base_url.clone()));
        let service = NeteaseService::from_config(config.mode, &config.managed, endpoint.clone());
        let client =
            NeteaseClient::new_with_endpoint(endpoint, config.cookie.as_deref(), service.clone());
        Arc::new(Self {
            info,
            inner: Arc::new(Inner {
                source_id: id.clone(),
                client,
                service,
                level,
                url_cache: std::sync::Mutex::new(lru::LruCache::new(256)),
                cache,
                refreshing: std::sync::Mutex::new(HashSet::new()),
                session: std::sync::RwLock::new(session),
                session_epoch: AtomicU64::new(0),
            }),
        })
    }

    /// 服务可达性探测（UI 加载源前可选调用）。任何 HTTP 响应都算存活。
    pub async fn health(&self) -> bool {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.client.health().await })
            .await
            .unwrap_or(false)
    }
}

pub fn shared_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("netease shared tokio runtime")
    })
}

/// 把持有 `Arc<Inner>` 的 future 送到共享 runtime 执行，
/// 并在当前 executor（gpui / tokio / 其它）上 await 其 JoinHandle。
async fn spawn_inner<T, F>(make: impl FnOnce() -> F) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: std::future::Future<Output = T> + Send + 'static,
{
    shared_runtime().spawn(make()).await
}

fn join_error(source: &str, e: tokio::task::JoinError) -> CoreError {
    CoreError::source_err(source, format!("task join failed: {e}"))
}

#[cfg(test)]
mod tests;
