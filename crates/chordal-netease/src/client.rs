//! api-enhanced HTTP 客户端封装。
//!
//! 性能与健壮性约定：
//! - 每 请求 10s 超时（reqwest 层），失败退避 300ms 重试一次；
//! - 每请求一个 `netease.request` 性能检查点（tracing + ledger）；
//! - 鉴权走 `Cookie` 头（api-enhanced 支持从环境变量或请求参数注入，
//!   适配器统一用请求头，避免把 Cookie 写进 URL 日志）。

use std::sync::{Arc, RwLock};
use std::time::Duration;

use chordal_core::error::{CoreError, CoreResult};
use chordal_core::perf::PerfScope;

use crate::service::{Endpoint, NeteaseService};

pub struct NeteaseClient {
    base_url: Arc<Endpoint>,
    http: reqwest::Client,
    /// 登录后由 UI/会话层更新；每个请求按当前值注入 Cookie 头。
    cookie: RwLock<Option<String>>,
    /// 托管模式：每个请求前确保 Node 进程就绪。
    service: Option<Arc<NeteaseService>>,
}

impl NeteaseClient {
    pub fn new(base_url: &str, cookie: Option<&str>) -> Self {
        Self::new_with_endpoint(Arc::new(Endpoint::new(base_url)), cookie, None)
    }

    pub fn new_with_endpoint(
        base_url: Arc<Endpoint>,
        cookie: Option<&str>,
        service: Option<Arc<NeteaseService>>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            // 适配器只与服务端 JSON 交互，连接池上限给小值防句柄膨胀。
            .pool_max_idle_per_host(4)
            .user_agent(concat!("Chordial/2.0 (", env!("CARGO_PKG_VERSION"), ")"))
            .build()
            .expect("reqwest client");
        Self {
            base_url,
            http,
            cookie: RwLock::new(cookie.map(str::to_string)),
            service,
        }
    }

    pub fn set_cookie(&self, cookie: Option<String>) {
        if let Ok(mut current) = self.cookie.write() {
            *current = cookie.filter(|c| !c.trim().is_empty());
        }
    }

    pub fn cookie(&self) -> Option<String> {
        self.cookie.read().ok().and_then(|c| c.clone())
    }

    /// GET `{base}{path}?params` → JSON。默认 code != 200 视为源错误。
    pub async fn get_json(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> CoreResult<serde_json::Value> {
        self.get_json_with_codes(path, params, &[200]).await
    }

    /// 允许指定业务状态码（登录轮询的 800/801/802/803 不是错误）。
    pub async fn get_json_with_codes(
        &self,
        path: &str,
        params: &[(&str, &str)],
        allowed_codes: &[i64],
    ) -> CoreResult<serde_json::Value> {
        if let Some(service) = &self.service {
            service.ensure_ready().await?;
        }
        let scope = PerfScope::enter("netease.request");
        let url = format!("{}{}", self.base_url.get(), path);

        let mut last_err: Option<String> = None;
        for attempt in 0..2u8 {
            if attempt > 0 {
                // 退避重试（运行在共享 tokio runtime 上，sleep 合法）。
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            let mut request = self.http.get(&url).query(params);
            if let Some(cookie) = self.cookie()
                && let Ok(value) = reqwest::header::HeaderValue::from_str(&cookie)
            {
                request = request.header(reqwest::header::COOKIE, value);
            }
            let resp = match request.send().await {
                Ok(resp) => resp,
                Err(e) => {
                    last_err = Some(format!("request failed: {e}"));
                    continue;
                }
            };
            let status = resp.status();
            if status.is_server_error() {
                last_err = Some(format!("server error: {status}"));
                continue;
            }
            if !status.is_success() {
                // 4xx 不重试：参数/鉴权问题。
                return Err(CoreError::source_err(
                    "netease",
                    format!("HTTP {status} from {path}"),
                ));
            }
            let body: serde_json::Value = resp.json().await.map_err(|e| {
                CoreError::source_err("netease", format!("bad json from {path}: {e}"))
            })?;
            let code = body.get("code").and_then(serde_json::Value::as_i64);
            if let Some(code) = code
                && code != 200
                && !allowed_codes.contains(&code)
            {
                let msg = body
                    .get("message")
                    .or_else(|| body.get("msg"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                return Err(CoreError::source_err(
                    "netease",
                    format!("api code {code} from {path} {msg}"),
                ));
            }
            scope.finish(0);
            return Ok(body);
        }
        scope.finish(0);
        Err(CoreError::source_err(
            "netease",
            last_err.unwrap_or_else(|| "request failed".into()),
        ))
    }

    /// 存活探测：任何 HTTP 响应（含 4xx/5xx）都说明服务在监听。
    /// 只能在 tokio 上下文内轮询（由 spawn_inner 桥接保证）。
    pub async fn health(&self) -> bool {
        if let Some(service) = &self.service {
            return service.ensure_ready().await.is_ok();
        }
        let url = format!("{}/", self.base_url.get());
        self.http
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .is_ok()
    }
}
