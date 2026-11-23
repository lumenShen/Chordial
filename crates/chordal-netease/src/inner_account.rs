//! 网易云源内部状态：inner_account（由 lib.rs 拆分，行为不变）。

use super::*;

impl Inner {

    // ---------- 账号 / 集合 ----------

    pub(super) fn account_state(&self) -> AccountState {
        self.session
            .read()
            .map(|s| s.clone())
            .unwrap_or(AccountState::Unknown)
    }

    pub(super) fn set_account_state(&self, state: AccountState) {
        let changed = {
            let Ok(current) = self.session.read() else {
                return;
            };
            match (&*current, &state) {
                (AccountState::LoggedIn(a), AccountState::LoggedIn(b)) => a.uid != b.uid,
                (a, b) => a != b,
            }
        };
        if let Ok(mut current) = self.session.write() {
            *current = state;
        }
        if changed {
            self.session_epoch.fetch_add(1, Ordering::Release);
        }
    }

    pub(super) fn account_key(&self, uid: &str, kind: &str, id: &str) -> String {
        format!("{}/account/{}/{}/{}", self.source_id, uid, kind, id)
    }

    pub(super) async fn purge_account_cache(&self) {
        if let Some(cache) = &self.cache {
            let account_prefix = format!("{}/account/", self.source_id);
            let avatar_prefix = format!("{}-account-", self.source_id);
            let playlist_prefix = format!("{}/collection/Playlist/", self.source_id);
            let cache = cache.clone();
            let _ = shared_runtime()
                .spawn_blocking(move || {
                    cache.purge_prefix(&account_prefix);
                    cache.purge_prefix(&avatar_prefix);
                    cache.purge_prefix(&playlist_prefix);
                })
                .await;
        }
    }

    pub(super) async fn fetch_account_profile(&self) -> CoreResult<Option<AccountProfile>> {
        let body = self
            .client
            .get_json_with_codes("/user/account", &[], &[200, 301])
            .await?;
        Ok(mapping::map_account(&self.source_id, &body))
    }

    pub(super) async fn refresh_account_inner(&self) -> CoreResult<Option<AccountProfile>> {
        if self.client.cookie().is_none() {
            self.set_account_state(AccountState::LoggedOut);
            self.purge_account_cache().await;
            return Ok(None);
        }
        match self.fetch_account_profile().await {
            Ok(Some(profile)) => {
                self.set_account_state(AccountState::LoggedIn(profile.clone()));
                Ok(Some(profile))
            }
            Ok(None) => {
                // 服务端明确说未登录：旧 Cookie 已失效，账号级缓存一并回收。
                self.client.set_cookie(None);
                self.set_account_state(AccountState::LoggedOut);
                self.purge_account_cache().await;
                Ok(None)
            }
            Err(e) => {
                // 网络/服务不可达时不要误清 Cookie。
                self.set_account_state(AccountState::Unknown);
                Err(e)
            }
        }
    }

    pub(super) async fn login_begin_inner(&self, method: LoginMethod) -> CoreResult<LoginChallenge> {
        match method {
            LoginMethod::Qr => {
                // 防缓存 timestamp 由 NeteaseClient 统一附加（api-enhanced
                // 的 apicache 会缓存未登录结果，扫码流程尤其不能命中旧响应）。
                let key_body = self.client.get_json("/login/qr/key", &[]).await?;
                let key = key_body
                    .get("data")
                    .and_then(|d| d.get("unikey"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| CoreError::source_err("netease", "qr key missing"))?
                    .to_string();
                let create = self
                    .client
                    .get_json(
                        "/login/qr/create",
                        &[("key", key.as_str()), ("qrimg", "true")],
                    )
                    .await?;
                let qr_data_url = create
                    .get("data")
                    .and_then(|d| d.get("qrimg"))
                    .and_then(serde_json::Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                Ok(LoginChallenge {
                    method: LoginMethod::Qr,
                    key: Some(key),
                    qr_data_url,
                    expires_at: Some(now_secs() + 180),
                })
            }
            LoginMethod::CellphoneCaptcha { .. } => Err(CoreError::Config(
                "手机号登录暂未开放，请使用扫码登录".into(),
            )),
        }
    }

    pub(super) async fn login_poll_inner(&self, challenge: &LoginChallenge) -> CoreResult<LoginProgress> {
        let Some(key) = challenge.key.as_deref() else {
            return Ok(LoginProgress::Failed("登录挑战缺少 key".into()));
        };
        let body = self
            .client
            .get_json_with_codes("/login/qr/check", &[("key", key)], &[800, 801, 802, 803])
            .await?;
        let code = body
            .get("code")
            .and_then(serde_json::Value::as_i64)
            .or_else(|| {
                body.get("data")
                    .and_then(|d| d.get("code"))
                    .and_then(serde_json::Value::as_i64)
            })
            .unwrap_or(-1);
        match code {
            800 => Ok(LoginProgress::Expired),
            801 => Ok(LoginProgress::Waiting),
            802 => Ok(LoginProgress::Scanned),
            803 => {
                let cookie = body
                    .get("cookie")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        body.get("data")
                            .and_then(|d| d.get("cookie"))
                            .and_then(serde_json::Value::as_str)
                    })
                    .filter(|c| !c.trim().is_empty())
                    .map(str::to_string);
                let Some(cookie) = cookie else {
                    return Ok(LoginProgress::Failed(
                        "登录成功但服务端未返回 Cookie".into(),
                    ));
                };
                self.client.set_cookie(Some(cookie));
                match self.fetch_account_profile().await {
                    Ok(Some(profile)) => {
                        // 切号/重新登录：旧账号的歌单缓存不得复用。
                        self.purge_account_cache().await;
                        self.set_account_state(AccountState::LoggedIn(profile.clone()));
                        Ok(LoginProgress::Success(profile))
                    }
                    Ok(None) => {
                        self.client.set_cookie(None);
                        Ok(LoginProgress::Failed("Cookie 已获取但账号校验失败".into()))
                    }
                    Err(e) => Ok(LoginProgress::Failed(e.to_string())),
                }
            }
            other => Ok(LoginProgress::Failed(format!(
                "未知登录状态 {other}: {}",
                body.get("message")
                    .or_else(|| body.get("msg"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
            ))),
        }
    }

    pub(super) async fn logout_inner(&self) -> CoreResult<()> {
        let _ = self
            .client
            .get_json_with_codes("/logout", &[], &[200, 301])
            .await;
        self.client.set_cookie(None);
        self.set_account_state(AccountState::LoggedOut);
        self.purge_account_cache().await;
        Ok(())
    }

    pub(super) async fn send_captcha_inner(&self, phone: &str) -> CoreResult<()> {
        self.client
            .get_json("/captcha/sent", &[("phone", phone)])
            .await?;
        Ok(())
    }

    pub(super) async fn login_cellphone_inner(
        &self,
        phone: &str,
        captcha: &str,
    ) -> CoreResult<AccountProfile> {
        let body = self
            .client
            .get_json(
                "/login/cellphone",
                &[("phone", phone), ("captcha", captcha)],
            )
            .await?;
        let cookie = body
            .get("cookie")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                body.get("data")
                    .and_then(|d| d.get("cookie"))
                    .and_then(serde_json::Value::as_str)
            })
            .filter(|c| !c.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                CoreError::source_err("netease", "手机号登录成功但服务端未返回 Cookie")
            })?;
        self.client.set_cookie(Some(cookie));
        let profile = self
            .fetch_account_profile()
            .await?
            .ok_or_else(|| CoreError::source_err("netease", "手机号登录后账号校验失败"))?;
        self.purge_account_cache().await;
        self.set_account_state(AccountState::LoggedIn(profile.clone()));
        Ok(profile)
    }
}
