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
//! | [`manager`] | [`SourceDeployer`] 主体（进程管理 / git 操作 / 配置持久化） |
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

pub mod manager;
pub mod types;

pub use manager::{find_spec, list_deployable_sources, SourceDeployer};
pub use types::{
    DeployConfig, DeployRuntimeInfo, DeployStatus, DeployableSourceSpec, SourceDeployInfo,
    ToolchainCheck,
};
