//! 部署工具的数据模型 — 静态规格 / 持久化配置 / 运行时状态。

use serde::{Deserialize, Serialize};

/// 部署状态枚举。
///
/// 持久化时以 snake_case 字符串保存（如 `"running"` / `"stopped"`）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeployStatus {
    /// 未部署（未克隆仓库）。
    #[default]
    NotDeployed,
    /// 已克隆仓库，未安装依赖。
    Cloned,
    /// 已安装依赖，未运行。
    Installed,
    /// 运行中。
    Running,
    /// 已停止。
    Stopped,
    /// 异常退出或启动失败。
    Error,
}

impl DeployStatus {
    /// 转为字符串（用于持久化与前端展示）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotDeployed => "not_deployed",
            Self::Cloned => "cloned",
            Self::Installed => "installed",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Error => "error",
        }
    }

    /// 从字符串解析（容错：未知值降级为 `NotDeployed`）。
    pub fn from_str(s: &str) -> Self {
        match s {
            "cloned" => Self::Cloned,
            "installed" => Self::Installed,
            "running" => Self::Running,
            "stopped" => Self::Stopped,
            "error" => Self::Error,
            _ => Self::NotDeployed,
        }
    }
}

/// 可部署的音乐源服务规格（固定信息，代码内硬编码）。
///
/// 字段按大小降序排列以减少结构体 padding。
#[derive(Debug, Clone, Serialize)]
pub struct DeployableSourceSpec {
    /// 来源 ID（`"netease"` / `"kugou"`），与 `WebSourceConfig` 的来源名对齐。
    pub id: String,
    /// 展示名（如 `"网易云音乐"`）。
    pub display_name: String,
    /// GitHub 仓库地址（`https://...git`）。
    pub git_url: String,
    /// 镜像 URL（国内网络环境备用，如 `https://ghproxy.com/...`）。
    /// 当 `git_url` 克隆失败时自动回退到此 URL 重试。
    /// `None` 表示不启用镜像回退。
    pub mirror_url: Option<String>,
    /// 启动命令（通常为 `"npm"`）。
    pub start_command: String,
    /// 启动参数（如 `["start"]`）。
    pub start_args: Vec<String>,
    /// 安装依赖命令（通常为 `"npm"`）。
    pub install_command: String,
    /// 安装依赖参数（如 `["install"]`）。
    pub install_args: Vec<String>,
    /// 项目描述（前端展示用）。
    pub description: String,
    /// 默认监听端口（与各 API 项目 `package.json` 中的默认端口一致）。
    pub default_port: u16,
}

/// 部署配置（持久化到 `PersistentStore`）。
///
/// 字段按大小降序排列以减少结构体 padding。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeployConfig {
    /// 项目克隆到的本地路径（克隆完成后写入）。
    pub install_path: String,
    /// 上次状态（snake_case 字符串，对应 [`DeployStatus::as_str`]）。
    pub last_status: String,
    /// 上次错误信息（启动 / 安装失败时写入）。
    pub last_error: String,
    /// 上次启动时间（Unix 秒）。
    pub last_started_at: u64,
    /// 上次停止时间（Unix 秒）。
    pub last_stopped_at: u64,
    /// 监听端口（`0` = 使用 `default_port`）。
    pub port: u16,
    /// 是否启用快速部署（关闭后忽略 `auto_start`）。
    pub enabled: bool,
    /// 程序启动时自动运行此服务。
    pub auto_start: bool,
    /// 服务启动后自动把对应来源的 `base_url` 设为 `http://localhost:<port>`。
    pub auto_configure_base_url: bool,
}

/// 运行时进程信息（非持久化，每次启动后构建）。
#[derive(Debug, Clone, Serialize)]
pub struct DeployRuntimeInfo {
    /// 当前状态（`running` / `stopped` / ...）。
    pub status: String,
    /// 进程 PID（未运行时为 `None`）。
    pub pid: Option<u32>,
    /// 服务的 `base_url`（`http://localhost:<port>`）。
    pub base_url: String,
    /// 最近日志行（最多 200 条，按时间正序）。
    pub recent_logs: Vec<String>,
}

/// 完整的部署信息（前端列表展示用）。
#[derive(Debug, Clone, Serialize)]
pub struct SourceDeployInfo {
    /// 来源规格（固定信息）。
    pub spec: DeployableSourceSpec,
    /// 持久化配置（用户可编辑）。
    pub config: DeployConfig,
    /// 运行时状态（实时）。
    pub runtime: DeployRuntimeInfo,
}

/// 工具链检测结果。
#[derive(Debug, Clone, Serialize, Default)]
pub struct ToolchainCheck {
    /// git 是否可用。
    pub git_available: bool,
    /// node 是否可用。
    pub node_available: bool,
    /// npm 是否可用。
    pub npm_available: bool,
    /// git 版本字符串（如 `"git version 2.43.0"`）。
    pub git_version: String,
    /// node 版本字符串（如 `"v20.10.0"`）。
    pub node_version: String,
    /// npm 版本字符串（如 `"10.2.3"`）。
    pub npm_version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

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
