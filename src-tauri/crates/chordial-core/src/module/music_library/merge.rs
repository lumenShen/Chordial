//! 合并/去重辅助函数 — 在内存中合并歌曲/艺人/专辑，使用预建索引实现 O(1) 查重。

use super::models::{Album, Artist, Song};
use crate::module::music_source::types::{EntityType, SourceId};
use std::collections::HashMap;

/// 在内存中合并或插入一首歌。
///
/// 使用预建索引实现 O(1) 去重查找。索引在新建条目时会同步更新。
///
/// 返回 `(stored_id, songs_changed, artists_changed, albums_changed)`。
pub(super) fn merge_or_init_song_in_memory(
    song: &Song,
    all_songs: &mut HashMap<String, Song>,
    all_artists: &mut HashMap<String, Artist>,
    all_albums: &mut HashMap<String, Album>,
    song_index: &mut HashMap<(String, Vec<String>), String>,
    artist_name_index: &mut HashMap<String, String>,
    album_index: &mut HashMap<(String, String), String>,
) -> (String, bool, bool, bool) {
    let title_lower = song.title.to_lowercase();
    let mut names_sorted: Vec<String> = song.artist_names.iter().map(|n| n.to_lowercase()).collect();
    names_sorted.sort();
    let lookup_key = (title_lower, names_sorted);

    if let Some(existing_id) = song_index.get(&lookup_key).cloned() {
        // ── 合并模式 ──
        let mut songs_changed = false;

        // 先调用 merge_artists_in_memory 拿到 resolved 真实 artist_ids。
        // 这一步必须在更新 existing 之前完成，因为后续 album 合并也要用 resolved ids。
        let (artists_changed, resolved_artist_ids) = merge_artists_in_memory(
            &song.artist_ids,
            &song.artist_names,
            &song.source_ids,
            all_artists,
            artist_name_index,
        );

        // 用 resolved artist_ids 调用 merge_album_in_memory，确保 lookup_key 用真实 artist_id
        let mut resolved_album_id: Option<String> = None;
        let mut albums_changed = false;
        if let (Some(album_id), Some(album_title)) = (&song.album_id, &song.album_title) {
            let (ac, rid) = merge_album_in_memory(
                album_id,
                album_title,
                &resolved_artist_ids,
                &song.source_ids,
                &existing_id,
                song.year,
                all_albums,
                album_index,
            );
            albums_changed = ac;
            resolved_album_id = rid;
        }

        if let Some(existing) = all_songs.get_mut(&existing_id) {
            let sid_before = existing.source_ids.len();
            merge_source_ids(&mut existing.source_ids, &song.source_ids);
            if existing.source_ids.len() > sid_before {
                songs_changed = true;
            }

            // 用 resolved_artist_ids 修正 existing.artist_ids：
            // - resolved_ids 中每个 ID 都是真实存在的 artist_id
            // - 保留 existing.artist_ids 中真实存在的 ID（避免丢失历史合并的正确 ID）
            // - 过滤掉 existing.artist_ids 中已不存在的脏 UUID（修复历史数据）
            let mut new_artist_ids: Vec<String> =
                Vec::with_capacity(resolved_artist_ids.len() + existing.artist_ids.len());
            for aid in &resolved_artist_ids {
                if !new_artist_ids.contains(aid) {
                    new_artist_ids.push(aid.clone());
                }
            }
            for aid in existing.artist_ids.iter() {
                if all_artists.contains_key(aid) && !new_artist_ids.contains(aid) {
                    new_artist_ids.push(aid.clone());
                }
            }
            if new_artist_ids != existing.artist_ids {
                existing.artist_ids = new_artist_ids;
                songs_changed = true;
            }

            // 用 resolved_album_id 修正 existing.album_id
            if let Some(real_album_id) = &resolved_album_id {
                if existing.album_id.as_deref() != Some(real_album_id.as_str()) {
                    existing.album_id = Some(real_album_id.clone());
                    existing.album_title = song.album_title.clone();
                    songs_changed = true;
                }
            } else if existing.album_id.is_none() {
                existing.album_id = song.album_id.clone();
                existing.album_title = song.album_title.clone();
                songs_changed = true;
            }
            // 反向填充 year：已有歌曲缺失 year 而新扫描歌曲有 year 时补齐
            if existing.year.is_none() && song.year.is_some() {
                existing.year = song.year;
                songs_changed = true;
            }
            // 反向填充 lyric_id：已有歌曲缺失 lyric_id 而新扫描歌曲有 lyric_id 时补齐。
            // 典型场景：首次扫描时 lyrics 键不存在导致 Lyric 写入失败，
            // song.lyric_id 残留为 None；修复 set_subkey 后重扫，
            // 合并模式需要把新 lyric_id 回填到 existing 才能让前端找到歌词。
            if existing.lyric_id.is_none() && song.lyric_id.is_some() {
                existing.lyric_id = song.lyric_id.clone();
                songs_changed = true;
            }
        }

        (existing_id, songs_changed, artists_changed, albums_changed)
    } else {
        // ── 新建模式 ──
        // 先调用 merge_artists_in_memory 拿到 resolved 真实 artist_ids
        let (artists_changed, resolved_artist_ids) = merge_artists_in_memory(
            &song.artist_ids,
            &song.artist_names,
            &song.source_ids,
            all_artists,
            artist_name_index,
        );

        // 用 resolved artist_ids 调用 merge_album_in_memory
        let mut resolved_album_id: Option<String> = None;
        let mut albums_changed = false;
        if let (Some(album_id), Some(album_title)) = (&song.album_id, &song.album_title) {
            let (ac, rid) = merge_album_in_memory(
                album_id,
                album_title,
                &resolved_artist_ids,
                &song.source_ids,
                &song.id,
                song.year,
                all_albums,
                album_index,
            );
            albums_changed = ac;
            resolved_album_id = rid;
        }

        // 用 resolved ids 修正即将插入的 song（克隆后覆盖字段）
        let mut song_to_insert = song.clone();
        if song_to_insert.artist_ids != resolved_artist_ids {
            song_to_insert.artist_ids = resolved_artist_ids;
        }
        if let Some(real_album_id) = &resolved_album_id {
            if song_to_insert.album_id.as_deref() != Some(real_album_id.as_str()) {
                song_to_insert.album_id = Some(real_album_id.clone());
            }
        }
        all_songs.insert(song_to_insert.id.clone(), song_to_insert);
        song_index.insert(lookup_key, song.id.clone());

        (song.id.clone(), true, artists_changed, albums_changed)
    }
}

/// 在内存中合并或创建艺人，使用名称索引 O(1) 查找。
///
/// 返回 `(是否有变化, resolved_artist_ids)`：
/// - 第二个元素是与 `artist_ids` 等长的真实 artist_id 列表。
///   当按 name 命中已有 artist 时，对应位置会被替换为 existing artist 的真实 ID；
///   否则保留 `artist_ids` 中的原值（新建或按 id 命中）。
/// - 调用方应使用 `resolved_artist_ids` 覆盖 `song.artist_ids`，
///   避免 song 引用每次扫描生成的不存在的临时 UUID。
pub(super) fn merge_artists_in_memory(
    artist_ids: &[String],
    artist_names: &[String],
    song_source_ids: &[SourceId],
    all_artists: &mut HashMap<String, Artist>,
    artist_name_index: &mut HashMap<String, String>,
) -> (bool, Vec<String>) {
    let artist_sids: Vec<SourceId> = song_source_ids
        .iter()
        .map(|s| s.with_entity_type(EntityType::Artist))
        .collect();
    let mut changed = false;
    let mut resolved_ids: Vec<String> = Vec::with_capacity(artist_ids.len());

    for (i, artist_id) in artist_ids.iter().enumerate() {
        let artist_name = artist_names.get(i).map(|s| s.as_str()).unwrap_or("");

        if let Some(artist) = all_artists.get_mut(artist_id) {
            // 按 id 命中已有 artist（首次扫描后再次扫描同一首歌时会走这里）
            let sid_before = artist.source_ids.len();
            merge_source_ids(&mut artist.source_ids, &artist_sids);
            if artist.source_ids.len() > sid_before {
                changed = true;
            }
            resolved_ids.push(artist_id.clone());
        } else if let Some(aid) = artist_name_index.get(&artist_name.to_lowercase()).cloned() {
            // 按 name 命中已有 artist：用真实 ID 替换 song 中的临时 UUID
            if let Some(artist) = all_artists.get_mut(&aid) {
                let sid_before = artist.source_ids.len();
                merge_source_ids(&mut artist.source_ids, &artist_sids);
                if artist.source_ids.len() > sid_before {
                    changed = true;
                }
            }
            resolved_ids.push(aid);
        } else {
            // 新建 artist，保留 song 中的 ID
            all_artists.insert(
                artist_id.clone(),
                Artist {
                    id: artist_id.clone(),
                    name: artist_name.to_string(),
                    bio: None,
                    source_ids: artist_sids.clone(),
                },
            );
            artist_name_index.insert(artist_name.to_lowercase(), artist_id.clone());
            changed = true;
            resolved_ids.push(artist_id.clone());
        }
    }

    (changed, resolved_ids)
}

/// 在内存中合并或创建专辑，使用 (title, artist_id) 索引 O(1) 查找。
///
/// `song_year` 来自扫描得到的 Song.year，会反向写入 album.year（若 album.year 为 None）。
/// 这实现了"专辑年份从同名歌曲年份聚合"的需求。
///
/// 返回 `(是否有变化, resolved_album_id)`：
/// - 第二个元素是真实 album_id（按 id 命中或新建时为传入的 `album_id`，
///   按 (title, artist_id) 索引命中时为已存在 album 的真实 ID）。
/// - 调用方应使用 `resolved_album_id` 覆盖 `song.album_id`，
///   避免 song 引用每次扫描生成的不存在的临时 UUID。
///
/// 注意：`artist_ids` 应为已 resolve 的真实 artist_id 列表（来自 `merge_artists_in_memory`），
/// 否则不同歌曲的同一专辑会因 artist_id 不同而被视为不同专辑。
pub(super) fn merge_album_in_memory(
    album_id: &str,
    album_title: &str,
    artist_ids: &[String],
    song_source_ids: &[SourceId],
    song_id: &str,
    song_year: Option<u32>,
    all_albums: &mut HashMap<String, Album>,
    album_index: &mut HashMap<(String, String), String>,
) -> (bool, Option<String>) {
    let album_sids: Vec<SourceId> = song_source_ids
        .iter()
        .map(|s| s.with_entity_type(EntityType::Album))
        .collect();
    let artist_id = artist_ids.first().map(|s| s.as_str()).unwrap_or("");
    let lookup_key = (album_title.to_lowercase(), artist_id.to_string());
    let mut changed = false;
    let resolved_id: Option<String>;

    if let Some(album) = all_albums.get_mut(album_id) {
        // 按 id 命中已有 album
        let sid_before = album.source_ids.len();
        merge_source_ids(&mut album.source_ids, &album_sids);
        if album.source_ids.len() > sid_before {
            changed = true;
        }
        if !album.song_ids.iter().any(|s| s == song_id) {
            album.song_ids.push(song_id.to_string());
            changed = true;
        }
        // 反向填充 year：album 缺失 year 而扫描到的 song.year 存在时补齐
        if album.year.is_none() && song_year.is_some() {
            album.year = song_year;
            changed = true;
        }
        // album.artist_id 可能为空或历史脏数据，用 resolved 的真实 artist_id 反向填充
        if album.artist_id.is_empty() && !artist_id.is_empty() {
            album.artist_id = artist_id.to_string();
            changed = true;
        }
        resolved_id = Some(album_id.to_string());
    } else if let Some(aid) = album_index.get(&lookup_key).cloned() {
        // 按 (title, artist_id) 索引命中已有 album：用真实 ID 替换 song 中的临时 UUID
        if let Some(album) = all_albums.get_mut(&aid) {
            let sid_before = album.source_ids.len();
            merge_source_ids(&mut album.source_ids, &album_sids);
            if album.source_ids.len() > sid_before {
                changed = true;
            }
            if !album.song_ids.iter().any(|s| s == song_id) {
                album.song_ids.push(song_id.to_string());
                changed = true;
            }
            if album.year.is_none() && song_year.is_some() {
                album.year = song_year;
                changed = true;
            }
            if album.artist_id.is_empty() && !artist_id.is_empty() {
                album.artist_id = artist_id.to_string();
                changed = true;
            }
        }
        resolved_id = Some(aid);
    } else {
        all_albums.insert(
            album_id.to_string(),
            Album {
                id: album_id.to_string(),
                title: album_title.to_string(),
                artist_id: artist_id.to_string(),
                cover_url: None,
                song_ids: vec![song_id.to_string()],
                source_ids: album_sids,
                year: song_year,
            },
        );
        album_index.insert(lookup_key, album_id.to_string());
        changed = true;
        resolved_id = Some(album_id.to_string());
    }

    (changed, resolved_id)
}

/// 构建歌曲去重索引：(title_lower, sorted_artist_names_lower) → song_id。
pub(super) fn build_song_index(all_songs: &HashMap<String, Song>) -> HashMap<(String, Vec<String>), String> {
    let mut index = HashMap::with_capacity(all_songs.len());
    for (id, song) in all_songs {
        let title_lower = song.title.to_lowercase();
        let mut names: Vec<String> = song.artist_names.iter().map(|n| n.to_lowercase()).collect();
        names.sort();
        index.insert((title_lower, names), id.clone());
    }
    index
}

/// 构建艺人名称索引：name_lower → artist_id。
pub(super) fn build_artist_name_index(all_artists: &HashMap<String, Artist>) -> HashMap<String, String> {
    let mut index = HashMap::with_capacity(all_artists.len());
    for (id, artist) in all_artists {
        index.insert(artist.name.to_lowercase(), id.clone());
    }
    index
}

/// 构建专辑索引：(title_lower, artist_id) → album_id。
pub(super) fn build_album_index(all_albums: &HashMap<String, Album>) -> HashMap<(String, String), String> {
    let mut index = HashMap::with_capacity(all_albums.len());
    for (id, album) in all_albums {
        index.insert((album.title.to_lowercase(), album.artist_id.clone()), id.clone());
    }
    index
}

/// 将 `incoming` 中不存在于 `existing` 的 SourceId 追加进去（去重合并）。
///
/// 优化：existing 较大时使用 HashSet 临时索引，避免 O(n²) 扫描。
pub(super) fn merge_source_ids(existing: &mut Vec<SourceId>, incoming: &[SourceId]) {
    if existing.is_empty() {
        existing.extend_from_slice(incoming);
        return;
    }
    // existing 较大（>16）时构建 HashSet 索引；小切片直接线性扫描更快
    if existing.len() > 16 {
        // 先收集待追加项（避免在迭代 incoming 时同时可变借用 existing）
        let set: std::collections::HashSet<&SourceId> =
            existing.iter().collect();
        let to_add: Vec<SourceId> = incoming
            .iter()
            .filter(|sid| !set.contains(sid))
            .cloned()
            .collect();
        existing.extend(to_add);
    } else {
        for sid in incoming {
            if !existing.contains(sid) {
                existing.push(sid.clone());
            }
        }
    }
}
