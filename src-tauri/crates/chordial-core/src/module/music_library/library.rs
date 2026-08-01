use super::{albums, artists, lyrics, models::*, relations, search, songs};
use crate::module::music_source::registrar::SourceCleanup;
use crate::module::music_source::types::EntityType;
use crate::module::perf;
use crate::module::storage::persistent::PersistentStore;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::merge::{
    build_album_index, build_artist_name_index, build_song_index, merge_or_init_song_in_memory,
};

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
    pub(crate) store: PersistentStore,
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
    pub(crate) fn bump_version(&self) {
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

impl SourceCleanup for MusicLibrary {
    fn remove_source_from_all_entities(&self, source_name: &str) -> Result<(), String> {
        MusicLibrary::remove_source_from_all_entities(self, source_name)
    }
}
