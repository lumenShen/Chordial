//! `ChordialApp` 的 animations 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;
use crate::motion::{CubicBezier, MotionTween, PROGRESS_TOL, scaled_duration};

fn cover_scale_tween(from: f32, playing: bool, now: Instant, duration: f32) -> MotionTween {
    let (base, curve) = if playing {
        (
            Duration::from_millis(500),
            CubicBezier::new(0.3, 0.2, 0.2, 1.4),
        )
    } else {
        (
            Duration::from_millis(600),
            CubicBezier::new(0.4, 0.2, 0.1, 1.0),
        )
    };
    MotionTween::new(
        from,
        if playing { 1.0 } else { 0.75 },
        scaled_duration(base, duration),
        curve,
        now,
    )
}

impl ChordialApp {
    pub(super) fn anim_progress(start: Option<Instant>, duration_ms: f32) -> f32 {
        match start {
            Some(t) => (t.elapsed().as_secs_f32() * 1000.0 / duration_ms).clamp(0.0, 1.0),
            None => 1.0,
        }
    }

    pub(super) fn latest_anim(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
        match (a, b) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, None) => a,
            (None, b) => b,
        }
    }

    /// AMLL Cover：暂停时缩到 0.75，播放时回到 1。
    pub(super) fn cover_scale_target(&self) -> f32 {
        if self.player_status.state == PlayerState::Playing {
            1.0
        } else {
            0.75
        }
    }

    pub(super) fn cover_scale_now(&self, now: Instant) -> f32 {
        self.motion
            .cover
            .as_ref()
            .map(|tween| tween.value(now))
            .unwrap_or_else(|| self.cover_scale_target())
    }

    /// 播放状态变化时启动封面缩放；`from` 必须在写入新状态之前采样，
    /// 作为没有在途动画时的起点。已有在途动画时从当前可见值重新开始 CSS 补间。
    pub(super) fn start_cover_scale_anim(&mut self, from: f32, playing: bool) {
        let target = if playing { 1.0 } else { 0.75 };
        let now = Instant::now();
        if let Some(tween) = self.motion.cover.as_mut() {
            let (base, curve) = if playing {
                (
                    Duration::from_millis(500),
                    CubicBezier::new(0.3, 0.2, 0.2, 1.4),
                )
            } else {
                (
                    Duration::from_millis(600),
                    CubicBezier::new(0.4, 0.2, 0.1, 1.0),
                )
            };
            tween.retarget(
                target,
                scaled_duration(base, self.motion_tuning.duration),
                curve,
                now,
            );
            return;
        }
        if (from - target).abs() < 0.001 {
            self.motion.cover = None;
            return;
        }
        self.motion.cover = Some(cover_scale_tween(
            from,
            playing,
            now,
            self.motion_tuning.duration,
        ));
    }

    /// 沉浸页顶部把手的展开进度：`1` = 文案胶囊，`0` = 原本的 50×8 小白条。
    ///
    /// 进入沉浸页后展开成「拖动/ESC 退出」文案展示 5s，随后用 320ms
    /// 收回成原来的小白条。
    pub(super) fn immersive_hint_expand(&self, now: Instant) -> f32 {
        self.motion
            .hint
            .as_ref()
            .map(|spring| spring.value(now).clamp(0.0, 1.0))
            .unwrap_or(0.0)
    }

    /// 提示的 5s 静态展示期不请求动画帧，只有淡出阶段需要逐帧重绘。
    pub(super) fn immersive_hint_fading(&self, now: Instant) -> bool {
        self.motion.hint.is_some()
            && self.immersive_hint_at.is_some_and(|start| {
                now.saturating_duration_since(start).as_secs_f32() * 1000.0
                    >= Self::IMMERSIVE_HINT_HOLD_MS
            })
    }

    /// 闲置收起的显隐进度：`1` = 完整控件，`0` = 完全隐藏。
    /// 起点/终点之间的过渡交给弹簧，打断闲置判定时保留当前速度与位置。
    pub(super) fn stage_chrome_visibility(&self, now: Instant) -> f32 {
        self.motion
            .chrome
            .as_ref()
            .map(|spring| spring.value(now).clamp(0.0, 1.0))
            .unwrap_or(if self.stage_chrome_hidden { 0.0 } else { 1.0 })
    }

    /// 同一帧的顶部动作与底部控件必须共用这份视觉参数。
    pub(super) fn stage_chrome_visual(&self, now: Instant) -> StageChromeVisual {
        StageChromeVisual::from_progress(self.stage_chrome_visibility(now))
    }

    /// 闲置收起后的整体居中位移。
    ///
    /// `group_top` / `group_bottom` 是完整布局里保留下来的可见组（顶部把手
    /// 到进度条底边）。位移只做刚体平移，内部间距保持设计值；显隐弹簧
    /// 同时驱动滤镜与位移，展开/收起时位置与透明度连续。
    pub(super) fn stage_idle_shift(
        &self,
        now: Instant,
        height: f32,
        group_top: f32,
        group_bottom: f32,
    ) -> f32 {
        let idle = 1.0 - self.stage_chrome_visibility(now);
        crate::layout_morph::idle_chrome_shift(height, group_top, group_bottom) * idle
    }

    pub(super) fn stage_chrome_fading(&self, now: Instant) -> bool {
        self.motion.chrome.as_ref().is_some_and(|s| s.active(now))
    }

    /// 显隐弹簧：从当前值（或逻辑默认值）换向，速度连续。
    fn retarget_chrome(&mut self, hidden: bool, now: Instant) {
        let target = if hidden { 0.0 } else { 1.0 };
        if let Some(spring) = self.motion.chrome.as_mut() {
            spring.retarget_with(target, spring.sample(now).velocity, now);
            return;
        }
        let from = if hidden { 1.0 } else { 0.0 };
        self.motion.chrome = Some(self.motion_tuning.spring(
            Duration::from_secs_f32(Self::STAGE_CHROME_FADE_MS / 1000.0),
            from,
            target,
            0.0,
            now,
            PROGRESS_TOL.0,
            PROGRESS_TOL.1,
        ));
    }

    /// 只有歌词模式与「仅显示主内容」允许闲置隐藏；弹窗/抽屉/手势期间不隐藏。
    fn stage_chrome_idle_eligible(&self, cx: &App) -> bool {
        self.immersive
            && self.portrait_page == PortraitPage::Art
            && (self.stage_view == StageView::MainOnly || self.immersive_lyrics_visible(cx))
            && !self.drawer_open
            && !self.eq_open
            && !self.quality_dialog_open
            && !self.stage_view_menu_open
            && self.sheet_drag.is_none()
            && !self.immersive_expanding(Instant::now())
    }

    /// 指针/键盘交互后立刻恢复控件；闲置判定只依赖时间戳，由 200ms
    /// 播放状态轮询调用 `tick_stage_chrome` 触发，不另开定时循环。
    pub(super) fn note_stage_interaction(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.stage_last_input_at = now;
        if self.stage_chrome_hidden {
            self.stage_chrome_hidden = false;
            self.stage_chrome_at = now;
            self.retarget_chrome(false, now);
            self.ensure_ui_anim_ticker(cx);
            cx.notify();
        }
    }

    /// 只有指针坐标真实变化才算交互；歌词滚动或 hover 子树变化可能产生
    /// 同坐标 mousemove，不能因此让闲置隐藏永远无法触发。
    pub(super) fn note_stage_pointer(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        if self
            .stage_last_pointer
            .is_some_and(|(last_x, last_y)| (last_x - x).abs() < 0.5 && (last_y - y).abs() < 0.5)
        {
            return;
        }
        self.stage_last_pointer = Some((x, y));
        self.note_stage_interaction(cx);
    }

    /// 复用播放状态轮询检查 5s 闲置；返回是否需要重绘。
    pub(super) fn tick_stage_chrome(&mut self, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let idle_ms = now
            .saturating_duration_since(self.stage_last_input_at)
            .as_secs_f32()
            * 1000.0;
        let should_hide = self.stage_chrome_idle_eligible(cx) && idle_ms >= Self::STAGE_IDLE_MS;
        if should_hide == self.stage_chrome_hidden {
            return false;
        }
        self.stage_chrome_hidden = should_hide;
        self.stage_chrome_at = now;
        self.retarget_chrome(should_hide, now);
        self.ensure_ui_anim_ticker(cx);
        true
    }

    pub(super) fn ui_anim_active(&self) -> bool {
        if self.bench_anim {
            return true;
        }
        let running = |start: Option<Instant>, ms: f32| {
            start.is_some_and(|t| t.elapsed().as_secs_f32() * 1000.0 < ms)
        };
        running(self.ui_anim_enter, Self::UI_ANIM_PRESS_MS)
            || running(self.ui_anim_play, Self::UI_ANIM_PRESS_MS)
            || running(self.ui_anim_shuffle, Self::UI_ANIM_PRESS_MS)
            || running(self.ui_anim_repeat, Self::UI_ANIM_PRESS_MS)
            || self
                .ui_anim_volume
                .is_some_and(|(t, _)| t.elapsed().as_secs_f32() * 1000.0 < Self::UI_ANIM_SPEAKER_MS)
            || self.immersive_hint_fading(Instant::now())
            || self.stage_chrome_fading(Instant::now())
            || self.motion.any_active(Instant::now())
            || self
                .info_marquee
                .is_some_and(|marquee| marquee.active(Instant::now()))
            || self.portrait_transition.is_some()
            || self
                .sheet_drag
                .as_ref()
                .is_some_and(|drag| drag.animating(Instant::now()))
            || self.phone_sheet_animating(Instant::now())
            || self
                .phone_edge_swipe
                .as_ref()
                .is_some_and(|swipe| swipe.animating(Instant::now()))
    }

    /// 安排一次重绘。帧节奏由 `Render` 中的 `request_animation_frame` 驱动：
    /// 只要有动画未结束，就跟随显示器刷新率（300Hz 屏即 300fps），动画
    /// 结束后自动停止。这里不再维护固定 16ms 定时泵。
    pub(super) fn ensure_ui_anim_ticker(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    /// 在渲染开始前清理已结束的过渡状态，避免动画结束后仍请求下一帧。
    pub(super) fn cleanup_finished_animations(&mut self, now: Instant, cx: &mut Context<Self>) {
        if self
            .portrait_transition
            .is_some_and(|transition| transition.done(now))
        {
            self.portrait_transition = None;
        }
        if self
            .info_marquee
            .is_some_and(|marquee| !marquee.active(now))
        {
            self.info_marquee = None;
        }
        // 统一弹簧管线：一次性清理所有收敛通道，并把完成事件变成副作用。
        let events = self.motion.advance(now);
        if events.immersive_exit_done
            && self.stage_request.is_none()
            && self
                .motion
                .stage
                .as_ref()
                .is_none_or(|spring| !spring.active(now))
        {
            // 收起弹簧与共享元素都落点后才真正卸载沉浸子树。
            self.finish_immersive_exit(cx);
        }
        if let Some(target) = events.phone_sheet_target
            && target < 0.5
            && self.phone_page == PhonePage::Player
        {
            self.phone_page = PhonePage::Browse;
        }
        // 顶部把手提示展示完毕：清掉起点，回到静态小白条。
        if self.immersive_hint_at.is_some_and(|t| {
            now.saturating_duration_since(t).as_secs_f32() * 1000.0
                >= Self::IMMERSIVE_HINT_HOLD_MS + Self::IMMERSIVE_HINT_FADE_MS
        }) {
            self.immersive_hint_at = None;
        }
        // 下拉关闭：回弹结束直接收手；关闭动画结束才真正退出沉浸层。
        let drag_outcome = self.sheet_drag.as_ref().and_then(|drag| drag.settled(now));
        match drag_outcome {
            Some(SheetDragOutcome::Returned) => self.sheet_drag = None,
            Some(SheetDragOutcome::Dismissed) => {
                // 下拉手势已经把卡片带到 Dock 位置，无需再跑一遍收起弹簧。
                self.sheet_drag = None;
                self.finish_immersive_exit(cx);
            }
            None => {}
        }
        // 手机壳 mini player ⇄ 播放页：手势结束后才切页，避免中途把
        // sheet 的 `lyric_view` 背景交还给曲库页；点击展开/收起交给 `motion.phone_sheet`。
        let sheet_outcome = self.phone_sheet.as_ref().and_then(|drag| drag.settled(now));
        match sheet_outcome {
            Some(SheetPage::Full) => {
                self.phone_sheet = None;
                self.phone_page = PhonePage::Player;
                self.motion.phone_sheet = None;
            }
            Some(SheetPage::Mini) => {
                self.phone_sheet = None;
                if self.phone_page == PhonePage::Player {
                    self.phone_page = PhonePage::Browse;
                }
            }
            None => {}
        }
        // 封面 ⇄ 歌词横滑：提交后在动画落点切换页面。
        let swipe_outcome = self
            .phone_edge_swipe
            .as_ref()
            .and_then(|swipe| swipe.settled(now));
        match swipe_outcome {
            Some(SwipeOutcome::Committed(crate::phone_gesture::SwipeDirection::Forward)) => {
                self.phone_edge_swipe = None;
                self.phone_swipe_suppress_click = Some(now);
                self.open_lyrics_immersive(cx);
            }
            Some(SwipeOutcome::Committed(crate::phone_gesture::SwipeDirection::Back)) => {
                self.phone_edge_swipe = None;
                self.phone_page = PhonePage::Player;
                self.motion.phone_sheet = Some(self.motion_tuning.spring(
                    Duration::from_millis(200),
                    0.0,
                    1.0,
                    0.0,
                    now,
                    PROGRESS_TOL.0,
                    PROGRESS_TOL.1,
                ));
                self.ensure_ui_anim_ticker(cx);
                self.phone_swipe_suppress_click = Some(now);
            }
            Some(SwipeOutcome::Returned) => self.phone_edge_swipe = None,
            None => {}
        }
    }

    /// 音量滑杆输入：跨越 0 / 1 边界时触发 AMLL 的喇叭弹跳。
    pub(super) fn note_volume_input(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(0.0, 1.0);
        let prev = self.last_volume_input;
        self.last_volume_input = value;
        let side = if value <= 0.0001 && prev > 0.0001 {
            Some(false)
        } else if value >= 0.9999 && prev < 0.9999 {
            Some(true)
        } else {
            None
        };
        if let Some(high) = side {
            self.ui_anim_volume = Some((Instant::now(), high));
            self.ensure_ui_anim_ticker(cx);
            cx.notify();
        }
    }

    // ---------- 沉浸页下拉关闭 ----------
    //
    // 顶部拖拽条是统一的 sheet 把手：按下开始记手，向下拖动时整页跟手
    // 下移并露出主面板，松手后按位移/速度回弹或退出沉浸层（参考视频的
    // 播放页下拉关闭手感）。

    pub(super) fn begin_sheet_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        let now = Instant::now();
        if !self.immersive
            || self.eq_open
            || self.sheet_drag.is_some()
            || self.motion.dock_collapsing
            || self.immersive_expanding(now)
        {
            return;
        }
        self.sheet_drag = Some(SheetDrag::begin(y, now));
        cx.notify();
    }

    pub(super) fn update_sheet_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        if let Some(drag) = &mut self.sheet_drag {
            if drag.update(y, Instant::now()) {
                cx.notify();
            }
        }
    }

    pub(super) fn release_sheet_drag(&mut self, y: f32, height: f32, cx: &mut Context<Self>) {
        let now = Instant::now();
        let dock_y = self.current_dock_rect().y;
        if let Some(drag) = &mut self.sheet_drag {
            drag.release(y, height, dock_y, now, self.motion_tuning);
            if !drag.animating(now) && drag.settled(now).is_none() {
                self.sheet_drag = None;
            }
        }
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    pub(super) fn current_dock_rect(&self) -> StageRect {
        let mode = self.source_stage_mode();
        if mode == StageMode::PhonePlayer {
            return StageRect {
                x: 0.0,
                y: 0.0,
                w: self.viewport_width,
                h: self.viewport_height,
            };
        }
        self.stage_measurements
            .cached_card_current(mode, self.viewport_width, self.viewport_height)
            .unwrap_or_else(|| match mode {
                StageMode::MiniPlayer => {
                    crate::layout_morph::mini_player_rect(self.viewport_width, self.viewport_height)
                }
                _ => dock_rect(self.viewport_width, self.viewport_height),
            })
    }

    /// Dock→全屏共享布局的当前进度：`1.0` = 页面仍完全贴在 Dock 上，
    /// `0.0` = 已铺满整个窗口。展开动画与下拉手势共用这一条通道。
    pub(super) fn immersive_dock_progress(&self, now: Instant, _height: f32) -> f32 {
        // 展开与收起共用同一条 dock 弹簧，方向只由 `dock_collapsing` 标记区分，
        // 因此退出第一帧不会先弹回满屏。
        if let Some(spring) = self
            .motion
            .stage
            .as_ref()
            .filter(|spring| spring.active(now) && spring.touches_source())
        {
            return spring.source_progress(now);
        }
        if let Some(spring) = self.motion.dock.as_ref() {
            return spring.value(now).clamp(0.0, 1.0);
        }
        if let Some(drag) = self.sheet_drag.as_ref() {
            if drag.engaged() || drag.animating(now) {
                let dock_y = self.current_dock_rect().y.max(1.0);
                return (drag.offset(now) / dock_y).clamp(0.0, 1.0);
            }
        }
        if self.motion.dock_collapsing {
            return 1.0;
        }
        0.0
    }

    pub(super) fn immersive_expanding(&self, now: Instant) -> bool {
        self.motion.dock.as_ref().is_some_and(|s| s.active(now))
    }

    pub(super) fn stage_measurement_mode(&self, cx: &App) -> StageMode {
        if self.is_phone() && (!self.immersive || self.motion.dock_collapsing) {
            self.source_stage_mode()
        } else {
            self.immersive_stage_mode(cx, self.viewport_width, self.viewport_height)
        }
    }

    /// 当前沉浸页的共享元素目标模态（与页面实际布局选择一致）。
    pub(super) fn immersive_stage_mode(&self, cx: &App, width: f32, height: f32) -> StageMode {
        if !self.immersive {
            return self.source_stage_mode();
        }
        if self.portrait_page == PortraitPage::Queue {
            StageMode::CompactQueue
        } else if self.stage_split_active(cx, width, height) {
            StageMode::WideSplit
        } else if self.compact_lyrics_visible(cx) {
            StageMode::CompactLyrics
        } else {
            StageMode::CompactArt
        }
    }

    /// 请求退出沉浸层：先播完反向收起动画，落点与 Dock 重合后才真正卸载。
    ///
    /// 从当前 `p` 起弹簧而不是固定从 0 起：展开没结束就按 Esc、或连续切换时，
    /// 卡片不会先跳回满屏再收起。
    pub(super) fn exit_immersive(&mut self, cx: &mut Context<Self>) {
        if !self.immersive || self.motion.dock_collapsing {
            return;
        }
        let now = Instant::now();
        // 采样当前位置与速度：展开没结束就退出时也不会顿一下再反向。
        let (p, velocity) = self
            .motion
            .dock
            .as_ref()
            .map(|s| {
                let sample = s.sample(now);
                (sample.value, sample.velocity)
            })
            .unwrap_or((0.0, 0.0));
        let p = p.clamp(0.0, 1.0);
        self.sheet_drag = None;
        self.motion.dock = Some(self.motion_tuning.spring(
            Duration::from_secs_f32(Self::COLLAPSE_MS / 1000.0),
            p,
            1.0,
            velocity,
            now,
            PROGRESS_TOL.0,
            PROGRESS_TOL.1,
        ));
        self.motion.dock_collapsing = true;
        self.portrait_transition = None;
        let mode = self.immersive_stage_mode(cx, self.viewport_width, self.viewport_height);
        self.request_stage_to(self.source_stage_mode(), mode, now, cx);
        // 收起期间不再展示顶部提示胶囊，让它随控件一起淡出。
        self.immersive_hint_at = None;
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    /// 真正卸载沉浸子树并恢复主面板样式；只由收起动画结束或下拉关闭触发。
    pub(super) fn finish_immersive_exit(&mut self, cx: &mut Context<Self>) {
        if !self.immersive {
            return;
        }
        self.immersive = false;
        self.sheet_drag = None;
        self.motion.dock = None;
        self.motion.dock_collapsing = false;
        self.motion.hint = None;
        self.immersive_hint_at = None;
        self.force_portrait = false;
        self.portrait_page = PortraitPage::Art;
        self.portrait_transition = None;
        self.stage_view_menu_open = false;
        self.stage_chrome_hidden = false;
        self.stage_chrome_at = Instant::now();
        self.motion.chrome = None;
        self.stage_last_input_at = Instant::now();
        self.stage_last_pointer = None;
        self.hide_drawer_now();
        self.wide_lyrics_hidden = false;
        self.compact_lyrics_open = false;
        self.motion.stage = None;
        self.stage_request = None;
        self.motion.portrait = None;
        self.motion.eq_enter = None;
        let theme = Theme::get(cx).clone();
        self.lyric_view.update(cx, |view, _| {
            view.set_text_visible(true);
            view.set_click_seek_enabled(true);
            // 非沉浸页面按自身容器尺寸布局；纯背景层不预热不可见歌词。
            view.set_layout_when_hidden(false);
            view.set_background_visible(true);
            view.set_background_bounds(None);
            view.set_colors(LyricRenderColors {
                text: theme.text,
                translation: theme.text,
                overlay: theme.overlay,
                interlude_dot: theme.text,
                fallback_background: theme.bg,
                glow: theme.playing,
            });
            view.set_text_style(compact_lyric_style(&theme, self.lyric_sweep_fade_em));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_play_uses_amll_500ms_curve_and_overshoots() {
        let now = Instant::now();
        let tween = cover_scale_tween(0.75, true, now, 1.0);
        assert_eq!(tween.value(now), 0.75);
        let quarter = tween.value(now + Duration::from_millis(125));
        assert!(quarter > 0.75 && quarter < 1.0, "quarter={quarter}");
        let peak = (0..=500)
            .map(|ms| tween.value(now + Duration::from_millis(ms)))
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(peak > 1.0, "play curve should overshoot, got {peak}");
        assert!(!tween.done(now + Duration::from_millis(499)));
        assert_eq!(tween.value(now + Duration::from_millis(500)), 1.0);
        assert!(tween.done(now + Duration::from_millis(500)));
    }

    #[test]
    fn cover_pause_uses_amll_600ms_curve() {
        let now = Instant::now();
        let tween = cover_scale_tween(1.0, false, now, 1.0);
        let half = tween.value(now + Duration::from_millis(300));
        assert!(half > 0.75 && half < 1.0, "half={half}");
        assert!(!tween.done(now + Duration::from_millis(599)));
        assert_eq!(tween.value(now + Duration::from_millis(600)), 0.75);
    }

    #[test]
    fn interrupted_cover_tween_restarts_from_visible_value() {
        let now = Instant::now();
        let mut tween = cover_scale_tween(0.75, true, now, 1.0);
        let mid = now + Duration::from_millis(200);
        let visible = tween.value(mid);
        tween.retarget(
            0.75,
            Duration::from_millis(600),
            CubicBezier::new(0.4, 0.2, 0.1, 1.0),
            mid,
        );
        assert!((tween.value(mid) - visible).abs() < 1e-5);
        assert!(tween.active(mid));
    }
}
