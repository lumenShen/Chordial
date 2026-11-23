//! 抽屉 / 提示 / 空闲刷新等覆盖层行为（由 navigation.rs 拆分，行为不变）。

use super::*;

impl ChordialApp {
    // ---------- 底部「更多」抽屉 ----------

    pub(super) const DRAWER_ANIM_MS: f32 = 240.0;

    pub(super) fn hide_drawer_now(&mut self) {
        self.drawer_open = false;
        self.drawer_at = None;
        self.drawer_page = DrawerPage::Root;
    }

    pub(super) fn open_drawer(&mut self, cx: &mut Context<Self>) {
        if self.eq_open {
            return;
        }
        self.drawer_open = true;
        self.drawer_page = DrawerPage::Root;
        self.drawer_at = Some(Instant::now());
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    pub(super) fn close_drawer(&mut self, cx: &mut Context<Self>) {
        if !self.drawer_open {
            return;
        }
        self.drawer_open = false;
        self.drawer_at = Some(Instant::now());
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    pub(super) fn drawer_animating(&self, now: Instant) -> bool {
        self.drawer_at.is_some_and(|at| {
            now.saturating_duration_since(at).as_secs_f32() * 1000.0 < Self::DRAWER_ANIM_MS
        })
    }

    pub(super) fn drawer_visible(&self) -> bool {
        self.drawer_open || self.drawer_at.is_some()
    }

    pub(super) fn drawer_progress(&self, now: Instant) -> f32 {
        let Some(at) = self.drawer_at else {
            return if self.drawer_open { 1.0 } else { 0.0 };
        };
        let t = (now.saturating_duration_since(at).as_secs_f32() * 1000.0 / Self::DRAWER_ANIM_MS)
            .clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        if self.drawer_open { eased } else { 1.0 - eased }
    }

    pub(crate) fn show_portrait_toast(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let started = Instant::now();
        self.portrait_toast = Some((text.into(), started));
        // Toast 是静态文本，不需要动画帧；用一次性定时器清理即可，
        // 避免为 1.8 秒的显示期持续 300Hz 重绘。
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1800))
                .await;
            let _ = weak.update(cx, |app, cx| {
                if app
                    .portrait_toast
                    .as_ref()
                    .is_some_and(|(_, timestamp)| *timestamp == started)
                {
                    app.portrait_toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// 3.2 秒无输入后隐藏标题 / 进度 / Dock；移动鼠标或点击即恢复。
    pub(super) fn ensure_portrait_idle_ticker(&mut self, cx: &mut Context<Self>) {
        if self.portrait_idle_ticker {
            return;
        }
        self.portrait_idle_ticker = true;
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(220))
                    .await;
                let alive = weak
                    .update(cx, |app, cx| {
                        if !app.immersive || app.portrait_chrome_hidden {
                            app.portrait_idle_ticker = false;
                            return false;
                        }
                        // 横向布局保持 AMLL 常驻控件，不做自动隐藏。
                        if !app.force_portrait && app.viewport_width >= app.viewport_height {
                            app.portrait_idle_ticker = false;
                            return false;
                        }
                        let idle = app.portrait_last_input.elapsed() >= Duration::from_millis(3200);
                        if idle && !app.drawer_open && app.portrait_transition.is_none() {
                            app.portrait_chrome_hidden = true;
                            app.portrait_chrome_at = Some(Instant::now());
                            app.portrait_idle_ticker = false;
                            app.ensure_ui_anim_ticker(cx);
                            cx.notify();
                            return false;
                        }
                        true
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn portrait_animating(&self) -> bool {
        if !self.immersive {
            return false;
        }
        let within = |t: Option<Instant>, ms: u64| {
            t.is_some_and(|t| t.elapsed() < Duration::from_millis(ms))
        };
        self.portrait_transition.is_some()
            || within(self.portrait_enter, 260)
            || within(self.portrait_chrome_at, 220)
    }

    pub(super) fn cycle_play_mode(&mut self, cx: &mut Context<Self>) {
        self.play_mode = self.play_mode.next();
        if self.play_mode != PlayMode::Shuffle {
            self.last_repeat_mode = self.play_mode;
        }
        let text = self.play_mode.full_label();
        self.show_portrait_toast(text, cx);
        cx.notify();
    }

    /// AMLL 播放列的随机按钮：独立开关，关闭时回到上一次的循环档。
    pub(super) fn toggle_amll_shuffle(&mut self) {
        if self.play_mode == PlayMode::Shuffle {
            self.play_mode = self.last_repeat_mode;
        } else {
            self.last_repeat_mode = self.play_mode;
            self.play_mode = PlayMode::Shuffle;
        }
    }

    /// AMLL 播放列的循环按钮：Off → All → One → Off。
    pub(super) fn cycle_amll_repeat(&mut self) {
        self.play_mode = match self.play_mode {
            PlayMode::RepeatAll => PlayMode::RepeatOne,
            PlayMode::RepeatOne => PlayMode::Sequential,
            // Shuffle / Sequential 一律先进入列表循环，并把 shuffle 关掉。
            PlayMode::Shuffle | PlayMode::Sequential => PlayMode::RepeatAll,
        };
        self.last_repeat_mode = self.play_mode;
    }

    pub(super) fn set_sleep_minutes(&mut self, minutes: Option<u32>, cx: &mut Context<Self>) {
        self.sleep_gen = self.sleep_gen.wrapping_add(1);
        let generation = self.sleep_gen;
        self.sleep_minutes = minutes;
        self.sleep_until = minutes.map(|m| Instant::now() + Duration::from_secs(m as u64 * 60));
        if let Some(minutes) = minutes {
            let weak = cx.entity().downgrade();
            cx.spawn(async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(minutes as u64 * 60))
                    .await;
                let _ = weak.update(cx, |app, cx| {
                    if app.sleep_gen != generation {
                        return;
                    }
                    app.sleep_until = None;
                    app.sleep_minutes = None;
                    app.player.pause();
                    app.player_status = app.player.status();
                    app.portrait_toast = Some(("睡眠定时结束，已暂停播放".into(), Instant::now()));
                    app.ensure_ui_anim_ticker(cx);
                    cx.notify();
                });
            })
            .detach();
            self.show_portrait_toast(format!("{minutes} 分钟后暂停播放"), cx);
        } else {
            self.show_portrait_toast("已取消睡眠定时", cx);
        }
        cx.notify();
    }
}
