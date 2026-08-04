//! MusicLibrary 命令 — 音乐库 CRUD / 搜索 / 关系查询 / 文件管理。

use super::common::LIBRARY_CHANGED_EVENT;
use chordial_core::module::music_source::types::EntityType;
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

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