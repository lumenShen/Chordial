//! 酷狗音乐 API 的类型定义。
//!
//! 字段命名严格遵循 KuGouMusicApi 上游响应（酷狗接口多为 PascalCase + 部分 snake_case 混用）。
//! 所有 `#[serde(default)]` 保证部分缺失字段不会导致反序列化失败。
//!
//! 本模块按领域拆分为独立子模块（`song` / `artist` / `album` / `lyric` / `login`），
//! 并通过 `pub use` 统一向外导出，保持外部 API 路径（`types::*`）不变。

#![allow(non_snake_case)]

pub mod album;
pub mod artist;
pub mod login;
pub mod lyric;
pub mod song;

pub use album::*;
pub use artist::*;
pub use login::*;
pub use lyric::*;
pub use song::*;