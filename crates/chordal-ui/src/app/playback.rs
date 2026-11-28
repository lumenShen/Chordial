//! `ChordialApp` 的 playback 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

/// 私人 FM 剩余不足这个数量时提前拉取下一批，给网络请求留出缓冲。
const FM_REFILL_AHEAD: usize = 5;

impl ChordialApp {
    pub(super) fn playback_status_label(&self) -> &'static str {
        playback_state_label(&self.player_status.state)
    }

    pub(super) fn playback_action_label(&self) -> &'static str {
        playback_action_label(&self.player_status.state)
    }

    /// 解析播放句柄（可能跨源网络请求）后交给播放线程。
    pub(super) fn resolve_and_play(&mut self, cx: &mut Context<Self>) {
        self.resolve_and_play_impl(true, cx);
    }

    pub(super) fn resolve_and_play_impl(&mut self, load_extras: bool, cx: &mut Context<Self>) {
        let Some(pos) = self.queue_pos else { return };
        let Some(track) = self.queue.get(pos).cloned() else {
            return;
        };
        let registry = self.registry.clone();
        let source_id = track.id.source.clone();
        let track_id = track.id.clone();
        let title = track.title.clone();
        let preference = self.source_preference;

        // 显式下载优先：下载文件是“本地资产”，不占缓存配额，也不再请求网络。
        let downloaded = self.downloads.path_for(&track_id);
        // 源实例当前是否活跃。关闭/删除后不再解析远端，但可以从已启用的
        // 本地库按跨源身份键找替代副本。
        let source_available = self.registry.get(&source_id).is_some();
        let local_match = if matches!(track.origin, TrackOrigin::Remote { .. }) {
            self.enabled_local_sources(cx)
                .into_iter()
                .find_map(|(_, local)| {
                    local
                        .find_by_identity(&track)
                        .or_else(|| local.find_equivalent(&track))
                })
        } else if !source_available {
            // 本地源被关闭时：文件仍存在就直接播；否则去其它本地实例找同曲。
            match &track.origin {
                TrackOrigin::Local { path } if path.exists() => Some(track.clone()),
                _ => self
                    .enabled_local_sources(cx)
                    .into_iter()
                    .find_map(|(_, local)| local.find_by_identity(&track)),
            }
        } else {
            None
        };

        if !source_available && downloaded.is_none() && local_match.is_none() {
            let name = self
                .source_config_by_id(&source_id.0)
                .map(|source| source.name.clone())
                .unwrap_or_else(|| source_id.0.clone());
            self.status_message = Some(format!("「{name}」已关闭，且本地没有替代副本"));
            cx.notify();
            return;
        }

        self.play_gen = self.play_gen.wrapping_add(1);
        let play_gen = self.play_gen;
        self.status_message = None;
        self.info_message = None;
        self.current_track = Some(track.clone());
        if load_extras {
            self.load_track_extras(track.clone(), cx);
        }

        if let Some(path) = downloaded {
            self.player.play(StreamHandle::File(path), title, 0);
            self.info_message = Some("播放已下载文件（不占缓存配额）".into());
            cx.notify();
            return;
        }

        cx.spawn(async move |weak, cx| {
            let (remote, remote_err) = match registry.get(&source_id) {
                Some(source) => match source.resolve_stream(&track_id).await {
                    Ok(resolved) => (Some(resolved), None),
                    Err(e) => (None, Some(e.to_string())),
                },
                None => (None, Some(format!("源 {source_id} 未加载"))),
            };
            let remote_quality = remote.as_ref().and_then(|r| r.quality.clone());
            let remote_label = remote_quality
                .as_ref()
                .map(|q| q.label())
                .unwrap_or_else(|| "未知音质".into());
            let remote_handle = remote.map(|r| r.handle);

            let local_choice = local_match.and_then(|t| match t.origin.clone() {
                TrackOrigin::Local { path } => Some((t, path)),
                _ => None,
            });
            let local_score = local_choice
                .as_ref()
                .map(|(t, _)| t.quality_score())
                .unwrap_or(0);
            let remote_score = remote_quality.as_ref().map(|q| q.score()).unwrap_or(0);
            let use_local = match preference {
                SourcePreference::PreferLocal => local_choice.is_some(),
                SourcePreference::PreferOnline => false,
                SourcePreference::Auto => local_choice.is_some() && local_score > remote_score,
            };

            // 在线解析失败时自动回退本地同曲；否则按偏好/音质分选择。
            let chosen_local = if use_local || remote_handle.is_none() {
                local_choice.clone()
            } else {
                None
            };
            let local_label = local_choice
                .as_ref()
                .and_then(|(t, _)| t.quality.clone())
                .unwrap_or_else(|| "未知音质".into());

            let _ = weak.update(cx, |app, cx| {
                if app.play_gen != play_gen {
                    return;
                }
                if let Some((local_track, path)) = chosen_local {
                    app.player
                        .play(StreamHandle::File(path), local_track.title.clone(), 0);
                    app.current_track = Some(local_track.clone());
                    if remote_err.is_some() {
                        app.status_message =
                            Some(format!("在线解析失败，已改播本地「{}」", local_track.title));
                    } else if preference == SourcePreference::PreferLocal {
                        app.info_message =
                            Some(format!("本地优先：{} · {local_label}", local_track.title));
                    } else {
                        app.info_message =
                            Some(format!("本地音质更高：{local_label} > 在线 {remote_label}"));
                    }
                    if load_extras {
                        app.load_track_extras(local_track, cx);
                    }
                    cx.notify();
                    return;
                }
                match remote_handle {
                    Some(handle) => {
                        app.player.play(handle, title.clone(), 0);
                        if let Some(current) = app.current_track.as_mut() {
                            current.quality = Some(remote_label.clone());
                            current.quality_info = remote_quality.clone();
                        }
                        // 只在“有本地同曲但最终选了在线”时提示，避免打扰。
                        if local_choice.is_some() && preference == SourcePreference::Auto {
                            app.info_message = Some(format!(
                                "在线 {remote_label} · 本地同曲 {local_label}（按音质选择）"
                            ));
                        }
                        cx.notify();
                    }
                    None => {
                        app.status_message = Some(format!(
                            "无法播放「{title}」：{}",
                            remote_err.unwrap_or_else(|| "无可用播放地址".into())
                        ));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// 点击列表行：把当前可见列表设为队列并播放。
    pub(super) fn play_from_list(&mut self, list: QueueList, index: usize, cx: &mut Context<Self>) {
        let tracks: Arc<Vec<Track>> = match list {
            QueueList::Search => match &self.source_route {
                // 路由到单源时，"播放全部"与行点击都只在可见结果内流转。
                Some(route) => Arc::new(
                    self.search_tracks
                        .iter()
                        .filter(|t| &t.id.source == route)
                        .cloned()
                        .collect(),
                ),
                None => self.search_tracks.clone(),
            },
            QueueList::Library => self.library.clone(),
            QueueList::Queue => self.queue.clone(),
            QueueList::Collection => self
                .collection
                .as_ref()
                .map(|c| c.tracks.clone())
                .unwrap_or_else(|| Arc::new(Vec::new())),
        };
        if index >= tracks.len() {
            return;
        }
        self.set_queue(tracks, None);
        self.queue_pos = Some(index);
        self.status_message = None;
        self.resolve_and_play(cx);
        // 手机壳：选歌的最终目的是播放，直接回到播放页，避免用户还要
        // 再找一次返回入口。宽屏三栏同时可见，不需要这一步。
        self.show_phone_player(cx);
        cx.notify();
    }

    /// 从播放队列移除一项；不打断当前播放，只修正 queue_pos。
    pub(super) fn remove_queue_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.queue.len() {
            return;
        }
        let mut queue = self.queue.as_ref().clone();
        queue.remove(index);
        self.queue = Arc::new(queue);
        self.queue_pos = match self.queue_pos {
            Some(pos) if pos > index => Some(pos - 1),
            Some(pos) if pos == index => {
                if self.queue.is_empty() {
                    None
                } else {
                    Some(pos.min(self.queue.len() - 1))
                }
            }
            other => other,
        };
        self.shuffle_plan = None;
        cx.notify();
    }

    /// 私人 FM 接近末尾时向所追踪的音源再拉一批（同一时刻最多一个在途请求）。
    pub(super) fn maybe_refill_fm_queue(&mut self, cx: &mut Context<Self>) {
        let Some(pos) = self.queue_pos else { return };
        let Some(refill) = self.queue_refill.as_ref() else {
            return;
        };
        if refill.in_flight {
            return;
        }
        // 上一批全是重复曲目时冷却一段距离，避免尾部逐首重复请求。
        if refill
            .last_empty_at
            .is_some_and(|at| pos < at.saturating_add(FM_REFILL_AHEAD))
        {
            return;
        }
        let remaining = self.queue.len().saturating_sub(pos.saturating_add(1));
        if remaining >= FM_REFILL_AHEAD {
            return;
        }
        self.refill_fm_queue(cx);
    }

    /// 自动播完：单曲循环原地重播；随机模式按随机轮次推进；否则顺序下一首。
    pub(super) fn on_track_ended(&mut self, cx: &mut Context<Self>) {
        match self.play_mode {
            PlayMode::RepeatOne => {
                if self.queue_pos.is_some() {
                    self.resolve_and_play_impl(false, cx);
                }
            }
            PlayMode::Shuffle => self.play_shuffle_next(cx),
            PlayMode::RepeatAll | PlayMode::Sequential => self.play_next(cx),
        }
    }

    /// F5：没有当前曲目时从本地库第一首开始；否则等价于播放/暂停。
    pub fn play_first(&mut self, cx: &mut Context<Self>) {
        if self.queue_pos.is_none() && !self.library.is_empty() {
            self.play_from_list(QueueList::Library, 0, cx);
        } else {
            self.toggle_playback(cx);
        }
    }

    pub fn play_next(&mut self, cx: &mut Context<Self>) {
        // 随机模式必须与自动播完走同一套顺序；手动下一首原先会按队列
        // 顺序前进一步，导致“随机播放”看起来没有生效。
        if self.play_mode == PlayMode::Shuffle {
            self.play_shuffle_next(cx);
            return;
        }
        let Some(pos) = self.queue_pos else { return };
        if pos + 1 < self.queue.len() {
            self.queue_pos = Some(pos + 1);
            self.resolve_and_play(cx);
        } else if self.play_mode == PlayMode::RepeatAll && !self.queue.is_empty() {
            self.queue_pos = Some(0);
            self.resolve_and_play(cx);
        } else {
            self.stop_playback(cx);
        }
        self.maybe_refill_fm_queue(cx);
        cx.notify();
    }

    pub(super) fn play_prev(&mut self, cx: &mut Context<Self>) {
        if self.play_mode == PlayMode::Shuffle {
            self.play_shuffle_prev(cx);
            return;
        }
        let Some(pos) = self.queue_pos else { return };
        if pos == 0 && self.play_mode == PlayMode::RepeatAll && !self.queue.is_empty() {
            self.queue_pos = Some(self.queue.len() - 1);
            self.resolve_and_play(cx);
        } else {
            self.queue_pos = Some(pos.saturating_sub(1));
            self.resolve_and_play(cx);
        }
        cx.notify();
    }

    /// 保证随机计划对应当前队列与曲目；退出随机模式时丢弃计划。
    pub(super) fn sync_shuffle_plan(&mut self) {
        if self.play_mode != PlayMode::Shuffle {
            self.shuffle_plan = None;
            return;
        }
        let len = self.queue.len();
        let Some(current) = self.queue_pos.filter(|pos| *pos < len) else {
            self.shuffle_plan = None;
            return;
        };
        if self
            .shuffle_plan
            .as_ref()
            .is_some_and(|plan| plan.matches(len, current))
        {
            return;
        }
        self.shuffle_plan = Some(ShufflePlan::new(len, current));
    }

    /// 接下来 `count` 首的队列下标。随机模式与真实播放顺序同源，
    /// 顺序模式保持原有队列顺序。
    pub(super) fn upcoming_queue_indices(&self, count: usize) -> Vec<usize> {
        if self.play_mode == PlayMode::Shuffle {
            let len = self.queue.len();
            let Some(current) = self.queue_pos.filter(|pos| *pos < len) else {
                return Vec::new();
            };
            return self
                .shuffle_plan
                .as_ref()
                .filter(|plan| plan.matches(len, current))
                .map(|plan| plan.preview(count))
                .unwrap_or_default();
        }
        let start = self.queue_pos.map(|pos| pos + 1).unwrap_or(0);
        (start..self.queue.len()).take(count).collect()
    }

    /// 随机模式下的“下一首”：手动点击、键盘与自动播完共用。
    pub(super) fn play_shuffle_next(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.queue_pos else {
            return;
        };
        self.sync_shuffle_plan();
        let Some(next) = self
            .shuffle_plan
            .as_mut()
            .and_then(|plan| plan.advance(current))
        else {
            return;
        };
        self.queue_pos = Some(next);
        self.resolve_and_play(cx);
        self.maybe_refill_fm_queue(cx);
        cx.notify();
    }

    /// 随机模式下的“上一首”：按真实播放历史回退，而不是重新抽签。
    pub(super) fn play_shuffle_prev(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.queue_pos else {
            return;
        };
        self.sync_shuffle_plan();
        let Some(prev) = self
            .shuffle_plan
            .as_mut()
            .and_then(|plan| plan.retreat(current))
        else {
            return;
        };
        self.queue_pos = Some(prev);
        self.resolve_and_play(cx);
        cx.notify();
    }

    pub(super) fn stop_playback(&mut self, cx: &mut Context<Self>) {
        self.player.stop();
        self.queue_pos = None;
        self.shuffle_plan = None;
        self.play_gen = self.play_gen.wrapping_add(1);
        self.current_track = None;
        self.lyrics_status = None;
        if self.is_phone() {
            self.phone_page = PhonePage::Browse;
        }
        let old_cover = self.cover_image.take();
        self.retire_image(old_cover);
        mem::set_gauge(Gauge::Cover, 0);
        mem::set_gauge(Gauge::Lyrics, 0);
        self.lyric_view.update(cx, |view, _| {
            view.clear_lyrics();
            view.clear_album();
        });
        cx.notify();
    }

    // ---------- 账号源 / 集合页 ----------

    pub(super) fn netease_source(&self) -> Option<Arc<dyn MusicSource>> {
        self.primary_netease_id()
            .and_then(|id| self.registry.get(&id))
    }

    /// 登录浮层当前绑定的实例：优先用户点开的实例，否则首个支持账号的已启用实例。
    pub(super) fn login_source_id(&self) -> Option<SourceId> {
        let selected = self.login_source.as_ref().filter(|id| {
            self.source_config_by_id(&id.0)
                .is_some_and(|source| source.enabled)
                && self
                    .registry
                    .get(id)
                    .is_some_and(|source| source.account().is_some())
        });
        selected.cloned().or_else(|| {
            self.source_configs
                .iter()
                .filter(|source| source.enabled)
                .find_map(|config| {
                    let id = SourceId::new(config.id.clone());
                    self.registry
                        .get(&id)
                        .filter(|source| source.account().is_some())
                        .map(|_| id)
                })
        })
    }

    pub(super) fn login_source(&self) -> Option<Arc<dyn MusicSource>> {
        self.login_source_id().and_then(|id| self.registry.get(&id))
    }

    pub(super) fn save_cookie(&mut self, cookie: Option<String>) {
        // 旧入口没有实例上下文：写回首个网易云实例。
        let Some(id) = self.login_source_id().or_else(|| self.primary_netease_id()) else {
            let mut cfg = self.config.clone();
            cfg.netease.cookie = cookie;
            self.commit_config(&cfg);
            return;
        };
        self.set_source_cookie(&id, cookie);
    }
}

fn playback_state_label(state: &PlayerState) -> &'static str {
    match state {
        PlayerState::Playing => "正在播放",
        PlayerState::Paused => "已暂停",
        PlayerState::Loading => "加载中",
        PlayerState::Failed(_) => "播放失败 · 重试",
        PlayerState::Stopped => "等待播放",
    }
}

fn playback_action_label(state: &PlayerState) -> &'static str {
    match state {
        PlayerState::Playing => "暂停",
        PlayerState::Failed(_) => "重试",
        PlayerState::Loading => "加载中",
        _ => "播放",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_labels_distinguish_failure_and_loading() {
        assert_eq!(playback_state_label(&PlayerState::Playing), "正在播放");
        assert_eq!(playback_state_label(&PlayerState::Paused), "已暂停");
        assert_eq!(playback_state_label(&PlayerState::Loading), "加载中");
        assert_eq!(
            playback_state_label(&PlayerState::Failed("decoder".into())),
            "播放失败 · 重试"
        );
        assert_eq!(
            playback_action_label(&PlayerState::Failed("decoder".into())),
            "重试"
        );
        assert_eq!(playback_action_label(&PlayerState::Loading), "加载中");
    }
}
