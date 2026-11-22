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

pub mod client;
pub mod mapping;
pub mod service;

pub use client::NeteaseClient;
pub use mapping::SongQualityLevel;
pub use service::{Endpoint, ManagedConfig, NeteaseMode, NeteaseService};

/// 适配器配置（UI 配置文件反序列化用）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

impl Inner {
    async fn search_inner(
        &self,
        source_id: &SourceId,
        query: &SearchQuery,
    ) -> CoreResult<SearchResults> {
        let body = self
            .client
            .get_json(
                "/search",
                &[
                    ("keywords", query.text.as_str()),
                    ("type", "1"),
                    ("limit", &query.limit.to_string()),
                    ("offset", &query.offset.to_string()),
                ],
            )
            .await?;
        mapping::map_search(source_id, &body)
    }

    async fn fetch_track(&self, source_id: &SourceId, id: &TrackId) -> CoreResult<Track> {
        let body = self
            .client
            .get_json("/song/detail", &[("ids", id.id.as_str())])
            .await?;
        mapping::map_song_detail(source_id, &body, &id.id)
    }

    /// 先返回磁盘缓存（stale-while-revalidate），过期则后台刷新。
    async fn track_inner(
        self: &Arc<Self>,
        source_id: &SourceId,
        id: &TrackId,
    ) -> CoreResult<Track> {
        let key = object_key(source_id.as_str(), "track", &id.id);
        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && let Some(track) = cache.get_json::<Track>(&key)
        {
            if now_secs().saturating_sub(meta.fetched_at) >= TRACK_REFRESH_AFTER_SECS {
                self.spawn_track_refresh(source_id.clone(), id.clone(), key);
            }
            return Ok(track);
        }

        let track = self.fetch_track(source_id, id).await?;
        if let Some(cache) = &self.cache {
            cache.put_json(&key, &track, CacheTags::default());
        }
        Ok(track)
    }

    fn spawn_track_refresh(self: &Arc<Self>, source_id: SourceId, id: TrackId, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            match inner.fetch_track(&source_id, &id).await {
                Ok(track) => {
                    if let Some(cache) = &inner.cache {
                        let outcome = cache.put_json(&key, &track, CacheTags::default());
                        tracing::debug!(
                            target: "chordial::cache",
                            key = %key,
                            ?outcome,
                            "netease track refreshed"
                        );
                    }
                }
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "netease track background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }

    async fn fetch_lyrics(&self, id: &TrackId) -> CoreResult<Option<Lyrics>> {
        // 新接口能拿到逐词 YRC；旧接口作为兼容回退。
        let new_body = self
            .client
            .get_json("/lyric/new", &[("id", id.id.as_str())])
            .await?;
        if let Some(lyrics) = mapping::map_lyrics_new(&new_body) {
            return Ok(Some(lyrics));
        }
        let body = self
            .client
            .get_json("/lyric", &[("id", id.id.as_str())])
            .await?;
        Ok(mapping::map_lyrics(&body))
    }

    async fn lyrics_inner(self: &Arc<Self>, id: &TrackId) -> CoreResult<Option<Lyrics>> {
        let key = object_key(id.source.as_str(), "lyric", &id.id);
        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && let Some(obj) = cache.get(&key)
            && let Ok(cached) = serde_json::from_slice::<Option<Lyrics>>(&obj.bytes)
        {
            if now_secs().saturating_sub(meta.fetched_at) >= LYRIC_REFRESH_AFTER_SECS {
                self.spawn_lyric_refresh(id.clone(), key);
            }
            return Ok(cached);
        }

        let lyrics = self.fetch_lyrics(id).await?;
        if let Some(cache) = &self.cache {
            cache.put_json(&key, &lyrics, CacheTags::default());
        }
        Ok(lyrics)
    }

    fn spawn_lyric_refresh(self: &Arc<Self>, id: TrackId, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            match inner.fetch_lyrics(&id).await {
                Ok(lyrics) => {
                    if let Some(cache) = &inner.cache {
                        let outcome = cache.put_json(&key, &lyrics, CacheTags::default());
                        tracing::debug!(
                            target: "chordial::cache",
                            key = %key,
                            ?outcome,
                            "netease lyric refreshed"
                        );
                    }
                }
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "netease lyric background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }

    async fn fetch_stream(&self, id: &TrackId) -> CoreResult<ResolvedStream> {
        let _scope = PerfScope::enter("netease.stream_resolve");
        let body = self
            .client
            .get_json(
                "/song/url/v1",
                &[
                    ("id", id.id.as_str()),
                    ("level", self.level.as_query_value()),
                ],
            )
            .await?;
        mapping::map_song_url(&body, &id.id)
    }

    /// 直链有时效：内存/磁盘都只在未过期时复用；临近过期后台续期。
    async fn resolve_stream_inner(self: &Arc<Self>, id: &TrackId) -> CoreResult<ResolvedStream> {
        let now = now_secs();
        let key = object_key(id.source.as_str(), "stream", &id.id);

        if let Some(hit) = self
            .url_cache
            .lock()
            .expect("url cache lock poisoned")
            .get(&id.id)
            .cloned()
            && hit.expires_at > now
        {
            if hit.expires_at.saturating_sub(now) < URL_REFRESH_AHEAD_SECS {
                self.spawn_stream_refresh(id.clone(), key);
            }
            return Ok(hit.resolved);
        }

        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && !meta.is_expired(now)
            && let Some(resolved) = cache.get_json::<ResolvedStream>(&key)
        {
            let expires_at = meta.expires_at.unwrap_or(now + URL_TTL_SECS);
            self.url_cache.lock().expect("url cache lock poisoned").put(
                id.id.clone(),
                CachedStream {
                    resolved: resolved.clone(),
                    expires_at,
                },
            );
            if expires_at.saturating_sub(now) < URL_REFRESH_AHEAD_SECS {
                self.spawn_stream_refresh(id.clone(), key);
            }
            return Ok(resolved);
        }

        let resolved = self.fetch_stream(id).await?;
        self.store_stream(id, &key, &resolved);
        Ok(resolved)
    }

    fn store_stream(&self, id: &TrackId, key: &str, resolved: &ResolvedStream) {
        let expires_at = now_secs().saturating_add(URL_TTL_SECS);
        self.url_cache.lock().expect("url cache lock poisoned").put(
            id.id.clone(),
            CachedStream {
                resolved: resolved.clone(),
                expires_at,
            },
        );
        if let Some(cache) = &self.cache {
            cache.put_json(
                key,
                resolved,
                CacheTags {
                    expires_at: Some(expires_at),
                    ..Default::default()
                },
            );
        }
    }

    fn spawn_stream_refresh(self: &Arc<Self>, id: TrackId, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            match inner.fetch_stream(&id).await {
                Ok(resolved) => inner.store_stream(&id, &key, &resolved),
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "netease stream background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }

    // ---------- 账号 / 集合 ----------

    fn account_state(&self) -> AccountState {
        self.session
            .read()
            .map(|s| s.clone())
            .unwrap_or(AccountState::Unknown)
    }

    fn set_account_state(&self, state: AccountState) {
        let changed = {
            let Ok(current) = self.session.read() else {
                return;
            };
            match (&*current, &state) {
                (AccountState::LoggedIn(a), AccountState::LoggedIn(b)) => a.uid != b.uid,
                (a, b) => a != b,
            }
        };
        if let Ok(mut current) = self.session.write() {
            *current = state;
        }
        if changed {
            self.session_epoch.fetch_add(1, Ordering::Release);
        }
    }

    fn account_key(&self, uid: &str, kind: &str, id: &str) -> String {
        format!("{}/account/{}/{}/{}", self.source_id, uid, kind, id)
    }

    async fn purge_account_cache(&self) {
        if let Some(cache) = &self.cache {
            let account_prefix = format!("{}/account/", self.source_id);
            let avatar_prefix = format!("{}-account-", self.source_id);
            let playlist_prefix = format!("{}/collection/Playlist/", self.source_id);
            let cache = cache.clone();
            let _ = shared_runtime()
                .spawn_blocking(move || {
                    cache.purge_prefix(&account_prefix);
                    cache.purge_prefix(&avatar_prefix);
                    cache.purge_prefix(&playlist_prefix);
                })
                .await;
        }
    }

    async fn fetch_account_profile(&self) -> CoreResult<Option<AccountProfile>> {
        let body = self
            .client
            .get_json_with_codes("/user/account", &[], &[200, 301])
            .await?;
        Ok(mapping::map_account(&self.source_id, &body))
    }

    async fn refresh_account_inner(&self) -> CoreResult<Option<AccountProfile>> {
        if self.client.cookie().is_none() {
            self.set_account_state(AccountState::LoggedOut);
            self.purge_account_cache().await;
            return Ok(None);
        }
        match self.fetch_account_profile().await {
            Ok(Some(profile)) => {
                self.set_account_state(AccountState::LoggedIn(profile.clone()));
                Ok(Some(profile))
            }
            Ok(None) => {
                // 服务端明确说未登录：旧 Cookie 已失效，账号级缓存一并回收。
                self.client.set_cookie(None);
                self.set_account_state(AccountState::LoggedOut);
                self.purge_account_cache().await;
                Ok(None)
            }
            Err(e) => {
                // 网络/服务不可达时不要误清 Cookie。
                self.set_account_state(AccountState::Unknown);
                Err(e)
            }
        }
    }

    async fn login_begin_inner(&self, method: LoginMethod) -> CoreResult<LoginChallenge> {
        match method {
            LoginMethod::Qr => {
                // 防缓存 timestamp 由 NeteaseClient 统一附加（api-enhanced
                // 的 apicache 会缓存未登录结果，扫码流程尤其不能命中旧响应）。
                let key_body = self.client.get_json("/login/qr/key", &[]).await?;
                let key = key_body
                    .get("data")
                    .and_then(|d| d.get("unikey"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| CoreError::source_err("netease", "qr key missing"))?
                    .to_string();
                let create = self
                    .client
                    .get_json(
                        "/login/qr/create",
                        &[("key", key.as_str()), ("qrimg", "true")],
                    )
                    .await?;
                let qr_data_url = create
                    .get("data")
                    .and_then(|d| d.get("qrimg"))
                    .and_then(serde_json::Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                Ok(LoginChallenge {
                    method: LoginMethod::Qr,
                    key: Some(key),
                    qr_data_url,
                    expires_at: Some(now_secs() + 180),
                })
            }
            LoginMethod::CellphoneCaptcha { .. } => Err(CoreError::Config(
                "手机号登录暂未开放，请使用扫码登录".into(),
            )),
        }
    }

    async fn login_poll_inner(&self, challenge: &LoginChallenge) -> CoreResult<LoginProgress> {
        let Some(key) = challenge.key.as_deref() else {
            return Ok(LoginProgress::Failed("登录挑战缺少 key".into()));
        };
        let body = self
            .client
            .get_json_with_codes("/login/qr/check", &[("key", key)], &[800, 801, 802, 803])
            .await?;
        let code = body
            .get("code")
            .and_then(serde_json::Value::as_i64)
            .or_else(|| {
                body.get("data")
                    .and_then(|d| d.get("code"))
                    .and_then(serde_json::Value::as_i64)
            })
            .unwrap_or(-1);
        match code {
            800 => Ok(LoginProgress::Expired),
            801 => Ok(LoginProgress::Waiting),
            802 => Ok(LoginProgress::Scanned),
            803 => {
                let cookie = body
                    .get("cookie")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        body.get("data")
                            .and_then(|d| d.get("cookie"))
                            .and_then(serde_json::Value::as_str)
                    })
                    .filter(|c| !c.trim().is_empty())
                    .map(str::to_string);
                let Some(cookie) = cookie else {
                    return Ok(LoginProgress::Failed(
                        "登录成功但服务端未返回 Cookie".into(),
                    ));
                };
                self.client.set_cookie(Some(cookie));
                match self.fetch_account_profile().await {
                    Ok(Some(profile)) => {
                        // 切号/重新登录：旧账号的歌单缓存不得复用。
                        self.purge_account_cache().await;
                        self.set_account_state(AccountState::LoggedIn(profile.clone()));
                        Ok(LoginProgress::Success(profile))
                    }
                    Ok(None) => {
                        self.client.set_cookie(None);
                        Ok(LoginProgress::Failed("Cookie 已获取但账号校验失败".into()))
                    }
                    Err(e) => Ok(LoginProgress::Failed(e.to_string())),
                }
            }
            other => Ok(LoginProgress::Failed(format!(
                "未知登录状态 {other}: {}",
                body.get("message")
                    .or_else(|| body.get("msg"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
            ))),
        }
    }

    async fn logout_inner(&self) -> CoreResult<()> {
        let _ = self
            .client
            .get_json_with_codes("/logout", &[], &[200, 301])
            .await;
        self.client.set_cookie(None);
        self.set_account_state(AccountState::LoggedOut);
        self.purge_account_cache().await;
        Ok(())
    }

    async fn send_captcha_inner(&self, phone: &str) -> CoreResult<()> {
        self.client
            .get_json("/captcha/sent", &[("phone", phone)])
            .await?;
        Ok(())
    }

    async fn login_cellphone_inner(
        &self,
        phone: &str,
        captcha: &str,
    ) -> CoreResult<AccountProfile> {
        let body = self
            .client
            .get_json(
                "/login/cellphone",
                &[("phone", phone), ("captcha", captcha)],
            )
            .await?;
        let cookie = body
            .get("cookie")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                body.get("data")
                    .and_then(|d| d.get("cookie"))
                    .and_then(serde_json::Value::as_str)
            })
            .filter(|c| !c.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                CoreError::source_err("netease", "手机号登录成功但服务端未返回 Cookie")
            })?;
        self.client.set_cookie(Some(cookie));
        let profile = self
            .fetch_account_profile()
            .await?
            .ok_or_else(|| CoreError::source_err("netease", "手机号登录后账号校验失败"))?;
        self.purge_account_cache().await;
        self.set_account_state(AccountState::LoggedIn(profile.clone()));
        Ok(profile)
    }

    async fn fetch_user_playlists(&self, uid: &str) -> CoreResult<Vec<CollectionSummary>> {
        let body = self
            .client
            .get_json(
                "/user/playlist",
                &[("uid", uid), ("limit", "1000"), ("offset", "0")],
            )
            .await?;
        Ok(mapping::map_user_playlists(&self.source_id, &body))
    }

    async fn user_playlists_inner(self: &Arc<Self>) -> CoreResult<Vec<CollectionSummary>> {
        let Some(profile) = self.account_state().profile().cloned() else {
            return Err(CoreError::Config("尚未登录网易云".into()));
        };
        let key = self.account_key(&profile.uid, "playlists", "all");
        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && let Some(list) = cache.get_json::<Vec<CollectionSummary>>(&key)
        {
            if now_secs().saturating_sub(meta.fetched_at) >= ACCOUNT_REFRESH_AFTER_SECS {
                self.spawn_user_playlists_refresh(profile.uid, key);
            }
            return Ok(list);
        }
        let list = self.fetch_user_playlists(&profile.uid).await?;
        if let Some(cache) = &self.cache {
            cache.put_json(&key, &list, CacheTags::default());
        }
        Ok(list)
    }

    fn spawn_user_playlists_refresh(self: &Arc<Self>, uid: String, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            // 账号切换后旧 uid 的结果不得写回新会话。
            let epoch = inner.session_epoch.load(Ordering::Acquire);
            match inner.fetch_user_playlists(&uid).await {
                Ok(list) if inner.session_epoch.load(Ordering::Acquire) == epoch => {
                    if let Some(cache) = &inner.cache {
                        cache.put_json(&key, &list, CacheTags::default());
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "user playlists background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }

    async fn fetch_collection_page(&self, id: &CollectionId) -> CoreResult<CollectionPage> {
        match id.kind {
            CollectionKind::Playlist => {
                let detail = self
                    .client
                    .get_json("/playlist/detail", &[("id", id.id.as_str())])
                    .await?;
                let tracks = self
                    .client
                    .get_json(
                        "/playlist/track/all",
                        &[("id", id.id.as_str()), ("limit", "1000"), ("offset", "0")],
                    )
                    .await?;
                mapping::map_playlist_page(&self.source_id, &detail, &tracks)
            }
            CollectionKind::Album => {
                let body = self
                    .client
                    .get_json("/album", &[("id", id.id.as_str())])
                    .await?;
                mapping::map_album_page(&self.source_id, &body)
            }
            CollectionKind::Artist => {
                let detail = self
                    .client
                    .get_json("/artist/detail", &[("id", id.id.as_str())])
                    .await?;
                let songs = self
                    .client
                    .get_json("/artist/top/song", &[("id", id.id.as_str())])
                    .await?;
                let albums = self
                    .client
                    .get_json(
                        "/artist/album",
                        &[("id", id.id.as_str()), ("limit", "50"), ("offset", "0")],
                    )
                    .await?;
                mapping::map_artist_page(&self.source_id, &detail, &songs, &albums)
            }
        }
    }

    fn collection_key(&self, id: &CollectionId) -> String {
        format!("{}/collection/{:?}/{}", self.source_id, id.kind, id.id)
    }

    async fn collection_inner(self: &Arc<Self>, id: &CollectionId) -> CoreResult<CollectionPage> {
        let key = self.collection_key(id);
        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && let Some(page) = cache.get_json::<CollectionPage>(&key)
        {
            if now_secs().saturating_sub(meta.fetched_at) >= COLLECTION_REFRESH_AFTER_SECS {
                self.spawn_collection_refresh(id.clone(), key);
            }
            return Ok(page);
        }
        let page = self.fetch_collection_page(id).await?;
        if let Some(cache) = &self.cache {
            cache.put_json(&key, &page, CacheTags::default());
        }
        Ok(page)
    }

    fn spawn_collection_refresh(self: &Arc<Self>, id: CollectionId, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            let epoch = inner.session_epoch.load(Ordering::Acquire);
            match inner.fetch_collection_page(&id).await {
                Ok(page) if inner.session_epoch.load(Ordering::Acquire) == epoch => {
                    if let Some(cache) = &inner.cache {
                        cache.put_json(&key, &page, CacheTags::default());
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "collection background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }

    async fn collection_tracks_inner(
        self: &Arc<Self>,
        id: &CollectionId,
        offset: u32,
        limit: u32,
    ) -> CoreResult<TrackPage> {
        match id.kind {
            CollectionKind::Playlist => {
                let limit_s = limit.to_string();
                let offset_s = offset.to_string();
                let body = self
                    .client
                    .get_json(
                        "/playlist/track/all",
                        &[
                            ("id", id.id.as_str()),
                            ("limit", &limit_s),
                            ("offset", &offset_s),
                        ],
                    )
                    .await?;
                Ok(mapping::map_songs_page(&self.source_id, &body, offset))
            }
            _ => {
                let page = self.collection_inner(id).await?;
                let total = page.total;
                let tracks: Vec<Track> = page
                    .tracks
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .collect();
                Ok(TrackPage {
                    has_more: (offset as u64 + tracks.len() as u64) < total,
                    tracks,
                    total,
                    offset,
                })
            }
        }
    }

    async fn search_collections_inner(
        &self,
        query: &SearchQuery,
    ) -> CoreResult<Vec<CollectionSummary>> {
        let kind = match query.kind {
            SearchKind::Track => return Ok(Vec::new()),
            SearchKind::Album => "10",
            SearchKind::Artist => "100",
            SearchKind::Playlist => "1000",
        };
        let limit = query.limit.to_string();
        let offset = query.offset.to_string();
        let body = self
            .client
            .get_json(
                "/cloudsearch",
                &[
                    ("keywords", query.text.as_str()),
                    ("type", kind),
                    ("limit", &limit),
                    ("offset", &offset),
                ],
            )
            .await?;
        Ok(mapping::map_search_collections(
            &self.source_id,
            &body,
            query.kind,
        ))
    }

    fn begin_refresh(&self, key: &str) -> bool {
        self.refreshing
            .lock()
            .expect("refresh lock poisoned")
            .insert(key.to_string())
    }

    fn end_refresh(&self, key: &str) {
        self.refreshing
            .lock()
            .expect("refresh lock poisoned")
            .remove(key);
    }
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

#[async_trait]
impl MusicSource for NeteaseSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::remote_streaming()
    }

    fn catalog(&self) -> Option<&dyn CatalogSource> {
        Some(self)
    }

    fn account(&self) -> Option<&dyn AccountSource> {
        Some(self)
    }

    async fn search(&self, query: &SearchQuery) -> CoreResult<SearchResults> {
        let inner = self.inner.clone();
        let source_id = self.info.id.clone();
        let query = query.clone();
        spawn_inner(move || async move { inner.search_inner(&source_id, &query).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn track(&self, id: &TrackId) -> CoreResult<Track> {
        let inner = self.inner.clone();
        let source_id = self.info.id.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.track_inner(&source_id, &id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn stream(&self, id: &TrackId) -> CoreResult<StreamHandle> {
        let inner = self.inner.clone();
        let id = id.clone();
        let resolved = spawn_inner(move || async move { inner.resolve_stream_inner(&id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))??;
        Ok(resolved.handle)
    }

    async fn resolve_stream(&self, id: &TrackId) -> CoreResult<ResolvedStream> {
        let inner = self.inner.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.resolve_stream_inner(&id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn lyrics(&self, id: &TrackId) -> CoreResult<Option<Lyrics>> {
        let inner = self.inner.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.lyrics_inner(&id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    /// 源卸载 = 回收该 namespace 的全部缓存对象（歌单/歌词/直链/封面），
    /// 并停止托管的 Node 进程（除非 managed.keep_warm）。
    async fn shutdown(&self) {
        self.inner
            .url_cache
            .lock()
            .expect("url cache lock poisoned")
            .clear();
        if let Some(service) = &self.inner.service {
            let service = service.clone();
            // 卸载可能发生在 gpui executor 上，进程操作回到共享 tokio runtime。
            let _ = shared_runtime()
                .spawn(async move { service.stop_if_not_keep_warm().await })
                .await;
        }
        if let Some(cache) = &self.inner.cache {
            let cache = cache.clone();
            let namespace = self.info.id.as_str().to_string();
            let avatar_prefix = format!("{namespace}-account-");
            // 大缓存删除不能阻塞 UI executor：交给共享 runtime 的阻塞池。
            let _ = shared_runtime()
                .spawn_blocking(move || {
                    cache.purge_namespace(&namespace);
                    cache.purge_prefix(&avatar_prefix);
                })
                .await;
        }
    }

    /// 应用退出：只停 Node，不清缓存、不注销账号。
    async fn stop_services(&self) {
        self.inner
            .url_cache
            .lock()
            .expect("url cache lock poisoned")
            .clear();
        if let Some(service) = &self.inner.service {
            let service = service.clone();
            let _ = shared_runtime()
                .spawn(async move { service.stop().await })
                .await;
        }
    }

    /// UI 加载源后预热：托管模式下会启动/安装 api-enhanced。
    async fn warm_up(&self) -> CoreResult<()> {
        let inner = self.inner.clone();
        spawn_inner(move || async move {
            if let Some(service) = &inner.service {
                service.ensure_ready().await?;
            }
            Ok(())
        })
        .await
        .map_err(|e| join_error(self.info.id.as_str(), e))?
    }
}

#[async_trait]
impl CatalogSource for NeteaseSource {
    async fn collection(&self, id: &CollectionId) -> CoreResult<CollectionPage> {
        let inner = self.inner.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.collection_inner(&id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn collection_tracks(
        &self,
        id: &CollectionId,
        offset: u32,
        limit: u32,
    ) -> CoreResult<TrackPage> {
        let inner = self.inner.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.collection_tracks_inner(&id, offset, limit).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn search_collections(&self, query: &SearchQuery) -> CoreResult<Vec<CollectionSummary>> {
        let inner = self.inner.clone();
        let query = query.clone();
        spawn_inner(move || async move { inner.search_collections_inner(&query).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }
}

#[async_trait]
impl AccountSource for NeteaseSource {
    fn account_state(&self) -> AccountState {
        self.inner.account_state()
    }

    async fn refresh_account(&self) -> CoreResult<Option<AccountProfile>> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.refresh_account_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn login_begin(&self, method: LoginMethod) -> CoreResult<LoginChallenge> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.login_begin_inner(method).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn login_poll(&self, challenge: &LoginChallenge) -> CoreResult<LoginProgress> {
        let inner = self.inner.clone();
        let challenge = challenge.clone();
        spawn_inner(move || async move { inner.login_poll_inner(&challenge).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn send_captcha(&self, phone: &str) -> CoreResult<()> {
        let inner = self.inner.clone();
        let phone = phone.to_string();
        spawn_inner(move || async move { inner.send_captcha_inner(&phone).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn login_cellphone(&self, phone: &str, captcha: &str) -> CoreResult<AccountProfile> {
        let inner = self.inner.clone();
        let phone = phone.to_string();
        let captcha = captcha.to_string();
        spawn_inner(move || async move { inner.login_cellphone_inner(&phone, &captcha).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn logout(&self) -> CoreResult<()> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.logout_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn user_playlists(&self) -> CoreResult<Vec<CollectionSummary>> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.user_playlists_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    fn cookie(&self) -> Option<String> {
        self.inner.client.cookie()
    }
}

/// 进程级共享 tokio runtime：所有 HTTP 适配器复用，2 个 worker 足够
/// （瓶颈在网络而非 CPU），避免每个源各起一套 runtime 的线程浪费。
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

pub mod lru {
    //! 定容量 LRU：HashMap + 时间戳惰性淘汰队列，get/put 均摊 O(1)。

    use std::collections::{HashMap, VecDeque};

    pub struct LruCache<K, V> {
        map: HashMap<K, (V, u64)>,
        order: VecDeque<(K, u64)>,
        capacity: usize,
        tick: u64,
    }

    impl<K, V> LruCache<K, V>
    where
        K: std::hash::Hash + Eq + Clone,
    {
        pub fn new(capacity: usize) -> Self {
            Self {
                map: HashMap::with_capacity(capacity),
                order: VecDeque::with_capacity(capacity),
                capacity: capacity.max(1),
                tick: 0,
            }
        }

        pub fn get(&mut self, key: &K) -> Option<&V> {
            self.tick += 1;
            let entry = self.map.get_mut(key)?;
            entry.1 = self.tick;
            self.order.push_back((key.clone(), self.tick));
            Some(&entry.0)
        }

        pub fn put(&mut self, key: K, value: V) {
            self.tick += 1;
            if self.map.len() >= self.capacity && !self.map.contains_key(&key) {
                // 惰性淘汰：从队首弹出已过期（时间戳仍是最后一次 push 时）的键。
                while let Some((old_key, old_stamp)) = self.order.pop_front() {
                    match self.map.get(&old_key) {
                        Some((_, stamp)) if *stamp == old_stamp => {
                            self.map.remove(&old_key);
                            break;
                        }
                        _ => continue,
                    }
                }
            }
            self.map.insert(key.clone(), (value, self.tick));
            self.order.push_back((key, self.tick));
        }

        pub fn len(&self) -> usize {
            self.map.len()
        }

        pub fn clear(&mut self) {
            self.map.clear();
            self.order.clear();
        }

        pub fn is_empty(&self) -> bool {
            self.map.is_empty()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn eviction_is_bounded_and_lru() {
            let mut c = LruCache::new(2);
            c.put("a", 1);
            c.put("b", 2);
            assert_eq!(c.get(&"a"), Some(&1)); // a 变最近使用
            c.put("c", 3); // 淘汰 b（最久未用）
            assert_eq!(c.len(), 2);
            assert_eq!(c.get(&"b"), None);
            assert_eq!(c.get(&"a"), Some(&1));
            assert_eq!(c.get(&"c"), Some(&3));
        }

        #[test]
        fn repeated_access_keeps_capacity_bounded() {
            let mut c = LruCache::new(16);
            for i in 0..1000u32 {
                c.put(format!("k{i}"), i);
            }
            assert_eq!(c.len(), 16);
            for _ in 0..10_000 {
                let _ = c.get(&"k999".to_string());
            }
            assert_eq!(c.len(), 16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chordal_cache::CacheConfig;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn config_defaults() {
        let c = NeteaseConfig::default();
        assert!(c.base_url.contains("127.0.0.1:3000"));
        assert!(c.cookie.is_none());
    }

    /// 扫码登录：key → create → check(803) → user/account；Cookie 必须注入。
    #[test]
    fn qr_login_sets_cookie_and_profile() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("");
                // api-enhanced 的 apicache 会缓存未登录响应；扫码/校验链路
                // 每个请求都必须带 cache-buster，否则会读到旧的 801/匿名账号。
                assert!(
                    path.contains("timestamp="),
                    "missing cache buster in {path}"
                );
                let cookie = request
                    .lines()
                    .find(|line| line.to_ascii_lowercase().starts_with("cookie:"))
                    .unwrap_or("");
                let body = if path.starts_with("/login/qr/key") {
                    r#"{"code":200,"data":{"unikey":"k1"}}"#
                } else if path.starts_with("/login/qr/create") {
                    r#"{"code":200,"data":{"qrurl":"u","qrimg":"data:image/png;base64,AAA"}}"#
                } else if path.starts_with("/login/qr/check") {
                    // api-enhanced 的 cookie 是整串 Set-Cookie（带属性）拼接。
                    r#"{"code":803,"cookie":"MUSIC_U=abc; Max-Age=15552000; Path=/; __csrf=zzz"}"#
                } else if path.starts_with("/user/account") {
                    assert!(
                        !cookie.contains("Max-Age") && !cookie.contains("Path="),
                        "cookie attributes leaked into request: {cookie}"
                    );
                    if cookie.contains("MUSIC_U=abc") {
                        r#"{"code":200,"account":{"id":42},"profile":{"userId":42,"nickname":"n","vipType":0}}"#
                    } else {
                        r#"{"code":301}"#
                    }
                } else {
                    r#"{"code":404}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        let source = NeteaseSource::new(
            SourceId::new("netease"),
            NeteaseConfig {
                base_url: format!("http://{addr}"),
                cookie: None,
                level: None,
                mode: NeteaseMode::External,
                ..Default::default()
            },
            "test",
        );
        let account = source.account().expect("account source");
        let challenge = shared_runtime()
            .block_on(account.login_begin(LoginMethod::Qr))
            .unwrap();
        assert_eq!(challenge.key.as_deref(), Some("k1"));
        let progress = shared_runtime()
            .block_on(account.login_poll(&challenge))
            .unwrap();
        match progress {
            LoginProgress::Success(profile) => {
                assert_eq!(profile.uid, "42");
                assert_eq!(profile.nickname, "n");
            }
            other => panic!("unexpected login progress: {other:?}"),
        }
        assert!(account.account_state().is_logged_in());
        assert!(account.cookie().is_some_and(|c| c.contains("MUSIC_U=abc")));
        server.join().unwrap();
    }

    /// 手机号登录：captcha/sent → login/cellphone → user/account。
    #[test]
    fn cellphone_login_sets_cookie_and_profile() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("");
                let cookie = request
                    .lines()
                    .find(|line| line.to_ascii_lowercase().starts_with("cookie:"))
                    .unwrap_or("");
                let body = if path.starts_with("/captcha/sent") {
                    r#"{"code":200}"#
                } else if path.starts_with("/login/cellphone") {
                    r#"{"code":200,"cookie":"MUSIC_U=phone;"}"#
                } else if path.starts_with("/user/account") {
                    if cookie.contains("MUSIC_U=phone") {
                        r#"{"code":200,"account":{"id":7},"profile":{"userId":7,"nickname":"手机用户"}}"#
                    } else {
                        r#"{"code":301}"#
                    }
                } else {
                    r#"{"code":404}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });

        let source = NeteaseSource::new(
            SourceId::new("netease"),
            NeteaseConfig {
                base_url: format!("http://{addr}"),
                cookie: None,
                level: None,
                mode: NeteaseMode::External,
                ..Default::default()
            },
            "test",
        );
        let account = source.account().expect("account source");
        shared_runtime()
            .block_on(account.send_captcha("13800000000"))
            .unwrap();
        let profile = shared_runtime()
            .block_on(account.login_cellphone("13800000000", "123456"))
            .unwrap();
        assert_eq!(profile.uid, "7");
        assert_eq!(profile.nickname, "手机用户");
        assert!(account.account_state().is_logged_in());
        assert!(
            account
                .cookie()
                .is_some_and(|c| c.contains("MUSIC_U=phone"))
        );
        server.join().unwrap();
    }

    /// 单曲详情缓存：第一次请求落盘，服务关闭后第二次仍能返回。
    #[test]
    fn track_cache_serves_after_server_stops() {
        let root =
            std::env::temp_dir().join(format!("chordial-netease-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cache = Arc::new(
            ObjectCache::open(CacheConfig {
                root: root.clone(),
                limit_bytes: 8 * 1024 * 1024,
            })
            .unwrap(),
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            // 只服务一次；第二次调用必须命中磁盘缓存。
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let body = r#"{"code":200,"songs":[{"id":123,"name":"海阔天空","ar":[{"id":1,"name":"Beyond"}],"al":{"id":2,"name":"乐与怒","picUrl":null},"dt":326000,"no":1}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });

        let source = NeteaseSource::new_with_cache(
            SourceId::new("netease"),
            NeteaseConfig {
                base_url: format!("http://{addr}"),
                cookie: None,
                level: None,
                mode: NeteaseMode::External,
                ..Default::default()
            },
            "test",
            Some(cache.clone()),
        );
        let id = TrackId::new(SourceId::new("netease"), "123");
        let first = shared_runtime().block_on(source.track(&id)).unwrap();
        assert_eq!(first.title, "海阔天空");
        assert_eq!(first.primary_artist(), "Beyond");
        server.join().unwrap();

        let second = shared_runtime().block_on(source.track(&id)).unwrap();
        assert_eq!(second.title, first.title);
        assert!(cache.stats().entries >= 1);

        drop(source);
        let _ = std::fs::remove_dir_all(&root);
    }
}
