//! 专辑相关内容类型：搜索简略信息、基础信息、详情、歌曲列表。

#![allow(non_snake_case)]

use serde::Deserialize;

use super::song::KugouSongBrief;

/// `/search?type=album` 返回的专辑简略信息。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouAlbumBrief {
    /// 专辑 ID（字符串）
    #[serde(default)]
    pub AlbumID: String,
    /// 别名字段（小写）
    #[serde(default)]
    pub albumid: String,
    /// 专辑名
    #[serde(default)]
    pub AlbumName: String,
    /// 别名字段
    #[serde(default)]
    pub albumname: String,
    /// 歌手名
    #[serde(default)]
    pub SingerName: String,
    /// 歌手 ID
    #[serde(default)]
    pub SingerId: i64,
    /// 歌曲数
    #[serde(default)]
    pub SongNum: i64,
    /// 发行时间（秒级时间戳或字符串）
    #[serde(default)]
    pub PublishTime: String,
    /// 封面 URL
    #[serde(default)]
    pub Image: String,
    /// 别名字段
    #[serde(default)]
    pub pic: String,
}

impl KugouAlbumBrief {
    /// 取专辑 ID
    pub fn album_id(&self) -> &str {
        if !self.AlbumID.is_empty() {
            &self.AlbumID
        } else {
            &self.albumid
        }
    }

    /// 取专辑名
    pub fn album_name(&self) -> &str {
        if !self.AlbumName.is_empty() {
            &self.AlbumName
        } else {
            &self.albumname
        }
    }

    /// 取封面 URL
    pub fn image_url(&self) -> &str {
        if !self.Image.is_empty() {
            &self.Image
        } else {
            &self.pic
        }
    }
}

/// `/album?album_id=xxx` 响应。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouAlbumInfoResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    #[serde(default)]
    pub data: Vec<KugouAlbumInfo>,
}

/// `/album` 响应的专辑详情。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouAlbumInfo {
    #[serde(default)]
    pub album_id: String,
    #[serde(default)]
    pub album_name: String,
    #[serde(default)]
    pub author_name: String,
    #[serde(default)]
    pub publish_date: String,
    /// 封面 URL（推荐使用此字段，可调整尺寸）
    #[serde(default)]
    pub sizable_cover: String,
    /// 原始封面 URL
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub intro: String,
    #[serde(default)]
    pub language: String,
}

/// `/album/detail?album_id=xxx` 响应（含 singer_id 等更完整字段）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouAlbumDetailResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    #[serde(default)]
    pub data: KugouAlbumDetail,
}

/// `/album/detail` 响应的专辑详情（含 singer_id，便于填充 Album.artist_id）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouAlbumDetail {
    #[serde(default)]
    pub album_id: String,
    #[serde(default)]
    pub album_name: String,
    #[serde(default)]
    pub singer_id: i64,
    #[serde(default)]
    pub singer_name: String,
    #[serde(default)]
    pub publish_date: String,
    #[serde(default)]
    pub sizable_cover: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub intro: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub song_count: i64,
}

/// `/album/songs?album_id=xxx&page=1&pagesize=100` 响应。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouAlbumSongsResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    #[serde(default)]
    pub data: KugouAlbumSongsData,
}

/// `/album/songs` 响应数据。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouAlbumSongsData {
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub list: Vec<KugouSongBrief>,
}