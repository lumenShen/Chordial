//! 网易云 api-enhanced sidecar 管理。
//!
//! 两种模式：
//! - `External`：用户自行运行服务，只探测 `base_url`；
//! - `Managed`：Chordial 启动 Node 进程，自动挑端口、等就绪、失败给日志；
//!   `Auto` 在找到随包/本地 api-enhanced 目录时使用 Managed，否则回退 External。
//!
//! 进程生命周期与源开关绑定：源卸载时 `stop_if_not_keep_warm()`；`keep_warm`
//! 为 true 则保留进程。`kill_on_drop(true)` 兜底，避免 Rust 侧退出后残留
//! 子进程（Windows 仍建议安装器使用 Job Object 进一步约束）。
//!
//! 预期的 api-enhanced 目录：
//! ```text
//! resources/netease-api/
//!   app.js            # 或 server.js / index.js
//!   node_modules/...
//!   node(.exe)        # 可选，随包 Node
//! ```
//! 入口按 `app.js → server.js → index.js` 探测；`node_path` 可显式指定。

use std::collections::VecDeque;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use chordal_core::error::{CoreError, CoreResult};

/// 服务模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NeteaseMode {
    /// 有随包/本地 api-enhanced 就托管，否则回退外部服务。
    #[default]
    Auto,
    /// 只连接用户已经运行的服务。
    External,
    /// 由 Chordial 启动 Node 进程。
    Managed,
}

/// 托管模式配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedConfig {
    /// 显式 Node 可执行文件；None = 随包 node / PATH 上的 node。
    #[serde(default)]
    pub node_path: Option<PathBuf>,
    /// api-enhanced 的 Node 源码目录；None = 自动探测 resources/netease-api 等。
    #[serde(default)]
    pub app_dir: Option<PathBuf>,
    /// 显式服务可执行文件（api-enhanced 官方 release 的二进制）。
    #[serde(default)]
    pub binary_path: Option<PathBuf>,
    /// 自动下载/安装官方 api-enhanced 二进制。
    #[serde(default = "default_true")]
    pub auto_install: bool,
    /// 安装目录；None = 平台数据目录/chordial/netease-api。
    #[serde(default)]
    pub install_dir: Option<PathBuf>,
    /// api-enhanced 版本；None 或 "latest" = 最新 release。
    #[serde(default)]
    pub api_version: Option<String>,
    /// 可选：固定 SHA-256（十六进制），用于覆盖/加强 release 提供的摘要。
    #[serde(default)]
    pub api_sha256: Option<String>,
    /// 固定端口；None = 自动挑空闲端口。
    #[serde(default)]
    pub port: Option<u16>,
    /// 源卸载后是否保留 Node 进程。
    #[serde(default)]
    pub keep_warm: bool,
    /// 启动就绪超时。
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout_ms: u64,
}

fn default_true() -> bool {
    true
}

fn default_startup_timeout() -> u64 {
    15_000
}

impl Default for ManagedConfig {
    fn default() -> Self {
        Self {
            node_path: None,
            app_dir: None,
            binary_path: None,
            auto_install: true,
            install_dir: None,
            api_version: None,
            api_sha256: None,
            port: None,
            keep_warm: false,
            startup_timeout_ms: default_startup_timeout(),
        }
    }
}

/// 动态 base_url：托管进程重启换端口时更新，客户端每次请求读取当前值。
#[derive(Debug)]
pub struct Endpoint {
    current: RwLock<String>,
}

impl Endpoint {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            current: RwLock::new(base_url.into().trim_end_matches('/').to_string()),
        }
    }

    pub fn get(&self) -> String {
        self.current
            .read()
            .map(|u| u.clone())
            .unwrap_or_else(|_| "http://127.0.0.1:3000".into())
    }

    pub fn set(&self, base_url: impl Into<String>) {
        if let Ok(mut current) = self.current.write() {
            *current = base_url.into().trim_end_matches('/').to_string();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ServiceState {
    Stopped,
    Ready { base_url: String, pid: u32 },
}

/// 托管服务管理器。线程安全，可被多个请求并发 `ensure_ready`。
pub struct NeteaseService {
    config: ManagedConfig,
    endpoint: Arc<Endpoint>,
    state: Mutex<ServiceState>,
    child: tokio::sync::Mutex<Option<Child>>,
    start_lock: tokio::sync::Mutex<()>,
    logs: Arc<Mutex<VecDeque<String>>>,
}

impl std::fmt::Debug for NeteaseService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NeteaseService")
            .field("state", &self.state.lock().ok().map(|s| s.clone()))
            .finish_non_exhaustive()
    }
}

impl Drop for NeteaseService {
    fn drop(&mut self) {
        // 正常退出时同步发 kill；若锁被占用则交给 Child 的 kill_on_drop。
        if let Ok(mut guard) = self.child.try_lock()
            && let Some(child) = guard.as_mut()
        {
            let _ = child.start_kill();
        }
    }
}

impl NeteaseService {
    /// 根据配置构造。`External` 返回 None（不管理进程）；`Auto` 只有找到
    /// api-enhanced 入口时才进入 Managed。
    pub fn from_config(
        mode: NeteaseMode,
        managed: &ManagedConfig,
        endpoint: Arc<Endpoint>,
    ) -> Option<Arc<Self>> {
        let use_managed = match mode {
            NeteaseMode::External => false,
            NeteaseMode::Managed => true,
            NeteaseMode::Auto => resolve_app_dir(managed).is_some(),
        };
        if !use_managed {
            return None;
        }
        Some(Arc::new(Self {
            config: managed.clone(),
            endpoint,
            state: Mutex::new(ServiceState::Stopped),
            child: tokio::sync::Mutex::new(None),
            start_lock: tokio::sync::Mutex::new(()),
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(200))),
        }))
    }

    /// 确保服务可用，返回当前 base_url。这是每个 HTTP 请求前的钩子。
    pub async fn ensure_ready(&self) -> CoreResult<String> {
        let _guard = self.start_lock.lock().await;
        if let Some(base) = self.current_ready().await? {
            return Ok(base);
        }
        self.start_managed(&self.config).await
    }

    /// 源卸载时调用；`keep_warm` 为 true 时保留进程。
    pub async fn stop_if_not_keep_warm(&self) {
        if self.config.keep_warm {
            return;
        }
        self.stop().await;
    }

    /// 主动停止 Node 进程。
    pub async fn stop(&self) {
        let mut guard = self.child.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if let Ok(mut state) = self.state.lock() {
            *state = ServiceState::Stopped;
        }
    }

    pub fn logs_tail(&self, lines: usize) -> Vec<String> {
        self.logs
            .lock()
            .map(|logs| logs.iter().rev().take(lines).rev().cloned().collect())
            .unwrap_or_default()
    }

    async fn current_ready(&self) -> CoreResult<Option<String>> {
        let mut child_guard = self.child.lock().await;
        let ready = {
            let state = self
                .state
                .lock()
                .map_err(|_| CoreError::source_err("netease", "service state poisoned"))?;
            match &*state {
                ServiceState::Ready { base_url, .. } => Some(base_url.clone()),
                ServiceState::Stopped => None,
            }
        };
        let Some(base_url) = ready else {
            return Ok(None);
        };
        match child_guard.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) => Ok(Some(base_url)),
                Ok(Some(status)) => {
                    tracing::warn!(
                        target: "chordial::netease",
                        code = status.code(),
                        "managed api-enhanced exited; will restart on next request"
                    );
                    *child_guard = None;
                    if let Ok(mut state) = self.state.lock() {
                        *state = ServiceState::Stopped;
                    }
                    Ok(None)
                }
                Err(e) => {
                    tracing::warn!(target: "chordial::netease", error = %e, "try_wait failed");
                    *child_guard = None;
                    if let Ok(mut state) = self.state.lock() {
                        *state = ServiceState::Stopped;
                    }
                    Ok(None)
                }
            },
            None => {
                if let Ok(mut state) = self.state.lock() {
                    *state = ServiceState::Stopped;
                }
                Ok(None)
            }
        }
    }

    async fn start_managed(&self, config: &ManagedConfig) -> CoreResult<String> {
        let app_dir = resolve_app_dir(config).ok_or_else(|| {
            CoreError::source_err(
                "netease",
                format!(
                    "未找到 api-enhanced 目录（app.js/server.js/index.js）。查找过：{}",
                    candidate_app_dirs(config)
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("、")
                ),
            )
        })?;
        let entry = resolve_entry(&app_dir).ok_or_else(|| {
            CoreError::source_err(
                "netease",
                format!("{} 中没有 app.js / server.js / index.js", app_dir.display()),
            )
        })?;
        let node = resolve_node(config, &app_dir);
        let port = pick_port(config.port)?;
        let base_url = format!("http://127.0.0.1:{port}");

        let mut command = Command::new(&node);
        command
            .arg(&entry)
            .current_dir(&app_dir)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .env("NODE_ENV", "production")
            .env("NODE_OPTIONS", "--max-old-space-size=256")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        tracing::info!(
            target: "chordial::netease",
            node = %node.display(),
            entry = %entry.display(),
            port,
            "spawning managed api-enhanced"
        );
        let mut child = command.spawn().map_err(|e| {
            CoreError::source_err(
                "netease",
                format!("启动 Node 失败（{}）：{e}", node.display()),
            )
        })?;
        let pid = child.id().unwrap_or(0);
        if let Some(stdout) = child.stdout.take() {
            spawn_log_reader(stdout, self.logs.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_log_reader(stderr, self.logs.clone());
        }

        let timeout = Duration::from_millis(config.startup_timeout_ms.max(1_000));
        if let Err(e) = wait_ready(&mut child, &base_url, timeout, &self.logs).await {
            let _ = child.kill().await;
            let tail = self.logs_tail(12).join("\n");
            return Err(CoreError::source_err(
                "netease",
                format!("api-enhanced 启动失败：{e}\n{tail}"),
            ));
        }

        self.endpoint.set(base_url.clone());
        if let Ok(mut state) = self.state.lock() {
            *state = ServiceState::Ready {
                base_url: base_url.clone(),
                pid,
            };
        }
        *self.child.lock().await = Some(child);
        tracing::info!(
            target: "chordial::netease",
            pid,
            base_url = %base_url,
            "managed api-enhanced ready"
        );
        Ok(base_url)
    }
}

async fn wait_ready(
    child: &mut Child,
    base_url: &str,
    timeout: Duration,
    logs: &Arc<Mutex<VecDeque<String>>>,
) -> CoreResult<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let tail = logs
                .lock()
                .map(|logs| {
                    logs.iter()
                        .rev()
                        .take(8)
                        .rev()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            return Err(CoreError::source_err(
                "netease",
                format!("Node 进程提前退出（{:?}）\n{tail}", status.code()),
            ));
        }
        if probe(base_url).await {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(CoreError::source_err(
                "netease",
                format!("等待 {base_url} 就绪超时"),
            ));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// 探测 `/login/status`：未登录也返回 JSON（code/data），足以确认服务身份。
async fn probe(base_url: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    let Ok(response) = client.get(format!("{base_url}/login/status")).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    response
        .json::<serde_json::Value>()
        .await
        .map(|body| body.get("code").is_some() || body.get("data").is_some())
        .unwrap_or(false)
}

fn spawn_log_reader<R>(stream: R, logs: Arc<Mutex<VecDeque<String>>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // 不把 Cookie/URL token 打进日志尾部。
            let line = redact(&line);
            if let Ok(mut logs) = logs.lock() {
                if logs.len() >= 200 {
                    logs.pop_front();
                }
                logs.push_back(line);
            }
        }
    });
}

fn redact(line: &str) -> String {
    let mut out = line.to_string();
    for needle in ["MUSIC_U=", "__csrf="] {
        if let Some(pos) = out.find(needle) {
            let end = out[pos..]
                .find(|c: char| c == ';' || c == '&' || c.is_whitespace())
                .map(|i| pos + i)
                .unwrap_or(out.len());
            out.replace_range(pos + needle.len()..end, "***");
        }
    }
    out
}

fn pick_port(configured: Option<u16>) -> CoreResult<u16> {
    if let Some(port) = configured {
        // 提前确认端口可用；绑定后立即释放，交给 Node 监听。
        TcpListener::bind(("127.0.0.1", port))
            .map(|_| port)
            .map_err(|e| {
                CoreError::source_err("netease", format!("managed.port {port} 不可用（{e}）"))
            })
    } else {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| CoreError::source_err("netease", format!("挑选本地端口失败：{e}")))?;
        listener
            .local_addr()
            .map(|addr| addr.port())
            .map_err(|e| CoreError::source_err("netease", format!("读取本地端口失败：{e}")))
    }
}

/// 解析 api-enhanced 目录。显式配置优先，其次环境变量、随包 resources、开发期 vendor。
pub fn resolve_app_dir(config: &ManagedConfig) -> Option<PathBuf> {
    candidate_app_dirs(config)
        .into_iter()
        .find(|dir| resolve_entry(dir).is_some())
}

fn candidate_app_dirs(config: &ManagedConfig) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = &config.app_dir {
        candidates.push(dir.clone());
    }
    if let Ok(dir) = std::env::var("CHORDIAL_NETEASE_APP") {
        if !dir.trim().is_empty() {
            candidates.push(PathBuf::from(dir));
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("resources").join("netease-api"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("resources").join("netease-api"));
        candidates.push(cwd.join("vendor").join("netease-api"));
    }
    candidates
}

fn resolve_entry(app_dir: &Path) -> Option<PathBuf> {
    ["app.js", "server.js", "index.js"]
        .into_iter()
        .map(|name| app_dir.join(name))
        .find(|path| path.is_file())
}

fn resolve_node(config: &ManagedConfig, app_dir: &Path) -> PathBuf {
    if let Some(node) = &config.node_path {
        return node.clone();
    }
    let bundled = if cfg!(windows) {
        app_dir.join("node.exe")
    } else {
        app_dir.join("node")
    };
    if bundled.is_file() {
        return bundled;
    }
    PathBuf::from(if cfg!(windows) { "node.exe" } else { "node" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_falls_back_to_external_without_assets() {
        let config = ManagedConfig {
            app_dir: Some(std::env::temp_dir().join("chordial-no-such-app")),
            ..Default::default()
        };
        assert!(resolve_app_dir(&config).is_none());
        let endpoint = Arc::new(Endpoint::new("http://127.0.0.1:3000"));
        let service = NeteaseService::from_config(NeteaseMode::Auto, &config, endpoint);
        assert!(service.is_none(), "Auto without assets must stay external");
    }

    #[test]
    fn explicit_app_dir_is_detected() {
        let dir = std::env::temp_dir().join(format!("chordial-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.js"), "console.log('stub')").unwrap();
        let config = ManagedConfig {
            app_dir: Some(dir.clone()),
            ..Default::default()
        };
        assert_eq!(resolve_app_dir(&config), Some(dir.clone()));
        assert_eq!(resolve_entry(&dir), Some(dir.join("app.js")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn redacts_cookie_fragments() {
        let line = "cookie: MUSIC_U=abc123; __csrf=xyz; other=1";
        let redacted = redact(line);
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("xyz"));
        assert!(redacted.contains("other=1"));
    }

    /// 真实 spawn 一个最小 Node HTTP 服务，验证端口/就绪/停止。
    /// 没有 Node 的环境自动跳过。
    #[test]
    fn spawns_and_stops_a_node_process() {
        let node_available = std::process::Command::new("node")
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        if !node_available {
            eprintln!("node not available; skipping sidecar spawn test");
            return;
        }
        let dir =
            std::env::temp_dir().join(format!("chordial-sidecar-spawn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("app.js"),
            r#"
const http = require('http');
const port = Number(process.env.PORT);
http.createServer((req, res) => {
  res.setHeader('content-type', 'application/json');
  res.end(JSON.stringify({ code: 200, data: { account: null } }));
}).listen(port, '127.0.0.1', () => console.log('stub-ready'));
"#,
        )
        .unwrap();
        let config = ManagedConfig {
            app_dir: Some(dir.clone()),
            startup_timeout_ms: 8_000,
            ..Default::default()
        };
        let endpoint = Arc::new(Endpoint::new("http://127.0.0.1:1"));
        let service =
            NeteaseService::from_config(NeteaseMode::Managed, &config, endpoint.clone()).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let base = runtime.block_on(service.ensure_ready()).unwrap();
        assert!(base.starts_with("http://127.0.0.1:"));
        assert_eq!(endpoint.get(), base);
        // 停止后再次 ensure_ready 应能重新拉起（端口可能变化）。
        runtime.block_on(service.stop());
        let base2 = runtime.block_on(service.ensure_ready()).unwrap();
        assert!(base2.starts_with("http://127.0.0.1:"));
        assert_eq!(endpoint.get(), base2);
        runtime.block_on(service.stop());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
