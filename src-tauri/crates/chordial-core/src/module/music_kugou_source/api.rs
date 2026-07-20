//! KuGouMusicApi 接口封装 — 将各 HTTP 端点映射到类型安全的方法。
//!
//! 调用约定与 [`crate::module::music_netease_source::api`] 一致：
//! - 所有方法返回 `Result<T, String>`，错误为人类可读字符串
//! - 自动检查响应 `status == 1` 或 `err_code == 0`，否则返回错误
//! - 自动剥离外层包装，直接返回业务数据

use super::types::*;
use crate::module::music_web_source::WebHttpClient;

/// 搜索类型 — 对应 KuGouMusicApi `/search` 接口的 `type` 参数。
#[derive(Debug, Clone, Copy)]
pub enum SearchType {
    /// 单曲（默认）
    Song,
    /// 专辑
    Album,
    /// 歌手
    Author,
    /// 歌单
    Special,
}

impl SearchType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Album => "album",
            Self::Author => "author",
            Self::Special => "special",
        }
    }
}

/// 生成 Unix 毫秒时间戳字符串（破坏 ncmapi 风格服务端的 2 分钟缓存）。
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
/// `/search?keywords=xxx&type=song&page=1&pagesize=30`
pub fn search_songs(
    client: &WebHttpClient,
    query: &str,
    page: u32,
    pagesize: u32,
) -> Result<Vec<KugouSongBrief>, String> {
    let ts = now_ts();
    let resp: KugouSearchResponse<KugouSongBrief> = client.get_json(
        "/search",
        &[
            ("keywords", query),
            ("type", SearchType::Song.as_str()),
            ("page", &page.to_string()),
            ("pagesize", &pagesize.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.status != 1 {
        return Err(format!("搜索歌曲失败 status={}, error_code={}", resp.status, resp.error_code));
    }
    Ok(resp.data.lists)
}

/// 搜索歌手。
///
/// `/search?keywords=xxx&type=author`
pub fn search_artists(
    client: &WebHttpClient,
    query: &str,
    page: u32,
    pagesize: u32,
) -> Result<Vec<KugouArtistBrief>, String> {
    let ts = now_ts();
    let resp: KugouSearchResponse<KugouArtistBrief> = client.get_json(
        "/search",
        &[
            ("keywords", query),
            ("type", SearchType::Author.as_str()),
            ("page", &page.to_string()),
            ("pagesize", &pagesize.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.status != 1 {
        return Err(format!("搜索歌手失败 status={}, error_code={}", resp.status, resp.error_code));
    }
    Ok(resp.data.lists)
}

/// 搜索专辑。
///
/// `/search?keywords=xxx&type=album`
pub fn search_albums(
    client: &WebHttpClient,
    query: &str,
    page: u32,
    pagesize: u32,
) -> Result<Vec<KugouAlbumBrief>, String> {
    let ts = now_ts();
    let resp: KugouSearchResponse<KugouAlbumBrief> = client.get_json(
        "/search",
        &[
            ("keywords", query),
            ("type", SearchType::Album.as_str()),
            ("page", &page.to_string()),
            ("pagesize", &pagesize.to_string()),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.status != 1 {
        return Err(format!("搜索专辑失败 status={}, error_code={}", resp.status, resp.error_code));
    }
    Ok(resp.data.lists)
}

// ── 歌曲详情与 URL ──────────────────────────────────────────

/// 获取歌曲可下载 URL。
///
/// `/song/url?hash=<hash>&quality=<quality>&album_audio_id=<id>`
///
/// # 参数
/// - `hash`：歌曲 FileHash（32 位十六进制，大小写不敏感）
/// - `quality`：音质字符串（"128"/"320"/"flac"/"high"/"super"）
/// - `album_audio_id`：可选，提升 URL 解析成功率
pub fn get_song_url(
    client: &WebHttpClient,
    hash: &str,
    quality: &str,
    album_audio_id: Option<i64>,
) -> Result<KugouSongUrlData, String> {
    let ts = now_ts();
    let album_audio_id_str = album_audio_id
        .filter(|&id| id != 0)
        .map(|id| id.to_string())
        .unwrap_or_else(|| "0".to_string());

    let resp: KugouSongUrlResponse = client.get_json(
        "/song/url",
        &[
            ("hash", hash),
            ("quality", quality),
            ("album_audio_id", &album_audio_id_str),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.status != 1 {
        return Err(format!("获取歌曲 URL 失败 status={}, err_code={}", resp.status, resp.err_code));
    }
    if resp.data.url.is_empty() {
        return Err(format!("歌曲 URL 为空（可能是无版权或 VIP 歌曲）hash={}", hash));
    }
    Ok(resp.data)
}

/// 下载歌曲字节（先获取 URL，再下载字节）。
///
/// 注意：此方法会将整个文件读入 `Vec<u8>`，适合小文件；
/// 大文件请使用 [`download_song_to_file`]。
pub fn download_song(
    client: &WebHttpClient,
    hash: &str,
    quality: &str,
    album_audio_id: Option<i64>,
) -> Result<(Vec<u8>, KugouSongUrlData), String> {
    let url_data = get_song_url(client, hash, quality, album_audio_id)?;
    let url = url_data
        .url
        .first()
        .map(|u| u.url.clone())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "歌曲 URL 为空".to_string())?;
    let bytes = client.get_bytes(&url)?;
    Ok((bytes, url_data))
}

/// 下载歌曲到文件（流式，避免大文件全量读入内存）。
///
/// 相比 `download_song`：内存占用从 O(file_size) 降到 O(8KB)，省去二次拷贝。
pub fn download_song_to_file(
    client: &WebHttpClient,
    hash: &str,
    quality: &str,
    album_audio_id: Option<i64>,
    path: &std::path::Path,
) -> Result<(u64, KugouSongUrlData), String> {
    let url_data = get_song_url(client, hash, quality, album_audio_id)?;
    let url = url_data
        .url
        .first()
        .map(|u| u.url.clone())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "歌曲 URL 为空".to_string())?;
    // 下载超时 300s（足以下载 50MB+ 无损文件）
    let written = client.download_to_file(&url, path, 300)?;
    Ok((written, url_data))
}

// ── 歌词 ─────────────────────────────────────────────────────

/// 搜索歌词候选（基于 hash 匹配）。
///
/// `/search/lyric?hash=<hash>&duration=<ms>&man=yes`
///
/// # 参数
/// - `hash`：歌曲 FileHash
/// - `duration`：歌曲时长（毫秒），帮助精确匹配
///
/// # 返回
/// 候选列表（按相关度排序），第一个即为最匹配项。
pub fn search_lyric(
    client: &WebHttpClient,
    hash: &str,
    duration_ms: i64,
) -> Result<Vec<KugouLyricCandidate>, String> {
    let ts = now_ts();
    let duration_str = duration_ms.to_string();
    let resp: KugouLyricSearchResponse = client.get_json(
        "/search/lyric",
        &[
            ("hash", hash),
            ("duration", &duration_str),
            ("man", "yes"), // 返回多个候选
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.status != 200 {
        return Err(format!(
            "搜索歌词失败 status={}, err_code={}",
            resp.status, resp.err_code
        ));
    }
    Ok(resp.candidates)
}

/// 下载歌词文本。
///
/// `/lyric?id=<id>&accesskey=<key>&fmt=lrc&decode=true`
///
/// # 参数
/// - `id`：歌词 ID（来自 [`search_lyric`]）
/// - `accesskey`：歌词 accesskey
///
/// # 返回
/// 解码后的 LRC 文本（带时间戳）；若不存在则返回空字符串。
pub fn get_lyric(
    client: &WebHttpClient,
    id: &str,
    accesskey: &str,
) -> Result<KugouLyricResponse, String> {
    let ts = now_ts();
    let resp: KugouLyricResponse = client.get_json(
        "/lyric",
        &[
            ("id", id),
            ("accesskey", accesskey),
            ("fmt", "lrc"),
            ("decode", "true"),
            ("client", "android"),
            ("ver", "1"),
            ("charset", "utf8"),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.status != 200 {
        return Err(format!(
            "获取歌词失败 status={}, err_code={}",
            resp.status, resp.err_code
        ));
    }
    Ok(resp)
}

// ── 专辑信息 ─────────────────────────────────────────────────

/// 获取专辑信息（含封面 URL）。
///
/// `/album?album_id=<id>&fields=sizable_cover,cover,album_name,author_name,publish_date,intro,language`
pub fn get_album_info(
    client: &WebHttpClient,
    album_id: &str,
) -> Result<KugouAlbumInfo, String> {
    let ts = now_ts();
    let resp: KugouAlbumInfoResponse = client.post_json(
        "/album",
        &[
            ("album_id", album_id),
            ("fields", "sizable_cover,cover,album_name,author_name,publish_date,intro,language"),
            ("timestamp", ts.as_str()),
        ],
    )?;
    if resp.status != 1 {
        return Err(format!(
            "获取专辑信息失败 status={}, err_code={}",
            resp.status, resp.err_code
        ));
    }
    resp.data.into_iter().next().ok_or_else(|| "专辑信息为空".to_string())
}
