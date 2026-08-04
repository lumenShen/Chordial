//! 网易云扫码登录流程。
//!
//! 三阶段扫码登录：
//! 1. [`login_qr_key`] 获取扫码登录的 unikey；
//! 2. [`login_qr_create`] 用 unikey 生成二维码；
//! 3. [`login_qr_check`] 轮询扫码状态，授权成功后自动将 cookie 写回配置。

use super::api;
use super::cookie::parse_set_cookie_to_header;
use super::types::{QrCheckStatus, NeteaseQrCheckResponse, NeteaseQrCreateData};

impl super::NeteaseCloudMusicApiSource {
    /// 第 1 步：获取扫码登录的 unikey。
    pub fn login_qr_key(&self) -> Result<String, String> {
        api::login_qr_key(&self.client)
    }

    /// 第 2 步：用 unikey 生成二维码。
    pub fn login_qr_create(&self, key: &str, qrimg: bool) -> Result<NeteaseQrCreateData, String> {
        api::login_qr_create(&self.client, key, qrimg)
    }

    /// 第 3 步：轮询扫码状态。
    ///
    /// **副作用**：当返回 `code == 803`（授权成功）时，自动将响应中的 cookie
    /// 写回当前配置（持久化）并更新 `WebHttpClient` 的 cookie，调用方无需再手动保存。
    pub fn login_qr_check(&self, key: &str) -> Result<NeteaseQrCheckResponse, String> {
        let resp = api::login_qr_check(&self.client, key)?;
        if resp.status() == QrCheckStatus::Authorized {
            if let Some(raw_cookie) = resp.cookie.clone() {
                if !raw_cookie.is_empty() {
                    // ncmapi 返回的 cookie 字段是多个 Set-Cookie 用 ", " 连接的字符串
                    // 每条格式：NAME=VALUE; Path=/; HttpOnly; Expires=Day, DD Mon YYYY HH:MM:SS GMT
                    // 直接当作 Cookie 请求头发送时，ncmapi 会把 Path/Expires/Max-Age 等属性名
                    // 当成 cookie 名解析，真正的 `MUSIC_U` 被属性淹没，导致登录态丢失（profile=null）
                    // 此处解析出 NAME=VALUE 对，用 "; " 连接成标准 Cookie 头格式
                    let cookie = parse_set_cookie_to_header(&raw_cookie);
                    eprintln!(
                        "[netease] 扫码登录成功，原始 cookie 长度={}，解析后长度={}，含 MUSIC_U={}",
                        raw_cookie.len(),
                        cookie.len(),
                        cookie.contains("MUSIC_U=")
                    );
                    // 写入运行时 client（立即生效）
                    self.client.set_cookie(cookie.clone());
                    // 持久化到 config
                    let mut cfg = self.config.write().clone();
                    cfg.cookie = cookie;
                    // update_config 会落盘并刷新 client
                    if let Err(e) = self.update_config(cfg) {
                        // 持久化失败不影响返回 cookie，仅 stderr 记录
                        eprintln!("[netease] 扫码登录 cookie 持久化失败: {}", e);
                    }
                }
            }
        }
        Ok(resp)
    }
}