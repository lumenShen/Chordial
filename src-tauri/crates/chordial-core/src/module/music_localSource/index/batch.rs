//! 批量索引处理 — 并行探测 + 单次批量合并写回。
//!
//! 负责 `batch_index_files` / `batch_unindex_files`。相比循环调用单文件方法，
//! 避免了每首歌曲都反序列化整个库 + 重建索引 + 全量写回的 O(N²) 开销，
//! 改为一次加载 + 并行探测 + 单次批量写回（O(N+K) 总复杂度）。

use super::super::scanner;
use super::super::source::{LocalMusicSource, LOCAL_SOURCE_NAME};
use crate::module::music_library::models::{Lyric, Song};
use crate::module::perf;
use crate::module::platform::{self, PlatformPath};
use std::collections::HashSet;
use uuid::Uuid;

impl LocalMusicSource {
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
}