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
    let resp: NeteaseResponse<NeteaseSearchResult> = client.get_json(
        "/search",
        &[
            ("keywords", query),
            ("type", SearchType::Song.as_str()),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(resp.error_message());
    }
    Ok(resp.data.songs)
}

/// 搜索歌单。
pub fn search_playlists(
    client: &WebHttpClient,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<NeteasePlaylistBrief>, String> {
    let resp: NeteaseResponse<NeteaseSearchResult> = client.get_json(
        "/search",
        &[
            ("keywords", query),
            ("type", SearchType::Playlist.as_str()),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(resp.error_message());
    }
    Ok(resp.data.playlists)
}

/// 获取歌单详情（含元信息）。
pub fn get_playlist_detail(
    client: &WebHttpClient,
    playlist_id: i64,
) -> Result<NeteasePlaylistDetail, String> {
    let resp: NeteasePlaylistDetailResponse =
        client.get_json("/playlist/detail", &[("id", &playlist_id.to_string())])?;
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
    let resp: NeteasePlaylistTracksResponse = client.get_json(
        "/playlist/track/all",
        &[
            ("id", &playlist_id.to_string()),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
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
    let resp: SongDetailResp = client.get_json("/song/detail", &[("ids", ids_json)])?;
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
    let resp: NeteaseSongUrlResponse = client.get_json(
        "/song/url/v1",
        &[
            ("id", &song_id.to_string()),
            ("level", level),
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
pub fn get_lyric(
    client: &WebHttpClient,
    song_id: i64,
) -> Result<NeteaseLyricResponse, String> {
    let resp: NeteaseLyricResponse =
        client.get_json("/lyric", &[("id", &song_id.to_string())])?;
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

// ───────────────────────────────────────────────────────────────────────────
// 扫码登录
// ───────────────────────────────────────────────────────────────────────────

/// 第 1 步：获取扫码登录的 unikey。
///
/// 调用 `/login/qr/key`，返回一个 32 位字符串 key，用于后续生成二维码与轮询。
pub fn login_qr_key(client: &WebHttpClient) -> Result<String, String> {
    let resp: NeteaseQrKeyResponse = client.get_json("/login/qr/key", &[])?;
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
    let resp: NeteaseQrCreateResponse = client.get_json(
        "/login/qr/create",
        &[("key", key), ("qrimg", qrimg_str)],
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
    let resp: NeteaseQrCheckResponse = client.get_json("/login/qr/check", &[("key", key)])?;
    // 此处允许非 200 的 code（800/801/802/803），它们是业务状态而非错误
    Ok(resp)
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
