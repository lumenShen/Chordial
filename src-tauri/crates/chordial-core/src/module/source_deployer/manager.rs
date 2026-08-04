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
//!
//! # 模块职责
//!
//! 本模块仅保留部署器主体结构、构造与目录/端口辅助方法；各职责拆分为：
//! - [`super::spec`]：固定规格（`list_deployable_sources` / `find_spec`）
//! - [`super::config`]：部署配置读写与持久化
//! - [`super::status`]：运行时状态查询（列表 / 详情 / 日志）
//! - [`super::deploy`]：git 克隆 / 拉取 / npm 安装
//! - [`super::process`]：后台进程管理（启动 / 停止 / 退出清理）
//! - [`super::toolchain`]：git / node / npm 工具链检测

use super::process::RunningProcess;
use super::types::DeployConfig;
use crate::module::storage::persistent::PersistentStore;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// 部署配置在 PersistentStore 中的 key。
pub(super) const CONFIG_KEY: &str = "source_deployer_configs";

/// 日志环形缓冲容量（每条 stdout/stderr 行算一条）。
pub(super) const LOG_BUFFER_CAP: usize = 500;

/// 前端展示的日志条数（`get_source_info` 时仅返回最近 N 条）。
pub(super) const RECENT_LOGS_DISPLAY: usize = 200;

/// 部署器主体。
///
/// 所有字段均为 `Arc` 或 `PathBuf`（`Clone` 廉价），便于在后台任务中共享。
pub struct SourceDeployer {
    /// 持久化存储（与 AppContext 共享同一份）。
    pub(super) store: Arc<PersistentStore>,
    /// 数据根目录（`source_services/` 子目录用于克隆项目）。
    pub(super) data_dir: PathBuf,
    /// 各来源的部署配置（source_id → config）。
    pub(super) configs: Arc<RwLock<HashMap<String, DeployConfig>>>,
    /// 当前运行中的进程（source_id → 元数据）。
    pub(super) processes: Arc<RwLock<HashMap<String, Arc<RunningProcess>>>>,
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
}