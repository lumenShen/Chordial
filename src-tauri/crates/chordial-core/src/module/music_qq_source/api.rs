//! QQMusicApi 接口封装 — 将各 HTTP 端点映射到类型安全的方法。
//!
//! 调用约定与 [`crate::module::music_netease_source::api`] 一致：
//! - 所有方法返回 `Result<T, String>`，错误为人类可读字符串
//! - 自动检查响应 `result == 100`，否则返回错误
//! - 自动剥离外层包装，直接返回业务数据

use super::types::*;
use crate::module::music_web_source::WebHttpClient;

/// 搜索类型 — 对应 QQMusicApi `/search` 接口的 `t` 参数。
#[derive(Debug, Clone, Copy)]
pub enum SearchType {
    /// 单曲（默认）
    Song,
    /// 专辑
    Album,
    /// 歌手
    Singer,
}

impl SearchType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Song => "0",
            Self::Album => "8",
            Self::Singer => "9",
        }
    }
}

/// 生成 Unix 毫秒时间戳字符串（破坏服务端缓存）。
fn now_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    ms.to_string()
}

// ── 搜索 ─────────────────────────────────────────────────────

/// 搜索歌曲。
///
/// `/search?key=<key>&t=0&pageNo=<n>&pageSize=<n>`
pub fn search_songs(
    client: &WebHttpClient,
    query: &str,
    page_no: u32,
    page_size: u32,
) -> Result<Vec<QqSongBrief>, String> {
    let ts = now_ts();
    let resp: QqResponse<QqSearchData<QqSongBrief>> = client.get_json(
        "/search",
        &[
            ("key", query),
            ("t", SearchType::Song.as_str()),
            ("pageNo", &page_no.to_string()),
            ("pageSize", &page_size.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(resp.error_message());
    }
    Ok(resp.data.list)
}

/// 搜索歌手。
///
/// `/search?key=<key>&t=9&pageNo=<n>&pageSize=<n>`
pub fn search_artists(
    client: &WebHttpClient,
    query: &str,
    page_no: u32,
    page_size: u32,
) -> Result<Vec<QqArtistBrief>, String> {
    let ts = now_ts();
    let resp: QqResponse<QqSearchData<QqArtistBrief>> = client.get_json(
        "/search",
        &[
            ("key", query),
            ("t", SearchType::Singer.as_str()),
            ("pageNo", &page_no.to_string()),
            ("pageSize", &page_size.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(resp.error_message());
    }
    Ok(resp.data.list)
}

/// 搜索专辑。
///
/// `/search?key=<key>&t=8&pageNo=<n>&pageSize=<n>`
pub fn search_albums(
    client: &WebHttpClient,
    query: &str,
    page_no: u32,
    page_size: u32,
) -> Result<Vec<QqAlbumBrief>, String> {
    let ts = now_ts();
    let resp: QqResponse<QqSearchData<QqAlbumBrief>> = client.get_json(
        "/search",
        &[
            ("key", query),
            ("t", SearchType::Album.as_str()),
            ("pageNo", &page_no.to_string()),
            ("pageSize", &page_size.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(resp.error_message());
    }
    Ok(resp.data.list)
}

// ── 歌曲 URL 与下载 ─────────────────────────────────────────

/// 获取歌曲可下载 URL。
///
/// `/song/url?id=<songmid>&type=<type>&mediaId=<songmid>`
///
/// # 参数
/// - `songmid`：歌曲 mid（字符串）
/// - `type`：音质字符串（`"128"` / `"320"` / `"m4a"` / `"ape"` / `"flac"`）
///
/// # 返回
/// 完整可下载 URL（domain + purl）。
pub fn get_song_url(
    client: &WebHttpClient,
    songmid: &str,
    quality: &str,
) -> Result<QqSongUrlData, String> {
    let ts = now_ts();
    let resp: QqResponse<QqSongUrlData> = client.get_json(
        "/song/url",
        &[
            ("id", songmid),
            ("type", quality),
            ("mediaId", songmid),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(format!(
            "获取歌曲 URL 失败: {}",
            resp.error_message()
        ));
    }
    if resp.data.data.is_empty() {
        return Err(format!(
            "歌曲 URL 为空（可能是无版权或 VIP 歌曲）songmid={}",
            songmid
        ));
    }
    Ok(resp.data)
}

/// 下载歌曲字节（先获取 URL，再下载字节）。
///
/// 注意：此方法会将整个文件读入 `Vec<u8>`，适合小文件；
/// 大文件请使用 [`download_song_to_file`]。
pub fn download_song(
    client: &WebHttpClient,
    songmid: &str,
    quality: &str,
) -> Result<(Vec<u8>, QqSongUrlData), String> {
    let url_data = get_song_url(client, songmid, quality)?;
    let bytes = client.get_bytes(&url_data.data)?;
    Ok((bytes, url_data))
}

/// 下载歌曲到文件（流式，避免大文件全量读入内存）。
///
/// 相比 `download_song`：内存占用从 O(file_size) 降到 O(8KB)，省去二次拷贝。
pub fn download_song_to_file(
    client: &WebHttpClient,
    songmid: &str,
    quality: &str,
    path: &std::path::Path,
) -> Result<(u64, QqSongUrlData), String> {
    let url_data = get_song_url(client, songmid, quality)?;
    // 下载超时 300s（足以下载 50MB+ 无损文件）
    let written = client.download_to_file(&url_data.data, path, 300)?;
    Ok((written, url_data))
}

// ── 歌词 ─────────────────────────────────────────────────────

/// 获取歌词（含翻译）。
///
/// `/lyric?songmid=<songmid>`
///
/// # 参数
/// - `songmid`：歌曲 mid
///
/// # 返回
/// [`QqLyricResponse`]，其中 `lyric` 和 `trans` 字段已由上游服务端解码为 UTF-8 文本。
/// 上游接口在非 raw 模式下会自动 base64 解码并包装为 `{ result: 100, data: { lyric, trans } }`。
pub fn get_lyric(
    client: &WebHttpClient,
    songmid: &str,
) -> Result<QqLyricResponse, String> {
    let ts = now_ts();
    let resp: QqResponse<QqLyricResponse> = client.get_json(
        "/lyric",
        &[
            ("songmid", songmid),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(format!("获取歌词失败: {}", resp.error_message()));
    }
    Ok(resp.data)
}

// ── 专辑信息 ─────────────────────────────────────────────────

/// 获取专辑信息。
///
/// `/album?albummid=<albummid>`
///
/// # 返回
/// [`QqAlbumInfo`]，包含专辑名、歌手列表、发行时间等。
pub fn get_album_info(
    client: &WebHttpClient,
    albummid: &str,
) -> Result<QqAlbumInfo, String> {
    let ts = now_ts();
    let resp: QqResponse<QqAlbumInfo> = client.get_json(
        "/album",
        &[
            ("albummid", albummid),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if !resp.is_ok() {
        return Err(format!("获取专辑信息失败: {}", resp.error_message()));
    }
    Ok(resp.data)
}
