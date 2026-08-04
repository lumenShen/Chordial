//! 歌曲相关内容类型：搜索响应、歌曲简略信息、URL 解析。

#![allow(non_snake_case)]

use serde::Deserialize;

/// `/search?type=song` 响应（外层）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouSearchResponse<T> {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub error_code: i32,
    #[serde(default)]
    pub data: KugouSearchData<T>,
}

/// 搜索响应的 `data` 字段。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouSearchData<T> {
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub lists: Vec<T>,
}

/// `/search?type=song` 返回的歌曲简略信息。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouSongBrief {
    /// 酷狗音频 ID
    #[serde(default)]
    pub AudioId: i64,
    /// 别名字段（部分响应使用小写）
    #[serde(default)]
    pub Audioid: i64,
    /// 文件 hash（32 位十六进制），下载必选参数
    #[serde(default)]
    pub FileHash: String,
    /// 高品质 hash
    #[serde(default)]
    pub HQFileHash: String,
    /// 无损 hash
    #[serde(default)]
    pub SQFileHash: String,
    /// 歌曲名
    #[serde(default)]
    pub SongName: String,
    /// 歌手名
    #[serde(default)]
    pub SingerName: String,
    /// 歌手 ID
    #[serde(default)]
    pub SingerId: i64,
    /// 专辑名
    #[serde(default)]
    pub AlbumName: String,
    /// 专辑 ID（字符串形式）
    #[serde(default)]
    pub AlbumID: String,
    /// 时长（秒）
    #[serde(default)]
    pub SongTime: i64,
    /// 文件大小（字节）
    #[serde(default)]
    pub FileSize: i64,
    /// 码率（字符串："128"/"320"）
    #[serde(default)]
    pub BitRate: String,
    /// MV hash
    #[serde(default)]
    pub MVHash: String,
}

impl KugouSongBrief {
    /// 取音频 ID（兼容 AudioId 和 Audioid）
    pub fn audio_id(&self) -> i64 {
        if self.AudioId != 0 {
            self.AudioId
        } else {
            self.Audioid
        }
    }

    /// 取主 FileHash（小写化，便于 entity_id 拼接）
    pub fn file_hash_lower(&self) -> String {
        self.FileHash.to_lowercase()
    }
}

/// `/song/url` 响应。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouSongUrlResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    #[serde(default)]
    pub data: KugouSongUrlData,
}

/// `/song/url` 响应的 `data` 字段。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouSongUrlData {
    /// 歌曲 hash
    #[serde(default)]
    pub hash: String,
    /// URL 列表（酷狗返回数组，通常只有一个）
    #[serde(default)]
    pub url: Vec<KugouSongUrl>,
}

/// 单个 URL 条目。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouSongUrl {
    /// 可下载 URL
    #[serde(default)]
    pub url: String,
    /// hash
    #[serde(default)]
    pub hash: String,
    /// 音质（"128"/"320"/"flac"/"high" 等）
    #[serde(default)]
    pub quality: String,
    /// 文件大小
    #[serde(default)]
    pub fileSize: i64,
    /// 码率（数字）
    #[serde(default)]
    pub bitRate: i64,
    /// 时长（秒）
    #[serde(default)]
    pub duration: i64,
    /// 文件类型（"mp3"/"flac"）
    #[serde(default)]
    pub r#type: String,
    /// 文件名
    #[serde(default)]
    pub name: String,
}