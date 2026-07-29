//! 音乐来源路由 — 本地文件夹管理 + 资源获取。
//!
//! | 方法 | 路径 | 对应命令 |
//! |------|------|---------|
//! | GET | `/sources/local/folders` | `local_get_folders` |
//! | POST | `/sources/local/folders` | `local_add_folder` |
//! | DELETE | `/sources/local/folders` | `local_remove_folder` (body: {path}) |
//! | GET | `/sources/local/stats` | `local_stats` |
//! | POST | `/sources/local/rescan` | `local_rescan` |
//! | POST | `/resource/song-file` | `get_song_file` (body: SourceId JSON) |
//! | POST | `/resource/album-picture` | `get_album_picture` (body: SourceId JSON) |
//! | POST | `/resource/lyric` | `get_lyric_text` (body: SourceId JSON) |

use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chordial_core::module::music_localSource;
use chordial_core::module::music_source::resource;
use chordial_core::module::music_source::types::SourceId;
use chordial_core::module::platform::{self, PlatformPath};
use serde::Deserialize;

pub fn router() -> Router<AppState> {
    Router::new()
        // 本地来源
        .route("/sources/local/folders", get(local_get_folders).post(local_add_folder).delete(local_remove_folder))
        .route("/sources/local/stats", get(local_stats))
        .route("/sources/local/rescan", post(local_rescan))
        // 资源获取
        .route("/resource/song-file", post(get_song_file))
        .route("/resource/album-picture", post(get_album_picture))
        .route("/resource/lyric", post(get_lyric_text))
}

// ── Local Source ────────────────────────────────────

async fn local_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let source = &state.ctx.local_source;
    let folder_count = source.folder_manager.count();
    let indexed_count = source.file_index.read().len();
    Json(serde_json::json!({
        "folder_count": folder_count,
        "indexed_files": indexed_count,
    }))
}

#[derive(Debug, Deserialize)]
struct FolderPathBody {
    path: String,
}

async fn local_get_folders(State(state): State<AppState>) -> Json<Vec<String>> {
    let source = &state.ctx.local_source;
    Json(
        source
            .folder_manager
            .get_folders()
            .iter()
            .map(|p| platform::path_to_string(p))
            .collect(),
    )
}

async fn local_add_folder(
    State(state): State<AppState>,
    Json(body): Json<FolderPathBody>,
) -> Result<Json<serde_json::Value>, String> {
    let source = &state.ctx.local_source;
    let folder_path = PlatformPath::from(body.path.as_str());
    source.folder_manager.add_folder(&folder_path)?;

    // 批量索引（单次加载库 + 并行探测 + 单次写回）
    let files = music_localSource::folder::collect_audio_files(&folder_path);
    let (indexed, errors) = source.batch_index_files(&files)?;

    source.library.save()?;

    Ok(Json(serde_json::json!({
        "added": true,
        "path": body.path,
        "files_found": files.len(),
        "indexed": indexed,
        "errors": errors,
    })))
}

async fn local_remove_folder(
    State(state): State<AppState>,
    Json(body): Json<FolderPathBody>,
) -> Result<Json<serde_json::Value>, String> {
    let source = &state.ctx.local_source;
    let folder_path = PlatformPath::from(body.path.as_str());

    // 批量清理（单次库调用替代 N 次 unindex_file）
    let files = music_localSource::folder::collect_audio_files(&folder_path);
    let cleaned = source.batch_unindex_files(&files)?;

    let removed = source.folder_manager.remove_folder(&folder_path);
    source.library.save()?;

    Ok(Json(serde_json::json!({
        "removed": removed,
        "path": body.path,
        "cleaned_files": cleaned,
    })))
}

async fn local_rescan(State(state): State<AppState>) -> Result<Json<serde_json::Value>, String> {
    let source = &state.ctx.local_source;
    let folders = source.folder_manager.get_folders();

    // 收集所有文件夹下的音频文件，一次性批量索引
    let mut all_files: Vec<PlatformPath> = Vec::new();
    for folder in &folders {
        all_files.extend(music_localSource::folder::collect_audio_files(folder));
    }
    let files_found = all_files.len();
    let (indexed, errors) = source.batch_index_files(&all_files)?;
    for e in &errors {
        eprintln!("[local_rescan] {}", e);
    }

    source.library.save()?;

    Ok(Json(serde_json::json!({
        "indexed": indexed,
        "files_found": files_found,
        "folders_scanned": folders.len(),
        "errors": errors,
    })))
}

// ── Resource ────────────────────────────────────────
//
// `MusicSource` trait 是同步阻塞接口（底层 `reqwest::blocking`），必须用
// `spawn_blocking` 包裹，否则会阻塞 tokio 工作线程导致整个 server 冻结。

async fn get_song_file(
    State(state): State<AppState>,
    Json(source_id): Json<SourceId>,
) -> Result<Vec<u8>, String> {
    let registrar = state.ctx.registrar.clone();
    tokio::task::spawn_blocking(move || resource::get_song_file(&registrar, &source_id))
        .await
        .map_err(|e| format!("阻塞任务失败: {}", e))?
}

async fn get_album_picture(
    State(state): State<AppState>,
    Json(source_id): Json<SourceId>,
) -> Result<Vec<u8>, String> {
    let registrar = state.ctx.registrar.clone();
    tokio::task::spawn_blocking(move || resource::get_album_picture(&registrar, &source_id))
        .await
        .map_err(|e| format!("阻塞任务失败: {}", e))?
}

async fn get_lyric_text(
    State(state): State<AppState>,
    Json(source_id): Json<SourceId>,
) -> Result<String, String> {
    let registrar = state.ctx.registrar.clone();
    tokio::task::spawn_blocking(move || resource::get_lyric_text(&registrar, &source_id))
        .await
        .map_err(|e| format!("阻塞任务失败: {}", e))?
}
