//! 酷狗音乐来源 — [`KugouMusicApiSource`] 是 [`MusicSource`] 的具体实现。
//!
//! 基于 [KuGouMusicApi](https://github.com/MakcRe/KuGouMusicApi) 项目提供的 HTTP API，
//! 通过自建/公共 API 服务器访问酷狗音乐资源。
//!
//! # 设计要点
//!
//! - **来源名**：`"kugou"`（固定常量 [`KUGOU_SOURCE_NAME`]）
//! - **entity_id 编码**：
//!   - 歌曲：`"song:<hash>"`（FileHash 是 32 位小写十六进制）
//!   - 专辑：`"album:<album_id>"`（字符串形式，部分为 0 表示无专辑）
//!   - 歌手：`"artist:<singer_id>"`
//!   - 歌词：`"lyric:<id>:<accesskey>"`（搜索 lyric 时获得的 id + accesskey 复合键）
//! - **音频 URL 解析**：调用 `/song/url` 获取真实 CDN URL，再用流式下载落盘
//! - **歌词流程**：先调 `/search/lyric?hash=<hash>` 获得 `id + accesskey`，
//!   再调 `/lyric?id=<id>&accesskey=<key>&fmt=lrc&decode=true` 获取解码后的 LRC 文本
//! - **音质标识**：酷狗 `quality` 参数（`128` / `320` / `flac` / `high` / `super`）映射为内部 [`AudioQuality`]
//!
//! # 接口路径
//!
//! - `GET /search?type=song|album|author|special`：搜索歌曲/专辑/歌手/歌单
//! - `GET /song/url?hash=<hash>&quality=<quality>`：获取歌曲可下载 URL
//! - `GET /search/lyric?hash=<hash>`：搜索歌词候选
//! - `GET /lyric?id=<id>&accesskey=<key>&fmt=lrc&decode=true`：下载歌词
//! - `POST /album?album_id=<id>`：获取专辑信息（含 `sizable_cover` 封面 URL）

pub mod api;
pub mod entity_id;
pub mod login;
pub mod quality;
pub mod source;
pub mod types;

pub use source::{KugouMusicApiSource, KUGOU_SOURCE_NAME};
pub use types::{
    KugouAlbumBrief, KugouArtistBrief, KugouLyricResponse, KugouSongBrief, KugouSongUrlData,
};
