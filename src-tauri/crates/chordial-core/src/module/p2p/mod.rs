//! P2P 资源共享模块 — Chordial 实例之间的对等音乐库共享。
//!
//! # 设计
//!
//! 两台 Chordial 实例通过 TCP + 自定义 JSON 帧协议互相握手；
//! 握手成功后双方各自创建一个 [`P2pSource`] 实例（实现 [`MusicSource`]），
//! 注册到本地 [`SourceRegistrar`]，于是对方的曲库就变成可查询、可播放的来源。
//!
//! # 权限
//!
//! 由本机用户决定向对方暴露的权限（[`Permission`]）：
//! - [`Permission::ReadOnly`]（默认）：对方只能查询、流式播放本库
//! - [`Permission::Editable`]：对方还能向本库推送新歌曲
//!
//! # 发现方式
//!
//! - **不广播**：仅监听 TCP，对方必须知道本机 `IP:端口` + 当前的 6 位匹配码
//! - **广播**：额外向局域网发送 UDP 信标，前端可展示已发现的设备并点击发起握手
//!
//! # 模块布局
//!
//! | 文件 | 职责 |
//! |------|------|
//! | [`protocol`] | 帧定义 / serde / 长度前缀读写 |
//! | [`source`] | `P2pSource` — `MusicSource` 实现 |
//! | [`types`] | 数据模型 / 常量 / 状态容器 |
//! | [`manager`] | `P2pManager` — 生命周期 / 启停 / 广播 / 状态 |
//! | [`handshake`] | 入站 / 出站握手 |
//! | [`peer_io`] | 对端查询循环与处理 |
//! | [`trusted`] | 可信设备管理 |
//! | [`auto`] | 匹配码轮换 + 可信自动连接 |
//! | [`broadcast`] | UDP 信标发送 + 接收 |

pub mod auto;
pub mod broadcast;
pub mod handshake;
pub mod manager;
pub mod peer_io;
pub mod protocol;
pub mod source;
pub mod trusted;
pub mod types;

pub use manager::P2pManager;
pub use protocol::Permission;
pub use source::P2pSource;
pub use types::{P2pEvent, P2pStatus, PeerInfo, TrustedDevice};
