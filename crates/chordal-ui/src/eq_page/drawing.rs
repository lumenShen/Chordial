//! EQ 页面支撑代码：drawing（由 eq_page.rs 拆分，行为不变）。

use super::*;

// ================= 绘制 =================

pub(super) fn eq_rect(window: &mut Window, x: f32, y: f32, w: f32, h: f32, color: Hsla) {
    window.paint_quad(fill(
        Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
        color,
    ));
}

pub(super) fn eq_circle(window: &mut Window, cx: f32, cy: f32, r: f32, color: Hsla) {
    // 圆点用圆角 quad 表示：与路径相比不会触发全屏 MSAA 路径通道。
    window.paint_quad(
        fill(
            Bounds::new(
                point(px(cx - r), px(cy - r)),
                size(px(r * 2.0), px(r * 2.0)),
            ),
            color,
        )
        .corner_radii(px(r)),
    );
}

pub(super) fn eq_polyline(window: &mut Window, points: &[(f32, f32)], thickness: f32, color: Hsla) {
    if points.len() < 2 {
        return;
    }
    let mut path = PathBuilder::stroke(px(thickness.max(0.5)));
    path.move_to(point(px(points[0].0), px(points[0].1)));
    for &(x, y) in &points[1..] {
        path.line_to(point(px(x), px(y)));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// 频响网格、曲线与频段控制点。
pub(super) fn paint_eq_graph(
    bounds: Bounds<gpui::Pixels>,
    settings: &EqSettings,
    selected: Option<usize>,
    compact: bool,
    pal: &EqPalette,
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    if width <= 2.0 || height <= 2.0 {
        return;
    }
    let ox = f32::from(bounds.left());
    let oy = f32::from(bounds.top());
    let geom = if compact {
        GraphGeom::compact(width, height)
    } else {
        GraphGeom::new(width, height)
    };

    for step in 0..=8 {
        let db = 24.0 - step as f32 * 6.0;
        let y = oy + geom.y(db);
        eq_rect(
            window,
            ox + geom.left,
            y,
            geom.width,
            1.0,
            if step == 4 { pal.grid_major } else { pal.grid },
        );
    }
    for freq in [
        31.5f32, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
    ] {
        let x = ox + geom.x(freq);
        eq_rect(
            window,
            x,
            oy + 10.0,
            1.0,
            (f32::from(bounds.size.height) - 30.0).max(1.0),
            if (freq - 1_000.0).abs() < 1.0 {
                pal.grid_major
            } else {
                pal.grid
            },
        );
    }

    let signal = if settings.enabled {
        pal.accent
    } else {
        pal.text_faint
    };
    let global = settings.input_gain_db + settings.output_gain_db;
    let points = eq_curve_points(settings, &geom, ox, oy, width, height, compact);
    // 一条折线就是整条曲线：之前每个采样点建 4 边形 + 一次 paint_path，
    // 260 点 × 3 层 = 780 条路径，每帧都会触发多次全屏 MSAA 路径通道。
    eq_polyline(window, points.as_slice(), 1.8, signal);
    // 曲线辉光：先宽后窄，让蓝色在深色面板上有「发光」的层次而不是硬描边。
    eq_polyline(window, points.as_slice(), 6.0, signal.opacity(0.07));
    eq_polyline(window, points.as_slice(), 3.0, signal.opacity(0.16));

    let handle = if compact { 17.0 } else { 14.0 };
    for (index, band) in settings.bands.iter().enumerate() {
        let x = ox + geom.x(band.freq_hz);
        let y = oy + geom.y(band.gain_db + global);
        let color = if band.enabled { signal } else { pal.text_faint };
        // 控制点到 0 dB 的细线，帮助读取提升/衰减幅度。
        if band.enabled {
            let base_y = oy + geom.y(global);
            eq_rect(
                window,
                x - 0.5,
                y.min(base_y),
                1.0,
                (base_y - y).abs().max(1.0),
                pal.grid,
            );
        }
        if selected == Some(index) {
            eq_circle(window, x, y, handle * 0.5 + 5.0, pal.text.opacity(0.16));
            eq_circle(window, x, y, handle * 0.5 + 2.0, pal.text.opacity(0.85));
        }
        eq_circle(window, x, y, handle * 0.5, pal.inset);
        eq_circle(window, x, y, handle * 0.5 - 4.0, color);
    }
    let _ = width;
}

// ================= 滑杆 =================

pub(super) struct SliderState {
    bounds: Cell<Bounds<gpui::Pixels>>,
    dragging: Cell<bool>,
    view: Cell<Option<gpui::EntityId>>,
}

pub(super) fn notify_slider_view(state: &SliderState, cx: &mut App) {
    if let Some(view) = state.view.get() {
        cx.notify(view);
    }
}

pub(super) fn eq_fraction_at(
    bounds: Bounds<gpui::Pixels>,
    position: gpui::Point<gpui::Pixels>,
) -> Option<f32> {
    let w = f32::from(bounds.size.width);
    if w <= 0.0 {
        return None;
    }
    Some((f32::from(position.x - bounds.left()) / w).clamp(0.0, 1.0))
}

/// 双极性滑杆（增益）：填充从 0 点向两侧展开，避免负增益看起来像「剩余量」。
pub(super) struct BipolarSlider {
    value: f32,
    colors: SliderColors,
    tick: Hsla,
    height: f32,
    thumb_r: f32,
    id: &'static str,
    on_change: Rc<std::cell::RefCell<Box<dyn FnMut(f32, &mut App)>>>,
}

pub(super) fn eq_bipolar_slider(
    value: f32,
    colors: SliderColors,
    tick: Hsla,
    height: f32,
    thumb_r: f32,
    id: &'static str,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    Component::new(BipolarSlider {
        value: value.clamp(0.0, 1.0),
        colors,
        tick,
        height,
        thumb_r,
        id,
        on_change: Rc::new(std::cell::RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

impl RenderOnce for BipolarSlider {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let state = Rc::new(SliderState {
            bounds: Cell::new(Bounds::new(point(px(0.0), px(0.0)), size(px(0.0), px(0.0)))),
            dragging: Cell::new(false),
            view: Cell::new(None),
        });
        let on_change = self.on_change;
        let value = self.value;
        let colors = self.colors;
        let tick = self.tick;
        let height = self.height;
        let thumb_r = self.thumb_r;
        let id = self.id;

        let state_down = state.clone();
        let change_down = on_change.clone();
        let state_move = state.clone();
        let change_move = on_change.clone();
        let state_up = state.clone();
        let state_paint_bounds = state.clone();
        let state_paint = state.clone();

        div()
            .id(SharedString::from(format!("eq-bipolar-{id}")))
            .h(px(height))
            .w_full()
            .relative()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                state_down.dragging.set(true);
                if let Some(f) = eq_fraction_at(state_down.bounds.get(), event.position) {
                    (change_down.borrow_mut())(f, cx);
                    notify_slider_view(&state_down, cx);
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if state_move.dragging.get()
                    && event.pressed_button == Some(MouseButton::Left)
                    && let Some(f) = eq_fraction_at(state_move.bounds.get(), event.position)
                {
                    (change_move.borrow_mut())(f, cx);
                    notify_slider_view(&state_move, cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                state_up.dragging.set(false);
            })
            .child(
                canvas(
                    move |bounds, _, _| {
                        state_paint_bounds.bounds.set(bounds);
                        bounds
                    },
                    move |_bounds, bounds, window, _| {
                        paint_bipolar(
                            bounds,
                            value,
                            colors,
                            tick,
                            state_paint.dragging.get(),
                            thumb_r,
                            window,
                        );
                    },
                )
                .size_full(),
            )
    }
}

pub(super) fn paint_bipolar(
    bounds: Bounds<gpui::Pixels>,
    value: f32,
    colors: SliderColors,
    tick: Hsla,
    active: bool,
    thumb_r: f32,
    window: &mut Window,
) {
    let cy = bounds.center().y;
    let thickness = if active { 6.0 } else { 4.0 };
    let track = Bounds::new(
        point(bounds.left(), cy - px(thickness / 2.0)),
        size(bounds.size.width, px(thickness)),
    );
    window.paint_quad(fill(track, colors.track).corner_radii(px(thickness / 2.0)));

    let width = f32::from(bounds.size.width);
    let center_x = f32::from(bounds.left()) + width / 2.0;
    eq_rect(
        window,
        center_x - 0.5,
        f32::from(cy) - 6.0,
        1.0,
        12.0,
        tick.opacity(0.55),
    );

    let value_x = f32::from(bounds.left()) + width * value.clamp(0.0, 1.0);
    let (x0, x1) = if value_x >= center_x {
        (center_x, value_x)
    } else {
        (value_x, center_x)
    };
    if x1 - x0 > 0.5 {
        let fill_rect = Bounds::new(
            point(px(x0), cy - px(thickness / 2.0)),
            size(px(x1 - x0), px(thickness)),
        );
        window.paint_quad(fill(fill_rect, colors.fill).corner_radii(px(thickness / 2.0)));
    }
    eq_circle(window, value_x, f32::from(cy), thumb_r + 1.5, colors.track);
    eq_circle(window, value_x, f32::from(cy), thumb_r, colors.thumb);
}

// ================= 频段推子 =================

pub(super) struct FaderState {
    bounds: Cell<Bounds<gpui::Pixels>>,
    dragging: Cell<bool>,
    view: Cell<Option<gpui::EntityId>>,
}

pub(super) fn notify_fader_view(state: &FaderState, cx: &mut App) {
    if let Some(view) = state.view.get() {
        cx.notify(view);
    }
}

/// 桌面端竖向频段推子：中点 0 dB，向上提升、向下衰减。
pub(super) fn eq_band_fader(
    index: usize,
    freq_hz: f32,
    gain_db: f32,
    enabled: bool,
    uses_gain: bool,
    kind_label: &'static str,
    selected: bool,
    pal: &EqPalette,
    on_select: impl Fn(&mut App) + 'static,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    let state = Rc::new(FaderState {
        bounds: Cell::new(Bounds::new(point(px(0.0), px(0.0)), size(px(0.0), px(0.0)))),
        dragging: Cell::new(false),
        view: Cell::new(None),
    });
    let change = Rc::new(std::cell::RefCell::new(
        Box::new(on_change) as Box<dyn FnMut(f32, &mut App)>
    ));
    let select = Rc::new(on_select);
    let shown = Rc::new(Cell::new(gain_db));
    let pal_paint = pal.clone();

    let down_state = state.clone();
    let down_select = select.clone();
    let down_change = change.clone();
    let down_shown = shown.clone();
    let move_state = state.clone();
    let move_change = change.clone();
    let move_shown = shown.clone();
    let up_state = state.clone();
    let paint_state = state.clone();
    let paint_state2 = state.clone();
    let paint_shown = shown.clone();

    let cursor = if uses_gain {
        gpui::CursorStyle::ResizeUpDown
    } else {
        gpui::CursorStyle::PointingHand
    };

    div()
        .id(SharedString::from(format!("eq-fader-{index}")))
        .w(px(50.0))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(3.0))
        .rounded(px(9.0))
        .cursor(cursor)
        .hover(|s| s.bg(pal.chip))
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            (down_select)(cx);
            if uses_gain {
                if let Some(db) = fader_db_at(down_state.bounds.get(), event.position) {
                    down_state.dragging.set(true);
                    down_shown.set(db);
                    (down_change.borrow_mut())(db, cx);
                    notify_fader_view(&down_state, cx);
                }
            }
        })
        .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
            if !move_state.dragging.get()
                || !uses_gain
                || event.pressed_button != Some(MouseButton::Left)
            {
                return;
            }
            if let Some(db) = fader_db_at(move_state.bounds.get(), event.position) {
                move_shown.set(db);
                (move_change.borrow_mut())(db, cx);
                notify_fader_view(&move_state, cx);
            }
        })
        .on_mouse_up(MouseButton::Left, move |_, _, _| {
            up_state.dragging.set(false);
        })
        .child(
            div()
                .pt(px(5.0))
                .text_size(px(9.0))
                .font_family(pal.mono.clone())
                .text_color(if selected { pal.accent } else { pal.text_dim })
                .child(SharedString::from(fmt_freq(freq_hz))),
        )
        .child(
            div().w(px(38.0)).flex_1().min_h_0().child(
                canvas(
                    move |bounds, window, _| {
                        paint_state.bounds.set(bounds);
                        paint_state.view.set(Some(window.current_view()));
                        bounds
                    },
                    move |_bounds, bounds, window, _| {
                        paint_fader(
                            bounds,
                            paint_shown.get(),
                            selected,
                            enabled,
                            uses_gain,
                            &pal_paint,
                            paint_state2.dragging.get(),
                            window,
                        );
                    },
                )
                .size_full(),
            ),
        )
        .child(
            div()
                .pb(px(5.0))
                .text_size(px(9.5))
                .font_family(pal.mono.clone())
                .text_color(if !uses_gain || !enabled {
                    pal.text_faint
                } else if selected {
                    pal.accent
                } else {
                    pal.text_dim
                })
                .child(SharedString::from(if uses_gain {
                    db_num(shown.get())
                } else {
                    kind_label.to_string()
                })),
        )
        .into_any_element()
}

pub(super) fn fader_db_at(bounds: Bounds<gpui::Pixels>, position: gpui::Point<gpui::Pixels>) -> Option<f32> {
    let h = f32::from(bounds.size.height);
    if h <= 12.0 {
        return None;
    }
    let center = f32::from(bounds.top()) + h / 2.0;
    let half = (h / 2.0 - 8.0).max(1.0);
    let db = (center - f32::from(position.y)) / half * 24.0;
    Some(db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB))
}

pub(super) fn paint_fader(
    bounds: Bounds<gpui::Pixels>,
    gain_db: f32,
    selected: bool,
    enabled: bool,
    uses_gain: bool,
    pal: &EqPalette,
    dragging: bool,
    window: &mut Window,
) {
    let h = f32::from(bounds.size.height);
    if h <= 12.0 {
        return;
    }
    let x = f32::from(bounds.center().x);
    let top = f32::from(bounds.top()) + 6.0;
    let bottom = f32::from(bounds.bottom()) - 6.0;
    let center = (top + bottom) / 2.0;
    let alpha = if enabled { 1.0 } else { 0.38 };
    let track_color = pal
        .fader_track
        .opacity(if selected { 1.0 } else { 0.75 } * alpha);
    eq_rect(window, x - 1.0, top, 2.0, bottom - top, track_color);
    eq_rect(
        window,
        x - 10.0,
        center - 0.5,
        20.0,
        1.0,
        pal.grid_major.opacity(alpha),
    );

    let db = gain_db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB);
    let half = (h / 2.0 - 8.0).max(1.0);
    let y = center - db / 24.0 * half;
    if uses_gain {
        let (y0, y1) = if y <= center {
            (y, center)
        } else {
            (center, y)
        };
        if y1 - y0 > 0.5 {
            eq_rect(
                window,
                x - 1.5,
                y0,
                3.0,
                y1 - y0,
                if enabled {
                    pal.accent.opacity(0.85)
                } else {
                    pal.text_faint.opacity(0.4)
                },
            );
        }
        if selected || dragging {
            eq_circle(window, x, y, 8.0, pal.accent.opacity(0.16 * alpha));
        }
        eq_circle(
            window,
            x,
            y,
            5.0,
            if enabled { pal.accent } else { pal.text_faint },
        );
        if selected {
            eq_circle(window, x, y, 2.0, pal.inset);
        }
    } else {
        eq_circle(
            window,
            x,
            center,
            7.0,
            pal.fader_track.opacity(0.55 * alpha),
        );
        eq_circle(window, x, center, 3.0, pal.text_faint.opacity(alpha));
    }
}
