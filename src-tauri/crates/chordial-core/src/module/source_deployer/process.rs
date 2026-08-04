//! 后台进程管理：启动 / 停止 / 移除 / 日志缓冲 / 退出清理。

use super::deploy::spawn_npm;
use super::manager::{CONFIG_KEY, LOG_BUFFER_CAP};
use super::spec::find_spec;
use super::types::{DeployConfig, DeployStatus};
use crate::module::perf;
use crate::module::storage::persistent::PersistentStore;
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::oneshot;

/// 运行中的进程元数据（轻量，存于 `processes` 表中）。
pub(super) struct RunningProcess {
    /// 进程 PID（启动时缓存，避免后续读取需锁 Child）。
    pub(super) pid: u32,
    /// 日志环形缓冲（stdout + stderr 合并，按时间顺序）。
    pub(super) logs: Arc<RwLock<VecDeque<String>>>,
    /// 停止信号 Sender（`stop_source` 时 take 并发送）。
    /// 用 `Mutex<Option<...>>` 因为：发送方（用户点停止）和接收方（wait_task）
    /// 都可能访问；oneshot Sender 只能 send 一次，故包 `Option`。
    pub(super) stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl super::SourceDeployer {
    /// 启动服务。
    ///
    /// - 通过 `PORT` 环境变量传入端口（NeteaseCloudMusicApi / KuGou 都遵守此约定）
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

        // 启动前检查 node_modules 是否存在；缺失则自动 npm install
        // （用户可能只 clone 了仓库但没装依赖，直接 node app.js 会因找不到 express 退出）
        let node_modules = target_dir.join("node_modules");
        if !node_modules.exists() {
            eprintln!(
                "[source_deployer] {} missing node_modules, auto-running npm install",
                source_id
            );
            // 复用 install_source 的逻辑（含状态更新）
            self.install_source(source_id).await?;
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

        let mut child = spawn_npm(
            &spec.start_command,
            &spec.start_args,
            Some(&target_dir),
            Some(("PORT", port.to_string())),
        )
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
        let logs_arc = logs.clone();
        let sid = source_id.to_string();
        tokio::spawn(async move {
            wait_task(sid, child, stop_rx, processes, configs_lock, store, logs_arc).await;
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
    logs: Arc<RwLock<VecDeque<String>>>,
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

    // 给 stdout/stderr reader 一点时间把最后的输出刷入 logs
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 子进程已退出，从 processes map 移除（若已由 stop_source 移除则无操作）
    processes.write().remove(&source_id);

    // 读取最近日志（用于错误诊断）
    let recent_logs: Vec<String> = logs.read().iter().take(8).cloned().collect();

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
        // 拼接 exit code 和最近日志，让前端直接看到真实错误（如 Cannot find module 'express'）
        let exit_desc = match &status {
            Some(s) => format!("exit code: {}", s.code().unwrap_or(-1)),
            None => "等待进程退出失败".to_string(),
        };
        let mut err = format!("进程异常退出: {}", exit_desc);
        if !recent_logs.is_empty() {
            err.push_str("\n最近日志:\n");
            err.push_str(&recent_logs.join("\n"));
        }
        cfg.last_error = err;
        eprintln!(
            "[source_deployer] {} exited unexpectedly: {:?} | logs: {:?}",
            source_id, status, recent_logs
        );
    } else {
        cfg.last_error.clear();
        eprintln!("[source_deployer] {} stopped cleanly", source_id);
    }
    let _ = store.set(CONFIG_KEY, &*configs);
    let _ = store.save();
}