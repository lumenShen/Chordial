//! 跨实体关系追溯 + 来源 ID。

use super::super::models::*;
use super::super::relations;
use super::MusicLibrary;

impl MusicLibrary {
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

    /// 获取歌曲的所有来源 ID。
    pub fn get_source_ids_of_song(
        &self,
        song_id: &str,
    ) -> Vec<crate::module::music_source::types::SourceId> {
        relations::get_source_ids_of_song(&self.store, song_id)
    }
}