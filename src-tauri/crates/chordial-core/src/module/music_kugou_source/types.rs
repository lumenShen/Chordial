//! 酷狗音乐 API 的类型定义。
//!
//! 字段命名严格遵循 KuGouMusicApi 上游响应（酷狗接口多为 PascalCase + 部分 snake_case 混用）。
//! 所有 `#[serde(default)]` 保证部分缺失字段不会导致反序列化失败。

#![allow(non_snake_case)]

use serde::Deserialize;

// ── 搜索响应 ──────────────────────────────────────────────────

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

// ── 歌曲 ─────────────────────────────────────────────────────

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

// ── 歌手 ─────────────────────────────────────────────────────

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

// ── 专辑 ─────────────────────────────────────────────────────

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

// ── 歌词 ─────────────────────────────────────────────────────

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

// ── 扫码登录 ─────────────────────────────────────────────────

/// `/login/qr/key` 响应。
///
/// KuGouMusicApi 调用 `https://login-user.kugou.com/v2/qrcode`，
/// 返回的 key 用于后续生成二维码和轮询扫码状态。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouQrKeyResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    #[serde(default)]
    pub data: KugouQrKeyData,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouQrKeyData {
    /// 二维码 key（用于 /login/qr/create 和 /login/qr/check）
    #[serde(default)]
    pub qrcode: String,
}

/// `/login/qr/create` 响应。
///
/// KuGouMusicApi 将 key 组装为 `https://h5.kugou.com/apps/loginQRCode/html/index.html?qrcode={key}`，
/// 然后可选生成 base64 图片。
///
/// 响应结构与酷狗其它端点一致：`status == 1` 表示成功，`err_code == 0` 表示无错误。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouQrCreateResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    #[serde(default)]
    pub data: KugouQrCreateData,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouQrCreateData {
    /// 二维码 URL（可用于前端 qrcode 库生成图片）
    #[serde(default, alias = "qrcode")]
    pub url: String,
    /// Base64 data URL（当请求带 qrimg=true 时返回）
    #[serde(default, alias = "qrimg")]
    pub base64: String,
}

/// `/login/qr/check` 响应。
///
/// KuGouMusicApi 调用 `https://login-user.kugou.com/v2/get_userinfo_qrcode`。
/// - status 0: 二维码已过期
/// - status 1: 等待扫码
/// - status 2: 已扫码，待确认
/// - status 4: 授权登录成功（返回 token + userid）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouQrCheckResponse {
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub err_code: i32,
    #[serde(default)]
    pub data: KugouQrCheckData,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct KugouQrCheckData {
    /// 扫码状态：0=过期, 1=等待, 2=已扫码, 4=授权成功
    #[serde(default)]
    pub status: i32,
    /// 授权成功时返回的 token
    #[serde(default)]
    pub token: String,
    /// 授权成功时返回的 userid
    #[serde(default)]
    pub userid: i64,
}

/// 酷狗扫码状态枚举（与网易云 QrCheckStatus 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KugouQrCheckStatus {
    Expired,
    Waiting,
    Scanned,
    Authorized,
    Unknown,
}

impl KugouQrCheckStatus {
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

impl From<i32> for KugouQrCheckStatus {
    fn from(status: i32) -> Self {
        match status {
            0 => Self::Expired,
            1 => Self::Waiting,
            2 => Self::Scanned,
            4 => Self::Authorized,
            _ => Self::Unknown,
        }
    }
}
