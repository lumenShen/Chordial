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
///
/// `origins` 与 `settings.bands` 一一对应：耳机档案用弱化的文字色，内置
/// 预设用半透明强调色，自定义频段用完整强调色，一眼能看出节点属于哪层。
#[allow(clippy::too_many_arguments)] // 绘制入口：设置 / 来源 / 选中态 / 几何 / 缓存槽位
pub(super) fn paint_eq_graph(
    bounds: Bounds<gpui::Pixels>,
    settings: &EqSettings,
    origins: &[BandOrigin],
    selected: Option<EqSelection>,
    compact: bool,
    slot: usize,
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
    let points = eq_curve_points(settings, &geom, ox, oy, width, height, compact, slot);
    // 一条折线就是整条曲线：之前每个采样点建 4 边形 + 一次 paint_path，
    // 260 点 × 3 层 = 780 条路径，每帧都会触发多次全屏 MSAA 路径通道。
    eq_polyline(window, points.as_slice(), 1.8, signal);
    // 曲线辉光：先宽后窄，让蓝色在深色面板上有「发光」的层次而不是硬描边。
    eq_polyline(window, points.as_slice(), 6.0, signal.opacity(0.07));
    eq_polyline(window, points.as_slice(), 3.0, signal.opacity(0.16));

    let selected_index = selected.and_then(|selection| {
        origins.iter().position(|origin| match (origin, selection) {
            (BandOrigin::Profile(_), EqSelection::Profile) => true,
            (BandOrigin::Block { block, .. }, EqSelection::Block(selected)) => *block == selected,
            (BandOrigin::Custom(index), EqSelection::Custom(selected)) => *index == selected,
            _ => false,
        })
    });

    let handle = if compact { 17.0 } else { 14.0 };
    for (index, band) in settings.bands.iter().enumerate() {
        let x = ox + geom.x(band.freq_hz);
        let y = oy + geom.y(band.gain_db + global);
        let origin = origins.get(index).copied();
        let color = if !band.enabled {
            pal.text_faint
        } else {
            match origin {
                Some(BandOrigin::Profile(_)) => pal.text_dim,
                Some(BandOrigin::Block { .. }) => signal.opacity(0.72),
                Some(BandOrigin::Custom(_)) | None => signal,
            }
        };
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
        if selected_index == Some(index) {
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
