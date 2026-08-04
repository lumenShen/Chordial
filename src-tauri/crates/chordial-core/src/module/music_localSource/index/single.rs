//! 单文件索引维护 — 单点增删改查。
//!
//! 负责 `index_file` / `unindex_file` / `reindex_file` 三个单文件索引方法，
//! 供 watcher 增量同步与命令层调用。每次调用仅处理一个文件，
//! 与批量路径（`batch`）共用 `build_song` / `save_cover_to_disk` 等底层原语。

use super::super::scanner;
use super::super::source::{LocalMusicSource, LOCAL_SOURCE_NAME};
use crate::module::music_library::models::Lyric;
use crate::module::perf;
use crate::module::platform::{self, PlatformPath};
use std::collections::HashSet;
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
}