use super::{albums, artists, lyrics, models::*, relations, search, songs};
use crate::module::music_source::registrar::SourceCleanup;
use crate::module::music_source::types::{EntityType, SourceId};
use crate::module::perf;
use crate::module::storage::persistent::PersistentStore;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 音乐库 — 所有音乐实体的统一管理入口。
///
/// 内部持有 [`PersistentStore`]，启动时自动加载已有数据，
/// 并通过各子模块提供 CRUD、搜索和关系追溯功能。
///
/// # 子模块职责
///
/// | 模块 | 职责 |
/// |------|------|
/// | [`songs`] | 歌曲 CRUD + 搜索 |
/// | [`artists`] | 艺术家 CRUD + 搜索 |
/// | [`albums`] | 专辑 CRUD + 搜索 |
/// | [`lyrics`] | 歌词 CRUD + 搜索 |
/// | [`relations`] | 跨实体关系追溯 |
/// | [`search`] | 统一搜索引擎（trigram 倒排索引） |
pub struct MusicLibrary {
    store: PersistentStore,
    /// 库版本号 — 任何写操作递增，用于 [`search::SearchIndex`] 失效检测。
    version: AtomicU64,
    /// 搜索索引缓存 — 首次查询时构建，写操作使其失效。
    /// `Arc<SearchIndex>` 允许并发查询无锁读取。
    search_index: RwLock<Option<Arc<search::SearchIndex>>>,
}

impl MusicLibrary {
    /// 创建音乐库实例，从 `path` 指定的 JSON 文件加载已有数据。
    pub fn new(path: PathBuf) -> Self {
        Self {
            store: PersistentStore::new(path),
            version: AtomicU64::new(0),
            search_index: RwLock::new(None),
        }
    }

    /// 返回当前库版本号（每次写操作后递增）。
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    /// 标记搜索索引失效，下次查询时重建。
    /// 由所有写操作调用。
    fn bump_version(&self) {
        self.version.fetch_add(1, Ordering::Release);
        // 直接丢弃缓存的索引；正在使用旧索引的查询线程仍持有 Arc，安全继续。
        *self.search_index.write() = None;
    }

    /// 强制使搜索索引失效（外部数据变更时调用，如 reload 后）。
    pub fn invalidate_search_index(&self) {
        self.bump_version();
    }

    // ── 持久化 ───────────────────────────────────────

    /// 立即将所有未落盘的修改写入磁盘。
    pub fn save(&self) -> Result<(), String> {
        self.store.save()
    }

    /// 仅当存在未保存修改时才写入磁盘。
    pub fn save_if_dirty(&self) -> Result<(), String> {
        self.store.save_if_dirty()
    }

    /// 从磁盘重新加载，丢弃所有未保存的修改。
    pub fn reload(&self) {
        self.store.reload();
        self.bump_version();
    }

    /// 返回持久化存储的文件路径。
    pub fn store_path(&self) -> &std::path::PathBuf {
        self.store.path()
    }

    /// 从内存缓存中删除指定 key（标记脏，下次 save 时落盘）。
    pub fn remove_store_key(&self, key: &str) {
        self.store.remove(key);
        self.bump_version();
    }

    /// 清空整个音乐库（Song / Artist / Album / Lyric）。
    ///
    /// 直接调用 `PersistentStore::clear` 一次性清掉所有 key，
    /// 比 `remove_store_key` 逐个删除快得多。
    /// 调用后需 `save()` 落盘。
    pub fn clear_all(&self) {
        self.store.clear();
        self.bump_version();
    }

    // ── Song ─────────────────────────────────────────

    pub fn song_count(&self) -> usize {
        songs::count(&self.store)
    }

    pub fn get_song(&self, id: &str) -> Option<Song> {
        songs::get(&self.store, id)
    }

    /// 批量按 ID 获取歌曲（O(1) 每条，避免 N 次 IPC 调用）。
    pub fn get_songs_by_ids(&self, ids: &[String]) -> Vec<Song> {
        ids.iter()
            .filter_map(|id| songs::get(&self.store, id))
            .collect()
    }

    pub fn get_all_songs(&self) -> HashMap<String, Song> {
        self.store.get_all_map::<Song>(songs::KEY)
    }

    /// 分页获取歌曲。
    ///
    /// 当前存储为全量 HashMap，分页无法避免全量反序列化，
    /// 但可显著减少通过 IPC 传往前端的 JSON 载荷。
    pub fn get_songs_page(&self, offset: usize, limit: usize) -> Vec<Song> {
        songs::get_page(&self.store, offset, limit)
    }

    /// 添加一首歌曲（智能去重合并 + 自动初始化艺人/专辑）。
    ///
    /// 一次性加载 songs/artists/albums 到内存中处理，避免对每项实体反复反序列化。
    /// 仅当数据实际变化时才写回存储。
    /// 返回库中实际存储的歌曲 ID（合并模式返回已有歌曲 ID，新建模式返回新 ID）。
    pub fn add_song(&self, song: &Song) -> Result<String, String> {
        let _scope = perf::scope("library.add_song");
        let mut all_songs = songs::get_all(&self.store);
        let mut all_artists = artists::get_all(&self.store);
        let mut all_albums = albums::get_all(&self.store);

        let mut song_index = build_song_index(&all_songs);
        let mut artist_name_index = build_artist_name_index(&all_artists);
        let mut album_index = build_album_index(&all_albums);

        let (stored_id, sc, ac, alc) = merge_or_init_song_in_memory(
            song,
            &mut all_songs,
            &mut all_artists,
            &mut all_albums,
            &mut song_index,
            &mut artist_name_index,
            &mut album_index,
        );

        if sc {
            self.store.set("songs", &all_songs)?;
        }
        if ac {
            self.store.set("artists", &all_artists)?;
        }
        if alc {
            self.store.set("albums", &all_albums)?;
        }
        if sc || ac || alc {
            self.bump_version();
        }

        Ok(stored_id)
    }

    pub fn update_song(&self, song: &Song) -> Result<(), String> {
        songs::update(&self.store, song)?;
        self.bump_version();
        Ok(())
    }

    pub fn remove_song(&self, id: &str) -> Result<bool, String> {
        let removed = songs::remove(&self.store, id)?;
        if removed {
            self.bump_version();
        }
        Ok(removed)
    }

    pub fn search_songs(&self, query: &str) -> Vec<Song> {
        let _scope = perf::scope("library.search");
        let artists_map = artists::get_all(&self.store);
        songs::search(&self.store, query, &artists_map)
    }

    // ── Artist ───────────────────────────────────────

    pub fn artist_count(&self) -> usize {
        artists::count(&self.store)
    }

    pub fn get_artist(&self, id: &str) -> Option<Artist> {
        artists::get(&self.store, id)
    }

    /// 批量按 ID 获取艺术家（O(1) 每条，避免 `get_all_artists` 全量反序列化）。
    pub fn get_artists_by_ids(&self, ids: &[String]) -> Vec<Artist> {
        ids.iter()
            .filter_map(|id| artists::get(&self.store, id))
            .collect()
    }

    pub fn get_all_artists(&self) -> HashMap<String, Artist> {
        self.store.get_all_map::<Artist>(artists::KEY)
    }

    /// 分页获取艺术家。
    pub fn get_artists_page(&self, offset: usize, limit: usize) -> Vec<Artist> {
        artists::get_page(&self.store, offset, limit)
    }

    pub fn add_artist(&self, artist: &Artist) -> Result<(), String> {
        artists::add(&self.store, artist)?;
        self.bump_version();
        Ok(())
    }

    pub fn update_artist(&self, artist: &Artist) -> Result<(), String> {
        artists::update(&self.store, artist)?;
        self.bump_version();
        Ok(())
    }

    pub fn remove_artist(&self, id: &str) -> Result<bool, String> {
        let removed = artists::remove(&self.store, id)?;
        if removed {
            self.bump_version();
        }
        Ok(removed)
    }

    pub fn search_artists(&self, query: &str) -> Vec<Artist> {
        artists::search(&self.store, query)
    }

    // ── Album ────────────────────────────────────────

    pub fn album_count(&self) -> usize {
        albums::count(&self.store)
    }

    pub fn get_album(&self, id: &str) -> Option<Album> {
        albums::get(&self.store, id)
    }

    /// 批量按 ID 获取专辑（O(1) 每条，避免 `get_all_albums` 全量反序列化）。
    pub fn get_albums_by_ids(&self, ids: &[String]) -> Vec<Album> {
        ids.iter()
            .filter_map(|id| albums::get(&self.store, id))
            .collect()
    }

    pub fn get_all_albums(&self) -> HashMap<String, Album> {
        // 注：仍需 HashMap 返回（多处调用方 .into_values() 或按 key 查）
        // 用 get_all_map 替代 get::<HashMap> 避免整体反序列化失败 + 预分配容量
        self.store.get_all_map::<Album>(albums::KEY)
    }

    /// 分页获取专辑。
    pub fn get_albums_page(&self, offset: usize, limit: usize) -> Vec<Album> {
        albums::get_page(&self.store, offset, limit)
    }

    /// 获取首页所需的数据：计数 + 少量示例条目。
    ///
    /// 优化：计数用 `count_entries`（O(1) JSON 键数），样本用 `get_page_entries`
    /// 仅反序列化需要的 10/6/8 条，而非全部数千条。
    /// 旧实现 3x `get_all` = ~56ms，新实现 ~2ms。
    pub fn get_home_stats(&self) -> serde_json::Value {
        let _scope = perf::scope("library.get_home_stats");
        let tracks = self.store.count_entries(songs::KEY);
        let artists = self.store.count_entries(artists::KEY);
        let albums = self.store.count_entries(albums::KEY);

        let recent_songs: Vec<Song> = self.store.get_page_entries(songs::KEY, 0, 10);
        let recent_artists: Vec<Artist> = self.store.get_page_entries(artists::KEY, 0, 6);
        let recent_albums: Vec<Album> = self.store.get_page_entries(albums::KEY, 0, 8);

        serde_json::json!({
            "stats": {
                "tracks": tracks,
                "artists": artists,
                "albums": albums,
            },
            "recentTracks": recent_songs,
            "featuredArtists": recent_artists,
            "recentAlbums": recent_albums,
        })
    }

    pub fn add_album(&self, album: &Album) -> Result<(), String> {
        albums::add(&self.store, album)?;
        self.bump_version();
        Ok(())
    }

    pub fn update_album(&self, album: &Album) -> Result<(), String> {
        albums::update(&self.store, album)?;
        self.bump_version();
        Ok(())
    }

    pub fn remove_album(&self, id: &str) -> Result<bool, String> {
        let removed = albums::remove(&self.store, id)?;
        if removed {
            self.bump_version();
        }
        Ok(removed)
    }

    pub fn search_albums(&self, query: &str) -> Vec<Album> {
        let artists_map = artists::get_all(&self.store);
        albums::search(&self.store, query, &artists_map)
    }

    // ── Lyric ────────────────────────────────────────

    pub fn lyric_count(&self) -> usize {
        lyrics::count(&self.store)
    }

    pub fn get_lyric(&self, id: &str) -> Option<Lyric> {
        lyrics::get(&self.store, id)
    }

    pub fn get_all_lyrics(&self) -> HashMap<String, Lyric> {
        lyrics::get_all(&self.store)
    }

    pub fn add_lyric(&self, lyric: &Lyric) -> Result<(), String> {
        lyrics::add(&self.store, lyric)
    }

    pub fn update_lyric(&self, lyric: &Lyric) -> Result<(), String> {
        lyrics::update(&self.store, lyric)
    }

    pub fn remove_lyric(&self, id: &str) -> Result<bool, String> {
        lyrics::remove(&self.store, id)
    }

    pub fn search_lyrics(&self, query: &str) -> Vec<Lyric> {
        lyrics::search(&self.store, query)
    }

    // ── Relations ────────────────────────────────────

    /// 获取歌曲的艺术家列表。
    pub fn get_artists_of_song(&self, song_id: &str) -> Vec<Artist> {
        relations::get_artists_of_song(&self.store, song_id)
    }

    /// 获取歌曲所属的专辑。
    pub fn get_album_of_song(&self, song_id: &str) -> Option<Album> {
        relations::get_album_of_song(&self.store, song_id)
    }

    /// 获取歌曲的歌词。
    pub fn get_lyric_of_song(&self, song_id: &str) -> Option<Lyric> {
        relations::get_lyric_of_song(&self.store, song_id)
    }

    /// 获取某艺术家的所有歌曲。
    pub fn get_songs_by_artist(&self, artist_id: &str) -> Vec<Song> {
        relations::get_songs_by_artist(&self.store, artist_id)
    }

    /// 获取某艺术家的所有专辑。
    pub fn get_albums_by_artist(&self, artist_id: &str) -> Vec<Album> {
        relations::get_albums_by_artist(&self.store, artist_id)
    }

    /// 获取专辑中的所有歌曲。
    pub fn get_songs_in_album(&self, album_id: &str) -> Vec<Song> {
        relations::get_songs_in_album(&self.store, album_id)
    }

    // ── Sources ──────────────────────────────────────

    /// 获取歌曲的所有来源 ID。
    pub fn get_source_ids_of_song(&self, song_id: &str) -> Vec<crate::module::music_source::types::SourceId> {
        relations::get_source_ids_of_song(&self.store, song_id)
    }

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

    /// 批量添加歌曲（智能去重合并 + 自动初始化艺人/专辑）。
    ///
    /// 一次性加载全部数据，预建 O(1) 查重索引后逐首合并。
    /// 仅当数据实际变化时才序列化写回对应集合。
    ///
    /// 返回与输入等长的 `Vec<String>`，每个元素为对应歌曲在库中的实际存储 ID。
    pub fn add_songs_batch(&self, songs: &[Song]) -> Result<Vec<String>, String> {
        let _scope = perf::scope("library.add_songs_batch");
        let mut all_songs = songs::get_all(&self.store);
        let mut all_artists = artists::get_all(&self.store);
        let mut all_albums = albums::get_all(&self.store);

        let mut song_index = build_song_index(&all_songs);
        let mut artist_name_index = build_artist_name_index(&all_artists);
        let mut album_index = build_album_index(&all_albums);

        let mut songs_changed = false;
        let mut artists_changed = false;
        let mut albums_changed = false;
        let mut stored_ids = Vec::with_capacity(songs.len());

        for song in songs {
            let (stored_id, sc, ac, alc) = merge_or_init_song_in_memory(
                song,
                &mut all_songs,
                &mut all_artists,
                &mut all_albums,
                &mut song_index,
                &mut artist_name_index,
                &mut album_index,
            );
            stored_ids.push(stored_id);
            songs_changed |= sc;
            artists_changed |= ac;
            albums_changed |= alc;
        }

        if songs_changed {
            self.store.set("songs", &all_songs)?;
        }
        if artists_changed {
            self.store.set("artists", &all_artists)?;
        }
        if albums_changed {
            self.store.set("albums", &all_albums)?;
        }
        if songs_changed || artists_changed || albums_changed {
            self.bump_version();
        }

        Ok(stored_ids)
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

    // ── 统一搜索 ─────────────────────────────────────

    /// 统一搜索引擎 — 跨 Song / Artist / Album 的子串搜索，
    /// 基于 [`search::SearchIndex`]（trigram 倒排索引，支持 Unicode）。
    ///
    /// # 参数
    ///
    /// - `query`：搜索关键词，大小写不敏感的子串匹配
    /// - `entity_type`：限定实体类型；`None` 表示三类全搜
    /// - `source_name`：限定来源名称；`None` 表示不限制
    /// - `limit_per_type`：每类实体最多返回多少条；`None` 表示无限制
    ///
    /// # 缓存策略
    ///
    /// 首次查询构建索引并缓存；后续查询无锁读 `Arc<SearchIndex>`。
    /// 任何写操作通过 [`bump_version`](Self::bump_version) 失效缓存，
    /// 查询时若版本号不匹配则重建。
    pub fn search(
        &self,
        query: &str,
        entity_type: Option<EntityType>,
        source_name: Option<&str>,
        limit_per_type: Option<usize>,
    ) -> search::SearchResults {
        let _scope = perf::scope("library.search");

        let index = self.get_or_build_search_index();
        let filter = search::SearchFilter {
            query,
            entity_type,
            source_name,
            limit: limit_per_type,
        };
        let id_sets = search::search_ids(&index, &filter);

        // 按 ID 反序列化具体实体；若指定了 source_name，再做来源过滤
        let songs = if !id_sets.songs.is_empty() {
            let mut v: Vec<Song> = self
                .get_songs_by_ids(&id_sets.songs)
                .into_iter()
                .filter(|s| {
                    source_name.map_or(true, |name| {
                        s.source_ids.iter().any(|sid| sid.source_name == name)
                    })
                })
                .collect();
            v.truncate(limit_per_type.unwrap_or(usize::MAX));
            v
        } else {
            Vec::new()
        };

        let artists = if !id_sets.artists.is_empty() {
            let mut v: Vec<Artist> = self
                .get_artists_by_ids(&id_sets.artists)
                .into_iter()
                .filter(|a| {
                    source_name.map_or(true, |name| {
                        a.source_ids.iter().any(|sid| sid.source_name == name)
                    })
                })
                .collect();
            v.truncate(limit_per_type.unwrap_or(usize::MAX));
            v
        } else {
            Vec::new()
        };

        let albums = if !id_sets.albums.is_empty() {
            let mut v: Vec<Album> = self
                .get_albums_by_ids(&id_sets.albums)
                .into_iter()
                .filter(|a| {
                    source_name.map_or(true, |name| {
                        a.source_ids.iter().any(|sid| sid.source_name == name)
                    })
                })
                .collect();
            v.truncate(limit_per_type.unwrap_or(usize::MAX));
            v
        } else {
            Vec::new()
        };

        search::SearchResults {
            songs,
            artists,
            albums,
        }
    }

    /// 获取或构建缓存的搜索索引（按版本号校验）。
    ///
    /// 临界区仅做版本比较与 `Arc::clone`，构建在临界区外完成
    /// （构建期间不阻塞其他读线程，仅最后写入时短暂加锁）。
    fn get_or_build_search_index(&self) -> Arc<search::SearchIndex> {
        // 快速路径：读锁检查已有缓存且版本匹配
        {
            let guard = self.search_index.read();
            if let Some(arc) = guard.as_ref() {
                if arc.version() == self.version() {
                    return Arc::clone(arc);
                }
            }
        }

        // 慢速路径：构建新索引。构建在锁外完成，避免阻塞其他读者。
        let new_index = Arc::new(search::SearchIndex::build(&self.store, self.version()));

        // 写入缓存。若期间另一线程已抢先构建并写入，丢弃本线程结果即可。
        // 双重检查版本号，避免覆盖更新鲜的索引。
        let mut guard = self.search_index.write();
        if let Some(existing) = guard.as_ref() {
            if existing.version() == self.version() {
                return Arc::clone(existing);
            }
        }
        *guard = Some(Arc::clone(&new_index));
        new_index
    }

    // ── 私有辅助 ─────────────────────────────────────

}

/// 在内存中合并或插入一首歌。
///
/// 使用预建索引实现 O(1) 去重查找。索引在新建条目时会同步更新。
///
/// 返回 `(stored_id, songs_changed, artists_changed, albums_changed)`。
fn merge_or_init_song_in_memory(
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
fn merge_artists_in_memory(
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
fn merge_album_in_memory(
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
fn build_song_index(all_songs: &HashMap<String, Song>) -> HashMap<(String, Vec<String>), String> {
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
fn build_artist_name_index(all_artists: &HashMap<String, Artist>) -> HashMap<String, String> {
    let mut index = HashMap::with_capacity(all_artists.len());
    for (id, artist) in all_artists {
        index.insert(artist.name.to_lowercase(), id.clone());
    }
    index
}

/// 构建专辑索引：(title_lower, artist_id) → album_id。
fn build_album_index(all_albums: &HashMap<String, Album>) -> HashMap<(String, String), String> {
    let mut index = HashMap::with_capacity(all_albums.len());
    for (id, album) in all_albums {
        index.insert((album.title.to_lowercase(), album.artist_id.clone()), id.clone());
    }
    index
}

/// 将 `incoming` 中不存在于 `existing` 的 SourceId 追加进去（去重合并）。
///
/// 优化：existing 较大时使用 HashSet 临时索引，避免 O(n²) 扫描。
fn merge_source_ids(existing: &mut Vec<SourceId>, incoming: &[SourceId]) {
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

// ── SourceCleanup 实现 ────────────────────────────

impl SourceCleanup for MusicLibrary {
    fn remove_source_from_all_entities(&self, source_name: &str) -> Result<(), String> {
        MusicLibrary::remove_source_from_all_entities(self, source_name)
    }
}
