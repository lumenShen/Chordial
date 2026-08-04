//! Music Source 资源获取命令 — 大文件通过 raw payload 返回前端。

use chordial_core::module::music_source::resource;
use chordial_core::module::music_source::types::SourceId;
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

// ══════════════════════════════════════════════════════════════════════════════
// Music Source 资源获取命令 — 大文件通过 raw payload 返回前端
// ══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn get_song_file(
    ctx: State<'_, Arc<AppContext>>,
    source_id_json: String,
) -> Result<Vec<u8>, String> {
    let source_id: SourceId = serde_json::from_str(&source_id_json)
        .map_err(|e| format!("解析 SourceId 失败: {}", e))?;
    resource::get_song_file(&ctx.registrar, &source_id)
}

#[tauri::command]
pub fn get_album_picture(
    ctx: State<'_, Arc<AppContext>>,
    source_id_json: String,
) -> Result<Vec<u8>, String> {
    let source_id: SourceId = serde_json::from_str(&source_id_json)
        .map_err(|e| format!("解析 SourceId 失败: {}", e))?;
    resource::get_album_picture(&ctx.registrar, &source_id)
}

#[tauri::command]
pub fn get_lyric_text(
    ctx: State<'_, Arc<AppContext>>,
    source_id_json: String,
) -> Result<String, String> {
    let source_id: SourceId = serde_json::from_str(&source_id_json)
        .map_err(|e| format!("解析 SourceId 失败: {}", e))?;
    resource::get_lyric_text(&ctx.registrar, &source_id)
}