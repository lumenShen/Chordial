//! Song 增删改查 + 搜索。

use super::super::merge::{
    build_album_index, build_artist_name_index, build_song_index, merge_or_init_song_in_memory,
};
use super::super::models::*;
use super::super::{albums, artists, songs};
use super::MusicLibrary;
use crate::module::perf;
use std::collections::HashMap;

impl MusicLibrary {
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
}