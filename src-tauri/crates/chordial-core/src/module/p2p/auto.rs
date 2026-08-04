//! 自动任务 — 匹配码轮换 + 可信设备自动连接扫描。

use crate::module::p2p::manager::P2pManager;
use crate::module::p2p::types::{
    P2pEvent, AUTO_CONNECT_SCAN_INTERVAL, MATCH_CODE_ROTATE_INTERVAL,
};
use std::sync::Arc;
use tokio::sync::oneshot;

impl P2pManager {
    pub(crate) fn start_auto_tasks(self: &Arc<Self>) {
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        self.inner.write().auto_task_shutdown = Some(shutdown_tx);

        let this = self.clone();
        self.runtime.spawn(async move {
            Self::auto_task_loop(this, shutdown_rx).await;
        });
    }

    async fn auto_task_loop(self: Arc<Self>, mut shutdown: oneshot::Receiver<()>) {
        let mut code_timer = tokio::time::interval(MATCH_CODE_ROTATE_INTERVAL);
        code_timer.tick().await; // 跳过首次立即触发
        let mut connect_timer = tokio::time::interval(AUTO_CONNECT_SCAN_INTERVAL);
        connect_timer.tick().await;

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    eprintln!("[p2p] 自动任务收到关闭信号");
                    return;
                }
                _ = code_timer.tick() => {
                    // 匹配码轮换
                    let new_code = self.regenerate_match_code();
                    let code_for_log = new_code.clone();
                    self.emit(P2pEvent::MatchCodeRotated { new_code });
                    eprintln!("[p2p] 匹配码已轮换: {code_for_log}");
                }
                _ = connect_timer.tick() => {
                    // 可信设备自动连接扫描
                    Arc::clone(&self).auto_connect_scan().await;
                }
            }
        }
    }

    /// 扫描已发现的广播 peer，对可信设备自动发起握手。
    async fn auto_connect_scan(self: Arc<Self>) {
        let discovered = self.discovery.list_discovered();

        for peer in discovered {
            // 必须有 instance_id 才能匹配可信
            let Some(ref peer_iid) = peer.instance_id else { continue };

            // 检查是否可信
            let is_trusted = {
                let g = self.inner.read();
                g.trusted_devices.iter().any(|t| t.instance_id == *peer_iid)
            };
            if !is_trusted { continue; }

            // 检查是否已连接
            let already_connected = {
                let g = self.inner.read();
                g.peers.values().any(|e| e.name == peer.name)
            };
            if already_connected { continue; }

            // 检查是否已在连接中
            let already_connecting = {
                let mut g = self.inner.write();
                if g.auto_connecting.contains_key(peer_iid) {
                    true
                } else {
                    g.auto_connecting.insert(peer_iid.clone(), ());
                    false
                }
            };
            if already_connecting { continue; }

            let addr = format!("{}:{}", peer.addr, peer.port);
            let match_code = peer.match_code.clone();
            let peer_name = peer.name.clone();
            let iid = peer_iid.clone();
            let this = self.clone();

            eprintln!("[p2p] 自动连接可信设备: {peer_name} ({addr})");
            self.runtime.spawn(async move {
                let result = Self::outgoing_handshake(this.clone(), addr.clone(), match_code).await;
                this.inner.write().auto_connecting.remove(&iid);
                if result.is_ok() {
                    this.emit(P2pEvent::TrustedAutoConnected {
                        peer_name,
                        addr,
                    });
                } else if let Err(e) = result {
                    eprintln!("[p2p] 自动连接失败 {addr}: {e}");
                }
            });
        }
    }
}