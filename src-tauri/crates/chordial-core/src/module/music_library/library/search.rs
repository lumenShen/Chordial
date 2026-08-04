//! 统一搜索引擎入口 — 跨 Song / Artist / Album 的子串搜索。

use super::super::models::*;
use super::super::search;
use super::MusicLibrary;
use crate::module::music_source::types::EntityType;
use crate::module::perf;

impl MusicLibrary {
    /// 统一搜索引擎 — 跨 Song / Artist / Album 的子串搜索，
    /// 基于 `search::SearchIndex`（trigram 倒排索引，支持 Unicode）。
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
    /// 任何写操作通过 `bump_version` 失效缓存，查询时若版本号不匹配则重建。
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
}