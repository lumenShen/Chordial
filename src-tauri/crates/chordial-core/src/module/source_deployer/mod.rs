//! 音乐源服务快速部署工具。
//!
//! 提供 git clone → npm install → npm start 的全链路自动化部署，
//! 让用户一键在本地拉起 NeteaseCloudMusicApi / KuGouMusicApi 服务，
//! 并可设置程序启动时自动运行。
//!
//! # 模块结构
//!
//! | 子模块 | 职责 |
//! |--------|------|
//! | [`types`] | 数据模型（`DeployableSourceSpec` / `DeployConfig` / `SourceDeployInfo` / ...） |
//! | [`spec`] | 可部署来源的固定规格（`list_deployable_sources` / `find_spec`） |
//! | [`manager`] | [`SourceDeployer`] 主体结构（构造 / 目录辅助） |
//! | [`config`] | 部署配置读写与持久化 |
//! | [`status`] | 运行时状态查询（列表 / 详情 / 日志） |
//! | [`deploy`] | git 克隆 / 拉取 / npm 安装 |
//! | [`process`] | 后台进程管理（启动 / 停止 / 退出清理） |
//! | [`toolchain`] | git / node / npm 工具链检测 |
//!
//! # 使用示例
//!
//! ```ignore
//! use chordial_core::module::source_deployer::SourceDeployer;
//! use std::sync::Arc;
//! use std::path::PathBuf;
//!
//! let deployer = SourceDeployer::new(store.clone(), data_dir);
//! // 列出所有可部署来源及其状态
//! let infos = deployer.list_sources();
//! // 一键部署 + 启动
//! deployer.clone_source("netease").await?;
//! deployer.install_source("netease").await?;
//! deployer.start_source("netease").await?;
//! // 停止
//! deployer.stop_source("netease")?;
//! ```

pub mod config;
pub mod deploy;
pub mod manager;
pub mod process;
pub mod spec;
pub mod status;
pub mod toolchain;
pub mod types;

pub use manager::SourceDeployer;
pub use spec::{find_spec, list_deployable_sources};
pub use types::{
    DeployConfig, DeployRuntimeInfo, DeployStatus, DeployableSourceSpec, SourceDeployInfo,
    ToolchainCheck,
};
