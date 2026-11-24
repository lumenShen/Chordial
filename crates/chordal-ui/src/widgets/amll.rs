//! widgets：amll（由 widgets.rs 拆分，行为不变）。

use super::*;

use crate::motion::{MotionSpring, MotionTuning, PIXEL_TOL, VelocityTracker, current_tuning};

/// AMLL `BouncingSlider` 的等价实现。
///
/// 几何：命中区 24px，轨道 20px，白色 15%；静止时用 `inset(6px)` 裁剪成
/// 8px 可见条，hover 时 280ms 弹簧展开到 20px，离开后用 200ms、稍多回弹的
/// 弹簧收回。填充白色 40%。
///
/// 交互：
/// - `change_on_drag = false`（进度条）：拖动只更新视觉位置，松手/点击才 seek；
/// - `change_on_drag = true`（音量）：拖动过程中持续回调；
/// - 拖动超出两端时整条按 `tanh(rel * 2) * 12px` 偏移，松手用 180ms 弹簧归零，
///   初速度来自指针追踪器，甩手不会先顿一下。
///
/// 状态按 `id` 持久化：gpui 每次 render 都会重建元素，若把 hover/拖拽状态放在
/// `RenderOnce` 内部，notify 触发重绘后状态就丢了，展开动画会闪回。
pub fn amll_slider(
    value: f32,
    id: &'static str,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    amll_slider_impl(value, id, false, on_change)
}

/// 与 [`amll_slider`] 相同，但拖动过程中就回调（AMLL 音量滑杆）。
pub fn amll_live_slider(
    value: f32,
    id: &'static str,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    amll_slider_impl(value, id, true, on_change)
}

pub(super) fn amll_slider_impl(
    value: f32,
    id: &'static str,
    change_on_drag: bool,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    Component::new(AmllSlider {
        value: value.clamp(0.0, 1.0),
        id,
        change_on_drag,
        on_change: Rc::new(RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

/// AMLL `BouncingSlider` 的初始裁剪量：(20 - 8) / 2。
pub(super) const AMLL_SLIDER_INSET: f32 = 6.0;
pub(super) const AMLL_SLIDER_HEIGHT: f32 = 20.0;
pub(super) const AMLL_MAX_BOUNCE: f32 = 12.0;

/// hover 展开（以及悬停中松手）的基准时长，对应原来的 0.28s ease-out。
const INSET_EXPAND_MS: f32 = 280.0;
/// 移出收起的基准时长，对应原来的 damping=12 / stiffness=200。
const INSET_COLLAPSE_MS: f32 = 200.0;
/// 越界回弹的基准时长，对应原来的 damping=12 / stiffness=300。
const BOUNCE_MS: f32 = 180.0;
/// 收起与回弹比默认预设更弹一点，只作用于这两条局部弹簧。
const SNAPPY_OVERSHOOT: f32 = 1.35;

pub(super) struct AmllSliderState {
    bounds: Cell<Bounds<Pixels>>,
    hovered: Cell<bool>,
    dragging: Cell<bool>,
    view: Cell<Option<gpui::EntityId>>,
    /// 轨道上下裁剪量：6 = 8px 可见，0 = 20px 展开。
    inset: Cell<f32>,
    inset_anim: Cell<Option<MotionSpring>>,
    /// 越界拖动时的整条水平偏移。
    bounce: Cell<f32>,
    bounce_anim: Cell<Option<MotionSpring>>,
    /// 松手时把指针速度交给回弹弹簧；静止超过 100ms 会归零。
    pointer_velocity: RefCell<VelocityTracker>,
    /// 拖动中的本地进度，松手后保留到外部数值追上为止。
    local_value: Cell<Option<f32>>,
    /// 音量这类「拖动即生效」的滑杆，外部状态要等下一次轮询才追上，
    /// 松手后必须继续显示本地值，否则会先闪回旧值再跳回。
    hold_local: Cell<bool>,
    last_external: Cell<f32>,
    external_init: Cell<bool>,
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
            pointer_velocity: RefCell::new(VelocityTracker::default()),
            local_value: Cell::new(None),
            hold_local: Cell::new(false),
            last_external: Cell::new(0.0),
            external_init: Cell::new(false),
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
    on_change: Rc<RefCell<Box<dyn FnMut(f32, &mut App)>>>,
}

/// 只放大这一条弹簧的回弹，不改全局手感设置。
fn snappy_tuning(tuning: MotionTuning) -> MotionTuning {
    MotionTuning::new(
        tuning.preset,
        tuning.duration,
        tuning.overshoot * SNAPPY_OVERSHOOT,
    )
}

/// 起点几乎到位时直接落位，避免重绘里反复申请动画帧。
pub(super) fn start_inset_anim(
    state: &AmllSliderState,
    to: f32,
    base_ms: f32,
    tuning: MotionTuning,
    velocity: f32,
    now: Instant,
) {
    let from = state.inset.get();
    if (from - to).abs() < 0.01 {
        state.inset.set(to);
        state.inset_anim.set(None);
        return;
    }
    state.inset_anim.set(Some(tuning.spring(
        base_ms,
        from,
        to,
        velocity,
        now,
        PIXEL_TOL.0,
        PIXEL_TOL.1,
    )));
}

/// 松手归零：从当前偏移出发，带上指针速度。
pub(super) fn start_bounce_anim(
    state: &AmllSliderState,
    to: f32,
    tuning: MotionTuning,
    velocity: f32,
    now: Instant,
) {
    state.bounce_anim.set(Some(tuning.spring(
        BOUNCE_MS,
        state.bounce.get(),
        to,
        velocity,
        now,
        PIXEL_TOL.0,
        PIXEL_TOL.1,
    )));
}

/// 收敛后钳到目标并清掉弹簧；未收敛则继续要下一帧。
pub(super) fn advance_amll_anim(
    anim: &Cell<Option<MotionSpring>>,
    value: &Cell<f32>,
    now: Instant,
    window: &mut Window,
) {
    if let Some(spring) = anim.get() {
        value.set(spring.value(now));
        if spring.done(now) {
            value.set(spring.target());
            anim.set(None);
        } else {
            window.request_animation_frame();
        }
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
        state.hold_local.set(self.change_on_drag);
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
            let final_fraction = state.local_value.get();
            if let Some(f) = final_fraction {
                (change.borrow_mut())(f, cx);
            }
            if !state.hold_local.get() {
                state.local_value.set(None);
            }
            // 追踪器把静止超过 100ms 的旧速度清零，停住再松手不会被当成甩动。
            let now = Instant::now();
            let velocity = state.pointer_velocity.borrow().velocity_at(now);
            state.pointer_velocity.borrow_mut().reset();
            let tuning = current_tuning(cx);
            start_bounce_anim(state, 0.0, snappy_tuning(tuning), velocity, now);
            if state.hovered.get() {
                start_inset_anim(state, 0.0, INSET_EXPAND_MS, tuning, 0.0, now);
            } else {
                start_inset_anim(
                    state,
                    AMLL_SLIDER_INSET,
                    INSET_COLLAPSE_MS,
                    snappy_tuning(tuning),
                    0.0,
                    now,
                );
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
                state_down.pointer_velocity.borrow_mut().reset();
                state_down
                    .pointer_velocity
                    .borrow_mut()
                    .push(f32::from(event.position.x), now);
                state_down.dragging.set(true);
                if let Some(f) = fraction_at(state_down.bounds.get(), event.position) {
                    state_down.local_value.set(Some(f));
                    if change_on_drag {
                        (change_down.borrow_mut())(f, cx);
                    }
                }
                start_inset_anim(
                    &state_down,
                    0.0,
                    INSET_EXPAND_MS,
                    current_tuning(cx),
                    0.0,
                    now,
                );
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
                    // 拖动期间越界回弹按 AMLL 的 tanh 曲线直接跟手。
                    state_move.bounce.set(bounce_for(raw));
                    state_move.bounce_anim.set(None);
                    // 记录指针位置，松手时换算成回弹弹簧的初速度。
                    state_move
                        .pointer_velocity
                        .borrow_mut()
                        .push(f32::from(event.position.x), Instant::now());
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
                        start_inset_anim(&state_hover, 0.0, INSET_EXPAND_MS, tuning, 0.0, now);
                    }
                } else if !state_hover.dragging.get() {
                    start_inset_anim(
                        &state_hover,
                        AMLL_SLIDER_INSET,
                        INSET_COLLAPSE_MS,
                        snappy_tuning(tuning),
                        0.0,
                        now,
                    );
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
    advance_amll_anim(&state.inset_anim, &state.inset, now, w);
    advance_amll_anim(&state.bounce_anim, &state.bounce, now, w);

    // 外部数值变化时清掉本地拖动值；拖动中忽略外部更新（与 AMLL 一致）。
    let external = value.clamp(0.0, 1.0);
    if !state.external_init.get() {
        state.last_external.set(external);
        state.external_init.set(true);
    } else if (state.last_external.get() - external).abs() > 0.0001 {
        state.last_external.set(external);
        if !state.dragging.get() {
            state.local_value.set(None);
        }
    }
    let shown = state.local_value.get().unwrap_or(external).clamp(0.0, 1.0);

    let area_h = f32::from(bounds.size.height);
    let top = bounds.top() + px(((area_h - AMLL_SLIDER_HEIGHT) * 0.5).max(0.0));
    let inset = state.inset.get().clamp(0.0, AMLL_SLIDER_INSET);
    let visible_h = AMLL_SLIDER_HEIGHT - inset * 2.0;
    let cy = top + px(AMLL_SLIDER_HEIGHT * 0.5);
    let x = bounds.left() + px(state.bounce.get());

    let track = Bounds::new(
        point(x, cy - px(visible_h * 0.5)),
        gpui::size(bounds.size.width, px(visible_h)),
    );
    w.paint_quad(gpui::fill(track, gpui::white().opacity(0.15)).corner_radii(px(visible_h * 0.5)));
    let fill_w = bounds.size.width * shown;
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
