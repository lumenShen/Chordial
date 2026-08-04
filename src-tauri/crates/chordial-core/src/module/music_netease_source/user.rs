//! 网易云用户账户与"我的资源"查询。

use super::api;
use super::types::{NeteaseAlbumDetail, NeteasePlaylistBrief, NeteaseSongDetail,
    NeteaseSubAlbum, NeteaseUserAccountResponse};

impl super::NeteaseCloudMusicApiSource {
    /// 获取当前登录账号信息（依赖 cookie）。
    pub fn get_user_account(&self) -> Result<NeteaseUserAccountResponse, String> {
        api::get_user_account(&self.client)
    }

    /// 获取指定用户的全部歌单（创建 + 收藏）。
    pub fn get_user_playlists(&self, uid: i64) -> Result<Vec<NeteasePlaylistBrief>, String> {
        api::get_user_playlists(&self.client, uid)
    }

    /// 获取当前用户收藏的专辑（依赖 cookie，分页）。
    ///
    /// 返回 `(albums, total_count)`。
    pub fn get_user_albums(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<NeteaseSubAlbum>, i64), String> {
        api::get_user_albums(&self.client, limit, offset)
    }

    /// 获取专辑详情（含全部曲目）。
    pub fn get_album_detail(
        &self,
        album_id: i64,
    ) -> Result<(NeteaseAlbumDetail, Vec<NeteaseSongDetail>), String> {
        api::get_album_detail(&self.client, album_id)
    }
}