//! [`NeteaseCloudMusicApiSource`] — 网易云音乐来源的 [`MusicSource`] 实现。

use super::api;
use super::cookie::{needs_cookie_normalization, parse_set_cookie_to_header};
use super::entity_id::{build_entity_id, build_lyric_entity_id, parse_remote_id, EntityKind};
use super::time::publish_time_to_year;
use super::types::AudioQuality;
use crate::module::music_library::models::{Album, Artist, Lyric, Song};
use crate::module::music_source::traits::MusicSource;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};
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
            id: format!("netease_artist_{}", remote_id),
            name: b.name,
            bio: b.briefDesc,
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
        let (album, songs) = api::get_album_detail(&self.client, remote_id)?;
        let artist_id = album
            .artist
            .as_ref()
            .map(|a| format!("netease_artist_{}", a.id))
            .unwrap_or_default();
        let song_ids: Vec<String> = songs
            .iter()
            .map(|s| format!("netease_song_{}", s.id))
            .collect();
        let year = publish_time_to_year(album.publishTime);
        Ok(Some(Album {
            id: format!("netease_album_{}", remote_id),
            title: album.name,
            artist_id,
            cover_url: album.picUrl,
            song_ids,
            source_ids: vec![SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
                entity_type: EntityType::Album,
                entity_id: build_entity_id(EntityKind::album, remote_id),
            }],
            year,
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
            id: format!("netease_lyric_{}_{}", remote_id, format),
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
        // 使用音质降级下载：从 exhigh 开始向下降级，确保尽可能返回可播放字节
        // 相比固定 exhigh 的 download_song，能避免 VIP/无版权等级导致的播放失败
        let result = api::download_song_with_fallback(
            &self.client,
            remote_id,
            Some(AudioQuality::Exhigh.to_level_str()),
        )?;
        Ok(result.bytes)
    }

    fn song_file_path(&self, _entity_id: &str) -> Option<String> {
        // 网络来源无本地路径；下载后才会落盘到 download_dir
        None
    }

    fn album_picture_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }

        // 1. 先查字节缓存：命中则直接返回
        //    持锁期间只 clone Arc（廉价），锁外再 clone Vec<u8>，避免持锁深拷贝
        {
            let cache = self.cover_cache.lock();
            if let Some(hit) = cache.get(entity_id) {
                let arc = Arc::clone(hit);
                drop(cache); // 显式释放锁
                return Ok((*arc).clone()); // 锁外深拷贝
            }
        }

        // 2. entity_id 可能是 album:<id> / song:<id>，也可能直接是图片 URL
        //    若是 URL，直接下载并以 URL 本身作为缓存 key
        if entity_id.starts_with("http://") || entity_id.starts_with("https://") {
            let bytes = self.client.get_bytes(entity_id)?;
            self.insert_cover_cache(entity_id, bytes.clone());
            return Ok(bytes);
        }

        let Some(remote_id) = parse_remote_id(entity_id) else {
            return Err(format!("无效的 entity_id: {}", entity_id));
        };

        // 3. 查 picUrl 缓存：命中则跳过 /song/detail 或 /album 接口
        let pic_url_cached: Option<String> = {
            let cache = self.pic_url_cache.lock();
            cache.get(entity_id).cloned()
        };

        // 4. picUrl 未命中 → 走原有 /song/detail 或 /album 逻辑拿 picUrl
        let pic_url = match pic_url_cached {
            Some(cached) => cached,
            None => {
                // 根据 entity_id 前缀选择 API：
                // - album:<id> → /album?id=<id> 拿 album.picUrl
                // - song:<id>  → /song/detail?ids=<id> 拿 songs[0].al.picUrl
                //   （旧实现统一调 /album，把 song id 当 album id 导致 404）
                let fetched = if entity_id.starts_with("song:") {
                    #[derive(serde::Deserialize)]
                    struct SongDetailResp {
                        #[serde(default)]
                        code: i32,
                        #[serde(default)]
                        songs: Vec<super::types::NeteaseSongDetail>,
                    }
                    let resp: SongDetailResp = self.client.get_json(
                        "/song/detail",
                        &[("ids", &remote_id.to_string())],
                    )?;
                    if resp.code != 200 {
                        return Err(format!("获取歌曲详情失败 code={}", resp.code));
                    }
                    resp.songs
                        .into_iter()
                        .next()
                        .and_then(|s| s.al.and_then(|a| a.picUrl))
                        .ok_or_else(|| "歌曲无封面 URL".to_string())
                } else {
                    // album:<id> 或裸 id → /album 接口
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
                    resp.album
                        .and_then(|a| a.picUrl)
                        .ok_or_else(|| "专辑无封面 URL".to_string())
                };
                // picUrl 获取成功才写缓存，避免空字符串污染缓存
                let fetched = fetched?;
                if !fetched.is_empty() {
                    self.insert_pic_url_cache(entity_id, fetched.clone());
                }
                fetched
            }
        };

        // 5. 用 picUrl 调 get_bytes，写入 cover_cache，返回字节
        let bytes = self.client.get_bytes(&pic_url)?;
        self.insert_cover_cache(entity_id, bytes.clone());
        Ok(bytes)
    }

    fn lyric_text_get(&self, song_id: &str) -> Result<String, String> {
        let lyric = self
            .get_lyric(song_id)?
            .ok_or_else(|| "未找到歌词".to_string())?;
        Ok(lyric.text)
    }
}

// ── 搜索映射（非 trait 方法，由专门的 Tauri 命令调用）────────────────────

impl NeteaseCloudMusicApiSource {
    /// 搜索网易云歌手，返回库内 Artist 模型（附带 netease SourceId）。
    pub fn search_artists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Artist>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        let briefs = api::search_artists(&self.client, query, limit, offset)?;
        let artists = briefs
            .into_iter()
            .map(|b| Artist {
                id: format!("netease_artist_{}", b.id),
                name: b.name,
                bio: b.briefDesc,
                source_ids: vec![SourceId {
                    source_name: self.name.clone(),
                    source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
                    entity_type: EntityType::Artist,
                    entity_id: build_entity_id(EntityKind::artist, b.id),
                }],
            })
            .collect();
        Ok(artists)
    }

    /// 搜索网易云专辑，返回库内 Album 模型（附带 netease SourceId）。
    pub fn search_albums(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Album>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        let briefs = api::search_albums(&self.client, query, limit, offset)?;
        let albums = briefs
            .into_iter()
            .map(|b| Album {
                id: format!("netease_album_{}", b.id),
                title: b.name,
                artist_id: b
                    .artist
                    .as_ref()
                    .map(|a| format!("netease_artist_{}", a.id))
                    .unwrap_or_default(),
                cover_url: b.picUrl,
                song_ids: Vec::new(),
                source_ids: vec![SourceId {
                    source_name: self.name.clone(),
                    source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
                    entity_type: EntityType::Album,
                    entity_id: build_entity_id(EntityKind::album, b.id),
                }],
                year: publish_time_to_year(b.publishTime),
            })
            .collect();
        Ok(albums)
    }
}