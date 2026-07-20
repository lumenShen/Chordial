//! QQ 音乐来源 — [`QqMusicApiSource`] 是 [`MusicSource`] 的具体实现。
//!
//! 基于 [QQMusicApi](https://github.com/jsososo/QQMusicApi) 项目提供的 HTTP API，
//! 通过自建/公共 API 服务器访问 QQ 音乐资源。
//!
//! # 设计要点
//!
//! - **来源名**：`"qq"`（固定常量 [`QQ_SOURCE_NAME`]）
//! - **entity_id 编码**：
//!   - 歌曲：`"song:<songmid>"`（songmid 是 QQ 音乐内部字符串标识）
//!   - 专辑：`"album:<albummid>"`
//!   - 歌手：`"artist:<singermid>"`
//!   - 歌词：`"lyric:<songmid>"`（QQ 音乐歌词直接通过 songmid 拉取）
//! - **音频 URL 解析**：调用 `/song/url?id=<songmid>&type=<type>` 获取可下载 URL
//! - **歌词流程**：直接调用 `/lyric?songmid=<songmid>` 获取 base64 编码的 LRC 文本
//! - **音质标识**：QQ 音质字符串（`128` / `320` / `m4a` / `ape` / `flac`）映射为内部 [`AudioQuality`]
//!
//! # 接口路径
//!
//! - `GET /search?key=<key>&t=<0|2|7|8|9|12>&pageNo=<n>&pageSize=<n>`：搜索歌曲/歌单/歌词/专辑/歌手/MV
//!   - t=0 单曲, t=2 歌单, t=7 歌词, t=8 专辑, t=9 歌手, t=12 MV
//! - `GET /song?songmid=<songmid>`：获取歌曲详情
//! - `GET /song/url?id=<songmid>&type=<128|320|m4a|ape|flac>`：获取歌曲可下载 URL
//! - `GET /lyric?songmid=<songmid>`：获取歌词（base64 编码的 LRC + 翻译）
//! - `GET /album?albummid=<albummid>`：获取专辑信息
//! - `GET /album/songs?albummid=<albummid>`：获取专辑内歌曲列表
//! - `GET /singer/desc?singermid=<singermid>`：获取歌手介绍
//! - `GET /singer/songs?singermid=<singermid>&num=<n>&page=<n>`：获取歌手热门歌曲
//! - `GET /singer/album?singermid=<singermid>&pageNo=<n>&pageSize=<n>`：获取歌手专辑列表

pub mod api;
pub mod source;
pub mod types;

pub use source::{QqMusicApiSource, QQ_SOURCE_NAME};
pub use types::{
    QqAlbumBrief, QqAlbumInfo, QqArtistBrief, QqLyricResponse, QqSongBrief, QqSongUrlData,
};
