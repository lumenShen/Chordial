//! 网易云歌曲下载命令（单曲 / 批量后台 / 取消）。
//!
//! 批量下载复用 `commands::downloads` 的任务注册与 `commands::local_matches`
//! 的本地匹配去重，从 `State` 中克隆 `Arc<AppContext>` 到后台线程执行。

use crate::commands::common::DOWNLOAD_PROGRESS_EVENT;
use crate::commands::downloads::{
    cancel_download_task, register_download_task, unregister_download_task, BatchDownloadItem,
    DedupStrategy, DownloadProgress,
};
use crate::commands::local_matches::{best_local_quality, find_local_matches};
use chordial_core::module::music_netease_source::{self, AudioQuality};
use chordial_core::module::music_source::cache;
use chordial_core::AppContext;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

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