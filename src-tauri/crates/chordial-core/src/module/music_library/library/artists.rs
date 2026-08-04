//! Artist 增删改查 + 搜索。

use super::super::models::*;
use super::super::artists;
use super::MusicLibrary;
use std::collections::HashMap;

impl MusicLibrary {
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
}