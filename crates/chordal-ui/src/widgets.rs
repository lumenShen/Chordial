//! Studio Neutral 基础组件：按钮、连续滑杆、封面、空态、播放指示。

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

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

/// AMLL `BouncingSlider` 等价滑杆：
/// 命中区 24px；轨道 20px 高、白色 15%，经 `inset(6px 0 round 100px)` 裁剪成
/// 8px 可见圆角条；填充为白色 40%，hover/拖动时整条展开为 20px。
pub fn amll_slider(
    value: f32,
    id: &'static str,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    Component::new(AmllSlider {
        value: value.clamp(0.0, 1.0),
        id,
        on_change: Rc::new(RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

struct AmllSlider {
    value: f32,
    id: &'static str,
    on_change: Rc<RefCell<Box<dyn FnMut(f32, &mut App)>>>,
}

impl RenderOnce for AmllSlider {
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
            .id(SharedString::from(format!("amll-slider-{id}")))
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
                        paint_amll_slider(
                            bounds,
                            value,
                            state_paint.hovered.get() || state_paint.dragging.get(),
                            window,
                        );
                    },
                )
                .size_full(),
            )
    }
}

fn paint_amll_slider(bounds: Bounds<Pixels>, value: f32, active: bool, w: &mut Window) {
    // 静息 8px 可见条（20px 轨道上下各裁 6px），hover/拖动展开为 20px。
    let full_h = f32::from(bounds.size.height).min(20.0);
    let visible_h = if active { full_h } else { 8.0 };
    let cy = bounds.top() + px(full_h / 2.0);
    let track = Bounds::new(
        point(bounds.left(), cy - px(visible_h / 2.0)),
        gpui::size(bounds.size.width, px(visible_h)),
    );
    w.paint_quad(gpui::fill(track, gpui::white().opacity(0.15)).corner_radii(px(visible_h / 2.0)));
    let fill_w = bounds.size.width * value.clamp(0.0, 1.0);
    if f32::from(fill_w) > 0.0 {
        let fill = Bounds::new(
            point(bounds.left(), cy - px(visible_h / 2.0)),
            gpui::size(fill_w, px(visible_h)),
        );
        w.paint_quad(
            gpui::fill(fill, gpui::white().opacity(0.4)).corner_radii(px(visible_h / 2.0)),
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
}
