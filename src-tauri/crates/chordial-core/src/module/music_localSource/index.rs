//! 文件索引维护 — [`LocalMusicSource`] 的纯索引方法。
//!
//! 负责文件索引的增删改查（`index_file` / `unindex_file` / `reindex_file`）、
//! 批量处理（`batch_index_files` / `batch_unindex_files`）、索引重建
//! （`restore_index_from_library`）以及 mtime 缓存（`check_file_unchanged` /
//! `update_file_mtime` / `save_mtime_cache`）。

use super::scanner;
use super::source::{LocalMusicSource, LOCAL_SOURCE_NAME};
use crate::module::music_library::models::{Lyric, Song};
use crate::module::music_source::types::EntityType;
use crate::module::perf;
use crate::module::platform::{self, PlatformPath};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl LocalMusicSource {
    /// 扫描单个音频文件并添加到音乐库（或合并到已有条目）。
    ///
    /// 同时尝试读取同目录的 `.lrc` / `.txt` 歌词文件，若命中则构造 `Lyric` 实体
    /// 写入 PersistentStore，并让 `Song.lyric_id` 指向该 `Lyric.id`。
    /// 若无歌词文件，`lyric_id` 置为 `None`，避免库中残留孤儿引用。
    ///
    /// 返回 `true` 表示成功处理（新增或合并），`false` 表示跳过（非音频文件）。
    pub fn index_file(&self, path: &PlatformPath) -> Result<bool, String> {
        let _scope = perf::scope("source.index_file");
        let canonical = platform::canonicalize(path)
            .unwrap_or_else(|_| path.clone());

        // 跳过非音频文件
        if !scanner::is_supported_audio(&canonical) {
            return Ok(false);
        }

        // 若已索引过，跳过
        if self.file_index.read().contains_key(&canonical) {
            return Ok(true);
        }

        // 探测元数据 + 提取嵌入封面（一次 symphonia probe）
        let probed = scanner::probe_file_with_cover(&canonical)?;
        let meta = &probed.audio;

        // 预热封面磁盘缓存：扫描阶段写入，浏览阶段直接命中（~1-5ms vs 117-296ms）
        if let Some(cover) = &probed.cover {
            let entity_id = platform::path_to_string(&canonical);
            self.save_cover_to_disk(&entity_id, cover);
        }

        // 读取同目录歌词文件（.lrc 优先，.txt 兜底）；
        // 找不到时回退到音频文件嵌入的歌词（ID3 USLT/SYLT、Vorbis LYRICS、MP4 ©lyr）
        let mut lyric_text = scanner::read_lyric_file(&canonical);
        if lyric_text.is_none() {
            lyric_text = meta.lyrics.clone();
        }

        // 构建 Song（lyric_id 占位 UUID 仅在确实有歌词时保留）
        let mut song = self.build_song(&canonical, meta);
        if lyric_text.is_none() {
            song.lyric_id = None;
        }

        // 添加到音乐库（自动去重合并），获取实际存储的 ID
        let stored_id = self.library.add_song(&song)?;

        // 写入 Lyric 实体到 PersistentStore
        // —— 这一步是关键：前端 library_get_lyric_of_song(songId) 直接查 lyrics 表
        if let Some(text) = lyric_text {
            let lyric = Lyric {
                id: song.lyric_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string()),
                song_id: stored_id.clone(),
                text,
                source_id: song.source_ids[0].clone(),
            };
            // add_lyric 在 id 冲突时返回 Err，reindex 时旧 Lyric 已被
            // remove_specific_song_source_ids 清理，此处不会冲突
            if let Err(e) = self.library.add_lyric(&lyric) {
                eprintln!(
                    "[local_source] 写入歌词失败 '{}': {}",
                    platform::path_to_string(&canonical),
                    e
                );
            }
        }

        // 更新索引（使用库中实际存储的 ID）
        self.file_index
            .write()
            .insert(canonical.clone(), stored_id.clone());
        self.id_to_path
            .write()
            .insert(stored_id, canonical);

        Ok(true)
    }

    /// 从索引中移除文件引用，并从音乐库中移除对应的 SourceId。
    ///
    /// 若歌曲失去全部来源引用，则歌曲被自动删除；
    /// 级联空实体由 [`MusicLibrary::cleanup_empty_entities`] 处理。
    pub fn unindex_file(&self, path: &PlatformPath) -> Result<bool, String> {
        let canonical = platform::canonicalize(path)
            .unwrap_or_else(|_| path.clone());

        let song_id = {
            let index = self.file_index.read();
            index.get(&canonical).cloned()
        };

        let Some(song_id) = song_id else {
            return Ok(false); // 不在索引中
        };

        // 从音乐库中精准移除该文件的 SourceId
        let entity_id = platform::path_to_string(&canonical);
        let mut entity_ids = HashSet::new();
        entity_ids.insert(entity_id);
        self.library
            .remove_specific_song_source_ids(LOCAL_SOURCE_NAME, &entity_ids)?;

        // 更新本地索引
        self.file_index.write().remove(&canonical);
        self.id_to_path.write().remove(&song_id);

        Ok(true)
    }

    /// 重新索引文件（适用于文件修改事件）。
    pub fn reindex_file(&self, path: &PlatformPath) -> Result<bool, String> {
        let _scope = perf::scope("source.reindex_file");
        // 先卸载旧索引
        self.unindex_file(path)?;
        // 再重新索引
        self.index_file(path)
    }

    /// 批量索引音频文件 — 一次性加载库 + 并行探测 + 单次批量合并写回。
    ///
    /// 相比循环调用 [`index_file`](Self::index_file)，避免了每首歌曲都
    /// 反序列化整个库（songs/artists/albums）+ 重建索引 + 全量写回
    /// 带来的 O(N²) 开销。典型场景：1000 个文件
    /// - 旧路径（逐个 `index_file`）：~96ms × 1000 ≈ 96s
    /// - 新路径（本方法）：~150ms 总计（单次加载 + 并行探测 + 单次写回）
    ///
    /// # 流程
    /// 1. 规范化路径并过滤：跳过非音频文件、已在 file_index 中的文件
    /// 2. 并行 `probe_file` + `read_lyric_file`（CPU/IO 并行）
    /// 3. 构建 Song 列表
    /// 4. 一次性调用 [`MusicLibrary::add_songs_batch`] 合并入库
    /// 5. 串行写入 Lyric 实体、更新 file_index / id_to_path / mtime
    ///
    /// # 返回
    /// `(indexed, errors)` — 成功索引条目数 + 失败文件描述列表
    pub fn batch_index_files(&self, paths: &[PlatformPath]) -> Result<(usize, Vec<String>), String> {
        let _scope = perf::scope("source.batch_index_files");

        // 1. 规范化 + 过滤：跳过非音频文件、已索引文件
        // 预估待探测数量以减少扩容；上限为 paths.len()
        let mut needs_probe: Vec<PlatformPath> = Vec::with_capacity(paths.len());
        {
            let file_index = self.file_index.read();
            for path in paths {
                let canonical = platform::canonicalize(path)
                    .unwrap_or_else(|_| path.clone());
                if !scanner::is_supported_audio(&canonical) {
                    continue;
                }
                if file_index.contains_key(&canonical) {
                    continue;
                }
                needs_probe.push(canonical);
            }
        }

        if needs_probe.is_empty() {
            return Ok((0, Vec::new()));
        }

        // 2. 并行 probe + read_lyric_file
        // 线程数：取 CPU 核心数与文件数的较小值；至少 1
        let probe_count = needs_probe.len();
        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(probe_count)
            .max(1);
        let chunk_size = (probe_count + num_threads - 1) / num_threads;

        // 每个探测结果携带 (path, Result<(probed, lyric_text), error_msg>)
        let mut probe_results: Vec<(
            PlatformPath,
            Result<(scanner::ProbedMeta, Option<String>), String>,
        )> = Vec::with_capacity(probe_count);

        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(num_threads);
            for chunk in needs_probe.chunks(chunk_size) {
                // chunk.to_vec() 避免跨线程借用 needs_probe
                let chunk: Vec<PlatformPath> = chunk.to_vec();
                handles.push(s.spawn(move || {
                    let mut chunk_results = Vec::with_capacity(chunk.len());
                    for path in &chunk {
                        // probe + cover + read_lyric 在同一线程内串行，
                        // 避免再次起线程的开销；线程间仍是并行的
                        let result = scanner::probe_file_with_cover(path).and_then(|probed| {
                            // 外部 .lrc/.txt 优先；找不到时回退到嵌入歌词
                            let mut lyric = scanner::read_lyric_file(path);
                            if lyric.is_none() {
                                lyric = probed.audio.lyrics.clone();
                            }
                            Ok((probed, lyric))
                        });
                        chunk_results.push((path.clone(), result));
                    }
                    chunk_results
                }));
            }
            for handle in handles {
                match handle.join() {
                    Ok(chunk_results) => probe_results.extend(chunk_results),
                    Err(_) => eprintln!("[local_source] batch_index_files 探测线程异常退出"),
                }
            }
        });

        // 3. 构建 Song 列表，收集错误；同时预热封面磁盘缓存
        // 容量上限 = 探测成功的条目数
        let success_count = probe_results
            .iter()
            .filter(|r| r.1.is_ok())
            .count();
        let mut songs_and_lyrics: Vec<(PlatformPath, Song, Option<String>)> =
            Vec::with_capacity(success_count);
        let mut errors: Vec<String> = Vec::new();

        for (path, result) in probe_results {
            match result {
                Ok((probed, lyric_text)) => {
                    // 预热封面磁盘缓存：扫描阶段写入，浏览阶段直接命中
                    if let Some(cover) = &probed.cover {
                        let entity_id = platform::path_to_string(&path);
                        self.save_cover_to_disk(&entity_id, cover);
                    }
                    let mut song = self.build_song(&path, &probed.audio);
                    if lyric_text.is_none() {
                        song.lyric_id = None;
                    }
                    songs_and_lyrics.push((path, song, lyric_text));
                }
                Err(e) => {
                    errors.push(format!("{}: {}", platform::path_to_string(&path), e));
                }
            }
        }

        if songs_and_lyrics.is_empty() {
            return Ok((0, errors));
        }

        // 4. 批量合并入库（单次加载 + 单次写回，O(N+K) 总复杂度）
        let songs: Vec<Song> = songs_and_lyrics
            .iter()
            .map(|(_, s, _)| s.clone())
            .collect();
        let stored_ids = self.library.add_songs_batch(&songs)?;

        // 5. 串行写入 Lyric + 更新索引/mtime
        // 这部分都是 O(1) 操作或单次 fs 调用，不在热路径
        for (i, (path, song, lyric_text)) in songs_and_lyrics.iter().enumerate() {
            let stored_id = &stored_ids[i];

            // 写入 Lyric 实体（若有歌词）
            if let Some(text) = lyric_text {
                let lyric = Lyric {
                    id: song.lyric_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string()),
                    song_id: stored_id.clone(),
                    text: text.clone(),
                    source_id: song.source_ids[0].clone(),
                };
                if let Err(e) = self.library.add_lyric(&lyric) {
                    eprintln!(
                        "[local_source] 批量写入歌词失败 '{}': {}",
                        platform::path_to_string(path),
                        e
                    );
                }
            }

            // 更新索引（使用库中实际存储的 ID）
            self.file_index
                .write()
                .insert(path.clone(), stored_id.clone());
            self.id_to_path
                .write()
                .insert(stored_id.clone(), path.clone());
            self.update_file_mtime(path, stored_id);
        }

        Ok((songs_and_lyrics.len(), errors))
    }

    /// 批量取消索引音频文件 — 单次库调用替代 N 次 `unindex_file`。
    ///
    /// 旧路径循环 `unindex_file` 每次都会触发 `remove_specific_song_source_ids`
    /// → `cleanup_empty_entities` → `save`，N 个文件 = N 次全量序列化写盘。
    /// 本方法仅调用一次库清理，再批量更新本地索引。
    ///
    /// # 返回
    /// 成功从索引中移除的条目数
    pub fn batch_unindex_files(&self, paths: &[PlatformPath]) -> Result<usize, String> {
        let _scope = perf::scope("source.batch_unindex_files");

        // 1. 规范化路径 + 收集对应的 entity_id 和 song_id
        let mut entity_ids: HashSet<String> = HashSet::with_capacity(paths.len());
        let mut to_remove_from_index: Vec<(PlatformPath, String)> = Vec::with_capacity(paths.len());

        for path in paths {
            let canonical = platform::canonicalize(path)
                .unwrap_or_else(|_| path.clone());
            let entity_id = platform::path_to_string(&canonical);
            entity_ids.insert(entity_id);
            // 同时记录 song_id 用于清理 id_to_path
            if let Some(song_id) = self.file_index.read().get(&canonical).cloned() {
                to_remove_from_index.push((canonical, song_id));
            }
        }

        if entity_ids.is_empty() {
            return Ok(0);
        }

        // 2. 一次性库清理（内部已用 get_entries_filtered 避免全量反序列化）
        self.library.remove_specific_song_source_ids(
            LOCAL_SOURCE_NAME,
            &entity_ids,
        )?;

        // 3. 批量更新本地索引
        let removed = to_remove_from_index.len();
        {
            let mut file_index = self.file_index.write();
            let mut id_to_path = self.id_to_path.write();
            for (canonical, song_id) in &to_remove_from_index {
                file_index.remove(canonical);
                id_to_path.remove(song_id);
            }
        }

        Ok(removed)
    }

    /// 清空本地来源的所有索引（file_index / id_to_path / mtime / 封面缓存）。
    ///
    /// 不影响音乐库本身（Song/Artist/Album/Lyric）— 调用方需自行 `library.clear_all()`。
    /// 持久化的 mtime_store 也一并清空并落盘。
    pub fn clear_all_index(&self) -> Result<(), String> {
        let _scope = perf::scope("source.clear_all_index");
        self.file_index.write().clear();
        self.id_to_path.write().clear();
        self.file_mtimes.write().clear();
        self.cover_cache.lock().clear();
        // 清空封面磁盘缓存
        self.clear_cover_disk_cache();
        // mtime_store 持久化清空
        self.mtime_store.clear();
        self.mtime_store.save()
    }

    /// 从持久化的 MusicLibrary 重建内存索引并加载 mtime 缓存（启动时调用）。
    ///
    /// 遍历库中所有带 `"local"` SourceId 的歌曲，以 `entity_id`（即文件路径）
    /// 重建 `file_index` 和 `id_to_path`。同时从持久存储加载文件 mtime 缓存，
    /// 用于启动时跳过未变化文件的重扫描。文件已不存在的条目自动从库中和 mtime 缓存中移除。
    ///
    /// # 返回
    /// `(restored, removed)` — 成功恢复的条目数和因文件丢失而移除的条目数。
    pub fn restore_index_from_library(&self) -> (usize, usize) {
        use std::time::Instant;

        let _scope = perf::scope("source.restore_index");

        let _t0 = Instant::now();
        let all_songs = self.library.get_all_songs();
        let t0 = _t0.elapsed();
        eprintln!(
            "[local_source] ⏱ 4a. 从库加载 {} 首歌曲: {:?}",
            all_songs.len(),
            t0
        );

        // 从独立 mtime 存储加载
        let _t1 = Instant::now();
        let mut mtimes: HashMap<String, (u64, u64, String)> = self
            .mtime_store
            .get("file_mtimes")
            .unwrap_or_default();
        let t1 = _t1.elapsed();
        eprintln!(
            "[local_source] ⏱ 4b. 加载 mtime 缓存 ({} 条): {:?}",
            mtimes.len(),
            t1
        );

        // 收集需要 canonicalize 的路径（预分配上界避免多次扩容）
        let mut to_canonicalize: Vec<(String, String)> = Vec::with_capacity(all_songs.len());
        for (song_id, song) in &all_songs {
            let local_sid = song.source_ids.iter().find(|sid| {
                sid.source_name == LOCAL_SOURCE_NAME && sid.entity_type == EntityType::Song
            });
            if let Some(sid) = local_sid {
                to_canonicalize.push((song_id.clone(), sid.entity_id.clone()));
            }
        }

        let _t2 = Instant::now();
        let mut restored = 0usize;
        let mut removed_paths: HashSet<String> = HashSet::new();
        let canonicalize_count = to_canonicalize.len();

        let mut results: Vec<(String, String, bool)> = Vec::with_capacity(canonicalize_count);

        // 并行 canonicalize（空库时跳过，避免 chunks(0) panic）
        let num_threads = if canonicalize_count > 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(canonicalize_count)
        } else {
            0
        };
        if num_threads > 0 {
            let chunk_size = (canonicalize_count + num_threads - 1) / num_threads;

            std::thread::scope(|s| {
                let mut handles = Vec::with_capacity(num_threads);
                for chunk in to_canonicalize.chunks(chunk_size) {
                    let chunk: Vec<(String, String)> = chunk.to_vec();
                    handles.push(s.spawn(move || {
                        let mut chunk_results = Vec::with_capacity(chunk.len());
                        for (song_id, path_str) in &chunk {
                            let path = PlatformPath::from(path_str.as_str());
                            let ok = platform::canonicalize(&path).is_ok();
                            chunk_results.push((song_id.clone(), path_str.clone(), ok));
                        }
                        chunk_results
                    }));
                }
                for handle in handles {
                    if let Ok(chunk_results) = handle.join() {
                        results.extend(chunk_results);
                    }
                }
            });
        }

        // 串行写入索引
        for (song_id, path_str, ok) in &results {
            if *ok {
                let canonical = PlatformPath::from(path_str.as_str());
                self.file_index
                    .write()
                    .insert(canonical.clone(), song_id.clone());
                self.id_to_path
                    .write()
                    .insert(song_id.clone(), canonical);
                restored += 1;
            } else {
                removed_paths.insert(path_str.clone());
            }
        }

        let t2 = _t2.elapsed();
        eprintln!(
            "[local_source] ⏱ 4c. 并行 canonicalize {} 首 ({} 线程, {} 个丢失): {:?}",
            canonicalize_count,
            num_threads,
            removed_paths.len(),
            t2
        );

        let _t3 = Instant::now();
        // 清理不存在的文件对应的 mtime 条目
        mtimes.retain(|path_str, _| {
            let p = PlatformPath::from(path_str.as_str());
            platform::exists(&p)
        });
        *self.file_mtimes.write() = mtimes;
        let t3 = _t3.elapsed();
        eprintln!(
            "[local_source] ⏱ 4d. 清理 mtime 缓存: {:?}",
            t3
        );

        if !removed_paths.is_empty() {
            let _t4 = Instant::now();
            if let Err(e) = self
                .library
                .remove_specific_song_source_ids(LOCAL_SOURCE_NAME, &removed_paths)
            {
                eprintln!(
                    "[local_source] 清理已删除文件的 SourceId 失败: {}",
                    e
                );
            }
            let t4 = _t4.elapsed();
            let removed = removed_paths.len();
            eprintln!(
                "[local_source] ⏱ 4e. 批量移除 {} 个丢失文件: {:?}",
                removed,
                t4
            );
            eprintln!(
                "[local_source] 索引恢复: {} 首歌曲, {} 个已删除文件已清理",
                restored, removed
            );
            return (restored, removed);
        }

        eprintln!("[local_source] 索引恢复: {} 首歌曲已从库中恢复", restored);
        (restored, 0)
    }

    /// 检查文件 mtime+size 是否与缓存一致，返回缓存的 song_id（若未变化）。
    ///
    /// `path` 应为已规范化的路径。
    pub fn check_file_unchanged(&self, path: &PlatformPath) -> Option<String> {
        let path_str = platform::path_to_string(path);
        let mtimes = self.file_mtimes.read();
        let (cached_mtime, cached_size, song_id) = mtimes.get(&path_str)?;

        let current_mtime = platform::file_modified_secs(
            &PlatformPath::from(path_str.as_str()),
        )
        .ok()?;
        let current_size = platform::file_size(
            &PlatformPath::from(path_str.as_str()),
        )
        .ok()?;

        if current_mtime == *cached_mtime && current_size == *cached_size {
            Some(song_id.clone())
        } else {
            None
        }
    }

    /// 更新文件的 mtime 缓存条目。
    ///
    /// `path` 应为已规范化的路径。
    pub fn update_file_mtime(&self, path: &PlatformPath, song_id: &str) {
        let path_str = platform::path_to_string(path);
        let platform_path = PlatformPath::from(path_str.as_str());
        if let (Ok(mtime), Ok(size)) = (
            platform::file_modified_secs(&platform_path),
            platform::file_size(&platform_path),
        ) {
            self.file_mtimes.write().insert(
                path_str,
                (mtime, size, song_id.to_string()),
            );
        }
    }

    /// 将 mtime 缓存持久化到独立存储（与 library 分离，避免每次保存都序列化全部歌曲）。
    pub fn save_mtime_cache(&self) -> Result<(), String> {
        let mtimes = self.file_mtimes.read().clone();
        self.mtime_store.set("file_mtimes", &mtimes)?;
        self.mtime_store.save()
    }
}