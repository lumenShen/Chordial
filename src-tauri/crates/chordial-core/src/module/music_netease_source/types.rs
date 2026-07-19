//! NeteaseCloudMusicApi 返回的数据模型。
//!
//! 这些结构体严格对应 NeteaseCloudMusicApi 的 JSON 响应格式，
//! 用 `#[serde(default)]` 容忍字段缺失（API 不同版本字段可能略有差异）。
//!
//! 注意：JSON 字段保持 camelCase（与 API 一致），所以模块整体允许 non_snake_case。

#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};

/// 音质等级 — 用于本地/在线音质比较。
///
/// 顺序由低到高，便于通过 `as u8` 直接比较。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum AudioQuality {
    /// 未知 / 无可用音源
    Unknown = 0,
    /// 标准音质（128 kbps MP3）
    Standard = 1,
    /// 较高音质（192 kbps MP3）
    Higher = 2,
    /// 极高音质（320 kbps MP3）
    Exhigh = 3,
    /// 无损音质（FLAC）
    Lossless = 4,
    /// Hi-Res（FLAC 24bit/96kHz+）
    Hires = 5,
}

impl AudioQuality {
    /// 从 NeteaseCloudMusicApi 的 `level` 字符串解析。
    pub fn from_level_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "standard" => Self::Standard,
            "higher" => Self::Higher,
            "exhigh" | "exhighs" => Self::Exhigh,
            "lossless" => Self::Lossless,
            "hires" => Self::Hires,
            _ => Self::Unknown,
        }
    }

    /// 转换为 NeteaseCloudMusicApi 的 `level` 字符串。
    pub fn to_level_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Higher => "higher",
            Self::Exhigh => "exhigh",
            Self::Lossless => "lossless",
            Self::Hires => "hires",
            Self::Unknown => "standard",
        }
    }

    /// 从文件扩展名推断音质（粗略）。
    /// `.flac` → Lossless，`.mp3` → Exhigh（假设 320kbps）。
    /// 真实音质需要读取文件元数据。
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "flac" => Self::Lossless,
            "wav" | "ape" => Self::Hires,
            "mp3" | "m4a" | "aac" => Self::Exhigh,
            _ => Self::Unknown,
        }
    }

    /// 是否为无损及以上音质。
    pub fn is_lossless_or_above(self) -> bool {
        self >= Self::Lossless
    }
}

impl Default for AudioQuality {
    fn default() -> Self {
        Self::Unknown
    }
}

// ───────────────────────────────────────────────────────────────────────────
// API 响应数据模型
// ───────────────────────────────────────────────────────────────────────────

/// NeteaseCloudMusicApi 通用响应包装。
///
/// 大部分接口返回 `{ code: 200, ... }`，code != 200 表示业务错误。
#[derive(Debug, Clone, Deserialize)]
pub struct NeteaseResponse<T> {
    pub code: i32,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    /// 实际数据字段，名称因接口而异（`songs` / `result` / `playlist` 等）
    #[serde(flatten)]
    pub data: T,
}

impl<T> NeteaseResponse<T> {
    /// 检查响应码是否成功（200）。
    pub fn is_ok(&self) -> bool {
        self.code == 200
    }

    /// 提取错误消息（优先 message，其次 msg）。
    pub fn error_message(&self) -> String {
        self.message
            .clone()
            .or_else(|| self.msg.clone())
            .unwrap_or_else(|| format!("NeteaseCloudMusicApi 错误 code={}", self.code))
    }
}

/// 搜索结果（/search 接口）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseSearchResult {
    #[serde(default)]
    pub songs: Vec<NeteaseSongBrief>,
    #[serde(default)]
    pub playlists: Vec<NeteasePlaylistBrief>,
    #[serde(default)]
    pub albums: Vec<NeteaseAlbumBrief>,
    #[serde(default)]
    pub artists: Vec<NeteaseArtistBrief>,
}

/// 歌曲简略信息（搜索结果）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseSongBrief {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artists: Vec<NeteaseArtistBrief>,
    #[serde(default)]
    pub album: Option<NeteaseAlbumBrief>,
    #[serde(default)]
    pub duration: i64,
}

/// 歌手简略信息。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteaseArtistBrief {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub img1v1Url: Option<String>,
    #[serde(default)]
    pub picUrl: Option<String>,
}

/// 专辑简略信息。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteaseAlbumBrief {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub picUrl: Option<String>,
    #[serde(default)]
    pub artist: Option<NeteaseArtistBrief>,
    #[serde(default)]
    pub artists: Vec<NeteaseArtistBrief>,
    #[serde(default)]
    pub publishTime: i64,
}

/// 歌单简略信息（搜索结果）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteasePlaylistBrief {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub coverImgUrl: Option<String>,
    #[serde(default)]
    pub creator: Option<NeteasePlaylistCreator>,
    #[serde(default)]
    pub trackCount: i32,
    #[serde(default)]
    pub playCount: i64,
    #[serde(default)]
    pub description: Option<String>,
}

/// 歌单创建者信息。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteasePlaylistCreator {
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub userId: i64,
}

/// 歌单详情（/playlist/detail 或 /playlist/track/all 接口）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteasePlaylistDetail {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub coverImgUrl: Option<String>,
    #[serde(default)]
    pub creator: Option<NeteasePlaylistCreator>,
    #[serde(default)]
    pub trackCount: i32,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 歌单包含的歌曲 ID 列表（部分接口返回）
    #[serde(default)]
    pub trackIds: Vec<NeteaseTrackId>,
}

/// trackIds 数组项。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteaseTrackId {
    pub id: i64,
}

/// 歌曲详情（/song/detail 接口）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteaseSongDetail {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub ar: Vec<NeteaseArtistBrief>,
    #[serde(default)]
    pub al: Option<NeteaseAlbumBrief>,
    #[serde(default)]
    pub dt: i64,
    /// 文件大小（字节），不同音质不同
    #[serde(default)]
    pub size: Option<i64>,
}

/// 歌曲可下载 URL 响应（/song/url/v1 接口）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseSongUrlData {
    pub id: i64,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub br: i64,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub r#type: Option<String>,
    /// level 字段（"standard" / "higher" / "exhigh" / "lossless" / "hires"）
    #[serde(default)]
    pub level: Option<String>,
}

/// 歌曲 URL 响应包装。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseSongUrlResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub data: Vec<NeteaseSongUrlData>,
}

/// 歌词响应（/lyric 接口）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseLyricResponse {
    #[serde(default)]
    pub code: i32,
    /// 普通 LRC 歌词
    #[serde(default)]
    pub lrc: Option<NeteaseLyricContent>,
    /// 翻译歌词
    #[serde(default)]
    pub tlyric: Option<NeteaseLyricContent>,
    /// 音译歌词（罗马音）
    #[serde(default)]
    pub romalrc: Option<NeteaseLyricContent>,
    /// 逐字歌词（YRC）
    #[serde(default)]
    pub yrc: Option<NeteaseLyricContent>,
    /// 逐字歌词的翻译（部分歌曲有）
    #[serde(default)]
    pub ytlrc: Option<NeteaseLyricContent>,
    /// 逐字歌词的音译（部分歌曲有）
    #[serde(default)]
    pub yromalrc: Option<NeteaseLyricContent>,
}

/// 歌词内容包装。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseLyricContent {
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub lyric: String,
}

/// 歌单详情响应包装（/playlist/detail 接口）。
///
/// 注意：歌单详情返回的是 `{ code, playlist: {...} }`，与搜索不同。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteasePlaylistDetailResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub playlist: NeteasePlaylistDetail,
}

/// 歌单曲目响应（/playlist/track/all 接口）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteasePlaylistTracksResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub songs: Vec<NeteaseSongDetail>,
}

// ───────────────────────────────────────────────────────────────────────────
// 扫码登录响应（/login/qr/* 接口）
// ───────────────────────────────────────────────────────────────────────────

/// `/login/qr/key` 响应包装。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseQrKeyResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub data: Option<NeteaseQrKeyData>,
}

/// `/login/qr/key` 返回的 unikey 载荷。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseQrKeyData {
    #[serde(default)]
    pub unikey: String,
}

/// `/login/qr/create` 响应包装。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseQrCreateResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub data: Option<NeteaseQrCreateData>,
}

/// `/login/qr/create` 返回的二维码载荷。
///
/// - `qrurl`: 可直接转化为二维码的 URL（用前端 qrcode 库渲染即可）
/// - `qrimg`: 当请求带 `qrimg=true` 时返回 `data:image/png;base64,...`，可直接作为 `<img src>`
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseQrCreateData {
    #[serde(default)]
    pub qrurl: String,
    #[serde(default)]
    pub qrimg: Option<String>,
}

/// `/login/qr/check` 响应。
///
/// `code` 取值：
/// - `800`: 二维码过期
/// - `801`: 等待扫码
/// - `802`: 已扫码，等待用户确认
/// - `803`: 授权成功，`cookie` 字段返回登录 cookie 字符串
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseQrCheckResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub message: Option<String>,
    /// 仅 code == 802 时可能返回（用户头像 URL）
    #[serde(default)]
    pub avatarUrl: Option<String>,
    /// 仅 code == 803 时返回（登录 cookie 字符串）
    #[serde(default)]
    pub cookie: Option<String>,
    /// 部分版本返回 nickname（仅 803）
    #[serde(default)]
    pub nickname: Option<String>,
}

impl NeteaseQrCheckResponse {
    /// 状态枚举，便于 match。
    pub fn status(&self) -> QrCheckStatus {
        match self.code {
            800 => QrCheckStatus::Expired,
            801 => QrCheckStatus::Waiting,
            802 => QrCheckStatus::Scanned,
            803 => QrCheckStatus::Authorized,
            _ => QrCheckStatus::Unknown,
        }
    }
}

/// 扫码状态枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrCheckStatus {
    /// 二维码已过期
    Expired,
    /// 等待扫码
    Waiting,
    /// 已扫码，等待确认
    Scanned,
    /// 已授权（成功）
    Authorized,
    /// 未知状态码
    Unknown,
}

impl QrCheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expired => "expired",
            Self::Waiting => "waiting",
            Self::Scanned => "scanned",
            Self::Authorized => "authorized",
            Self::Unknown => "unknown",
        }
    }
}
