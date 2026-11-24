//! 网易云源内部状态：inner_collections（由 lib.rs 拆分，行为不变）。

use super::*;

/// `/playlist/track/all` 单次最多返回的曲目数：服务端按 1000 分片，
/// 超过 1000 首的歌单必须按 offset 续拉。
const PLAYLIST_TRACK_PAGE: u64 = 1000;
/// 分页请求上限（按每页 1000 计约 20 万首），只用于防止服务端异常时死循环。
const PLAYLIST_TRACK_MAX_PAGES: u32 = 200;

impl Inner {
    /// 拉取歌单全部曲目。`track_count` 来自 `/playlist/detail`：
    /// - 有值时用它作为总长度上限（少请求一次空页）；
    /// - 缺省时以“短页/空页”判定列表结束。
    ///
    /// 不因短页直接结束：部分服务端会把 limit 压到更小的分片大小，
    /// 只要没到 trackCount 且还有数据就继续按 offset 续拉。
    pub(super) async fn fetch_all_playlist_tracks(
        &self,
        id: &str,
        track_count: u64,
    ) -> CoreResult<serde_json::Value> {
        let mut songs: Vec<serde_json::Value> = Vec::new();
        let mut offset: u64 = 0;
        for _ in 0..PLAYLIST_TRACK_MAX_PAGES {
            let want = if track_count > offset {
                (track_count - offset).min(PLAYLIST_TRACK_PAGE)
            } else if track_count == 0 {
                PLAYLIST_TRACK_PAGE
            } else {
                break;
            };
            let limit_s = want.to_string();
            let offset_s = offset.to_string();
            let body = self
                .client
                .get_json(
                    "/playlist/track/all",
                    &[
                        ("id", id),
                        ("limit", limit_s.as_str()),
                        ("offset", offset_s.as_str()),
                    ],
                )
                .await?;
            let page: Vec<serde_json::Value> = body
                .get("songs")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            let got = page.len() as u64;
            songs.extend(page);
            offset = offset.saturating_add(got);
            // 空页一定到底；未知总数时短页视为最后一页。
            if got == 0 || (track_count == 0 && got < want) {
                break;
            }
            if track_count > 0 && offset >= track_count {
                break;
            }
        }
        Ok(serde_json::json!({ "songs": songs }))
    }

    pub(super) async fn fetch_user_playlists(
        &self,
        uid: &str,
    ) -> CoreResult<Vec<CollectionSummary>> {
        let body = self
            .client
            .get_json(
                "/user/playlist",
                &[("uid", uid), ("limit", "1000"), ("offset", "0")],
            )
            .await?;
        Ok(mapping::map_user_playlists(&self.source_id, &body))
    }

    pub(super) async fn user_playlists_inner(
        self: &Arc<Self>,
    ) -> CoreResult<Vec<CollectionSummary>> {
        let Some(profile) = self.account_state().profile().cloned() else {
            return Err(CoreError::Config("尚未登录网易云".into()));
        };
        let key = self.account_key(&profile.uid, "playlists", "all");
        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && let Some(list) = cache.get_json::<Vec<CollectionSummary>>(&key)
        {
            if now_secs().saturating_sub(meta.fetched_at) >= ACCOUNT_REFRESH_AFTER_SECS {
                self.spawn_user_playlists_refresh(profile.uid, key);
            }
            return Ok(list);
        }
        let list = self.fetch_user_playlists(&profile.uid).await?;
        if let Some(cache) = &self.cache {
            cache.put_json(&key, &list, CacheTags::default());
        }
        Ok(list)
    }

    pub(super) fn spawn_user_playlists_refresh(self: &Arc<Self>, uid: String, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            // 账号切换后旧 uid 的结果不得写回新会话。
            let epoch = inner.session_epoch.load(Ordering::Acquire);
            match inner.fetch_user_playlists(&uid).await {
                Ok(list) if inner.session_epoch.load(Ordering::Acquire) == epoch => {
                    if let Some(cache) = &inner.cache {
                        cache.put_json(&key, &list, CacheTags::default());
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "user playlists background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }

    pub(super) async fn fetch_collection_page(
        &self,
        id: &CollectionId,
    ) -> CoreResult<CollectionPage> {
        match id.kind {
            CollectionKind::Playlist => {
                let detail = self
                    .client
                    .get_json("/playlist/detail", &[("id", id.id.as_str())])
                    .await?;
                let track_count = detail
                    .pointer("/playlist/trackCount")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let tracks = self
                    .fetch_all_playlist_tracks(id.id.as_str(), track_count)
                    .await?;
                mapping::map_playlist_page(&self.source_id, &detail, &tracks)
            }
            CollectionKind::Album => {
                let body = self
                    .client
                    .get_json("/album", &[("id", id.id.as_str())])
                    .await?;
                mapping::map_album_page(&self.source_id, &body)
            }
            CollectionKind::Artist => {
                let detail = self
                    .client
                    .get_json("/artist/detail", &[("id", id.id.as_str())])
                    .await?;
                let songs = self
                    .client
                    .get_json("/artist/top/song", &[("id", id.id.as_str())])
                    .await?;
                let albums = self
                    .client
                    .get_json(
                        "/artist/album",
                        &[("id", id.id.as_str()), ("limit", "50"), ("offset", "0")],
                    )
                    .await?;
                mapping::map_artist_page(&self.source_id, &detail, &songs, &albums)
            }
        }
    }

    pub(super) fn collection_key(&self, id: &CollectionId) -> String {
        format!("{}/collection/{:?}/{}", self.source_id, id.kind, id.id)
    }

    pub(super) async fn collection_inner(
        self: &Arc<Self>,
        id: &CollectionId,
    ) -> CoreResult<CollectionPage> {
        let key = self.collection_key(id);
        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && let Some(page) = cache.get_json::<CollectionPage>(&key)
        {
            if now_secs().saturating_sub(meta.fetched_at) >= COLLECTION_REFRESH_AFTER_SECS {
                self.spawn_collection_refresh(id.clone(), key);
            }
            return Ok(page);
        }
        let page = self.fetch_collection_page(id).await?;
        if let Some(cache) = &self.cache {
            cache.put_json(&key, &page, CacheTags::default());
        }
        Ok(page)
    }

    pub(super) fn spawn_collection_refresh(self: &Arc<Self>, id: CollectionId, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            let epoch = inner.session_epoch.load(Ordering::Acquire);
            match inner.fetch_collection_page(&id).await {
                Ok(page) if inner.session_epoch.load(Ordering::Acquire) == epoch => {
                    if let Some(cache) = &inner.cache {
                        cache.put_json(&key, &page, CacheTags::default());
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "collection background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }

    pub(super) async fn collection_tracks_inner(
        self: &Arc<Self>,
        id: &CollectionId,
        offset: u32,
        limit: u32,
    ) -> CoreResult<TrackPage> {
        match id.kind {
            CollectionKind::Playlist => {
                let limit_s = limit.to_string();
                let offset_s = offset.to_string();
                let body = self
                    .client
                    .get_json(
                        "/playlist/track/all",
                        &[
                            ("id", id.id.as_str()),
                            ("limit", &limit_s),
                            ("offset", &offset_s),
                        ],
                    )
                    .await?;
                Ok(mapping::map_songs_page(&self.source_id, &body, offset))
            }
            _ => {
                let page = self.collection_inner(id).await?;
                let total = page.total;
                let tracks: Vec<Track> = page
                    .tracks
                    .into_iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .collect();
                Ok(TrackPage {
                    has_more: (offset as u64 + tracks.len() as u64) < total,
                    tracks,
                    total,
                    offset,
                })
            }
        }
    }

    pub(super) async fn search_collections_inner(
        &self,
        query: &SearchQuery,
    ) -> CoreResult<Vec<CollectionSummary>> {
        let kind = match query.kind {
            SearchKind::Track => return Ok(Vec::new()),
            SearchKind::Album => "10",
            SearchKind::Artist => "100",
            SearchKind::Playlist => "1000",
        };
        let limit = query.limit.to_string();
        let offset = query.offset.to_string();
        let body = self
            .client
            .get_json(
                "/cloudsearch",
                &[
                    ("keywords", query.text.as_str()),
                    ("type", kind),
                    ("limit", &limit),
                    ("offset", &offset),
                ],
            )
            .await?;
        Ok(mapping::map_search_collections(
            &self.source_id,
            &body,
            query.kind,
        ))
    }

    pub(super) fn begin_refresh(&self, key: &str) -> bool {
        self.refreshing
            .lock()
            .expect("refresh lock poisoned")
            .insert(key.to_string())
    }

    pub(super) fn end_refresh(&self, key: &str) {
        self.refreshing
            .lock()
            .expect("refresh lock poisoned")
            .remove(key);
    }
}
