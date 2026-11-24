//! 音源实例管理：启用/关闭、新增、编辑、删除、扫描与库聚合。
//!
//! 配置是唯一真相；registry 只保存“当前活跃”的实例。关闭音源用
//! `deactivate`（不 shutdown），删除实例才 `unload/unregister`。

use super::*;

use chordal_core::model::SourceId;
use chordal_core::registry::{SourceDescriptor, SourceStatus};
use chordal_core::source::SourceInfo;

use crate::config::{SOURCE_TYPE_LOCAL, SOURCE_TYPE_NETEASE, SourceInstanceConfig, next_source_id};
use crate::sources;

impl ChordialApp {
    pub(super) fn source_config(&self, id: &SourceId) -> Option<&SourceInstanceConfig> {
        self.source_configs.iter().find(|source| source.id == id.0)
    }

    pub(super) fn source_config_by_id(&self, id: &str) -> Option<&SourceInstanceConfig> {
        self.source_configs.iter().find(|source| source.id == id)
    }

    pub(super) fn source_is_enabled(&self, id: &SourceId) -> bool {
        self.source_config_by_id(&id.0)
            .is_some_and(|source| source.enabled)
    }

    /// 音源开关的展示状态：关闭优先于 registry 的 Registered。
    pub(super) fn source_status(&self, id: &SourceId) -> SourceStatus {
        if !self.source_is_enabled(id) {
            return SourceStatus::Registered;
        }
        self.registry.status(id).unwrap_or(SourceStatus::Registered)
    }

    pub(super) fn source_is_healthy(&self, id: &SourceId) -> bool {
        self.source_is_enabled(id) && self.registry.get(id).is_some()
    }

    pub(super) fn enabled_local_sources(
        &self,
        cx: &App,
    ) -> Vec<(SourceId, Arc<chordal_local::LocalSource>)> {
        let globals = cx.global::<Globals>();
        self.source_configs
            .iter()
            .filter(|source| source.is_local() && source.enabled)
            .filter_map(|source| {
                let id = SourceId::new(source.id.clone());
                globals
                    .local_sources
                    .get(&id)
                    .cloned()
                    .map(|local| (id, local))
            })
            .collect()
    }

    /// 兼容旧代码的单本地句柄：始终指向第一个已启用的本地实例。
    fn refresh_primary_local(&mut self, cx: &App) {
        self.local_source = self
            .enabled_local_sources(cx)
            .into_iter()
            .next()
            .map(|(_, local)| local);
    }

    pub(super) fn primary_netease_id(&self) -> Option<SourceId> {
        self.source_configs
            .iter()
            .find(|source| source.is_netease() && source.enabled)
            .or_else(|| {
                self.source_configs
                    .iter()
                    .find(|source| source.is_netease())
            })
            .map(|source| SourceId::new(source.id.clone()))
    }

    /// 持久化音源配置；同时回写旧版 `local_roots` / `netease` 字段。
    pub(super) fn persist_source_configs(&mut self) {
        let mut cfg = crate::config::load_or_default();
        cfg.sources = self.source_configs.clone();
        cfg.normalize_sources();
        self.source_configs = cfg.sources.clone();
        crate::config::save(&cfg);
    }

    /// 启用/关闭音源。关闭只摘除活跃实例，不清理缓存/不停止托管服务。
    pub(super) fn set_source_enabled(
        &mut self,
        id: &SourceId,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .source_configs
            .iter()
            .position(|source| source.id == id.0)
        else {
            return;
        };
        if self.source_configs[index].enabled == enabled {
            return;
        }
        self.source_configs[index].enabled = enabled;
        self.persist_source_configs();

        if enabled {
            if self.registry.get(id).is_none() {
                let builder = cx.global::<Globals>().builders.get(id).cloned();
                if let Some(build) = builder
                    && let Err(e) = self.registry.load(id, |info| build(info))
                {
                    self.status_message = Some(format!("加载音源失败：{e}"));
                }
            }
            self.after_source_enabled(id, cx);
        } else {
            let _ = self.registry.deactivate(id);
            if self
                .source_config_by_id(&id.0)
                .is_some_and(|s| s.is_local())
            {
                self.remove_source_tracks_from_library(id);
            }
            if self.collection.as_ref().is_some_and(|c| &c.id.source == id) {
                self.collection = None;
                let old_cover = self.collection_cover.take();
                self.retire_image(old_cover);
                if self.view_tab == ViewTab::Collection {
                    self.view_tab = ViewTab::Library;
                }
            }
            if self.page == Some(MainPage::Source(id.clone())) {
                self.page = Some(MainPage::ManageSources);
            }
        }
        self.refresh_primary_local(cx);
        cx.notify();
    }

    fn after_source_enabled(&mut self, id: &SourceId, cx: &mut Context<Self>) {
        if self
            .source_config_by_id(&id.0)
            .is_some_and(|s| s.is_local())
        {
            let local = cx.global::<Globals>().local_sources.get(id).cloned();
            if let Some(local) = local {
                self.spawn_library_scan_source(local, cx);
            }
            return;
        }
        // NetEase：预热该实例并校验账号；账号态是实例私有的，
        // 自定义实例不能等默认实例的 bootstrap。
        let source = self.registry.get(id);
        let id = id.clone();
        cx.spawn(async move |weak, cx| {
            if let Some(source) = source {
                let _ = source.warm_up().await;
            }
            let _ = weak.update(cx, |app, cx| {
                app.refresh_source_account(&id, cx);
                if app.page == Some(MainPage::Source(id)) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn attach_source_instance(
        &mut self,
        built: sources::BuiltSource,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let id = built.info.id.clone();
        let builder = built.builder();
        let instance = built.source.clone();
        let descriptor: Box<
            dyn Fn(
                    &SourceInfo,
                )
                    -> chordal_core::error::CoreResult<Arc<dyn chordal_core::source::MusicSource>>
                + Send
                + Sync,
        > = Box::new(move |_| Ok(instance.clone()));
        if let Err(e) = self.registry.register(SourceDescriptor {
            info: built.info.clone(),
            build: descriptor,
        }) {
            self.status_message = Some(format!("注册音源失败：{e}"));
            return false;
        }
        {
            let globals = cx.global_mut::<Globals>();
            globals.builders.insert(id.clone(), builder);
            if let Some(local) = &built.local {
                globals.local_sources.insert(id.clone(), local.clone());
            } else {
                globals.local_sources.remove(&id);
            }
            if let Some(netease) = &built.netease {
                globals.netease_sources.insert(id.clone(), netease.clone());
            } else {
                globals.netease_sources.remove(&id);
            }
        }
        if enabled {
            let instance = built.source.clone();
            if let Err(e) = self.registry.load(&id, |_| Ok(instance.clone())) {
                self.status_message = Some(format!("加载音源失败：{e}"));
                return false;
            }
        }
        true
    }

    /// 新增一个音源实例。ID 由配置层生成，保证多实例不冲突。
    pub(super) fn create_source_instance(
        &mut self,
        mut config: SourceInstanceConfig,
        cx: &mut Context<Self>,
    ) -> Option<SourceId> {
        config.normalize();
        if config.id.trim().is_empty() {
            config.id = self.new_source_id(&config.source_type);
        }
        if self.source_config_by_id(&config.id).is_some() {
            config.id = self.new_source_id(&config.source_type);
        }
        let id = SourceId::new(config.id.clone());
        let built = match sources::build(&config, self.cache.clone()) {
            Ok(built) => built,
            Err(e) => {
                self.status_message = Some(format!("创建音源失败：{e}"));
                return None;
            }
        };
        self.source_configs.push(config);
        self.persist_source_configs();
        if !self.attach_source_instance(built, true, cx) {
            self.source_configs.retain(|source| source.id != id.0);
            self.persist_source_configs();
            return None;
        }
        self.after_source_enabled(&id, cx);
        self.refresh_primary_local(cx);
        cx.notify();
        Some(id)
    }

    /// 编辑实例：名称/开关只改元信息；适配器配置变化则重建实例。
    pub(super) fn replace_source_config(
        &mut self,
        mut next: SourceInstanceConfig,
        cx: &mut Context<Self>,
    ) -> bool {
        next.normalize();
        let Some(index) = self
            .source_configs
            .iter()
            .position(|source| source.id == next.id)
        else {
            return false;
        };
        let old = self.source_configs[index].clone();
        if old.id != next.id {
            return false;
        }
        let adapter_changed = old.source_type != next.source_type
            || local_roots(&old) != local_roots(&next)
            || old.netease != next.netease;
        let enabled_changed = old.enabled != next.enabled;

        if !adapter_changed {
            // 只改名称/优先级/开关：更新 registry 的展示信息，不重建实例。
            // 开关必须在写回新配置之前应用，否则 set_source_enabled 会看到
            // 新旧值相同而提前返回，registry 实际状态不会变化。
            if enabled_changed {
                self.set_source_enabled(&SourceId::new(next.id.clone()), next.enabled, cx);
            }
            self.source_configs[index] = next.clone();
            self.persist_source_configs();
            if let Some(mut info) = self.registry.slot_info(&SourceId::new(next.id.clone())) {
                info.display_name = next.name.clone();
                info.priority = next.priority;
                let _ = self
                    .registry
                    .update_info(&SourceId::new(next.id.clone()), info);
            }
            self.refresh_primary_local(cx);
            cx.notify();
            return true;
        }

        let id = SourceId::new(next.id.clone());
        // 旧实例彻底卸载（配置编辑/类型变化，缓存可重建）。
        let old_instance = self.registry.get(&id);
        let _ = self.registry.deactivate(&id);
        self.registry.unregister(&id);
        if let Some(source) = old_instance {
            cx.spawn(async move |_, _| {
                source.shutdown().await;
            })
            .detach();
        }
        {
            let globals = cx.global_mut::<Globals>();
            globals.builders.remove(&id);
            globals.local_sources.remove(&id);
            globals.netease_sources.remove(&id);
        }
        if old.is_local() {
            self.remove_source_tracks_from_library(&id);
        }
        let built = match sources::build(&next, self.cache.clone()) {
            Ok(built) => built,
            Err(e) => {
                self.status_message = Some(format!("重建音源失败：{e}"));
                // 配置保留但实例不可用；管理页会显示失败状态。
                self.source_configs[index] = next;
                self.persist_source_configs();
                cx.notify();
                return false;
            }
        };
        self.source_configs[index] = next.clone();
        self.persist_source_configs();
        if !self.attach_source_instance(built, next.enabled, cx) {
            return false;
        }
        if next.enabled {
            self.after_source_enabled(&id, cx);
        }
        self.refresh_primary_local(cx);
        cx.notify();
        true
    }

    pub(super) fn delete_source_instance(&mut self, id: &SourceId, cx: &mut Context<Self>) {
        self.source_configs.retain(|source| source.id != id.0);
        self.persist_source_configs();
        let old_instance = self.registry.get(id);
        let _ = self.registry.deactivate(id);
        self.registry.unregister(id);
        if let Some(source) = old_instance {
            cx.spawn(async move |_, _| {
                source.shutdown().await;
            })
            .detach();
        }
        {
            let globals = cx.global_mut::<Globals>();
            globals.builders.remove(id);
            globals.local_sources.remove(id);
            globals.netease_sources.remove(id);
        }
        self.remove_source_tracks_from_library(id);
        if self.collection.as_ref().is_some_and(|c| &c.id.source == id) {
            self.collection = None;
            let old_cover = self.collection_cover.take();
            self.retire_image(old_cover);
            if self.view_tab == ViewTab::Collection {
                self.view_tab = ViewTab::Library;
            }
        }
        if matches!(
            &self.page,
            Some(MainPage::Source(source)) | Some(MainPage::EditSource(source))
                if source == id
        ) {
            self.page = Some(MainPage::ManageSources);
        }
        self.refresh_primary_local(cx);
        cx.notify();
    }

    pub(super) fn set_source_cookie(&mut self, id: &SourceId, cookie: Option<String>) {
        if let Some(config) = self.source_configs.iter_mut().find(|s| s.id == id.0)
            && let Some(netease) = config.netease.as_mut()
        {
            netease.cookie = cookie;
            self.persist_source_configs();
        }
    }

    pub(super) fn logout_netease_source(&mut self, id: &SourceId, cx: &mut Context<Self>) {
        let Some(source) = self.registry.get(id) else {
            return;
        };
        let source_id = id.clone();
        cx.spawn(async move |weak, cx| {
            if let Some(account) = source.account() {
                let _ = account.logout().await;
            }
            let _ = weak.update(cx, |app, cx| {
                app.set_source_cookie(&source_id, None);
                app.source_user_playlists.remove(&source_id);
                app.source_user_playlists_loading.remove(&source_id);
                app.clear_source_feeds(&source_id);
                if app.primary_netease_id().as_ref() == Some(&source_id) {
                    app.account_state = AccountState::LoggedOut;
                    let old_avatar = app.account_avatar.take();
                    app.retire_image(old_avatar);
                    app.user_playlists = Arc::new(Vec::new());
                }
                app.info_message = Some("已退出网易云账号".into());
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn refresh_source_account(&mut self, id: &SourceId, cx: &mut Context<Self>) {
        let Some(source) = self.registry.get(id) else {
            return;
        };
        let source_id = id.clone();
        cx.spawn(async move |weak, cx| {
            let result = match source.account() {
                Some(account) => account.refresh_account().await,
                None => return,
            };
            let _ = weak.update(cx, |app, cx| {
                match result {
                    Ok(Some(profile)) => {
                        app.info_message = Some(format!("已连接：{}", profile.nickname));
                        app.source_user_playlists.remove(&source_id);
                        app.clear_source_feeds(&source_id);
                        if app.primary_netease_id().as_ref() == Some(&source_id) {
                            app.account_state = AccountState::LoggedIn(profile);
                            app.load_account_avatar(cx);
                            app.load_user_playlists(cx);
                        }
                        app.load_source_user_playlists(&source_id, cx);
                        app.ensure_source_feeds(&source_id, cx);
                    }
                    Ok(None) => {
                        app.info_message = Some("账号未登录或 Cookie 已失效".into());
                        app.source_user_playlists.remove(&source_id);
                        app.clear_source_feeds(&source_id);
                        if app.primary_netease_id().as_ref() == Some(&source_id) {
                            app.account_state = AccountState::LoggedOut;
                            let old_avatar = app.account_avatar.take();
                            app.retire_image(old_avatar);
                            app.user_playlists = Arc::new(Vec::new());
                        }
                    }
                    Err(e) => {
                        app.status_message = Some(format!("账号校验失败：{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_new_source(&mut self, cx: &mut Context<Self>) {
        self.page = Some(MainPage::NewSource);
        cx.notify();
    }

    pub(super) fn open_source_edit(&mut self, id: SourceId, cx: &mut Context<Self>) {
        let Some(config) = self.source_config_by_id(&id.0).cloned() else {
            return;
        };
        let name = config.name.clone();
        let url = config
            .netease
            .as_ref()
            .map(|netease| netease.base_url.clone())
            .unwrap_or_default();
        self.source_edit_name_input
            .update(cx, |input, cx| input.set_content(name, cx));
        self.source_edit_url_input
            .update(cx, |input, cx| input.set_content(url, cx));
        self.source_edit_root_input
            .update(cx, |input, cx| input.set_content("", cx));
        self.page = Some(MainPage::EditSource(id));
        cx.notify();
    }

    pub(super) fn create_source_of_type(
        &mut self,
        source_type: &str,
        cx: &mut Context<Self>,
    ) -> Option<SourceId> {
        let config = self.new_source_config(source_type);
        let id = self.create_source_instance(config, cx)?;
        self.open_source_edit(id.clone(), cx);
        Some(id)
    }

    pub(super) fn apply_source_name(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(MainPage::EditSource(id)) = self.page.clone() else {
            return;
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(mut config) = self.source_config_by_id(&id.0).cloned() else {
            return;
        };
        if config.name == text {
            return;
        }
        config.name = text;
        self.replace_source_config(config, cx);
    }

    pub(super) fn apply_source_url(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(MainPage::EditSource(id)) = self.page.clone() else {
            return;
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(mut config) = self.source_config_by_id(&id.0).cloned() else {
            return;
        };
        let Some(netease) = config.netease.as_mut() else {
            return;
        };
        if netease.base_url == text {
            return;
        }
        netease.base_url = text;
        self.replace_source_config(config, cx);
    }

    pub(super) fn apply_source_add_root(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(MainPage::EditSource(id)) = self.page.clone() else {
            return;
        };
        let root = text.trim();
        if root.is_empty() {
            return;
        }
        let Some(mut config) = self.source_config_by_id(&id.0).cloned() else {
            return;
        };
        let Some(local) = config.local.as_mut() else {
            return;
        };
        let path = PathBuf::from(root);
        if local.roots.iter().any(|existing| existing == &path) {
            return;
        }
        local.roots.push(path);
        self.source_edit_root_input
            .update(cx, |input, cx| input.set_content("", cx));
        self.replace_source_config(config, cx);
    }

    pub(super) fn remove_source_root(
        &mut self,
        id: &SourceId,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(mut config) = self.source_config_by_id(&id.0).cloned() else {
            return;
        };
        let Some(local) = config.local.as_mut() else {
            return;
        };
        if index >= local.roots.len() || local.roots.len() <= 1 {
            return;
        }
        local.roots.remove(index);
        self.replace_source_config(config, cx);
    }

    fn remove_source_tracks_from_library(&mut self, id: &SourceId) {
        if !self.library.iter().any(|track| &track.id.source == id) {
            return;
        }
        let tracks: Vec<Track> = self
            .library
            .iter()
            .filter(|track| &track.id.source != id)
            .cloned()
            .collect();
        self.library = Arc::new(tracks);
        chordal_core::mem::set_gauge(Gauge::Library, estimate_library_bytes(&self.library));
        let mut cache = self.row_text_cache.borrow_mut();
        *cache = None;
    }

    /// 扫描单个本地实例，替换该实例在聚合库中的曲目（按源 ID 过滤）。
    pub(super) fn spawn_library_scan_source(
        &mut self,
        local: Arc<chordal_local::LocalSource>,
        cx: &mut Context<Self>,
    ) {
        self.library_scanning = true;
        let source_id = local.info().id.clone();
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let tracks = cx
                .background_executor()
                .spawn(async move {
                    let summary = local.scan_blocking();
                    (summary.tracks, local.all_tracks())
                })
                .await;
            let _ = weak.update(cx, |app, cx| {
                let mut merged: Vec<Track> = app
                    .library
                    .iter()
                    .filter(|track| track.id.source != source_id)
                    .cloned()
                    .collect();
                merged.extend(tracks.1);
                let deduped = dedupe_library(merged);
                chordal_core::mem::set_gauge(Gauge::Library, estimate_library_bytes(&deduped));
                app.library = Arc::new(deduped);
                app.library_scanning = false;
                app.row_text_cache.borrow_mut().take();
                tracing::info!(tracks = tracks.0, source = %source_id, "local source scan finished");
                if std::env::var_os("CHORDIAL_AUTOPLAY").is_some() && !app.library.is_empty() {
                    app.play_first(cx);
                    app.enter_immersive(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn spawn_library_scan_all(&mut self, cx: &mut Context<Self>) {
        let sources = self.enabled_local_sources(cx);
        if sources.is_empty() {
            self.library = Arc::new(Vec::new());
            self.library_scanning = false;
            cx.notify();
            return;
        }
        for (_, local) in sources {
            self.spawn_library_scan_source(local, cx);
        }
    }

    pub(super) fn open_source_page(&mut self, id: SourceId, cx: &mut Context<Self>) {
        // 切换实例时清掉旧的推荐详情，避免详情页挂在别的实例上。
        if self
            .source_feed_detail
            .as_ref()
            .is_some_and(|detail| &detail.source != &id)
        {
            self.source_feed_detail = None;
        }
        if self
            .source_config_by_id(&id.0)
            .is_some_and(|source| source.is_netease())
        {
            self.load_source_user_playlists(&id, cx);
            self.ensure_source_feeds(&id, cx);
        }
        self.page = Some(MainPage::Source(id));
        self.track_menu = None;
        cx.notify();
    }

    pub(super) fn open_source_manager(&mut self, cx: &mut Context<Self>) {
        self.page = Some(MainPage::ManageSources);
        cx.notify();
    }

    pub(super) fn open_playlist_page(
        &mut self,
        id: chordal_core::playlist::PlaylistId,
        cx: &mut Context<Self>,
    ) {
        if let Some(playlist) = self.playlists.get(&id) {
            let title = playlist.title.clone();
            let subscribed = playlist.kind.is_subscribed();
            self.source_playlist_rename_input
                .update(cx, |input, cx| input.set_content(title, cx));
            if subscribed {
                // 先渲染本地缓存，再后台拉一次远端。
                self.refresh_playlist_subscription(&id, cx);
            }
        }
        self.playlist_delete_confirm = None;
        self.page = Some(MainPage::Playlist(id));
        cx.notify();
    }

    pub(super) fn close_main_page(&mut self, cx: &mut Context<Self>) {
        self.page = None;
        cx.notify();
    }

    /// 新建音源的默认配置（管理页的“添加音源”）。
    pub(super) fn new_source_config(&self, source_type: &str) -> SourceInstanceConfig {
        let id = self.new_source_id(source_type);
        match source_type {
            SOURCE_TYPE_LOCAL => SourceInstanceConfig::local(
                id,
                "本地音乐",
                vec![dirs::audio_dir().unwrap_or_else(|| PathBuf::from("."))],
            ),
            SOURCE_TYPE_NETEASE => SourceInstanceConfig::netease(
                id,
                "网易云音乐",
                chordal_netease::NeteaseConfig::default(),
            ),
            _ => SourceInstanceConfig::local(id, "新音源", vec![]),
        }
    }

    fn new_source_id(&self, source_type: &str) -> String {
        next_source_id(
            source_type,
            self.source_configs.iter().map(|source| source.id.as_str()),
        )
    }
}

fn local_roots(config: &SourceInstanceConfig) -> Option<&[PathBuf]> {
    config.local.as_ref().map(|local| local.roots.as_slice())
}

/// 多本地实例聚合时按身份键去重；同曲保留音质分更高的一份。
fn dedupe_library(tracks: Vec<Track>) -> Vec<Track> {
    let mut merged: Vec<Track> = Vec::with_capacity(tracks.len());
    let mut seen: HashMap<String, usize> = HashMap::with_capacity(tracks.len());
    for track in tracks {
        let key = track.identity_key();
        match seen.get(&key).copied() {
            Some(index) => {
                if track.quality_score() > merged[index].quality_score() {
                    merged[index] = track;
                }
            }
            None => {
                seen.insert(key, merged.len());
                merged.push(track);
            }
        }
    }
    merged
}
