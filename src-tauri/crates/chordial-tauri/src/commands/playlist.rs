//! 歌单命令 — 本地歌单 CRUD + 曲目管理。

use chordial_core::module::playlist::{self, Playlist};
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

/// 列出所有本地保存的歌单。
#[tauri::command]
pub fn playlist_list(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<Playlist>, String> {
    Ok(playlist::get_all(&ctx.store))
}

/// 获取指定歌单（含完整曲目详情）。
#[tauri::command]
pub fn playlist_get(
    ctx: State<'_, Arc<AppContext>>,
    id: String,
) -> Result<Option<serde_json::Value>, String> {
    let pl = playlist::get(&ctx.store, &id);
    if let Some(pl) = pl {
        let songs = ctx.library.get_songs_by_ids(&pl.song_ids);
        Ok(Some(serde_json::json!({
            "playlist": pl,
            "songs": songs,
        })))
    } else {
        Ok(None)
    }
}

/// 创建新的本地空歌单。
#[tauri::command]
pub fn playlist_create(
    ctx: State<'_, Arc<AppContext>>,
    name: String,
    description: Option<String>,
) -> Result<Playlist, String> {
    let mut pl = Playlist::new("local", &name);
    pl.description = description;
    playlist::add(&ctx.store, &pl)?;
    ctx.store.save()?;
    Ok(pl)
}

/// 更新歌单元信息（name / description / tags 等）。
#[tauri::command]
pub fn playlist_update(
    ctx: State<'_, Arc<AppContext>>,
    playlist: Playlist,
) -> Result<(), String> {
    playlist::update(&ctx.store, &playlist)?;
    ctx.store.save()
}

/// 删除歌单。
#[tauri::command]
pub fn playlist_delete(
    ctx: State<'_, Arc<AppContext>>,
    id: String,
) -> Result<bool, String> {
    let removed = playlist::remove(&ctx.store, &id);
    ctx.store.save()?;
    Ok(removed)
}

/// 向歌单追加歌曲（自动去重）。
#[tauri::command]
pub fn playlist_add_song(
    ctx: State<'_, Arc<AppContext>>,
    playlist_id: String,
    song_id: String,
) -> Result<(), String> {
    playlist::add_song(&ctx.store, &playlist_id, &song_id)?;
    ctx.store.save()
}

/// 从歌单移除歌曲。
#[tauri::command]
pub fn playlist_remove_song(
    ctx: State<'_, Arc<AppContext>>,
    playlist_id: String,
    song_id: String,
) -> Result<(), String> {
    playlist::remove_song(&ctx.store, &playlist_id, &song_id)?;
    ctx.store.save()
}

/// 调整歌单内歌曲顺序。
///
/// `new_order` 为新的 song_id 顺序，长度必须与原 song_ids 一致。
#[tauri::command]
pub fn playlist_reorder_songs(
    ctx: State<'_, Arc<AppContext>>,
    playlist_id: String,
    new_order: Vec<String>,
) -> Result<(), String> {
    playlist::reorder_songs(&ctx.store, &playlist_id, new_order)?;
    ctx.store.save()
}

/// 获取歌单总数（用于侧边栏 badge 显示）。
#[tauri::command]
pub fn playlist_count(ctx: State<'_, Arc<AppContext>>) -> Result<usize, String> {
    Ok(playlist::count(&ctx.store))
}