//! 批量添加歌曲（智能去重合并 + 自动初始化艺人/专辑）。

use super::super::merge::{
    build_album_index, build_artist_name_index, build_song_index, merge_or_init_song_in_memory,
};
use super::super::models::*;
use super::super::{albums, artists, songs};
use super::MusicLibrary;
use crate::module::perf;

impl MusicLibrary {
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
}