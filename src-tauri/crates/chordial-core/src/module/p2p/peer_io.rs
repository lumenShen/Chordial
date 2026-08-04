//! P2P 对端 IO — 对端任务创建、查询循环与查询处理。
//!
//! 握手成功后，由本模块接管 TCP 流：转发本地 `P2pSource` 的查询、回应对端
//! 查询，并在断开时清理注册表与事件。

use crate::module::music_library::library::MusicLibrary;
use crate::module::music_library::models::Song;
use crate::module::music_source::registrar::SourceRegistrar;
use crate::module::music_source::resource as src_resource;
use crate::module::music_source::traits::MusicSource;
use crate::module::music_source::types::{EntityType, SourceType};
use crate::module::p2p::manager::P2pManager;
use crate::module::p2p::protocol::{write_frame, Frame, Op, Permission};
use crate::module::p2p::source::{PeerCommand, P2pSource};
use crate::module::p2p::types::{PeerEntry, P2pEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

impl P2pManager {
    // ── 对端 IO 任务 ─────────────────────────────

    pub(crate) async fn spawn_peer_task(
        self: Arc<Self>,
        stream: TcpStream,
        session_id: String,
        peer_name: String,
        peer_addr: String,
        permission: Permission,
        _incoming: bool,
    ) {
        let source_name = format!("p2p-{}", &session_id[..8.min(session_id.len())]);
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<PeerCommand>();
        let runtime = self.runtime.handle().clone();
        let source = P2pSource::new(
            source_name.clone(),
            peer_name.clone(),
            permission,
            cmd_tx,
            runtime,
        );

        // 注册到 SourceRegistrar
        if let Err(e) = self.registrar.register(source.clone()) {
            eprintln!("[p2p] 注册 P2pSource 失败: {e}");
            return;
        }

        let peer_id = session_id.clone();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        self.inner.write().peers.insert(
            peer_id.clone(),
            PeerEntry {
                source: source.clone(),
                name: peer_name.clone(),
                addr: peer_addr.clone(),
                permission,
                shutdown_tx,
            },
        );

        self.emit(P2pEvent::PeerConnected {
            peer_id: peer_id.clone(),
            peer_name: peer_name.clone(),
            addr: peer_addr.clone(),
            permission,
        });

        let library = self.library.clone();
        let registrar = self.registrar.clone();
        let offered_permission = self.inner.read().permission;
        let pid_for_task = peer_id.clone();
        let this_for_task = self.clone();

        self.runtime.spawn(async move {
            Self::peer_io_loop(
                this_for_task,
                pid_for_task,
                source,
                stream,
                cmd_rx,
                shutdown_rx,
                library,
                registrar,
                offered_permission,
            )
            .await;
        });
    }

    async fn peer_io_loop(
        self: Arc<Self>,
        peer_id: String,
        source: Arc<P2pSource>,
        stream: TcpStream,
        mut cmd_rx: mpsc::UnboundedReceiver<PeerCommand>,
        mut shutdown: oneshot::Receiver<()>,
        library: Arc<MusicLibrary>,
        registrar: Arc<SourceRegistrar>,
        offered_permission: Permission,
    ) {
        let (mut reader, mut writer) = stream.into_split();
        let mut pending: HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>> =
            HashMap::new();
        let mut next_query_id: u64 = 1;

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    eprintln!("[p2p:{}] 收到关闭信号", peer_id);
                    break;
                }
                // 本地 P2pSource 发来一条查询
                cmd = cmd_rx.recv() => {
                    let Some(cmd) = cmd else { break; };
                    let id = next_query_id;
                    next_query_id += 1;
                    pending.insert(id, cmd.reply);
                    if write_frame(&mut writer, &Frame::Query { id, op: cmd.op }).await.is_err() {
                        if let Some(tx) = pending.remove(&id) {
                            let _ = tx.send(Err("发送查询失败".into()));
                        }
                        break;
                    }
                }
                // 对端发来一帧
                frame_result = crate::module::p2p::protocol::read_frame(&mut reader) => {
                    let frame = match frame_result {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("[p2p:{}] 读帧失败: {e}", peer_id);
                            break;
                        }
                    };
                    match frame {
                        Frame::Response { id, ok, data, error } => {
                            let res = if ok { Ok(data) } else { Err(error.unwrap_or_else(|| "未知错误".into())) };
                            if let Some(tx) = pending.remove(&id) {
                                let _ = tx.send(res);
                            }
                        }
                        Frame::Query { id, op } => {
                            let resp = Self::handle_query(&library, &registrar, offered_permission, op).await;
                            let (ok, data, error) = match resp {
                                Ok(v) => (true, v, None),
                                Err(e) => (false, serde_json::Value::Null, Some(e)),
                            };
                            if write_frame(&mut writer, &Frame::Response { id, ok, data, error }).await.is_err() {
                                break;
                            }
                        }
                        _ => {
                            eprintln!("[p2p:{}] 握手后收到非查询帧，忽略", peer_id);
                        }
                    }
                }
            }
        }

        // 清理
        source.mark_closed();
        self.registrar.unregister(&source.name());
        self.inner.write().peers.remove(&peer_id);
        self.emit(P2pEvent::PeerDisconnected {
            peer_id,
            reason: "连接断开".into(),
        });
    }

    /// 处理对端发来的查询 — 操作本地库 / 来源系统。
    async fn handle_query(
        library: &MusicLibrary,
        registrar: &SourceRegistrar,
        offered_permission: Permission,
        op: Op,
    ) -> Result<serde_json::Value, String> {
        use base64::{engine::general_purpose::STANDARD as B64, Engine};
        match op {
            Op::SearchSongs { query } => {
                let songs = library.search_songs(&query);
                serde_json::to_value(songs).map_err(|e| e.to_string())
            }
            Op::GetSong { id } => {
                let s = library.get_song(&id);
                serde_json::to_value(s).map_err(|e| e.to_string())
            }
            Op::GetArtist { id } => {
                let a = library.get_artist(&id);
                serde_json::to_value(a).map_err(|e| e.to_string())
            }
            Op::GetAlbum { id } => {
                let a = library.get_album(&id);
                serde_json::to_value(a).map_err(|e| e.to_string())
            }
            Op::GetLyric { song_id } => {
                let l = library.get_lyric_of_song(&song_id);
                serde_json::to_value(l).map_err(|e| e.to_string())
            }
            Op::SongFileGet { entity_id } => {
                // entity_id 是本库内 song UUID；找一个 Local 来源的 source_id 取音频
                let song = library.get_song(&entity_id)
                    .ok_or_else(|| format!("歌曲 {entity_id} 不存在"))?;
                let local_sid = song.source_ids.iter().find(|s| s.source_type == SourceType::Local)
                    .ok_or_else(|| "该歌曲无可流式播放的本地来源".to_string())?;
                let bytes = src_resource::get_song_file(registrar, local_sid)?;
                Ok(serde_json::json!({ "bytes": B64.encode(&bytes) }))
            }
            Op::AlbumPictureGet { entity_id } => {
                let album = library.get_album(&entity_id)
                    .ok_or_else(|| format!("专辑 {entity_id} 不存在"))?;
                let local_sid = album.source_ids.iter().find(|s| s.source_type == SourceType::Local)
                    .ok_or_else(|| "该专辑无本地来源".to_string())?;
                let bytes = src_resource::get_album_picture(registrar, local_sid)?;
                Ok(serde_json::json!({ "bytes": B64.encode(&bytes) }))
            }
            Op::LyricTextGet { song_id } => {
                let sids = library.get_source_ids_of_song(&song_id);
                let local_sid = sids.iter().find(|s| s.source_type == SourceType::Local && s.entity_type == EntityType::Song)
                    .ok_or_else(|| "该歌曲无本地来源".to_string())?;
                let text = src_resource::get_lyric_text(registrar, local_sid)?;
                Ok(serde_json::json!({ "text": text }))
            }
            Op::AddSong { song } => {
                if offered_permission != Permission::Editable {
                    return Err("本机未开放可编辑权限".into());
                }
                let song: Song = serde_json::from_value(song).map_err(|e| e.to_string())?;
                let stored_id = library.add_song(&song)?;
                library.save_if_dirty()?;
                Ok(serde_json::json!({ "id": stored_id }))
            }
            Op::GetAllSongs => {
                let songs = library.get_all_songs();
                let v: Vec<Song> = songs.into_values().collect();
                serde_json::to_value(v).map_err(|e| e.to_string())
            }
        }
    }
}