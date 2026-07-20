//! QQ 音乐 API 的类型定义。
//!
//! 字段命名严格遵循 QQMusicApi 上游响应（QQ 音乐接口多为 camelCase + 部分 snake_case 混用）。
//! 所有 `#[serde(default)]` 保证部分缺失字段不会导致反序列化失败。

#![allow(non_snake_case)]

use serde::Deserialize;

// ── 通用响应包装 ───────────────────────────────────────────────

/// QQMusicApi 通用响应包装。
///
/// 上游约定：`result == 100` 表示成功，其他为业务错误。
/// 部分接口（如 `/lyric`）在 raw 模式下返回原始字段，需调用方自行处理。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QqResponse<T> {
    #[serde(default)]
    pub result: i32,
    #[serde(default)]
    pub errMsg: Option<String>,
    #[serde(default)]
    pub data: T,
}

impl<T> QqResponse<T> {
    pub fn is_ok(&self) -> bool {
        self.result == 100
    }

    pub fn error_message(&self) -> String {
        self.errMsg
            .clone()
            .unwrap_or_else(|| format!("QQMusicApi 错误 result={}", self.result))
    }
}

// ── 搜索响应 ──────────────────────────────────────────────────

/// `/search` 响应数据（外层）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QqSearchData<T> {
    #[serde(default)]
    pub list: Vec<T>,
    #[serde(default)]
    pub pageNo: i32,
    #[serde(default)]
    pub pageSize: i32,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub t: i32,
    #[serde(default)]
    pub r#type: String,
}

// ── 歌曲 ─────────────────────────────────────────────────────

/// `/search?t=0` 返回的歌曲简略信息。
///
/// 字段对应 QQ 音乐搜索响应中 list 项的结构（部分字段在不同版本可能缺失）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QqSongBrief {
    /// 歌曲 mid（字符串，下载与歌词调用必选）
    #[serde(default)]
    pub songmid: String,
    /// 歌曲 id（数字）
    #[serde(default)]
    pub songid: i64,
    /// 歌曲名（可能含转义字符）
    #[serde(default)]
    pub songname: String,
    /// 歌手列表
    #[serde(default)]
    pub singer: Vec<QqSingerBrief>,
    /// 专辑信息
    #[serde(default)]
    pub albummid: String,
    /// 专辑名
    #[serde(default)]
    pub albumname: String,
    /// 专辑 id（数字）
    #[serde(default)]
    pub albumid: i64,
    /// 时长（秒）
    #[serde(default)]
    pub interval: i32,
    /// 文件大小（字节，128 kbps 版本）
    #[serde(default)]
    pub size128: i64,
    /// 文件大小（字节，320 kbps 版本）
    #[serde(default)]
    pub size320: i64,
    /// 文件大小（字节，flac 版本）
    #[serde(default)]
    pub sizeflac: i64,
    /// 是否 VIP 歌曲（1=VIP）
    #[serde(default)]
    pub isVip: i32,
}

impl QqSongBrief {
    /// 取主歌手名（多个歌手用 `/` 连接）
    pub fn artist_display(&self) -> String {
        if self.singer.is_empty() {
            "未知艺术家".to_string()
        } else {
            self.singer
                .iter()
                .map(|s| s.name.as_str())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("/")
        }
    }

    /// 取艺术家名列表（每个歌手一项）
    pub fn artist_names(&self) -> Vec<String> {
        if self.singer.is_empty() {
            vec!["未知艺术家".to_string()]
        } else {
            self.singer
                .iter()
                .map(|s| {
                    if s.name.is_empty() {
                        "未知艺术家".to_string()
                    } else {
                        s.name.clone()
                    }
                })
                .collect()
        }
    }
}

/// 歌手简略信息（歌曲列表内嵌）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QqSingerBrief {
    #[serde(default)]
    pub mid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub id: i64,
}

// ── 歌曲 URL ─────────────────────────────────────────────────

/// `/song/url` 响应数据。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QqSongUrlData {
    /// 完整可下载 URL（domain + purl）
    #[serde(default)]
    pub data: String,
}

// ── 歌手 ─────────────────────────────────────────────────────

/// `/search?t=9` 返回的歌手简略信息。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QqArtistBrief {
    /// 歌手 mid
    #[serde(default)]
    pub singerMID: String,
    /// 别名字段（部分响应使用小写）
    #[serde(default)]
    pub singer_mid: String,
    /// 歌手 id（数字）
    #[serde(default)]
    pub singerID: i64,
    /// 别名字段
    #[serde(default)]
    pub singer_id: i64,
    /// 歌手名
    #[serde(default)]
    pub singerName: String,
    /// 别名字段
    #[serde(default)]
    pub singer_name: String,
    /// 歌曲数
    #[serde(default)]
    pub songNum: i32,
    /// 专辑数
    #[serde(default)]
    pub albumNum: i32,
    /// 头像 URL
    #[serde(default)]
    pub img: String,
    /// 别名字段
    #[serde(default)]
    pub pic: String,
}

impl QqArtistBrief {
    pub fn singer_mid(&self) -> &str {
        if !self.singerMID.is_empty() {
            &self.singerMID
        } else {
            &self.singer_mid
        }
    }

    pub fn singer_name(&self) -> &str {
        if !self.singerName.is_empty() {
            &self.singerName
        } else {
            &self.singer_name
        }
    }

    pub fn image_url(&self) -> &str {
        if !self.img.is_empty() {
            &self.img
        } else {
            &self.pic
        }
    }
}

// ── 专辑 ─────────────────────────────────────────────────────

/// `/search?t=8` 返回的专辑简略信息。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QqAlbumBrief {
    /// 专辑 mid
    #[serde(default)]
    pub albumMID: String,
    /// 别名字段
    #[serde(default)]
    pub album_mid: String,
    /// 专辑 id
    #[serde(default)]
    pub albumID: i64,
    /// 别名字段
    #[serde(default)]
    pub album_id: i64,
    /// 专辑名
    #[serde(default)]
    pub albumName: String,
    /// 别名字段
    #[serde(default)]
    pub album_name: String,
    /// 歌手 mid
    #[serde(default)]
    pub singerMID: String,
    /// 歌手名
    #[serde(default)]
    pub singerName: String,
    /// 发行时间（字符串形式）
    #[serde(default)]
    pub publicTime: String,
    /// 封面 URL（部分响应不含）
    #[serde(default)]
    pub img: String,
    /// 别名字段
    #[serde(default)]
    pub pic: String,
}

impl QqAlbumBrief {
    pub fn album_mid(&self) -> &str {
        if !self.albumMID.is_empty() {
            &self.albumMID
        } else {
            &self.album_mid
        }
    }

    pub fn album_name(&self) -> &str {
        if !self.albumName.is_empty() {
            &self.albumName
        } else {
            &self.album_name
        }
    }

    pub fn image_url(&self) -> &str {
        if !self.img.is_empty() {
            &self.img
        } else {
            &self.pic
        }
    }
}

/// `/album?albummid=<mid>` 响应的专辑详情。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QqAlbumInfo {
    #[serde(default)]
    pub mid: String,
    /// 别名字段（部分响应使用 albumMid）
    #[serde(default)]
    pub albumMid: String,
    #[serde(default)]
    pub name: String,
    /// 别名字段
    #[serde(default)]
    pub albumName: String,
    /// 歌手列表
    #[serde(default)]
    pub ar: Vec<QqSingerBrief>,
    /// 别名字段（部分响应使用 singer）
    #[serde(default)]
    pub singer: Vec<QqSingerBrief>,
    /// 发行时间（秒级时间戳或字符串）
    #[serde(default)]
    pub publishTime: i64,
    /// 别名字段
    #[serde(default)]
    pub ctime: i64,
    /// 简介
    #[serde(default)]
    pub intro: String,
    /// 语言
    #[serde(default)]
    pub language: String,
    /// 曲目数
    #[serde(default)]
    pub songCount: i32,
}

impl QqAlbumInfo {
    pub fn mid(&self) -> &str {
        if !self.mid.is_empty() {
            &self.mid
        } else {
            &self.albumMid
        }
    }

    pub fn name(&self) -> &str {
        if !self.name.is_empty() {
            &self.name
        } else {
            &self.albumName
        }
    }

    pub fn singers(&self) -> &[QqSingerBrief] {
        if !self.ar.is_empty() {
            &self.ar
        } else {
            &self.singer
        }
    }

    pub fn publish_time_secs(&self) -> i64 {
        if self.publishTime > 0 {
            self.publishTime
        } else {
            self.ctime
        }
    }
}

// ── 歌词 ─────────────────────────────────────────────────────

/// `/lyric?songmid=<mid>` 响应数据（已 base64 解码为字符串）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QqLyricResponse {
    /// 原始 LRC 文本（已解码）
    #[serde(default)]
    pub lyric: String,
    /// 翻译 LRC 文本（已解码，可能为空）
    #[serde(default)]
    pub trans: String,
}
