//! 手动去重 — 对每首"逻辑歌曲"保留本地文件中音质最高的版本，删除其他本地文件。

use super::super::library::MusicLibrary;
use super::super::songs;
use crate::module::music_source::types::{AudioQuality, SourceId, SourceType};
use crate::module::perf;
use std::collections::{HashMap, HashSet};

impl MusicLibrary {
    /// 手动去重：对每首"逻辑歌曲"（按 title + sorted artist_names 匹配），
    /// 保留本地文件中音质最高的版本，删除其他本地文件 + 移除其 SourceId。
    ///
    /// 规则：
    /// 1. 按 (title_lower, sorted_artist_names_lower) 分组所有歌曲。
    /// 2. 每组内遍历所有 `source_ids` 中的 `Local` 来源，按文件扩展名推断音质。
    /// 3. 选出 rank 最高的本地 SourceId 作为保留项；其他本地文件被加入删除列表。
    /// 4. 删除物理文件 + 移除库内 SourceId。
    /// 5. 仅 Local 来源参与去重，Web 来源（如 netease）保留不动。
    ///
    /// 音质推断：使用 `AudioQuality::from_extension`。
    /// 同 rank 时按 entity_id 字典序保留首个，结果稳定可预测。
    ///
    /// # 返回
    /// `(removed_files: Vec<String>, kept_files: usize, groups_processed: usize)`
    /// - `removed_files`: 被删除的本地文件路径列表（前端可展示）
    /// - `kept_files`: 实际保留的本地文件数（每组至多 1 个）
    /// - `groups_processed`: 处理的逻辑歌曲组数（含无需去重的组）
    pub fn dedup_keep_highest_quality(
        &self,
    ) -> Result<(Vec<String>, usize, usize), String> {
        let _scope = perf::scope("library.dedup_keep_highest_quality");

        let all_songs = songs::get_all(&self.store);

        // 1. 按 (title_lower, sorted_artist_names_lower) 分组
        // value: Vec<(song_id, Vec<(SourceId, AudioQuality)>)> — 仅 Local 来源
        let mut groups: HashMap<(String, Vec<String>), Vec<(String, Vec<(SourceId, AudioQuality)>)>> =
            HashMap::new();
        for (id, song) in all_songs.iter() {
            let title_lower = song.title.to_lowercase();
            let mut names_sorted: Vec<String> =
                song.artist_names.iter().map(|n| n.to_lowercase()).collect();
            names_sorted.sort();
            let key = (title_lower, names_sorted);

            let locals: Vec<(SourceId, AudioQuality)> = song
                .source_ids
                .iter()
                .filter(|sid| matches!(sid.source_type, SourceType::Local))
                .filter_map(|sid| {
                    // entity_id 即文件路径
                    let quality = std::path::Path::new(&sid.entity_id)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(AudioQuality::from_extension)
                        .unwrap_or(AudioQuality::Unknown);
                    Some((sid.clone(), quality))
                })
                .collect();
            // 仅当有 ≥2 个 Local 来源时才可能需要去重
            if locals.len() >= 2 {
                groups.entry(key).or_default().push((id.clone(), locals));
            }
        }

        let groups_processed = groups.len();
        if groups_processed == 0 {
            return Ok((Vec::new(), 0, 0));
        }

        // 2. 每组选出保留项（rank 最高），其余加入待删除 entity_ids
        // 同时记录待删除的物理文件路径
        let mut files_to_delete: Vec<String> = Vec::new();
        let mut kept_count = 0usize;
        // 按 source_name 分桶（实际全是 "local"），便于调用 remove_specific_song_source_ids
        let mut local_entity_ids_to_remove: HashSet<String> = HashSet::new();

        for (_, group) in groups.iter() {
            // group 内每首歌都有自己的 local 列表，把所有 local 收集起来按音质排序
            // 取最高 rank 作为保留；其余加入删除列表
            // 若不同歌曲指向同一物理文件，去重处理
            let mut all_locals: Vec<&(SourceId, AudioQuality)> = Vec::new();
            for (_, locals) in group.iter() {
                for l in locals.iter() {
                    all_locals.push(l);
                }
            }
            if all_locals.is_empty() {
                continue;
            }
            // 排序：rank 降序 → entity_id 升序（稳定）
            all_locals.sort_by(|a, b| {
                b.1.cmp(&a.1).then_with(|| a.0.entity_id.cmp(&b.0.entity_id))
            });
            let keep_entity_id = &all_locals[0].0.entity_id;
            kept_count += 1;
            for (sid, _) in all_locals.iter().skip(1) {
                if sid.entity_id == *keep_entity_id {
                    continue; // 同物理文件不删
                }
                if !local_entity_ids_to_remove.contains(&sid.entity_id) {
                    local_entity_ids_to_remove.insert(sid.entity_id.clone());
                    files_to_delete.push(sid.entity_id.clone());
                }
            }
        }

        if files_to_delete.is_empty() {
            return Ok((Vec::new(), kept_count, groups_processed));
        }

        // 3. 删除物理文件（失败不致命，文件可能已被外部删除）
        let mut deleted_files: Vec<String> = Vec::with_capacity(files_to_delete.len());
        for path_str in files_to_delete.iter() {
            let path = std::path::Path::new(path_str);
            match std::fs::remove_file(path) {
                Ok(()) => deleted_files.push(path_str.clone()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // 文件已不存在，仍需清理库内 SourceId
                    deleted_files.push(path_str.clone());
                }
                Err(e) => {
                    eprintln!(
                        "[library.dedup] 删除文件失败 '{}': {}（继续清理库引用）",
                        path_str, e
                    );
                    // 删文件失败仍清理库引用，避免库内残留指向不存在文件的 SourceId
                    deleted_files.push(path_str.clone());
                }
            }
        }

        // 4. 移除库内 SourceId（用 Local source_name 精准移除）
        // 注意：source_name 在 Local 来源中固定为 "local"
        self.remove_specific_song_source_ids("local", &local_entity_ids_to_remove)?;

        Ok((deleted_files, kept_count, groups_processed))
    }
}