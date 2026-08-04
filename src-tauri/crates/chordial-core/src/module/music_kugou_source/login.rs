//! 酷狗扫码登录流程。
//!
//! 三阶段扫码登录：
//! 1. [`login_qr_key`] 获取扫码登录的 key；
//! 2. [`login_qr_create`] 用 key 生成二维码；
//! 3. [`login_qr_check`] 轮询扫码状态，授权成功后自动将 token + userid 写回配置。

use super::api;
use super::types::KugouQrCheckStatus;

impl super::KugouMusicApiSource {
    /// 第 1 步：获取扫码登录的 key。
    pub fn login_qr_key(&self) -> Result<String, String> {
        api::login_qr_key(&self.client)
    }

    /// 第 2 步：用 key 生成二维码。
    pub fn login_qr_create(
        &self,
        key: &str,
        qrimg: bool,
    ) -> Result<super::types::KugouQrCreateData, String> {
        api::login_qr_create(&self.client, key, qrimg)
    }

    /// 第 3 步：轮询扫码状态。
    ///
    /// **副作用**：当 `data.status == 4`（授权成功）时，自动将 token + userid
    /// 组装为 cookie 写回配置（持久化）并更新 WebHttpClient，调用方无需再手动保存。
    pub fn login_qr_check(
        &self,
        key: &str,
    ) -> Result<super::types::KugouQrCheckResponse, String> {
        let resp = api::login_qr_check(&self.client, key)?;
        let status = KugouQrCheckStatus::from(resp.data.status);
        if status == KugouQrCheckStatus::Authorized {
            let token = resp.data.token.clone();
            let userid = resp.data.userid;
            if !token.is_empty() {
                // 组装 cookie：token=xxx; userid=yyy
                // 保留已有 cookie 中的其他字段（如 dfid/mid），追加/更新 token 和 userid
                let mut cookie_parts: Vec<String> = Vec::new();
                let existing = self.config.read().cookie.clone();
                if !existing.is_empty() {
                    // 保留非 token/userid 的已有 cookie 字段
                    for part in existing.split(';') {
                        let trimmed = part.trim();
                        if trimmed.is_empty() { continue; }
                        let key = trimmed.split('=').next().unwrap_or("").trim();
                        if key != "token" && key != "userid" {
                            cookie_parts.push(trimmed.to_string());
                        }
                    }
                }
                cookie_parts.push(format!("token={}", token));
                cookie_parts.push(format!("userid={}", userid));
                let cookie = cookie_parts.join("; ");

                eprintln!(
                    "[kugou] 扫码登录成功，userid={}，cookie 长度={}",
                    userid, cookie.len()
                );
                // 写入运行时 client
                self.client.set_cookie(cookie.clone());
                // 持久化到 config
                let mut cfg = self.config.write().clone();
                cfg.cookie = cookie;
                if let Err(e) = self.update_config(cfg) {
                    eprintln!("[kugou] 扫码登录 cookie 持久化失败: {}", e);
                }
            }
        }
        Ok(resp)
    }
}