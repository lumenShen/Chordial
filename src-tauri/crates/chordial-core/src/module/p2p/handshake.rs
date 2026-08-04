//! P2P 握手 — 入站 accept 循环、入站握手、出站握手。
//!
//! 独立于 `P2pManager` 生命周期，专注「建立连接」这一阶段；握手成功后
//! 将流交给 [`peer_io`] 接管。

use crate::module::music_source::traits::MusicSource;
use crate::module::p2p::manager::P2pManager;
use crate::module::p2p::protocol::{read_frame, write_frame, Frame, PROTOCOL_VERSION};
use crate::module::p2p::types::{PendingRequest, P2pEvent};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use uuid::Uuid;

impl P2pManager {
    // ── 入站 accept 循环 ──────────────────────────

    pub(crate) async fn accept_loop(
        self: Arc<Self>,
        listener: TcpListener,
        mut shutdown: oneshot::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    eprintln!("[p2p] accept 循环收到关闭信号");
                    return;
                }
                accept = listener.accept() => {
                    let (stream, peer_addr) = match accept {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("[p2p] accept 失败: {e}");
                            continue;
                        }
                    };
                    let this = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = this.incoming_handshake(stream, peer_addr).await {
                            eprintln!("[p2p] 入站握手失败 {}: {e}", peer_addr);
                        }
                    });
                }
            }
        }
    }

    async fn incoming_handshake(
        self: Arc<Self>,
        mut stream: TcpStream,
        peer_addr: SocketAddr,
    ) -> Result<(), String> {
        // 1. 读 Hello
        let (match_code, client_name, client_instance_id) =
            match read_frame(&mut stream).await.map_err(|e| e.to_string())? {
                Frame::Hello { version, match_code, client_name, instance_id } => {
                    if version != PROTOCOL_VERSION {
                        let perm = self.inner.read().permission;
                        let _ = write_frame(
                            &mut stream,
                            &Frame::HelloAck {
                                accepted: false,
                                server_name: self.server_name.clone(),
                                offered_permission: perm,
                                session_id: String::new(),
                                reason: Some(format!(
                                    "协议版本不匹配: 本机 {PROTOCOL_VERSION}, 对端 {version}"
                                )),
                            },
                        )
                        .await;
                        return Err(format!("协议版本不匹配: {version}"));
                    }
                    (match_code, client_name, instance_id)
                }
                _ => return Err("期望 Hello 帧".into()),
            };

        // 2. 检查是否为可信设备（by instance_id）→ 自动接受，跳过匹配码 + 用户确认
        let is_trusted = client_instance_id
            .as_ref()
            .map(|id| self.inner.read().trusted_devices.iter().any(|t| t.instance_id == *id))
            .unwrap_or(false);

        let accepted = if is_trusted {
            // 可信设备：直接接受，无需匹配码校验和用户确认
            true
        } else {
            // 3. 非可信：校验匹配码
            let code_ok = match_code == self.inner.read().match_code;
            if !code_ok {
                let perm = self.inner.read().permission;
                let _ = write_frame(
                    &mut stream,
                    &Frame::HelloAck {
                        accepted: false,
                        server_name: self.server_name.clone(),
                        offered_permission: perm,
                        session_id: String::new(),
                        reason: Some("匹配码错误".into()),
                    },
                )
                .await;
                return Err(format!("匹配码错误 (对端 {})", match_code));
            }

            // 4. 请求本机用户确认
            let request_id = Uuid::new_v4().to_string();
            let (tx, rx) = oneshot::channel::<bool>();
            self.inner.write().pending_requests.insert(
                request_id.clone(),
                PendingRequest { reply: tx },
            );
            self.emit(P2pEvent::MatchRequested {
                request_id: request_id.clone(),
                peer_addr: peer_addr.to_string(),
                peer_name: client_name.clone(),
            });

            let _ = write_frame(
                &mut stream,
                &Frame::MatchPending {
                    request_id: request_id.clone(),
                    server_name: self.server_name.clone(),
                },
            )
            .await;

            // 等待本机用户响应（超时 60 秒）
            match tokio::time::timeout(Duration::from_secs(60), rx).await {
                Ok(Ok(b)) => {
                    if !b {
                        let _ = write_frame(
                            &mut stream,
                            &Frame::MatchResult {
                                request_id: request_id.clone(),
                                accepted: false,
                                permission: None,
                                session_id: None,
                                reason: Some("本机用户拒绝".into()),
                                server_instance_id: None,
                            },
                        )
                        .await;
                        self.emit(P2pEvent::MatchResult {
                            request_id,
                            accepted: false,
                            reason: Some("本机用户拒绝".into()),
                        });
                        return Ok(());
                    }
                    true
                }
                _ => {
                    self.inner.write().pending_requests.remove(&request_id);
                    let _ = write_frame(
                        &mut stream,
                        &Frame::MatchResult {
                            request_id: request_id.clone(),
                            accepted: false,
                            permission: None,
                            session_id: None,
                            reason: Some("超时或取消".into()),
                            server_instance_id: None,
                        },
                    )
                    .await;
                    return Err("等待用户确认超时".into());
                }
            }
        };

        if !accepted {
            return Ok(());
        }

        // 5. 接受 → 发送 MatchResult（含本机 instance_id），开始对端 IO
        let permission = self.inner.read().permission;
        let session_id = Uuid::new_v4().to_string();
        let _ = write_frame(
            &mut stream,
            &Frame::MatchResult {
                request_id: String::new(),
                accepted: true,
                permission: Some(permission),
                session_id: Some(session_id.clone()),
                reason: None,
                server_instance_id: Some(self.instance_id.clone()),
            },
        )
        .await;

        self.emit(P2pEvent::MatchResult {
            request_id: String::new(),
            accepted: true,
            reason: None,
        });

        // 6. 启动对端 IO 任务 + 注册 P2pSource
        self.spawn_peer_task(
            stream,
            session_id,
            client_name.clone(),
            peer_addr.to_string(),
            permission,
            /* incoming = */ true,
        )
        .await;

        Ok(())
    }

    // ── 出站握手 ──────────────────────────────────

    pub(crate) async fn outgoing_handshake(
        self: Arc<Self>,
        addr: String,
        match_code: String,
    ) -> Result<(), String> {
        let mut stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| format!("连接 {addr} 失败: {e}"))?;

        // 发 Hello（携带本机 instance_id，对端可据此判断可信）
        write_frame(
            &mut stream,
            &Frame::Hello {
                version: PROTOCOL_VERSION,
                match_code,
                client_name: self.server_name.clone(),
                instance_id: Some(self.instance_id.clone()),
            },
        )
        .await
        .map_err(|e| format!("发送 Hello 失败: {e}"))?;

        // 读回复：可能是 HelloAck（对端自动接受/拒绝）或 MatchPending + MatchResult
        let mut peer_name = String::from("unknown");
        loop {
            let frame = read_frame(&mut stream).await.map_err(|e| format!("读握手响应失败: {e}"))?;
            match frame {
                Frame::HelloAck { accepted, server_name, reason, .. } => {
                    peer_name = server_name;
                    if !accepted {
                        return Err(reason.unwrap_or_else(|| "对端拒绝握手".into()));
                    }
                    break;
                }
                Frame::MatchPending { server_name, .. } => {
                    peer_name = server_name;
                }
                Frame::MatchResult { accepted, permission, session_id, reason, server_instance_id, .. } => {
                    if !accepted {
                        return Err(reason.unwrap_or_else(|| "对端用户拒绝".into()));
                    }
                    let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
                    let permission = permission.unwrap_or(crate::module::p2p::protocol::Permission::ReadOnly);
                    // 收到对端 instance_id → 存为可信设备（自动信任）
                    if let Some(ref sid) = server_instance_id {
                        self.add_trusted_internal(sid.clone(), peer_name.clone(), addr.clone(), permission);
                    }
                    self.spawn_peer_task(
                        stream,
                        session_id,
                        peer_name,
                        addr,
                        permission,
                        /* incoming = */ false,
                    )
                    .await;
                    return Ok(());
                }
                other => {
                    return Err(format!("握手期间收到非预期帧: {:?}", other.tag()));
                }
            }
        }

        // 对端用 HelloAck 直接接受（无 MatchResult，无 instance_id 交换）
        let session_id = Uuid::new_v4().to_string();
        let permission = self.inner.read().permission;
        self.spawn_peer_task(
            stream,
            session_id,
            peer_name,
            addr,
            permission,
            /* incoming = */ false,
        )
        .await;
        Ok(())
    }

    /// 主动发起握手。
    pub fn request_match(self: &Arc<Self>, addr: String, match_code: String) -> Result<(), String> {
        let this = self.clone();
        self.runtime.spawn(async move {
            let result = Self::outgoing_handshake(this.clone(), addr.clone(), match_code).await;
            if let Err(e) = result {
                eprintln!("[p2p] 出站握手失败 {addr}: {e}");
                // 出站握手没有 request_id（不需要本机用户确认），直接以失败事件回报
                this.emit(P2pEvent::MatchResult {
                    request_id: String::new(),
                    accepted: false,
                    reason: Some(e),
                });
            }
        });
        Ok(())
    }

    /// 响应入站握手请求。
    pub fn respond_match(&self, request_id: String, accepted: bool) -> Result<(), String> {
        let req = self.inner.write().pending_requests.remove(&request_id);
        match req {
            Some(r) => {
                let _ = r.reply.send(accepted);
                if !accepted {
                    self.emit(P2pEvent::MatchResult {
                        request_id,
                        accepted: false,
                        reason: Some("本机用户拒绝".into()),
                    });
                }
                Ok(())
            }
            None => Err(format!("未找到匹配请求 {request_id}")),
        }
    }

    /// 断开指定对端。
    pub fn disconnect_peer(&self, peer_id: String) {
        let entry = self.inner.write().peers.remove(&peer_id);
        if let Some(e) = entry {
            let _ = e.shutdown_tx.send(());
            self.registrar.unregister(&e.source.name());
            e.source.mark_closed();
            self.emit(P2pEvent::PeerDisconnected {
                peer_id,
                reason: "本机用户断开".into(),
            });
        }
    }
}