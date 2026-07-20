//! Tauri 命令层 — chordial-core 的薄封装（库调用形式）。
//!
//! 所有命令通过 Tauri 的 `State<'_, Arc<AppContext>>` 提取由 `setup` 注入的
//! server 层上下文，再委托给 core 方法。命令本身不含业务逻辑。
//!
//! 这正是「库调用形式」的 front 层：前端 `invoke` → 本层 → core 同步函数调用，
//! 全程进程内，无网络开销。

use chordial_core::module::music_localSource;
use chordial_core::module::music_source::resource;
use chordial_core::module::music_source::traits::MusicSource;
use chordial_core::module::music_source::types::{EntityType, SourceId};
use chordial_core::module::platform::{self, PlatformPath};
use chordial_core::module::storage::entry::Ttl;
use chordial_core::AppContext;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{AppHandle, Emitter, State};

/// 库变更事件名 — 任何改变库内容的操作（添加/移除源、扫描、CRUD）都应 emit。
///
/// 前端通过 `listen("library-changed")` 订阅，触发专辑/艺人列表刷新。
const LIBRARY_CHANGED_EVENT: &str = "library-changed";

/// 批量下载进度事件名 — 后台下载每完成一首（成功/失败）emit。
///
/// 前端通过 `listen("netease-download-progress")` 订阅，更新进度条 UI。
const DOWNLOAD_PROGRESS_EVENT: &str = "netease-download-progress";

/// 全局下载任务注册表 — task_id → 取消标志。
///
/// 用于支持前端取消进行中的批量下载任务。后台线程定期检查标志，
/// 若为 true 则提前终止剩余下载。
static DOWNLOAD_TASKS: LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 注册一个新下载任务，返回其取消标志。
fn register_download_task(task_id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    DOWNLOAD_TASKS
        .lock()
        .unwrap()
        .insert(task_id.to_string(), flag.clone());
    flag
}

/// 取消指定下载任务。返回 true 表示找到并取消，false 表示任务不存在或已完成。
fn cancel_download_task(task_id: &str) -> bool {
    if let Some(flag) = DOWNLOAD_TASKS.lock().unwrap().get(task_id) {
        flag.store(true, Ordering::SeqCst);
        return true;
    }
    false
}

/// 移除已完成的下载任务（后台线程结束时调用）。
fn unregister_download_task(task_id: &str) {
    DOWNLOAD_TASKS.lock().unwrap().remove(task_id);
}

// ══════════════════════════════════════════════════════════════════════════════
// TTL 参数辅助类型
// ══════════════════════════════════════════════════════════════════════════════

/// 前端传入的 TTL 参数。
///
/// 支持三种格式（`serde_json` 可自动识别）：
/// - `"forever"` → [`Ttl::Forever`]
/// - `"session"` → [`Ttl::Session`]
/// - `{"duration_secs": 600}` → [`Ttl::DurationSecs(600)`]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TtlArg {
    Forever,
    Session,
    DurationSecs(u64),
}

impl From<TtlArg> for Ttl {
    fn from(arg: TtlArg) -> Self {
        match arg {
            TtlArg::Forever => Ttl::Forever,
            TtlArg::Session => Ttl::Session,
            TtlArg::DurationSecs(n) => Ttl::DurationSecs(n),
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Config 命令 — 自动防抖落盘
// ══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn config_get(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Value, String> {
    ctx.config
        .get_raw(&key)
        .ok_or_else(|| format!("配置项 '{}' 不存在", key))
}

#[tauri::command]
pub fn config_set(ctx: State<'_, Arc<AppContext>>, key: String, value: Value) -> Result<(), String> {
    ctx.config.set_raw(&key, value);
    Ok(())
}

#[tauri::command]
pub fn config_remove(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.config.remove(&key))
}

#[tauri::command]
pub fn config_has(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.config.has(&key))
}

#[tauri::command]
pub fn config_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.config.keys())
}

#[tauri::command]
pub fn config_clear(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.config.clear();
    Ok(())
}

#[tauri::command]
pub fn config_flush(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.config.flush()
}

#[tauri::command]
pub fn config_reload(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.config.reload();
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════════
// Storage 命令 — 手动落盘
// ══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn storage_get(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Value, String> {
    ctx.store
        .get_raw(&key)
        .ok_or_else(|| format!("键 '{}' 不存在", key))
}

#[tauri::command]
pub fn storage_set(ctx: State<'_, Arc<AppContext>>, key: String, value: Value) -> Result<(), String> {
    ctx.store.set_raw(&key, value);
    Ok(())
}

#[tauri::command]
pub fn storage_remove(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.store.remove(&key))
}

#[tauri::command]
pub fn storage_has(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.store.has(&key))
}

#[tauri::command]
pub fn storage_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.store.keys())
}

#[tauri::command]
pub fn storage_clear(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.store.clear();
    Ok(())
}

#[tauri::command]
pub fn storage_save(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.store.save()
}

// ══════════════════════════════════════════════════════════════════════════════
// Cache 命令 — 纯内存，TTL 过期
// ══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn cache_get(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Value, String> {
    ctx.cache
        .get_raw(&key)
        .ok_or_else(|| format!("缓存项 '{}' 不存在或已过期", key))
}

#[tauri::command]
pub fn cache_set(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
    value: Value,
    ttl: TtlArg,
) -> Result<(), String> {
    ctx.cache.set_raw(&key, value, &ttl.into());
    Ok(())
}

#[tauri::command]
pub fn cache_remove(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.cache.remove(&key))
}

#[tauri::command]
pub fn cache_has(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.cache.has(&key))
}

#[tauri::command]
pub fn cache_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.cache.keys())
}

#[tauri::command]
pub fn cache_clear(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.cache.clear();
    Ok(())
}

#[tauri::command]
pub fn cache_clear_expired(ctx: State<'_, Arc<AppContext>>) -> Result<usize, String> {
    Ok(ctx.cache.clear_expired())
}

#[tauri::command]
pub fn cache_touch(ctx: State<'_, Arc<AppContext>>, key: String, ttl: TtlArg) -> Result<bool, String> {
    Ok(ctx.cache.touch(&key, &ttl.into()))
}

// ══════════════════════════════════════════════════════════════════════════════
// Blob Cache 命令 — 磁盘文件存储 + 内存 TTL 过期
// ══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn cache_enable_blob_storage(ctx: State<'_, Arc<AppContext>>, dir: String) -> Result<(), String> {
    ctx.cache.enable_blob_storage(PathBuf::from(dir))
}

#[tauri::command]
pub fn cache_blob_storage_enabled(ctx: State<'_, Arc<AppContext>>) -> Result<bool, String> {
    Ok(ctx.cache.blob_storage_enabled())
}

#[tauri::command]
pub fn cache_set_blob(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
    data: Vec<u8>,
    ttl: TtlArg,
) -> Result<(), String> {
    ctx.cache.set_blob(&key, &data, &ttl.into())
}

#[tauri::command]
pub fn cache_get_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Vec<u8>, String> {
    ctx.cache
        .get_blob(&key)
        .ok_or_else(|| format!("Blob 缓存项 '{}' 不存在或已过期", key))
}

#[tauri::command]
pub fn cache_remove_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.cache.remove_blob(&key))
}

#[tauri::command]
pub fn cache_has_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.cache.has_blob(&key))
}

#[tauri::command]
pub fn cache_blob_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.cache.blob_keys())
}

#[tauri::command]
pub fn cache_clear_blobs(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.cache.clear_blobs();
    Ok(())
}

#[tauri::command]
pub fn cache_clear_expired_blobs(ctx: State<'_, Arc<AppContext>>) -> Result<usize, String> {
    Ok(ctx.cache.clear_expired_blobs())
}

// ══════════════════════════════════════════════════════════════════════════════
// Blob Storage 命令 — 持久化二进制文件存储
// ══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn storage_set_blob(ctx: State<'_, Arc<AppContext>>, key: String, data: Vec<u8>) -> Result<(), String> {
    ctx.store.set_blob(&key, &data)
}

#[tauri::command]
pub fn storage_get_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Vec<u8>, String> {
    ctx.store
        .get_blob(&key)
        .ok_or_else(|| format!("Blob 存储项 '{}' 不存在", key))
}

#[tauri::command]
pub fn storage_remove_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.store.remove_blob(&key))
}

#[tauri::command]
pub fn storage_has_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.store.has_blob(&key))
}

#[tauri::command]
pub fn storage_blob_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.store.blob_keys())
}

#[tauri::command]
pub fn storage_clear_blobs(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.store.clear_blobs();
    Ok(())
}

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

// ══════════════════════════════════════════════════════════════════════════════
// MusicLibrary 命令 — 音乐库 CRUD / 搜索 / 关系查询
// ══════════════════════════════════════════════════════════════════════════════

// ── 持久化 ──────────────────────────────────────────

#[tauri::command]
pub fn library_save(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.library.save()
}

#[tauri::command]
pub fn library_cleanup_empty_entities(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.library.cleanup_empty_entities()?;
    ctx.library.save()
}

// ── Song ────────────────────────────────────────────

#[tauri::command]
pub fn library_song_count(ctx: State<'_, Arc<AppContext>>) -> Result<usize, String> {
    Ok(ctx.library.song_count())
}

#[tauri::command]
pub fn library_get_song(ctx: State<'_, Arc<AppContext>>, id: String) -> Result<serde_json::Value, String> {
    let song = ctx
        .library
        .get_song(&id)
        .ok_or_else(|| format!("歌曲 '{}' 不存在", id))?;
    serde_json::to_value(&song).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_all_songs(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let songs = ctx.library.get_all_songs();
    serde_json::to_value(&songs).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_songs_page(
    ctx: State<'_, Arc<AppContext>>,
    offset: usize,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let songs = ctx.library.get_songs_page(offset, limit);
    serde_json::to_value(&songs).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_search_songs(ctx: State<'_, Arc<AppContext>>, query: String) -> Result<serde_json::Value, String> {
    let songs = ctx.library.search_songs(&query);
    serde_json::to_value(&songs).map_err(|e| format!("序列化失败: {}", e))
}

// ── Artist ──────────────────────────────────────────

#[tauri::command]
pub fn library_artist_count(ctx: State<'_, Arc<AppContext>>) -> Result<usize, String> {
    Ok(ctx.library.artist_count())
}

#[tauri::command]
pub fn library_get_artist(ctx: State<'_, Arc<AppContext>>, id: String) -> Result<serde_json::Value, String> {
    let artist = ctx
        .library
        .get_artist(&id)
        .ok_or_else(|| format!("艺术家 '{}' 不存在", id))?;
    serde_json::to_value(&artist).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_all_artists(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let artists = ctx.library.get_all_artists();
    serde_json::to_value(&artists).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_artists_page(
    ctx: State<'_, Arc<AppContext>>,
    offset: usize,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let artists = ctx.library.get_artists_page(offset, limit);
    serde_json::to_value(&artists).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_search_artists(ctx: State<'_, Arc<AppContext>>, query: String) -> Result<serde_json::Value, String> {
    let artists = ctx.library.search_artists(&query);
    serde_json::to_value(&artists).map_err(|e| format!("序列化失败: {}", e))
}

// ── Album ───────────────────────────────────────────

#[tauri::command]
pub fn library_album_count(ctx: State<'_, Arc<AppContext>>) -> Result<usize, String> {
    Ok(ctx.library.album_count())
}

#[tauri::command]
pub fn library_get_album(ctx: State<'_, Arc<AppContext>>, id: String) -> Result<serde_json::Value, String> {
    let album = ctx
        .library
        .get_album(&id)
        .ok_or_else(|| format!("专辑 '{}' 不存在", id))?;
    serde_json::to_value(&album).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_all_albums(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let albums = ctx.library.get_all_albums();
    serde_json::to_value(&albums).map_err(|e| format!("序列化失败: {}", e))
}

/// 批量按 ID 获取专辑。
///
/// 用于替代「加载全部再按 id 过滤」的反模式：
/// `library_get_all_albums`（全量反序列化 ~110ms）
/// → 本命令 O(n) 每条 O(1)，n=10 时 ~1ms。
#[tauri::command]
pub fn library_get_albums_by_ids(
    ctx: State<'_, Arc<AppContext>>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    let albums = ctx.library.get_albums_by_ids(&ids);
    serde_json::to_value(&albums).map_err(|e| format!("序列化失败: {}", e))
}

/// 批量按 ID 获取歌曲。
///
/// 用于替代 N 次 `library_get_song` 的 N 次 IPC 开销。
#[tauri::command]
pub fn library_get_songs_by_ids(
    ctx: State<'_, Arc<AppContext>>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    let songs = ctx.library.get_songs_by_ids(&ids);
    serde_json::to_value(&songs).map_err(|e| format!("序列化失败: {}", e))
}

/// 批量按 ID 获取艺术家。
///
/// 用于替代「加载全部再按 id 过滤」的反模式：
/// `library_get_all_artists`（全量反序列化）→ 本命令 O(n) 每条 O(1)。
#[tauri::command]
pub fn library_get_artists_by_ids(
    ctx: State<'_, Arc<AppContext>>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    let artists = ctx.library.get_artists_by_ids(&ids);
    serde_json::to_value(&artists).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_albums_page(
    ctx: State<'_, Arc<AppContext>>,
    offset: usize,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let albums = ctx.library.get_albums_page(offset, limit);
    serde_json::to_value(&albums).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_home_stats(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    Ok(ctx.library.get_home_stats())
}

#[tauri::command]
pub fn library_search_albums(ctx: State<'_, Arc<AppContext>>, query: String) -> Result<serde_json::Value, String> {
    let albums = ctx.library.search_albums(&query);
    serde_json::to_value(&albums).map_err(|e| format!("序列化失败: {}", e))
}

/// 统一搜索引擎 — 基于 trigram 倒排索引的跨类型子串搜索。
///
/// 参数：
/// - `query`：搜索关键词（大小写不敏感子串匹配）
/// - `entity_type`：可选，限定实体类型（"song" / "artist" / "album"）
/// - `source_name`：可选，限定来源名称
/// - `limit_per_type`：可选，每类实体最多返回多少条
#[tauri::command]
pub fn library_search(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    entity_type: Option<String>,
    source_name: Option<String>,
    limit_per_type: Option<usize>,
) -> Result<serde_json::Value, String> {
    let et = entity_type
        .as_deref()
        .map(parse_entity_type)
        .transpose()?;
    let results = ctx.library.search(&query, et, source_name.as_deref(), limit_per_type);
    serde_json::to_value(&results).map_err(|e| format!("序列化失败: {}", e))
}

/// 将字符串解析为 `EntityType`（大小写不敏感）。
fn parse_entity_type(s: &str) -> Result<EntityType, String> {
    match s.to_lowercase().as_str() {
        "song" => Ok(EntityType::Song),
        "artist" => Ok(EntityType::Artist),
        "album" => Ok(EntityType::Album),
        "lyric" => Ok(EntityType::Lyric),
        other => Err(format!("未知实体类型 '{}'（支持: song/artist/album/lyric）", other)),
    }
}

// ── Lyric ───────────────────────────────────────────

#[tauri::command]
pub fn library_lyric_count(ctx: State<'_, Arc<AppContext>>) -> Result<usize, String> {
    Ok(ctx.library.lyric_count())
}

#[tauri::command]
pub fn library_get_lyric(ctx: State<'_, Arc<AppContext>>, id: String) -> Result<serde_json::Value, String> {
    let lyric = ctx
        .library
        .get_lyric(&id)
        .ok_or_else(|| format!("歌词 '{}' 不存在", id))?;
    serde_json::to_value(&lyric).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_all_lyrics(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let lyrics = ctx.library.get_all_lyrics();
    serde_json::to_value(&lyrics).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_search_lyrics(ctx: State<'_, Arc<AppContext>>, query: String) -> Result<serde_json::Value, String> {
    let lyrics = ctx.library.search_lyrics(&query);
    serde_json::to_value(&lyrics).map_err(|e| format!("序列化失败: {}", e))
}

// ── Relations ───────────────────────────────────────

#[tauri::command]
pub fn library_get_artists_of_song(
    ctx: State<'_, Arc<AppContext>>,
    song_id: String,
) -> Result<serde_json::Value, String> {
    let artists = ctx.library.get_artists_of_song(&song_id);
    serde_json::to_value(&artists).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_album_of_song(
    ctx: State<'_, Arc<AppContext>>,
    song_id: String,
) -> Result<serde_json::Value, String> {
    let album = ctx.library.get_album_of_song(&song_id);
    serde_json::to_value(&album).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_lyric_of_song(
    ctx: State<'_, Arc<AppContext>>,
    song_id: String,
) -> Result<serde_json::Value, String> {
    let lyric = ctx.library.get_lyric_of_song(&song_id);
    serde_json::to_value(&lyric).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_songs_by_artist(
    ctx: State<'_, Arc<AppContext>>,
    artist_id: String,
) -> Result<serde_json::Value, String> {
    let songs = ctx.library.get_songs_by_artist(&artist_id);
    serde_json::to_value(&songs).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_albums_by_artist(
    ctx: State<'_, Arc<AppContext>>,
    artist_id: String,
) -> Result<serde_json::Value, String> {
    let albums = ctx.library.get_albums_by_artist(&artist_id);
    serde_json::to_value(&albums).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_songs_in_album(
    ctx: State<'_, Arc<AppContext>>,
    album_id: String,
) -> Result<serde_json::Value, String> {
    let songs = ctx.library.get_songs_in_album(&album_id);
    serde_json::to_value(&songs).map_err(|e| format!("序列化失败: {}", e))
}

#[tauri::command]
pub fn library_get_source_ids_of_song(
    ctx: State<'_, Arc<AppContext>>,
    song_id: String,
) -> Result<serde_json::Value, String> {
    let ids = ctx.library.get_source_ids_of_song(&song_id);
    serde_json::to_value(&ids).map_err(|e| format!("序列化失败: {}", e))
}

/// 删除指定的本地音频文件并移除库内对应的 Local SourceId。
///
/// 适用场景：
/// - 批量下载替换旧版本时删除旧文件
/// - 手动清理某个本地文件
///
/// 与 `local_remove_folder` / `local_rescan` 不同，此命令只处理单个文件，
/// 不依赖 `LocalMusicSource` 索引（旧文件可能从未被本地源索引）。
///
/// # 参数
/// - `file_path`: 本地音频文件的绝对路径
///
/// # 返回
/// `true` 表示找到并清理了库内 SourceId（物理文件无论是否存在都已尝试删除）
#[tauri::command]
pub fn library_delete_song_file(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
    file_path: String,
) -> Result<bool, String> {
    let removed = ctx.library.delete_local_file_and_source(&file_path)?;
    ctx.library.save()?;
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(removed)
}

/// 手动去重：保留每首"逻辑歌曲"中音质最高的本地文件，删除其他本地副本。
///
/// 按 (title_lower, sorted_artist_names_lower) 分组所有歌曲，每组内：
/// - 比较所有 Local 来源的文件扩展名推断音质
/// - 保留 rank 最高的本地文件
/// - 删除其他本地文件 + 移除其 SourceId
///
/// Web 来源（如 netease）不参与去重，保留不动。
///
/// # 返回
/// `{ removed_files: string[], kept_files: number, groups_processed: number }`
#[tauri::command]
pub fn library_dedup_keep_highest_quality(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    let (removed, kept, groups) = ctx.library.dedup_keep_highest_quality()?;
    ctx.library.save()?;
    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(serde_json::json!({
        "removed_files": removed,
        "kept_files": kept,
        "groups_processed": groups,
    }))
}

// ══════════════════════════════════════════════════════════════════════════════
// P2P 资源共享命令
// ══════════════════════════════════════════════════════════════════════════════

use chordial_core::module::p2p::{Permission, P2pStatus, TrustedDevice};

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

// ══════════════════════════════════════════════════════════════════════════════
// 数据维护命令 — 设置页"清理数据"工具
// ══════════════════════════════════════════════════════════════════════════════

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

/// 清空内存缓存 + Blob 磁盘缓存（不影响音乐库 / 配置 / 存储）。
#[tauri::command]
pub fn maintenance_clear_cache(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let mem_keys = ctx.cache.keys().len();
    let blob_keys = ctx.cache.blob_keys().len();
    ctx.cache.clear();
    ctx.cache.clear_blobs();
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

/// 诊断命令：检查指定歌曲的歌词状态。
///
/// 用于排查"部分歌曲歌词无法显示"问题。返回：
/// - `song`: 库中的 Song 实体（含 source_ids、lyric_id 等）
/// - `lyric`: 库中通过 song_id 查到的 Lyric 实体（可能为 null）
/// - `file_path`: 实际音频文件路径
/// - `file_exists`: 文件是否存在
/// - `external_lyric_file`: 同目录 .lrc/.txt 文件内容预览（若存在）
/// - `raw_tags`: symphonia 探测到的所有原始标签键值对
/// - `embedded_lyrics_in_meta`: scanner.probe_file 提取出的嵌入歌词预览
///
/// 前端可通过此命令一次性看到：
///   1. 库里是否真的有 Lyric 实体（lyric 字段）
///   2. 若没有，是因为外部歌词文件缺失 + 嵌入歌词也没被提取（看 raw_tags 里是否有歌词键）
///   3. 嵌入歌词的 raw tag key 实际叫什么（可能不在我们的匹配列表里）
#[tauri::command]
pub fn debug_inspect_song_lyrics(
    ctx: State<'_, Arc<AppContext>>,
    song_id: String,
) -> Result<serde_json::Value, String> {
    use chordial_core::module::music_localSource::scanner;

    // 1. 查 Song 实体
    let song = ctx.library.get_song(&song_id);
    let song_json = serde_json::to_value(&song)
        .map_err(|e| format!("序列化 song 失败: {}", e))?;

    // 2. 查 Lyric 实体（按 song_id 索引）
    let lyric = ctx.library.get_lyric_of_song(&song_id);
    let lyric_json = serde_json::to_value(&lyric)
        .map_err(|e| format!("序列化 lyric 失败: {}", e))?;

    // 3. 找到实际音频文件路径
    let path_str = ctx
        .local_source
        .id_to_path
        .read()
        .get(&song_id)
        .map(|p| platform::path_to_string(p))
        .or_else(|| {
            // fallback：从 song.source_ids 找 local 来源的 entity_id
            song.as_ref().and_then(|s| {
                s.source_ids
                    .iter()
                    .find(|sid| sid.source_name == "local")
                    .map(|sid| sid.entity_id.clone())
            })
        });

    let Some(path_str) = path_str else {
        return Ok(serde_json::json!({
            "song": song_json,
            "lyric": lyric_json,
            "file_path": null,
            "error": "无法定位音频文件路径（song_id 既不在 id_to_path，也无 local source_id）",
        }));
    };

    let path = PlatformPath::from(path_str.as_str());
    let file_exists = platform::exists(&path);

    // 4. 外部 .lrc/.txt 文件
    let external_lyric = if file_exists {
        scanner::read_lyric_file(&path)
    } else {
        None
    };
    let external_lyric_preview = external_lyric
        .as_ref()
        .map(|t| {
            if t.len() > 300 {
                format!("{}...(len={})", &t[..300], t.len())
            } else {
                t.clone()
            }
        });

    // 5. 列出所有原始标签
    let raw_tags: Vec<(String, String)> = if file_exists {
        scanner::list_raw_tags(&path).unwrap_or_else(|e| vec![("<error>".to_string(), e)])
    } else {
        vec![]
    };

    // 6. 提取嵌入歌词（与扫描时逻辑一致）
    let embedded_lyrics_in_meta = if file_exists {
        scanner::probe_file(&path)
            .ok()
            .and_then(|m| m.lyrics)
            .map(|t| {
                if t.len() > 300 {
                    format!("{}...(len={})", &t[..300], t.len())
                } else {
                    t
                }
            })
    } else {
        None
    };

    Ok(serde_json::json!({
        "song": song_json,
        "lyric": lyric_json,
        "file_path": path_str,
        "file_exists": file_exists,
        "external_lyric_file": external_lyric_preview,
        "embedded_lyrics_in_meta": embedded_lyrics_in_meta,
        "raw_tags": raw_tags.into_iter().map(|(k, v)| {
            serde_json::json!({ "key": k, "value": v })
        }).collect::<Vec<_>>(),
    }))
}

// ══════════════════════════════════════════════════════════════════════════════
// Netease Cloud Music API 来源命令
// ══════════════════════════════════════════════════════════════════════════════

use chordial_core::module::music_netease_source::{
    self, AudioQuality, NeteasePlaylistBrief,
};
use chordial_core::module::music_source::cache;
use chordial_core::module::music_web_source::WebSourceConfig;
use chordial_core::module::playlist::{self, Playlist};
use chordial_core::module::music_library::models::{Album, Artist, Song};

/// 获取网易云来源配置（base_url / cookie / 超时 / 启用 / 下载目录）。
#[tauri::command]
pub fn netease_get_config(ctx: State<'_, Arc<AppContext>>) -> Result<WebSourceConfig, String> {
    Ok(ctx.netease_source.config())
}

/// 更新网易云来源配置并持久化。
///
/// 重建 HTTP 客户端以应用新的 base_url / cookie / timeout。
#[tauri::command]
pub fn netease_update_config(
    ctx: State<'_, Arc<AppContext>>,
    config: WebSourceConfig,
) -> Result<(), String> {
    ctx.netease_source.update_config(config)
}

/// 测试与网易云 API 服务器的连通性。
///
/// 通过发起一次 limit=1 的搜索请求验证 base_url + cookie 是否有效。
#[tauri::command]
pub fn netease_test_connection(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let cfg = ctx.netease_source.config();
    if cfg.base_url.is_empty() {
        return Err("API base_url 未配置".to_string());
    }
    let result = ctx.netease_source.search_songs("test");
    Ok(serde_json::json!({
        "ok": result.is_ok(),
        "error": result.err(),
        "base_url": cfg.base_url,
    }))
}

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

/// 获取歌单详情（元信息 + 全部曲目）。
#[tauri::command]
pub fn netease_get_playlist_full(
    ctx: State<'_, Arc<AppContext>>,
    playlist_id: i64,
) -> Result<serde_json::Value, String> {
    let (detail, tracks) = ctx.netease_source.get_playlist_full(playlist_id)?;
    Ok(serde_json::json!({
        "detail": detail,
        "tracks": tracks,
    }))
}

/// 将在线歌单保存为本地 Playlist，并把所有曲目加入音乐库（自动去重）。
///
/// # 工作流
/// 1. 调用 `get_playlist_full` 拉取歌单元信息 + 全部曲目
/// 2. 每首曲目通过 `library.add_song` 加入库（带 netease SourceId，自动合并去重）
/// 3. 创建本地 Playlist 记录，song_ids 引用库内 Song.id
/// 4. 持久化 + 触发 `library-changed` 事件
#[tauri::command]
pub fn netease_save_playlist_to_library(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
    playlist_id: i64,
) -> Result<Playlist, String> {
    let (detail, tracks) = ctx.netease_source.get_playlist_full(playlist_id)?;

    // 批量构建 Song 后一次性入库（add_songs_batch 只反序列化+回写一次）
    // 旧实现 for-loop + add_song 每首都会全量反序列化+回写，1000 首歌约 80s
    let songs: Vec<_> = tracks
        .iter()
        .map(|t| ctx.netease_source.build_song_from_detail(t))
        .collect();
    let song_ids = ctx.library.add_songs_batch(&songs)?;
    // ⚠ 关键：add_songs_batch 只修改 library.store 的内存缓存并标记 dirty，
    // 必须显式调用 library.save() 落盘 music_library.json，否则重启后歌曲全部丢失。
    // ctx.store.save() 只保存 storage.json（歌单元数据），与 library.store 是两个独立的 PersistentStore。
    ctx.library.save()?;

    let mut playlist = Playlist::new("netease", &detail.name);
    playlist.remote_id = Some(detail.id);
    playlist.cover_url = detail.coverImgUrl;
    playlist.creator = detail.creator.map(|c| c.nickname);
    playlist.description = detail.description;
    playlist.tags = detail.tags;
    playlist.song_ids = song_ids;

    playlist::add(&ctx.store, &playlist)?;
    ctx.store.save()?;

    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(playlist)
}

/// 获取当前登录账号信息（依赖 cookie）。
///
/// # 返回
/// `{ code, profile: { userId, nickname, avatarUrl, signature } | null,
///    account: { id, userName } | null }`
#[tauri::command]
pub fn netease_get_user_account(
    ctx: State<'_, Arc<AppContext>>,
) -> Result<serde_json::Value, String> {
    // 调试日志：确认 cookie 是否被正确传递到 HTTP 客户端
    let cookie = ctx.netease_source.client().cookie();
    let cookie_preview: String = cookie.chars().take(80).collect();
    eprintln!(
        "[netease] get_user_account 调用，cookie 长度={}，预览=\"{}...\"",
        cookie.len(),
        cookie_preview
    );

    let resp = ctx.netease_source.get_user_account()?;
    eprintln!(
        "[netease] get_user_account 响应：code={}，profile={}，account={}",
        resp.code,
        resp.profile.is_some(),
        resp.account.is_some()
    );

    let json = serde_json::json!({
        "code": resp.code,
        "profile": resp.profile,
        "account": resp.account,
    });
    eprintln!(
        "[netease] get_user_account 返回 JSON：{}",
        serde_json::to_string(&json).unwrap_or_default()
    );
    Ok(json)
}

/// 获取指定用户的全部歌单（创建 + 收藏，通过 `subscribed` 字段区分）。
///
/// # 返回
/// `Vec<NeteasePlaylistBrief>` — 每个 item 含 `subscribed: bool`（true=收藏，false=创建）
#[tauri::command]
pub fn netease_get_user_playlists(
    ctx: State<'_, Arc<AppContext>>,
    uid: i64,
) -> Result<Vec<NeteasePlaylistBrief>, String> {
    ctx.netease_source.get_user_playlists(uid)
}

/// 获取当前用户收藏的专辑（依赖 cookie，分页）。
///
/// # 返回
/// `{ albums: [...], total: number, hasMore: bool }`
#[tauri::command]
pub fn netease_get_user_albums(
    ctx: State<'_, Arc<AppContext>>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<serde_json::Value, String> {
    let lim = limit.unwrap_or(50);
    let off = offset.unwrap_or(0);
    let (albums, total) = ctx.netease_source.get_user_albums(lim, off)?;
    Ok(serde_json::json!({
        "albums": albums,
        "total": total,
        "offset": off,
        "limit": lim,
        "hasMore": (off + albums.len() as u32) < total as u32,
    }))
}

/// 将网易云专辑导入本地音乐库（不创建本地 Playlist，仅把所有曲目加入 library）。
///
/// # 工作流
/// 1. 调用 `get_album_detail` 拉取专辑元信息 + 全部曲目
/// 2. 每首曲目通过 `library.add_song` 加入库（带 netease SourceId，自动合并去重）
/// 3. 触发 `library-changed` 事件
///
/// # 返回
/// `{ album: {...}, imported_count: number }`
#[tauri::command]
pub fn netease_save_album_to_library(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
    album_id: i64,
) -> Result<serde_json::Value, String> {
    let (album, songs) = ctx.netease_source.get_album_detail(album_id)?;

    // 批量构建 Song 后一次性入库（add_songs_batch 只反序列化+回写一次）
    let built: Vec<_> = songs
        .iter()
        .map(|t| ctx.netease_source.build_song_from_detail(t))
        .collect();
    let stored_ids = ctx.library.add_songs_batch(&built)?;
    let imported = stored_ids.len();
    // ⚠ 关键：必须调用 library.save() 落盘 music_library.json，
    // 否则重启后歌曲全部丢失（ctx.store.save() 只保存 storage.json）。
    ctx.library.save()?;
    ctx.store.save()?;

    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(serde_json::json!({
        "album": {
            "id": album.id,
            "name": album.name,
            "picUrl": album.picUrl,
            "artist": album.artist,
            "artists": album.artists,
            "description": album.description,
            "publishTime": album.publishTime,
            "size": album.size,
        },
        "imported_count": imported,
        "total_count": songs.len(),
    }))
}

/// 下载网易云歌曲到本地 download_dir。
///
/// # 参数
/// - `song_entity_id`: 网易云 entity_id（如 `"song:123456"`）
/// - `title`: 歌曲标题（用于文件命名）
/// - `artist`: 主艺术家名（用于文件命名）
/// - `level`: 音质等级（"standard"/"higher"/"exhigh"/"lossless"/"hires"），默认 "exhigh"
///
/// # 返回
/// 下载后的本地文件绝对路径。
#[tauri::command]
pub fn netease_download_song(
    ctx: State<'_, Arc<AppContext>>,
    song_entity_id: String,
    title: String,
    artist: String,
    level: Option<String>,
) -> Result<String, String> {
    let remote_id = song_entity_id
        .splitn(3, ':')
        .nth(1)
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| format!("无效的 entity_id: {}", song_entity_id))?;

    let level = level.unwrap_or_else(|| AudioQuality::Exhigh.to_level_str().to_string());

    // 推断扩展名：优先 url_data.type，其次 level（lossless→flac），最后默认 mp3
    // 但扩展名需要先调 /song/url/v1 才知道，为避免两次 API 调用，
    // 这里按 level 预推断扩展名，url_data.type 仅用于校验
    let ext = match level.as_str() {
        "lossless" | "hires" => "flac",
        _ => "mp3",
    };

    // 解析 download_dir：优先 netease 配置；为空则回退到 app_data_dir/downloads
    let download_dir_str = ctx.netease_source.download_dir();
    let download_dir = if download_dir_str.is_empty() {
        let config_path = ctx.config.path();
        let parent = config_path
            .parent()
            .ok_or_else(|| "无法定位应用数据目录".to_string())?;
        parent.join("downloads")
    } else {
        std::path::PathBuf::from(download_dir_str)
    };

    // 先构建目标文件路径（创建父目录），再流式下载到文件
    let path = cache::prepare_download_path(&download_dir, "netease", &artist, &title, ext)?;

    // 流式下载到文件（避免大文件全量读入内存）
    // 旧实现：get_bytes 返回 Vec<u8> → write_downloaded_file 写盘（50MB FLAC 占 50MB 内存）
    // 新实现：download_song_to_file 直接流式写入文件（8KB 缓冲区）
    let (written, _url_data) = music_netease_source::api::download_song_to_file(
        ctx.netease_source.client(),
        remote_id,
        &level,
        &path,
    )?;

    eprintln!(
        "[netease] download_song id={} level={} 写入 {} 字节到 {}",
        remote_id,
        level,
        written,
        path.display()
    );

    Ok(path.to_string_lossy().to_string())
}

/// 批量下载请求项（前端传入）。
#[derive(Deserialize, Clone)]
pub struct BatchDownloadItem {
    /// 网易云 entity_id（如 `"song:123456"`）
    song_entity_id: String,
    /// 歌曲标题（用于文件命名）
    title: String,
    /// 主艺术家名（用于文件命名）
    artist: String,
    /// 音质等级（"standard"/"higher"/"exhigh"/"lossless"/"hires"），None 默认 exhigh
    #[serde(default)]
    level: Option<String>,
}

/// 批量下载去重策略（前端传入）。
///
/// - `none`: 不去重，直接下载（默认）
/// - `skip_if_equal`: 本地与目标音质相同时跳过；本地更高也跳过
/// - `replace_if_better`: 本地音质低于在线时下载新版本并删除本地旧文件；
///   本地音质 ≥ 在线时跳过
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DedupStrategy {
    /// 不去重（默认）
    None,
    /// 本地已有相同或更高音质 → 跳过
    SkipIfEqual,
    /// 本地音质 < 在线 → 下载并替换（删除旧文件）
    /// 本地音质 ≥ 在线 → 跳过
    ReplaceIfBetter,
}

impl Default for DedupStrategy {
    fn default() -> Self {
        Self::None
    }
}

/// 批量下载进度事件 payload。
#[derive(serde::Serialize, Clone)]
struct DownloadProgress {
    /// 任务 ID（与启动时返回的一致）
    task_id: String,
    /// 当前完成的索引（0-based）
    index: usize,
    /// 总曲目数
    total: usize,
    /// 状态：completed / failed / cancelled / skipped / replaced
    status: &'static str,
    /// 当前曲目标题（前端展示用）
    title: String,
    /// 当前曲目艺术家
    artist: String,
    /// 成功时的文件路径
    path: Option<String>,
    /// 失败时的错误信息
    error: Option<String>,
    /// 已成功下载数（含 replaced）
    succeeded: usize,
    /// 已失败数
    failed: usize,
    /// 已跳过数（含 SkipIfEqual 与 ReplaceIfBetter 中的"本地已更好"场景）
    skipped: usize,
    /// 实际下载到的音质等级（如 "exhigh"），降级时可能与目标不同
    actual_level: Option<String>,
    /// 单曲处理动作描述（前端展示用）：downloaded / skipped / replaced
    action: Option<&'static str>,
}

/// 批量下载网易云歌曲到本地（后台静默执行）。
///
/// 命令立即返回 `task_id`，下载在后台线程逐首进行。每完成一首（成功/失败/跳过/替换）
/// 通过 `netease-download-progress` 事件推送进度，前端 listen 该事件更新 UI。
///
/// # 参数
/// - `songs`: 待下载曲目列表（每个含 song_entity_id/title/artist/level）
/// - `target_level`: 全局目标音质，覆盖每个 item 的 level。None 时使用 item.level
/// - `dedup_strategy`: 去重策略，详见 [`DedupStrategy`]
///
/// # 音质降级
/// 每首曲目按目标音质开始尝试，若该等级不可用（VIP/无版权/URL 为空），
/// 自动向下降级（hires→lossless→exhigh→higher→standard），直到成功或全部失败。
/// 实际下载到的等级通过 `actual_level` 字段返回。
///
/// # 去重逻辑
/// - `none`: 直接下载
/// - `skip_if_equal`: 查询本地匹配，若有相同或更高音质 → 跳过
/// - `replace_if_better`: 查询本地匹配，若本地音质 < 目标 → 下载并删除本地旧文件；
///   若本地音质 ≥ 目标 → 跳过
///
/// # 返回
/// `{ task_id: string, total: number }` — 任务 ID + 总曲目数
#[tauri::command]
pub fn netease_download_songs_batch(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
    songs: Vec<BatchDownloadItem>,
    target_level: Option<String>,
    dedup_strategy: Option<DedupStrategy>,
) -> Result<serde_json::Value, String> {
    if songs.is_empty() {
        return Err("待下载列表为空".to_string());
    }

    let total = songs.len();
    let task_id = format!(
        "dl-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        total
    );

    let cancel_flag = register_download_task(&task_id);

    // 解析 download_dir（在主线程做，避免后台线程访问 State）
    let download_dir_str = ctx.netease_source.download_dir();
    let download_dir = if download_dir_str.is_empty() {
        let config_path = ctx.config.path();
        let parent = config_path
            .parent()
            .ok_or_else(|| "无法定位应用数据目录".to_string())?;
        parent.join("downloads")
    } else {
        std::path::PathBuf::from(download_dir_str)
    };

    let strategy = dedup_strategy.unwrap_or_default();

    // 克隆 Arc<AppContext> 到后台线程
    let ctx = ctx.inner().clone();
    let app_handle = app.clone();
    let task_id_for_return = task_id.clone();

    std::thread::spawn(move || {
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;

        // 辅助闭包：emit 进度事件（减少重复代码）
        let emit_progress = |app: &AppHandle,
                             task_id: &str,
                             index: usize,
                             total: usize,
                             status: &'static str,
                             title: &str,
                             artist: &str,
                             path: Option<String>,
                             error: Option<String>,
                             succeeded: usize,
                             failed: usize,
                             skipped: usize,
                             actual_level: Option<String>,
                             action: Option<&'static str>| {
            let _ = app.emit(
                DOWNLOAD_PROGRESS_EVENT,
                DownloadProgress {
                    task_id: task_id.to_string(),
                    index,
                    total,
                    status,
                    title: title.to_string(),
                    artist: artist.to_string(),
                    path,
                    error,
                    succeeded,
                    failed,
                    skipped,
                    actual_level,
                    action,
                },
            );
        };

        for (index, item) in songs.iter().enumerate() {
            // 检查取消标志
            if cancel_flag.load(Ordering::SeqCst) {
                emit_progress(
                    &app_handle,
                    &task_id,
                    index,
                    total,
                    "cancelled",
                    &item.title,
                    &item.artist,
                    None,
                    Some("用户取消".to_string()),
                    succeeded,
                    failed,
                    skipped,
                    None,
                    None,
                );
                eprintln!(
                    "[netease] 批量下载 task={} 被取消（已完成 {}/{}）",
                    task_id, index, total
                );
                unregister_download_task(&task_id);
                return;
            }

            // 目标音质：优先 target_level，其次 item.level，最后默认 exhigh
            let level_str = target_level
                .as_deref()
                .or(item.level.as_deref())
                .map(|s| s.to_string())
                .unwrap_or_else(|| AudioQuality::Exhigh.to_level_str().to_string());

            // 解析 remote_id
            let remote_id = item.song_entity_id.splitn(3, ':').nth(1).and_then(|s| s.parse::<i64>().ok());
            let remote_id = match remote_id {
                Some(id) => id,
                None => {
                    failed += 1;
                    emit_progress(
                        &app_handle,
                        &task_id,
                        index,
                        total,
                        "failed",
                        &item.title,
                        &item.artist,
                        None,
                        Some(format!("无效的 entity_id: {}", item.song_entity_id)),
                        succeeded,
                        failed,
                        skipped,
                        None,
                        None,
                    );
                    continue;
                }
            };

            // ── 去重检查 ──
            // 策略为 SkipIfEqual / ReplaceIfBetter 时，先查询本地匹配
            if strategy != DedupStrategy::None {
                let local_matches = find_local_matches(&ctx, &item.title, &item.artist);
                if let Some(best_local) = best_local_quality(&local_matches) {
                    let target_quality = AudioQuality::from_level_str(&level_str);
                    let comparison = cache::compare_quality(best_local.quality, target_quality);

                    let should_skip = match strategy {
                        DedupStrategy::SkipIfEqual => {
                            // 本地 ≥ 目标 → 跳过
                            matches!(
                                comparison,
                                cache::QualityComparison::LocalBetter | cache::QualityComparison::Equal
                            )
                        }
                        DedupStrategy::ReplaceIfBetter => {
                            // 本地 ≥ 目标 → 跳过；本地 < 目标 → 下载并替换
                            matches!(
                                comparison,
                                cache::QualityComparison::LocalBetter | cache::QualityComparison::Equal
                            )
                        }
                        DedupStrategy::None => false,
                    };

                    if should_skip {
                        skipped += 1;
                        emit_progress(
                            &app_handle,
                            &task_id,
                            index,
                            total,
                            "skipped",
                            &item.title,
                            &item.artist,
                            best_local.file_path.clone(),
                            Some(format!(
                                "本地已有 {} 音质（{}）",
                                best_local.quality.to_level_str(),
                                best_local.quality_label
                            )),
                            succeeded,
                            failed,
                            skipped,
                            None,
                            Some("skipped"),
                        );
                        continue;
                    }
                }
            }

            // ── 下载（含降级） ──
            // 扩展名按目标音质预推断；实际下载到的音质可能不同，但文件名保留目标等级标识
            let ext = match level_str.as_str() {
                "lossless" | "hires" => "flac",
                _ => "mp3",
            };

            let download_result: Result<(PathBuf, String), String> = (|| {
                let path = cache::prepare_download_path(
                    &download_dir,
                    "netease",
                    &item.artist,
                    &item.title,
                    ext,
                )?;
                let result = music_netease_source::api::download_song_to_file_with_fallback(
                    ctx.netease_source.client(),
                    remote_id,
                    Some(level_str.as_str()),
                    &path,
                )?;
                Ok((path, result.level))
            })();

            match download_result {
                Ok((path, actual_level)) => {
                    succeeded += 1;

                    // ── 替换策略：删除本地旧文件 ──
                    let mut replaced_paths: Vec<String> = Vec::new();
                    if strategy == DedupStrategy::ReplaceIfBetter {
                        let local_matches = find_local_matches(&ctx, &item.title, &item.artist);
                        for m in local_matches.iter() {
                            // 跳过刚下载的新文件
                            if m.file_path.as_deref() == Some(path.to_str().unwrap_or("")) {
                                continue;
                            }
                            if let Some(fp) = m.file_path.as_ref() {
                                if let Ok(true) = ctx.library.delete_local_file_and_source(fp) {
                                    replaced_paths.push(fp.clone());
                                }
                            }
                        }
                    }

                    let action: Option<&'static str> = if !replaced_paths.is_empty() {
                        Some("replaced")
                    } else {
                        Some("downloaded")
                    };

                    eprintln!(
                        "[netease] 批量下载 task={} {}/{} 成功：{} - {}（实际音质 {}，替换 {} 个旧文件）",
                        task_id,
                        index + 1,
                        total,
                        item.artist,
                        item.title,
                        actual_level,
                        replaced_paths.len()
                    );

                    emit_progress(
                        &app_handle,
                        &task_id,
                        index,
                        total,
                        if replaced_paths.is_empty() { "completed" } else { "replaced" },
                        &item.title,
                        &item.artist,
                        Some(path.to_string_lossy().to_string()),
                        None,
                        succeeded,
                        failed,
                        skipped,
                        Some(actual_level),
                        action,
                    );
                }
                Err(e) => {
                    failed += 1;
                    eprintln!(
                        "[netease] 批量下载 task={} {}/{} 失败：{} - {}：{}",
                        task_id, index + 1, total, item.artist, item.title, e
                    );
                    emit_progress(
                        &app_handle,
                        &task_id,
                        index,
                        total,
                        "failed",
                        &item.title,
                        &item.artist,
                        None,
                        Some(e),
                        succeeded,
                        failed,
                        skipped,
                        None,
                        None,
                    );
                }
            }
        }

        eprintln!(
            "[netease] 批量下载 task={} 完成：成功 {}，失败 {}，跳过 {}，总计 {}",
            task_id, succeeded, failed, skipped, total
        );
        unregister_download_task(&task_id);
    });

    Ok(serde_json::json!({
        "task_id": task_id_for_return,
        "total": total,
    }))
}

/// 本地匹配项（用于去重比较）。
struct LocalMatch {
    file_path: Option<String>,
    quality: AudioQuality,
    quality_label: String,
}

/// 按标题+艺术家在本地库中查找匹配歌曲，返回每个匹配项的文件路径与推断音质。
///
/// 复用 `library.search_songs` 的模糊匹配能力，结果与 `source_compare_with_local`
/// 命令保持一致，但返回强类型 [`LocalMatch`] 便于后续比较。
fn find_local_matches(ctx: &Arc<AppContext>, title: &str, artist: &str) -> Vec<LocalMatch> {
    let title_lower = title.to_lowercase();
    let artist_lower = artist.to_lowercase();

    let candidates = ctx.library.search_songs(title);
    candidates
        .into_iter()
        .filter(|s| s.title.to_lowercase().contains(&title_lower))
        .filter(|s| {
            if artist.is_empty() {
                return true;
            }
            s.artist_names
                .iter()
                .any(|a| a.to_lowercase().contains(&artist_lower))
        })
        .filter_map(|s| {
            let local_path = s
                .source_ids
                .iter()
                .find(|sid| sid.source_name == "local")
                .map(|sid| sid.entity_id.clone());
            let descriptor = local_path
                .as_ref()
                .and_then(|p| std::path::Path::new(p).extension())
                .and_then(|e| e.to_str())
                .map(AudioQualityDescriptor::from_extension)
                .unwrap_or_else(AudioQualityDescriptor::unknown);
            let quality = AudioQuality::from(descriptor.rank);
            Some(LocalMatch {
                file_path: local_path,
                quality_label: descriptor.label(),
                quality,
            })
        })
        .collect()
}

/// 从本地匹配项中选出音质最高的（rank 最大；同 rank 任选其一）。
fn best_local_quality(matches: &[LocalMatch]) -> Option<&LocalMatch> {
    matches
        .iter()
        .max_by(|a, b| a.quality.cmp(&b.quality))
}

/// 取消指定的批量下载任务。
///
/// # 参数
/// - `task_id`: `netease_download_songs_batch` 返回的任务 ID
///
/// # 返回
/// `true` 表示找到并标记取消，`false` 表示任务不存在或已完成。
#[tauri::command]
pub fn netease_cancel_download_task(task_id: String) -> Result<bool, String> {
    Ok(cancel_download_task(&task_id))
}

/// 比较 online 歌曲与本地库中相同歌曲的音质。
///
/// 通过 标题+艺术家 模糊匹配本地库，返回所有匹配项及其推断音质。
#[tauri::command]
pub fn netease_compare_with_local(
    ctx: State<'_, Arc<AppContext>>,
    title: String,
    artist: String,
    _album: Option<String>,
) -> Result<serde_json::Value, String> {
    let title_lower = title.to_lowercase();
    let artist_lower = artist.to_lowercase();

    let candidates = ctx.library.search_songs(&title);
    let matches: Vec<serde_json::Value> = candidates
        .into_iter()
        .filter(|s| s.title.to_lowercase().contains(&title_lower))
        .filter(|s| {
            if artist.is_empty() {
                return true;
            }
            s.artist_names
                .iter()
                .any(|a| a.to_lowercase().contains(&artist_lower))
        })
        .map(|s| {
            let local_path = s
                .source_ids
                .iter()
                .find(|sid| sid.source_name == "local")
                .map(|sid| sid.entity_id.clone());
            let descriptor = local_path
                .as_ref()
                .and_then(|p| std::path::Path::new(p).extension())
                .and_then(|e| e.to_str())
                .map(AudioQualityDescriptor::from_extension)
                .unwrap_or_else(AudioQualityDescriptor::unknown);
            let quality = AudioQuality::from(descriptor.rank);
            serde_json::json!({
                "song_id": s.id,
                "title": s.title,
                "artist_names": s.artist_names,
                "album_title": s.album_title,
                "file_path": local_path,
                "quality": quality.to_level_str(),
                "quality_rank": quality as u8,
                "quality_descriptor": descriptor,
                "quality_label": descriptor.label(),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "matches": matches,
        "has_local_match": !matches.is_empty(),
    }))
}

// ══════════════════════════════════════════════════════════════════════════════
// 酷狗音乐来源命令 — 配置/测试/搜索
// ══════════════════════════════════════════════════════════════════════════════

/// 获取酷狗来源配置（base_url / cookie / 超时 / 启用 / 下载目录）。
#[tauri::command]
pub fn kugou_get_config(ctx: State<'_, Arc<AppContext>>) -> Result<WebSourceConfig, String> {
    Ok(ctx.kugou_source.config())
}

/// 更新酷狗来源配置并持久化。
///
/// 重建 HTTP 客户端以应用新的 base_url / cookie / timeout。
#[tauri::command]
pub fn kugou_update_config(
    ctx: State<'_, Arc<AppContext>>,
    config: WebSourceConfig,
) -> Result<(), String> {
    ctx.kugou_source.update_config(config)
}

/// 测试与酷狗 API 服务器的连通性。
///
/// 通过发起一次 page=1 pagesize=1 的搜索请求验证 base_url + cookie 是否有效。
#[tauri::command]
pub fn kugou_test_connection(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let cfg = ctx.kugou_source.config();
    if cfg.base_url.is_empty() {
        return Err("API base_url 未配置".to_string());
    }
    let result = ctx.kugou_source.search_songs("test");
    Ok(serde_json::json!({
        "ok": result.is_ok(),
        "error": result.err(),
        "base_url": cfg.base_url,
    }))
}

/// 在线搜索歌曲（返回库内 Song 模型，附带 kugou SourceId）。
#[tauri::command]
pub fn kugou_search_songs(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
) -> Result<Vec<Song>, String> {
    ctx.kugou_source.search_songs(&query)
}

/// 在线搜索歌手（返回库内 Artist 模型）。
///
/// 酷狗 API 使用 1-based page/pagesize；此处将前端 limit/offset 转换为对应分页。
#[tauri::command]
pub fn kugou_search_artists(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<Artist>, String> {
    let pagesize = limit.unwrap_or(30);
    let offset_val = offset.unwrap_or(0);
    let page = (offset_val / pagesize.max(1)) + 1;
    ctx.kugou_source.search_artists(&query, page, pagesize)
}

/// 在线搜索专辑（返回库内 Album 模型）。
///
/// 酷狗 API 使用 1-based page/pagesize；此处将前端 limit/offset 转换为对应分页。
#[tauri::command]
pub fn kugou_search_albums(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<Album>, String> {
    let pagesize = limit.unwrap_or(30);
    let offset_val = offset.unwrap_or(0);
    let page = (offset_val / pagesize.max(1)) + 1;
    ctx.kugou_source.search_albums(&query, page, pagesize)
}

/// 酷狗扫码登录第 1 步：获取 key。
///
/// 返回 `String` — key，用于后续生成二维码与轮询。
#[tauri::command]
pub fn kugou_login_qr_key(ctx: State<'_, Arc<AppContext>>) -> Result<String, String> {
    ctx.kugou_source.login_qr_key()
}

/// 酷狗扫码登录第 2 步：用 key 生成二维码。
///
/// # 参数
/// - `key`: 从 `kugou_login_qr_key` 获取的 key
/// - `qrimg`: 是否返回 base64 data URL
///
/// # 返回
/// `{ url: string, base64: string }`
#[tauri::command]
pub fn kugou_login_qr_create(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
    qrimg: Option<bool>,
) -> Result<serde_json::Value, String> {
    use chordial_core::module::music_kugou_source::types::KugouQrCheckStatus;
    let data = ctx.kugou_source.login_qr_create(&key, qrimg.unwrap_or(true))?;
    Ok(serde_json::json!({
        "url": data.url,
        "base64": data.base64,
    }))
}

/// 酷狗扫码登录第 3 步：轮询扫码状态。
///
/// # 返回
/// `{ status: number, statusText: string, token: string|null, userid: number|null }`
///
/// 当 `status == 4` 时后端会自动持久化 cookie 到配置。
#[tauri::command]
pub fn kugou_login_qr_check(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
) -> Result<serde_json::Value, String> {
    use chordial_core::module::music_kugou_source::types::KugouQrCheckStatus;
    let resp = ctx.kugou_source.login_qr_check(&key)?;
    let status_enum = KugouQrCheckStatus::from(resp.data.status);
    Ok(serde_json::json!({
        "status": resp.data.status,
        "statusText": status_enum.as_str(),
        "token": resp.data.token,
        "userid": resp.data.userid,
    }))
}

// ══════════════════════════════════════════════════════════════════════════════
// QQ 音乐来源命令 — 配置/测试/搜索
// ══════════════════════════════════════════════════════════════════════════════

/// 获取 QQ 来源配置（base_url / cookie / 超时 / 启用 / 下载目录）。
#[tauri::command]
pub fn qq_get_config(ctx: State<'_, Arc<AppContext>>) -> Result<WebSourceConfig, String> {
    Ok(ctx.qq_source.config())
}

/// 更新 QQ 来源配置并持久化。
///
/// 重建 HTTP 客户端以应用新的 base_url / cookie / timeout。
#[tauri::command]
pub fn qq_update_config(
    ctx: State<'_, Arc<AppContext>>,
    config: WebSourceConfig,
) -> Result<(), String> {
    ctx.qq_source.update_config(config)
}

/// 测试与 QQ API 服务器的连通性。
///
/// 通过发起一次 pageNo=1 pageSize=1 的搜索请求验证 base_url + cookie 是否有效。
#[tauri::command]
pub fn qq_test_connection(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let cfg = ctx.qq_source.config();
    if cfg.base_url.is_empty() {
        return Err("API base_url 未配置".to_string());
    }
    let result = ctx.qq_source.search_songs("test");
    Ok(serde_json::json!({
        "ok": result.is_ok(),
        "error": result.err(),
        "base_url": cfg.base_url,
    }))
}

/// 在线搜索 QQ 歌曲（返回库内 Song 模型，附带 qq SourceId）。
#[tauri::command]
pub fn qq_search_songs(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
) -> Result<Vec<Song>, String> {
    ctx.qq_source.search_songs(&query)
}

/// 在线搜索 QQ 歌手（返回库内 Artist 模型）。
///
/// QQ 音乐 API 使用 1-based pageNo/pageSize；此处将前端 limit/offset 转换为对应分页。
#[tauri::command]
pub fn qq_search_artists(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<Artist>, String> {
    let page_size = limit.unwrap_or(30);
    let offset_val = offset.unwrap_or(0);
    let page_no = (offset_val / page_size.max(1)) + 1;
    ctx.qq_source.search_artists(&query, page_no, page_size)
}

/// 在线搜索 QQ 专辑（返回库内 Album 模型）。
///
/// QQ 音乐 API 使用 1-based pageNo/pageSize；此处将前端 limit/offset 转换为对应分页。
#[tauri::command]
pub fn qq_search_albums(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<Album>, String> {
    let page_size = limit.unwrap_or(30);
    let offset_val = offset.unwrap_or(0);
    let page_no = (offset_val / page_size.max(1)) + 1;
    ctx.qq_source.search_albums(&query, page_no, page_size)
}

// ══════════════════════════════════════════════════════════════════════════════
// 通用云端来源命令 — 从网络来源实例选择一个云端音乐源进行下载/比较
// ══════════════════════════════════════════════════════════════════════════════

use chordial_core::module::music_source::types::AudioQualityDescriptor;

/// 列出已注册的云端（Web）音乐来源。
///
/// 用于下载对话框的"来源选择"下拉框。仅返回已通过 `registrar.register` 注册的 Web 类型来源。
/// 本地来源不会出现在此列表中。
///
/// # 返回
/// `[{ name, display_name, source_type, enabled }]`
#[tauri::command]
pub fn source_list_web_sources(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<serde_json::Value>, String> {
    let entries = ctx.registrar.get_entries();
    let result: Vec<serde_json::Value> = entries
        .into_iter()
        .filter(|e| e.source_type.is_web())
        .map(|e| {
            // 显示名映射：未来可通过 trait 方法提供；当前硬编码 netease/kugou/qq
            let display_name = match e.name.as_str() {
                "netease" => "网易云音乐",
                "kugou" => "酷狗音乐",
                "qq" => "QQ 音乐",
                _ => &e.name,
            };
            serde_json::json!({
                "name": e.name,
                "display_name": display_name,
                "source_type": match &e.source_type {
                    chordial_core::module::music_source::types::SourceType::Web(name) => name.clone(),
                    _ => String::new(),
                },
                "enabled": true, // 已注册即视为启用
            })
        })
        .collect();
    Ok(result)
}

/// 从指定云端来源下载歌曲到本地 download_dir。
///
/// # 参数
/// - `source_name`: 来源名（如 `"netease"`），必须为已注册的 Web 来源
/// - `song_entity_id`: 来源内部的歌曲 entity_id（如 `"song:123456"`）
/// - `title`: 歌曲标题（用于文件命名）
/// - `artist`: 主艺术家名（用于文件命名）
/// - `level`: 音质等级（可选，默认 `"exhigh"`）
///
/// # 返回
/// `{ path: string, quality_descriptor: AudioQualityDescriptor }`
#[tauri::command]
pub fn source_download_song(
    ctx: State<'_, Arc<AppContext>>,
    source_name: String,
    song_entity_id: String,
    title: String,
    artist: String,
    level: Option<String>,
) -> Result<serde_json::Value, String> {
    // 校验来源已注册
    let _ = ctx
        .registrar
        .get(&source_name)
        .ok_or_else(|| format!("来源 '{}' 未注册或未启用", source_name))?;

    use chordial_core::module::music_kugou_source;
    use chordial_core::module::music_netease_source;
    use chordial_core::module::music_qq_source;

    // 分支处理：netease / kugou / qq 各自走对应的下载 API
    let (bytes, quality_descriptor, ext, download_dir_str) = if source_name
        == music_netease_source::NETEASE_SOURCE_NAME
    {
        // ── 网易云下载 ──
        let remote_id = song_entity_id
            .splitn(3, ':')
            .nth(1)
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| format!("无效的 entity_id: {}", song_entity_id))?;

        let level_str = level.unwrap_or_else(|| AudioQuality::Exhigh.to_level_str().to_string());
        let (b, url_data) = music_netease_source::api::download_song(
            ctx.netease_source.client(),
            remote_id,
            &level_str,
        )?;
        let qd = url_data.to_quality_descriptor();
        let e = url_data
            .r#type
            .as_deref()
            .and_then(|t| match t.to_lowercase().as_str() {
                "flac" => Some("flac"),
                "mp3" => Some("mp3"),
                "m4a" => Some("m4a"),
                _ => None,
            })
            .unwrap_or_else(|| match level_str.as_str() {
                "lossless" | "hires" => "flac",
                _ => "mp3",
            });
        (b, qd, e.to_string(), ctx.netease_source.download_dir())
    } else if source_name == music_kugou_source::KUGOU_SOURCE_NAME {
        // ── 酷狗下载 ──
        // entity_id 格式：song:<hash>，hash 是 32 位十六进制字符串
        let hash = song_entity_id
            .splitn(3, ':')
            .nth(1)
            .ok_or_else(|| format!("无效的 entity_id: {}", song_entity_id))?;
        if hash.is_empty() {
            return Err(format!("无效的 hash: {}", song_entity_id));
        }
        // level 可选：酷狗音质字符串（"128"/"320"/"flac"/"high"/"super"），默认 "320"
        let quality_str = level.unwrap_or_else(|| "320".to_string());
        let (b, url_data) = music_kugou_source::api::download_song(
            ctx.kugou_source.client(),
            hash,
            &quality_str,
            None,
        )?;
        // 从酷狗返回的 URL 数据构造音质描述符
        let url_entry = url_data
            .url
            .first()
            .ok_or_else(|| "酷狗返回空 URL 列表".to_string())?;
        let qd = build_kugou_quality_descriptor(&url_entry.quality, url_entry.bitRate, &url_entry.r#type);
        let e = match url_entry.r#type.to_lowercase().as_str() {
            "flac" => "flac",
            "mp3" => "mp3",
            "m4a" | "aac" => "m4a",
            _ => match url_entry.quality.as_str() {
                "flac" | "high" | "super" => "flac",
                _ => "mp3",
            },
        };
        (b, qd, e.to_string(), ctx.kugou_source.download_dir())
    } else if source_name == music_qq_source::QQ_SOURCE_NAME {
        // ── QQ 音乐下载 ──
        // entity_id 格式：song:<songmid>，songmid 是字符串标识
        let songmid = song_entity_id
            .splitn(3, ':')
            .nth(1)
            .ok_or_else(|| format!("无效的 entity_id: {}", song_entity_id))?;
        if songmid.is_empty() {
            return Err(format!("无效的 songmid: {}", song_entity_id));
        }
        // level 可选：QQ 音质字符串（"128"/"320"/"m4a"/"ape"/"flac"），默认 "320"
        let quality_str = level.unwrap_or_else(|| "320".to_string());
        let (b, _url_data) = music_qq_source::api::download_song(
            ctx.qq_source.client(),
            songmid,
            &quality_str,
        )?;
        let qd = build_qq_quality_descriptor(&quality_str);
        let e = match quality_str.to_lowercase().as_str() {
            "flac" | "ape" => "flac",
            "m4a" | "aac" => "m4a",
            _ => "mp3",
        };
        (b, qd, e.to_string(), ctx.qq_source.download_dir())
    } else {
        return Err(format!(
            "来源 '{}' 暂不支持下载（仅支持 netease/kugou/qq）",
            source_name
        ));
    };

    // 解析 download_dir：优先来源配置；为空则回退到 app_data_dir/downloads
    let download_dir = if download_dir_str.is_empty() {
        let config_path = ctx.config.path();
        let parent = config_path
            .parent()
            .ok_or_else(|| "无法定位应用数据目录".to_string())?;
        parent.join("downloads")
    } else {
        std::path::PathBuf::from(download_dir_str)
    };

    let path = cache::write_downloaded_file(&download_dir, &source_name, &artist, &title, &ext, &bytes)?;

    Ok(serde_json::json!({
        "path": path.to_string_lossy(),
        "quality_descriptor": quality_descriptor,
        "quality_label": quality_descriptor.label(),
    }))
}

/// 从酷狗返回数据构造音质描述符。
///
/// 酷狗 quality 字段为字符串（"128"/"320"/"flac"/"high"/"super"），
/// 映射到内部 [`AudioQuality`] 后再生成描述符。
fn build_kugou_quality_descriptor(
    quality: &str,
    bit_rate: i64,
    file_type: &str,
) -> AudioQualityDescriptor {
    use chordial_core::module::music_source::types::{AudioCodec, AudioQuality};
    let aq = match quality.to_lowercase().as_str() {
        "128" => AudioQuality::Standard,
        "320" => AudioQuality::Exhigh,
        "flac" | "high" => AudioQuality::Lossless,
        "super" => AudioQuality::Hires,
        _ => AudioQuality::Unknown,
    };
    let mut desc = aq.descriptor();
    // 若接口返回了真实码率，覆盖默认值
    if bit_rate > 0 {
        desc.bitrate_kbps = bit_rate as u32;
    }
    // 若接口返回了真实编码格式，覆盖默认值
    if !file_type.is_empty() {
        desc.codec = match file_type.to_lowercase().as_str() {
            "flac" => AudioCodec::Flac,
            "mp3" => AudioCodec::Mp3,
            "m4a" | "aac" => AudioCodec::Aac,
            _ => desc.codec,
        };
    }
    desc
}

/// 从 QQ 音乐请求音质字符串构造音质描述符。
///
/// QQ 音乐 quality 字符串（`"128"` / `"320"` / `"m4a"` / `"ape"` / `"flac"`）映射到内部 [`AudioQuality`]。
/// 由于 `/song/url` 仅返回 URL 字符串而无码率/编码等元数据，这里基于请求音质推断描述符。
fn build_qq_quality_descriptor(quality: &str) -> AudioQualityDescriptor {
    use chordial_core::module::music_source::types::{AudioCodec, AudioQuality};
    let (aq, codec) = match quality.to_lowercase().as_str() {
        "128" => (AudioQuality::Standard, AudioCodec::Mp3),
        "320" => (AudioQuality::Exhigh, AudioCodec::Mp3),
        "m4a" => (AudioQuality::Higher, AudioCodec::Aac),
        "ape" => (AudioQuality::Lossless, AudioCodec::Ape),
        "flac" => (AudioQuality::Lossless, AudioCodec::Flac),
        _ => (AudioQuality::Unknown, AudioCodec::Unknown),
    };
    let mut desc = aq.descriptor();
    desc.codec = codec;
    desc
}

/// 比较 online 歌曲与本地库中相同歌曲的音质（通用版本）。
///
/// 与 [`netease_compare_with_local`] 行为一致，但接受 `source_name` 参数以便未来支持多来源。
/// 当前仅 netease 来源可用，其他来源返回空匹配。
///
/// # 返回
/// `{ matches: [...], has_local_match: bool }`
/// 每个 match 包含完整的 `quality_descriptor`（rank + bitrate_kbps + codec + 采样率 + 位深）
#[tauri::command]
pub fn source_compare_with_local(
    ctx: State<'_, Arc<AppContext>>,
    source_name: String,
    title: String,
    artist: String,
    album: Option<String>,
) -> Result<serde_json::Value, String> {
    // 校验来源已注册
    if ctx.registrar.get(&source_name).is_none() {
        return Err(format!("来源 '{}' 未注册或未启用", source_name));
    }

    let title_lower = title.to_lowercase();
    let artist_lower = artist.to_lowercase();
    let album_lower = album
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);

    let candidates = ctx.library.search_songs(&title);
    let matches: Vec<serde_json::Value> = candidates
        .into_iter()
        .filter(|s| s.title.to_lowercase().contains(&title_lower))
        .filter(|s| {
            if artist.is_empty() {
                return true;
            }
            s.artist_names
                .iter()
                .any(|a| a.to_lowercase().contains(&artist_lower))
        })
        .filter(|s| match (&album_lower, &s.album_title) {
            (None, _) | (_, None) => true,
            (Some(al), Some(at)) => at.to_lowercase().contains(al),
        })
        .map(|s| {
            let local_path = s
                .source_ids
                .iter()
                .find(|sid| sid.source_name == "local")
                .map(|sid| sid.entity_id.clone());
            let descriptor = local_path
                .as_ref()
                .and_then(|p| std::path::Path::new(p).extension())
                .and_then(|e| e.to_str())
                .map(AudioQualityDescriptor::from_extension)
                .unwrap_or_else(AudioQualityDescriptor::unknown);
            let quality = AudioQuality::from(descriptor.rank);
            serde_json::json!({
                "song_id": s.id,
                "title": s.title,
                "artist_names": s.artist_names,
                "album_title": s.album_title,
                "file_path": local_path,
                "quality": quality.to_level_str(),
                "quality_rank": quality as u8,
                "quality_descriptor": descriptor,
                "quality_label": descriptor.label(),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "matches": matches,
        "has_local_match": !matches.is_empty(),
    }))
}

/// 扫码登录第 1 步：获取 unikey。
///
/// 返回 `String` — 32 位 unikey，用于后续生成二维码与轮询。
#[tauri::command]
pub fn netease_login_qr_key(ctx: State<'_, Arc<AppContext>>) -> Result<String, String> {
    ctx.netease_source.login_qr_key()
}

/// 扫码登录第 2 步：用 unikey 生成二维码。
///
/// # 参数
/// - `key`: 从 `netease_login_qr_key` 获取的 unikey
/// - `qrimg`: 是否返回 base64 data URL（直接作为 `<img src>`）
///
/// # 返回
/// `{ qrurl: string, qrimg: string|null }`
#[tauri::command]
pub fn netease_login_qr_create(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
    qrimg: Option<bool>,
) -> Result<serde_json::Value, String> {
    let data = ctx.netease_source.login_qr_create(&key, qrimg.unwrap_or(true))?;
    Ok(serde_json::json!({
        "qrurl": data.qrurl,
        "qrimg": data.qrimg,
    }))
}

/// 扫码登录第 3 步：轮询扫码状态。
///
/// # 返回
/// `{ code: number, status: string, message: string|null, avatarUrl: string|null,
///    nickname: string|null, cookie: string|null }`
///
/// 当 `code == 803` 时后端会自动持久化 cookie 到配置，调用方无需再保存。
#[tauri::command]
pub fn netease_login_qr_check(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
) -> Result<serde_json::Value, String> {
    let resp = ctx.netease_source.login_qr_check(&key)?;
    Ok(serde_json::json!({
        "code": resp.code,
        "status": resp.status().as_str(),
        "message": resp.message,
        "avatarUrl": resp.avatarUrl,
        "nickname": resp.nickname,
        "cookie": resp.cookie,
    }))
}

// ══════════════════════════════════════════════════════════════════════════════
// 歌词优先级命令 — 多来源歌词选择最佳
// ══════════════════════════════════════════════════════════════════════════════

use chordial_core::module::lyric_preference::{self, LyricCandidate, LyricPreference, LyricSourceKind};

/// 获取歌词优先级配置。
#[tauri::command]
pub fn lyric_preference_get(ctx: State<'_, Arc<AppContext>>) -> Result<LyricPreference, String> {
    Ok(LyricPreference::load(&ctx.config))
}

/// 更新歌词优先级配置（自动防抖落盘到 config.json）。
#[tauri::command]
pub fn lyric_preference_set(
    ctx: State<'_, Arc<AppContext>>,
    preference: LyricPreference,
) -> Result<(), String> {
    preference.save(&ctx.config)
}

/// 列出所有可用的歌词来源（用于设置页构建可拖拽优先级列表）。
///
/// 返回顺序：固定 `Local` 在前，其余按 `LyricSourceKind` 枚举顺序。
/// 未注册的来源也会列出（标记 `available=false`），便于用户在 UI 中看到全部可选项。
#[tauri::command]
pub fn lyric_source_list(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let registered: std::collections::HashSet<String> =
        ctx.registrar.list_names().into_iter().collect();

    // Local 永远视为可用（库内 Lyric 实体，不依赖 registrar）
    let all_kinds = [
        LyricSourceKind::Local,
        LyricSourceKind::Netease,
        LyricSourceKind::Kugou,
        LyricSourceKind::Qq,
    ];

    let list: Vec<serde_json::Value> = all_kinds
        .iter()
        .map(|k| {
            let name = k.source_name();
            let available = *k == LyricSourceKind::Local || registered.contains(name);
            serde_json::json!({
                "kind": k,
                "source_name": name,
                "display_name": k.display_name(),
                "available": available,
            })
        })
        .collect();

    Ok(serde_json::json!({ "sources": list }))
}

/// 获取指定歌曲的最佳歌词（综合所有可用来源，按用户偏好选择）。
///
/// # 工作流
/// 1. 加载用户偏好 `LyricPreference`（自动规范化 source_order）
/// 2. 收集候选歌词：
///    - 本地：库内通过 `song_id` 索引的 Lyric 实体
///    - 在线：遍历 `song.source_ids`，跳过 Local，对每个来源通过 registrar 查找实现并调用 `get_lyric`
/// 3. 用 `select_best` 选出得分最高的候选
/// 4. 返回歌词文本 + 格式 + 来源
///
/// # 容错
/// 单个来源失败不影响其他来源；只要有一个候选成功即返回。
#[tauri::command]
pub fn lyric_get_preferred(
    ctx: State<'_, Arc<AppContext>>,
    song_id: String,
) -> Result<serde_json::Value, String> {
    let pref = LyricPreference::load(&ctx.config);

    let mut candidates: Vec<LyricCandidate> = Vec::new();

    // 候选 1：本地歌词（库内 Lyric 实体，按 song_id 索引）
    if let Some(local_lyric) = ctx.library.get_lyric_of_song(&song_id) {
        if !local_lyric.text.is_empty() {
            candidates.push(LyricCandidate::new(
                LyricSourceKind::Local,
                local_lyric.text,
            ));
        }
    }

    // 候选 2..N：遍历 song.source_ids 中所有在线来源
    // 每个 SourceId 携带 source_name + entity_id，通过 registrar 动态查找来源实现
    if let Some(song) = ctx.library.get_song(&song_id) {
        for sid in &song.source_ids {
            // Local 来源已通过库内 Lyric 实体处理，跳过
            if sid.source_name == LyricSourceKind::Local.source_name() {
                continue;
            }
            // 仅处理已知 LyricSourceKind
            let kind = match LyricSourceKind::from_source_name(&sid.source_name) {
                Some(k) => k,
                None => continue,
            };
            // 仅处理 Song 实体类型（避免拿到 Album/Artist 等错误类型）
            if sid.entity_type != chordial_core::module::music_source::types::EntityType::Song {
                continue;
            }
            if sid.entity_id.is_empty() {
                continue;
            }
            // 通过 registrar 查找已注册的来源实现
            let source = match ctx.registrar.get(&sid.source_name) {
                Some(s) => s,
                None => continue, // 来源未注册（未启用），静默跳过
            };
            // 调用 get_lyric；失败时记录但不影响其他来源
            match source.get_lyric(&sid.entity_id) {
                Ok(Some(lyric)) if !lyric.text.is_empty() => {
                    candidates.push(LyricCandidate::new(kind, lyric.text));
                }
                Ok(_) => {} // None：来源中无歌词
                Err(e) => {
                    eprintln!(
                        "[lyric_get_preferred] 来源 {} 获取歌词失败 song_id={} entity_id={}: {}",
                        sid.source_name, song_id, sid.entity_id, e
                    );
                }
            }
        }
    }

    // 选最佳
    if let Some(best) = lyric_preference::select_best(&candidates, &pref) {
        Ok(serde_json::json!({
            "text": best.text,
            "format": best.format,
            "source": best.source,
            "is_word_by_word": best.format.is_word_by_word(),
            "candidates_count": candidates.len(),
        }))
    } else {
        Ok(serde_json::json!({
            "text": null,
            "format": null,
            "source": null,
            "candidates_count": 0,
        }))
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// 歌单命令 — 本地歌单 CRUD + 曲目管理
// ══════════════════════════════════════════════════════════════════════════════

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
