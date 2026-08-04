//! 歌手相关内容类型：搜索简略信息、详情。

#![allow(non_snake_case)]

use serde::Deserialize;

/// `/search?type=author` 返回的歌手简略信息。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouArtistBrief {
    /// 歌手 ID
    #[serde(default)]
    pub SingerId: i64,
    /// 别名字段（部分响应使用小写）
    #[serde(default)]
    pub singerid: i64,
    /// 歌手名
    #[serde(default)]
    pub SingerName: String,
    /// 别名字段
    #[serde(default)]
    pub singername: String,
    /// 歌曲数
    #[serde(default)]
    pub SongNum: i64,
    /// 专辑数
    #[serde(default)]
    pub AlbumNum: i64,
    /// 头像 URL
    #[serde(default)]
    pub Image: String,
    /// 别名字段
    #[serde(default)]
    pub imgurl: String,
}

impl KugouArtistBrief {
    /// 取歌手 ID
    pub fn singer_id(&self) -> i64 {
        if self.SingerId != 0 {
            self.SingerId
        } else {
            self.singerid
        }
    }

    /// 取歌手名
    pub fn singer_name(&self) -> &str {
        if !self.SingerName.is_empty() {
            &self.SingerName
        } else {
            &self.singername
        }
    }

    /// 取头像 URL
    pub fn image_url(&self) -> &str {
        if !self.Image.is_empty() {
            &self.Image
        } else {
            &self.imgurl
        }
    }
}

/// `/artist/detail?id=xxx` 响应。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouArtistDetailResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    #[serde(default)]
    pub data: KugouArtistDetail,
}

/// `/artist/detail` 响应的歌手详情（含简介、粉丝数等）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouArtistDetail {
    #[serde(default)]
    pub singer_id: i64,
    #[serde(default)]
    pub singer_name: String,
    /// 别名字段（部分响应使用小写）
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub intro: String,
    /// 别名字段
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub img: String,
    /// 别名字段
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub song_count: i64,
    /// 别名字段
    #[serde(default)]
    pub songs: i64,
    #[serde(default)]
    pub album_count: i64,
    /// 别名字段
    #[serde(default)]
    pub albums: i64,
    #[serde(default)]
    pub fans: i64,
}

impl KugouArtistDetail {
    /// 取歌手名（兼容 singer_name 和 name）
    pub fn name(&self) -> &str {
        if !self.singer_name.is_empty() {
            &self.singer_name
        } else {
            &self.name
        }
    }

    /// 取简介（兼容 intro 和 brief）
    pub fn intro(&self) -> &str {
        if !self.intro.is_empty() {
            &self.intro
        } else {
            &self.brief
        }
    }

    /// 取头像 URL（兼容 img 和 image）
    pub fn image_url(&self) -> &str {
        if !self.img.is_empty() {
            &self.img
        } else {
            &self.image
        }
    }
}