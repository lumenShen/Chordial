//! Tauri 命令层 — 按领域拆分的模块汇总。
//!
//! 所有命令通过 Tauri 的 `State<'_, Arc<AppContext>>` 提取由 `setup` 注入的
//! server 层上下文，再委托给 core 方法。命令本身不含业务逻辑。
//!
//! 这正是「库调用形式」的 front 层：前端 `invoke` → 本层 → core 同步函数调用，
//! 全程进程内，无网络开销。
//!
//! 各领域命令位于独立子模块，此处统一 re-export 供 `lib.rs` 的
//! `invoke_handler` 以 `commands::*` 引用。

// 跨模块共享的常量与类型（不直接对前端暴露命令）
pub(crate) mod common;
pub(crate) mod downloads;
pub(crate) mod local_matches;

// 各领域命令模块
mod cache;
mod config;
mod debug;
mod deployer;
mod kugou;
mod library;
mod local;
mod maintenance;
mod media;
mod netease;
mod p2p;
mod playlist;
mod source;
mod storage;
mod lyric;

pub use cache::*;
pub use config::*;
pub use debug::*;
pub use deployer::*;
pub use kugou::*;
pub use library::*;
pub use local::*;
pub use maintenance::*;
pub use media::*;
pub use netease::*;
pub use p2p::*;
pub use playlist::*;
pub use source::*;
pub use storage::*;
pub use lyric::*;