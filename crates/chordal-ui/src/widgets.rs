//! Studio Neutral 基础组件：按钮、连续滑杆、封面、空态、播放指示。

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Component, Hsla, ImageSource, IntoElement, MouseButton,
    MouseMoveEvent, ObjectFit, Pixels, RenderImage, RenderOnce, SharedString, Window,
    canvas, div, img, point, px,
};

use crate::theme::Theme;

pub fn icon_button<F>(
    theme: &Theme,
    icon: impl IntoElement,
    size: f32,
    label: &'static str,
    on_click: F,
) -> AnyElement
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let radius = (size * 0.23).clamp(5.0, 8.0);
    div()
        // 稳定 ID：随机 ID 会在 hover/按下重绘时变化，mouse-up 找不到目标导致 click 丢失。
        .id(SharedString::from(format!("icon-btn-{label}")))
        .size(px(size))
        .rounded(px(radius))
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(theme.hover))
        .active(|s| s.bg(theme.active))
        .child(icon)
        .on_click(on_click)
        .into_any_element()
}

pub fn text_button<F>(
    theme: &Theme,
    label: impl Into<SharedString>,
    primary: bool,
    on_click: F,
) -> AnyElement
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let label = label.into();
    let base = div()
        // 稳定 ID：随机 ID 会在 hover/按下重绘时变化，导致 click 丢失。
        .id(SharedString::from(format!("text-btn-{label}")))
        .h(px(30.0))
        .px(px(12.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .text_size(px(12.0))
        .font_weight(gpui::FontWeight::MEDIUM);
    let base = if primary {
        base.bg(theme.accent)
            .text_color(theme.on_accent)
            .hover(|s| s.bg(theme.accent_hover))
    } else {
        base.bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .text_color(theme.text)
            .hover(|s| s.bg(theme.surface_alt))
    };
    base.child(label)
        .on_click(on_click)
        .into_any_element()
}

#[derive(Clone, Copy)]
pub struct SliderColors {
    pub track: Hsla,
    pub fill: Hsla,
    pub thumb: Hsla,
}

pub struct Slider {
    value: f32,
    colors: SliderColors,
    height: f32,
    id: &'static str,
    /// 始终显示白色圆形拇指（均衡器滑杆）。
    knob: bool,
    on_change: Rc<RefCell<Box<dyn FnMut(f32)>>>,
}

/// 连续滑杆（点击/拖动）；返回 AnyElement 以便直接塞进布局。
pub fn slider(
    value: f32,
    colors: SliderColors,
    id: &'static str,
    on_change: impl FnMut(f32) + 'static,
) -> AnyElement {
    Component::new(Slider {
        value: value.clamp(0.0, 1.0),
        colors,
        height: 16.0,
        id,
        knob: false,
        on_change: Rc::new(RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

/// 带常驻白色拇指的滑杆（均衡器输入/输出增益、滤波器参数）。
pub fn knob_slider(
    value: f32,
    colors: SliderColors,
    id: &'static str,
    on_change: impl FnMut(f32) + 'static,
) -> AnyElement {
    Component::new(Slider {
        value: value.clamp(0.0, 1.0),
        colors,
        height: 24.0,
        id,
        knob: true,
        on_change: Rc::new(RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

struct SliderState {
    bounds: Cell<Bounds<Pixels>>,
    hovered: Cell<bool>,
    dragging: Cell<bool>,
    /// 绘制该滑杆时所属的视图；高频拖动/悬停只通知它，避免
    /// `refresh_windows()` 让所有 AnyView 缓存失效。
    view: Cell<Option<gpui::EntityId>>,
}

fn notify_slider_view(state: &SliderState, cx: &mut App) {
    if let Some(view) = state.view.get() {
        cx.notify(view);
    }
}

impl RenderOnce for Slider {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let state = Rc::new(SliderState {
            bounds: Cell::new(Bounds::new(
                point(px(0.), px(0.)),
                gpui::size(px(0.), px(0.)),
            )),
            hovered: Cell::new(false),
            dragging: Cell::new(false),
            view: Cell::new(None),
        });
        let on_change = self.on_change;
        let value = self.value;
        let colors = self.colors;
        let height = self.height;
        let id = self.id;
        let knob = self.knob;

        let state_down = state.clone();
        let change_down = on_change.clone();
        let state_move = state.clone();
        let change_move = on_change.clone();
        let state_up = state.clone();
        let state_hover = state.clone();
        let state_bounds = state.clone();
        let state_paint = state.clone();

        div()
            .id(SharedString::from(format!("slider-{id}")))
            .h(px(height))
            .w_full()
            .relative()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                state_down.dragging.set(true);
                if let Some(f) = fraction_at(state_down.bounds.get(), event.position) {
                    (change_down.borrow_mut())(f);
                    notify_slider_view(&state_down, cx);
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if state_move.dragging.get()
                    && event.pressed_button == Some(MouseButton::Left)
                    && let Some(f) = fraction_at(state_move.bounds.get(), event.position)
                {
                    (change_move.borrow_mut())(f);
                    notify_slider_view(&state_move, cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                state_up.dragging.set(false);
            })
            .on_hover(move |hovered, _, cx| {
                state_hover.hovered.set(*hovered);
                notify_slider_view(&state_hover, cx);
            })
            .child(
                canvas(
                    move |bounds, window, _| {
                        state_bounds.bounds.set(bounds);
                        state_bounds.view.set(Some(window.current_view()));
                        bounds
                    },
                    move |_bounds, bounds, window, _| {
                        paint_slider(
                            bounds,
                            value,
                            colors,
                            state_paint.hovered.get() || state_paint.dragging.get(),
                            knob,
                            window,
                        );
                    },
                )
                .size_full(),
            )
    }
}

fn fraction_at(bounds: Bounds<Pixels>, position: gpui::Point<Pixels>) -> Option<f32> {
    let w = f32::from(bounds.size.width);
    if w <= 0.0 {
        return None;
    }
    let x = f32::from(position.x - bounds.left()) / w;
    Some(x.clamp(0.0, 1.0))
}

/// AMLL `BouncingSlider` 的等价实现。
///
/// 几何：命中区 24px，轨道 20px，白色 15%；静止时用 `inset(6px)` 裁剪成
/// 8px 可见条，hover 时 0.28s ease-out 展开到 20px，离开后用
/// damping=12 / stiffness=200 的弹簧收回。填充白色 40%。
///
/// 交互：
/// - `change_on_drag = false`（进度条）：拖动只更新视觉位置，松手/点击才 seek；
/// - `change_on_drag = true`（音量）：拖动过程中持续回调；
/// - 拖动超出两端时整条按 `tanh(rel * 2) * 12px` 偏移，松手用弹簧归零。
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

fn amll_slider_impl(
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
const AMLL_SLIDER_INSET: f32 = 6.0;
const AMLL_SLIDER_HEIGHT: f32 = 20.0;
const AMLL_MAX_BOUNCE: f32 = 12.0;

#[derive(Clone, Copy)]
enum AmllAnimKind {
    /// 指定时长、ease-out 曲线。
    EaseOut(f32),
    /// 阻尼弹簧（质量固定为 1）。
    Spring { damping: f32, stiffness: f32 },
}

#[derive(Clone, Copy)]
struct AmllAnim {
    start: Instant,
    from: f32,
    to: f32,
    kind: AmllAnimKind,
}

struct AmllSliderState {
    bounds: Cell<Bounds<Pixels>>,
    hovered: Cell<bool>,
    dragging: Cell<bool>,
    view: Cell<Option<gpui::EntityId>>,
    /// 轨道上下裁剪量：6 = 8px 可见，0 = 20px 展开。
    inset: Cell<f32>,
    inset_anim: Cell<Option<AmllAnim>>,
    /// 越界拖动时的整条水平偏移。
    bounce: Cell<f32>,
    bounce_anim: Cell<Option<AmllAnim>>,
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

fn amll_slider_state(id: &'static str) -> Rc<AmllSliderState> {
    AMLL_SLIDER_STATES.with(|states| {
        states
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| Rc::new(AmllSliderState::new()))
            .clone()
    })
}

fn notify_amll_slider_view(state: &AmllSliderState, cx: &mut App) {
    if let Some(view) = state.view.get() {
        cx.notify(view);
    }
}

struct AmllSlider {
    value: f32,
    id: &'static str,
    change_on_drag: bool,
    on_change: Rc<RefCell<Box<dyn FnMut(f32, &mut App)>>>,
}

fn start_inset_anim(state: &AmllSliderState, to: f32, kind: AmllAnimKind) {
    let from = state.inset.get();
    if (from - to).abs() < 0.01 {
        state.inset.set(to);
        state.inset_anim.set(None);
        return;
    }
    state.inset_anim.set(Some(AmllAnim {
        start: Instant::now(),
        from,
        to,
        kind,
    }));
}

fn start_bounce_anim(state: &AmllSliderState, to: f32) {
    state.bounce_anim.set(Some(AmllAnim {
        start: Instant::now(),
        from: state.bounce.get(),
        to,
        kind: AmllAnimKind::Spring {
            damping: 12.0,
            stiffness: 300.0,
        },
    }));
}

fn ease_out(x: f32) -> f32 {
    // Framer Motion 的 "easeOut"。
    crate::icons::cubic_bezier(0.0, 0.0, 0.58, 1.0, x)
}

/// 对阻尼弹簧做解析采样（初始速度 0）。
fn spring_at(from: f32, to: f32, damping: f32, stiffness: f32, t: f32) -> (f32, f32) {
    let x0 = from - to;
    if x0.abs() < 0.001 {
        return (to, 0.0);
    }
    let w0 = stiffness.max(0.0001).sqrt();
    let zeta = damping / (2.0 * stiffness.max(0.0001).sqrt());
    if zeta >= 1.0 {
        // 临界/过阻尼：用指数收敛，避免 sqrt(负数)。
        let decay = (-w0 * t).exp();
        let x = to + x0 * decay * (1.0 + w0 * t);
        let v = -w0 * w0 * t * x0 * decay;
        return (x, v);
    }
    let wd = w0 * (1.0 - zeta * zeta).sqrt();
    let a = zeta * w0;
    let decay = (-a * t).exp();
    let c = wd * t;
    let (sin, cos) = c.sin_cos();
    let b = a * x0 / wd;
    let x = to + decay * (x0 * cos + b * sin);
    // x'(t) 在 v0 = 0 时只剩 -(A·ωd + a·B)·sin(ωd·t) 项。
    let v = -decay * (x0 * wd + a * b) * sin;
    (x, v)
}

fn sample_amll_anim(anim: &AmllAnim, now: Instant) -> (f32, bool) {
    let t = now.saturating_duration_since(anim.start).as_secs_f32();
    match anim.kind {
        AmllAnimKind::EaseOut(duration) => {
            let p = (t / duration.max(0.001)).clamp(0.0, 1.0);
            (anim.from + (anim.to - anim.from) * ease_out(p), p >= 1.0)
        }
        AmllAnimKind::Spring {
            damping,
            stiffness,
        } => {
            let (x, v) = spring_at(anim.from, anim.to, damping, stiffness, t);
            let settled =
                (x - anim.to).abs() < 0.02 && v.abs() < 0.5 && t > 0.15;
            (x, settled || t > 4.0)
        }
    }
}

fn advance_amll_anim(
    anim: &Cell<Option<AmllAnim>>,
    value: &Cell<f32>,
    now: Instant,
    window: &mut Window,
) {
    if let Some(sample) = anim.get() {
        let (v, done) = sample_amll_anim(&sample, now);
        value.set(v);
        if done {
            value.set(sample.to);
            anim.set(None);
        } else {
            window.request_animation_frame();
        }
    }
}

/// 原始（未夹取）位置比例，用于 AMLL 的越界回弹。
fn raw_fraction_at(bounds: Bounds<Pixels>, position: gpui::Point<Pixels>) -> Option<f32> {
    let w = f32::from(bounds.size.width);
    if w <= 0.0 {
        return None;
    }
    Some(f32::from(position.x - bounds.left()) / w)
}

fn bounce_for(rel: f32) -> f32 {
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
            start_bounce_anim(state, 0.0);
            if state.hovered.get() {
                start_inset_anim(state, 0.0, AmllAnimKind::EaseOut(0.28));
            } else {
                start_inset_anim(
                    state,
                    AMLL_SLIDER_INSET,
                    AmllAnimKind::Spring {
                        damping: 12.0,
                        stiffness: 200.0,
                    },
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
                state_down.dragging.set(true);
                if let Some(f) = fraction_at(state_down.bounds.get(), event.position) {
                    state_down.local_value.set(Some(f));
                    if change_on_drag {
                        (change_down.borrow_mut())(f, cx);
                    }
                }
                start_inset_anim(&state_down, 0.0, AmllAnimKind::EaseOut(0.28));
                notify_amll_slider_view(&state_down, cx);
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if state_move.dragging.get() && event.pressed_button == Some(MouseButton::Left)
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
                if *hovered {
                    if !state_hover.dragging.get() {
                        start_inset_anim(
                            &state_hover,
                            0.0,
                            AmllAnimKind::EaseOut(0.28),
                        );
                    }
                } else if !state_hover.dragging.get() {
                    start_inset_anim(
                        &state_hover,
                        AMLL_SLIDER_INSET,
                        AmllAnimKind::Spring {
                            damping: 12.0,
                            stiffness: 200.0,
                        },
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

fn paint_amll_slider(
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
    let shown = state
        .local_value
        .get()
        .unwrap_or(external)
        .clamp(0.0, 1.0);

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
fn paint_dot(w: &mut Window, cx: Pixels, cy: Pixels, r: f32, color: Hsla) {
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

fn paint_slider(
    bounds: Bounds<Pixels>,
    value: f32,
    colors: SliderColors,
    active: bool,
    knob: bool,
    w: &mut Window,
) {
    let cy = bounds.center().y;
    let thickness = if knob {
        if active { 6.0 } else { 5.0 }
    } else if active {
        4.0
    } else {
        2.0
    };
    let track = Bounds::new(
        point(bounds.left(), cy - px(thickness / 2.0)),
        gpui::size(bounds.size.width, px(thickness)),
    );
    w.paint_quad(gpui::fill(track, colors.track).corner_radii(px(thickness / 2.0)));
    let fill_w = bounds.size.width * value.clamp(0.0, 1.0);
    let fill = Bounds::new(
        point(bounds.left(), cy - px(thickness / 2.0)),
        gpui::size(fill_w, px(thickness)),
    );
    if f32::from(fill_w) > 0.0 {
        w.paint_quad(gpui::fill(fill, colors.fill).corner_radii(px(thickness / 2.0)));
    }
    if knob || active {
        let r = if knob { 5.5 } else { 5.0 };
        let cx = bounds.left() + fill_w;
        let cy = bounds.center().y;
        paint_dot(w, cx, cy, r, colors.thumb);
    }
}

pub fn cover_thumb(
    theme: &Theme,
    image: Option<Arc<RenderImage>>,
    size: f32,
    radius: f32,
    placeholder: AnyElement,
) -> AnyElement {
    let inner: AnyElement = match image {
        Some(image) => img(ImageSource::Render(image))
            .size(px(size))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => placeholder,
    };
    div()
        .size(px(size))
        .rounded(px(radius))
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .flex()
        .items_center()
        .justify_center()
        .child(inner)
        .into_any_element()
}

/// 无边框封面（沉浸层大图，避免像相框）。
pub fn cover_plain(
    theme: &Theme,
    image: Option<Arc<RenderImage>>,
    size: f32,
    radius: f32,
    placeholder: AnyElement,
) -> AnyElement {
    let inner: AnyElement = match image {
        Some(image) => img(ImageSource::Render(image))
            .size(px(size))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => placeholder,
    };
    div()
        .size(px(size))
        .rounded(px(radius))
        .overflow_hidden()
        .bg(theme.surface_alt)
        .flex()
        .items_center()
        .justify_center()
        .child(inner)
        .into_any_element()
}

pub fn equalizer(theme: &Theme, playing: bool) -> AnyElement {
    let color = if playing {
        theme.playing
    } else {
        theme.text_faint
    };
    div()
        .flex()
        .items_end()
        .gap(px(2.0))
        .h(px(11.0))
        .children(
            [5.0, 10.0, 7.0]
                .into_iter()
                .map(move |h| div().w(px(2.0)).h(px(h)).rounded(px(1.0)).bg(color)),
        )
        .into_any_element()
}

pub fn empty_state(theme: &Theme, icon: AnyElement, title: &str, hint: &str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .pt(px(40.0))
        .child(icon)
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(SharedString::from(title.to_string())),
        )
        .child(
            div()
                .max_w(px(360.0))
                .text_center()
                .text_size(px(12.0))
                .line_height(px(18.0))
                .text_color(theme.text_faint)
                .child(SharedString::from(hint.to_string())),
        )
        .into_any_element()
}

/// 开/关开关（均衡器启用、频段启用）。点击时停止冒泡，避免卡片同时被选中。
pub fn switch(
    on: bool,
    accent: Hsla,
    id: impl Into<SharedString>,
    on_change: impl Fn(bool, &mut App) + 'static,
) -> AnyElement {
    let knob = if on { 20.0 } else { 2.0 };
    let id: SharedString = id.into();
    let element_id = SharedString::from(format!("switch-{}", id.as_ref()));
    div()
        .id(element_id)
        .w(px(42.0))
        .h(px(22.0))
        .rounded(px(11.0))
        .bg(if on {
            accent
        } else {
            gpui::white().opacity(0.18)
        })
        .relative()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            on_change(!on, cx);
            cx.refresh_windows();
        })
        .child(
            div()
                .absolute()
                .top(px(2.0))
                .left(px(knob))
                .size(px(18.0))
                .rounded(px(9.0))
                .bg(gpui::white()),
        )
        .into_any_element()
}

struct VSliderState {
    bounds: Cell<Bounds<Pixels>>,
    dragging: Cell<bool>,
    view: Cell<Option<gpui::EntityId>>,
}

fn notify_vslider_view(state: &VSliderState, cx: &mut App) {
    if let Some(view) = state.view.get() {
        cx.notify(view);
    }
}

/// 均衡器滑杆：回调带回 `&mut App`，可在拖动时直接更新实体并重绘。
pub fn eq_knob_slider(
    value: f32,
    colors: SliderColors,
    id: &'static str,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    Component::new(EqKnobSlider {
        value: value.clamp(0.0, 1.0),
        colors,
        id,
        on_change: Rc::new(RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

struct EqKnobSlider {
    value: f32,
    colors: SliderColors,
    id: &'static str,
    on_change: Rc<RefCell<Box<dyn FnMut(f32, &mut App)>>>,
}

impl RenderOnce for EqKnobSlider {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let state = Rc::new(SliderState {
            bounds: Cell::new(Bounds::new(
                point(px(0.), px(0.)),
                gpui::size(px(0.), px(0.)),
            )),
            hovered: Cell::new(false),
            dragging: Cell::new(false),
            view: Cell::new(None),
        });
        let on_change = self.on_change;
        let value = self.value;
        let colors = self.colors;
        let id = self.id;

        let state_down = state.clone();
        let change_down = on_change.clone();
        let state_move = state.clone();
        let change_move = on_change.clone();
        let state_up = state.clone();
        let state_hover = state.clone();
        let state_bounds = state.clone();
        let state_paint = state.clone();

        div()
            .id(SharedString::from(format!("eq-slider-{id}")))
            .h(px(24.0))
            .w_full()
            .relative()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                state_down.dragging.set(true);
                if let Some(f) = fraction_at(state_down.bounds.get(), event.position) {
                    (change_down.borrow_mut())(f, cx);
                    notify_slider_view(&state_down, cx);
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if state_move.dragging.get()
                    && event.pressed_button == Some(MouseButton::Left)
                    && let Some(f) = fraction_at(state_move.bounds.get(), event.position)
                {
                    (change_move.borrow_mut())(f, cx);
                    notify_slider_view(&state_move, cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                state_up.dragging.set(false);
            })
            .on_hover(move |hovered, _, cx| {
                state_hover.hovered.set(*hovered);
                notify_slider_view(&state_hover, cx);
            })
            .child(
                canvas(
                    move |bounds, window, _| {
                        state_bounds.bounds.set(bounds);
                        state_bounds.view.set(Some(window.current_view()));
                        bounds
                    },
                    move |_bounds, bounds, window, _| {
                        paint_slider(
                            bounds,
                            value,
                            colors,
                            state_paint.hovered.get() || state_paint.dragging.get(),
                            true,
                            window,
                        );
                    },
                )
                .size_full(),
            )
    }
}

/// 竖向音量条（竖屏播放页右侧）：2px 轨道，白色填充，hover 显示拇指。
pub fn vertical_slider(
    value: f32,
    id: &'static str,
    on_change: impl FnMut(f32) + 'static,
) -> AnyElement {
    Component::new(VSlider {
        value: value.clamp(0.0, 1.0),
        id,
        on_change: Rc::new(RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

struct VSlider {
    value: f32,
    id: &'static str,
    on_change: Rc<RefCell<Box<dyn FnMut(f32)>>>,
}

impl RenderOnce for VSlider {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let state = Rc::new(VSliderState {
            bounds: Cell::new(Bounds::new(
                point(px(0.), px(0.)),
                gpui::size(px(0.), px(0.)),
            )),
            dragging: Cell::new(false),
            view: Cell::new(None),
        });
        let on_change = self.on_change;
        let value = self.value;
        let id = self.id;
        // 拖动期间本地值立刻跟手；外部状态（200ms 轮询）随后对齐。
        let shown = Rc::new(Cell::new(value));

        let d_down = state.clone();
        let c_down = on_change.clone();
        let s_down = shown.clone();
        let d_move = state.clone();
        let c_move = on_change.clone();
        let s_move = shown.clone();
        let d_up = state.clone();
        let b_paint = state.clone();
        let b_paint2 = state.clone();
        let s_paint = shown.clone();

        div()
            .id(SharedString::from(format!("vslider-{id}")))
            .w(px(20.0))
            .h_full()
            .relative()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                d_down.dragging.set(true);
                if let Some(f) = vertical_fraction_at(d_down.bounds.get(), event.position) {
                    s_down.set(f);
                    (c_down.borrow_mut())(f);
                    notify_vslider_view(&d_down, cx);
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if d_move.dragging.get()
                    && event.pressed_button == Some(MouseButton::Left)
                    && let Some(f) = vertical_fraction_at(d_move.bounds.get(), event.position)
                {
                    s_move.set(f);
                    (c_move.borrow_mut())(f);
                    notify_vslider_view(&d_move, cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                d_up.dragging.set(false);
            })
            .child(
                canvas(
                    move |bounds, window, _| {
                        b_paint2.bounds.set(bounds);
                        b_paint2.view.set(Some(window.current_view()));
                        bounds
                    },
                    move |_bounds, bounds, window, _| {
                        paint_vertical(bounds, s_paint.get(), b_paint.dragging.get(), window);
                    },
                )
                .size_full(),
            )
    }
}

fn vertical_fraction_at(bounds: Bounds<Pixels>, position: gpui::Point<Pixels>) -> Option<f32> {
    let h = f32::from(bounds.size.height);
    if h <= 0.0 {
        return None;
    }
    let from_bottom = f32::from(bounds.bottom() - position.y) / h;
    Some(from_bottom.clamp(0.0, 1.0))
}

fn paint_vertical(bounds: Bounds<Pixels>, value: f32, dragging: bool, w: &mut Window) {
    let cx = bounds.center().x;
    let top = bounds.top();
    let height = bounds.size.height;
    let track = Bounds::new(point(cx - px(1.5), top), gpui::size(px(3.0), height));
    w.paint_quad(gpui::fill(track, gpui::white().opacity(0.28)).corner_radii(px(1.5)));
    let fill_h = height * value.clamp(0.0, 1.0);
    if f32::from(fill_h) > 0.0 {
        let fill = Bounds::new(
            point(cx - px(1.5), bounds.bottom() - fill_h),
            gpui::size(px(3.0), fill_h),
        );
        w.paint_quad(gpui::fill(fill, gpui::white().opacity(0.92)).corner_radii(px(1.5)));
    }
    if dragging {
        paint_dot(w, cx, bounds.bottom() - fill_h, 5.0, gpui::white());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::point;

    #[test]
    fn fraction_maps_edges() {
        let b = Bounds::new(point(px(10.0), px(0.0)), gpui::size(px(100.0), px(16.0)));
        assert_eq!(fraction_at(b, point(px(10.0), px(8.0))), Some(0.0));
        assert_eq!(fraction_at(b, point(px(110.0), px(8.0))), Some(1.0));
        assert_eq!(fraction_at(b, point(px(60.0), px(8.0))), Some(0.5));
    }

    #[test]
    fn amll_spring_settles_at_target() {
        // AMLL 收起弹簧：damping=12 / stiffness=200。
        let (x, v) = spring_at(AMLL_SLIDER_INSET, 0.0, 12.0, 200.0, 1.2);
        assert!(x.abs() < 0.05, "x={x}");
        assert!(v.abs() < 1.0, "v={v}");
        assert_eq!(spring_at(0.0, 0.0, 12.0, 200.0, 0.5), (0.0, 0.0));
    }

    #[test]
    fn amll_bounce_only_engages_outside_track() {
        assert_eq!(bounce_for(0.0), 0.0);
        assert_eq!(bounce_for(0.5), 0.0);
        assert_eq!(bounce_for(1.0), 0.0);
        // AMLL 的 tanh 曲线：越界一个轨道宽度时接近 12px 上限。
        assert!(bounce_for(2.0) > 11.0);
        assert!(bounce_for(-2.0) < -11.0);
    }
}
