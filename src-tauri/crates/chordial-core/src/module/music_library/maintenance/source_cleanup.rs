//! 来源清理 — 从库中移除指定来源的 SourceId、删除本地文件、清理空实体。

use super::super::library::MusicLibrary;
use super::super::models::*;
use super::super::{albums, artists, lyrics, songs};
use crate::module::perf;
use std::collections::HashSet;

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
        entity_ids: &HashSet<String>,
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
}