//! api-enhanced HTTP 客户端封装。
//!
//! 性能与健壮性约定：
//! - 每 请求 10s 超时（reqwest 层），失败退避 300ms 重试一次；
//! - 每请求一个 `netease.request` 性能检查点（tracing + ledger）；
//! - 鉴权走 `Cookie` 头（api-enhanced 支持从环境变量或请求参数注入，
//!   适配器统一用请求头，避免把 Cookie 写进 URL 日志）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
            cookie: RwLock::new(normalize_cookie_opt(cookie)),
            service,
        }
    }

    pub fn set_cookie(&self, cookie: Option<String>) {
        if let Ok(mut current) = self.cookie.write() {
            *current = normalize_cookie_opt(cookie.as_deref());
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
        // api-enhanced 的 apicache 以 `host + path?query` 为键缓存 200 响应
        // 2 分钟，且键里不含 Cookie/请求头。登录轮询、`/user/account` 这类
        // 随登录态变化的响应一旦复用缓存就会一直读到“未登录”结果，因此每个
        // 请求都附带唯一 timestamp 强制换键（纯数字，兼容按数值解析的服务端）。
        let timestamp = cache_bust();
        let mut query: Vec<(&str, &str)> = Vec::with_capacity(params.len() + 1);
        query.extend_from_slice(params);
        query.push(("timestamp", timestamp.as_str()));

        let mut last_err: Option<String> = None;
        for attempt in 0..2u8 {
            if attempt > 0 {
                // 退避重试（运行在共享 tokio runtime 上，sleep 合法）。
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            let mut request = self.http.get(&url).query(&query);
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

/// 单调递增的毫秒时间戳，作为 GET 请求的缓存键扰动值。
fn cache_bust() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // 毫秒 + 进程内序号：同一毫秒内的并发请求也不会撞键。
    format!(
        "{}{:04}",
        millis,
        SEQ.fetch_add(1, Ordering::Relaxed) % 10_000
    )
}

fn normalize_cookie_opt(raw: Option<&str>) -> Option<String> {
    let normalized = normalize_cookie(raw?);
    (!normalized.is_empty()).then_some(normalized)
}

/// 把 api-enhanced 返回的 `Set-Cookie` 拼接串规范化为可放进 `Cookie` 请求头
/// 的 `name=value; ...`：丢弃 `Max-Age`/`Expires`/`Path` 等属性，同名 Cookie
/// 保留最后一个值。
///
/// 直接把带属性的串回传会被网易云解析成匿名会话（`anonimousUser: true`），
/// 表现为“扫码登录成功但歌单/我喜欢为空”。
fn normalize_cookie(raw: &str) -> String {
    const ATTRIBUTES: &[&str] = &[
        "max-age",
        "expires",
        "path",
        "domain",
        "httponly",
        "secure",
        "samesite",
        "priority",
        "partitioned",
    ];
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for part in raw.split(';') {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let lowered = name.to_ascii_lowercase();
        if name.is_empty() || ATTRIBUTES.contains(&lowered.as_str()) {
            continue;
        }
        let value = value.trim();
        let mut replaced = false;
        for pair in &mut pairs {
            if pair.0 == name {
                pair.1 = value;
                replaced = true;
                break;
            }
        }
        if !replaced {
            pairs.push((name, value));
        }
    }
    pairs
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_normalization_strips_set_cookie_attributes() {
        let raw = "MUSIC_R_T=1513381609303; Max-Age=2147483647; \
                   Expires=Fri, 10 Dec 2094 12:46:16 GMT; Path=/openapi/clientlog;;\
                   MUSIC_A_T=1; Path=/; MUSIC_U=abc==; Max-Age=1296010; \
                   __csrf=xyz; MUSIC_A_T=2";
        assert_eq!(
            normalize_cookie(raw),
            "MUSIC_R_T=1513381609303; MUSIC_A_T=2; MUSIC_U=abc==; __csrf=xyz"
        );
    }

    #[test]
    fn cookie_without_pairs_normalizes_to_none() {
        assert_eq!(normalize_cookie(" ; Path=/ "), "");
        assert!(normalize_cookie_opt(Some("Max-Age=1")).is_none());
        assert!(normalize_cookie_opt(None).is_none());
    }
}
