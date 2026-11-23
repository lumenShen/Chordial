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
    MouseMoveEvent, ObjectFit, Pixels, RenderImage, RenderOnce, SharedString, Window, canvas, div,
    img, point, px,
};

use crate::theme::Theme;

mod amll;
mod media;
mod controls;
pub use amll::*;
pub use media::*;
pub use controls::*;

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
    icon_button_with_tooltip(theme, icon, size, label, label, on_click)
}

/// 与 [`icon_button`] 相同，但把元素 id 与气泡文案分开。
///
/// 用于同一个按钮会随状态改变功能的情况（如「静音 / 取消静音」）：
/// id 保持稳定，气泡文案可以随状态变化而不会重建元素状态。
pub fn icon_button_with_tooltip<F>(
    theme: &Theme,
    icon: impl IntoElement,
    size: f32,
    id: &'static str,
    tooltip: impl Into<SharedString>,
    on_click: F,
) -> AnyElement
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let radius = (size * 0.23).clamp(5.0, 8.0);
    // 外观留在内层：hover / active 伪类需要稳定 id。
    let trigger = div()
        .id(SharedString::from(format!("icon-btn-{id}")))
        .size(px(size))
        .rounded(px(radius))
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(theme.hover))
        .active(|s| s.bg(theme.active))
        .child(icon)
        .into_any_element();
    // 点击交给气泡转发：触屏长按看功能后抬手不会误触按钮。
    crate::bubble::bubble_button(
        crate::bubble::BubbleStyle::for_theme(theme),
        id,
        tooltip,
        trigger,
        on_click,
    )
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
    base.child(label).on_click(on_click).into_any_element()
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
