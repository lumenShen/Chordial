//! 网易云在线搜索命令（歌曲/歌手/专辑/歌单）。

use chordial_core::module::music_library::models::{Album, Artist, Song};
use chordial_core::module::music_netease_source::NeteasePlaylistBrief;
use chordial_core::module::music_source::traits::MusicSource;
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

/// 在线搜索歌曲（返回库内 Song 模型，附带 netease SourceId）。
#[tauri::command]
pub fn netease_search_songs(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
) -> Result<Vec<Song>, String> {
    ctx.netease_source.search_songs(&query)
}

/// 在线搜索歌手（返回库内 Artist 模型，附带 netease SourceId）。
#[tauri::command]
pub fn netease_search_artists(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<Artist>, String> {
    ctx.netease_source
        .search_artists(&query, limit.unwrap_or(30), offset.unwrap_or(0))
}

/// 在线搜索专辑（返回库内 Album 模型，附带 netease SourceId）。
#[tauri::command]
pub fn netease_search_albums(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<Album>, String> {
    ctx.netease_source
        .search_albums(&query, limit.unwrap_or(30), offset.unwrap_or(0))
}

/// 在线搜索歌单（返回简略信息列表）。
#[tauri::command]
pub fn netease_search_playlists(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<NeteasePlaylistBrief>, String> {
    ctx.netease_source
        .search_playlists(&query, limit.unwrap_or(30), offset.unwrap_or(0))
}