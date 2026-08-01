//! 维护方法 — 去重、引用修复、空实体清理等库维护操作。

use super::{albums, artists, lyrics, songs};
use super::merge::{build_album_index, build_artist_name_index, merge_source_ids};
use super::models::*;
use crate::module::music_source::types::{EntityType, SourceId};
use crate::module::perf;
use std::collections::HashMap;

use super::library::MusicLibrary;

impl MusicLibrary {
    /// 从所有实体中移除指定来源的 `SourceId`。
    ///
    /// 对每类实体（Song / Artist / Album / Lyric）：
    /// - 过滤掉 `source_name` 匹配的 `SourceId`
    /// - 若某实体的 `source_ids` 全部被移除，则删除该实体
    ///
    /// 此方法由 [`SourceCleanup`] 回调调用，参见
    /// [`SourceRegistrar::unregister`](crate::module::music_source::registrar::SourceRegistrar::unregister)。
    pub fn remove_source_from_all_entities(&self, source_name: &str) -> Result<(), String> {
        let _scope = perf::scope("library.remove_source_from_all_entities");

        // 优化：仅反序列化 source_ids 数组中含目标 source_name 的条目，
        // 用 set_subkey / remove_entry 仅写回受影响条目，避免全量重写。
        // 典型场景：注销一个 source 时影响 ~5-50 条，而非全部 3853 条。

        // ── Songs ──
        {
            let mut affected: Vec<Song> = self
                .store
                .get_entries_filtered::<Song, _>(songs::KEY, |v| {
                    v.get("source_ids")
                        .and_then(|s| s.as_array())
                        .map_or(false, |arr| {
                            arr.iter().any(|sid| {
                                sid.get("source_name").and_then(|n| n.as_str()) == Some(source_name)
                            })
                        })
                });
            let mut to_remove: Vec<String> = Vec::with_capacity(affected.len());
            let mut any_changed = false;
            for song in affected.iter_mut() {
                let before = song.source_ids.len();
                song.source_ids.retain(|sid| sid.source_name != source_name);
                if song.source_ids.len() < before {
                    any_changed = true;
                    if song.source_ids.is_empty() {
                        to_remove.push(song.id.clone());
                    }
                }
            }
            if any_changed {
                for song in &affected {
                    self.store.set_subkey(songs::KEY, &song.id, song)?;
                }
                for id in &to_remove {
                    self.store.remove_entry(songs::KEY, id);
                }
            }
        }

        // ── Artists ──
        {
            let mut affected: Vec<Artist> = self
                .store
                .get_entries_filtered::<Artist, _>(artists::KEY, |v| {
                    v.get("source_ids")
                        .and_then(|s| s.as_array())
                        .map_or(false, |arr| {
                            arr.iter().any(|sid| {
                                sid.get("source_name").and_then(|n| n.as_str()) == Some(source_name)
                            })
                        })
                });
            let mut to_remove: Vec<String> = Vec::with_capacity(affected.len());
            let mut any_changed = false;
            for artist in affected.iter_mut() {
                let before = artist.source_ids.len();
                artist.source_ids.retain(|sid| sid.source_name != source_name);
                if artist.source_ids.len() < before {
                    any_changed = true;
                    if artist.source_ids.is_empty() {
                        to_remove.push(artist.id.clone());
                    }
                }
            }
            if any_changed {
                for artist in &affected {
                    self.store.set_subkey(artists::KEY, &artist.id, artist)?;
                }
                for id in &to_remove {
                    self.store.remove_entry(artists::KEY, id);
                }
            }
        }

        // ── Albums ──
        {
            let mut affected: Vec<Album> = self
                .store
                .get_entries_filtered::<Album, _>(albums::KEY, |v| {
                    v.get("source_ids")
                        .and_then(|s| s.as_array())
                        .map_or(false, |arr| {
                            arr.iter().any(|sid| {
                                sid.get("source_name").and_then(|n| n.as_str()) == Some(source_name)
                            })
                        })
                });
            let mut to_remove: Vec<String> = Vec::with_capacity(affected.len());
            let mut any_changed = false;
            for album in affected.iter_mut() {
                let before = album.source_ids.len();
                album.source_ids.retain(|sid| sid.source_name != source_name);
                if album.source_ids.len() < before {
                    any_changed = true;
                    if album.source_ids.is_empty() {
                        to_remove.push(album.id.clone());
                    }
                }
            }
            if any_changed {
                for album in &affected {
                    self.store.set_subkey(albums::KEY, &album.id, album)?;
                }
                for id in &to_remove {
                    self.store.remove_entry(albums::KEY, id);
                }
            }
        }

        // ── Lyrics ──
        // Lyrics 的 source_id 是单对象（非数组），用 source_name 直接等值匹配
        {
            let affected: Vec<Lyric> = self
                .store
                .get_entries_filtered::<Lyric, _>(lyrics::KEY, |v| {
                    v.get("source_id")
                        .and_then(|sid| sid.get("source_name"))
                        .and_then(|n| n.as_str())
                        == Some(source_name)
                });
            for lyric in &affected {
                self.store.remove_entry(lyrics::KEY, &lyric.id);
            }
        }

        // Songs / Artists / Albums 任一变化都要重建搜索索引
        self.bump_version();
        self.save()
    }

    /// 通过文件路径删除本地文件 + 移除库内对应的 Local SourceId。
    ///
    /// 与 [`remove_specific_song_source_ids`] 的区别：
    /// - 此方法同时删除物理文件
    /// - 不依赖 `LocalMusicSource::unindex_file`（即不要求文件已被本地源索引）
    /// - 适用于批量下载后清理旧版本（旧文件可能来自任意下载源，未通过 local_source 索引）
    ///
    /// # 参数
    /// - `file_path`: 本地音频文件的绝对路径
    ///
    /// # 返回
    /// `true` 表示找到了匹配的 SourceId 并清理（无论物理文件是否存在）
    pub fn delete_local_file_and_source(&self, file_path: &str) -> Result<bool, String> {
        let _scope = perf::scope("library.delete_local_file_and_source");
        use std::collections::HashSet;

        // 规范化路径用于匹配 entity_id（不同来源记录的路径形式可能不同，
        // 这里用原始路径与 canonicalize 后的路径都加入待删除集合）
        let mut entity_ids: HashSet<String> = HashSet::new();
        entity_ids.insert(file_path.to_string());
        if let Ok(canon) = std::fs::canonicalize(file_path) {
            if let Some(s) = canon.to_str() {
                entity_ids.insert(s.to_string());
            }
        }

        // 检测是否有匹配（避免无谓的 store 写入）
        // 直接尝试删除，根据 affected.len() 判断是否找到
        // 但 remove_specific_song_source_ids 内部已做过滤，无法直接拿到 affected 数。
        // 这里先用 get_entries_filtered 探测，决定是否要写库。
        let has_match: bool = self
            .store
            .get_entries_filtered::<serde_json::Value, _>(songs::KEY, |v| {
                v.get("source_ids")
                    .and_then(|s| s.as_array())
                    .map_or(false, |arr| {
                        arr.iter().any(|sid| {
                            sid.get("source_name").and_then(|n| n.as_str()) == Some("local")
                                && sid
                                    .get("entity_id")
                                    .and_then(|eid| eid.as_str())
                                    .map_or(false, |eid| entity_ids.contains(eid))
                        })
                    })
            })
            .into_iter()
            .next()
            .is_some();

        if !has_match {
            // 库内无匹配：仅删物理文件
            let _ = std::fs::remove_file(file_path);
            return Ok(false);
        }

        // 删除物理文件（失败不致命，继续清理库引用）
        let _ = std::fs::remove_file(file_path);

        // 清理库内 SourceId
        self.remove_specific_song_source_ids("local", &entity_ids)?;
        Ok(true)
    }

    /// 精准移除：从歌曲中删除匹配的 SourceId，并清理因此变为空的实体。
    ///
    /// 对每首歌曲，若其 `source_ids` 中有 `(source_name, entity_id)` 匹配项，
    /// 则移除该 SourceId；若歌曲的 `source_ids` 因此变空，则删除该歌曲。
    ///
    /// 随后调用 [`cleanup_empty_entities`](Self::cleanup_empty_entities)
    /// 清理所有失去全部来源引用的艺人、专辑、歌词。
    ///
    /// 适用场景：本地来源的某个文件被删除，仅移除该文件的引用，
    /// 而不影响同一来源的其他文件。
    pub fn remove_specific_song_source_ids(
        &self,
        source_name: &str,
        entity_ids: &std::collections::HashSet<String>,
    ) -> Result<(), String> {
        let _scope = perf::scope("library.remove_specific_song_source_ids");
        if entity_ids.is_empty() {
            return Ok(());
        }

        // 优化：仅反序列化 source_ids 数组中含目标 source_name 的歌曲
        // 避免反序列化全部歌曲（数千条）只为修改少量条目
        let mut affected: Vec<Song> = self
            .store
            .get_entries_filtered::<Song, _>(songs::KEY, |v| {
                v.get("source_ids")
                    .and_then(|s| s.as_array())
                    .map_or(false, |arr| {
                        arr.iter().any(|sid| {
                            sid.get("source_name").and_then(|n| n.as_str()) == Some(source_name)
                                && sid.get("entity_id")
                                    .and_then(|eid| eid.as_str())
                                    .map_or(false, |eid| entity_ids.contains(eid))
                        })
                    })
            });

        let mut any_changed = false;
        let mut to_remove: Vec<String> = Vec::with_capacity(affected.len());

        for song in affected.iter_mut() {
            let before = song.source_ids.len();
            song.source_ids.retain(|sid| {
                !(sid.source_name == source_name && entity_ids.contains(&sid.entity_id))
            });
            if song.source_ids.len() < before {
                any_changed = true;
                if song.source_ids.is_empty() {
                    to_remove.push(song.id.clone());
                }
            }
        }

        // 仅写回被修改的条目（每条用 set_subkey，避免全量重写）
        if any_changed {
            for song in &affected {
                self.store.set_subkey(songs::KEY, &song.id, song)?;
            }
            for id in &to_remove {
                self.store.remove_entry(songs::KEY, id);
            }
            self.bump_version();
        }

        // ── Lyrics ──
        // Lyrics 的 source_id 是单对象（非数组），按 (source_name, entity_id) 精准匹配
        // 场景：unindex_file / reindex_file 移除某个音频文件时，对应 Lyric 一并删除，
        //      避免孤儿 Lyric 残留（旧 song_id 指向已删除的歌曲）
        {
            let affected_lyrics: Vec<Lyric> = self
                .store
                .get_entries_filtered::<Lyric, _>(lyrics::KEY, |v| {
                    let Some(sid) = v.get("source_id") else { return false };
                    sid.get("source_name").and_then(|n| n.as_str()) == Some(source_name)
                        && sid
                            .get("entity_id")
                            .and_then(|eid| eid.as_str())
                            .map_or(false, |eid| entity_ids.contains(eid))
                });
            for lyric in &affected_lyrics {
                self.store.remove_entry(lyrics::KEY, &lyric.id);
            }
        }

        // 清理级联空实体
        self.cleanup_empty_entities()?;
        self.save()
    }

    /// 清理所有 `source_ids` 为空的实体（Song / Artist / Album / Lyric）。
    ///
    /// 优化：仅反序列化「source_ids 为空」的条目（通常 ≤ 总量的 1%），
    /// 用 `get_entries_filtered` 在 JSON 层筛选后逐条 `remove_entry`，
    /// 避免反序列化全部实体再 retain。
    pub fn cleanup_empty_entities(&self) -> Result<(), String> {
        let _scope = perf::scope("library.cleanup_empty_entities");

        // Songs — JSON 层判断 source_ids 数组为空
        let empty_songs: Vec<String> = self
            .store
            .get_entries_filtered::<serde_json::Value, _>(songs::KEY, |v| {
                v.get("source_ids")
                    .and_then(|s| s.as_array())
                    .map_or(true, |arr| arr.is_empty())
            })
            .into_iter()
            .filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
            .collect();
        for id in &empty_songs {
            self.store.remove_entry(songs::KEY, id);
        }

        // Artists — 同上
        let empty_artists: Vec<String> = self
            .store
            .get_entries_filtered::<serde_json::Value, _>(artists::KEY, |v| {
                v.get("source_ids")
                    .and_then(|s| s.as_array())
                    .map_or(true, |arr| arr.is_empty())
            })
            .into_iter()
            .filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
            .collect();
        for id in &empty_artists {
            self.store.remove_entry(artists::KEY, id);
        }

        // Albums — 同上
        let empty_albums: Vec<String> = self
            .store
            .get_entries_filtered::<serde_json::Value, _>(albums::KEY, |v| {
                v.get("source_ids")
                    .and_then(|s| s.as_array())
                    .map_or(true, |arr| arr.is_empty())
            })
            .into_iter()
            .filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
            .collect();
        for id in &empty_albums {
            self.store.remove_entry(albums::KEY, id);
        }

        // Lyrics — source_id 是单对象（非数组），判 source_name 为空
        let empty_lyrics: Vec<String> = self
            .store
            .get_entries_filtered::<serde_json::Value, _>(lyrics::KEY, |v| {
                v.get("source_id")
                    .and_then(|s| s.get("source_name"))
                    .and_then(|n| n.as_str())
                    .map_or(true, |n| n.is_empty())
            })
            .into_iter()
            .filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(String::from))
            .collect();
        for id in &empty_lyrics {
            self.store.remove_entry(lyrics::KEY, id);
        }

        if !empty_songs.is_empty() || !empty_artists.is_empty() || !empty_albums.is_empty() || !empty_lyrics.is_empty() {
            self.bump_version();
            self.save()?;
        }
        Ok(())
    }

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
        use crate::module::music_source::types::SourceType;
        use crate::module::music_source::types::AudioQuality;
        use std::collections::HashSet;

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

    /// 一次性修复库中 song.artist_ids / song.album_id 引用的无效 UUID。
    ///
    /// 修复策略：
    /// 1. 构建 artist_name_index（name_lower → artist_id），用于按 name 反查真实 ID。
    /// 2. 构建 album_index（(title_lower, artist_id) → album_id），用于按 (title, artist) 反查真实 ID。
    /// 3. 遍历所有 song：
    ///    - 用 artist_names + artist_name_index 重建 song.artist_ids（每个 name 命中已有 artist，
    ///      未命中则保留原 ID；同时合并 source_ids 到 existing artist）。
    ///    - 用 (album_title, real_artist_id) + album_index 重建 song.album_id。
    ///    - 若 song.artist_ids 全部命中且均不存在于 all_artists，则按 name 创建新 artist。
    /// 4. 同时合并重复 artist（同名但 ID 不同）与重复 album（同 title+artist 但 ID 不同）。
    /// 5. 写回 songs / artists / albums。
    ///
    /// 返回 `(repaired_songs, repaired_artists, repaired_albums)`。
    pub fn repair_references(&self) -> Result<(usize, usize, usize), String> {
        let _scope = perf::scope("library.repair_references");
        let mut all_songs = songs::get_all(&self.store);
        let mut all_artists = artists::get_all(&self.store);
        let mut all_albums = albums::get_all(&self.store);

        let mut artist_name_index = build_artist_name_index(&all_artists);
        let mut album_index = build_album_index(&all_albums);

        let mut repaired_songs = 0usize;
        let mut repaired_artists = 0usize;
        let mut repaired_albums = 0usize;

        // 第一遍：修正每首歌的 artist_ids 与 album_id
        for song in all_songs.values_mut() {
            let mut new_artist_ids: Vec<String> = Vec::with_capacity(song.artist_ids.len());
            let mut song_artists_changed = false;

            for (i, orig_id) in song.artist_ids.iter().enumerate() {
                let name = song
                    .artist_names
                    .get(i)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let lower = name.to_lowercase();

                // 优先：原 ID 真实存在 → 保留
                if all_artists.contains_key(orig_id) {
                    if !new_artist_ids.contains(orig_id) {
                        new_artist_ids.push(orig_id.clone());
                    }
                    continue;
                }

                // 次选：按 name 命中已有 artist → 用真实 ID 替换
                if let Some(real_id) = artist_name_index.get(&lower).cloned() {
                    if !new_artist_ids.contains(&real_id) {
                        new_artist_ids.push(real_id.clone());
                    }
                    song_artists_changed = true;
                    continue;
                }

                // 兜底：原 ID 不存在且 name 未命中 → 用原 ID 创建新 artist
                // （保留原 ID，避免丢失 artist 信息）
                if !new_artist_ids.contains(orig_id) {
                    new_artist_ids.push(orig_id.clone());
                }
                let new_artist = Artist {
                    id: orig_id.clone(),
                    name: name.to_string(),
                    bio: None,
                    source_ids: song
                        .source_ids
                        .iter()
                        .map(|s| s.with_entity_type(EntityType::Artist))
                        .collect(),
                };
                all_artists.insert(orig_id.clone(), new_artist);
                artist_name_index.insert(lower, orig_id.clone());
                repaired_artists += 1;
                song_artists_changed = true;
            }

            if song.artist_ids != new_artist_ids {
                song.artist_ids = new_artist_ids.clone();
                song_artists_changed = true;
            }

            // 修正 album_id：用 (album_title, real_artist_id) 反查
            let mut song_album_changed = false;
            if let (Some(album_title), Some(orig_album_id)) =
                (song.album_title.as_ref(), song.album_id.as_ref())
            {
                let real_artist_id = new_artist_ids
                    .first()
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let lookup_key = (album_title.to_lowercase(), real_artist_id.to_string());

                // 优先：原 album_id 真实存在 → 保留
                if all_albums.contains_key(orig_album_id) {
                    // ok
                } else if let Some(real_album_id) = album_index.get(&lookup_key).cloned() {
                    // 按 (title, artist_id) 命中已有 album
                    song.album_id = Some(real_album_id);
                    song_album_changed = true;
                } else {
                    // 兜底：用原 album_id 创建新 album
                    let new_album = Album {
                        id: orig_album_id.clone(),
                        title: album_title.clone(),
                        artist_id: real_artist_id.to_string(),
                        cover_url: None,
                        song_ids: vec![song.id.clone()],
                        source_ids: song
                            .source_ids
                            .iter()
                            .map(|s| s.with_entity_type(EntityType::Album))
                            .collect(),
                        year: song.year,
                    };
                    all_albums.insert(orig_album_id.clone(), new_album);
                    album_index.insert(lookup_key, orig_album_id.clone());
                    repaired_albums += 1;
                }
            }

            if song_artists_changed || song_album_changed {
                repaired_songs += 1;
            }
        }

        // 第二遍：合并重复 artist（同名但 ID 不同）
        // artist_name_index 已经记录了每个 name 对应的"首选 ID"，
        // 检测 all_artists 中其他同名 artist，把它们的 source_ids 合并到首选 artist
        //
        // 注意：不能在 `for (id, artist) in all_artists.iter()` 内直接
        // `all_artists.get_mut(preferred_id)` —— 会触发 E0502 借用冲突。
        // 因此先收集合并任务，循环结束后统一应用。
        let mut artists_to_remove: Vec<String> = Vec::new();
        // (preferred_id, source_ids_to_merge, duplicate_id)
        let mut merge_ops: Vec<(String, Vec<crate::module::music_source::types::SourceId>, String)> =
            Vec::new();
        for (id, artist) in all_artists.iter() {
            if let Some(preferred_id) = artist_name_index.get(&artist.name.to_lowercase()) {
                if preferred_id != id {
                    merge_ops.push((preferred_id.clone(), artist.source_ids.clone(), id.clone()));
                }
            }
        }
        for (preferred_id, source_ids_to_merge, duplicate_id) in merge_ops {
            if let Some(preferred) = all_artists.get_mut(&preferred_id) {
                let before = preferred.source_ids.len();
                merge_source_ids(&mut preferred.source_ids, &source_ids_to_merge);
                if preferred.source_ids.len() > before {
                    repaired_artists += 1;
                }
            }
            artists_to_remove.push(duplicate_id);
        }
        for id in &artists_to_remove {
            all_artists.remove(id);
            repaired_artists += 1;
        }

        // 第三遍：把 songs 中残留的旧 artist_id 替换为 preferred_id
        if !artists_to_remove.is_empty() {
            // 重建 name → preferred_id 映射（所有保留的 artist）
            let name_to_preferred: HashMap<String, String> = all_artists
                .iter()
                .map(|(id, a)| (a.name.to_lowercase(), id.clone()))
                .collect();
            for song in all_songs.values_mut() {
                let mut changed = false;
                for (i, aid) in song.artist_ids.iter_mut().enumerate() {
                    if !all_artists.contains_key(aid) {
                        // 当前 ID 不存在，按 name 反查
                        let name = song
                            .artist_names
                            .get(i)
                            .map(|s| s.to_lowercase())
                            .unwrap_or_default();
                        if let Some(pref) = name_to_preferred.get(&name) {
                            *aid = pref.clone();
                            changed = true;
                        }
                    }
                }
                if changed {
                    repaired_songs += 1;
                }
            }
        }

        // 写回（用 set 全量覆盖，确保删除的脏条目也被清理）
        self.store.set("songs", &all_songs)?;
        self.store.set("artists", &all_artists)?;
        self.store.set("albums", &all_albums)?;
        self.bump_version();

        Ok((repaired_songs, repaired_artists, repaired_albums))
    }
}
