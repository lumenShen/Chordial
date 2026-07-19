//! Web 来源配置 — 持久化 base_url / cookie / 超时等参数。
//!
//! [`WebSourceConfig`] 是一个简单的 serde 结构体，通过 [`PersistentStore`] 持久化。
//! 每个来源一个独立的 key（如 `web_source_netease`），互不干扰。

use crate::module::storage::persistent::PersistentStore;
use serde::{Deserialize, Serialize};

/// Web 来源的持久化配置。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebSourceConfig {
    /// API 基础 URL（如 `https://ncm-api.example.com`）
    #[serde(default)]
    pub base_url: String,
    /// 登录后的 cookie 字符串（用于身份认证）
    #[serde(default)]
    pub cookie: String,
    /// 请求超时（秒，0 = 默认 15s）
    #[serde(default)]
    pub timeout_secs: u64,
    /// 是否启用此来源
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 下载目录（用于缓存音频文件；为空则使用应用默认下载目录）
    #[serde(default)]
    pub download_dir: String,
}

fn default_enabled() -> bool {
    true
}

impl WebSourceConfig {
    /// 从 PersistentStore 加载配置。
    ///
    /// 若 key 不存在则返回默认值（首次启动）。
    pub fn load(store: &PersistentStore, key: &str) -> Self {
        store.get::<Self>(key).unwrap_or_default()
    }

    /// 保存配置到 PersistentStore（不立即落盘，由调用方决定 save 时机）。
    /// `set` 内部会自动标记 dirty，下次 `save_if_dirty` 时落盘。
    pub fn save(&self, store: &PersistentStore, key: &str) -> Result<(), String> {
        store.set(key, self)
    }

    /// 立即落盘。
    pub fn flush(&self, store: &PersistentStore, key: &str) -> Result<(), String> {
        self.save(store, key)?;
        store.save()
    }
}

/// 标准化的 web 来源配置 key 前缀。
pub const CONFIG_KEY_PREFIX: &str = "web_source_";

/// 生成 web 来源的标准配置 key。
pub fn config_key(source_name: &str) -> String {
    format!("{}{}", CONFIG_KEY_PREFIX, source_name)
}
