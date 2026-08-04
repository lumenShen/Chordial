//! P2P 数据模型与常量 — 供整个 P2P 模块共享的纯类型定义。
//!
//! 本文件不包含任何逻辑，只承载：
//! - 对外暴露的 DTO（`TrustedDevice` / `P2pEvent` / `PeerInfo` / `P2pStatus`）
//! - 管理器内部状态容器（`Inner` / `PeerEntry` / `PendingRequest`）
//! - 共享常量

use crate::module::p2p::broadcast::DiscoveredPeer;
use crate::module::p2p::protocol::{Permission, Frame};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// 可信设备记录 — 用户曾手动同意的 peer，后续自动连接。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedDevice {
    /// 对端持久实例 ID
    pub instance_id: String,
    pub name: String,
    pub addr: String,
    pub permission: Permission,
    pub added_at: u64,
}

pub const TRUSTED_CONFIG_KEY: &str = "p2p.trusted_devices";
pub const INSTANCE_ID_CONFIG_KEY: &str = "p2p.instance_id";
pub const MATCH_CODE_HISTORY_LEN: usize = 10;
pub const MATCH_CODE_ROTATE_INTERVAL: Duration = Duration::from_secs(60);
pub const AUTO_CONNECT_SCAN_INTERVAL: Duration = Duration::from_secs(5);

/// P2P 事件 — 通过外部注入的事件通道送出，由 Tauri 层转发为前端事件。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum P2pEvent {
    /// 收到入站握手请求，等待本机用户确认
    MatchRequested {
        request_id: String,
        peer_addr: String,
        peer_name: String,
    },
    /// 握手完成、对端已连接
    PeerConnected {
        peer_id: String,
        peer_name: String,
        addr: String,
        permission: Permission,
    },
    /// 对端断开
    PeerDisconnected {
        peer_id: String,
        reason: String,
    },
    /// 出站握手结果
    MatchResult {
        request_id: String,
        accepted: bool,
        reason: Option<String>,
    },
    /// 匹配码已自动轮换（防撞码机制，每 60s 一次）
    MatchCodeRotated {
        new_code: String,
    },
    /// 可信设备自动连接成功
    TrustedAutoConnected {
        peer_name: String,
        addr: String,
    },
}

/// 已连接的对端信息（用于 status() 输出）。
#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub addr: String,
    pub permission: Permission,
}

/// 管理器状态快照。
#[derive(Debug, Clone, Serialize)]
pub struct P2pStatus {
    pub listening: bool,
    pub listen_addr: String,
    pub match_code: String,
    pub permission: Permission,
    pub broadcast_enabled: bool,
    pub peers: Vec<PeerInfo>,
    pub discovered: Vec<DiscoveredPeer>,
    pub trusted_devices: Vec<TrustedDevice>,
    pub instance_id: String,
    /// 匹配码下次轮换的 Unix 时间戳（秒）
    pub match_code_rotates_at: u64,
}

/// 已连接对端在管理器内部的生命周期记录。
pub struct PeerEntry {
    pub source: Arc<crate::module::p2p::source::P2pSource>,
    pub name: String,
    pub addr: String,
    pub permission: Permission,
    /// 关闭信号：peer task 收到后退出
    pub shutdown_tx: oneshot::Sender<()>,
}

/// 待本机用户确认的入站握手请求。
pub struct PendingRequest {
    pub reply: oneshot::Sender<bool>,
}

/// 管理器内部可变状态 — 由 `P2pManager::inner` 以 `RwLock` 保护。
pub struct Inner {
    pub listening: bool,
    pub listen_addr: String,
    pub match_code: String,
    pub permission: Permission,
    pub peers: HashMap<String, PeerEntry>,
    pub pending_requests: HashMap<String, PendingRequest>,
    /// 接收方为外部事件订阅者；manager 内部 clone 后发出事件
    pub event_tx: Option<mpsc::UnboundedSender<P2pEvent>>,
    /// 关闭整个监听 accept 循环
    pub server_shutdown: Option<oneshot::Sender<()>>,
    /// 可信设备列表（持久化到 ConfigStore）
    pub trusted_devices: Vec<TrustedDevice>,
    /// 最近用过的匹配码（防撞码）
    pub match_code_history: Vec<String>,
    /// 匹配码下次轮换的 Unix 时间戳（秒）
    pub match_code_rotates_at: u64,
    /// 自动任务（轮换 + 自动连接）关闭信号
    pub auto_task_shutdown: Option<oneshot::Sender<()>>,
    /// 已发起自动连接的 instance_id 集合（防止重复连接）
    pub auto_connecting: HashMap<String, ()>,
}

/// 将 `Inner` 的外层容器（`RwLock`）统一封装，便于各子模块独立持有。
pub type InnerLock = RwLock<Inner>;

/// 当前 Unix 时间戳（秒），用于匹配码轮换计时。
pub fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// Frame tag 调试用（保留在类型层，随协议类型集中展示）
impl Frame {
    pub(crate) fn tag(&self) -> &'static str {
        match self {
            Frame::Hello { .. } => "hello",
            Frame::HelloAck { .. } => "hello_ack",
            Frame::MatchPending { .. } => "match_pending",
            Frame::MatchResult { .. } => "match_result",
            Frame::Query { .. } => "query",
            Frame::Response { .. } => "response",
        }
    }
}