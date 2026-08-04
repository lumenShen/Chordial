//! 扫码登录相关内容类型：key、二维码生成、状态检查。

#![allow(non_snake_case)]

use serde::Deserialize;

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