//! 运行时状态查询：来源列表 / 详情 / 最近日志。

use super::manager::RECENT_LOGS_DISPLAY;
use super::spec::list_deployable_sources;
use super::types::{DeployRuntimeInfo, DeployStatus, SourceDeployInfo};
use crate::module::perf;

impl super::SourceDeployer {
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
                    runtime: DeployRuntimeInfo {
                        status,
                        pid,
                        base_url,
                        recent_logs,
                    },
                }
            })
            .collect()
    }

    /// 获取单个来源的完整信息。
    pub fn get_source_info(&self, source_id: &str) -> Option<SourceDeployInfo> {
        self.list_sources().into_iter().find(|s| s.spec.id == source_id)
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
}