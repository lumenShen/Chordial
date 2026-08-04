//! 索引重建与清理 — 启动时从库恢复索引 + 索引整体清空。
//!
//! 负责 `restore_index_from_library`（从持久化 MusicLibrary 并行 canonicalize 重建
//! 内存索引并加载 mtime 缓存）与 `clear_all_index`（清空本地来源全部索引）。

use super::super::source::{LocalMusicSource, LOCAL_SOURCE_NAME};
use crate::module::music_source::types::EntityType;
use crate::module::perf;
use crate::module::platform::{self, PlatformPath};
use std::collections::{HashMap, HashSet};

impl LocalMusicSource {
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
}