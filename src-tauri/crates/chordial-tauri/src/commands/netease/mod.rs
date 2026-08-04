//! 网易云音乐来源命令 — 按领域拆分的子模块汇总。
//!
//! 命令统一以 `netease_` 前缀命名，经 `commands` 的 `pub use netease::*`
//! 聚合导出，供 `lib.rs` 的 `invoke_handler` 以 `commands::netease_*` 引用。
//! 命令本身不含业务逻辑，仅取 `State<'_, Arc<AppContext>>` 委托给 core。

mod account;
mod compare;
mod config;
mod download;
mod login;
mod playlist;
mod search;

pub use account::*;
pub use compare::*;
pub use config::*;
pub use download::*;
pub use login::*;
pub use playlist::*;
pub use search::*;