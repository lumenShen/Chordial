//! 部署配置管理：读取 / 更新 / 持久化部署配置。

use super::manager::CONFIG_KEY;
use super::spec::find_spec;
use super::types::DeployConfig;
use std::collections::HashMap;

impl super::SourceDeployer {
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

    /// 获取单个来源的配置（前端可编辑后回写）。
    pub fn get_config(&self, source_id: &str) -> Option<DeployConfig> {
        self.configs.read().get(source_id).cloned()
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

    /// 持久化当前 configs 到 PersistentStore。
    pub(super) fn persist(&self, configs: &HashMap<String, DeployConfig>) -> Result<(), String> {
        self.store.set(CONFIG_KEY, configs)?;
        self.store.save()
    }
}