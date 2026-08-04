//! Source Deployer — 音乐源服务快速部署工具。

use chordial_core::module::source_deployer::{DeployConfig, SourceDeployInfo, ToolchainCheck};
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

// ══════════════════════════════════════════════════════════════════════════════
// Source Deployer — 音乐源服务快速部署工具
// ══════════════════════════════════════════════════════════════════════════════
//
// 提供 git clone → npm install → npm start 的全链路自动化部署，
// 让用户一键在本地拉起 NeteaseCloudMusicApi / KuGouMusicApi 服务。
// 配置（端口 / auto_start / auto_configure_base_url）持久化到 PersistentStore。

/// 列出所有可部署来源的完整信息（规格 + 配置 + 运行时状态）。
#[tauri::command]
pub fn source_deployer_list(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<SourceDeployInfo>, String> {
    Ok(ctx.source_deployer.list_sources())
}

/// 获取单个来源的完整信息。
#[tauri::command]
pub fn source_deployer_get(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<SourceDeployInfo, String> {
    ctx.source_deployer
        .get_source_info(&source_id)
        .ok_or_else(|| format!("未知来源: {}", source_id))
}

/// 获取单个来源的部署配置。
#[tauri::command]
pub fn source_deployer_get_config(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<DeployConfig, String> {
    Ok(ctx.source_deployer.get_config(&source_id).unwrap_or_default())
}

/// 更新来源的部署配置（端口 / auto_start / auto_configure_base_url / enabled）。
#[tauri::command]
pub fn source_deployer_update_config(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
    config: DeployConfig,
) -> Result<(), String> {
    ctx.source_deployer.update_config(&source_id, config)
}

/// 切换 `auto_start` 标志（程序启动时自动运行）。
#[tauri::command]
pub fn source_deployer_set_auto_start(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
    auto_start: bool,
) -> Result<(), String> {
    ctx.source_deployer.set_auto_start(&source_id, auto_start)
}

/// 切换 `enabled` 标志（关闭后忽略 auto_start）。
#[tauri::command]
pub fn source_deployer_set_enabled(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
    enabled: bool,
) -> Result<(), String> {
    ctx.source_deployer.set_enabled(&source_id, enabled)
}

/// 检测 git / node / npm 是否可用及版本号。
#[tauri::command]
pub fn source_deployer_check_toolchain(
    ctx: State<'_, Arc<AppContext>>,
) -> Result<ToolchainCheck, String> {
    Ok(ctx.source_deployer.check_toolchain())
}

/// 克隆项目（`git clone --depth 1` 浅克隆）。
#[tauri::command]
pub async fn source_deployer_clone(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<(), String> {
    ctx.source_deployer.clone_source(&source_id).await
}

/// 拉取最新代码（`git pull`）。
#[tauri::command]
pub async fn source_deployer_pull(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<(), String> {
    ctx.source_deployer.pull_source(&source_id).await
}

/// 安装依赖（`npm install`）。
#[tauri::command]
pub async fn source_deployer_install(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<(), String> {
    ctx.source_deployer.install_source(&source_id).await
}

/// 启动服务（PORT 环境变量 + kill_on_drop）。
#[tauri::command]
pub async fn source_deployer_start(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<(), String> {
    ctx.source_deployer.start_source(&source_id).await
}

/// 停止服务（发送 stop 信号 → wait_task kill child）。
#[tauri::command]
pub fn source_deployer_stop(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<(), String> {
    ctx.source_deployer.stop_source(&source_id)
}

/// 移除部署：停止服务（若运行中）+ 删除项目目录 + 清除配置。
#[tauri::command]
pub async fn source_deployer_remove(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<(), String> {
    ctx.source_deployer.remove_source(&source_id).await
}

/// 获取最近 N 条日志（默认 200 条）。未运行时返回空数组。
#[tauri::command]
pub fn source_deployer_get_logs(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
    n: Option<usize>,
) -> Result<Vec<String>, String> {
    let n = n.unwrap_or(200);
    Ok(ctx.source_deployer.get_recent_logs(&source_id, n))
}

/// 获取某来源服务的 `base_url`（`http://localhost:<port>`）。
/// 用于部署成功后自动配置对应来源（netease/kugou）的 base_url。
#[tauri::command]
pub fn source_deployer_get_base_url(
    ctx: State<'_, Arc<AppContext>>,
    source_id: String,
) -> Result<Option<String>, String> {
    Ok(ctx.source_deployer.get_base_url(&source_id))
}