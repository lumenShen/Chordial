//! widgets：controls（由 widgets.rs 拆分，行为不变）。

use super::*;

pub(super) struct VSliderState {
    bounds: Cell<Bounds<Pixels>>,
    dragging: Cell<bool>,
    view: Cell<Option<gpui::EntityId>>,
}

pub(super) fn notify_vslider_view(state: &VSliderState, cx: &mut App) {
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

pub(super) struct EqKnobSlider {
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

pub(super) struct VSlider {
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

pub(super) fn vertical_fraction_at(bounds: Bounds<Pixels>, position: gpui::Point<Pixels>) -> Option<f32> {
    let h = f32::from(bounds.size.height);
    if h <= 0.0 {
        return None;
    }
    let from_bottom = f32::from(bounds.bottom() - position.y) / h;
    Some(from_bottom.clamp(0.0, 1.0))
}

pub(super) fn paint_vertical(bounds: Bounds<Pixels>, value: f32, dragging: bool, w: &mut Window) {
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
