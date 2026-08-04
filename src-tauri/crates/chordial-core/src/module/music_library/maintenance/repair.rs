//! 引用修复 — 一次性修复库中 song.artist_ids / song.album_id 引用的无效 UUID。

use super::super::library::MusicLibrary;
use super::super::merge::{build_album_index, build_artist_name_index, merge_source_ids};
use super::super::models::*;
use super::super::{albums, artists, songs};
use crate::module::music_source::types::EntityType;
use crate::module::perf;
use std::collections::HashMap;

impl MusicLibrary {
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