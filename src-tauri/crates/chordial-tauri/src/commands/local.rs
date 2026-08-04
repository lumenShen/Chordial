//! Local Source 文件夹管理命令。

use super::common::LIBRARY_CHANGED_EVENT;
use chordial_core::module::music_localSource;
use chordial_core::module::platform::{self, PlatformPath};
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

// ══════════════════════════════════════════════════════════════════════════════
// Local Source 文件夹管理命令
// ══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn local_stats(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let source = &ctx.local_source;
    let folder_count = source.folder_manager.count();
    let indexed_count = source.file_index.read().len();

    Ok(serde_json::json!({
        "folder_count": folder_count,
        "indexed_files": indexed_count,
    }))
}

#[tauri::command]
pub fn local_add_folder(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
    path: String,
) -> Result<serde_json::Value, String> {
    let source = &ctx.local_source;
    let folder_path = PlatformPath::from(path.as_str());
    source.folder_manager.add_folder(&folder_path)?;

    // 扫描并索引文件夹中的音频文件（批量：单次加载库 + 并行探测 + 单次写回）
    let files = music_localSource::folder::collect_audio_files(&folder_path);
    let (indexed, errors) = source.batch_index_files(&files)?;

    // 持久化音乐库
    source.library.save()?;

    // 通知前端：库内容已变更，触发专辑/艺人列表刷新
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());

    Ok(serde_json::json!({
        "added": true,
        "path": path,
        "files_found": files.len(),
        "indexed": indexed,
        "errors": errors,
    }))
}

#[tauri::command]
pub fn local_remove_folder(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
    path: String,
) -> Result<serde_json::Value, String> {
    let source = &ctx.local_source;
    let folder_path = PlatformPath::from(path.as_str());

    // 1. 批量清理该文件夹下所有文件的库引用 + 本地索引（单次库调用）
    let files = music_localSource::folder::collect_audio_files(&folder_path);
    let cleaned = source.batch_unindex_files(&files)?;

    // 2. 从文件夹管理器移除
    let removed = source.folder_manager.remove_folder(&folder_path);

    // 3. 持久化
    source.library.save()?;

    // 通知前端：库内容已变更，触发专辑/艺人列表刷新
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());

    Ok(serde_json::json!({
        "removed": removed,
        "path": path,
        "cleaned_files": cleaned,
    }))
}

#[tauri::command]
pub fn local_get_folders(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    let source = &ctx.local_source;
    Ok(source
        .folder_manager
        .get_folders()
        .iter()
        .map(|p| platform::path_to_string(p))
        .collect())
}

#[tauri::command]
pub fn local_rescan(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    let source = &ctx.local_source;
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

    // 通知前端：库内容已变更，触发专辑/艺人列表刷新
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());

    Ok(serde_json::json!({
        "indexed": indexed,
        "files_found": files_found,
        "folders_scanned": folders.len(),
        "errors": errors,
    }))
}