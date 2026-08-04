//! [`KugouMusicApiSource`] — 酷狗音乐来源的 [`MusicSource`] 实现。

use super::api;
use super::entity_id::{build_entity_id, build_lyric_entity_id, parse_remote_id, EntityKind};
use super::quality::default_quality;
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
fn parse_year_from_date(s: &str) -> Option<u32> {
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
    name: String,
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
    fn build_song(&self, brief: &KugouSongBrief) -> Song {
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
        // id 是 song:<hash>
        let hash = parse_remote_id(id).unwrap_or_default();
        if hash.is_empty() {
            return Ok(None);
        }
        let song_id = format!("kugou_song_{}", hash);
        let source_id = SourceId {
            source_name: self.name.clone(),
            source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
            entity_type: EntityType::Song,
            entity_id: id.to_string(),
        };

        // 优先通过 /search/lyric 获取歌曲元信息（歌曲名 + 歌手 + 时长）
        // 这个接口仅需 hash，返回的 KugouLyricCandidate 含 song/singer/duration 字段
        let candidates = api::search_lyric(&self.client, &hash, 0).unwrap_or_default();
        if let Some(candidate) = candidates.into_iter().next() {
            let artist_names: Vec<String> = if candidate.singer.is_empty() {
                vec!["未知艺术家".to_string()]
            } else {
                candidate
                    .singer
                    .split('/')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            };
            // candidate.duration 是毫秒，转换为秒
            let duration = if candidate.duration > 0 {
                Some((candidate.duration / 1000) as u64)
            } else {
                None
            };
            return Ok(Some(Song {
                id: song_id,
                title: if candidate.song.is_empty() {
                    hash.clone()
                } else {
                    candidate.song
                },
                artist_names,
                album_title: None,
                duration,
                artist_ids: Vec::new(), // search/lyric 不返回 SingerId
                album_id: None,
                lyric_id: None,
                source_ids: vec![source_id],
                year: None,
                cover_url: None,
            }));
        }

        // 退化为 /song/url 验证 hash 有效性并从 URL 数据提取 name/duration
        match api::get_song_url(&self.client, &hash, default_quality(), None) {
            Ok(url_data) => {
                let Some(url_entry) = url_data.url.first() else {
                    return Ok(None);
                };
                let duration = if url_entry.duration > 0 {
                    Some(url_entry.duration as u64)
                } else {
                    None
                };
                Ok(Some(Song {
                    id: song_id,
                    title: url_entry.name.clone(),
                    artist_names: vec!["未知艺术家".to_string()],
                    album_title: None,
                    duration,
                    artist_ids: Vec::new(),
                    album_id: None,
                    lyric_id: None,
                    source_ids: vec![source_id],
                    year: None,
                    cover_url: None,
                }))
            }
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

        // 调用 /artist/detail 获取歌手详情（含简介 brief/intro、头像 img、粉丝数等）
        let detail = api::get_artist_detail(&self.client, singer_id)?;
        let name = detail.name();
        let bio = {
            let intro = detail.intro();
            if intro.is_empty() {
                None
            } else {
                Some(intro.to_string())
            }
        };

        Ok(Some(Artist {
            id: format!("kugou_artist_{}", singer_id),
            name: if name.is_empty() {
                format!("singer_{}", singer_id)
            } else {
                name.to_string()
            },
            bio,
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

        // 1. 获取专辑基础信息（含封面 URL、author_name、publish_date）
        let info = api::get_album_info(&self.client, &album_id)?;

        // 2. 尝试获取专辑详情（含 singer_id 数值，便于填充 Album.artist_id）
        //    /album/detail 是较新的接口，部分自建 ncmapi 风格服务端可能未实现，
        //    失败时降级为仅使用 /album 信息（artist_id 留空）
        let (artist_id, year_from_detail) = match api::get_album_detail(&self.client, &album_id) {
            Ok(detail) => {
                let artist_id = if detail.singer_id > 0 {
                    format!("kugou_artist_{}", detail.singer_id)
                } else {
                    String::new()
                };
                let year = parse_year_from_date(&detail.publish_date);
                (artist_id, year)
            }
            Err(e) => {
                eprintln!(
                    "[kugou] get_album: /album/detail 失败 album_id={}，降级使用 /album 信息: {}",
                    album_id, e
                );
                (String::new(), None)
            }
        };

        // 3. 获取专辑歌曲列表（用于填充 song_ids）
        //    默认拉取前 1000 首（pagesize=500, 最多 2 页）
        let mut song_ids: Vec<String> = Vec::new();
        let pagesize: u32 = 500;
        let max_pages: u32 = 4; // 最多 2000 首
        let mut page = 1u32;
        let mut total: i64 = -1;
        loop {
            match api::get_album_songs(&self.client, &album_id, page, pagesize) {
                Ok((list, t)) => {
                    if total < 0 {
                        total = t;
                    }
                    for song in &list {
                        let hash = song.file_hash_lower();
                        if !hash.is_empty() {
                            song_ids.push(format!("kugou_song_{}", hash));
                        }
                    }
                    let len = list.len() as u32;
                    if len < pagesize
                        || (total > 0 && song_ids.len() as i64 >= total)
                        || page >= max_pages
                    {
                        break;
                    }
                    page += 1;
                }
                Err(e) => {
                    eprintln!(
                        "[kugou] get_album: /album/songs 失败 album_id={} page={}：{}",
                        album_id, page, e
                    );
                    // 失败不致命，继续返回专辑基本信息（song_ids 可能部分为空）
                    break;
                }
            }
        }

        // 4. 年份：优先 /album/detail 的 publish_date，回退到 /album 的 publish_date
        let year = year_from_detail.or_else(|| parse_year_from_date(&info.publish_date));

        // 5. 封面 URL：优先 sizable_cover（可调整尺寸），回退到 cover
        let cover_url = if !info.sizable_cover.is_empty() {
            Some(info.sizable_cover)
        } else if !info.cover.is_empty() {
            Some(info.cover)
        } else {
            None
        };

        Ok(Some(Album {
            id: format!("kugou_album_{}", album_id),
            title: info.album_name,
            artist_id,
            cover_url,
            song_ids,
            source_ids: vec![SourceId {
                source_name: self.name.clone(),
                source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
                entity_type: EntityType::Album,
                entity_id: build_entity_id(EntityKind::album, &album_id),
            }],
            year,
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
        // duration=0 表示不指定时长过滤；若 candidate.duration > 0，下次可优化为传入真实时长
        let candidates = api::search_lyric(&self.client, &hash, 0)?;
        let candidate = candidates
            .into_iter()
            .next()
            .ok_or_else(|| "未找到歌词候选".to_string())?;

        // 第二步：下载歌词
        let lyric_resp = api::get_lyric(&self.client, &candidate.id, &candidate.accesskey)?;

        // 优先使用已解码的 decodeContent；若为空则尝试 base64 解码 content 字段
        let lyric_text = if !lyric_resp.decodeContent.is_empty() {
            lyric_resp.decodeContent
        } else if !lyric_resp.content.is_empty() {
            // 兜底：base64 解码 content 字段
            use base64::Engine;
            match base64::engine::general_purpose::STANDARD.decode(&lyric_resp.content) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!(
                            "[kugou] 歌词 base64 解码后 UTF-8 转换失败 hash={} id={}: {}",
                            hash, candidate.id, e
                        );
                        return Ok(None);
                    }
                },
                Err(e) => {
                    eprintln!(
                        "[kugou] 歌词 base64 解码失败 hash={} id={}: {}",
                        hash, candidate.id, e
                    );
                    return Ok(None);
                }
            }
        } else {
            return Ok(None);
        };

        if lyric_text.is_empty() {
            return Ok(None);
        }

        // 歌词库内 ID：基于 candidate id 确定性，便于去重
        let entity_id = build_lyric_entity_id(&candidate.id, &candidate.accesskey);
        Ok(Some(Lyric {
            id: format!("kugou_lyric_{}", candidate.id),
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
        // 默认 320 kbps MP3（Exhigh）
        // trait 方法签名不接受音质参数，因此使用默认值；
        // 前端需要指定音质时通过专门的 Tauri 命令（source_download_song）调用
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

        // 支持 album:<id> 和 song:<hash>
        // song:<hash> 时先通过 /search/lyric 获取歌曲信息，但该接口不返回 AlbumID，
        // 因此 song:<hash> 路径会失败，建议前端直接用 song.cover_url 或调 album:<id>
        let album_id = if entity_id.starts_with("album:") {
            parse_remote_id(entity_id).unwrap_or_default()
        } else {
            // song:<hash> 无法直接获取专辑封面（需先查 AlbumID，搜索接口未返回）
            return Err(format!(
                "酷狗 album_picture_get 仅支持 album:<id> 或 http(s):// URL，收到: {}；\
                 建议前端使用歌曲详情中已缓存的 cover_url",
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
            .map(|b| {
                let singer_id = b.singer_id();
                Artist {
                    id: if singer_id > 0 {
                        format!("kugou_artist_{}", singer_id)
                    } else {
                        String::new()
                    },
                    name: b.singer_name().to_string(),
                    bio: None,
                    source_ids: vec![SourceId {
                        source_name: self.name.clone(),
                        source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
                        entity_type: EntityType::Artist,
                        entity_id: build_entity_id(
                            EntityKind::artist,
                            &singer_id.to_string(),
                        ),
                    }],
                }
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
            .map(|b| {
                let album_id_str = b.album_id().to_string();
                let singer_id = b.SingerId;
                Album {
                    id: if !album_id_str.is_empty() && album_id_str != "0" {
                        format!("kugou_album_{}", album_id_str)
                    } else {
                        String::new()
                    },
                    title: b.album_name().to_string(),
                    artist_id: if singer_id > 0 {
                        format!("kugou_artist_{}", singer_id)
                    } else {
                        String::new()
                    },
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
                        entity_id: build_entity_id(EntityKind::album, &album_id_str),
                    }],
                    year: None,
                }
            })
            .collect())
    }

    /// 从 [`KugouSongBrief`] 构建 [`Song`]（公开版本，供 Tauri 命令使用）。
    pub fn build_song_from_brief(&self, brief: &KugouSongBrief) -> Song {
        self.build_song(brief)
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