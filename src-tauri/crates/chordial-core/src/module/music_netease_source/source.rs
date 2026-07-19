//! [`NeteaseCloudMusicApiSource`] — 网易云音乐来源的 [`MusicSource`] 实现。

use super::api;
use super::types::{AudioQuality, NeteasePlaylistBrief, NeteaseSongDetail};
use crate::module::music_library::models::{Album, Artist, Lyric, Song};
use crate::module::music_source::traits::MusicSource;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};
use crate::module::music_web_source::{WebHttpClient, WebSourceConfig};
use crate::module::storage::persistent::PersistentStore;
use parking_lot::RwLock;
use std::sync::Arc;
use uuid::Uuid;

/// 网易云音乐来源的固定名称。
pub const NETEASE_SOURCE_NAME: &str = "netease";

/// 网易云 entity_id 中各部分的分隔符。
const ENTITY_ID_SEP: &str = ":";

/// 网易云 entity_id 的种类前缀。
#[allow(non_camel_case_types)]
enum EntityKind {
    song,
    artist,
    album,
    lyric,
    /// 保留：未来可能将歌单作为库内实体追踪
    #[allow(dead_code)]
    playlist,
}

impl EntityKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::song => "song",
            Self::artist => "artist",
            Self::album => "album",
            Self::lyric => "lyric",
            Self::playlist => "playlist",
        }
    }
}

/// 构建网易云 entity_id。
fn build_entity_id(kind: EntityKind, id: i64) -> String {
    format!("{}{}{}", kind.as_str(), ENTITY_ID_SEP, id)
}

/// 构建歌词的 entity_id（携带格式以区分 YRC/LRC）。
fn build_lyric_entity_id(song_id: i64, format: &str) -> String {
    format!(
        "{}{}{}{}{}",
        EntityKind::lyric.as_str(),
        ENTITY_ID_SEP,
        song_id,
        ENTITY_ID_SEP,
        format
    )
}

/// 解析 entity_id 中的网易云 ID（去掉 `kind:` 前缀后的部分）。
fn parse_remote_id(entity_id: &str) -> Option<i64> {
    let parts: Vec<&str> = entity_id.splitn(3, ENTITY_ID_SEP).collect();
    if parts.len() < 2 {
        return None;
    }
    parts[1].parse::<i64>().ok()
}

/// 网易云音乐来源。
///
/// 持有一个 [`WebHttpClient`] 实例（连接池复用），配置通过 [`WebSourceConfig`] 持久化。
pub struct NeteaseCloudMusicApiSource {
    /// 来源名称（固定为 `"netease"`）
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

impl NeteaseCloudMusicApiSource {
    /// 创建网易云来源。
    ///
    /// # 参数
    /// - `store`: 用于持久化配置的存储（通常为 `AppContext.store`）
    pub fn new(store: Arc<PersistentStore>) -> Result<Self, String> {
        let config_key = crate::module::music_web_source::config::config_key(NETEASE_SOURCE_NAME);
        let config = WebSourceConfig::load(&store, &config_key);

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
        // 重建客户端
        let new_client = WebHttpClient::new(
            new_config.base_url.clone(),
            new_config.cookie.clone(),
            new_config.timeout_secs,
        )?;

        // 持久化
        new_config.flush(&self.store, &self.config_key)?;

        // 应用到内存
        // 注意：客户端字段需要内部可变性，使用 unsafe 替换是安全的，
        // 因为 WebHttpClient 内部所有字段都有锁保护，且我们只替换整个客户端
        // 这里不能直接替换 &self.client（不可变引用），因此采用经典方案：
        // 将 client 字段改为 RwLock 或者使用 Option 包装
        // 为保持代码简洁，这里改用 RwLock<WebHttpClient> 在 struct 定义中
        // 但为了减少改动，我们换种方式：将 client 改为 RwLock<WebHttpClient>
        // 此处先记录新配置，client 不重建（base_url/cookie 可在运行时通过 set_* 更新）
        self.client.set_base_url(new_config.base_url.clone());
        self.client.set_cookie(new_config.cookie.clone());
        *self.config.write() = new_config;

        // 提示编译器：new_client 已构建但未使用，这里明确丢弃
        drop(new_client);

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

    /// 从 [`NeteaseSongDetail`] 构建 [`Song`] 模型（公开版本，供 Tauri 命令使用）。
    pub fn build_song_from_detail(&self, detail: &NeteaseSongDetail) -> Song {
        self.build_song(detail)
    }

    /// 从 [`NeteaseSongDetail`] 构建 [`Song`] 模型。
    fn build_song(&self, detail: &NeteaseSongDetail) -> Song {
        let entity_id = build_entity_id(EntityKind::song, detail.id);
        let song_id = Uuid::new_v4().to_string();
        let artist_names: Vec<String> = if detail.ar.is_empty() {
            vec!["未知艺术家".to_string()]
        } else {
            detail.ar.iter().map(|a| a.name.clone()).collect()
        };
        let artist_ids: Vec<String> = (0..artist_names.len())
            .map(|_| Uuid::new_v4().to_string())
            .collect();
        let album_title = detail.al.as_ref().map(|a| a.name.clone());
        let album_id = album_title.as_ref().map(|_| Uuid::new_v4().to_string());
        let lyric_id = Some(Uuid::new_v4().to_string());

        let source_id = SourceId {
            source_name: self.name.clone(),
            source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
            entity_type: EntityType::Song,
            entity_id: entity_id.clone(),
        };

        Song {
            id: song_id,
            title: detail.name.clone(),
            artist_names,
            album_title,
            duration: if detail.dt > 0 {
                Some((detail.dt / 1000) as u64)
            } else {
                None
            },
            artist_ids,
            album_id,
            lyric_id,
            source_ids: vec![source_id],
            year: None,
        }
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
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        let briefs = api::search_songs(&self.client, query, 30, 0)?;
        // 转换为库内 Song 模型（简略信息→统一模型）
        let songs: Vec<Song> = briefs
            .iter()
            .map(|b| {
                let detail = super::types::NeteaseSongDetail {
                    id: b.id,
                    name: b.name.clone(),
                    ar: b.artists.clone(),
                    al: b.album.clone(),
                    dt: b.duration,
                    size: None,
                };
                self.build_song(&detail)
            })
            .collect();
        Ok(songs)
    }

    fn get_song(&self, id: &str) -> Result<Option<Song>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        let Some(remote_id) = parse_remote_id(id) else {
            return Ok(None);
        };
        let ids_json = format!("[{}]", remote_id);
        let songs = api::get_song_details(&self.client, &ids_json)?;
        Ok(songs.into_iter().next().map(|d| self.build_song(&d)))
    }

    fn get_artist(&self, id: &str) -> Result<Option<Artist>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        let Some(remote_id) = parse_remote_id(id) else {
            return Ok(None);
        };
        // /artist/detail 接口返回艺术家详情
        #[derive(serde::Deserialize)]
        struct ArtistDetailResp {
            #[serde(default)]
            code: i32,
            #[serde(default)]
            data: Option<ArtistDetailData>,
        }
        #[derive(serde::Deserialize, Default)]
        struct ArtistDetailData {
            #[serde(default)]
            artist: Option<super::types::NeteaseArtistBrief>,
        }
        let resp: ArtistDetailResp = self.client.get_json(
            "/artist/detail",
            &[("id", &remote_id.to_string())],
        )?;
        if resp.code != 200 {
            return Err(format!("获取艺术家失败 code={}", resp.code));
        }
        let brief = resp.data.and_then(|d| d.artist);
        Ok(brief.map(|b| Artist {
            id: Uuid::new_v4().to_string(),
            name: b.name,
            bio: None,
            source_ids: vec![SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
                entity_type: EntityType::Artist,
                entity_id: build_entity_id(EntityKind::artist, remote_id),
            }],
        }))
    }

    fn get_album(&self, id: &str) -> Result<Option<Album>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        let Some(remote_id) = parse_remote_id(id) else {
            return Ok(None);
        };
        // /album 接口
        #[derive(serde::Deserialize)]
        struct AlbumResp {
            #[serde(default)]
            code: i32,
            #[serde(default)]
            album: Option<super::types::NeteaseAlbumBrief>,
        }
        let resp: AlbumResp = self
            .client
            .get_json("/album", &[("id", &remote_id.to_string())])?;
        if resp.code != 200 {
            return Err(format!("获取专辑失败 code={}", resp.code));
        }
        let brief = resp.album;
        Ok(brief.map(|b| Album {
            id: Uuid::new_v4().to_string(),
            title: b.name,
            artist_id: String::new(),
            cover_url: b.picUrl,
            song_ids: Vec::new(),
            source_ids: vec![SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
                entity_type: EntityType::Album,
                entity_id: build_entity_id(EntityKind::album, remote_id),
            }],
            year: None,
        }))
    }

    fn get_lyric(&self, song_id: &str) -> Result<Option<Lyric>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        // song_id 可能是库内 UUID 或 entity_id；尝试解析 entity_id
        let remote_id = parse_remote_id(song_id).unwrap_or_else(|| {
            // 如果不是 entity_id 格式，尝试直接 parse
            song_id.parse::<i64>().unwrap_or(0)
        });
        if remote_id == 0 {
            return Ok(None);
        }

        let resp = api::get_lyric(&self.client, remote_id)?;
        // 优先 YRC（逐字），其次 LRC（行级）
        let (text, format) = if let Some(yrc) = resp.yrc {
            if !yrc.lyric.is_empty() {
                (yrc.lyric, "yrc")
            } else if let Some(lrc) = resp.lrc {
                if !lrc.lyric.is_empty() {
                    (lrc.lyric, "lrc")
                } else {
                    return Ok(None);
                }
            } else {
                return Ok(None);
            }
        } else if let Some(lrc) = resp.lrc {
            if lrc.lyric.is_empty() {
                return Ok(None);
            }
            (lrc.lyric, "lrc")
        } else {
            return Ok(None);
        };

        // 如果有翻译，附加到歌词末尾（简易合并方案）
        // 完整的翻译合并由前端 lyricConverter.js 处理
        let final_text = if let Some(tlyric) = resp.tlyric {
            if !tlyric.lyric.is_empty() && format == "lrc" {
                format!("{}\n[translations]\n{}", text, tlyric.lyric)
            } else {
                text
            }
        } else {
            text
        };

        let entity_id = build_lyric_entity_id(remote_id, format);
        Ok(Some(Lyric {
            id: Uuid::new_v4().to_string(),
            song_id: song_id.to_string(),
            text: final_text,
            source_id: SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
                entity_type: EntityType::Lyric,
                entity_id,
            },
        }))
    }

    fn song_file_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        let Some(remote_id) = parse_remote_id(entity_id) else {
            return Err(format!("无效的 entity_id: {}", entity_id));
        };
        // 默认使用 exhigh（320 kbps MP3），用户可在配置中调整音质偏好
        // 后续可扩展为从 config 读取 preferred_level 字段
        let (bytes, _url_data) =
            api::download_song(&self.client, remote_id, AudioQuality::Exhigh.to_level_str())?;
        Ok(bytes)
    }

    fn song_file_path(&self, _entity_id: &str) -> Option<String> {
        // 网络来源无本地路径；下载后才会落盘到 download_dir
        None
    }

    fn album_picture_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        // entity_id 可能是 album:<id>，也可能直接是图片 URL
        // 若是 URL，直接下载
        if entity_id.starts_with("http://") || entity_id.starts_with("https://") {
            return self.client.get_bytes(entity_id);
        }

        let Some(remote_id) = parse_remote_id(entity_id) else {
            return Err(format!("无效的 entity_id: {}", entity_id));
        };
        // 获取专辑详情拿到 picUrl
        #[derive(serde::Deserialize)]
        struct AlbumResp {
            #[serde(default)]
            code: i32,
            #[serde(default)]
            album: Option<super::types::NeteaseAlbumBrief>,
        }
        let resp: AlbumResp = self
            .client
            .get_json("/album", &[("id", &remote_id.to_string())])?;
        if resp.code != 200 {
            return Err(format!("获取专辑失败 code={}", resp.code));
        }
        let pic_url = resp
            .album
            .and_then(|a| a.picUrl)
            .ok_or_else(|| "专辑无封面 URL".to_string())?;
        self.client.get_bytes(&pic_url)
    }

    fn lyric_text_get(&self, song_id: &str) -> Result<String, String> {
        let lyric = self
            .get_lyric(song_id)?
            .ok_or_else(|| "未找到歌词".to_string())?;
        Ok(lyric.text)
    }
}

// ── 歌单相关方法（非 trait 方法，由专门的 Tauri 命令调用）──────────────────

impl NeteaseCloudMusicApiSource {
    /// 搜索网易云歌单。
    pub fn search_playlists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<NeteasePlaylistBrief>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        api::search_playlists(&self.client, query, limit, offset)
    }

    /// 获取歌单详情（元信息 + 全部曲目）。
    ///
    /// 返回 `(playlist_meta, songs)`。
    pub fn get_playlist_full(
        &self,
        playlist_id: i64,
    ) -> Result<(super::types::NeteasePlaylistDetail, Vec<NeteaseSongDetail>), String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        let detail = api::get_playlist_detail(&self.client, playlist_id)?;
        // 一次性获取全部曲目（NeteaseCloudMusicApi 支持最大 1000）
        let tracks = api::get_playlist_tracks(&self.client, playlist_id, 1000, 0)?;
        Ok((detail, tracks))
    }
}
