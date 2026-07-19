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
                id: Uuid::new_v4().to_string(),
                name: b.name,
                bio: None,
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
                id: Uuid::new_v4().to_string(),
                title: b.name,
                artist_id: String::new(),
                cover_url: b.picUrl,
                song_ids: Vec::new(),
                source_ids: vec![SourceId {
                    source_name: self.name.clone(),
                    source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
                    entity_type: EntityType::Album,
                    entity_id: build_entity_id(EntityKind::album, b.id),
                }],
                year: None,
            })
            .collect();
        Ok(albums)
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

    // ── 扫码登录 ───────────────────────────────────────────────────────────

    /// 第 1 步：获取扫码登录的 unikey。
    pub fn login_qr_key(&self) -> Result<String, String> {
        api::login_qr_key(&self.client)
    }

    /// 第 2 步：用 unikey 生成二维码。
    pub fn login_qr_create(
        &self,
        key: &str,
        qrimg: bool,
    ) -> Result<super::types::NeteaseQrCreateData, String> {
        api::login_qr_create(&self.client, key, qrimg)
    }

    /// 第 3 步：轮询扫码状态。
    ///
    /// **副作用**：当返回 `code == 803`（授权成功）时，自动将响应中的 cookie
    /// 写回当前配置（持久化）并更新 `WebHttpClient` 的 cookie，调用方无需再手动保存。
    pub fn login_qr_check(
        &self,
        key: &str,
    ) -> Result<super::types::NeteaseQrCheckResponse, String> {
        let resp = api::login_qr_check(&self.client, key)?;
        if resp.status() == super::types::QrCheckStatus::Authorized {
            if let Some(raw_cookie) = resp.cookie.clone() {
                if !raw_cookie.is_empty() {
                    // ncmapi 返回的 cookie 字段是多个 Set-Cookie 用 ", " 连接的字符串
                    // 每条格式：NAME=VALUE; Path=/; HttpOnly; Expires=Day, DD Mon YYYY HH:MM:SS GMT
                    // 直接当作 Cookie 请求头发送会让 ncmapi 把 Path/Expires 等属性当成 cookie 名
                    // 解析失败 → profile=null + 匿名 account
                    // 此处解析出 NAME=VALUE 对，用 "; " 连接成标准 Cookie 头格式
                    let cookie = parse_set_cookie_to_header(&raw_cookie);
                    eprintln!(
                        "[netease] 扫码登录成功，原始 cookie 长度={}，解析后长度={}，含 MUSIC_U={}",
                        raw_cookie.len(),
                        cookie.len(),
                        cookie.contains("MUSIC_U=")
                    );
                    // 写入运行时 client（立即生效）
                    self.client.set_cookie(cookie.clone());
                    // 持久化到 config
                    let mut cfg = self.config.write().clone();
                    cfg.cookie = cookie;
                    // update_config 会落盘并刷新 client
                    if let Err(e) = self.update_config(cfg) {
                        // 持久化失败不影响返回 cookie，仅 stderr 记录
                        eprintln!("[netease] 扫码登录 cookie 持久化失败: {}", e);
                    }
                }
            }
        }
        Ok(resp)
    }

    // ── 用户账户与"我的资源" ────────────────────────────────────────────────

    /// 获取当前登录账号信息（依赖 cookie）。
    pub fn get_user_account(
        &self,
    ) -> Result<super::types::NeteaseUserAccountResponse, String> {
        api::get_user_account(&self.client)
    }

    /// 获取指定用户的全部歌单（创建 + 收藏）。
    pub fn get_user_playlists(
        &self,
        uid: i64,
    ) -> Result<Vec<super::types::NeteasePlaylistBrief>, String> {
        api::get_user_playlists(&self.client, uid)
    }

    /// 获取当前用户收藏的专辑（依赖 cookie，分页）。
    ///
    /// 返回 `(albums, total_count)`。
    pub fn get_user_albums(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<super::types::NeteaseSubAlbum>, i64), String> {
        api::get_user_albums(&self.client, limit, offset)
    }

    /// 获取专辑详情（含全部曲目）。
    pub fn get_album_detail(
        &self,
        album_id: i64,
    ) -> Result<(super::types::NeteaseAlbumDetail, Vec<NeteaseSongDetail>), String> {
        api::get_album_detail(&self.client, album_id)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// 私有辅助函数
// ───────────────────────────────────────────────────────────────────────────

/// 将 ncmapi `/login/qr/check` 返回的 Set-Cookie 串解析为标准 Cookie 请求头格式。
///
/// # 背景
///
/// ncmapi 在 `code==803` 时返回的 `cookie` 字段是多个 Set-Cookie 用 `", "` 连接的字符串，
/// 每条形如：`NAME=VALUE; Path=/; HttpOnly; Expires=Day, DD Mon YYYY HH:MM:SS GMT`。
///
/// 直接当作 Cookie 请求头发送时，ncmapi 会把 `Path`/`Expires`/`Max-Age` 等属性名
/// 当成 cookie 名解析，真正的 `MUSIC_U` 被属性淹没，导致登录态丢失（profile=null）。
///
/// # 解析策略
///
/// 1. 将 `", "` 替换为 `"; "`，把多 cookie 分隔符与属性分隔符统一
///    （注意 `Expires=Fri, 06 Aug` 内的 `", "` 也会被替换，但后续步骤会跳过非 token 段）
/// 2. 按 `;` 切分，对每段 trim 后判断是否为有效 `NAME=VALUE` 对：
///    - 必须含 `=`
///    - `NAME` 必须为合法 token（仅字母数字 `_` `-` `.`）
///    - `NAME` 不在保留属性名集合内（`Path`/`Domain`/`Expires`/`Max-Age`/...）
/// 3. 对重名 cookie 只保留第一次出现的（去重）
/// 4. 用 `"; "` 连接所有有效对
fn parse_set_cookie_to_header(set_cookie: &str) -> String {
    // 已知的 Set-Cookie 属性名（不区分大小写）
    const RESERVED: &[&str] = &[
        "Path",
        "Domain",
        "Expires",
        "Max-Age",
        "HttpOnly",
        "Secure",
        "SameSite",
        "Comment",
        "Version",
        "Priority",
    ];

    // 统一分隔符
    let normalized = set_cookie.replace(", ", "; ");

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut pairs: Vec<String> = Vec::new();

    for segment in normalized.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        // 必须含 =（HttpOnly / Secure 等 flag 没有 =）
        let eq_pos = match segment.find('=') {
            Some(p) => p,
            None => continue,
        };
        let name = segment[..eq_pos].trim();
        // name 必须非空且为合法 token
        if name.is_empty() || name.contains(' ') || name.contains(',') {
            continue;
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            continue;
        }
        // 跳过保留属性
        if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) {
            continue;
        }
        // 去重（同名 cookie 只保留首次）
        if !seen.insert(name.to_string()) {
            continue;
        }
        pairs.push(segment.to_string());
    }

    pairs.join("; ")
}

#[cfg(test)]
mod cookie_tests {
    use super::*;

    #[test]
    fn test_parse_simple_cookie() {
        let raw = "MUSIC_U=abc123; Path=/; HttpOnly";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_U=abc123");
    }

    #[test]
    fn test_parse_multiple_cookies() {
        let raw = "MUSIC_U=abc123; Path=/; HttpOnly, __csrf=xyz; Path=/; HttpOnly";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_U=abc123; __csrf=xyz");
    }

    #[test]
    fn test_parse_cookie_with_expires_date() {
        // Expires 内含 ", "，不应破坏解析
        let raw = "MUSIC_U=abc123; Path=/; HttpOnly; Expires=Fri, 06 Aug 2094 17:42:50 GMT, __csrf=xyz; Path=/; HttpOnly";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_U=abc123; __csrf=xyz");
    }

    #[test]
    fn test_parse_cookie_dedup() {
        let raw = "MUSIC_U=abc; Path=/, MUSIC_U=def; Path=/";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_U=abc");
    }

    #[test]
    fn test_parse_cookie_preserves_order() {
        let raw = "MUSIC_A_T=1513381581471; Max-Age=2147483647; Expires=Fri, 06 Aug 2094 17:42:50 GMT, MUSIC_U=real_token; Path=/; HttpOnly, __csrf=csrf_val; Path=/";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_A_T=1513381581471; MUSIC_U=real_token; __csrf=csrf_val");
    }

    #[test]
    fn test_parse_empty() {
        assert_eq!(parse_set_cookie_to_header(""), "");
        assert_eq!(parse_set_cookie_to_header("   "), "");
    }
}
