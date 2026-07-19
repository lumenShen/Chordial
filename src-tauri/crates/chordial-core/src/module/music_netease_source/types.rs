//! NeteaseCloudMusicApi 返回的数据模型。
//!
//! 这些结构体严格对应 NeteaseCloudMusicApi 的 JSON 响应格式，
//! 用 `#[serde(default)]` 容忍字段缺失（API 不同版本字段可能略有差异）。
//!
//! 注意：JSON 字段保持 camelCase（与 API 一致），所以模块整体允许 non_snake_case。

#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
// 重导出统一的音质类型 —— 网易云的 `AudioQuality` 与 `music_source::AudioQuality` 是同一个类型，
// 历史代码通过 `music_netease_source::AudioQuality` 引用，保留以维持向后兼容。
pub use crate::module::music_source::types::{AudioCodec, AudioQuality, AudioQualityDescriptor};

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

/// 搜索结果（/search 接口 `result` 字段）。
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

/// `/search` 接口响应包装。
///
/// 注意：网易云搜索接口把数据嵌套在 `result` 字段下（与其他接口直接平铺不同），
/// 因此不能复用 `NeteaseResponse<T>` 的 `flatten` 模式，需要单独定义。
///
/// ```json
/// { "code": 200, "result": { "songs": [...], "playlists": [...] } }
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct NeteaseSearchResponse {
    pub code: i32,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub result: NeteaseSearchResult,
}

impl NeteaseSearchResponse {
    pub fn is_ok(&self) -> bool {
        self.code == 200
    }

    pub fn error_message(&self) -> String {
        self.message
            .clone()
            .or_else(|| self.msg.clone())
            .unwrap_or_else(|| format!("NeteaseCloudMusicApi 错误 code={}", self.code))
    }
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
    /// 是否被当前用户收藏（仅 /user/playlist 接口返回，搜索接口不带）。
    /// `true` = 收藏的歌单；`false` = 自己创建的歌单。
    #[serde(default)]
    pub subscribed: Option<bool>,
    /// 歌单标签（/user/playlist 接口返回）。
    #[serde(default)]
    pub tags: Vec<String>,
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

impl NeteaseSongUrlData {
    /// 从网易云返回的 URL 数据构造音质描述符。
    ///
    /// - `level` 字段决定等级 rank
    /// - `r#type` 字段（"flac" / "mp3" / "m4a"）决定编码格式
    /// - `br` 字段为真实码率（bps → kbps）
    pub fn to_quality_descriptor(&self) -> AudioQualityDescriptor {
        let quality = self
            .level
            .as_deref()
            .map(AudioQuality::from_level_str)
            .unwrap_or(AudioQuality::Unknown);

        let codec = self
            .r#type
            .as_deref()
            .map(AudioCodec::from_extension)
            .unwrap_or(quality.typical_codec());

        // 网易云 br 单位为 bps，转换为 kbps
        let bitrate_kbps = if self.br > 0 {
            (self.br / 1000) as u32
        } else {
            quality.typical_bitrate_kbps()
        };

        // 无损格式从 level 推断采样率/位深
        let (sample_rate_hz, bit_depth) = match quality {
            AudioQuality::Lossless => (Some(44_100u32), Some(16u8)),
            AudioQuality::Hires => (Some(96_000u32), Some(24u8)),
            _ => (None, None),
        };

        AudioQualityDescriptor {
            rank: quality as u8,
            bitrate_kbps,
            codec,
            sample_rate_hz,
            bit_depth,
        }
    }
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

// ───────────────────────────────────────────────────────────────────────────
// 用户账户与"我的资源"响应（/user/account、/user/playlist、/album/sublist）
// ───────────────────────────────────────────────────────────────────────────

/// `/user/account` 响应（需要登录 cookie）。
///
/// ```json
/// { "code": 200, "account": {...}, "profile": { "userId": 123, "nickname": "...", "avatarUrl": "..." } }
/// ```
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseUserAccountResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub profile: Option<NeteaseUserProfile>,
    #[serde(default)]
    pub account: Option<NeteaseUserAccount>,
}

/// 用户公开资料（`/user/account` 的 `profile` 字段）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteaseUserProfile {
    #[serde(default)]
    pub userId: i64,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub avatarUrl: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
}

/// 账户字段（`/user/account` 的 `account` 字段，含登录账号信息）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteaseUserAccount {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub userName: String,
}

/// `/user/playlist?uid=xxx` 响应。
///
/// 注意：网易云在这里把数组放在 `playlist`（单数）字段下。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseUserPlaylistsResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub playlist: Vec<NeteasePlaylistBrief>,
    #[serde(default)]
    pub more: Option<bool>,
}

/// `/album/sublist` 响应（收藏的专辑）。
///
/// ```json
/// { "code": 200, "data": [...], "count": 100, "hasMore": true }
/// ```
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseAlbumSublistResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub data: Vec<NeteaseSubAlbum>,
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub hasMore: Option<bool>,
}

/// 收藏的专辑条目（`/album/sublist` 的 `data[i]`）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteaseSubAlbum {
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
    pub trackCount: i32,
    #[serde(default)]
    pub playCount: i64,
    #[serde(default)]
    pub subTime: i64,
}

/// `/album?id=xxx` 响应（专辑详情，含曲目）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NeteaseAlbumDetailResponse {
    #[serde(default)]
    pub code: i32,
    #[serde(default)]
    pub album: Option<NeteaseAlbumDetail>,
    #[serde(default)]
    pub songs: Vec<NeteaseSongDetail>,
}

/// 专辑详情（`/album` 的 `album` 字段）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NeteaseAlbumDetail {
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
    pub description: Option<String>,
    #[serde(default)]
    pub publishTime: i64,
    #[serde(default)]
    pub size: i32,
}
