//! [`SourceDeployer`] — 音乐源服务快速部署器。
//!
//! 提供 git clone → npm install → npm start 的全链路自动化，并支持：
//! - 持久化部署配置（端口 / 自动启动 / 自动配置 base_url）
//! - 后台进程管理（启动 / 停止 / 日志缓冲）
//! - 程序启动时自动拉起 `auto_start = true` 的服务
//!
//! # 设计
//!
//! - 进程管理使用 `tokio::process::Command`（已启用 `kill_on_drop`）
//! - 每个 running 服务在 `processes` 表中存一份轻量元数据（PID + 日志 Arc + 停止信号）
//! - 实际的 `Child` 句柄由后台 `wait_task` 持有，等待退出后清理 map
//! - 日志使用 `parking_lot::RwLock<VecDeque<String>>` 环形缓冲（容量 500 行）

use super::types::{DeployConfig, DeployStatus, DeployableSourceSpec, SourceDeployInfo, ToolchainCheck};
use crate::module::perf;
use crate::module::storage::persistent::PersistentStore;
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

/// 部署配置在 PersistentStore 中的 key。
const CONFIG_KEY: &str = "source_deployer_configs";

/// 日志环形缓冲容量（每条 stdout/stderr 行算一条）。
const LOG_BUFFER_CAP: usize = 500;

/// 前端展示的日志条数（`get_source_info` 时仅返回最近 N 条）。
const RECENT_LOGS_DISPLAY: usize = 200;

/// 返回所有可部署来源的固定规格（按 ID 升序）。
pub fn list_deployable_sources() -> Vec<DeployableSourceSpec> {
    vec![
        DeployableSourceSpec {
            id: "netease".into(),
            display_name: "网易云音乐".into(),
            git_url: "https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced.git".into(),
            start_command: "npm".into(),
            start_args: vec!["start".into()],
            install_command: "npm".into(),
            install_args: vec!["install".into()],
            description: "NeteaseCloudMusicApi Enhanced — 提供网易云搜索/下载/扫码登录".into(),
            default_port: 3000,
        },
        DeployableSourceSpec {
            id: "kugou".into(),
            display_name: "酷狗音乐".into(),
            git_url: "https://github.com/MakcRe/KuGouMusicApi.git".into(),
            start_command: "npm".into(),
            start_args: vec!["start".into()],
            install_command: "npm".into(),
            install_args: vec!["install".into()],
            description: "KuGouMusicApi — 提供酷狗音乐搜索/下载/扫码登录".into(),
            default_port: 3000,
        },
        DeployableSourceSpec {
            id: "qq".into(),
            display_name: "QQ 音乐".into(),
            git_url: "https://github.com/jsososo/QQMusicApi.git".into(),
            start_command: "npm".into(),
            start_args: vec!["start".into()],
            install_command: "npm".into(),
            install_args: vec!["install".into()],
            description: "QQMusicApi — 提供 QQ 音乐搜索/下载/歌词".into(),
            default_port: 3300,
        },
    ]
}

/// 按 ID 查找固定规格。
pub fn find_spec(source_id: &str) -> Option<DeployableSourceSpec> {
    list_deployable_sources()
        .into_iter()
        .find(|s| s.id == source_id)
}

/// 运行中的进程元数据（轻量，存于 `processes` 表中）。
struct RunningProcess {
    /// 进程 PID（启动时缓存，避免后续读取需锁 Child）。
    pid: u32,
    /// 日志环形缓冲（stdout + stderr 合并，按时间顺序）。
    logs: Arc<RwLock<VecDeque<String>>>,
    /// 停止信号 Sender（`stop_source` 时 take 并发送）。
    /// 用 `Mutex<Option<...>>` 因为：发送方（用户点停止）和接收方（wait_task）
    /// 都可能访问；oneshot Sender 只能 send 一次，故包 `Option`。
    stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

/// 部署器主体。
///
/// 所有字段均为 `Arc` 或 `PathBuf`（`Clone` 廉价），便于在后台任务中共享。
pub struct SourceDeployer {
    /// 持久化存储（与 AppContext 共享同一份）。
    store: Arc<PersistentStore>,
    /// 数据根目录（`source_services/` 子目录用于克隆项目）。
    data_dir: PathBuf,
    /// 各来源的部署配置（source_id → config）。
    configs: Arc<RwLock<HashMap<String, DeployConfig>>>,
    /// 当前运行中的进程（source_id → 元数据）。
    processes: Arc<RwLock<HashMap<String, Arc<RunningProcess>>>>,
}

impl SourceDeployer {
    /// 创建部署器。从 PersistentStore 加载已有配置。
    pub fn new(store: Arc<PersistentStore>, data_dir: PathBuf) -> Self {
        let configs = store
            .get::<HashMap<String, DeployConfig>>(CONFIG_KEY)
            .unwrap_or_default();
        Self {
            store,
            data_dir,
            configs: Arc::new(RwLock::new(configs)),
            processes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 部署根目录：所有项目克隆到此目录下的子文件夹。
    pub fn deploy_root(&self) -> PathBuf {
        self.data_dir.join("source_services")
    }

    /// 某来源的项目目录。
    pub fn source_dir(&self, source_id: &str) -> PathBuf {
        self.deploy_root().join(source_id)
    }

    /// 计算某来源的实际监听端口（`config.port > 0` 优先，否则 `spec.default_port`）。
    pub fn effective_port(&self, source_id: &str) -> Option<u16> {
        let spec = find_spec(source_id)?;
        let cfg_port = self
            .configs
            .read()
            .get(source_id)
            .map(|c| c.port)
            .unwrap_or(0);
        Some(if cfg_port > 0 { cfg_port } else { spec.default_port })
    }

    /// 返回某来源服务的 `base_url`（`http://localhost:<port>`）。
    pub fn get_base_url(&self, source_id: &str) -> Option<String> {
        self.effective_port(source_id)
            .map(|p| format!("http://localhost:{}", p))
    }

    /// 列出所有 auto_start 且 enabled 的来源 ID（用于程序启动时自动拉起）。
    pub fn list_auto_start_ids(&self) -> Vec<String> {
        self.configs
            .read()
            .iter()
            .filter(|(_, c)| c.auto_start && c.enabled)
            .map(|(id, _)| id.clone())
            .collect()
    }

    // ── 查询 ───────────────────────────────────────────────────────────

    /// 列出所有可部署来源的完整信息（规格 + 配置 + 运行时）。
    pub fn list_sources(&self) -> Vec<SourceDeployInfo> {
        let _scope = perf::scope("source_deployer.list");
        let specs = list_deployable_sources();
        let configs = self.configs.read();
        let processes = self.processes.read();

        specs
            .into_iter()
            .map(|spec| {
                let cfg = configs.get(&spec.id).cloned().unwrap_or_default();
                let port = if cfg.port > 0 {
                    cfg.port
                } else {
                    spec.default_port
                };
                let base_url = format!("http://localhost:{}", port);

                let (status, pid, recent_logs) = if let Some(handle) = processes.get(&spec.id) {
                    let logs_guard = handle.logs.read();
                    let logs_vec: Vec<String> =
                        logs_guard.iter().rev().take(RECENT_LOGS_DISPLAY).cloned().rev().collect();
                    (DeployStatus::Running.as_str().to_string(), Some(handle.pid), logs_vec)
                } else {
                    let status_str = if cfg.last_status.is_empty() {
                        DeployStatus::NotDeployed.as_str().to_string()
                    } else {
                        cfg.last_status.clone()
                    };
                    (status_str, None, Vec::new())
                };

                SourceDeployInfo {
                    spec,
                    config: cfg,
                    runtime: super::types::DeployRuntimeInfo {
                        status,
                        pid,
                        base_url,
                        recent_logs,
                    },
                }
            })
            .collect()
    }

    /// 获取单个来源的配置（前端可编辑后回写）。
    pub fn get_config(&self, source_id: &str) -> Option<DeployConfig> {
        self.configs.read().get(source_id).cloned()
    }

    /// 获取单个来源的完整信息。
    pub fn get_source_info(&self, source_id: &str) -> Option<SourceDeployInfo> {
        self.list_sources().into_iter().find(|s| s.spec.id == source_id)
    }

    /// 更新某个来源的部署配置并持久化。
    pub fn update_config(&self, source_id: &str, config: DeployConfig) -> Result<(), String> {
        let mut configs = self.configs.write();
        configs.insert(source_id.to_string(), config);
        self.persist(&configs)
    }

    /// 切换 `auto_start` 标志（便捷方法）。
    pub fn set_auto_start(&self, source_id: &str, auto_start: bool) -> Result<(), String> {
        let mut configs = self.configs.write();
        let cfg = configs.entry(source_id.to_string()).or_default();
        cfg.auto_start = auto_start;
        self.persist(&configs)
    }

    /// 切换 `enabled` 标志。
    pub fn set_enabled(&self, source_id: &str, enabled: bool) -> Result<(), String> {
        let mut configs = self.configs.write();
        let cfg = configs.entry(source_id.to_string()).or_default();
        cfg.enabled = enabled;
        self.persist(&configs)
    }

    // ── 工具链检测 ──────────────────────────────────────────────────────

    /// 同步检测 git / node / npm 是否可用（同步阻塞，启动时调用）。
    pub fn check_toolchain(&self) -> ToolchainCheck {
        let _scope = perf::scope("source_deployer.check_toolchain");
        let git_out = run_version_command("git", &["--version"]);
        let node_out = run_version_command("node", &["--version"]);
        let npm_out = run_version_command("npm", &["--version"]);
        ToolchainCheck {
            git_available: git_out.is_some(),
            node_available: node_out.is_some(),
            npm_available: npm_out.is_some(),
            git_version: git_out.unwrap_or_default(),
            node_version: node_out.unwrap_or_default(),
            npm_version: npm_out.unwrap_or_default(),
        }
    }

    // ── 部署操作 ────────────────────────────────────────────────────────

    /// 克隆项目（`git clone --depth 1`，浅克隆节省时间与磁盘）。
    pub async fn clone_source(&self, source_id: &str) -> Result<(), String> {
        let spec = find_spec(source_id)
            .ok_or_else(|| format!("未知来源: {}", source_id))?;

        let target_dir = self.source_dir(source_id);
        if target_dir.exists() {
            return Err(format!("目标目录已存在: {}", target_dir.display()));
        }
        if let Some(parent) = target_dir.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("创建父目录失败: {}", e))?;
        }

        let _scope = perf::scope("source_deployer.clone");
        eprintln!(
            "[source_deployer] cloning {} from {}",
            source_id, spec.git_url
        );
        let output = Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg(&spec.git_url)
            .arg(&target_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("执行 git clone 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // 清理半成品目录
            let _ = tokio::fs::remove_dir_all(&target_dir).await;
            return Err(format!("git clone 失败: {}", stderr.trim()));
        }

        // 更新配置
        let mut configs = self.configs.write();
        let cfg = configs.entry(source_id.to_string()).or_default();
        cfg.install_path = target_dir.to_string_lossy().to_string();
        cfg.last_status = DeployStatus::Cloned.as_str().to_string();
        cfg.last_error.clear();
        let _ = self.persist(&configs);

        eprintln!("[source_deployer] cloned {} to {}", source_id, target_dir.display());
        Ok(())
    }

    /// 拉取最新代码（`git pull`）。仅在已克隆时可用。
    pub async fn pull_source(&self, source_id: &str) -> Result<(), String> {
        let target_dir = self.source_dir(source_id);
        if !target_dir.exists() {
            return Err(format!("项目尚未克隆: {}", target_dir.display()));
        }
        // 运行中的服务不可 pull（避免文件锁冲突）
        if self.processes.read().contains_key(source_id) {
            return Err("服务运行中，请先停止后再拉取代码".to_string());
        }

        let output = Command::new("git")
            .arg("-C")
            .arg(&target_dir)
            .arg("pull")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("执行 git pull 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git pull 失败: {}", stderr.trim()));
        }
        Ok(())
    }

    /// 安装依赖（`npm install`）。仅在已克隆时可用。
    pub async fn install_source(&self, source_id: &str) -> Result<(), String> {
        let spec = find_spec(source_id)
            .ok_or_else(|| format!("未知来源: {}", source_id))?;

        let target_dir = self.source_dir(source_id);
        if !target_dir.exists() {
            return Err(format!("项目尚未克隆，请先 clone: {}", target_dir.display()));
        }
        if self.processes.read().contains_key(source_id) {
            return Err("服务运行中，请先停止后再安装依赖".to_string());
        }

        let _scope = perf::scope("source_deployer.install");
        eprintln!("[source_deployer] installing deps for {}", source_id);
        let output = Command::new(&spec.install_command)
            .args(&spec.install_args)
            .current_dir(&target_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("执行 {} 失败: {}", spec.install_command, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let err_msg = format!("npm install 失败: {}", stderr.trim());
            let mut configs = self.configs.write();
            let cfg = configs.entry(source_id.to_string()).or_default();
            cfg.last_status = DeployStatus::Error.as_str().to_string();
            cfg.last_error = err_msg.clone();
            let _ = self.persist(&configs);
            return Err(err_msg);
        }

        // 更新配置：标记为 installed
        let mut configs = self.configs.write();
        let cfg = configs.entry(source_id.to_string()).or_default();
        cfg.last_status = DeployStatus::Installed.as_str().to_string();
        cfg.last_error.clear();
        let _ = self.persist(&configs);

        eprintln!("[source_deployer] installed deps for {}", source_id);
        Ok(())
    }

    /// 启动服务。
    ///
    /// - 通过 `PORT` 环境变量传入端口（NeteaseCloudMusicApi / KuGou / QQMusicApi 都遵守此约定）
    /// - 子进程设置 `kill_on_drop(true)`：deployer 退出时自动清理子进程
    /// - 启动后 spawn 一个 `wait_task` 等待退出并清理状态
    pub async fn start_source(&self, source_id: &str) -> Result<(), String> {
        let spec = find_spec(source_id)
            .ok_or_else(|| format!("未知来源: {}", source_id))?;

        // 已在运行
        if self.processes.read().contains_key(source_id) {
            return Ok(());
        }

        let target_dir = self.source_dir(source_id);
        if !target_dir.exists() {
            return Err(format!("项目尚未克隆，请先 clone: {}", target_dir.display()));
        }

        let cfg = self
            .configs
            .read()
            .get(source_id)
            .cloned()
            .unwrap_or_default();
        let port = if cfg.port > 0 { cfg.port } else { spec.default_port };

        let _scope = perf::scope("source_deployer.start");
        eprintln!(
            "[source_deployer] starting {} on port {}",
            source_id, port
        );

        let mut child = Command::new(&spec.start_command)
            .args(&spec.start_args)
            .current_dir(&target_dir)
            .env("PORT", port.to_string())
            .env("NODE_ENV", "production")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("启动服务失败: {}", e))?;

        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let logs = Arc::new(RwLock::new(VecDeque::with_capacity(LOG_BUFFER_CAP)));
        // 启动横幅（便于前端辨识日志起点）
        push_log(&logs, format!("── 服务启动 @ port {} ──", port));

        // 启动两个后台任务读取 stdout / stderr
        if let Some(stdout) = stdout {
            let logs_clone = logs.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    push_log(&logs_clone, format!("[out] {}", line));
                }
            });
        }
        if let Some(stderr) = stderr {
            let logs_clone = logs.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    push_log(&logs_clone, format!("[err] {}", line));
                }
            });
        }

        // 停止信号通道
        let (stop_tx, stop_rx) = oneshot::channel::<()>();

        let running = Arc::new(RunningProcess {
            pid: pid.unwrap_or(0),
            logs: logs.clone(),
            stop_tx: Mutex::new(Some(stop_tx)),
        });

        self.processes
            .write()
            .insert(source_id.to_string(), running.clone());

        // 启动 wait_task：等待进程退出（或收到停止信号后 kill）
        // 注意：此处不再传递 `running` Arc，因为：
        // - 进程自然退出时，wait_task 通过 `processes` map 清理条目
        // - 用户主动停止时，stop_source 已 take 走 stop_tx 并发送信号
        // - 日志 Arc 由子任务（stdout/stderr reader）独立持有，不依赖 RunningProcess 存活
        let processes = self.processes.clone();
        let configs_lock = self.configs.clone();
        let store = self.store.clone();
        let sid = source_id.to_string();
        tokio::spawn(async move {
            wait_task(sid, child, stop_rx, processes, configs_lock, store).await;
        });

        // 更新配置：标记为 running
        let mut configs = self.configs.write();
        let cfg = configs.entry(source_id.to_string()).or_default();
        cfg.last_status = DeployStatus::Running.as_str().to_string();
        cfg.last_started_at = now_secs();
        cfg.last_error.clear();
        let _ = self.persist(&configs);

        Ok(())
    }

    /// 停止服务（发送 stop 信号 → wait_task 收到后 kill child）。
    pub fn stop_source(&self, source_id: &str) -> Result<(), String> {
        let entry = self.processes.write().remove(source_id);
        let Some(handle) = entry else {
            return Ok(()); // 未运行，幂等返回
        };
        // 取出 stop_tx 并发送信号；wait_task 收到后会 kill child 并清理状态
        if let Some(tx) = handle.stop_tx.lock().take() {
            let _ = tx.send(());
        }
        // 注意：不在这里更新 last_status；由 wait_task 在 child 真正退出后更新
        eprintln!("[source_deployer] stop signal sent for {}", source_id);
        Ok(())
    }

    /// 移除部署：停止服务（若运行中）+ 删除项目目录 + 清除配置。
    pub async fn remove_source(&self, source_id: &str) -> Result<(), String> {
        // 1. 停止运行中的服务
        let _ = self.stop_source(source_id);

        // 2. 删除项目目录
        let target_dir = self.source_dir(source_id);
        if target_dir.exists() {
            tokio::fs::remove_dir_all(&target_dir)
                .await
                .map_err(|e| format!("删除目录失败: {}", e))?;
        }

        // 3. 清除配置
        let mut configs = self.configs.write();
        configs.remove(source_id);
        self.persist(&configs)
    }

    /// 检查某来源是否正在运行。
    pub fn is_running(&self, source_id: &str) -> bool {
        self.processes.read().contains_key(source_id)
    }

    /// 获取某来源最近 N 条日志（不运行时返回空）。
    pub fn get_recent_logs(&self, source_id: &str, n: usize) -> Vec<String> {
        let processes = self.processes.read();
        let Some(handle) = processes.get(source_id) else {
            return Vec::new();
        };
        let logs = handle.logs.read();
        logs.iter().rev().take(n).cloned().rev().collect()
    }

    /// 持久化当前 configs 到 PersistentStore。
    fn persist(&self, configs: &HashMap<String, DeployConfig>) -> Result<(), String> {
        self.store.set(CONFIG_KEY, configs)?;
        self.store.save()
    }
}

// ── 私有辅助 ────────────────────────────────────────────────────────────

/// 推送一行日志到环形缓冲（容量满时弹出最旧的一条）。
fn push_log(logs: &Arc<RwLock<VecDeque<String>>>, line: String) {
    let mut buf = logs.write();
    if buf.len() >= LOG_BUFFER_CAP {
        buf.pop_front();
    }
    buf.push_back(line);
}

/// 当前 Unix 时间戳（秒）。
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 同步执行版本命令，返回 stdout 字符串（trim 后）。失败返回 None。
///
/// Windows 上通过 `cmd /c` 调用以正确解析 .cmd/.bat 扩展名并继承用户 PATH。
#[cfg(target_os = "windows")]
fn run_version_command(cmd: &str, args: &[&str]) -> Option<String> {
    let mut full: Vec<&str> = vec!["/c", cmd];
    full.extend(args);
    let output = std::process::Command::new("cmd")
        .args(&full)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() || !output.status.success() {
        None
    } else {
        Some(s)
    }
}

#[cfg(not(target_os = "windows"))]
fn run_version_command(cmd: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// `wait_task`：等待子进程退出（或被停止信号 kill），然后清理状态并更新配置。
///
/// 参数说明：
/// - `source_id`: 用于清理 `processes` / `configs` map
/// - `child`: 子进程句柄（所有权转移到此任务）
/// - `stop_rx`: 停止信号接收端（`stop_source` 发送时触发 kill 分支）
/// - `processes` / `configs_lock` / `store`: 用于状态更新与持久化
async fn wait_task(
    source_id: String,
    mut child: Child,
    stop_rx: oneshot::Receiver<()>,
    processes: Arc<RwLock<HashMap<String, Arc<RunningProcess>>>>,
    configs_lock: Arc<RwLock<HashMap<String, DeployConfig>>>,
    store: Arc<PersistentStore>,
) {
    // 等待进程退出 OR 收到停止信号
    let (was_stopped, status) = tokio::select! {
        s = child.wait() => (false, s.ok()),
        _ = stop_rx => {
            // 收到停止信号：kill child 再 wait
            let _ = child.kill().await;
            (true, child.wait().await.ok())
        }
    };

    // 子进程已退出，从 processes map 移除（若已由 stop_source 移除则无操作）
    processes.write().remove(&source_id);

    // 更新配置
    let mut configs = configs_lock.write();
    let cfg = configs.entry(source_id.clone()).or_default();
    let new_status = if was_stopped {
        DeployStatus::Stopped
    } else {
        match &status {
            Some(s) if s.success() => DeployStatus::Stopped,
            Some(_) => DeployStatus::Error,
            None => DeployStatus::Error,
        }
    };
    cfg.last_status = new_status.as_str().to_string();
    cfg.last_stopped_at = now_secs();
    if !was_stopped {
        cfg.last_error = match &status {
            Some(s) => format!("进程异常退出: {}", s),
            None => "等待进程退出失败".to_string(),
        };
        eprintln!(
            "[source_deployer] {} exited unexpectedly: {:?}",
            source_id, status
        );
    } else {
        cfg.last_error.clear();
        eprintln!("[source_deployer] {} stopped cleanly", source_id);
    }
    let _ = store.set(CONFIG_KEY, &*configs);
    let _ = store.save();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spec_ids() {
        let specs = list_deployable_sources();
        assert_eq!(specs.len(), 3);
        let ids: Vec<_> = specs.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"netease"));
        assert!(ids.contains(&"kugou"));
        assert!(ids.contains(&"qq"));
    }

    #[test]
    fn test_status_round_trip() {
        for s in [
            DeployStatus::NotDeployed,
            DeployStatus::Cloned,
            DeployStatus::Installed,
            DeployStatus::Running,
            DeployStatus::Stopped,
            DeployStatus::Error,
        ] {
            assert_eq!(DeployStatus::from_str(s.as_str()), s);
        }
        // 未知字符串降级为 NotDeployed
        assert_eq!(DeployStatus::from_str("unknown"), DeployStatus::NotDeployed);
        assert_eq!(DeployStatus::from_str(""), DeployStatus::NotDeployed);
    }

    #[test]
    fn test_default_config() {
        let cfg = DeployConfig::default();
        assert!(!cfg.enabled);
        assert!(!cfg.auto_start);
        assert!(!cfg.auto_configure_base_url);
        assert_eq!(cfg.port, 0);
        assert!(cfg.last_status.is_empty());
    }
}
