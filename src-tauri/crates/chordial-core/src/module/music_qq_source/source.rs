//! [`QqMusicApiSource`] — QQ 音乐来源的 [`MusicSource`] 实现。

use super::api;
use super::types::{QqAlbumBrief, QqArtistBrief, QqSongBrief};
use crate::module::music_library::models::{Album, Artist, Lyric, Song};
use crate::module::music_source::traits::MusicSource;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};
use crate::module::music_web_source::{WebHttpClient, WebSourceConfig};
use crate::module::storage::persistent::PersistentStore;
use parking_lot::RwLock;
use std::sync::Arc;
use uuid::Uuid;

/// QQ 音乐来源的固定名称。
pub const QQ_SOURCE_NAME: &str = "qq";

/// entity_id 中各部分的分隔符。
const ENTITY_ID_SEP: &str = ":";

/// entity_id 的种类前缀。
#[allow(non_camel_case_types)]
enum EntityKind {
    song,
    artist,
    album,
    lyric,
}

impl EntityKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::song => "song",
            Self::artist => "artist",
            Self::album => "album",
            Self::lyric => "lyric",
        }
    }
}

/// 构建 QQ 音乐 entity_id。
fn build_entity_id(kind: EntityKind, id: &str) -> String {
    format!("{}{}{}", kind.as_str(), ENTITY_ID_SEP, id)
}

/// 解析 entity_id 中的远程 ID（去掉 `kind:` 前缀后的部分）。
fn parse_remote_id(entity_id: &str) -> Option<String> {
    let parts: Vec<&str> = entity_id.splitn(3, ENTITY_ID_SEP).collect();
    if parts.len() < 2 {
        return None;
    }
    Some(parts[1].to_string())
}

/// QQ 音乐默认音质字符串（320 kbps MP3）。
fn default_quality() -> &'static str {
    "320"
}

/// QQ 音乐专辑封面 URL 模板（300x300）。
///
/// 通过 albummid 拼接官方 CDN 路径，避免对每首歌都触发 /album 接口调用导致限流。
fn album_cover_url_by_mid(albummid: &str) -> Option<String> {
    if albummid.is_empty() {
        return None;
    }
    Some(format!(
        "https://y.gtimg.cn/music/photo_new/T002R300x300M000{}.jpg",
        albummid
    ))
}

/// 歌手头像 URL 模板（300x300）。
fn singer_photo_url_by_mid(singermid: &str) -> Option<String> {
    if singermid.is_empty() {
        return None;
    }
    Some(format!(
        "https://y.gtimg.cn/music/photo_new/T001R300x300M000{}.jpg",
        singermid
    ))
}

/// QQ 音乐来源。
///
/// 持有一个 [`WebHttpClient`] 实例（连接池复用），配置通过 [`WebSourceConfig`] 持久化。
pub struct QqMusicApiSource {
    /// 来源名称（固定为 `"qq"`）
    name: String,
    /// HTTP 客户端（线程安全，连接池复用）
    client: WebHttpClient,
    /// 配置（运行时可通过 `update_config` 更新）
    config: RwLock<WebSourceConfig>,
    /// 配置持久化存储
    store: Arc<PersistentStore>,
    /// 配置在 PersistentStore 中的 key
    config_key: String,
}

impl QqMusicApiSource {
    /// 创建 QQ 音乐来源。
    pub fn new(store: Arc<PersistentStore>) -> Result<Self, String> {
        let config_key = crate::module::music_web_source::config::config_key(QQ_SOURCE_NAME);
        let config = WebSourceConfig::load(&store, &config_key);

        let client = WebHttpClient::new(
            config.base_url.clone(),
            config.cookie.clone(),
            config.timeout_secs,
        )?;

        Ok(Self {
            name: QQ_SOURCE_NAME.to_string(),
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

    /// 从 [`QqSongBrief`] 构建 [`Song`] 模型。
    fn build_song(&self, brief: &QqSongBrief) -> Song {
        let entity_id = build_entity_id(EntityKind::song, &brief.songmid);
        let song_id = Uuid::new_v4().to_string();
        let artist_names = brief.artist_names();
        let artist_ids: Vec<String> = (0..artist_names.len())
            .map(|_| Uuid::new_v4().to_string())
            .collect();
        let album_title = if brief.albumname.is_empty() {
            None
        } else {
            Some(brief.albumname.clone())
        };
        let album_id = album_title.as_ref().map(|_| Uuid::new_v4().to_string());
        let lyric_id = Some(Uuid::new_v4().to_string());

        let source_id = SourceId {
            source_name: self.name.clone(),
            source_type: SourceType::Web(QQ_SOURCE_NAME.to_string()),
            entity_type: EntityType::Song,
            entity_id: entity_id.clone(),
        };

        // 封面 URL：优先使用搜索结果中的 img 字段；
        // 若缺失，通过 albummid 拼接官方 CDN 路径（避免对每首歌触发 /album 调用）
        let cover_url = album_cover_url_by_mid(&brief.albummid);

        Song {
            id: song_id,
            title: brief.songname.clone(),
            artist_names,
            album_title,
            duration: if brief.interval > 0 {
                Some(brief.interval as u64)
            } else {
                None
            },
            artist_ids,
            album_id,
            lyric_id,
            source_ids: vec![source_id],
            year: None,
            cover_url,
        }
    }
}

impl MusicSource for QqMusicApiSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> SourceType {
        SourceType::Web(QQ_SOURCE_NAME.to_string())
    }

    fn search_songs(&self, query: &str) -> Result<Vec<Song>, String> {
        if !self.is_enabled() {
            return Err("QQ 音乐来源未启用".to_string());
        }
        let briefs = api::search_songs(&self.client, query, 1, 30)?;
        Ok(briefs.iter().map(|b| self.build_song(b)).collect())
    }

    fn get_song(&self, id: &str) -> Result<Option<Song>, String> {
        if !self.is_enabled() {
            return Err("QQ 音乐来源未启用".to_string());
        }
        // id 是 song:<songmid>；通过 /song/url 验证 songmid 有效
        let songmid = parse_remote_id(id).unwrap_or_default();
        if songmid.is_empty() {
            return Ok(None);
        }
        match api::get_song_url(&self.client, &songmid, default_quality()) {
            Ok(_) => Ok(Some(Song {
                id: Uuid::new_v4().to_string(),
                title: String::new(),
                artist_names: vec!["未知艺术家".to_string()],
                album_title: None,
                duration: None,
                artist_ids: vec![Uuid::new_v4().to_string()],
                album_id: None,
                lyric_id: Some(Uuid::new_v4().to_string()),
                source_ids: vec![SourceId {
                    source_name: self.name.clone(),
                    source_type: SourceType::Web(QQ_SOURCE_NAME.to_string()),
                    entity_type: EntityType::Song,
                    entity_id: id.to_string(),
                }],
                year: None,
                cover_url: None,
            })),
            Err(_) => Ok(None),
        }
    }

    fn get_artist(&self, id: &str) -> Result<Option<Artist>, String> {
        if !self.is_enabled() {
            return Err("QQ 音乐来源未启用".to_string());
        }
        let singermid = parse_remote_id(id).unwrap_or_default();
        if singermid.is_empty() {
            return Ok(None);
        }
        // QQ 音乐 /singer/desc 接口返回歌手介绍；
        // 简化实现：仅基于 mid 构造 Artist，名称为空（前端从搜索结果获取完整信息）
        Ok(Some(Artist {
            id: Uuid::new_v4().to_string(),
            name: String::new(),
            bio: None,
            source_ids: vec![SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(QQ_SOURCE_NAME.to_string()),
                entity_type: EntityType::Artist,
                entity_id: build_entity_id(EntityKind::artist, &singermid),
            }],
        }))
    }

    fn get_album(&self, id: &str) -> Result<Option<Album>, String> {
        if !self.is_enabled() {
            return Err("QQ 音乐来源未启用".to_string());
        }
        let albummid = parse_remote_id(id).unwrap_or_default();
        if albummid.is_empty() {
            return Ok(None);
        }
        let info = api::get_album_info(&self.client, &albummid)?;
        let cover_url = album_cover_url_by_mid(info.mid())
            .filter(|s| !s.is_empty());
        Ok(Some(Album {
            id: Uuid::new_v4().to_string(),
            title: info.name().to_string(),
            artist_id: String::new(),
            cover_url,
            song_ids: Vec::new(),
            source_ids: vec![SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(QQ_SOURCE_NAME.to_string()),
                entity_type: EntityType::Album,
                entity_id: build_entity_id(EntityKind::album, &albummid),
            }],
            year: None,
        }))
    }

    fn get_lyric(&self, song_id: &str) -> Result<Option<Lyric>, String> {
        if !self.is_enabled() {
            return Err("QQ 音乐来源未启用".to_string());
        }
        // song_id 可能是 entity_id (song:<songmid>) 或纯 songmid
        let songmid = if song_id.starts_with("song:") {
            parse_remote_id(song_id).unwrap_or_default()
        } else {
            song_id.to_string()
        };
        if songmid.is_empty() {
            return Ok(None);
        }

        let resp = api::get_lyric(&self.client, &songmid)?;
        if resp.lyric.is_empty() {
            return Ok(None);
        }

        // 如果有翻译，附加到歌词末尾（与网易云一致的分隔符方案，前端 lyricConverter.js 处理）
        let final_text = if !resp.trans.is_empty() {
            format!("{}\n[translations]\n{}", resp.lyric, resp.trans)
        } else {
            resp.lyric
        };

        let entity_id = build_entity_id(EntityKind::lyric, &songmid);
        Ok(Some(Lyric {
            id: Uuid::new_v4().to_string(),
            song_id: song_id.to_string(),
            text: final_text,
            source_id: SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(QQ_SOURCE_NAME.to_string()),
                entity_type: EntityType::Lyric,
                entity_id,
            },
        }))
    }

    fn song_file_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        if !self.is_enabled() {
            return Err("QQ 音乐来源未启用".to_string());
        }
        let songmid = parse_remote_id(entity_id).unwrap_or_default();
        if songmid.is_empty() {
            return Err(format!("无效的 entity_id: {}", entity_id));
        }
        // 默认 320 kbps MP3
        let (bytes, _url_data) = api::download_song(&self.client, &songmid, default_quality())?;
        Ok(bytes)
    }

    fn song_file_path(&self, _entity_id: &str) -> Option<String> {
        None
    }

    fn album_picture_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        if !self.is_enabled() {
            return Err("QQ 音乐来源未启用".to_string());
        }
        // entity_id 可能是 http(s):// URL，直接下载
        if entity_id.starts_with("http://") || entity_id.starts_with("https://") {
            return self.client.get_bytes(entity_id);
        }

        // album:<mid> 或 song:<songmid>
        // song:<songmid> 无法直接获取专辑封面（需先查 albummid），
        // 简化实现：仅支持 album:<mid> 和直接 URL
        let albummid = if entity_id.starts_with("album:") {
            parse_remote_id(entity_id).unwrap_or_default()
        } else {
            return Err(format!(
                "QQ album_picture_get 暂仅支持 album:<mid>，收到: {}",
                entity_id
            ));
        };

        if albummid.is_empty() {
            return Err(format!("无效的专辑 ID: {}", entity_id));
        }

        let cover_url = album_cover_url_by_mid(&albummid)
            .ok_or_else(|| "专辑无封面 URL".to_string())?;
        self.client.get_bytes(&cover_url)
    }

    fn lyric_text_get(&self, song_id: &str) -> Result<String, String> {
        let lyric = self
            .get_lyric(song_id)?
            .ok_or_else(|| "未找到歌词".to_string())?;
        Ok(lyric.text)
    }
}

// ── 非 trait 方法（供 Tauri 命令使用）────────────────────

impl QqMusicApiSource {
    /// 搜索 QQ 歌手，返回库内 Artist 模型。
    pub fn search_artists(
        &self,
        query: &str,
        page_no: u32,
        page_size: u32,
    ) -> Result<Vec<Artist>, String> {
        if !self.is_enabled() {
            return Err("QQ 音乐来源未启用".to_string());
        }
        let briefs = api::search_artists(&self.client, query, page_no, page_size)?;
        Ok(briefs
            .into_iter()
            .map(|b: QqArtistBrief| {
                let mid = b.singer_mid().to_string();
                Artist {
                    id: Uuid::new_v4().to_string(),
                    name: b.singer_name().to_string(),
                    bio: None,
                    source_ids: vec![SourceId {
                        source_name: self.name.clone(),
                        source_type: SourceType::Web(QQ_SOURCE_NAME.to_string()),
                        entity_type: EntityType::Artist,
                        entity_id: build_entity_id(EntityKind::artist, &mid),
                    }],
                }
            })
            .collect())
    }

    /// 搜索 QQ 专辑，返回库内 Album 模型。
    pub fn search_albums(
        &self,
        query: &str,
        page_no: u32,
        page_size: u32,
    ) -> Result<Vec<Album>, String> {
        if !self.is_enabled() {
            return Err("QQ 音乐来源未启用".to_string());
        }
        let briefs = api::search_albums(&self.client, query, page_no, page_size)?;
        Ok(briefs
            .into_iter()
            .map(|b: QqAlbumBrief| {
                let mid = b.album_mid().to_string();
                let cover = b.image_url().to_string();
                Album {
                    id: Uuid::new_v4().to_string(),
                    title: b.album_name().to_string(),
                    artist_id: String::new(),
                    cover_url: if cover.is_empty() {
                        album_cover_url_by_mid(&mid)
                    } else {
                        Some(cover)
                    },
                    song_ids: Vec::new(),
                    source_ids: vec![SourceId {
                        source_name: self.name.clone(),
                        source_type: SourceType::Web(QQ_SOURCE_NAME.to_string()),
                        entity_type: EntityType::Album,
                        entity_id: build_entity_id(EntityKind::album, &mid),
                    }],
                    year: None,
                }
            })
            .collect())
    }

    /// 从 [`QqSongBrief`] 构建 [`Song`]（公开版本，供 Tauri 命令使用）。
    #[allow(dead_code)]
    pub fn build_song_from_brief(&self, brief: &QqSongBrief) -> Song {
        self.build_song(brief)
    }

    /// 获取歌手头像 URL（供前端直接作为 `<img src>`）。
    #[allow(dead_code)]
    pub fn singer_photo_url(&self, singermid: &str) -> Option<String> {
        singer_photo_url_by_mid(singermid)
    }
}
