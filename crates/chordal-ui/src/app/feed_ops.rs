//! 账号源推荐流：打开源页面时按需加载每日推荐 / 私人 FM / 推荐歌单 / 私人雷达。
//!
//! 快照缓存在 UI 层，源适配器只负责单次网络请求。四个区块独立加载、
//! 独立记录错误：某个接口（例如当前 api-enhanced 不提供的私人雷达）
//! 失败时，其余推荐照常显示。

use super::*;
use crate::playlists::now_ms;

/// 推荐快照的新鲜期；超过后再次打开页面会后台刷新。
const FEED_TTL_MS: u64 = 10 * 60 * 1000;

impl ChordialApp {
    /// 打开源页面时调用：没有快照或已过期才请求，避免每次进页都打网络。
    pub(super) fn ensure_source_feeds(&mut self, id: &SourceId, cx: &mut Context<Self>) {
        let needs = match self.source_feeds.get(id) {
            Some(state) => {
                !state.loading()
                    && (!state.has_content()
                        || now_ms().saturating_sub(state.fetched_at_ms) >= FEED_TTL_MS)
            }
            None => true,
        };
        if needs {
            self.load_source_feeds(id, false, cx);
        }
    }

    /// 「刷新推荐」按钮：无条件重拉所有区块。
    pub(super) fn refresh_source_feeds(&mut self, id: &SourceId, cx: &mut Context<Self>) {
        self.load_source_feeds(id, true, cx);
    }

    fn load_source_feeds(&mut self, id: &SourceId, force: bool, cx: &mut Context<Self>) {
        let state = self.source_feeds.entry(id.clone()).or_default();
        if state.loading() {
            return;
        }
        let Some(source) = self.registry.get(id) else {
            let message = SharedString::from("音源未加载，无法获取推荐");
            state.daily_error = Some(message.clone());
            state.fm_error = Some(message.clone());
            state.recommended_error = Some(message.clone());
            state.radar_error = Some(message);
            cx.notify();
            return;
        };
        if source.account().is_none() {
            return;
        }

        let load_daily = force || state.daily.is_empty();
        let load_fm = force || state.fm.is_empty();
        let load_recommended = force || state.recommended.is_empty();
        let load_radar = force || state.radar.is_empty();
        if !(load_daily || load_fm || load_recommended || load_radar) {
            return;
        }
        state.loading_daily = load_daily;
        state.loading_fm = load_fm;
        state.loading_recommended = load_recommended;
        state.loading_radar = load_radar;
        if load_daily {
            state.daily_error = None;
        }
        if load_fm {
            state.fm_error = None;
        }
        if load_recommended {
            state.recommended_error = None;
        }
        if load_radar {
            state.radar_error = None;
        }

        let source_id = id.clone();
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            if load_daily {
                let result = account.daily_recommendations().await;
                let _ = weak.update(cx, |app, cx| {
                    if let Some(state) = app.source_feeds.get_mut(&source_id) {
                        match result {
                            Ok(tracks) => {
                                state.daily = Arc::new(tracks);
                                state.daily_error = None;
                            }
                            Err(e) => {
                                state.daily_error =
                                    Some(SharedString::from(format!("今日推荐加载失败：{e}")));
                            }
                        }
                        state.loading_daily = false;
                    }
                    cx.notify();
                });
            }
            if load_fm {
                let result = account.personal_fm().await;
                let _ = weak.update(cx, |app, cx| {
                    if let Some(state) = app.source_feeds.get_mut(&source_id) {
                        match result {
                            Ok(tracks) => {
                                state.fm = Arc::new(tracks);
                                state.fm_error = None;
                            }
                            Err(e) => {
                                state.fm_error =
                                    Some(SharedString::from(format!("私人 FM 加载失败：{e}")));
                            }
                        }
                        state.loading_fm = false;
                    }
                    cx.notify();
                });
            }
            if load_recommended {
                let result = account.recommended_playlists().await;
                let _ = weak.update(cx, |app, cx| {
                    if let Some(state) = app.source_feeds.get_mut(&source_id) {
                        match result {
                            Ok(playlists) => {
                                state.recommended = playlists;
                                state.recommended_error = None;
                            }
                            Err(e) => {
                                state.recommended_error =
                                    Some(SharedString::from(format!("推荐歌单加载失败：{e}")));
                            }
                        }
                        state.loading_recommended = false;
                    }
                    cx.notify();
                });
            }
            if load_radar {
                let result = account.private_radar().await;
                let _ = weak.update(cx, |app, cx| {
                    if let Some(state) = app.source_feeds.get_mut(&source_id) {
                        match result {
                            Ok(playlists) => {
                                state.radar = playlists;
                                state.radar_error = None;
                            }
                            Err(e) => {
                                state.radar_error =
                                    Some(SharedString::from(format!("私人雷达不可用：{e}")));
                            }
                        }
                        state.loading_radar = false;
                    }
                    cx.notify();
                });
            }
            let _ = weak.update(cx, |app, cx| {
                if let Some(state) = app.source_feeds.get_mut(&source_id)
                    && !state.loading()
                {
                    state.fetched_at_ms = now_ms();
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 私人 FM 续接：向队列所追踪的音源再拉一批，同一时刻最多一个在途请求。
    pub(super) fn refill_fm_queue(&mut self, cx: &mut Context<Self>) {
        let Some(refill) = self.queue_refill.as_ref() else {
            return;
        };
        if refill.in_flight {
            return;
        }
        let source_id = refill.source.clone();
        let generation = self.queue_gen;

        let fail = |app: &mut Self, message: String, cx: &mut Context<Self>| {
            if let Some(refill) = app.queue_refill.as_mut() {
                refill.in_flight = false;
            }
            app.status_message = Some(message);
            cx.notify();
        };

        let Some(source) = self.registry.get(&source_id) else {
            fail(self, "私人 FM 续接失败：音源未加载".into(), cx);
            return;
        };
        if source.account().is_none() {
            fail(self, "私人 FM 续接失败：音源不支持账号推荐".into(), cx);
            return;
        }
        if let Some(refill) = self.queue_refill.as_mut() {
            refill.in_flight = true;
        }

        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            let result = account.personal_fm().await;
            let _ = weak.update(cx, |app, cx| {
                app.finish_fm_refill(generation, &source_id, result, cx);
            });
        })
        .detach();
    }

    /// 续接结果落盘：代际或来源不匹配时丢弃，避免写回已被替换的队列。
    fn finish_fm_refill(
        &mut self,
        generation: u64,
        source: &SourceId,
        result: chordal_core::CoreResult<Vec<Track>>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.queue_gen {
            return;
        }
        let Some(refill) = self.queue_refill.as_mut() else {
            return;
        };
        if &refill.source != source {
            return;
        }
        refill.in_flight = false;

        match result {
            Ok(incoming) => {
                let mut queue = self.queue.as_ref().clone();
                let added = Self::append_unique_tracks(&mut queue, incoming.clone());
                if added > 0 {
                    if let Some(refill) = self.queue_refill.as_mut() {
                        refill.last_empty_at = None;
                    }
                    self.queue = Arc::new(queue);
                    self.shuffle_plan = None;
                    if let Some(state) = self.source_feeds.get_mut(source) {
                        Self::append_unique_tracks(Arc::make_mut(&mut state.fm), incoming);
                        state.fm_error = None;
                    }
                    cx.notify();
                } else if let Some(refill) = self.queue_refill.as_mut() {
                    refill.last_empty_at = Some(self.queue_pos.unwrap_or(0));
                }
            }
            Err(e) => {
                // 续接失败是临时性的：走侧栏状态条提示，不写 fm_error，
                // 否则详情页会把已经加载好的整个列表替换成错误块。
                self.status_message = Some(format!("私人 FM 续接失败：{e}"));
                cx.notify();
            }
        }
    }

    /// 账号切换后清空该实例的推荐快照，下次进页重新拉取。
    pub(super) fn clear_source_feeds(&mut self, id: &SourceId) {
        self.source_feeds.remove(id);
        if self
            .source_feed_detail
            .as_ref()
            .is_some_and(|detail| &detail.source == id)
        {
            self.source_feed_detail = None;
        }
    }

    /// 点击推荐入口卡片：在当前源页面内展开对应详情。
    pub(super) fn open_feed_detail(
        &mut self,
        source: SourceId,
        kind: SourceFeedKind,
        cx: &mut Context<Self>,
    ) {
        self.source_feed_detail = Some(SourceFeedDetail { source, kind });
        cx.notify();
    }

    pub(super) fn close_feed_detail(&mut self, cx: &mut Context<Self>) {
        if self.source_feed_detail.take().is_some() {
            cx.notify();
        }
    }
}
