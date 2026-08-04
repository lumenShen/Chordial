//! [`NeteaseCloudMusicApiSource`] — 网易云音乐来源的 [`MusicSource`] 实现。
//!
//! 本文件仅保留来源的**核心**：结构体定义、构造、配置访问、资源模型映射。
//! 搜索 / 详情 / 歌词 / 下载等职责拆分到独立子模块：
//! - [`search`]：歌曲/歌手/专辑搜索
//! - [`detail`]：单曲/歌手/专辑详情
//! - [`lyric`]：歌词获取
//! - [`download`]：音频字节与封面图片获取

use super::cookie::{needs_cookie_normalization, parse_set_cookie_to_header};
use crate::module::music_library::models::{Album, Artist, Lyric, Song};
use crate::module::music_source::traits::MusicSource;
use crate::module::music_source::types::SourceType;
use crate::module::music_web_source::{WebHttpClient, WebSourceConfig};
use crate::module::storage::persistent::PersistentStore;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;

/// 网易云音乐来源的固定名称。
pub const NETEASE_SOURCE_NAME: &str = "netease";

/// 网易云音乐来源。
///
/// 持有一个 [`WebHttpClient`] 实例（连接池复用），配置通过 [`WebSourceConfig`] 持久化。
pub struct NeteaseCloudMusicApiSource {
    /// 来源名称（固定为 `"netease"`）
    pub(super) name: String,
    /// HTTP 客户端（线程安全，连接池复用）
    pub(super) client: WebHttpClient,
    /// 配置（运行时可通过 `update_config` 更新）
    pub(super) config: RwLock<WebSourceConfig>,
    /// 配置持久化存储
    store: Arc<PersistentStore>,
    /// 配置在 PersistentStore 中的 key
    config_key: String,
    /// 封面缓存：entity_id → 图片字节
    /// 避免重复下载（典型 50-500KB/张，限制 128 条 ≈ 6-64MB）
    pub(super) cover_cache: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    /// picUrl 缓存：entity_id → picUrl 字符串
    /// 避免重复调 /song/detail 或 /album 接口拿 picUrl
    pub(super) pic_url_cache: Mutex<HashMap<String, String>>,
}

impl NeteaseCloudMusicApiSource {
    /// 创建网易云来源。
    ///
    /// # 参数
    /// - `store`: 用于持久化配置的存储（通常为 `AppContext.store`）
    pub fn new(store: Arc<PersistentStore>) -> Result<Self, String> {
        let config_key = crate::module::music_web_source::config::config_key(NETEASE_SOURCE_NAME);
        let mut config = WebSourceConfig::load(&store, &config_key);

        // 兼容旧数据：如果 cookie 是 ncmapi 返回的 Set-Cookie 原始字符串
        // （含 Max-Age/Expires/Path 等属性），自动规范化为标准 Cookie 头格式
        if !config.cookie.is_empty() && needs_cookie_normalization(&config.cookie) {
            let original_len = config.cookie.len();
            let normalized = parse_set_cookie_to_header(&config.cookie);
            eprintln!(
                "[netease] 初始化时检测到 Set-Cookie 格式的 cookie，已规范化：{} -> {}",
                original_len,
                normalized.len()
            );
            config.cookie = normalized;
            // 持久化规范化后的 cookie
            let _ = store.set(&config_key, &config);
        }

        let client = WebHttpClient::new(
            config.base_url.clone(),
            config.cookie.clone(),
            config.timeout_secs,
        )?;

        Ok(Self {
            name: NETEASE_SOURCE_NAME.to_string(),
            client,
            config: RwLock::new(config),
            store,
            config_key,
            cover_cache: Mutex::new(HashMap::new()),
            pic_url_cache: Mutex::new(HashMap::new()),
        })
    }

    /// 获取 HTTP 客户端引用（用于 api 模块函数）。
    pub fn client(&self) -> &WebHttpClient {
        &self.client
    }

    /// 获取当前配置的快照。
    pub fn config(&self) -> WebSourceConfig {
        self.config.read().clone()
    }

    /// 更新配置并持久化。
    ///
    /// 会重建 HTTP 客户端以应用新的 base_url / cookie / timeout。
    pub fn update_config(&self, new_config: WebSourceConfig) -> Result<(), String> {
        // 持久化
        new_config.flush(&self.store, &self.config_key)?;

        // 应用到内存（base_url/cookie 可在运行时通过 set_* 更新，无需重建客户端）
        self.client.set_base_url(new_config.base_url.clone());
        self.client.set_cookie(new_config.cookie.clone());
        *self.config.write() = new_config;

        Ok(())
    }

    /// 是否启用（配置中的 `enabled` 字段）。
    pub fn is_enabled(&self) -> bool {
        self.config.read().enabled
    }

    /// 获取下载目录（从配置或默认值）。
    pub fn download_dir(&self) -> String {
        self.config.read().download_dir.clone()
    }
}

impl MusicSource for NeteaseCloudMusicApiSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> SourceType {
        SourceType::Web(NETEASE_SOURCE_NAME.to_string())
    }

    fn search_songs(&self, query: &str) -> Result<Vec<Song>, String> {
        super::search::search_songs(self, query)
    }

    fn get_song(&self, id: &str) -> Result<Option<Song>, String> {
        super::detail::get_song(self, id)
    }

    fn get_artist(&self, id: &str) -> Result<Option<Artist>, String> {
        super::detail::get_artist(self, id)
    }

    fn get_album(&self, id: &str) -> Result<Option<Album>, String> {
        super::detail::get_album(self, id)
    }

    fn get_lyric(&self, song_id: &str) -> Result<Option<Lyric>, String> {
        super::lyric::get_lyric(self, song_id)
    }

    fn song_file_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        super::download::song_file_get(self, entity_id)
    }

    fn song_file_path(&self, _entity_id: &str) -> Option<String> {
        // 网络来源无本地路径；下载后才会落盘到 download_dir
        None
    }

    fn album_picture_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        super::download::album_picture_get(self, entity_id)
    }

    fn lyric_text_get(&self, song_id: &str) -> Result<String, String> {
        super::lyric::lyric_text_get(self, song_id)
    }
}

impl NeteaseCloudMusicApiSource {
    /// 搜索网易云歌手，返回库内 Artist 模型（附带 netease SourceId）。
    pub fn search_artists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Artist>, String> {
        super::search::search_artists(self, query, limit, offset)
    }

    /// 搜索网易云专辑，返回库内 Album 模型（附带 netease SourceId）。
    pub fn search_albums(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Album>, String> {
        super::search::search_albums(self, query, limit, offset)
    }
}