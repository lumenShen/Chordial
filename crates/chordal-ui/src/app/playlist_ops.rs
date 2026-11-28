//! 本地/订阅歌单操作：创建、重命名、删除、追加、排序、订阅与刷新。

use super::*;
use chordal_core::playlist::{PlaylistId, PlaylistMutation};

impl ChordialApp {
    /// 统一编辑入口：本地歌单直接写本地存储；订阅歌单交给源的
    /// `PlaylistEditor`（网易云实现为远端路由），UI 不再按源分叉。
    pub(super) fn apply_playlist_mutation(
        &mut self,
        id: &PlaylistId,
        mutation: PlaylistMutation,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(playlist) = self.playlists.get(id) else {
            return false;
        };
        let subscribed = playlist.kind.subscribed_collection().cloned();
        let Some(collection) = subscribed else {
            return self.apply_local_playlist_mutation(id, mutation, cx);
        };
        // 取消订阅永远只影响本地缓存，绝不删除远端歌单。
        if matches!(mutation, PlaylistMutation::Delete) {
            self.delete_playlist(id, cx);
            return true;
        }
        let Some(source) = self.registry.get(&collection.source) else {
            self.info_message = Some("订阅来源已关闭，暂时无法编辑该歌单".into());
            cx.notify();
            return false;
        };
        let Some(editor) = source.playlist_editor() else {
            self.info_message = Some("该音源不支持歌单编辑".into());
            cx.notify();
            return false;
        };
        let caps = editor.playlist_edit_capabilities(collection.kind);
        let supported = match &mutation {
            PlaylistMutation::Create { .. } => caps.create,
            PlaylistMutation::Rename { .. } => caps.rename,
            PlaylistMutation::AddTracks { .. } => caps.add_tracks,
            PlaylistMutation::RemoveTracks { .. } => caps.remove_tracks,
            PlaylistMutation::Reorder { .. } => caps.reorder,
            PlaylistMutation::Delete => caps.delete,
        };
        if !supported {
            self.info_message = Some("该歌单不支持这种编辑操作".into());
            cx.notify();
            return false;
        }
        let playlist_id = id.clone();
        let source_for_task = source.clone();
        cx.spawn(async move |weak, cx| {
            let Some(editor) = source_for_task.playlist_editor() else {
                return;
            };
            let result = editor
                .apply_playlist_mutation(Some(&collection), &mutation)
                .await;
            let _ = weak.update(cx, |app, cx| {
                match result {
                    Ok(edit) => {
                        app.info_message = Some(format!("已同步到音源（{} 项变更）", edit.changed));
                        // 远端是权威：本地订阅缓存立即重新拉一次。
                        app.refresh_playlist_subscription(&playlist_id, cx);
                    }
                    Err(e) => {
                        app.status_message = Some(format!("歌单编辑失败：{e}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        true
    }

    fn apply_local_playlist_mutation(
        &mut self,
        id: &PlaylistId,
        mutation: PlaylistMutation,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = match mutation {
            PlaylistMutation::Rename { title } => self.playlists.rename(id, title),
            PlaylistMutation::AddTracks { tracks } => self.playlists.add_tracks(id, tracks) > 0,
            PlaylistMutation::RemoveTracks { tracks } => {
                let mut removed = false;
                for track in tracks {
                    removed |= self.playlists.remove_identity(id, &track.identity_key());
                }
                removed
            }
            PlaylistMutation::Reorder { from, to, .. } => self.playlists.reorder(id, from, to),
            PlaylistMutation::Delete => {
                if self.playlists.delete(id) {
                    if self.page == Some(MainPage::Playlist(id.clone())) {
                        self.page = None;
                    }
                    true
                } else {
                    false
                }
            }
            PlaylistMutation::Create { .. } => false,
        };
        if changed {
            self.playlist_page_cache.borrow_mut().take();
            cx.notify();
        }
        changed
    }

    pub(super) fn create_local_playlist(&mut self, cx: &mut Context<Self>) -> PlaylistId {
        let id = self.playlists.create_local("新建歌单", Vec::new());
        self.page = Some(MainPage::Playlist(id.clone()));
        cx.notify();
        id
    }

    pub(super) fn delete_playlist(&mut self, id: &PlaylistId, cx: &mut Context<Self>) {
        if self.playlists.delete(id) {
            if self.page == Some(MainPage::Playlist(id.clone())) {
                self.page = None;
            }
            cx.notify();
        }
    }

    pub(super) fn rename_playlist(
        &mut self,
        id: &PlaylistId,
        title: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        if self.playlists.rename(id, title) {
            cx.notify();
        }
    }

    pub(super) fn add_tracks_to_playlist(
        &mut self,
        id: &PlaylistId,
        tracks: Vec<Track>,
        cx: &mut Context<Self>,
    ) -> usize {
        let added = self.playlists.add_tracks(id, tracks);
        if added > 0 {
            cx.notify();
        }
        added
    }

    pub(super) fn remove_track_from_playlist(
        &mut self,
        id: &PlaylistId,
        identity_key: &str,
        cx: &mut Context<Self>,
    ) {
        if self.playlists.remove_identity(id, identity_key) {
            cx.notify();
        }
    }

    pub(super) fn reorder_playlist(
        &mut self,
        id: &PlaylistId,
        from: usize,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        if self.playlists.reorder(id, from, to) {
            cx.notify();
        }
    }

    /// 把当前播放队列整体追加到歌单（跨源曲目会保留各自的源实例 ID）。
    pub(super) fn add_queue_to_playlist(
        &mut self,
        id: &PlaylistId,
        cx: &mut Context<Self>,
    ) -> usize {
        let tracks = self.queue.as_ref().clone();
        let subscribed = self
            .playlists
            .get(id)
            .is_some_and(|playlist| playlist.kind.is_subscribed());
        if subscribed {
            self.apply_playlist_mutation(id, PlaylistMutation::AddTracks { tracks }, cx);
            return 0;
        }
        let added = self.add_tracks_to_playlist(id, tracks, cx);
        if added > 0 {
            self.info_message = Some(format!("已添加 {added} 首到歌单"));
        } else {
            self.info_message = Some("队列中的曲目都已在歌单里".into());
        }
        cx.notify();
        added
    }

    /// 播放歌单：把本地缓存里的曲目快照设为队列。
    pub(super) fn play_playlist(&mut self, id: &PlaylistId, cx: &mut Context<Self>) {
        self.play_playlist_at(id, 0, cx);
    }

    pub(super) fn play_playlist_at(
        &mut self,
        id: &PlaylistId,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(playlist) = self.playlists.get(id) else {
            return;
        };
        if index >= playlist.entries.len() {
            return;
        }
        let tracks: Vec<Track> = playlist
            .entries
            .iter()
            .map(|entry| entry.track.clone())
            .collect();
        self.set_queue(Arc::new(tracks), None);
        self.queue_pos = Some(index);
        self.resolve_and_play(cx);
        cx.notify();
    }

    /// 替换当前队列：统一递增代数并更新续接追踪，避免过期请求写回新队列。
    pub(super) fn set_queue(&mut self, tracks: Arc<Vec<Track>>, refill: Option<QueueRefill>) {
        self.queue = tracks;
        // 下标含义已变化，随机轮次必须按新队列重建。
        self.shuffle_plan = None;
        self.queue_gen = self.queue_gen.wrapping_add(1);
        self.queue_refill = refill;
        self.queue_clear_confirm = false;
    }

    /// 按 identity_key 去重后把新曲目追加到列表末尾，返回实际追加数量。
    pub(super) fn append_unique_tracks(base: &mut Vec<Track>, incoming: Vec<Track>) -> usize {
        let mut seen: HashSet<String> = base.iter().map(|t| t.identity_key().to_owned()).collect();
        let before = base.len();
        base.extend(
            incoming
                .into_iter()
                .filter(|t| seen.insert(t.identity_key().to_owned())),
        );
        base.len() - before
    }

    /// 播放任意曲目快照（源页面预览、歌单页行点击共用）。
    pub(super) fn play_tracks(
        &mut self,
        tracks: Arc<Vec<Track>>,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        self.play_tracks_with_refill(tracks, index, None, cx);
    }

    /// 从推荐详情页播放：私人 FM 记录追踪来源，接近末尾时可续接。
    pub(super) fn play_feed_tracks(
        &mut self,
        source: &SourceId,
        kind: SourceFeedKind,
        tracks: Arc<Vec<Track>>,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if index >= tracks.len() {
            return;
        }
        let refill = (kind == SourceFeedKind::Fm).then(|| QueueRefill {
            source: source.clone(),
            in_flight: false,
            last_empty_at: None,
        });
        self.play_tracks_with_refill(tracks, index, refill, cx);
        // 首批本身就很短时也提前补一批，避免刚开播就撞到末尾。
        self.maybe_refill_fm_queue(cx);
    }

    fn play_tracks_with_refill(
        &mut self,
        tracks: Arc<Vec<Track>>,
        index: usize,
        refill: Option<QueueRefill>,
        cx: &mut Context<Self>,
    ) {
        if index >= tracks.len() {
            return;
        }
        self.set_queue(tracks, refill);
        self.queue_pos = Some(index);
        self.resolve_and_play(cx);
        cx.notify();
    }

    pub(super) fn open_playlist_picker(
        &mut self,
        track: Track,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        self.track_menu = None;
        self.playlist_picker = Some(PlaylistPickerState { track, x, y });
        cx.notify();
    }

    pub(super) fn close_playlist_picker(&mut self, cx: &mut Context<Self>) {
        if self.playlist_picker.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn add_picker_track_to_playlist(
        &mut self,
        playlist_id: &PlaylistId,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.playlist_picker.take() else {
            return;
        };
        let subscribed = self
            .playlists
            .get(playlist_id)
            .is_some_and(|playlist| playlist.kind.is_subscribed());
        if subscribed {
            self.apply_playlist_mutation(
                playlist_id,
                PlaylistMutation::AddTracks {
                    tracks: vec![picker.track],
                },
                cx,
            );
        } else {
            let added = self.playlists.add_tracks(playlist_id, vec![picker.track]);
            self.info_message = Some(if added > 0 {
                "已添加到歌单".into()
            } else {
                "这首歌已经在歌单里".into()
            });
        }
        cx.notify();
    }

    pub(super) fn create_playlist_from_picker(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.playlist_picker.take() else {
            return;
        };
        let id = self.playlists.create_local("新建歌单", vec![picker.track]);
        self.info_message = Some("已新建歌单并添加".into());
        self.page = Some(MainPage::Playlist(id));
        if let Some(playlist) = self.page.clone().and_then(|page| match page {
            MainPage::Playlist(id) => self.playlists.get(&id).cloned(),
            _ => None,
        }) {
            self.source_playlist_rename_input
                .update(cx, |input, cx| input.set_content(playlist.title, cx));
        }
        cx.notify();
    }

    pub(super) fn apply_playlist_rename(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(MainPage::Playlist(id)) = self.page.clone() else {
            return;
        };
        let title = text.trim().to_string();
        if title.is_empty() {
            return;
        }
        self.apply_playlist_mutation(&id, PlaylistMutation::Rename { title }, cx);
    }

    pub(super) fn playlist_summaries(&self) -> Arc<Vec<chordal_core::playlist::PlaylistSummary>> {
        let revision = self.playlists.revision();
        let mut cache = self.playlist_summaries.borrow_mut();
        match cache.as_ref() {
            Some((cached_revision, summaries)) if *cached_revision == revision => summaries.clone(),
            _ => {
                let summaries = Arc::new(self.playlists.summaries());
                *cache = Some((revision, summaries.clone()));
                summaries
            }
        }
    }

    /// 歌单页行快照：按 identity 判定源可用性 / 本地替代，缓存到
    /// playlist 版本、registry 版本或本地库快照变化为止。
    pub(super) fn playlist_rows(
        &self,
        playlist: &chordal_core::playlist::SavedPlaylist,
        cx: &App,
    ) -> Arc<Vec<PlaylistRowView>> {
        let registry_version = self.registry.version();
        let library_ptr = Arc::as_ptr(&self.library) as usize;
        {
            let cache = self.playlist_page_cache.borrow();
            if let Some(cache) = cache.as_ref()
                && cache.id == playlist.id
                && cache.updated_at_ms == playlist.updated_at_ms
                && cache.registry_version == registry_version
                && cache.library_ptr == library_ptr
            {
                return cache.rows.clone();
            }
        }
        let locals: Vec<Arc<chordal_local::LocalSource>> = self
            .enabled_local_sources(cx)
            .into_iter()
            .map(|(_, local)| local)
            .collect();
        let rows: Vec<PlaylistRowView> = playlist
            .entries
            .iter()
            .map(|entry| {
                let track = &entry.track;
                let loaded = self.registry.get(&track.id.source).is_some();
                let substitute = if loaded {
                    None
                } else {
                    locals
                        .iter()
                        .find_map(|local| local.find_by_identity(track))
                };
                let unavailable = !loaded && substitute.is_none();
                let local_substitute = !loaded && substitute.is_some();
                let artist = track
                    .artists
                    .iter()
                    .map(|artist| artist.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ");
                let source_label = self
                    .source_config_by_id(&track.id.source.0)
                    .map(|source| source.name.clone())
                    .unwrap_or_else(|| track.id.source.0.clone());
                PlaylistRowView {
                    track: track.clone(),
                    identity_key: entry.identity_key.clone(),
                    title: SharedString::from(track.title.clone()),
                    artist: SharedString::from(artist),
                    source_label: SharedString::from(source_label),
                    duration: SharedString::from(
                        track
                            .duration_ms
                            .map(fmt_ms)
                            .unwrap_or_else(|| "--:--".into()),
                    ),
                    unavailable,
                    local_substitute,
                }
            })
            .collect();
        let rows = Arc::new(rows);
        *self.playlist_page_cache.borrow_mut() = Some(PlaylistPageCache {
            id: playlist.id.clone(),
            updated_at_ms: playlist.updated_at_ms,
            registry_version,
            library_ptr,
            rows: rows.clone(),
        });
        rows
    }

    /// 订阅歌单打开后后台刷新：先渲染本地缓存，远端返回后再替换。
    pub(super) fn refresh_playlist_subscription(
        &mut self,
        id: &PlaylistId,
        cx: &mut Context<Self>,
    ) {
        let Some(playlist) = self.playlists.get(id) else {
            return;
        };
        let Some(collection) = playlist.kind.subscribed_collection().cloned() else {
            return;
        };
        let Some(source) = self.registry.get(&collection.source) else {
            self.info_message = Some("订阅来源已关闭，当前显示本地缓存".into());
            cx.notify();
            return;
        };
        let playlist_id = id.clone();
        cx.spawn(async move |weak, cx| {
            let collection_for_task = collection.clone();
            let fetched = match ChordialApp::run_background(async move {
                let Some(catalog) = source.catalog() else {
                    return Err(chordal_core::CoreError::Config("该音源不支持集合页".into()));
                };
                catalog.collection(&collection_for_task).await
            })
            .await
            {
                Ok(result) => result,
                Err(_) => Err(chordal_core::CoreError::Other(
                    "后台任务繁忙，请稍后重试".into(),
                )),
            };
            let _ = weak.update(cx, |app, cx| {
                match fetched {
                    Ok(page) => {
                        if let Some(summary) = page.summary {
                            let changed = app.playlists.refresh_subscription(
                                &playlist_id,
                                &summary,
                                page.tracks,
                            );
                            if changed {
                                app.playlist_page_cache.borrow_mut().take();
                            }
                        }
                    }
                    Err(e) => {
                        app.info_message = Some(format!("订阅刷新失败，继续显示本地缓存：{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 关闭/卸载来源后，播放歌单曲目会走本地替代；没有替代则提示不可用。
    pub(super) fn playlist_track_available(&self, track: &Track, cx: &App) -> bool {
        if self.registry.get(&track.id.source).is_some() {
            return true;
        }
        self.enabled_local_sources(cx)
            .iter()
            .any(|(_, local)| local.find_by_identity(track).is_some())
    }

    /// 从集合页订阅当前歌单（本地缓存一份，后续打开先读本地）。
    pub(super) fn subscribe_current_collection(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.collection.as_ref() else {
            return;
        };
        let collection_id = view.id.clone();
        if self.playlists.is_subscribed(&collection_id) {
            self.info_message = Some("已经在我的歌单里".into());
            cx.notify();
            return;
        }
        let tracks = view.tracks.as_ref().clone();
        if tracks.is_empty() {
            self.status_message = Some("歌单还没有加载完成".into());
            cx.notify();
            return;
        }
        let cover = tracks
            .iter()
            .find_map(|track| track.album.as_ref().and_then(|album| album.cover.clone()));
        let summary = CollectionSummary {
            id: collection_id,
            title: view.title.clone(),
            cover,
            creator: view.creator.clone(),
            description: view.description.clone(),
            track_count: Some(tracks.len() as u64),
            year: None,
            subscribed: true,
            kind: chordal_core::model::CollectionKind::Playlist,
        };
        let id = self.playlists.upsert_subscription(&summary, tracks);
        self.info_message = Some("已加入我的歌单（本地缓存）".into());
        self.page = Some(MainPage::Playlist(id));
        cx.notify();
    }

    pub(super) fn load_source_user_playlists(&mut self, id: &SourceId, cx: &mut Context<Self>) {
        if self.source_user_playlists.contains_key(id)
            || self.source_user_playlists_loading.contains(id)
        {
            return;
        }
        let Some(source) = self.registry.get(id) else {
            return;
        };
        self.source_user_playlists_loading.insert(id.clone());
        let source_id = id.clone();
        cx.spawn(async move |weak, cx| {
            let result = match source.account() {
                Some(account) => {
                    // 新启用/新打开的实例可能还没探测过 Cookie：先校验账号，
                    // 否则 user_playlists 会因为本地 session=Unknown 直接报错。
                    if account.account_state().profile().is_none() {
                        if let Err(e) = account.refresh_account().await {
                            let _ = weak.update(cx, |app, cx| {
                                app.source_user_playlists_loading.remove(&source_id);
                                app.info_message = Some(format!("账号校验失败：{e}"));
                                cx.notify();
                            });
                            return;
                        }
                    }
                    account.user_playlists().await
                }
                None => {
                    let _ = weak.update(cx, |app, cx| {
                        app.source_user_playlists_loading.remove(&source_id);
                        cx.notify();
                    });
                    return;
                }
            };
            let _ = weak.update(cx, |app, cx| {
                app.source_user_playlists_loading.remove(&source_id);
                match result {
                    Ok(list) => {
                        app.source_user_playlists.insert(source_id, Arc::new(list));
                    }
                    Err(e) => {
                        app.info_message = Some(format!("加载用户歌单失败：{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
