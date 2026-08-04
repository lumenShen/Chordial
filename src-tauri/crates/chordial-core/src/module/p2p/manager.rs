//! `P2pManager` — P2P 共享的顶层控制中心（生命周期与状态入口）。
//!
//! 本文件只保留管理器结构体与「启停 / 状态 / 广播 / 匹配码」等生命周期方法；
//! 握手、对端 IO、可信设备、自动任务分别收敛到：
//! - [`handshake`]：入站 / 出站握手
//! - [`peer_io`]：对端查询循环
//! - [`trusted`]：可信设备管理
//! - [`auto`]：匹配码轮换与可信设备自动连接

use crate::module::config::store::ConfigStore;
use crate::module::music_library::library::MusicLibrary;
use crate::module::music_source::registrar::SourceRegistrar;
use crate::module::music_source::traits::MusicSource;
use crate::module::p2p::broadcast::{Beacon, BroadcastDiscovery};
use crate::module::p2p::protocol::{generate_match_code, generate_match_code_excluding, Permission, DEFAULT_PORT};
use crate::module::p2p::types::{
    Inner, InnerLock, P2pEvent, P2pStatus, PeerInfo, TrustedDevice, INSTANCE_ID_CONFIG_KEY,
    TRUSTED_CONFIG_KEY, MATCH_CODE_HISTORY_LEN, now_unix_secs,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

/// P2P 管理器。
pub struct P2pManager {
    pub(crate) library: Arc<MusicLibrary>,
    pub(crate) registrar: Arc<SourceRegistrar>,
    pub(crate) config: Arc<ConfigStore>,
    pub(crate) server_name: String,
    pub(crate) instance_id: String,
    pub(crate) inner: InnerLock,
    pub(crate) discovery: BroadcastDiscovery,
    pub(crate) runtime: Runtime,
}

impl P2pManager {
    /// 创建管理器。不会立即启动监听。
    pub fn new(
        library: Arc<MusicLibrary>,
        registrar: Arc<SourceRegistrar>,
        config: Arc<ConfigStore>,
    ) -> Arc<Self> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("p2p-runtime")
            .build()
            .expect("构建 P2P tokio 运行时失败");

        let server_name = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "chordial".to_string());
        let server_name = format!("{}-{}", server_name, &Uuid::new_v4().to_string()[..4]);

        // 持久化 instance_id（首次生成后存入 ConfigStore）
        let instance_id: String = config
            .get::<String>(INSTANCE_ID_CONFIG_KEY)
            .unwrap_or_else(|| {
                let id = Uuid::new_v4().to_string();
                let _ = config.set(INSTANCE_ID_CONFIG_KEY, &id);
                id
            });

        // 加载可信设备列表
        let trusted_devices: Vec<TrustedDevice> = config
            .get::<Vec<TrustedDevice>>(TRUSTED_CONFIG_KEY)
            .unwrap_or_default();

        let now = now_unix_secs();
        let initial_code = generate_match_code();

        let inner = Inner {
            listening: false,
            listen_addr: String::new(),
            match_code: initial_code.clone(),
            permission: Permission::ReadOnly,
            peers: Default::default(),
            pending_requests: Default::default(),
            event_tx: None,
            server_shutdown: None,
            trusted_devices,
            match_code_history: vec![initial_code],
            match_code_rotates_at: now + 60,
            auto_task_shutdown: None,
            auto_connecting: Default::default(),
        };

        Arc::new(Self {
            library,
            registrar,
            config,
            server_name,
            instance_id,
            inner: InnerLock::new(inner),
            discovery: BroadcastDiscovery::new(),
            runtime,
        })
    }

    /// 注入事件通道 — Tauri 层用此通道消费事件并转发为前端事件。
    pub fn set_event_channel(self: &Arc<Self>, tx: mpsc::UnboundedSender<P2pEvent>) {
        self.inner.write().event_tx = Some(tx);
    }

    pub(crate) fn emit(&self, evt: P2pEvent) {
        if let Some(tx) = self.inner.read().event_tx.as_ref() {
            let _ = tx.send(evt);
        }
    }

    /// 启动共享服务。
    pub fn start_server(
        self: &Arc<Self>,
        broadcast: bool,
        permission: Permission,
    ) -> Result<(), String> {
        let mut g = self.inner.write();
        if g.listening {
            return Err("服务已在运行".into());
        }
        g.permission = permission;

        let listener = self
            .runtime
            .block_on(async move { TcpListener::bind(("0.0.0.0", DEFAULT_PORT)).await })
            .map_err(|e| format!("绑定端口 {DEFAULT_PORT} 失败: {e}"))?;

        let local_addr = listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| format!("0.0.0.0:{DEFAULT_PORT}"));

        g.listen_addr = local_addr.clone();
        g.listening = true;

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        g.server_shutdown = Some(shutdown_tx);

        // 入站 accept 循环
        let this = self.clone();
        self.runtime.spawn(async move {
            let _ = Self::accept_loop(this, listener, shutdown_rx).await;
        });

        drop(g);

        // 广播
        if broadcast {
            let _ = self.start_broadcast();
        }

        // 启动自动任务：匹配码轮换 + 可信设备自动连接
        self.start_auto_tasks();

        Ok(())
    }

    /// 停止共享服务。
    pub fn stop_server(self: &Arc<Self>) {
        let mut g = self.inner.write();
        g.listening = false;
        g.listen_addr.clear();
        if let Some(tx) = g.server_shutdown.take() {
            let _ = tx.send(());
        }
        // 停止自动任务
        if let Some(tx) = g.auto_task_shutdown.take() {
            let _ = tx.send(());
        }
        g.auto_connecting.clear();
        // 关闭所有对端
        let peers: Vec<(String, crate::module::p2p::types::PeerEntry)> = g.peers.drain().collect();
        let pending: Vec<(String, crate::module::p2p::types::PendingRequest)> =
            g.pending_requests.drain().collect();
        drop(g);

        for (id, entry) in peers {
            let _ = entry.shutdown_tx.send(());
            self.registrar.unregister(&entry.source.name());
            entry.source.mark_closed();
            self.emit(P2pEvent::PeerDisconnected {
                peer_id: id,
                reason: "服务停止".into(),
            });
        }
        for (_, req) in pending {
            let _ = req.reply.send(false);
        }

        self.stop_broadcast();
    }

    /// 设置本机共享权限（影响后续握手；已连接对端不变）。
    pub fn set_permission(&self, permission: Permission) {
        self.inner.write().permission = permission;
    }

    /// 重新生成匹配码（使用防撞历史）。
    pub fn regenerate_match_code(&self) -> String {
        let mut g = self.inner.write();
        let code = generate_match_code_excluding(&g.match_code_history);
        g.match_code = code.clone();
        g.match_code_history.push(code.clone());
        if g.match_code_history.len() > MATCH_CODE_HISTORY_LEN {
            g.match_code_history.remove(0);
        }
        g.match_code_rotates_at = now_unix_secs() + 60;
        let port = g
            .listen_addr
            .rsplit(':')
            .next()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT);
        let beacon_name = self.server_name.clone();
        let instance_id = self.instance_id.clone();
        drop(g);

        // 同步广播信标
        if self.discovery.is_enabled() {
            self.discovery.update_beacon(Beacon {
                name: beacon_name,
                port,
                match_code: code.clone(),
                instance_id: Some(instance_id),
            });
        }
        code
    }

    /// 切换广播发现。
    pub fn set_broadcast(self: &Arc<Self>, enabled: bool) -> Result<(), String> {
        if enabled {
            self.start_broadcast()
        } else {
            self.stop_broadcast();
            Ok(())
        }
    }

    fn start_broadcast(self: &Arc<Self>) -> Result<(), String> {
        let g = self.inner.read();
        let port = g
            .listen_addr
            .rsplit(':')
            .next()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT);
        let code = g.match_code.clone();
        drop(g);
        let beacon = Beacon {
            name: self.server_name.clone(),
            port,
            match_code: code,
            instance_id: Some(self.instance_id.clone()),
        };
        self.runtime
            .block_on(self.discovery.enable(beacon))
            .map_err(|e| format!("启用广播失败: {e}"))
    }

    fn stop_broadcast(&self) {
        self.discovery.disable();
    }

    /// 获取匹配载荷（用于二维码生成）。
    pub fn get_match_payload(&self) -> serde_json::Value {
        let g = self.inner.read();
        let port = g
            .listen_addr
            .rsplit(':')
            .next()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT);
        serde_json::json!({
            "instance_id": self.instance_id,
            "name": self.server_name,
            "port": port,
            "match_code": g.match_code,
            "listen_addr": g.listen_addr,
        })
    }

    /// 当前状态快照。
    pub fn status(&self) -> P2pStatus {
        let g = self.inner.read();
        P2pStatus {
            listening: g.listening,
            listen_addr: g.listen_addr.clone(),
            match_code: g.match_code.clone(),
            permission: g.permission,
            broadcast_enabled: self.discovery.is_enabled(),
            peers: g
                .peers
                .iter()
                .map(|(id, e)| PeerInfo {
                    id: id.clone(),
                    name: e.name.clone(),
                    addr: e.addr.clone(),
                    permission: e.permission,
                })
                .collect(),
            discovered: self.discovery.list_discovered(),
            trusted_devices: g.trusted_devices.clone(),
            instance_id: self.instance_id.clone(),
            match_code_rotates_at: g.match_code_rotates_at,
        }
    }
}