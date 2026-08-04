//! 歌词相关内容类型：歌词搜索候选、歌词下载。

#![allow(non_snake_case)]

use serde::Deserialize;

/// `/search/lyric?hash=xxx` 响应。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouLyricSearchResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    #[serde(default)]
    pub candidates: Vec<KugouLyricCandidate>,
    #[serde(default)]
    pub candidates_count: i32,
}

/// `/search/lyric` 候选项。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouLyricCandidate {
    /// 歌词 ID（调用 /lyric 必选）
    #[serde(default)]
    pub id: String,
    /// accesskey（调用 /lyric 必选）
    #[serde(default)]
    pub accesskey: String,
    /// 时长（毫秒）
    #[serde(default)]
    pub duration: i64,
    /// 歌曲名
    #[serde(default)]
    pub song: String,
    /// 歌手名
    #[serde(default)]
    pub singer: String,
    /// 歌词格式（"krc"/"lrc"）
    #[serde(default)]
    pub krctype: String,
}

/// `/lyric?id=xxx&accesskey=xxx&fmt=lrc&decode=true` 响应。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouLyricResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    /// base64 编码的歌词内容
    #[serde(default)]
    pub content: String,
    /// 0=KRC, 1=LRC
    #[serde(default)]
    pub contenttype: i32,
    /// 请求的格式
    #[serde(default)]
    pub fmt: String,
    /// 编码
    #[serde(default)]
    pub charset: String,
    /// 解码后的纯文本歌词（仅 decode=true 时存在）
    #[serde(default)]
    pub decodeContent: String,
}