//! Album 增删改查 + 搜索 + 首页统计。

use super::super::models::*;
use super::super::{albums, artists, songs};
use super::MusicLibrary;
use crate::module::perf;
use std::collections::HashMap;

impl MusicLibrary {
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
}