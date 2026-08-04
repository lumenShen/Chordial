//! P2P 资源共享命令。

use chordial_core::module::p2p::{Permission, P2pStatus, TrustedDevice};
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn p2p_status(ctx: State<'_, Arc<AppContext>>) -> Result<P2pStatus, String> {
    Ok(ctx.p2p.status())
}

#[tauri::command]
pub fn p2p_start_server(
    ctx: State<'_, Arc<AppContext>>,
    broadcast: bool,
    permission: String,
) -> Result<(), String> {
    let perm = match permission.as_str() {
        "editable" => Permission::Editable,
        _ => Permission::ReadOnly,
    };
    ctx.p2p.start_server(broadcast, perm)
}

#[tauri::command]
pub fn p2p_stop_server(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.p2p.stop_server();
    Ok(())
}

#[tauri::command]
pub fn p2p_request_match(
    ctx: State<'_, Arc<AppContext>>,
    addr: String,
    match_code: String,
) -> Result<(), String> {
    ctx.p2p.request_match(addr, match_code)
}

#[tauri::command]
pub fn p2p_respond_match(
    ctx: State<'_, Arc<AppContext>>,
    request_id: String,
    accepted: bool,
) -> Result<(), String> {
    ctx.p2p.respond_match(request_id, accepted)
}

#[tauri::command]
pub fn p2p_disconnect_peer(
    ctx: State<'_, Arc<AppContext>>,
    peer_id: String,
) -> Result<(), String> {
    ctx.p2p.disconnect_peer(peer_id);
    Ok(())
}

#[tauri::command]
pub fn p2p_set_permission(
    ctx: State<'_, Arc<AppContext>>,
    permission: String,
) -> Result<(), String> {
    let perm = match permission.as_str() {
        "editable" => Permission::Editable,
        _ => Permission::ReadOnly,
    };
    ctx.p2p.set_permission(perm);
    Ok(())
}

#[tauri::command]
pub fn p2p_set_broadcast(
    ctx: State<'_, Arc<AppContext>>,
    enabled: bool,
) -> Result<(), String> {
    ctx.p2p.set_broadcast(enabled)
}

#[tauri::command]
pub fn p2p_regenerate_match_code(ctx: State<'_, Arc<AppContext>>) -> Result<String, String> {
    Ok(ctx.p2p.regenerate_match_code())
}

// ── 可信设备管理 ─────────────────────────────────────

#[tauri::command]
pub fn p2p_list_trusted(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<TrustedDevice>, String> {
    Ok(ctx.p2p.list_trusted())
}

#[tauri::command]
pub fn p2p_add_trusted(
    ctx: State<'_, Arc<AppContext>>,
    device: TrustedDevice,
) -> Result<(), String> {
    ctx.p2p.add_trusted(device)
}

#[tauri::command]
pub fn p2p_remove_trusted(
    ctx: State<'_, Arc<AppContext>>,
    instance_id: String,
) -> Result<bool, String> {
    ctx.p2p.remove_trusted(&instance_id)
}

/// 获取匹配载荷（用于前端二维码生成）。
#[tauri::command]
pub fn p2p_get_match_payload(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    Ok(ctx.p2p.get_match_payload())
}