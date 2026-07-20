use crate::module::music_source::types::SourceId;
use serde::{Deserialize, Serialize};

/// 歌曲。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Song {
    /// 库内统一 ID（UUID）
    pub id: String,
    /// 歌曲标题
    pub title: String,
    /// 艺人名称（无序），与 `artist_ids` 一一对应
    pub artist_names: Vec<String>,
    /// 专辑名称，与 `album_id` 对应
    pub album_title: Option<String>,
    /// 时长（秒）
    pub duration: Option<u64>,
    /// 关联的艺术家 ID 列表
    pub artist_ids: Vec<String>,
    /// 关联的专辑 ID
    pub album_id: Option<String>,
    /// 关联的歌词 ID
    pub lyric_id: Option<String>,
    /// 来源引用 — 该歌曲在哪些来源中存在
    pub source_ids: Vec<SourceId>,
    /// 发行年份（来自音频标签 Year/Date）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    /// 封面图片 URL（网络来源直接提供，避免通过 chordial:// 协议二次拉取）
    ///
    /// 对于网络来源（如网易云），搜索/详情接口已直接返回 `picUrl`，
    /// 存到此字段供前端直接作为 `<img src>` 使用，避免：
    /// 1. chordial://image 协议触发 `/album?id=xxx` 二次 API 调用
    /// 2. song id 被当成 album id 导致 404
    /// 3. 大量并发请求触发 ncmapi 限流（405）
    ///
    /// 本地来源此字段为 None，前端回退到 chordial:// 协议从音频标签提取封面。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
}

/// 艺术家。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    /// 库内统一 ID（UUID）
    pub id: String,
    /// 艺术家名称
    pub name: String,
    /// 简介
    pub bio: Option<String>,
    /// 来源引用
    pub source_ids: Vec<SourceId>,
}

/// 专辑。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    /// 库内统一 ID（UUID）
    pub id: String,
    /// 专辑标题
    pub title: String,
    /// 所属艺术家 ID
    pub artist_id: String,
    /// 封面图片 URL
    pub cover_url: Option<String>,
    /// 包含的歌曲 ID 列表
    pub song_ids: Vec<String>,
    /// 来源引用
    pub source_ids: Vec<SourceId>,
    /// 专辑发行年份（从同名歌曲标签聚合得到）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
}

/// 歌词。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lyric {
    /// 库内统一 ID（UUID）
    pub id: String,
    /// 关联的歌曲 ID
    pub song_id: String,
    /// 歌词文本
    pub text: String,
    /// 来源引用（歌词通常只有一个来源）
    pub source_id: SourceId,
}
