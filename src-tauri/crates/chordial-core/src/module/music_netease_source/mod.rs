//! 网易云音乐来源 — [`NeteaseCloudMusicApiSource`] 是 [`MusicSource`] 的具体实现。
//!
//! 基于 [`NeteaseCloudMusicApi`](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced)
//! 项目提供的 HTTP API，通过自建/公共 API 服务器访问网易云音乐资源。
//!
//! # 设计要点
//!
//! - **来源名**：`"netease"`（固定常量 [`NETEASE_SOURCE_NAME`]）
//! - **entity_id 编码**：`"<kind>:<id>[:<quality>]"`，如 `"song:123456"`、`"album:789"`、
//!   `"lyric:123456:yrc"`。多字段冒号分隔，便于在不反查 API 的情况下还原结构。
//! - **音频 URL 解析**：调用 `/song/url/v1` 获取真实 CDN URL，再通过 `WebHttpClient::get_bytes`
//!   下载字节。NeteaseCloudMusicApi 的 `song/url/v1` 返回 JSON 中 `data[0].url` 即为可下载 URL。
//! - **歌词优先级**：调用 `/lyric` 接口一次返回 `lrc`（普通 LRC）、`tlyric`（翻译）、
//!   `romalrc`（音译）、`yrc`（逐字 YRC）四种歌词。本实现优先返回 `yrc`（逐字），
//!   退而求其次返回 `lrc`。具体优先级由 [`crate::module::lyric_preference`] 控制前端使用哪个。
//! - **音质标识**：NeteaseCloudMusicApi 的 `level` 参数（`standard` / `higher` / `exhigh` /
//!   `lossless` / `hires`）映射为内部 [`AudioQuality`] 枚举，供 [`crate::module::music_source`]
//!   缓存机制比较本地/在线音质。
//!
//! # 接口路径
//!
//! - `GET /search`：搜索歌曲/歌单/专辑/歌手
//! - `GET /song/detail`：获取歌曲详情
//! - `GET /song/url/v1`：获取歌曲可下载 URL
//! - `GET /album`：获取专辑详情（含封面 URL）
//! - `GET /artist/detail`：获取歌手详情
//! - `GET /lyric`：获取歌词（含 YRC/LRC/翻译/音译）
//! - `GET /playlist/detail`：获取歌单详情（含所有歌曲 ID）
//! - `GET /playlist/track/all`：分页获取歌单曲目

pub mod api;
pub mod cache;
pub mod cookie;
pub mod detail;
pub mod download;
pub mod entity_id;
pub mod login;
pub mod lyric;
pub mod mapper;
pub mod playlist;
pub mod search;
pub mod source;
pub mod time;
pub mod types;
pub mod user;

pub use source::{NeteaseCloudMusicApiSource, NETEASE_SOURCE_NAME};
pub use types::{
    AudioCodec, AudioQuality, AudioQualityDescriptor, NeteaseAlbumDetail,
    NeteasePlaylistBrief, NeteasePlaylistDetail, NeteaseSongDetail, NeteaseSubAlbum,
    NeteaseUserAccountResponse, NeteaseUserProfile,
};
