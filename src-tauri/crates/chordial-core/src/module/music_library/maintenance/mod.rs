//! 维护方法 — 去重、引用修复、空实体清理等库维护操作。
//!
//! # 子模块
//!
//! ```text
//! source_cleanup.rs   ← 移除来源 / 删除本地文件 / 精准移除 / 清理空实体
//! dedup.rs            ← 手动去重（保留音质最高版本）
//! repair.rs           ← 一次性修复无效引用（artist / album）
//! ```

pub(super) mod dedup;
pub(super) mod repair;
pub(super) mod source_cleanup;