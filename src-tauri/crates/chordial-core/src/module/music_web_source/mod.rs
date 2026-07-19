//! Web 音乐来源模块 — 提供基于 HTTP 的远程来源基础设施。
//!
//! # 模块架构
//!
//! ```text
//! WebServiceSource (trait)            ← 扩展 MusicSource，提供 HTTP 客户端 + 配置管理
//!   ├── client.rs                     ← reqwest::blocking::Client 封装
//!   └── config.rs                     ← WebSourceConfig 持久化（base_url/cookie/timeout）
//! ```
//!
//! # 设计要点
//!
//! - **同步阻塞 HTTP**：`MusicSource` trait 是同步接口，因此使用 `reqwest::blocking`。
//!   每个来源持有自己的 `Client` 实例（内部自带连接池），避免每次请求新建 TCP/TLS 连接。
//! - **配置持久化**：base_url、cookie、超时等参数通过 [`ConfigStore`] 持久化到
//!   `web_source_<name>.json`，重启后自动恢复。
//! - **统一错误处理**：HTTP 错误、JSON 解析错误、API 业务错误统一转为 `String`。
//!
//! # 已实现子类
//!
//! - [`crate::module::music_netease_source`]: 网易云音乐（NeteaseCloudMusicApi）

pub mod client;
pub mod config;

pub use client::WebHttpClient;
pub use config::WebSourceConfig;
