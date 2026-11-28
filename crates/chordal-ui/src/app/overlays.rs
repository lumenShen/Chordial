//! 抽屉 / 提示 / 空闲刷新等覆盖层行为（由 navigation.rs 拆分，行为不变）。

use super::*;
use crate::motion::PROGRESS_TOL;

impl ChordialApp {
    // ---------- 底部「更多」抽屉 ----------

    pub(super) const DRAWER_ANIM_MS: f32 = 240.0;

    pub(super) fn hide_drawer_now(&mut self) {
        self.drawer_open = false;
        self.motion.drawer = None;
        self.drawer_page = DrawerPage::Root;
    }

    /// 抽屉显隐弹簧：从当前进度与速度换向，快速开合不会先跳到另一端。
    fn retarget_drawer(&mut self, target: f32, now: Instant) {
        if let Some(spring) = self.motion.drawer.as_mut() {
            spring.retarget_with(target, spring.sample(now).velocity, now);
            return;
        }
        let from = 1.0 - target;
        self.motion.drawer = Some(self.motion_tuning.spring(
            Duration::from_secs_f32(Self::DRAWER_ANIM_MS / 1000.0),
            from,
            target,
            0.0,
            now,
            PROGRESS_TOL.0,
            PROGRESS_TOL.1,
        ));
    }

    pub(super) fn open_drawer(&mut self, cx: &mut Context<Self>) {
        if self.eq_open {
            return;
        }
        let now = Instant::now();
        self.drawer_open = true;
        self.drawer_page = DrawerPage::Root;
        self.retarget_drawer(1.0, now);
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    pub(super) fn close_drawer(&mut self, cx: &mut Context<Self>) {
        if !self.drawer_open {
            return;
        }
        self.drawer_open = false;
        self.retarget_drawer(0.0, Instant::now());
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    pub(super) fn drawer_visible(&self) -> bool {
        self.drawer_open || self.motion.drawer.is_some()
    }

    pub(super) fn drawer_progress(&self, now: Instant) -> f32 {
        self.motion
            .drawer
            .as_ref()
            .map(|spring| spring.value(now).clamp(0.0, 1.0))
            .unwrap_or(if self.drawer_open { 1.0 } else { 0.0 })
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

    pub(super) fn cycle_play_mode(&mut self, cx: &mut Context<Self>) {
        self.play_mode = self.play_mode.next();
        if self.play_mode != PlayMode::Shuffle {
            self.last_repeat_mode = self.play_mode;
        }
        self.sync_shuffle_plan();
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
        self.sync_shuffle_plan();
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
        self.sync_shuffle_plan();
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
