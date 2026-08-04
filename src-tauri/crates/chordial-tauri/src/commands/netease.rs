//! Netease Cloud Music API 来源命令 — 配置/搜索/歌单/账户/扫码登录/下载。

use super::common::{DOWNLOAD_PROGRESS_EVENT, LIBRARY_CHANGED_EVENT};
use super::downloads::{
    cancel_download_task, register_download_task, unregister_download_task, BatchDownloadItem,
    DedupStrategy, DownloadProgress,
};
use super::local_matches::{best_local_quality, find_local_matches};
use chordial_core::module::music_library::models::{Album, Artist, Song};
use chordial_core::module::music_netease_source::{self, AudioQuality, NeteasePlaylistBrief};
use chordial_core::module::music_source::cache;
use chordial_core::module::music_source::traits::MusicSource;
use chordial_core::module::music_source::types::AudioQualityDescriptor;
use chordial_core::module::music_web_source::WebSourceConfig;
use chordial_core::module::playlist::{self, Playlist};
use chordial_core::AppContext;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

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

// 扫码登录命令（与 kugou 的 qr 流程保持一致的命名约定）

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