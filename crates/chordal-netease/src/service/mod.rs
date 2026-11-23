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

mod helpers;
mod lifecycle;
mod start;

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


#[cfg(test)]
mod tests;
