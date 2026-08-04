//! 网易云歌单相关查询（非 trait 方法，由专门的 Tauri 命令调用）。

use super::api;
use super::types::{NeteasePlaylistBrief, NeteasePlaylistDetail, NeteaseSongDetail};

impl super::NeteaseCloudMusicApiSource {
    /// 搜索网易云歌单。
    pub fn search_playlists(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<NeteasePlaylistBrief>, String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        api::search_playlists(&self.client, query, limit, offset)
    }

    /// 获取歌单详情（元信息 + 全部曲目）。
    ///
    /// 返回 `(playlist_meta, songs)`。
    pub fn get_playlist_full(
        &self,
        playlist_id: i64,
    ) -> Result<(NeteasePlaylistDetail, Vec<NeteaseSongDetail>), String> {
        if !self.is_enabled() {
            return Err("网易云来源未启用".to_string());
        }
        let detail = api::get_playlist_detail(&self.client, playlist_id)?;
        let total = detail.trackCount.max(0) as usize;

        // 分页获取全部曲目（NeteaseCloudMusicApi /playlist/track/all 单次最多 1000）
        // 即使 detail.trackCount=0 也至少请求一次（部分接口不返回 trackCount）
        let page_size = 1000u32;
        let mut all_tracks = Vec::with_capacity(total);
        let mut offset = 0u32;
        // 最大页数保护（100 页 = 10 万首），避免 API 异常导致死循环
        let max_pages = 100usize;
        let mut pages = 0usize;

        loop {
            pages += 1;
            if pages > max_pages {
                eprintln!(
                    "[netease] get_playlist_full 超过最大页数 {}，停止分页（已获取 {} 首）",
                    max_pages, all_tracks.len()
                );
                break;
            }

            let page = api::get_playlist_tracks(&self.client, playlist_id, page_size, offset)?;
            let page_len = page.len();
            all_tracks.extend(page);

            // 终止条件（任一满足即停）：
            // 1. 本次返回 0 首（API 无更多数据）
            // 2. 本次返回 < page_size（最后一页不足一页）
            // 3. 已知 total 且累计 >= total
            if page_len == 0 || page_len < page_size as usize {
                break;
            }
            if total > 0 && all_tracks.len() >= total {
                break;
            }
            offset += page_size;
        }

        // 去重保护：若 API 异常返回重复 song，按 id 去重
        if all_tracks.len() > 1 {
            let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::with_capacity(all_tracks.len());
            let before = all_tracks.len();
            all_tracks.retain(|s| seen.insert(s.id));
            let deduped = before - all_tracks.len();
            if deduped > 0 {
                eprintln!(
                    "[netease] get_playlist_full 去重 {} 首重复曲目（{} -> {}）",
                    deduped, before, all_tracks.len()
                );
            }
        }

        eprintln!(
            "[netease] get_playlist_full playlist={} trackCount={} 实际获取={} 页数={}",
            playlist_id, total, all_tracks.len(), pages
        );

        Ok((detail, all_tracks))
    }
}