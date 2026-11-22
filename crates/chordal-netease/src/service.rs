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
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// 优先本地资产/外部服务；都没有时在 auto_install 下自动下载。
    #[default]
    Auto,
    /// 只连接用户已经运行的服务。
    External,
    /// 由 Chordial 启动/自动安装 api-enhanced。
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
    /// 正在下载/安装或启动，其他请求应等待 Notify。
    Installing,
    /// Auto 模式探测到用户已有的外部服务。
    ExternalReady {
        base_url: String,
    },
    Ready {
        base_url: String,
        pid: u32,
    },
    Failed(String),
}

/// 托管服务管理器。线程安全，可被多个请求并发 `ensure_ready`。
pub struct NeteaseService {
    mode: NeteaseMode,
    /// Auto 模式回退的外部服务地址。
    external_base_url: String,
    config: ManagedConfig,
    endpoint: Arc<Endpoint>,
    state: Mutex<ServiceState>,
    child: tokio::sync::Mutex<Option<Child>>,
    start_lock: tokio::sync::Mutex<()>,
    notify: tokio::sync::Notify,
    logs: Arc<Mutex<VecDeque<String>>>,
    /// 每次 stop 自增，用于安装/启动完成后判断是否已被取消。
    generation: AtomicU64,
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
    /// 根据配置构造。`External` 返回 None（不管理进程）；`Auto`/`Managed`
    /// 返回管理器，首次需要时下载或启动 api-enhanced。
    pub fn from_config(
        mode: NeteaseMode,
        managed: &ManagedConfig,
        endpoint: Arc<Endpoint>,
    ) -> Option<Arc<Self>> {
        if mode == NeteaseMode::External {
            return None;
        }
        Some(Arc::new(Self {
            mode,
            external_base_url: endpoint.get(),
            config: managed.clone(),
            endpoint,
            state: Mutex::new(ServiceState::Stopped),
            child: tokio::sync::Mutex::new(None),
            start_lock: tokio::sync::Mutex::new(()),
            notify: tokio::sync::Notify::new(),
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(200))),
            generation: AtomicU64::new(0),
        }))
    }

    /// 确保服务可用，返回当前 base_url。安装/启动在后台任务中执行，
    /// 调用方超时被 drop 也不会中断下载。
    pub async fn ensure_ready(self: &Arc<Self>) -> CoreResult<String> {
        let guard = self.start_lock.lock().await;
        if let Some(base) = self.current_ready().await? {
            return Ok(base);
        }
        match self.state_snapshot() {
            ServiceState::Installing => {
                let notified = self.notify.notified();
                drop(guard);
                notified.await;
                return self.wait_result().await;
            }
            ServiceState::Failed(e) => {
                return Err(CoreError::source_err("netease", e));
            }
            _ => {}
        }

        // Auto 且没有本地可管理资产：优先复用用户已运行的外部服务。
        if self.mode == NeteaseMode::Auto && !self.has_local_managed() {
            let external = self.external_base_url.clone();
            if probe(&external).await {
                self.endpoint.set(external.clone());
                self.set_state(ServiceState::ExternalReady {
                    base_url: external.clone(),
                });
                return Ok(external);
            }
        }

        self.set_state(ServiceState::Installing);
        let notified = self.notify.notified();
        let this = self.clone();
        tokio::spawn(async move {
            let result = this.prepare_and_start().await;
            if let Err(e) = &result {
                tracing::error!(target: "chordial::netease", error = %e, "api-enhanced prepare failed");
                // 卸载/停止可能已经发生，不要把状态改回 Failed 之外的运行态。
                if matches!(this.state_snapshot(), ServiceState::Installing) {
                    this.set_state(ServiceState::Failed(e.to_string()));
                }
            }
            // 成功时 prepare_and_start 已经写入 Ready/child。
            this.notify.notify_waiters();
        });
        drop(guard);
        notified.await;
        self.wait_result().await
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
        self.generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut state) = self.state.lock() {
            *state = ServiceState::Stopped;
        }
        self.notify.notify_waiters();
        let mut guard = self.child.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
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
        let (ready, external) = {
            let state = self
                .state
                .lock()
                .map_err(|_| CoreError::source_err("netease", "service state poisoned"))?;
            match &*state {
                ServiceState::Ready { base_url, .. } => (Some(base_url.clone()), false),
                ServiceState::ExternalReady { base_url } => (Some(base_url.clone()), true),
                _ => (None, false),
            }
        };
        let Some(base_url) = ready else {
            return Ok(None);
        };
        if external {
            return Ok(Some(base_url));
        }
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

    fn state_snapshot(&self) -> ServiceState {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(ServiceState::Stopped)
    }

    fn set_state(&self, state: ServiceState) {
        if let Ok(mut current) = self.state.lock() {
            *current = state;
        }
    }

    async fn wait_result(&self) -> CoreResult<String> {
        loop {
            match self.state_snapshot() {
                ServiceState::Ready { base_url, .. } => {
                    if self.current_ready().await?.is_some() {
                        return Ok(base_url);
                    }
                }
                ServiceState::ExternalReady { base_url } => return Ok(base_url),
                ServiceState::Failed(e) => return Err(CoreError::source_err("netease", e)),
                ServiceState::Installing => {
                    self.notify.notified().await;
                }
                ServiceState::Stopped => {
                    return Err(CoreError::source_err("netease", "api-enhanced 服务已停止"));
                }
            }
        }
    }

    fn has_local_managed(&self) -> bool {
        if self
            .config
            .binary_path
            .as_ref()
            .is_some_and(|path| path.is_file())
        {
            return true;
        }
        if resolve_app_dir(&self.config).is_some() {
            return true;
        }
        installed_binary(&self.config).is_some()
    }

    async fn prepare_and_start(&self) -> CoreResult<String> {
        let target = self.resolve_launch_target().await?;
        let epoch = self.generation.load(Ordering::Acquire);
        let port = pick_port(self.config.port)?;
        let base_url = format!("http://127.0.0.1:{port}");

        let mut command = Command::new(&target.program);
        for arg in &target.args {
            command.arg(arg);
        }
        command
            .current_dir(&target.cwd)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .env("NODE_ENV", "production")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if target.node {
            command.env("NODE_OPTIONS", "--max-old-space-size=256");
        }
        #[cfg(windows)]
        {
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        tracing::info!(
            target: "chordial::netease",
            program = %target.program.display(),
            label = %target.label,
            port,
            "spawning managed api-enhanced"
        );
        let mut child = command.spawn().map_err(|e| {
            CoreError::source_err(
                "netease",
                format!(
                    "启动 api-enhanced 失败（{}）：{e}",
                    target.program.display()
                ),
            )
        })?;
        let pid = child.id().unwrap_or(0);
        if let Some(stdout) = child.stdout.take() {
            spawn_log_reader(stdout, self.logs.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_log_reader(stderr, self.logs.clone());
        }

        let timeout = Duration::from_millis(self.config.startup_timeout_ms.max(1_000));
        if let Err(e) = wait_ready(&mut child, &base_url, timeout, &self.logs).await {
            let _ = child.kill().await;
            let tail = self.logs_tail(12).join("\n");
            return Err(CoreError::source_err(
                "netease",
                format!("api-enhanced 启动失败：{e}\n{tail}"),
            ));
        }

        // stop() 可能在下载/启动期间发生：此时丢弃刚启动的进程，不写 Ready。
        if epoch != self.generation.load(Ordering::Acquire)
            || !matches!(self.state_snapshot(), ServiceState::Installing)
        {
            let _ = child.kill().await;
            return Err(CoreError::source_err(
                "netease",
                "api-enhanced 启动被取消（源已停止）",
            ));
        }
        self.endpoint.set(base_url.clone());
        self.set_state(ServiceState::Ready {
            base_url: base_url.clone(),
            pid,
        });
        *self.child.lock().await = Some(child);
        // 检查 set_state 与 child 写入之间的 stop 竞态。
        if epoch != self.generation.load(Ordering::Acquire) {
            if let Some(mut child) = self.child.lock().await.take() {
                let _ = child.kill().await;
            }
            self.set_state(ServiceState::Stopped);
            return Err(CoreError::source_err(
                "netease",
                "api-enhanced 启动被取消（源已停止）",
            ));
        }
        tracing::info!(
            target: "chordial::netease",
            pid,
            base_url = %base_url,
            "managed api-enhanced ready"
        );
        Ok(base_url)
    }

    async fn resolve_launch_target(&self) -> CoreResult<LaunchTarget> {
        if let Some(binary) = &self.config.binary_path {
            if !binary.is_file() {
                return Err(CoreError::source_err(
                    "netease",
                    format!("managed.binary_path 不存在：{}", binary.display()),
                ));
            }
            return Ok(LaunchTarget::binary(binary.clone()));
        }
        if let Some(dir) = &self.config.app_dir {
            return node_target(dir, &self.config).ok_or_else(|| {
                CoreError::source_err(
                    "netease",
                    format!("{} 中没有 app.js / server.js / index.js", dir.display()),
                )
            });
        }
        if let Some(binary) = installed_binary(&self.config) {
            return Ok(LaunchTarget::binary(binary));
        }
        if let Some(dir) = resolve_app_dir(&self.config) {
            let node = resolve_node(&self.config, &dir);
            let node_usable = node.is_file()
                || std::process::Command::new(&node)
                    .arg("--version")
                    .output()
                    .map(|out| out.status.success())
                    .unwrap_or(false);
            if node_usable && let Some(target) = node_target(&dir, &self.config) {
                return Ok(target);
            }
        }
        if self.config.auto_install {
            let binary = install_binary(&self.config, &self.logs).await?;
            return Ok(LaunchTarget::binary(binary));
        }
        Err(CoreError::source_err(
            "netease",
            format!(
                "未找到 api-enhanced 且 auto_install=false。查找过：{}",
                candidate_app_dirs(&self.config)
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            ),
        ))
    }
}

struct LaunchTarget {
    program: PathBuf,
    args: Vec<PathBuf>,
    cwd: PathBuf,
    node: bool,
    label: String,
}

impl LaunchTarget {
    fn binary(path: PathBuf) -> Self {
        let cwd = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            label: format!("binary {}", path.display()),
            program: path,
            args: Vec::new(),
            cwd,
            node: false,
        }
    }

    fn node(program: PathBuf, entry: PathBuf, app_dir: PathBuf) -> Self {
        Self {
            label: format!("node {} {}", program.display(), entry.display()),
            program,
            args: vec![entry],
            cwd: app_dir,
            node: true,
        }
    }
}

fn node_target(dir: &Path, config: &ManagedConfig) -> Option<LaunchTarget> {
    let entry = resolve_entry(dir)?;
    let node = resolve_node(config, dir);
    Some(LaunchTarget::node(node, entry, dir.to_path_buf()))
}

fn install_root(config: &ManagedConfig) -> PathBuf {
    config.install_dir.clone().unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| std::env::temp_dir())
            .join("chordial")
            .join("netease-api")
    })
}

fn binary_filename() -> &'static str {
    if cfg!(windows) {
        "api-enhanced.exe"
    } else {
        "api-enhanced"
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct InstalledMeta {
    version: String,
    asset: String,
    sha256: String,
}

fn installed_binary(config: &ManagedConfig) -> Option<PathBuf> {
    let binary = install_root(config).join(binary_filename());
    if !binary.is_file() {
        return None;
    }
    let meta: Option<InstalledMeta> =
        std::fs::read_to_string(binary.with_file_name("installed.json"))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok());
    if let Some(requested) = config
        .api_version
        .as_deref()
        .filter(|v| !v.is_empty() && *v != "latest")
    {
        match &meta {
            Some(meta) if meta.version == requested => {}
            _ => return None,
        }
    }
    if let Some(expected) = config.api_sha256.as_deref() {
        match &meta {
            Some(meta) if meta.sha256 == normalize_sha256(expected) => {}
            _ => return None,
        }
    }
    Some(binary)
}

fn release_asset_name() -> CoreResult<&'static str> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Ok("ncm-api-win-x64.exe");
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return Ok("ncm-api-linux-x64");
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return Ok("ncm-api-macos-x64");
    }
    #[allow(unreachable_code)]
    Err(CoreError::source_err(
        "netease",
        format!(
            "api-enhanced 暂无当前平台预编译包（{}/{}）",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    ))
}

fn normalize_sha256(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("sha256:")
        .trim_start_matches("SHA256:")
        .to_ascii_lowercase()
}

/// 下载官方 api-enhanced release 二进制，校验 SHA-256 后原子安装。
async fn install_binary(
    config: &ManagedConfig,
    logs: &Arc<Mutex<VecDeque<String>>>,
) -> CoreResult<PathBuf> {
    if let Some(binary) = installed_binary(config) {
        return Ok(binary);
    }
    let root = install_root(config);
    tokio::fs::create_dir_all(&root).await.map_err(|e| {
        CoreError::source_err(
            "netease",
            format!("创建安装目录失败 {}：{e}", root.display()),
        )
    })?;
    let asset_name = release_asset_name()?;
    let version = config.api_version.as_deref().unwrap_or("latest");
    let release_url = if version == "latest" {
        "https://api.github.com/repos/NeteaseCloudMusicApiEnhanced/api-enhanced/releases/latest"
            .to_string()
    } else {
        format!(
            "https://api.github.com/repos/NeteaseCloudMusicApiEnhanced/api-enhanced/releases/tags/{version}"
        )
    };
    push_log(
        logs,
        format!("fetching api-enhanced release: {release_url}"),
    );
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(1_800))
        .user_agent(concat!("Chordial/2.0 (", env!("CARGO_PKG_VERSION"), ")"))
        .build()
        .map_err(|e| CoreError::source_err("netease", format!("创建下载客户端失败：{e}")))?;
    let release: serde_json::Value = client
        .get(&release_url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| CoreError::source_err("netease", format!("获取 release 失败：{e}")))?
        .error_for_status()
        .map_err(|e| CoreError::source_err("netease", format!("release HTTP 错误：{e}")))?
        .json()
        .await
        .map_err(|e| CoreError::source_err("netease", format!("解析 release 失败：{e}")))?;
    let tag = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let asset = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .and_then(|assets| {
            assets
                .iter()
                .find(|a| a.get("name").and_then(serde_json::Value::as_str) == Some(asset_name))
        })
        .ok_or_else(|| {
            CoreError::source_err("netease", format!("{tag} 中没有资产 {asset_name}"))
        })?;
    // 优先用 API asset URL（Accept: application/octet-stream）：它重定向到
    // release-assets.githubusercontent.com，比 github.com/releases/download
    // 在受限网络下更容易下载。
    let api_asset_url = asset
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let url = api_asset_url
        .or_else(|| {
            asset
                .get("browser_download_url")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| CoreError::source_err("netease", "release 资产缺少下载地址"))?;
    let api_digest = asset
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .map(normalize_sha256);
    if let (Some(expected), Some(api)) = (&config.api_sha256, &api_digest)
        && normalize_sha256(expected) != *api
    {
        return Err(CoreError::source_err(
            "netease",
            "managed.api_sha256 与 GitHub release 摘要不一致",
        ));
    }
    let expected = config
        .api_sha256
        .as_deref()
        .map(normalize_sha256)
        .or(api_digest)
        .ok_or_else(|| {
            CoreError::source_err(
                "netease",
                "release 未提供 SHA-256 且未配置 managed.api_sha256；拒绝安装未校验的二进制",
            )
        })?;

    push_log(logs, format!("downloading {url}"));
    let response = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .map_err(|e| CoreError::source_err("netease", format!("下载失败：{e}")))?
        .error_for_status()
        .map_err(|e| CoreError::source_err("netease", format!("下载 HTTP 错误：{e}")))?;
    let temp = root.join(format!(".download-{}.part", std::process::id()));
    let mut file = tokio::fs::File::create(&temp)
        .await
        .map_err(|e| CoreError::source_err("netease", format!("创建临时文件失败：{e}")))?;
    use futures::StreamExt;
    use sha2::Digest;
    use tokio::io::AsyncWriteExt;
    let mut stream = response.bytes_stream();
    let mut total: u64 = 0;
    let mut hasher = sha2::Sha256::new();
    const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(e) => {
                let _ = tokio::fs::remove_file(&temp).await;
                return Err(CoreError::source_err(
                    "netease",
                    format!("读取下载流失败：{e}"),
                ));
            }
        };
        total += chunk.len() as u64;
        if total > MAX_DOWNLOAD {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(CoreError::source_err(
                "netease",
                "api-enhanced 下载超过 300MB，已中止",
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| CoreError::source_err("netease", format!("写入文件失败：{e}")))?;
    }
    file.flush().await.ok();
    drop(file);
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != expected {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(CoreError::source_err(
            "netease",
            format!("api-enhanced 校验失败：期望 {expected}，实际 {actual}"),
        ));
    }
    let binary = root.join(binary_filename());
    let _ = tokio::fs::remove_file(&binary).await;
    tokio::fs::rename(&temp, &binary)
        .await
        .map_err(|e| CoreError::source_err("netease", format!("安装二进制失败：{e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755));
    }
    let meta = InstalledMeta {
        version: tag.clone(),
        asset: asset_name.to_string(),
        sha256: actual.clone(),
    };
    if let Ok(json) = serde_json::to_vec_pretty(&meta) {
        let _ = tokio::fs::write(root.join("installed.json"), json).await;
    }
    push_log(
        logs,
        format!(
            "installed api-enhanced {tag} ({:.1} MB, sha256 {})",
            total as f64 / 1_048_576.0,
            actual
        ),
    );
    Ok(binary)
}

fn push_log(logs: &Arc<Mutex<VecDeque<String>>>, line: impl Into<String>) {
    if let Ok(mut logs) = logs.lock() {
        if logs.len() >= 200 {
            logs.pop_front();
        }
        logs.push_back(redact(&line.into()));
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
    fn auto_uses_manager_but_external_stays_direct() {
        let config = ManagedConfig {
            app_dir: Some(std::env::temp_dir().join("chordial-no-such-app")),
            ..Default::default()
        };
        assert!(resolve_app_dir(&config).is_none());
        let endpoint = Arc::new(Endpoint::new("http://127.0.0.1:3000"));
        let auto = NeteaseService::from_config(NeteaseMode::Auto, &config, endpoint.clone());
        assert!(auto.is_some(), "Auto may install/fallback at runtime");
        let external = NeteaseService::from_config(NeteaseMode::External, &config, endpoint);
        assert!(external.is_none());
    }

    #[test]
    fn release_asset_and_digest_helpers() {
        let asset = release_asset_name().unwrap();
        assert!(asset.starts_with("ncm-api-"));
        assert_eq!(normalize_sha256("SHA256:ABCdef"), "abcdef");
        assert_eq!(normalize_sha256(" abc "), "abc");
    }

    #[test]
    fn installed_binary_respects_pinned_version() {
        let dir = std::env::temp_dir().join(format!("chordial-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join(binary_filename());
        std::fs::write(&binary, b"stub").unwrap();
        let config = ManagedConfig {
            install_dir: Some(dir.clone()),
            api_version: Some("v1.0.0".into()),
            ..Default::default()
        };
        // 没有 installed.json：宁可重新安装，也不执行未知版本。
        assert!(installed_binary(&config).is_none());
        std::fs::write(
            dir.join("installed.json"),
            br#"{"version":"v1.0.0","asset":"x","sha256":"y"}"#,
        )
        .unwrap();
        assert_eq!(installed_binary(&config), Some(binary.clone()));
        let other = ManagedConfig {
            api_version: Some("v2.0.0".into()),
            ..config.clone()
        };
        assert!(installed_binary(&other).is_none());
        let _ = std::fs::remove_dir_all(&dir);
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

    /// 真实下载官方 api-enhanced release 二进制（约 60-80MB）。
    /// 默认忽略；网络可用时用 `cargo test -p chordal-netease -- --ignored` 跑。
    #[test]
    #[ignore = "downloads api-enhanced release from GitHub"]
    fn installs_official_binary_from_release() {
        let dir =
            std::env::temp_dir().join(format!("chordial-official-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = ManagedConfig {
            install_dir: Some(dir.clone()),
            ..Default::default()
        };
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let binary = runtime
            .block_on(install_binary(&config, &logs))
            .expect("official api-enhanced download");
        assert!(binary.is_file());
        let meta: InstalledMeta =
            serde_json::from_str(&std::fs::read_to_string(dir.join("installed.json")).unwrap())
                .unwrap();
        assert!(!meta.version.is_empty());
        assert_eq!(meta.sha256.len(), 64);

        // 用刚安装的官方二进制完整跑一遍启动/就绪/停止。
        let endpoint = Arc::new(Endpoint::new("http://127.0.0.1:1"));
        let service =
            NeteaseService::from_config(NeteaseMode::Managed, &config, endpoint.clone()).unwrap();
        let base = runtime.block_on(service.ensure_ready()).unwrap();
        assert!(base.starts_with("http://127.0.0.1:"));
        assert_eq!(endpoint.get(), base);
        runtime.block_on(service.stop());
        let _ = std::fs::remove_dir_all(&dir);
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
