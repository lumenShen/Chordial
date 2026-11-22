//! Studio Neutral 基础组件：按钮、连续滑杆、封面、空态、播放指示。

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Component, Hsla, ImageSource, IntoElement, MouseButton,
    MouseMoveEvent, ObjectFit, PathBuilder, Pixels, RenderImage, RenderOnce, SharedString, Window,
    canvas, div, img, point, px,
};

use crate::theme::Theme;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id(prefix: &str) -> SharedString {
    SharedString::from(format!(
        "{prefix}-{}",
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

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
    let base = div()
        .id(next_id("text-btn"))
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
    base.child(label.into())
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
        on_change: Rc::new(RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

struct SliderState {
    bounds: Cell<Bounds<Pixels>>,
    hovered: Cell<bool>,
    dragging: Cell<bool>,
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
        });
        let on_change = self.on_change;
        let value = self.value;
        let colors = self.colors;
        let height = self.height;
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
            .id(SharedString::from(format!("slider-{id}")))
            .h(px(height))
            .w_full()
            .relative()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                state_down.dragging.set(true);
                if let Some(f) = fraction_at(state_down.bounds.get(), event.position) {
                    (change_down.borrow_mut())(f);
                    cx.refresh_windows();
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if state_move.dragging.get()
                    && event.pressed_button == Some(MouseButton::Left)
                    && let Some(f) = fraction_at(state_move.bounds.get(), event.position)
                {
                    (change_move.borrow_mut())(f);
                    cx.refresh_windows();
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                state_up.dragging.set(false);
            })
            .on_hover(move |hovered, _, cx| {
                state_hover.hovered.set(*hovered);
                cx.refresh_windows();
            })
            .child(
                canvas(
                    move |bounds, _, _| {
                        state_bounds.bounds.set(bounds);
                        bounds
                    },
                    move |_bounds, bounds, window, _| {
                        paint_slider(
                            bounds,
                            value,
                            colors,
                            state_paint.hovered.get() || state_paint.dragging.get(),
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
                    cx.refresh_windows();
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if state_move.dragging.get()
                    && event.pressed_button == Some(MouseButton::Left)
                    && let Some(f) = fraction_at(state_move.bounds.get(), event.position)
                {
                    (change_move.borrow_mut())(f, cx);
                    cx.refresh_windows();
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                state_up.dragging.set(false);
            })
            .on_hover(move |hovered, _, cx| {
                state_hover.hovered.set(*hovered);
                cx.refresh_windows();
            })
            .child(
                canvas(
                    move |bounds, _, _| {
                        state_bounds.bounds.set(bounds);
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

fn paint_slider(
    bounds: Bounds<Pixels>,
    value: f32,
    colors: SliderColors,
    active: bool,
    w: &mut Window,
) {
    let cy = bounds.center().y;
    let thickness = if active { 4.0 } else { 2.0 };
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
    if active {
        let r = 5.0;
        let cx = bounds.left() + fill_w;
        let cy = bounds.center().y;
        let mut b = PathBuilder::fill();
        b.move_to(point(cx, cy - px(r)));
        for i in 1..=24 {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 / 24.0 * std::f32::consts::TAU;
            b.line_to(point(cx + px(r * a.cos()), cy + px(r * a.sin())));
        }
        b.close();
        if let Ok(path) = b.build() {
            w.paint_path(path, colors.thumb);
        }
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
