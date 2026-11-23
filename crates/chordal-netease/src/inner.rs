//! 网易云源内部状态：inner（由 lib.rs 拆分，行为不变）。

use super::*;

impl Inner {
    pub(super) async fn search_inner(
        &self,
        source_id: &SourceId,
        query: &SearchQuery,
    ) -> CoreResult<SearchResults> {
        let body = self
            .client
            .get_json(
                "/search",
                &[
                    ("keywords", query.text.as_str()),
                    ("type", "1"),
                    ("limit", &query.limit.to_string()),
                    ("offset", &query.offset.to_string()),
                ],
            )
            .await?;
        mapping::map_search(source_id, &body)
    }

    pub(super) async fn fetch_track(&self, source_id: &SourceId, id: &TrackId) -> CoreResult<Track> {
        let body = self
            .client
            .get_json("/song/detail", &[("ids", id.id.as_str())])
            .await?;
        mapping::map_song_detail(source_id, &body, &id.id)
    }

    /// 先返回磁盘缓存（stale-while-revalidate），过期则后台刷新。
    pub(super) async fn track_inner(
        self: &Arc<Self>,
        source_id: &SourceId,
        id: &TrackId,
    ) -> CoreResult<Track> {
        let key = object_key(source_id.as_str(), "track", &id.id);
        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && let Some(track) = cache.get_json::<Track>(&key)
        {
            if now_secs().saturating_sub(meta.fetched_at) >= TRACK_REFRESH_AFTER_SECS {
                self.spawn_track_refresh(source_id.clone(), id.clone(), key);
            }
            return Ok(track);
        }

        let track = self.fetch_track(source_id, id).await?;
        if let Some(cache) = &self.cache {
            cache.put_json(&key, &track, CacheTags::default());
        }
        Ok(track)
    }

    pub(super) fn spawn_track_refresh(self: &Arc<Self>, source_id: SourceId, id: TrackId, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            match inner.fetch_track(&source_id, &id).await {
                Ok(track) => {
                    if let Some(cache) = &inner.cache {
                        let outcome = cache.put_json(&key, &track, CacheTags::default());
                        tracing::debug!(
                            target: "chordial::cache",
                            key = %key,
                            ?outcome,
                            "netease track refreshed"
                        );
                    }
                }
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "netease track background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }

    pub(super) async fn fetch_lyrics(&self, id: &TrackId) -> CoreResult<Option<Lyrics>> {
        // 新接口能拿到逐词 YRC；旧接口作为兼容回退。
        let new_body = self
            .client
            .get_json("/lyric/new", &[("id", id.id.as_str())])
            .await?;
        if let Some(lyrics) = mapping::map_lyrics_new(&new_body) {
            return Ok(Some(lyrics));
        }
        let body = self
            .client
            .get_json("/lyric", &[("id", id.id.as_str())])
            .await?;
        Ok(mapping::map_lyrics(&body))
    }

    pub(super) async fn lyrics_inner(self: &Arc<Self>, id: &TrackId) -> CoreResult<Option<Lyrics>> {
        let key = object_key(id.source.as_str(), "lyric", &id.id);
        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && let Some(obj) = cache.get(&key)
            && let Ok(cached) = serde_json::from_slice::<Option<Lyrics>>(&obj.bytes)
        {
            if now_secs().saturating_sub(meta.fetched_at) >= LYRIC_REFRESH_AFTER_SECS {
                self.spawn_lyric_refresh(id.clone(), key);
            }
            return Ok(cached);
        }

        let lyrics = self.fetch_lyrics(id).await?;
        if let Some(cache) = &self.cache {
            cache.put_json(&key, &lyrics, CacheTags::default());
        }
        Ok(lyrics)
    }

    pub(super) fn spawn_lyric_refresh(self: &Arc<Self>, id: TrackId, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            match inner.fetch_lyrics(&id).await {
                Ok(lyrics) => {
                    if let Some(cache) = &inner.cache {
                        let outcome = cache.put_json(&key, &lyrics, CacheTags::default());
                        tracing::debug!(
                            target: "chordial::cache",
                            key = %key,
                            ?outcome,
                            "netease lyric refreshed"
                        );
                    }
                }
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "netease lyric background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }

    pub(super) async fn fetch_stream(&self, id: &TrackId) -> CoreResult<ResolvedStream> {
        let _scope = PerfScope::enter("netease.stream_resolve");
        let body = self
            .client
            .get_json(
                "/song/url/v1",
                &[
                    ("id", id.id.as_str()),
                    ("level", self.level.as_query_value()),
                ],
            )
            .await?;
        mapping::map_song_url(&body, &id.id)
    }

    /// 直链有时效：内存/磁盘都只在未过期时复用；临近过期后台续期。
    pub(super) async fn resolve_stream_inner(self: &Arc<Self>, id: &TrackId) -> CoreResult<ResolvedStream> {
        let now = now_secs();
        let key = object_key(id.source.as_str(), "stream", &id.id);

        if let Some(hit) = self
            .url_cache
            .lock()
            .expect("url cache lock poisoned")
            .get(&id.id)
            .cloned()
            && hit.expires_at > now
        {
            if hit.expires_at.saturating_sub(now) < URL_REFRESH_AHEAD_SECS {
                self.spawn_stream_refresh(id.clone(), key);
            }
            return Ok(hit.resolved);
        }

        if let Some(cache) = &self.cache
            && let Some(meta) = cache.meta(&key)
            && !meta.is_expired(now)
            && let Some(resolved) = cache.get_json::<ResolvedStream>(&key)
        {
            let expires_at = meta.expires_at.unwrap_or(now + URL_TTL_SECS);
            self.url_cache.lock().expect("url cache lock poisoned").put(
                id.id.clone(),
                CachedStream {
                    resolved: resolved.clone(),
                    expires_at,
                },
            );
            if expires_at.saturating_sub(now) < URL_REFRESH_AHEAD_SECS {
                self.spawn_stream_refresh(id.clone(), key);
            }
            return Ok(resolved);
        }

        let resolved = self.fetch_stream(id).await?;
        self.store_stream(id, &key, &resolved);
        Ok(resolved)
    }

    pub(super) fn store_stream(&self, id: &TrackId, key: &str, resolved: &ResolvedStream) {
        let expires_at = now_secs().saturating_add(URL_TTL_SECS);
        self.url_cache.lock().expect("url cache lock poisoned").put(
            id.id.clone(),
            CachedStream {
                resolved: resolved.clone(),
                expires_at,
            },
        );
        if let Some(cache) = &self.cache {
            cache.put_json(
                key,
                resolved,
                CacheTags {
                    expires_at: Some(expires_at),
                    ..Default::default()
                },
            );
        }
    }

    pub(super) fn spawn_stream_refresh(self: &Arc<Self>, id: TrackId, key: String) {
        if self.cache.is_none() || !self.begin_refresh(&key) {
            return;
        }
        let inner = self.clone();
        shared_runtime().spawn(async move {
            match inner.fetch_stream(&id).await {
                Ok(resolved) => inner.store_stream(&id, &key, &resolved),
                Err(e) => tracing::debug!(
                    target: "chordial::cache",
                    key = %key,
                    error = %e,
                    "netease stream background refresh failed"
                ),
            }
            inner.end_refresh(&key);
        });
    }
}
