//! widgets：amll（由 widgets.rs 拆分，行为不变）。

use super::*;

use std::time::Duration;

use crate::motion::{
    CubicBezier, MotionSpring, MotionTuning, MotionTween, current_tuning, scaled_duration,
};

/// AMLL `BouncingSlider` 的等价实现。
///
/// 几何：命中区 24px，轨道 20px，白色 15%；静止时用 `inset(6px)` 裁剪成
/// 8px 可见条，hover 时用 280ms `easeOut` 展开到 20px，离开后用 AMLL 的
/// `damping=12 / stiffness=200` 弹簧收回。填充白色 40%。
///
/// 交互：
/// - `change_on_drag = false`（进度条）：拖动只更新视觉位置，松手/点击才 seek；
/// - `change_on_drag = true`（音量）：拖动过程中持续回调；
/// - 拖动超出两端时目标为 `tanh(rel * 2) * 12px`，经过 `d12/k300` 弹簧平滑；
///   松手保留弹簧当前速度归零，与 AMLL 的 `bounceXSpring.set(0)` 一致。
///
/// 状态按 `id` 持久化：gpui 每次 render 都会重建元素，若把 hover/拖拽状态放在
/// `RenderOnce` 内部，notify 触发重绘后状态就丢了，展开动画会闪回。
pub fn amll_slider(
    value: f32,
    id: &'static str,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    amll_slider_impl(value, id, false, None, on_change)
}

/// AMLL 播放进度条：播放时按真实时间外推，避免状态轮询造成阶梯跳变。
pub fn amll_playback_slider(
    value: f32,
    duration_ms: u64,
    playing: bool,
    id: &'static str,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    amll_slider_impl(
        value,
        id,
        false,
        Some(PlaybackOptions {
            duration_ms,
            playing,
        }),
        on_change,
    )
}

/// 与 [`amll_slider`] 相同，但拖动过程中就回调（AMLL 音量滑杆）。
pub fn amll_live_slider(
    value: f32,
    id: &'static str,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    amll_slider_impl(value, id, true, None, on_change)
}

#[derive(Clone, Copy)]
struct PlaybackOptions {
    duration_ms: u64,
    playing: bool,
}

#[derive(Clone, Copy)]
struct PlaybackClock {
    value: f32,
    at: Instant,
    duration_ms: u64,
}

fn amll_slider_impl(
    value: f32,
    id: &'static str,
    change_on_drag: bool,
    playback: Option<PlaybackOptions>,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    Component::new(AmllSlider {
        value: value.clamp(0.0, 1.0),
        id,
        change_on_drag,
        playback,
        on_change: Rc::new(RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

/// AMLL `BouncingSlider` 的初始裁剪量：(20 - 8) / 2。
pub(super) const AMLL_SLIDER_INSET: f32 = 6.0;
pub(super) const AMLL_SLIDER_HEIGHT: f32 = 20.0;
pub(super) const AMLL_MAX_BOUNCE: f32 = 12.0;

/// hover 展开（以及悬停中松手）的基准时长，对应 AMLL 的 0.28s `easeOut`。
const INSET_EXPAND_MS: f32 = 280.0;
/// 滑杆行程只有几像素，不能复用大几何通道的 1px/24px-s 容差，否则会在过冲前吸附。
const SLIDER_TOL: (f32, f32) = (0.01, 0.5);

#[derive(Clone, Copy)]
enum SliderMotion {
    Spring(MotionSpring),
    Tween(MotionTween),
}

impl SliderMotion {
    fn value(self, now: Instant) -> f32 {
        match self {
            Self::Spring(spring) => spring.value(now),
            Self::Tween(tween) => tween.value(now),
        }
    }

    fn target(self) -> f32 {
        match self {
            Self::Spring(spring) => spring.target(),
            Self::Tween(tween) => tween.target(),
        }
    }

    fn done(self, now: Instant) -> bool {
        match self {
            Self::Spring(spring) => spring.done(now),
            Self::Tween(tween) => tween.done(now),
        }
    }
}

pub(super) struct AmllSliderState {
    bounds: Cell<Bounds<Pixels>>,
    hovered: Cell<bool>,
    dragging: Cell<bool>,
    view: Cell<Option<gpui::EntityId>>,
    /// 轨道上下裁剪量：6 = 8px 可见，0 = 20px 展开。
    inset: Cell<f32>,
    inset_anim: Cell<Option<SliderMotion>>,
    /// 越界拖动时的整条水平偏移。
    bounce: Cell<f32>,
    bounce_anim: Cell<Option<SliderMotion>>,
    /// 拖动中的本地进度，松手后保留到外部数值追上为止。
    local_value: Cell<Option<f32>>,
    last_external: Cell<f32>,
    external_init: Cell<bool>,
    playback_options: Cell<Option<PlaybackOptions>>,
    playback_clock: Cell<Option<PlaybackClock>>,
}

impl AmllSliderState {
    fn new() -> Self {
        Self {
            bounds: Cell::new(Bounds::new(
                point(px(0.), px(0.)),
                gpui::size(px(0.), px(0.)),
            )),
            hovered: Cell::new(false),
            dragging: Cell::new(false),
            view: Cell::new(None),
            inset: Cell::new(AMLL_SLIDER_INSET),
            inset_anim: Cell::new(None),
            bounce: Cell::new(0.0),
            bounce_anim: Cell::new(None),
            local_value: Cell::new(None),
            last_external: Cell::new(0.0),
            external_init: Cell::new(false),
            playback_options: Cell::new(None),
            playback_clock: Cell::new(None),
        }
    }
}

thread_local! {
    /// 按控件 id 复用的滑杆状态；与 app 生命周期一致。
    static AMLL_SLIDER_STATES: RefCell<HashMap<&'static str, Rc<AmllSliderState>>> =
        RefCell::new(HashMap::new());
}

pub(super) fn amll_slider_state(id: &'static str) -> Rc<AmllSliderState> {
    AMLL_SLIDER_STATES.with(|states| {
        states
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| Rc::new(AmllSliderState::new()))
            .clone()
    })
}

pub(super) fn notify_amll_slider_view(state: &AmllSliderState, cx: &mut App) {
    if let Some(view) = state.view.get() {
        cx.notify(view);
    }
}

pub(super) struct AmllSlider {
    value: f32,
    id: &'static str,
    change_on_drag: bool,
    playback: Option<PlaybackOptions>,
    on_change: Rc<RefCell<Box<dyn FnMut(f32, &mut App)>>>,
}

/// 展开使用 AMLL 的 tween；收起使用 AMLL 的物理弹簧。两者都从当前可见值开始。
pub(super) fn start_inset_anim(
    state: &AmllSliderState,
    to: f32,
    tuning: MotionTuning,
    now: Instant,
) {
    let from = state
        .inset_anim
        .get()
        .map(|motion| motion.value(now))
        .unwrap_or_else(|| state.inset.get());
    if (from - to).abs() < 0.01 {
        state.inset.set(to);
        state.inset_anim.set(None);
        return;
    }
    if to <= 0.0 {
        state
            .inset_anim
            .set(Some(SliderMotion::Tween(MotionTween::new(
                from,
                to,
                scaled_duration(
                    Duration::from_millis(INSET_EXPAND_MS as u64),
                    tuning.duration,
                ),
                CubicBezier::ease_out(),
                now,
            ))));
    } else if let Some(SliderMotion::Spring(mut spring)) = state.inset_anim.get() {
        spring.retarget_with(to, spring.sample(now).velocity, now);
        state.inset_anim.set(Some(SliderMotion::Spring(spring)));
    } else {
        state
            .inset_anim
            .set(Some(SliderMotion::Spring(MotionSpring::new(
                from,
                to,
                0.0,
                now,
                tuning.physical(1.0, 12.0, 200.0, false),
                SLIDER_TOL.0,
                SLIDER_TOL.1,
            ))));
    }
}

/// 更新越界回弹目标；已有弹簧保留当前位置与速度，不注入指针速度。
pub(super) fn start_bounce_anim(
    state: &AmllSliderState,
    to: f32,
    tuning: MotionTuning,
    now: Instant,
) {
    let from = state
        .bounce_anim
        .get()
        .map(|motion| motion.value(now))
        .unwrap_or_else(|| state.bounce.get());
    if let Some(SliderMotion::Spring(mut spring)) = state.bounce_anim.get() {
        spring.retarget_with(to, spring.sample(now).velocity, now);
        state.bounce_anim.set(Some(SliderMotion::Spring(spring)));
    } else {
        state
            .bounce_anim
            .set(Some(SliderMotion::Spring(MotionSpring::new(
                from,
                to,
                0.0,
                now,
                tuning.physical(1.0, 12.0, 300.0, false),
                SLIDER_TOL.0,
                SLIDER_TOL.1,
            ))));
    }
}

/// 纯帧推进：返回是否还需要下一帧，便于无显示器测试。
fn advance_amll_value(anim: &Cell<Option<SliderMotion>>, value: &Cell<f32>, now: Instant) -> bool {
    let Some(motion) = anim.get() else {
        return false;
    };
    let done = motion.done(now);
    value.set(if done {
        motion.target()
    } else {
        motion.value(now)
    });
    if done {
        anim.set(None);
    }
    !done
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SliderVisualState {
    pub inset: f32,
    pub bounce: f32,
    pub shown: f32,
    pub inset_active: bool,
    pub bounce_active: bool,
    pub playback_active: bool,
}

impl SliderVisualState {
    pub fn needs_frame(self) -> bool {
        self.inset_active || self.bounce_active || self.playback_active
    }
}

/// 组件在指定时刻的完整视觉状态；生产 paint 与测试共用，避免只测弹簧不测组件。
pub(super) fn slider_visual_state_at(
    state: &AmllSliderState,
    external: f32,
    now: Instant,
) -> SliderVisualState {
    let inset_active = advance_amll_value(&state.inset_anim, &state.inset, now);
    let bounce_active = advance_amll_value(&state.bounce_anim, &state.bounce, now);
    let external = external.clamp(0.0, 1.0);
    let options = state.playback_options.get();

    if !state.external_init.get() {
        state.last_external.set(external);
        state.external_init.set(true);
        if !state.dragging.get()
            && let Some(options) = options
            && options.playing
            && options.duration_ms > 0
        {
            state.playback_clock.set(Some(PlaybackClock {
                value: external,
                at: now,
                duration_ms: options.duration_ms,
            }));
        }
    } else if (state.last_external.get() - external).abs() > 0.0001 {
        state.last_external.set(external);
        if !state.dragging.get() {
            state.local_value.set(None);
            state.playback_clock.set(None);
            if let Some(options) = options
                && options.playing
                && options.duration_ms > 0
            {
                state.playback_clock.set(Some(PlaybackClock {
                    value: external,
                    at: now,
                    duration_ms: options.duration_ms,
                }));
            }
        }
    }

    if state.dragging.get() || !options.is_some_and(|options| options.playing) {
        state.playback_clock.set(None);
    }

    let mut playback_active = false;
    let shown = if let Some(local) = state.local_value.get() {
        local
    } else if let Some(clock) = state.playback_clock.get() {
        let elapsed = now.saturating_duration_since(clock.at).as_millis() as f32;
        let value = (clock.value + elapsed / clock.duration_ms as f32).clamp(0.0, 1.0);
        playback_active = value < 1.0;
        value
    } else {
        external
    }
    .clamp(0.0, 1.0);

    SliderVisualState {
        inset: state.inset.get(),
        bounce: state.bounce.get(),
        shown,
        inset_active,
        bounce_active,
        playback_active,
    }
}

/// 原始（未夹取）位置比例，用于 AMLL 的越界回弹。
pub(super) fn raw_fraction_at(
    bounds: Bounds<Pixels>,
    position: gpui::Point<Pixels>,
) -> Option<f32> {
    let w = f32::from(bounds.size.width);
    if w <= 0.0 {
        return None;
    }
    Some(f32::from(position.x - bounds.left()) / w)
}

pub(super) fn bounce_for(rel: f32) -> f32 {
    if rel < 0.0 {
        (rel * 2.0).tanh() * AMLL_MAX_BOUNCE
    } else if rel > 1.0 {
        ((rel - 1.0) * 2.0).tanh() * AMLL_MAX_BOUNCE
    } else {
        0.0
    }
}

impl RenderOnce for AmllSlider {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let state = amll_slider_state(self.id);
        state.playback_options.set(self.playback);
        let on_change = self.on_change;
        let value = self.value;
        let id = self.id;
        let change_on_drag = self.change_on_drag;

        let state_down = state.clone();
        let change_down = on_change.clone();
        let state_move = state.clone();
        let change_move = on_change.clone();
        let state_up = state.clone();
        let change_up = on_change.clone();
        let state_out = state.clone();
        let change_out = on_change.clone();
        let state_hover = state.clone();
        let state_bounds = state.clone();
        let state_paint = state.clone();

        let finish = move |state: &Rc<AmllSliderState>,
                           change: &Rc<RefCell<Box<dyn FnMut(f32, &mut App)>>>,
                           cx: &mut App| {
            if !state.dragging.get() {
                return;
            }
            state.dragging.set(false);
            if let Some(f) = state.local_value.get() {
                (change.borrow_mut())(f, cx);
            }
            // local_value 保留到外部状态追上，避免松手后先闪回旧值再跳回。
            let now = Instant::now();
            let tuning = current_tuning(cx);
            start_bounce_anim(state, 0.0, tuning, now);
            if state.hovered.get() {
                start_inset_anim(state, 0.0, tuning, now);
            } else {
                start_inset_anim(state, AMLL_SLIDER_INSET, tuning, now);
            }
            notify_amll_slider_view(state, cx);
        };
        let finish_up = finish.clone();
        let finish_out = finish.clone();

        div()
            .id(SharedString::from(format!("amll-slider-{id}")))
            .h(px(24.0))
            .w_full()
            .relative()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                let now = Instant::now();
                state_down.playback_clock.set(None);
                state_down.dragging.set(true);
                if let Some(f) = fraction_at(state_down.bounds.get(), event.position) {
                    state_down.local_value.set(Some(f));
                    if change_on_drag {
                        (change_down.borrow_mut())(f, cx);
                    }
                }
                start_inset_anim(&state_down, 0.0, current_tuning(cx), now);
                notify_amll_slider_view(&state_down, cx);
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if state_move.dragging.get()
                    && event.pressed_button == Some(MouseButton::Left)
                    && let Some(raw) = raw_fraction_at(state_move.bounds.get(), event.position)
                {
                    let f = raw.clamp(0.0, 1.0);
                    state_move.local_value.set(Some(f));
                    if change_on_drag {
                        (change_move.borrow_mut())(f, cx);
                    }
                    // AMLL：指针只更新弹簧目标，视觉偏移由 d12/k300 弹簧平滑。
                    start_bounce_anim(
                        &state_move,
                        bounce_for(raw),
                        current_tuning(cx),
                        Instant::now(),
                    );
                    notify_amll_slider_view(&state_move, cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                finish_up(&state_up, &change_up, cx);
            })
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                finish_out(&state_out, &change_out, cx);
            })
            .on_hover(move |hovered, _, cx| {
                state_hover.hovered.set(*hovered);
                let now = Instant::now();
                let tuning = current_tuning(cx);
                if *hovered {
                    if !state_hover.dragging.get() {
                        start_inset_anim(&state_hover, 0.0, tuning, now);
                    }
                } else if !state_hover.dragging.get() {
                    start_inset_anim(&state_hover, AMLL_SLIDER_INSET, tuning, now);
                }
                notify_amll_slider_view(&state_hover, cx);
            })
            .child(
                canvas(
                    move |bounds, window, _| {
                        state_bounds.bounds.set(bounds);
                        state_bounds.view.set(Some(window.current_view()));
                        bounds
                    },
                    move |_bounds, bounds, window, _| {
                        paint_amll_slider(bounds, value, &state_paint, window);
                    },
                )
                .size_full(),
            )
    }
}

pub(super) fn paint_amll_slider(
    bounds: Bounds<Pixels>,
    value: f32,
    state: &AmllSliderState,
    w: &mut Window,
) {
    let now = Instant::now();
    let visual = slider_visual_state_at(state, value, now);
    if visual.needs_frame() {
        w.request_animation_frame();
    }

    let area_h = f32::from(bounds.size.height);
    let top = bounds.top() + px(((area_h - AMLL_SLIDER_HEIGHT) * 0.5).max(0.0));
    // 收回弹簧允许超过静止 inset，否则回弹会被绘制层钳掉。
    let inset = visual.inset.max(0.0);
    let visible_h = (AMLL_SLIDER_HEIGHT - inset * 2.0).max(1.0);
    let cy = top + px(AMLL_SLIDER_HEIGHT * 0.5);
    let x = bounds.left() + px(visual.bounce);

    let track = Bounds::new(
        point(x, cy - px(visible_h * 0.5)),
        gpui::size(bounds.size.width, px(visible_h)),
    );
    w.paint_quad(gpui::fill(track, gpui::white().opacity(0.15)).corner_radii(px(visible_h * 0.5)));
    let fill_w = bounds.size.width * visual.shown;
    if f32::from(fill_w) > 0.0 {
        let fill = Bounds::new(
            point(x, cy - px(visible_h * 0.5)),
            gpui::size(fill_w, px(visible_h)),
        );
        w.paint_quad(
            gpui::fill(fill, gpui::white().opacity(0.4)).corner_radii(px(visible_h * 0.5)),
        );
    }
}

/// 小圆点用圆角 quad 绘制，避免为每个滑杆拇指触发一次全屏路径通道。
pub(super) fn paint_dot(w: &mut Window, cx: Pixels, cy: Pixels, r: f32, color: Hsla) {
    w.paint_quad(
        gpui::fill(
            Bounds::new(
                point(cx - px(r), cy - px(r)),
                gpui::size(px(r * 2.0), px(r * 2.0)),
            ),
            color,
        )
        .corner_radii(px(r)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_uses_amll_physical_spring() {
        let spec = MotionTuning::default().physical(1.0, 12.0, 200.0, false);
        let expected_omega = 200.0f32.sqrt();
        let expected_zeta = 12.0 / (2.0 * expected_omega);
        assert!((spec.omega - expected_omega).abs() < 0.01);
        assert!((spec.zeta - expected_zeta).abs() < 0.01);
    }

    #[test]
    fn bounce_uses_amll_physical_spring() {
        let spec = MotionTuning::default().physical(1.0, 12.0, 300.0, false);
        let expected_omega = 300.0f32.sqrt();
        let expected_zeta = 12.0 / (2.0 * expected_omega);
        assert!((spec.omega - expected_omega).abs() < 0.01);
        assert!((spec.zeta - expected_zeta).abs() < 0.01);
    }

    #[test]
    fn collapse_overshoot_is_visible_past_the_resting_inset() {
        let now = Instant::now();
        let state = AmllSliderState::new();
        state.inset.set(0.0);
        start_inset_anim(&state, AMLL_SLIDER_INSET, MotionTuning::default(), now);
        let first = slider_visual_state_at(&state, 0.0, now);
        assert_eq!(first.inset, 0.0);
        assert!(first.inset_active);

        let peak = (0..=1_200)
            .map(|ms| slider_visual_state_at(&state, 0.0, now + Duration::from_millis(ms)).inset)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            peak > AMLL_SLIDER_INSET + 1.0,
            "AMLL's d12/k200 spring should overshoot by about 1.5px, got {peak}"
        );
        let end = slider_visual_state_at(&state, 0.0, now + Duration::from_secs(2));
        assert_eq!(end.inset, AMLL_SLIDER_INSET);
        assert!(!end.inset_active);
    }

    #[test]
    fn expand_uses_280ms_ease_out_tween() {
        let now = Instant::now();
        let state = AmllSliderState::new();
        start_inset_anim(&state, 0.0, MotionTuning::default(), now);
        assert_eq!(
            slider_visual_state_at(&state, 0.0, now).inset,
            AMLL_SLIDER_INSET
        );
        let mid = slider_visual_state_at(&state, 0.0, now + Duration::from_millis(140));
        let expected = AMLL_SLIDER_INSET * (1.0 - CubicBezier::ease_out().sample(0.5));
        assert!((mid.inset - expected).abs() < 0.01);
        assert!(mid.inset_active);
        let end = slider_visual_state_at(&state, 0.0, now + Duration::from_millis(280));
        assert_eq!(end.inset, 0.0);
        assert!(!end.inset_active);
    }

    #[test]
    fn bounce_target_is_smoothed_and_release_crosses_zero() {
        let now = Instant::now();
        let state = AmllSliderState::new();
        start_bounce_anim(&state, 12.0, MotionTuning::default(), now);
        let first = slider_visual_state_at(&state, 0.0, now);
        assert_eq!(first.bounce, 0.0);
        assert!(first.bounce_active);
        let moving = slider_visual_state_at(&state, 0.0, now + Duration::from_millis(16));
        assert!(moving.bounce > 0.0 && moving.bounce < 12.0);

        let release = now + Duration::from_millis(16);
        let before = moving.bounce;
        start_bounce_anim(&state, 0.0, MotionTuning::default(), release);
        let after = slider_visual_state_at(&state, 0.0, release);
        assert!((after.bounce - before).abs() < 0.01);
        let minimum = (0..=600)
            .map(|ms| {
                slider_visual_state_at(&state, 0.0, release + Duration::from_millis(ms)).bounce
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            minimum < 0.0,
            "release spring should cross zero, got {minimum}"
        );
        let end = slider_visual_state_at(&state, 0.0, release + Duration::from_secs(2));
        assert_eq!(end.bounce, 0.0);
        assert!(!end.bounce_active);
    }

    #[test]
    fn released_local_value_waits_for_external_state_without_flashback() {
        let now = Instant::now();
        let state = AmllSliderState::new();
        slider_visual_state_at(&state, 0.2, now);
        state.dragging.set(true);
        state.local_value.set(Some(0.8));
        state.dragging.set(false);
        assert_eq!(slider_visual_state_at(&state, 0.2, now).shown, 0.8);
        assert_eq!(slider_visual_state_at(&state, 0.8, now).shown, 0.8);
    }

    #[test]
    fn playback_progress_extrapolates_and_requests_frames() {
        let now = Instant::now();
        let state = AmllSliderState::new();
        state.playback_options.set(Some(PlaybackOptions {
            duration_ms: 1_000,
            playing: true,
        }));
        let first = slider_visual_state_at(&state, 0.2, now);
        assert_eq!(first.shown, 0.2);
        assert!(first.playback_active);
        let mid = slider_visual_state_at(&state, 0.2, now + Duration::from_millis(250));
        assert!((mid.shown - 0.45).abs() < 0.001);
        assert!(mid.needs_frame());
        let end = slider_visual_state_at(&state, 0.2, now + Duration::from_secs(1));
        assert_eq!(end.shown, 1.0);
        assert!(!end.playback_active);
    }

    #[gpui::test]
    fn slider_mounts_draws_and_dispatches_clicks(cx: &mut gpui::TestAppContext) {
        use gpui::{Context, IntoElement, Modifiers, Render, Styled, Window, div, point, px};

        struct SliderView {
            calls: Rc<Cell<u32>>,
        }

        impl Render for SliderView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let calls = self.calls.clone();
                div().w(px(240.0)).h(px(24.0)).child(amll_slider(
                    0.4,
                    "gpui-slider-click",
                    move |_, _| {
                        calls.set(calls.get() + 1);
                    },
                ))
            }
        }

        let calls = Rc::new(Cell::new(0));
        let (view, cx) = cx.add_window_view(|_, _| SliderView {
            calls: calls.clone(),
        });
        cx.simulate_click(point(px(120.0), px(12.0)), Modifiers::none());
        assert_eq!(calls.get(), 1);

        // 同一实体重绘后仍复用 thread-local 状态，并继续处理输入。
        view.update_in(cx, |_, _, cx| cx.notify());
        cx.run_until_parked();
        cx.simulate_click(point(px(80.0), px(12.0)), Modifiers::none());
        assert_eq!(calls.get(), 2);
    }

    #[gpui::test]
    fn playback_slider_draws_without_a_real_raf_loop(cx: &mut gpui::TestAppContext) {
        use gpui::{Context, IntoElement, Render, Styled, Window, div, px};

        struct PlaybackSliderView;

        impl Render for PlaybackSliderView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(240.0)).h(px(24.0)).child(amll_playback_slider(
                    0.2,
                    10_000,
                    true,
                    "gpui-playback-slider",
                    |_, _| {},
                ))
            }
        }

        let (view, cx) = cx.add_window_view(|_, _| PlaybackSliderView);
        view.update_in(cx, |_, _, cx| cx.notify());
        cx.run_until_parked();
    }
}
