//! [`KugouMusicApiSource`] — 酷狗音乐来源的 [`MusicSource`] 实现。

use super::api;
use super::types::KugouSongBrief;
use crate::module::music_library::models::{Album, Artist, Lyric, Song};
use crate::module::music_source::traits::MusicSource;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};
use crate::module::music_web_source::{WebHttpClient, WebSourceConfig};
use crate::module::storage::persistent::PersistentStore;
use parking_lot::RwLock;
use std::sync::Arc;
use uuid::Uuid;

/// 酷狗音乐来源的固定名称。
pub const KUGOU_SOURCE_NAME: &str = "kugou";

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

/// 构建酷狗 entity_id。
fn build_entity_id(kind: EntityKind, id: &str) -> String {
    format!("{}{}{}", kind.as_str(), ENTITY_ID_SEP, id)
}

/// 构建歌词的 entity_id（携带 id + accesskey 复合键）。
fn build_lyric_entity_id(lyric_id: &str, accesskey: &str) -> String {
    format!(
        "{}{}{}{}{}",
        EntityKind::lyric.as_str(),
        ENTITY_ID_SEP,
        lyric_id,
        ENTITY_ID_SEP,
        accesskey
    )
}

/// 解析 entity_id 中的远程 ID（去掉 `kind:` 前缀后的部分）。
/// 对于 `lyric:<id>:<accesskey>` 格式，仅返回第一段。
fn parse_remote_id(entity_id: &str) -> Option<String> {
    let parts: Vec<&str> = entity_id.splitn(3, ENTITY_ID_SEP).collect();
    if parts.len() < 2 {
        return None;
    }
    Some(parts[1].to_string())
}

/// 酷狗音质字符串映射到内部 AudioQuality 枚举的等级字符串。
fn default_quality() -> &'static str {
    "320"
}

/// 酷狗音乐来源。
///
/// 持有一个 [`WebHttpClient`] 实例（连接池复用），配置通过 [`WebSourceConfig`] 持久化。
pub struct KugouMusicApiSource {
    /// 来源名称（固定为 `"kugou"`）
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
    fn build_song(&self, brief: &KugouSongBrief) -> Song {
        let hash = brief.file_hash_lower();
        let entity_id = build_entity_id(EntityKind::song, &hash);
        let song_id = Uuid::new_v4().to_string();
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
        let artist_ids: Vec<String> = (0..artist_names.len())
            .map(|_| Uuid::new_v4().to_string())
            .collect();
        let album_title = if brief.AlbumName.is_empty() {
            None
        } else {
            Some(brief.AlbumName.clone())
        };
        let album_id = album_title.as_ref().map(|_| Uuid::new_v4().to_string());
        let lyric_id = Some(Uuid::new_v4().to_string());

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
}

impl MusicSource for KugouMusicApiSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> SourceType {
        SourceType::Web(KUGOU_SOURCE_NAME.to_string())
    }

    fn search_songs(&self, query: &str) -> Result<Vec<Song>, String> {
        if !self.is_enabled() {
            return Err("酷狗来源未启用".to_string());
        }
        let briefs = api::search_songs(&self.client, query, 1, 30)?;
        Ok(briefs.iter().map(|b| self.build_song(b)).collect())
    }

    fn get_song(&self, id: &str) -> Result<Option<Song>, String> {
        if !self.is_enabled() {
            return Err("酷狗来源未启用".to_string());
        }
        // id 是 song:<hash>；通过 /search/lyric 等接口无法直接获取歌曲详情
        // 这里通过 hash 搜索歌词间接验证歌曲存在性，并构造 Song
        // 简化实现：直接基于 entity_id 构造 Song（hash 作为唯一标识）
        let hash = parse_remote_id(id).unwrap_or_default();
        if hash.is_empty() {
            return Ok(None);
        }
        // 通过 /song/url 验证 hash 有效，但不下载
        match api::get_song_url(&self.client, &hash, default_quality(), None) {
            Ok(_) => Ok(Some(Song {
                id: Uuid::new_v4().to_string(),
                title: String::new(), // 无法从 URL 接口获取标题
                artist_names: vec!["未知艺术家".to_string()],
                album_title: None,
                duration: None,
                artist_ids: vec![Uuid::new_v4().to_string()],
                album_id: None,
                lyric_id: Some(Uuid::new_v4().to_string()),
                source_ids: vec![SourceId {
                    source_name: self.name.clone(),
                    source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
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
            return Err("酷狗来源未启用".to_string());
        }
        let singer_id = parse_remote_id(id)
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        if singer_id == 0 {
            return Ok(None);
        }
        // 酷狗无直接获取歌手详情的接口（/artist/detail 不存在），
        // 简化实现：仅基于 ID 构造 Artist，名称为空（前端从搜索结果获取完整信息）
        Ok(Some(Artist {
            id: Uuid::new_v4().to_string(),
            name: String::new(),
            bio: None,
            source_ids: vec![SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
                entity_type: EntityType::Artist,
                entity_id: build_entity_id(EntityKind::artist, &singer_id.to_string()),
            }],
        }))
    }

    fn get_album(&self, id: &str) -> Result<Option<Album>, String> {
        if !self.is_enabled() {
            return Err("酷狗来源未启用".to_string());
        }
        let album_id = parse_remote_id(id).unwrap_or_default();
        if album_id.is_empty() || album_id == "0" {
            return Ok(None);
        }
        let info = api::get_album_info(&self.client, &album_id)?;
        Ok(Some(Album {
            id: Uuid::new_v4().to_string(),
            title: info.album_name,
            artist_id: String::new(),
            cover_url: if !info.sizable_cover.is_empty() {
                Some(info.sizable_cover)
            } else if !info.cover.is_empty() {
                Some(info.cover)
            } else {
                None
            },
            song_ids: Vec::new(),
            source_ids: vec![SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
                entity_type: EntityType::Album,
                entity_id: build_entity_id(EntityKind::album, &album_id),
            }],
            year: None,
        }))
    }

    fn get_lyric(&self, song_id: &str) -> Result<Option<Lyric>, String> {
        if !self.is_enabled() {
            return Err("酷狗来源未启用".to_string());
        }
        // song_id 可能是 entity_id (song:<hash>) 或纯 hash
        let hash = if song_id.starts_with("song:") {
            parse_remote_id(song_id).unwrap_or_default()
        } else {
            song_id.to_string()
        };
        if hash.is_empty() {
            return Ok(None);
        }

        // 第一步：搜索歌词候选
        let candidates = api::search_lyric(&self.client, &hash, 0)?;
        let candidate = candidates
            .into_iter()
            .next()
            .ok_or_else(|| "未找到歌词候选".to_string())?;

        // 第二步：下载歌词
        let lyric_resp = api::get_lyric(&self.client, &candidate.id, &candidate.accesskey)?;
        let lyric_text = if !lyric_resp.decodeContent.is_empty() {
            lyric_resp.decodeContent
        } else if !lyric_resp.content.is_empty() {
            // 兜底：base64 解码 content 字段
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(&lyric_resp.content)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_default()
        } else {
            return Ok(None);
        };

        if lyric_text.is_empty() {
            return Ok(None);
        }

        let entity_id = build_lyric_entity_id(&candidate.id, &candidate.accesskey);
        Ok(Some(Lyric {
            id: Uuid::new_v4().to_string(),
            song_id: song_id.to_string(),
            text: lyric_text,
            source_id: SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
                entity_type: EntityType::Lyric,
                entity_id,
            },
        }))
    }

    fn song_file_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        if !self.is_enabled() {
            return Err("酷狗来源未启用".to_string());
        }
        let hash = parse_remote_id(entity_id).unwrap_or_default();
        if hash.is_empty() {
            return Err(format!("无效的 entity_id: {}", entity_id));
        }
        // 默认 320 kbps MP3
        let (bytes, _url_data) = api::download_song(&self.client, &hash, default_quality(), None)?;
        Ok(bytes)
    }

    fn song_file_path(&self, _entity_id: &str) -> Option<String> {
        None
    }

    fn album_picture_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        if !self.is_enabled() {
            return Err("酷狗来源未启用".to_string());
        }
        // entity_id 可能是 http(s):// URL，直接下载
        if entity_id.starts_with("http://") || entity_id.starts_with("https://") {
            return self.client.get_bytes(entity_id);
        }

        // album:<id> 或 song:<hash> 都通过 /album 接口取封面
        // song:<hash> 时需要先从搜索结果找到 AlbumID，这里简化为仅支持 album:<id>
        let album_id = if entity_id.starts_with("album:") {
            parse_remote_id(entity_id).unwrap_or_default()
        } else {
            // song:<hash> 无法直接获取专辑封面（需先查 AlbumID）
            return Err(format!(
                "酷狗 album_picture_get 暂仅支持 album:<id>，收到: {}",
                entity_id
            ));
        };

        if album_id.is_empty() || album_id == "0" {
            return Err(format!("无效的专辑 ID: {}", entity_id));
        }

        let info = api::get_album_info(&self.client, &album_id)?;
        let cover_url = if !info.sizable_cover.is_empty() {
            info.sizable_cover
        } else if !info.cover.is_empty() {
            info.cover
        } else {
            return Err("专辑无封面 URL".to_string());
        };

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

impl KugouMusicApiSource {
    /// 搜索酷狗歌手，返回库内 Artist 模型。
    pub fn search_artists(
        &self,
        query: &str,
        page: u32,
        pagesize: u32,
    ) -> Result<Vec<Artist>, String> {
        if !self.is_enabled() {
            return Err("酷狗来源未启用".to_string());
        }
        let briefs = api::search_artists(&self.client, query, page, pagesize)?;
        Ok(briefs
            .into_iter()
            .map(|b| Artist {
                id: Uuid::new_v4().to_string(),
                name: b.singer_name().to_string(),
                bio: None,
                source_ids: vec![SourceId {
                    source_name: self.name.clone(),
                    source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
                    entity_type: EntityType::Artist,
                    entity_id: build_entity_id(EntityKind::artist, &b.singer_id().to_string()),
                }],
            })
            .collect())
    }

    /// 搜索酷狗专辑，返回库内 Album 模型。
    pub fn search_albums(
        &self,
        query: &str,
        page: u32,
        pagesize: u32,
    ) -> Result<Vec<Album>, String> {
        if !self.is_enabled() {
            return Err("酷狗来源未启用".to_string());
        }
        let briefs = api::search_albums(&self.client, query, page, pagesize)?;
        Ok(briefs
            .into_iter()
            .map(|b| Album {
                id: Uuid::new_v4().to_string(),
                title: b.album_name().to_string(),
                artist_id: String::new(),
                cover_url: {
                    let img = b.image_url();
                    if img.is_empty() {
                        None
                    } else {
                        Some(img.to_string())
                    }
                },
                song_ids: Vec::new(),
                source_ids: vec![SourceId {
                    source_name: self.name.clone(),
                    source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
                    entity_type: EntityType::Album,
                    entity_id: build_entity_id(EntityKind::album, b.album_id()),
                }],
                year: None,
            })
            .collect())
    }

    /// 从 [`KugouSongBrief`] 构建 [`Song`]（公开版本，供 Tauri 命令使用）。
    pub fn build_song_from_brief(&self, brief: &KugouSongBrief) -> Song {
        self.build_song(brief)
    }
}
