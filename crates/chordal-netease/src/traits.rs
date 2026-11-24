//! 网易云源 trait 实现（由 lib.rs 拆分，行为不变）。

use super::*;
use chordal_core::playlist::{
    PlaylistEditCapabilities, PlaylistEditResult, PlaylistEditor, PlaylistMutation,
};

#[async_trait]
impl MusicSource for NeteaseSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::remote_streaming()
    }

    fn catalog(&self) -> Option<&dyn CatalogSource> {
        Some(self)
    }

    fn account(&self) -> Option<&dyn AccountSource> {
        Some(self)
    }

    fn playlist_editor(&self) -> Option<&dyn PlaylistEditor> {
        Some(self)
    }

    async fn search(&self, query: &SearchQuery) -> CoreResult<SearchResults> {
        let inner = self.inner.clone();
        let source_id = self.info.id.clone();
        let query = query.clone();
        spawn_inner(move || async move { inner.search_inner(&source_id, &query).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn track(&self, id: &TrackId) -> CoreResult<Track> {
        let inner = self.inner.clone();
        let source_id = self.info.id.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.track_inner(&source_id, &id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn stream(&self, id: &TrackId) -> CoreResult<StreamHandle> {
        let inner = self.inner.clone();
        let id = id.clone();
        let resolved = spawn_inner(move || async move { inner.resolve_stream_inner(&id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))??;
        Ok(resolved.handle)
    }

    async fn resolve_stream(&self, id: &TrackId) -> CoreResult<ResolvedStream> {
        let inner = self.inner.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.resolve_stream_inner(&id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn lyrics(&self, id: &TrackId) -> CoreResult<Option<Lyrics>> {
        let inner = self.inner.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.lyrics_inner(&id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    /// 源卸载 = 回收该 namespace 的全部缓存对象（歌单/歌词/直链/封面），
    /// 并停止托管的 Node 进程（除非 managed.keep_warm）。
    async fn shutdown(&self) {
        self.inner
            .url_cache
            .lock()
            .expect("url cache lock poisoned")
            .clear();
        if let Some(service) = &self.inner.service {
            let service = service.clone();
            // 卸载可能发生在 gpui executor 上，进程操作回到共享 tokio runtime。
            let _ = shared_runtime()
                .spawn(async move { service.stop_if_not_keep_warm().await })
                .await;
        }
        if let Some(cache) = &self.inner.cache {
            let cache = cache.clone();
            let namespace = self.info.id.as_str().to_string();
            let avatar_prefix = format!("{namespace}-account-");
            // 大缓存删除不能阻塞 UI executor：交给共享 runtime 的阻塞池。
            let _ = shared_runtime()
                .spawn_blocking(move || {
                    cache.purge_namespace(&namespace);
                    cache.purge_prefix(&avatar_prefix);
                })
                .await;
        }
    }

    /// 应用退出：只停 Node，不清缓存、不注销账号。
    async fn stop_services(&self) {
        self.inner
            .url_cache
            .lock()
            .expect("url cache lock poisoned")
            .clear();
        if let Some(service) = &self.inner.service {
            let service = service.clone();
            let _ = shared_runtime()
                .spawn(async move { service.stop().await })
                .await;
        }
    }

    /// UI 加载源后预热：托管模式下会启动/安装 api-enhanced。
    async fn warm_up(&self) -> CoreResult<()> {
        let inner = self.inner.clone();
        spawn_inner(move || async move {
            if let Some(service) = &inner.service {
                service.ensure_ready().await?;
            }
            Ok(())
        })
        .await
        .map_err(|e| join_error(self.info.id.as_str(), e))?
    }
}

#[async_trait]
impl CatalogSource for NeteaseSource {
    async fn collection(&self, id: &CollectionId) -> CoreResult<CollectionPage> {
        let inner = self.inner.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.collection_inner(&id).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn collection_tracks(
        &self,
        id: &CollectionId,
        offset: u32,
        limit: u32,
    ) -> CoreResult<TrackPage> {
        let inner = self.inner.clone();
        let id = id.clone();
        spawn_inner(move || async move { inner.collection_tracks_inner(&id, offset, limit).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn search_collections(&self, query: &SearchQuery) -> CoreResult<Vec<CollectionSummary>> {
        let inner = self.inner.clone();
        let query = query.clone();
        spawn_inner(move || async move { inner.search_collections_inner(&query).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }
}

#[async_trait]
impl AccountSource for NeteaseSource {
    fn account_state(&self) -> AccountState {
        self.inner.account_state()
    }

    async fn refresh_account(&self) -> CoreResult<Option<AccountProfile>> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.refresh_account_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn login_begin(&self, method: LoginMethod) -> CoreResult<LoginChallenge> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.login_begin_inner(method).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn login_poll(&self, challenge: &LoginChallenge) -> CoreResult<LoginProgress> {
        let inner = self.inner.clone();
        let challenge = challenge.clone();
        spawn_inner(move || async move { inner.login_poll_inner(&challenge).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn send_captcha(&self, phone: &str) -> CoreResult<()> {
        let inner = self.inner.clone();
        let phone = phone.to_string();
        spawn_inner(move || async move { inner.send_captcha_inner(&phone).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn login_cellphone(&self, phone: &str, captcha: &str) -> CoreResult<AccountProfile> {
        let inner = self.inner.clone();
        let phone = phone.to_string();
        let captcha = captcha.to_string();
        spawn_inner(move || async move { inner.login_cellphone_inner(&phone, &captcha).await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn logout(&self) -> CoreResult<()> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.logout_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn user_playlists(&self) -> CoreResult<Vec<CollectionSummary>> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.user_playlists_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn daily_recommendations(&self) -> CoreResult<Vec<Track>> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.daily_songs_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn personal_fm(&self) -> CoreResult<Vec<Track>> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.personal_fm_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn recommended_playlists(&self) -> CoreResult<Vec<CollectionSummary>> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.recommended_playlists_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    async fn private_radar(&self) -> CoreResult<Vec<CollectionSummary>> {
        let inner = self.inner.clone();
        spawn_inner(move || async move { inner.private_radar_inner().await })
            .await
            .map_err(|e| join_error(self.info.id.as_str(), e))?
    }

    fn cookie(&self) -> Option<String> {
        self.inner.client.cookie()
    }
}

/// 从 `/playlist/create` 响应里取源内歌单 ID。
fn created_playlist_id(body: &serde_json::Value) -> Option<String> {
    body.get("id")
        .or_else(|| body.get("playlist").and_then(|playlist| playlist.get("id")))
        .and_then(|value| {
            value
                .as_u64()
                .map(|id| id.to_string())
                .or_else(|| value.as_str().map(str::to_string))
        })
}

/// 只保留本实例的曲目 ID（远端歌单无法写入其它源的曲目）。
fn own_track_ids(source: &SourceId, tracks: &[Track]) -> Vec<String> {
    let mut seen = HashSet::new();
    tracks
        .iter()
        .filter(|track| &track.id.source == source)
        .map(|track| track.id.id.clone())
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// 网易云远端歌单编辑：统一 `PlaylistMutation` → api-enhanced 路由。
///
/// 能力边界：
/// - 只能编辑账号自己的歌单（服务端会拒绝他人歌单）；
/// - 曲目集合只接受本实例的源内 ID，其它源的曲目会被忽略；
/// - 新建歌单时先创建空歌单，再批量写入本实例曲目。
#[async_trait]
impl PlaylistEditor for NeteaseSource {
    fn playlist_edit_capabilities(&self, kind: CollectionKind) -> PlaylistEditCapabilities {
        if kind == CollectionKind::Playlist {
            PlaylistEditCapabilities::local()
        } else {
            PlaylistEditCapabilities::read_only()
        }
    }

    async fn apply_playlist_mutation(
        &self,
        playlist: Option<&CollectionId>,
        mutation: &PlaylistMutation,
    ) -> CoreResult<PlaylistEditResult> {
        let require_id =
            || playlist.ok_or_else(|| CoreError::Config("远端歌单编辑需要歌单 ID".into()));
        match mutation {
            PlaylistMutation::Create { title, tracks } => {
                let body = self
                    .inner
                    .client
                    .get_json("/playlist/create", &[("name", title.as_str())])
                    .await?;
                let id = created_playlist_id(&body)
                    .ok_or_else(|| CoreError::source_err("netease", "新建歌单响应缺少歌单 ID"))?;
                let ids = own_track_ids(&self.info.id, tracks);
                let mut changed = 0usize;
                if !ids.is_empty() {
                    let joined = ids.join(",");
                    match self
                        .inner
                        .client
                        .get_json(
                            "/playlist/tracks",
                            &[
                                ("op", "add"),
                                ("pid", id.as_str()),
                                ("tracks", joined.as_str()),
                            ],
                        )
                        .await
                    {
                        Ok(_) => changed = ids.len(),
                        Err(e) => tracing::warn!(
                            target: "chordial::netease",
                            playlist = %id,
                            error = %e,
                            "playlist created but initial tracks failed"
                        ),
                    }
                }
                Ok(PlaylistEditResult {
                    playlist_id: Some(CollectionId::new(
                        self.info.id.clone(),
                        CollectionKind::Playlist,
                        id,
                    )),
                    changed,
                })
            }
            PlaylistMutation::Rename { title } => {
                let collection = require_id()?;
                self.inner
                    .client
                    .get_json(
                        "/playlist/name/update",
                        &[("id", collection.id.as_str()), ("name", title.as_str())],
                    )
                    .await?;
                Ok(PlaylistEditResult {
                    changed: 1,
                    ..Default::default()
                })
            }
            PlaylistMutation::Delete => {
                let collection = require_id()?;
                self.inner
                    .client
                    .get_json("/playlist/delete", &[("id", collection.id.as_str())])
                    .await?;
                Ok(PlaylistEditResult {
                    changed: 1,
                    ..Default::default()
                })
            }
            PlaylistMutation::AddTracks { tracks } => {
                let collection = require_id()?;
                let ids = own_track_ids(&self.info.id, tracks);
                if ids.is_empty() {
                    return Ok(PlaylistEditResult::default());
                }
                let joined = ids.join(",");
                self.inner
                    .client
                    .get_json(
                        "/playlist/tracks",
                        &[
                            ("op", "add"),
                            ("pid", collection.id.as_str()),
                            ("tracks", joined.as_str()),
                        ],
                    )
                    .await?;
                Ok(PlaylistEditResult {
                    changed: ids.len(),
                    ..Default::default()
                })
            }
            PlaylistMutation::RemoveTracks { tracks } => {
                let collection = require_id()?;
                let ids = own_track_ids(&self.info.id, tracks);
                if ids.is_empty() {
                    return Ok(PlaylistEditResult::default());
                }
                let joined = ids.join(",");
                self.inner
                    .client
                    .get_json(
                        "/playlist/tracks",
                        &[
                            ("op", "del"),
                            ("pid", collection.id.as_str()),
                            ("tracks", joined.as_str()),
                        ],
                    )
                    .await?;
                Ok(PlaylistEditResult {
                    changed: ids.len(),
                    ..Default::default()
                })
            }
            PlaylistMutation::Reorder { ordered, .. } => {
                let collection = require_id()?;
                let ids = own_track_ids(&self.info.id, ordered);
                if ids.len() < 2 {
                    return Ok(PlaylistEditResult::default());
                }
                let encoded = serde_json::to_string(&ids)
                    .map_err(|e| CoreError::source_err("netease", e.to_string()))?;
                self.inner
                    .client
                    .get_json(
                        "/playlist/order/update",
                        &[("id", collection.id.as_str()), ("ids", encoded.as_str())],
                    )
                    .await?;
                Ok(PlaylistEditResult {
                    changed: ids.len(),
                    ..Default::default()
                })
            }
        }
    }
}
