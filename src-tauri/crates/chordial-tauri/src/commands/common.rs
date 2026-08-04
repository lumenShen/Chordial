//! 命令层共享类型与常量 — 供各领域模块引用。
//!
//! 仅存放跨模块复用的极小类型与常量，避免在各领域模块中重复定义导致漂移。

use chordial_core::module::storage::entry::Ttl;
use serde::Deserialize;

/// 库变更事件名 — 任何改变库内容的操作（添加/移除源、扫描、CRUD）都应 emit。
///
/// 前端通过 `listen("library-changed")` 订阅，触发专辑/艺人列表刷新。
pub const LIBRARY_CHANGED_EVENT: &str = "library-changed";

/// 批量下载进度事件名 — 后台下载每完成一首（成功/失败）emit。
///
/// 前端通过 `listen("netease-download-progress")` 订阅，更新进度条 UI。
pub const DOWNLOAD_PROGRESS_EVENT: &str = "netease-download-progress";

/// 前端传入的 TTL 参数。
///
/// 支持三种格式（`serde_json` 可自动识别）：
/// - `"forever"` → [`Ttl::Forever`]
/// - `"session"` → [`Ttl::Session`]
/// - `{"duration_secs": 600}` → [`Ttl::DurationSecs(600)`]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TtlArg {
    Forever,
    Session,
    DurationSecs(u64),
}

impl From<TtlArg> for Ttl {
    fn from(arg: TtlArg) -> Self {
        match arg {
            TtlArg::Forever => Ttl::Forever,
            TtlArg::Session => Ttl::Session,
            TtlArg::DurationSecs(n) => Ttl::DurationSecs(n),
        }
    }
}