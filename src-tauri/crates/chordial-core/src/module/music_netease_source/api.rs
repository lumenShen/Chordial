//! NeteaseCloudMusicApi 接口封装 — 将各 HTTP 端点映射到类型安全的方法。
//!
//! 这些方法都是同步阻塞的，因为 [`super::source::NeteaseCloudMusicApiSource`]
//! 实现的 [`MusicSource`](crate::module::music_source::traits::MusicSource) 是同步接口。
//!
//! # 调用约定
//!
//! - 所有方法返回 `Result<T, String>`，错误为人类可读字符串
//! - 自动检查响应 `code == 200`，否则返回错误
//! - 自动剥离 NeteaseResponse 外层包装，直接返回业务数据

use super::types::*;
use crate::module::music_web_source::WebHttpClient;

/// 搜索类型 — 对应 NeteaseCloudMusicApi `/search` 接口的 `type` 参数。
#[derive(Debug, Clone, Copy)]
pub enum SearchType {
    /// 单曲（默认）
    Song,
    /// 歌单
    Playlist,
    /// 专辑
    Album,
    /// 歌手
    Artist,
}

impl SearchType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Song => "1",
            Self::Playlist => "1000",
            Self::Album => "10",
            Self::Artist => "100",
        }
    }
}

/// 搜索歌曲。
pub fn search_songs(
    client: &WebHttpClient,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<NeteaseSongBrief>, String> {
    let ts = now_ts();
    let resp: NeteaseSearchResponse = client.get_json(
        "/search",
        &[
            ("keywords", query),
            ("type", SearchType::Song.as_str()),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(resp.error_message());
    }
    Ok(resp.result.songs)
}

/// 搜索歌单。
pub fn search_playlists(
    client: &WebHttpClient,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<NeteasePlaylistBrief>, String> {
    let ts = now_ts();
    let resp: NeteaseSearchResponse = client.get_json(
        "/search",
        &[
            ("keywords", query),
            ("type", SearchType::Playlist.as_str()),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(resp.error_message());
    }
    Ok(resp.result.playlists)
}

/// 搜索歌手（type=100）。
pub fn search_artists(
    client: &WebHttpClient,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<NeteaseArtistBrief>, String> {
    let ts = now_ts();
    let resp: NeteaseSearchResponse = client.get_json(
        "/search",
        &[
            ("keywords", query),
            ("type", SearchType::Artist.as_str()),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(resp.error_message());
    }
    Ok(resp.result.artists)
}

/// 搜索专辑（type=10）。
pub fn search_albums(
    client: &WebHttpClient,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<NeteaseAlbumBrief>, String> {
    let ts = now_ts();
    let resp: NeteaseSearchResponse = client.get_json(
        "/search",
        &[
            ("keywords", query),
            ("type", SearchType::Album.as_str()),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(resp.error_message());
    }
    Ok(resp.result.albums)
}

/// 获取歌单详情（含元信息）。
pub fn get_playlist_detail(
    client: &WebHttpClient,
    playlist_id: i64,
) -> Result<NeteasePlaylistDetail, String> {
    let ts = now_ts();
    let resp: NeteasePlaylistDetailResponse = client.get_json(
        "/playlist/detail",
        &[("id", &playlist_id.to_string()), ("timestamp", ts.as_str())],
    )?;
    if resp.code != 200 {
        return Err(format!("获取歌单详情失败 code={}", resp.code));
    }
    Ok(resp.playlist)
}

/// 获取歌单全部曲目（分页）。
///
/// `limit` 通常最大 1000，超过会被 API 截断。
pub fn get_playlist_tracks(
    client: &WebHttpClient,
    playlist_id: i64,
    limit: u32,
    offset: u32,
) -> Result<Vec<NeteaseSongDetail>, String> {
    let ts = now_ts();
    let resp: NeteasePlaylistTracksResponse = client.get_json(
        "/playlist/track/all",
        &[
            ("id", &playlist_id.to_string()),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.code != 200 {
        return Err(format!("获取歌单曲目失败 code={}", resp.code));
    }
    Ok(resp.songs)
}

/// 获取歌曲详情（支持批量）。
///
/// `ids` 应为 `"[123,456,789]"` 格式（JSON 数组字符串）。
pub fn get_song_details(
    client: &WebHttpClient,
    ids_json: &str,
) -> Result<Vec<NeteaseSongDetail>, String> {
    // NeteaseCloudMusicApi 的 /song/detail 接口返回 { songs: [...] }
    #[derive(serde::Deserialize)]
    struct SongDetailResp {
        #[serde(default)]
        code: i32,
        #[serde(default)]
        songs: Vec<NeteaseSongDetail>,
    }
    let ts = now_ts();
    let resp: SongDetailResp = client.get_json(
        "/song/detail",
        &[("ids", ids_json), ("timestamp", ts.as_str())],
    )?;
    if resp.code != 200 {
        return Err(format!("获取歌曲详情失败 code={}", resp.code));
    }
    Ok(resp.songs)
}

/// 获取歌曲可下载 URL。
///
/// `level` 为音质等级字符串（"standard"/"higher"/"exhigh"/"lossless"/"hires"）。
/// 返回的 `NeteaseSongUrlData.url` 即为可直接下载的 CDN URL。
pub fn get_song_url(
    client: &WebHttpClient,
    song_id: i64,
    level: &str,
) -> Result<NeteaseSongUrlData, String> {
    let ts = now_ts();
    let resp: NeteaseSongUrlResponse = client.get_json(
        "/song/url/v1",
        &[
            ("id", &song_id.to_string()),
            ("level", level),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.code != 200 {
        return Err(format!("获取歌曲 URL 失败 code={}", resp.code));
    }
    resp.data
        .into_iter()
        .next()
        .ok_or_else(|| "歌曲无可用音源".to_string())
}

/// 获取歌词（含 LRC/YRC/翻译/音译）。
///
/// 优先调用 `/lyric/new`（逐字歌词 yrc 更完整，调用 `music.163.com/api/song/lyric/v1` 端点）；
/// 失败或非 200 时回退到 `/lyric`（旧端点，部分歌曲仅返回 lrc）。
///
/// yrc 字段格式示例：
/// ```text
/// [16210,3460](16210,670,0)还(16880,410,0)没...
/// ~~~~1 ~~~2  ~~~~3 ~~4 5 ~6
/// 1. 歌词行显示开始时间戳(ms)
/// 2. 歌词行显示总时长(ms)
/// 3. 逐字显示开始时间戳(ms)
/// 4. 逐字显示时长(ms)
/// 5. 未知
/// 6. 文字
/// ```
pub fn get_lyric(
    client: &WebHttpClient,
    song_id: i64,
) -> Result<NeteaseLyricResponse, String> {
    let ts = now_ts();
    let id_str = song_id.to_string();

    // 优先 /lyric/new（yrc 更完整）
    match client.get_json::<NeteaseLyricResponse>(
        "/lyric/new",
        &[("id", &id_str), ("timestamp", ts.as_str())],
    ) {
        Ok(resp) if resp.code == 200 => return Ok(resp),
        Ok(resp) if resp.code == 404 => {
            // 404 明确表示无此资源，直接返回错误，避免无谓重试
            return Err(format!("获取歌词失败 code=404 (lyric/new)"));
        }
        // 其他错误（网络/解析/限流）→ 回退到 /lyric
        Ok(_) | Err(_) => {}
    }

    // 回退 /lyric
    let resp: NeteaseLyricResponse = client.get_json(
        "/lyric",
        &[("id", &id_str), ("timestamp", ts.as_str())],
    )?;
    if resp.code != 200 {
        return Err(format!("获取歌词失败 code={}", resp.code));
    }
    Ok(resp)
}

/// 下载歌曲字节（先获取 URL，再下载字节）。
///
/// `level` 为音质等级字符串。返回 `Vec<u8>` 即为音频文件字节。
pub fn download_song(
    client: &WebHttpClient,
    song_id: i64,
    level: &str,
) -> Result<(Vec<u8>, NeteaseSongUrlData), String> {
    let url_data = get_song_url(client, song_id, level)?;
    let url = url_data
        .url
        .clone()
        .ok_or_else(|| "歌曲 URL 为空（可能是无版权或 VIP 歌曲）".to_string())?;
    let bytes = client.get_bytes(&url)?;
    Ok((bytes, url_data))
}

/// 下载歌曲到文件（流式，避免大文件全量读入内存）。
///
/// 相比 `download_song`：
/// - 内存占用从 O(file_size) 降到 O(8KB)（流式缓冲区）
/// - 省去 Vec<u8> → 文件 的二次拷贝
/// - 下载超时单独配置（默认 300s，足以下载 lossless/hires）
///
/// # 参数
/// - `song_id`: 网易云歌曲 ID
/// - `level`: 音质等级（"standard"/"higher"/"exhigh"/"lossless"/"hires"）
/// - `path`: 目标文件路径（父目录应已存在）
///
/// # 返回
/// `(written_bytes, url_data)` — 写入字节数 + URL 元信息（含音质/类型）
pub fn download_song_to_file(
    client: &WebHttpClient,
    song_id: i64,
    level: &str,
    path: &std::path::Path,
) -> Result<(u64, NeteaseSongUrlData), String> {
    let url_data = get_song_url(client, song_id, level)?;
    let url = url_data
        .url
        .clone()
        .ok_or_else(|| "歌曲 URL 为空（可能是无版权或 VIP 歌曲）".to_string())?;
    // 下载超时 300s（足以下载 50MB+ lossless 文件）
    let written = client.download_to_file(&url, path, 300)?;
    Ok((written, url_data))
}

// ───────────────────────────────────────────────────────────────────────────
// 扫码登录
// ───────────────────────────────────────────────────────────────────────────

/// 第 1 步：获取扫码登录的 unikey。
///
/// 调用 `/login/qr/key`，返回一个 32 位字符串 key，用于后续生成二维码与轮询。
pub fn login_qr_key(client: &WebHttpClient) -> Result<String, String> {
    // 加 timestamp 防止任何中间层（代理/浏览器/reqwest 连接池/ncmapi 内部）缓存
    let ts = now_ts();
    let resp: NeteaseQrKeyResponse =
        client.get_json("/login/qr/key", &[("timestamp", ts.as_str())])?;
    if resp.code != 200 {
        return Err(format!("获取扫码 key 失败 code={}", resp.code));
    }
    resp.data
        .map(|d| d.unikey)
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "API 未返回 unikey".to_string())
}

/// 第 2 步：用 unikey 生成二维码。
///
/// `qrimg = true` 时返回 base64 data URL（可直接作为 `<img src>`），否则只返回 qrurl。
pub fn login_qr_create(
    client: &WebHttpClient,
    key: &str,
    qrimg: bool,
) -> Result<NeteaseQrCreateData, String> {
    let qrimg_str = if qrimg { "true" } else { "false" };
    let ts = now_ts();
    let resp: NeteaseQrCreateResponse = client.get_json(
        "/login/qr/create",
        &[
            ("key", key),
            ("qrimg", qrimg_str),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.code != 200 {
        return Err(format!("生成二维码失败 code={}", resp.code));
    }
    resp.data.ok_or_else(|| "API 未返回二维码数据".to_string())
}

/// 第 3 步：轮询扫码状态。
///
/// 返回 [`NeteaseQrCheckResponse`]，调用方根据 `code` 字段判断状态：
/// - `800`: 二维码过期
/// - `801`: 等待扫码
/// - `802`: 已扫码，待确认
/// - `803`: 授权成功，`cookie` 字段为登录 cookie 字符串
pub fn login_qr_check(
    client: &WebHttpClient,
    key: &str,
) -> Result<NeteaseQrCheckResponse, String> {
    // 关键：加 timestamp 破坏任何缓存层。
    // ncmapi enhanced 在内部对 GET 请求做了缓存（基于 URL），
    // 不带 timestamp 时同一 URL 的 check 请求会被服务端缓存返回第一次的结果，
    // 即使前端持续轮询，ncmapi 日志可能只显示一次实际请求。
    let ts = now_ts();
    let resp: NeteaseQrCheckResponse = client.get_json(
        "/login/qr/check",
        &[("key", key), ("timestamp", ts.as_str())],
    )?;
    // 此处允许非 200 的 code（800/801/802/803），它们是业务状态而非错误
    Ok(resp)
}

/// 当前 Unix 时间戳（毫秒）— 用于破坏 ncmapi 内部的 GET 请求缓存。
fn now_ts() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

// ───────────────────────────────────────────────────────────────────────────
// 用户账户与"我的资源"
// ───────────────────────────────────────────────────────────────────────────

/// 获取当前登录账号信息（依赖 cookie）。
///
/// 调用 `/user/account`，返回 `NeteaseUserAccountResponse`。
/// 需要登录态 cookie（含 MUSIC_U），未登录时 `profile` 为 `None`。
pub fn get_user_account(client: &WebHttpClient) -> Result<NeteaseUserAccountResponse, String> {
    let ts = now_ts();
    let resp: NeteaseUserAccountResponse =
        client.get_json("/user/account", &[("timestamp", ts.as_str())])?;
    Ok(resp)
}

/// 获取指定用户的全部歌单（创建 + 收藏，通过 `subscribed` 字段区分）。
///
/// 调用 `/user/playlist?uid=xxx`。一次返回所有歌单（不分页）。
pub fn get_user_playlists(
    client: &WebHttpClient,
    uid: i64,
) -> Result<Vec<NeteasePlaylistBrief>, String> {
    let ts = now_ts();
    let resp: NeteaseUserPlaylistsResponse = client.get_json(
        "/user/playlist",
        &[("uid", &uid.to_string()), ("timestamp", ts.as_str())],
    )?;
    if resp.code != 200 {
        return Err(format!("获取用户歌单失败 code={}", resp.code));
    }
    Ok(resp.playlist)
}

/// 获取当前用户收藏的专辑（依赖 cookie，分页）。
///
/// 调用 `/album/sublist?limit=25&offset=0`。
pub fn get_user_albums(
    client: &WebHttpClient,
    limit: u32,
    offset: u32,
) -> Result<(Vec<NeteaseSubAlbum>, i64), String> {
    let ts = now_ts();
    let resp: NeteaseAlbumSublistResponse = client.get_json(
        "/album/sublist",
        &[
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.code != 200 {
        return Err(format!("获取收藏专辑失败 code={}", resp.code));
    }
    let count = resp.count;
    Ok((resp.data, count))
}

/// 获取专辑详情（含全部曲目）。
///
/// 调用 `/album?id=xxx`，返回 `(album_meta, songs)`。
pub fn get_album_detail(
    client: &WebHttpClient,
    album_id: i64,
) -> Result<(NeteaseAlbumDetail, Vec<NeteaseSongDetail>), String> {
    let ts = now_ts();
    let resp: NeteaseAlbumDetailResponse = client.get_json(
        "/album",
        &[("id", &album_id.to_string()), ("timestamp", ts.as_str())],
    )?;
    if resp.code != 200 {
        return Err(format!("获取专辑详情失败 code={}", resp.code));
    }
    let album = resp.album.ok_or_else(|| "API 未返回专辑数据".to_string())?;
    Ok((album, resp.songs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_type_str() {
        assert_eq!(SearchType::Song.as_str(), "1");
        assert_eq!(SearchType::Playlist.as_str(), "1000");
        assert_eq!(SearchType::Album.as_str(), "10");
        assert_eq!(SearchType::Artist.as_str(), "100");
    }

    #[test]
    fn test_audio_quality_ordering() {
        assert!(AudioQuality::Lossless > AudioQuality::Exhigh);
        assert!(AudioQuality::Hires > AudioQuality::Lossless);
        assert!(AudioQuality::Standard < AudioQuality::Higher);
    }

    #[test]
    fn test_audio_quality_from_str() {
        assert_eq!(
            AudioQuality::from_level_str("lossless"),
            AudioQuality::Lossless
        );
        assert_eq!(
            AudioQuality::from_level_str("STANDARD"),
            AudioQuality::Standard
        );
        assert_eq!(AudioQuality::from_level_str("unknown"), AudioQuality::Unknown);
    }

    #[test]
    fn test_audio_quality_to_str() {
        assert_eq!(AudioQuality::Lossless.to_level_str(), "lossless");
        assert_eq!(AudioQuality::Unknown.to_level_str(), "standard");
    }

    #[test]
    fn test_audio_quality_from_extension() {
        assert_eq!(AudioQuality::from_extension("flac"), AudioQuality::Lossless);
        assert_eq!(AudioQuality::from_extension("MP3"), AudioQuality::Exhigh);
        assert_eq!(AudioQuality::from_extension("wav"), AudioQuality::Hires);
    }
}
