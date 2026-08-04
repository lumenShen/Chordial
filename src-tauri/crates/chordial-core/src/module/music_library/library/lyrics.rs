//! Lyric 增删改查 + 搜索。

use super::super::models::*;
use super::super::lyrics;
use super::MusicLibrary;
use std::collections::HashMap;

impl MusicLibrary {
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
}