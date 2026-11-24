//! `ChordialApp` 的 animations 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

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
        let Some((start, from, to, duration)) = self.cover_scale_anim else {
            return self.cover_scale_target();
        };
        let t = (now.saturating_duration_since(start).as_secs_f32() / duration.max(0.001))
            .clamp(0.0, 1.0);
        let eased = if to > from {
            // 播放：AMLL 的 cubic-bezier(0.3, 0.2, 0.2, 1.4)，轻微过冲。
            crate::icons::cubic_bezier(0.3, 0.2, 0.2, 1.4, t)
        } else {
            crate::icons::cubic_bezier(0.4, 0.2, 0.1, 1.0, t)
        };
        from + (to - from) * eased
    }

    /// 播放状态变化时启动封面缩放；`from` 必须在写入新状态之前采样。
    pub(super) fn start_cover_scale_anim(&mut self, from: f32, playing: bool) {
        let target = if playing { 1.0 } else { 0.75 };
        if (from - target).abs() < 0.001 {
            self.cover_scale_anim = None;
            return;
        }
        let duration = if playing { 0.5 } else { 0.6 };
        self.cover_scale_anim = Some((Instant::now(), from, target, duration));
    }

    pub(super) fn cover_scale_anim_active(&self, now: Instant) -> bool {
        self.cover_scale_anim
            .is_some_and(|(start, _, _, duration)| {
                now.saturating_duration_since(start).as_secs_f32() < duration
            })
    }

    /// 沉浸页顶部把手的展开进度：`1` = 文案胶囊，`0` = 原本的 50×8 小白条。
    ///
    /// 进入沉浸页后展开成「拖动/ESC 退出」文案展示 5s，随后用 320ms
    /// 收回成原来的小白条。
    pub(super) fn immersive_hint_expand(&self, now: Instant) -> f32 {
        let Some(start) = self.immersive_hint_at else {
            return 0.0;
        };
        let ms = now.saturating_duration_since(start).as_secs_f32() * 1000.0;
        if ms < Self::IMMERSIVE_HINT_HOLD_MS {
            return 1.0;
        }
        (1.0 - (ms - Self::IMMERSIVE_HINT_HOLD_MS) / Self::IMMERSIVE_HINT_FADE_MS).clamp(0.0, 1.0)
    }

    /// 提示的 5s 静态展示期不请求动画帧，只有淡出阶段需要逐帧重绘。
    pub(super) fn immersive_hint_fading(&self, now: Instant) -> bool {
        self.immersive_hint_at.is_some_and(|start| {
            let ms = now.saturating_duration_since(start).as_secs_f32() * 1000.0;
            ms >= Self::IMMERSIVE_HINT_HOLD_MS
                && ms < Self::IMMERSIVE_HINT_HOLD_MS + Self::IMMERSIVE_HINT_FADE_MS
        })
    }

    /// 闲置收起的显隐进度：`1` = 完整控件，`0` = 完全隐藏。
    /// 用 smoothstep 过渡，和布局的 space-between 回收同步。
    pub(super) fn stage_chrome_visibility(&self, now: Instant) -> f32 {
        let t = (now
            .saturating_duration_since(self.stage_chrome_at)
            .as_secs_f32()
            * 1000.0
            / Self::STAGE_CHROME_FADE_MS)
            .clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        if self.stage_chrome_hidden {
            1.0 - eased
        } else {
            eased
        }
    }

    pub(super) fn stage_chrome_fading(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.stage_chrome_at)
            .as_secs_f32()
            * 1000.0
            < Self::STAGE_CHROME_FADE_MS
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
            self.ensure_ui_anim_ticker(cx);
            cx.notify();
        }
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
            || self
                .stage_motion
                .is_some_and(|motion| motion.active(Instant::now()))
            || self.cover_scale_anim_active(Instant::now())
            || self
                .info_marquee
                .is_some_and(|marquee| marquee.active(Instant::now()))
            || self.portrait_animating()
            || self.sheet_drag.is_some_and(|drag| drag.animating())
            || self.phone_sheet_animating(Instant::now())
            || self
                .phone_edge_swipe
                .is_some_and(|swipe| swipe.animating(Instant::now()))
            || self.drawer_animating(Instant::now())
            || self.immersive_expanding(Instant::now())
            || self.eq_animating()
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
        if self.stage_motion.is_some_and(|motion| motion.settled(now)) {
            self.stage_motion = None;
        }
        if let Some((start, _, _, duration)) = self.cover_scale_anim
            && now.saturating_duration_since(start).as_secs_f32() >= duration
        {
            self.cover_scale_anim = None;
        }
        if self
            .info_marquee
            .is_some_and(|marquee| !marquee.active(now))
        {
            self.info_marquee = None;
        }
        // 抽屉开/合动画结束：清掉起点；关闭状态由 `drawer_visible` 判定隐藏。
        if self.drawer_at.is_some_and(|at| {
            now.saturating_duration_since(at).as_secs_f32() * 1000.0 >= Self::DRAWER_ANIM_MS
        }) {
            self.drawer_at = None;
        }
        // Dock→全屏展开结束：撤销动画状态，页面由静态布局接管。
        if self
            .immersive_expand
            .is_some_and(|spring| spring.settled(now))
        {
            self.immersive_expand = None;
        }
        // 全屏→Dock 收起结束：此时卡片已经与 Dock 重合，才真正卸载沉浸层，
        // 让真实 Dock 无缝接手。提前卸载会看到整个沉浸页直接消失。
        if self
            .immersive_collapse
            .is_some_and(|spring| spring.settled(now))
        {
            self.immersive_collapse = None;
            self.finish_immersive_exit(cx);
        }
        // 顶部把手提示展示完毕：清掉起点，回到静态小白条。
        if self.immersive_hint_at.is_some_and(|t| {
            now.saturating_duration_since(t).as_secs_f32() * 1000.0
                >= Self::IMMERSIVE_HINT_HOLD_MS + Self::IMMERSIVE_HINT_FADE_MS
        }) {
            self.immersive_hint_at = None;
        }
        // 下拉关闭：回弹结束直接收手；关闭动画结束才真正退出沉浸层。
        if let Some(drag) = self.sheet_drag {
            match drag.settled(now) {
                Some(SheetDragOutcome::Returned) => self.sheet_drag = None,
                Some(SheetDragOutcome::Dismissed) => {
                    // 下拉手势已经把卡片带到 Dock 位置，无需再跑一遍收起弹簧。
                    self.sheet_drag = None;
                    self.finish_immersive_exit(cx);
                }
                None => {}
            }
        }
        // 手机壳 mini player ⇄ 播放页：动画结束后才切页，避免中途把
        // sheet 的 `lyric_view` 背景交还给曲库页。
        if let Some(spring) = self.phone_sheet_spring
            && spring.settled(now)
        {
            let end = spring.position(now);
            self.phone_sheet_spring = None;
            if end < 0.5 && self.phone_page == PhonePage::Player {
                self.phone_page = PhonePage::Browse;
            }
        }
        if let Some(drag) = self.phone_sheet {
            match drag.settled(now) {
                Some(SheetPage::Full) => {
                    self.phone_sheet = None;
                    self.phone_page = PhonePage::Player;
                    self.phone_sheet_spring = None;
                }
                Some(SheetPage::Mini) => {
                    self.phone_sheet = None;
                    if self.phone_page == PhonePage::Player {
                        self.phone_page = PhonePage::Browse;
                    }
                }
                None => {}
            }
        }
        // 封面 ⇄ 歌词横滑：提交后在动画落点切换页面。
        if let Some(swipe) = self.phone_edge_swipe {
            match swipe.settled(now) {
                Some(SwipeOutcome::Committed(crate::phone_gesture::SwipeDirection::Forward)) => {
                    self.phone_edge_swipe = None;
                    self.phone_page = PhonePage::Lyrics;
                    self.phone_swipe_suppress_click = Some(now);
                }
                Some(SwipeOutcome::Committed(crate::phone_gesture::SwipeDirection::Back)) => {
                    self.phone_edge_swipe = None;
                    self.phone_page = PhonePage::Player;
                    self.phone_sheet_spring =
                        Some(SpringSettle::new(0.0, 1.0, 0.0, now, 25.0, 190.0));
                    self.ensure_ui_anim_ticker(cx);
                    self.phone_swipe_suppress_click = Some(now);
                }
                Some(SwipeOutcome::Returned) => self.phone_edge_swipe = None,
                None => {}
            }
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
        if !self.immersive || self.eq_open || self.immersive_expanding(Instant::now()) {
            return;
        }
        self.sheet_drag = Some(SheetDrag::begin(y, Instant::now()));
        cx.notify();
    }

    pub(super) fn update_sheet_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        if let Some(drag) = &mut self.sheet_drag {
            if drag.update(y, Instant::now()) {
                cx.notify();
            }
        }
    }

    pub(super) fn release_sheet_drag(&mut self, height: f32, cx: &mut Context<Self>) {
        let now = Instant::now();
        let dock_y = dock_rect(self.viewport_width, self.viewport_height).y;
        if let Some(drag) = &mut self.sheet_drag {
            drag.release(height, dock_y, now);
            if !drag.animating() {
                self.sheet_drag = None;
            }
        }
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    /// Dock→全屏共享布局的当前进度：`1.0` = 页面仍完全贴在 Dock 上，
    /// `0.0` = 已铺满整个窗口。展开动画与下拉手势共用这一条通道。
    pub(super) fn immersive_dock_progress(&self, now: Instant, height: f32) -> f32 {
        // 收起优先于展开：退出动画期间即使旧的 expand 还在，也必须以收起的
        // 当前位置为准，否则卡片会在退出第一帧弹回满屏。
        if let Some(spring) = self.immersive_collapse {
            return spring.position(now).clamp(0.0, 1.0);
        }
        if let Some(spring) = self.immersive_expand {
            return spring.position(now).clamp(0.0, 1.0);
        }
        if let Some(drag) = self.sheet_drag.as_ref() {
            if drag.engaged() || drag.animating() {
                let dock_y = dock_rect(self.viewport_width, height).y.max(1.0);
                return (drag.offset(now) / dock_y).clamp(0.0, 1.0);
            }
        }
        0.0
    }

    pub(super) fn immersive_expanding(&self, now: Instant) -> bool {
        self.immersive_expand
            .is_some_and(|spring| !spring.settled(now))
            || self
                .immersive_collapse
                .is_some_and(|spring| !spring.settled(now))
    }

    /// 当前沉浸页的共享元素目标模态（与页面实际布局选择一致）。
    pub(super) fn immersive_stage_mode(&self, cx: &App, width: f32, height: f32) -> StageMode {
        if self.stage_split_active(cx, width, height) {
            StageMode::WideSplit
        } else {
            StageMode::CompactArt
        }
    }

    /// 请求退出沉浸层：先播完反向收起动画，落点与 Dock 重合后才真正卸载。
    ///
    /// 从当前 `p` 起弹簧而不是固定从 0 起：展开没结束就按 Esc、或连续切换时，
    /// 卡片不会先跳回满屏再收起。
    pub(super) fn exit_immersive(&mut self, cx: &mut Context<Self>) {
        if !self.immersive || self.immersive_collapse.is_some() {
            return;
        }
        let now = Instant::now();
        let p = self
            .immersive_dock_progress(now, self.viewport_height)
            .clamp(0.0, 1.0);
        self.sheet_drag = None;
        self.immersive_expand = None;
        self.immersive_collapse = Some(SpringSettle::new(
            p,
            1.0,
            0.0,
            now,
            Self::COLLAPSE_DAMPING,
            Self::EXPAND_STIFFNESS,
        ));
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
        self.immersive_expand = None;
        self.immersive_collapse = None;
        self.immersive_hint_at = None;
        self.force_portrait = false;
        self.portrait_page = PortraitPage::Art;
        self.portrait_transition = None;
        self.stage_view_menu_open = false;
        self.stage_chrome_hidden = false;
        self.stage_chrome_at = Instant::now();
        self.stage_last_input_at = Instant::now();
        self.hide_drawer_now();
        self.wide_lyrics_hidden = false;
        self.compact_lyrics_open = false;
        self.stage_motion = None;
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
