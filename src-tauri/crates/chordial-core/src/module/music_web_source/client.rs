//! HTTP 客户端封装 — 为 Web 来源提供统一的请求构建和响应处理。
//!
//! [`WebHttpClient`] 是 `reqwest::blocking::Client` 的薄包装，提供：
//! - 预配置的连接池（keep-alive、TLS、超时）
//! - 统一的 query/body 发送和 JSON 解析
//! - 自动附加 cookie / user-agent
//!
//! 所有方法都是同步阻塞的，因为 `MusicSource` trait 是同步接口。
//! 在 Tauri 命令中调用时，由 Tauri 自动将 `#[tauri::command]` 调度到线程池，不阻塞 UI。

use parking_lot::RwLock;
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;

/// 默认请求超时（秒）。
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// 默认 User-Agent — 部分远程 API（如网易云）对 UA 敏感。
const DEFAULT_USER_AGENT: &str = concat!(
    "Chordial/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/chordial)"
);

/// Web 来源共享的 HTTP 客户端。
///
/// 内部持有 `reqwest::blocking::Client`（连接池复用），以及可变 cookie。
/// `cookie` 通过 `RwLock` 保护，允许运行时更新（如登录后刷新）。
pub struct WebHttpClient {
    /// 复用的 reqwest Client（线程安全）
    client: Client,
    /// 当前 cookie（可能为空字符串）。所有请求自动附加到 `Cookie` header
    cookie: RwLock<String>,
    /// API base URL（如 `https://ncm-api.example.com`）
    base_url: RwLock<String>,
    /// 请求超时（秒）
    timeout_secs: u64,
}

impl WebHttpClient {
    /// 创建客户端。
    ///
    /// # 参数
    /// - `base_url`: API 基础 URL（结尾不带 `/`）
    /// - `cookie`: 初始 cookie（可空）
    /// - `timeout_secs`: 请求超时秒数（0 = 默认 15s）
    pub fn new(base_url: String, cookie: String, timeout_secs: u64) -> Result<Self, String> {
        let timeout_secs = if timeout_secs == 0 {
            DEFAULT_TIMEOUT_SECS
        } else {
            timeout_secs
        };

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(DEFAULT_USER_AGENT),
        );

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(5))
            .pool_idle_timeout(Some(Duration::from_secs(90)))
            .tcp_nodelay(true)
            .build()
            .map_err(|e| format!("构建 HTTP 客户端失败: {}", e))?;

        Ok(Self {
            client,
            cookie: RwLock::new(cookie),
            base_url: RwLock::new(base_url),
            timeout_secs,
        })
    }

    /// 更新 cookie（登录或刷新后调用）。
    pub fn set_cookie(&self, cookie: impl Into<String>) {
        *self.cookie.write() = cookie.into();
    }

    /// 获取当前 cookie。
    pub fn cookie(&self) -> String {
        self.cookie.read().clone()
    }

    /// 更新 base URL（切换 API 服务器时调用）。
    pub fn set_base_url(&self, base_url: impl Into<String>) {
        let mut url = base_url.into();
        // 统一去掉结尾 `/`，避免 `/api//search` 双斜杠
        while url.ends_with('/') {
            url.pop();
        }
        *self.base_url.write() = url;
    }

    /// 获取当前 base URL。
    pub fn base_url(&self) -> String {
        self.base_url.read().clone()
    }

    /// 获取请求超时（秒）。
    pub fn timeout(&self) -> u64 {
        self.timeout_secs
    }

    /// 构建 GET 请求 URL（base_url + path），自动添加查询参数。
    ///
    /// `path` 不以 `/` 开头时自动补。
    fn build_url(&self, path: &str) -> String {
        let base = self.base_url.read().clone();
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };
        format!("{}{}", base, path)
    }

    /// 构建 `RequestBuilder`，自动附加 cookie。
    fn attach_cookie(&self, mut rb: RequestBuilder) -> RequestBuilder {
        let cookie = self.cookie.read().clone();
        if !cookie.is_empty() {
            // 用 HeaderName 自动校验，避免无效字符触发 panic
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                rb = rb.header(reqwest::header::COOKIE, value);
            }
        }
        rb
    }

    /// 发送 GET 请求并解析 JSON。
    ///
    /// # 参数
    /// - `path`: 路径（如 `/search` 或 `search`）
    /// - `query`: 查询参数 `&[(&str, &str)]`
    pub fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, String> {
        let url = self.build_url(path);
        let rb = self.client.get(&url).query(query);
        let rb = self.attach_cookie(rb);
        let resp = rb.send().map_err(|e| format!("GET {} 失败: {}", url, e))?;
        Self::parse_json(resp)
    }

    /// 发送 POST 请求（form 表单）并解析 JSON。
    pub fn post_json<T: DeserializeOwned>(
        &self,
        path: &str,
        form: &[(&str, &str)],
    ) -> Result<T, String> {
        let url = self.build_url(path);
        let rb = self.client.post(&url).form(form);
        let rb = self.attach_cookie(rb);
        let resp = rb.send().map_err(|e| format!("POST {} 失败: {}", url, e))?;
        Self::parse_json(resp)
    }

    /// 发送 GET 请求并返回原始字节（用于下载音频/封面）。
    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        // url 是完整 URL，不拼接 base_url
        let rb = self.client.get(url);
        let rb = self.attach_cookie(rb);
        let resp = rb.send().map_err(|e| format!("GET {} 失败: {}", url, e))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("GET {} 状态码 {}", url, status));
        }
        resp.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("读取响应体失败: {}", e))
    }

    /// 发送 GET 请求并返回完整 URL 文本（用于重定向后的 URL 解析）。
    pub fn get_text(&self, url: &str) -> Result<String, String> {
        let rb = self.client.get(url);
        let rb = self.attach_cookie(rb);
        let resp = rb.send().map_err(|e| format!("GET {} 失败: {}", url, e))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("GET {} 状态码 {}", url, status));
        }
        resp.text()
            .map_err(|e| format!("读取响应体失败: {}", e))
    }

    /// 发送自定义请求（如 HEAD 用于探测）。
    pub fn request(&self, method: Method, url: &str) -> RequestBuilder {
        let rb = self.client.request(method, url);
        self.attach_cookie(rb)
    }

    /// 解析响应为 JSON，处理 HTTP 错误和 JSON 解析错误。
    fn parse_json<T: DeserializeOwned>(resp: Response) -> Result<T, String> {
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, truncate(&body, 200)));
        }
        resp.json::<T>()
            .map_err(|e| format!("JSON 解析失败: {}", e))
    }
}

/// 截断字符串到指定长度，超出部分用 `...` 替代。
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// 解析 HTTP 状态码到错误消息。
#[allow(dead_code)]
pub fn status_to_msg(status: StatusCode) -> String {
    format!("HTTP {}", status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        let result = truncate("hello world this is too long", 10);
        assert!(result.ends_with("..."));
        assert_eq!(result.len(), 13); // 10 + "..."
    }

    #[test]
    fn test_client_creation() {
        // 测试客户端能正常创建（不发起真实请求）
        let client = WebHttpClient::new(
            "https://example.com".to_string(),
            "".to_string(),
            0,
        );
        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.base_url(), "https://example.com");
        assert_eq!(client.timeout(), DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn test_set_base_url_trims_trailing_slash() {
        let client = WebHttpClient::new(
            "https://example.com/".to_string(),
            "".to_string(),
            0,
        )
        .unwrap();
        assert_eq!(client.base_url(), "https://example.com");
        client.set_base_url("https://api.example.com///");
        assert_eq!(client.base_url(), "https://api.example.com");
    }
}
