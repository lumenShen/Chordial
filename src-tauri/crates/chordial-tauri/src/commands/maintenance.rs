//! 数据维护命令 — 设置页"清理数据"工具。

use super::common::LIBRARY_CHANGED_EVENT;
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// 获取数据存储的统计信息（用于维护页展示）。
///
/// 返回各类数据的条目数 + 持久化文件路径 + 磁盘占用估算。
#[tauri::command]
pub fn maintenance_stats(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let library = &ctx.library;
    let local = &ctx.local_source;

    // 各类实体计数
    let song_count = library.song_count();
    let artist_count = library.artist_count();
    let album_count = library.album_count();
    let lyric_count = library.lyric_count();
    let folder_count = local.folder_manager.count();
    let indexed_files = local.file_index.read().len();

    // 持久化文件路径
    let library_path = library.store_path().to_string_lossy().to_string();
    let storage_path = ctx.store.path().to_string_lossy().to_string();
    let config_path = ctx.config.path().to_string_lossy().to_string();
    let folder_store_path = local.folder_manager.store().path().to_string_lossy().to_string();

    // 磁盘占用估算（仅统计主要数据文件）
    let mut total_bytes: u64 = 0;
    for path in [
        library.store_path(),
        ctx.store.path(),
        ctx.config.path(),
        local.folder_manager.store().path(),
    ] {
        if let Ok(meta) = std::fs::metadata(path) {
            total_bytes += meta.len();
        }
    }
    // Blob 缓存目录
    if let Some(blob_dir) = ctx.cache.blob_dir() {
        if let Ok(entries) = std::fs::read_dir(&blob_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    total_bytes += meta.len();
                }
            }
        }
    }

    Ok(serde_json::json!({
        "counts": {
            "songs": song_count,
            "artists": artist_count,
            "albums": album_count,
            "lyrics": lyric_count,
            "folders": folder_count,
            "indexed_files": indexed_files,
        },
        "paths": {
            "library": library_path,
            "storage": storage_path,
            "config": config_path,
            "folder_store": folder_store_path,
        },
        "disk_usage_bytes": total_bytes,
    }))
}

/// 清空音乐库（Song / Artist / Album / Lyric）+ 本地索引 + 文件夹列表。
///
/// 保留：配置（config.json）、通用存储（storage.json）、缓存。
/// 清空后需重新添加文件夹并扫描。
#[tauri::command]
pub fn maintenance_clear_library(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    // 1. 清空音乐库（内存 + 标记脏）
    ctx.library.clear_all();
    // 2. 清空本地索引（file_index / id_to_path / mtime / 封面缓存 / mtime_store 持久化）
    ctx.local_source.clear_all_index()?;
    // 3. 清空文件夹列表（持久化）
    ctx.local_source.folder_manager.clear_folders()?;
    // 4. 落盘音乐库
    ctx.library.save()?;

    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());

    Ok(serde_json::json!({
        "cleared": true,
        "message": "音乐库已清空，请重新添加文件夹并扫描",
    }))
}

/// 修复库中 song.artist_ids / song.album_id 引用的无效 UUID。
///
/// 适用场景：历史扫描产生的 song.artist_ids 引用了不存在的 artist UUID，
/// 导致点击歌手详情页时报"艺术家 'xxx' 不存在"。
///
/// 修复后无需重新扫描即可正常访问歌手/专辑页面。
#[tauri::command]
pub fn maintenance_repair_references(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    let (repaired_songs, repaired_artists, repaired_albums) =
        ctx.library.repair_references()?;

    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());

    Ok(serde_json::json!({
        "repaired": true,
        "repaired_songs": repaired_songs,
        "repaired_artists": repaired_artists,
        "repaired_albums": repaired_albums,
        "message": format!(
            "已修复 {} 首歌曲 / {} 个艺人 / {} 张专辑的引用",
            repaired_songs, repaired_artists, repaired_albums
        ),
    }))
}

/// 清空内存缓存 + Blob 磁盘缓存 + 封面磁盘缓存（不影响音乐库 / 配置 / 存储）。
#[tauri::command]
pub fn maintenance_clear_cache(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let mem_keys = ctx.cache.keys().len();
    let blob_keys = ctx.cache.blob_keys().len();
    ctx.cache.clear();
    ctx.cache.clear_blobs();
    // 清空封面磁盘缓存（扫描阶段预热的封面图）
    ctx.local_source.clear_cover_disk_cache();
    Ok(serde_json::json!({
        "cleared": true,
        "memory_items": mem_keys,
        "blob_items": blob_keys,
    }))
}

/// 清空通用存储 storage.json（不影响音乐库 / 配置 / 缓存）。
#[tauri::command]
pub fn maintenance_clear_storage(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let keys = ctx.store.keys();
    let count = keys.len();
    ctx.store.clear();
    ctx.store.save()?;
    Ok(serde_json::json!({
        "cleared": true,
        "items": count,
    }))
}

/// 重置全部数据 — 音乐库 + 本地索引 + 文件夹 + 配置 + 存储 + 缓存。
///
/// 等效于"恢复出厂设置"。保留可执行文件本身和 Blob 目录结构。
/// 调用后建议重启应用。
#[tauri::command]
pub fn maintenance_reset_all(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    // 1. 音乐库 + 本地索引 + 文件夹
    ctx.library.clear_all();
    ctx.local_source.clear_all_index()?;
    ctx.local_source.folder_manager.clear_folders()?;
    // 2. 通用存储 + 配置
    ctx.store.clear();
    ctx.store.save()?;
    ctx.config.clear();
    ctx.config.flush()?;
    // 3. 缓存（内存 + Blob）
    ctx.cache.clear();
    ctx.cache.clear_blobs();
    // 4. 落盘音乐库
    ctx.library.save()?;

    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());

    Ok(serde_json::json!({
        "reset": true,
        "message": "所有数据已清空，建议重启应用",
    }))
}