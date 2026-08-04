//! [`KugouMusicApiSource`] — 酷狗音乐来源的 [`MusicSource`] 实现。
//!
//! 本文件仅保留来源的**核心**：结构体定义、构造、配置访问、资源模型映射。
//! 搜索 / 详情 / 歌词 / 下载等职责拆分到独立子模块：
//! - [`search`]：歌曲/歌手/专辑搜索
//! - [`detail`]：单曲/歌手/专辑详情
//! - [`lyric`]：歌词搜索与下载
//! - [`download`]：音频字节与封面图片获取

use super::entity_id::{build_entity_id, EntityKind};
use super::types::KugouSongBrief;
use crate::module::music_library::models::{Album, Artist, Lyric, Song};
use crate::module::music_source::traits::MusicSource;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};
use crate::module::music_web_source::{WebHttpClient, WebSourceConfig};
use crate::module::storage::persistent::PersistentStore;
use parking_lot::RwLock;
use std::sync::Arc;

/// 酷狗音乐来源的固定名称。
pub const KUGOU_SOURCE_NAME: &str = "kugou";

/// 从日期字符串中解析年份。
///
/// 支持格式：`"2024"` / `"2024-01-01"` / `"2024/01/01"` / `"2024.01.01"`。
/// 不支持 Unix 时间戳（酷狗 `publish_date` 一般为字符串格式）。
pub(super) fn parse_year_from_date(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // 取前 4 个数字字符
    let year_str: String = s.chars().take(4).filter(|c| c.is_ascii_digit()).collect();
    if year_str.len() == 4 {
        if let Ok(y) = year_str.parse::<u32>() {
            if (1900..=2100).contains(&y) {
                return Some(y);
            }
        }
    }
    None
}

/// 酷狗音乐来源。
///
/// 持有一个 [`WebHttpClient`] 实例（连接池复用），配置通过 [`WebSourceConfig`] 持久化。
pub struct KugouMusicApiSource {
    /// 来源名称（固定为 `"kugou"`）
    pub(super) name: String,
    /// HTTP 客户端（线程安全，连接池复用）
    pub(super) client: WebHttpClient,
    /// 配置（运行时可通过 `update_config` 更新）
    pub(super) config: RwLock<WebSourceConfig>,
    /// 配置持久化存储
    store: Arc<PersistentStore>,
    /// 配置在 PersistentStore 中的 key
    config_key: String,
}

impl KugouMusicApiSource {
    /// 创建酷狗来源。
    pub fn new(store: Arc<PersistentStore>) -> Result<Self, String> {
        let config_key =
            crate::module::music_web_source::config::config_key(KUGOU_SOURCE_NAME);
        let config = WebSourceConfig::load(&store, &config_key);

        let client = WebHttpClient::new(
            config.base_url.clone(),
            config.cookie.clone(),
            config.timeout_secs,
        )?;

        Ok(Self {
            name: KUGOU_SOURCE_NAME.to_string(),
            client,
            config: RwLock::new(config),
            store,
            config_key,
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
    pub fn update_config(&self, new_config: WebSourceConfig) -> Result<(), String> {
        self.client.set_base_url(new_config.base_url.clone());
        self.client.set_cookie(new_config.cookie.clone());
        new_config.flush(&self.store, &self.config_key)?;
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

    /// 从 [`KugouSongBrief`] 构建 [`Song`] 模型。
    ///
    /// 使用确定性 ID（基于 `hash` / `singer_id` / `album_id`）替代随机 UUID，
    /// 这样同一首酷狗歌曲重复导入时不会产生重复条目，便于库内去重与跨实体引用。
    pub(super) fn build_song(&self, brief: &KugouSongBrief) -> Song {
        let hash = brief.file_hash_lower();
        let entity_id = build_entity_id(EntityKind::song, &hash);
        // 库内 ID：使用确定性前缀 + hash，确保同一首酷狗歌曲 ID 稳定
        let song_id = format!("kugou_song_{}", hash);

        let artist_names: Vec<String> = if brief.SingerName.is_empty() {
            vec!["未知艺术家".to_string()]
        } else {
            // 酷狗 SingerName 可能是 "歌手1/歌手2" 分隔
            brief
                .SingerName
                .split('/')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };

        // artist_ids：使用 singer_id 构造确定性 ID
        // 酷狗搜索结果中 SingerId 是单个，artist_names 可能含多名歌手（按 / 分隔），
        // 此处只能关联到主歌手 ID；多歌手精确关联需要 get_song 时分别查询
        let artist_ids: Vec<String> = if brief.SingerId > 0 {
            vec![format!("kugou_artist_{}", brief.SingerId)]
        } else {
            Vec::new()
        };

        let album_title = if brief.AlbumName.is_empty() {
            None
        } else {
            Some(brief.AlbumName.clone())
        };
        let album_id = if !brief.AlbumID.is_empty() && brief.AlbumID != "0" {
            Some(format!("kugou_album_{}", brief.AlbumID))
        } else {
            None
        };
        // lyric_id 仅在实际获取到歌词时填充，此处留 None
        let lyric_id = None;

        let source_id = SourceId {
            source_name: self.name.clone(),
            source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
            entity_type: EntityType::Song,
            entity_id: entity_id.clone(),
        };

        // 封面 URL：酷狗搜索结果不直接返回封面 URL，
        // 需要时通过 album_picture_get 调 /album 接口获取 sizable_cover
        // 这里留 None，避免对每首歌都触发 /album 调用导致限流
        Song {
            id: song_id,
            title: brief.SongName.clone(),
            artist_names,
            album_title,
            duration: if brief.SongTime > 0 {
                Some(brief.SongTime as u64)
            } else {
                None
            },
            artist_ids,
            album_id,
            lyric_id,
            source_ids: vec![source_id],
            year: None,
            cover_url: None,
        }
    }

    /// 从 [`KugouSongBrief`] 构建 [`Song`]（公开版本，供 Tauri 命令使用）。
    pub fn build_song_from_brief(&self, brief: &KugouSongBrief) -> Song {
        self.build_song(brief)
    }
}

impl MusicSource for KugouMusicApiSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> SourceType {
        SourceType::Web(KUGOU_SOURCE_NAME.to_string())
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
        None
    }

    fn album_picture_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        super::download::album_picture_get(self, entity_id)
    }

    fn lyric_text_get(&self, song_id: &str) -> Result<String, String> {
        super::lyric::lyric_text_get(self, song_id)
    }
}

impl KugouMusicApiSource {
    /// 搜索酷狗歌手，返回库内 Artist 模型。
    pub fn search_artists(
        &self,
        query: &str,
        page: u32,
        pagesize: u32,
    ) -> Result<Vec<Artist>, String> {
        super::search::search_artists(self, query, page, pagesize)
    }

    /// 搜索酷狗专辑，返回库内 Album 模型。
    pub fn search_albums(
        &self,
        query: &str,
        page: u32,
        pagesize: u32,
    ) -> Result<Vec<Album>, String> {
        super::search::search_albums(self, query, page, pagesize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::music_netease_source::time::{
        is_leap_year,
        publish_time_to_year as year_from_millis,
    };

    #[test]
    fn test_parse_year_from_date() {
        assert_eq!(parse_year_from_date("2024"), Some(2024));
        assert_eq!(parse_year_from_date("2024-01-01"), Some(2024));
        assert_eq!(parse_year_from_date("2024/01/01"), Some(2024));
        assert_eq!(parse_year_from_date("2024.01.01"), Some(2024));
        assert_eq!(parse_year_from_date(""), None);
        assert_eq!(parse_year_from_date("abc"), None);
        // 超出合理范围
        assert_eq!(parse_year_from_date("1800"), None);
        assert_eq!(parse_year_from_date("2200"), None);
    }

    #[test]
    fn test_year_from_millis() {
        // 1970-01-01 00:00:00 UTC = 0 ms
        assert_eq!(year_from_millis(0), None);
        // 2024-01-01 ≈ 1704067200000 ms
        assert_eq!(year_from_millis(1_704_067_200_000), Some(2024));
        // 1970-01-02 ≈ 86400000 ms
        assert_eq!(year_from_millis(86_400_000), Some(1970));
        // 1971-01-01 ≈ 31536000000 ms (365 days)
        assert_eq!(year_from_millis(31_536_000_000), Some(1971));
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000)); // 能被 400 整除
        assert!(is_leap_year(2024)); // 能被 4 整除但不被 100
        assert!(!is_leap_year(1900)); // 能被 100 整除但不被 400
        assert!(!is_leap_year(2023));
    }
}