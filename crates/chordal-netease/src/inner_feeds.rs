//! 网易云源内部状态：账号推荐流（每日推荐 / 私人 FM / 推荐歌单 / 私人雷达）。
//!
//! 推荐结果随账号与会话变化，缓存交给 UI 层（打开页面时先渲染快照，
//! 后台按需刷新），源适配器本身只做一次网络请求，避免双重缓存失效。

use super::*;

impl Inner {
    pub(super) async fn daily_songs_inner(&self) -> CoreResult<Vec<Track>> {
        let body = self.client.get_json("/recommend/songs", &[]).await?;
        let tracks = mapping::map_daily_songs(&self.source_id, &body);
        if tracks.is_empty() {
            return Err(CoreError::source_err(
                "netease",
                "每日推荐为空（接口未返回 dailySongs）",
            ));
        }
        Ok(tracks)
    }

    pub(super) async fn personal_fm_inner(&self) -> CoreResult<Vec<Track>> {
        let body = self.client.get_json("/personal_fm", &[]).await?;
        let tracks = mapping::map_personal_fm(&self.source_id, &body);
        if tracks.is_empty() {
            return Err(CoreError::source_err(
                "netease",
                "私人 FM 为空（接口未返回可播放曲目）",
            ));
        }
        Ok(tracks)
    }

    pub(super) async fn recommended_playlists_inner(&self) -> CoreResult<Vec<CollectionSummary>> {
        let body = self.client.get_json("/recommend/resource", &[]).await?;
        let mut playlists =
            mapping::map_playlist_list(&self.source_id, &body, CollectionKind::Playlist);
        // 私人雷达会出现在每日推荐歌单里；拆出去单独展示，避免重复。
        playlists.retain(|playlist| !playlist.title.contains("雷达"));
        if playlists.is_empty() {
            return Err(CoreError::source_err(
                "netease",
                "每日推荐歌单为空（需要登录账号）",
            ));
        }
        Ok(playlists)
    }

    /// 私人雷达：优先走专用接口（部分自建服务会实现）；官方
    /// api-enhanced 没有该路由，此时从每日推荐歌单里筛出「私人雷达」条目。
    pub(super) async fn private_radar_inner(&self) -> CoreResult<Vec<CollectionSummary>> {
        if let Ok(body) = self.client.get_json("/personal/radar", &[]).await {
            let playlists =
                mapping::map_playlist_list(&self.source_id, &body, CollectionKind::Playlist);
            if !playlists.is_empty() {
                return Ok(playlists);
            }
        }
        let body = self.client.get_json("/recommend/resource", &[]).await?;
        let radars: Vec<CollectionSummary> =
            mapping::map_playlist_list(&self.source_id, &body, CollectionKind::Playlist)
                .into_iter()
                .filter(|playlist| playlist.title.contains("雷达"))
                .collect();
        if radars.is_empty() {
            return Err(CoreError::source_err(
                "netease",
                "当前接口未返回私人雷达歌单",
            ));
        }
        Ok(radars)
    }
}
