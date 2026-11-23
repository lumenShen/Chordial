//! EQ 页面区块：desktop_graph（由 eq_page.rs 拆分，行为不变）。

use super::*;
use super::controls::*;
use super::drawing::*;

impl ChordialApp {
    pub(super) fn eq_graph(
        &self,
        pal: &EqPalette,
        width: f32,
        height: f32,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let geom = if compact {
            GraphGeom::compact(width, height)
        } else {
            GraphGeom::new(width, height)
        };
        let bounds_cell = Rc::new(Cell::new(Bounds::new(
            point(px(0.0), px(0.0)),
            size(px(0.0), px(0.0)),
        )));
        let dragging = Rc::new(Cell::new(false));
        let paint_settings = self.eq.settings.clone();
        let paint_selected = self.eq.selected;
        let paint_pal = pal.clone();
        let canvas_bounds = bounds_cell.clone();
        let canvas_el = canvas(
            move |bounds, _, _| {
                canvas_bounds.set(bounds);
                bounds
            },
            move |_bounds, bounds, window, _| {
                paint_eq_graph(
                    bounds,
                    &paint_settings,
                    paint_selected,
                    compact,
                    &paint_pal,
                    window,
                );
            },
        )
        .size_full();

        let weak = cx.entity().downgrade();
        let down_bounds = bounds_cell.clone();
        let down_drag = dragging.clone();
        let move_bounds = bounds_cell.clone();
        let move_drag = dragging.clone();
        let up_drag = dragging.clone();
        let right_bounds = bounds_cell.clone();
        let weak_down = weak.clone();
        let weak_move = weak.clone();
        let weak_right = weak.clone();
        let hit_radius = if compact { 28.0 } else { 18.0 };
        let add_limit = EQ_MAX_BANDS;

        let mut card = div()
            .id("eq-graph")
            .relative()
            .w_full()
            .h(px(height))
            .flex_none()
            .rounded(px(14.0))
            .bg(pal.inset)
            .border_1()
            .border_color(pal.border)
            .overflow_hidden()
            .cursor(gpui::CursorStyle::Crosshair)
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                let bounds = down_bounds.get();
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if w <= 1.0 || h <= 1.0 {
                    return;
                }
                let local_x = f32::from(event.position.x - bounds.left());
                let local_y = f32::from(event.position.y - bounds.top());
                let _ = weak_down.update(cx, |app, cx| {
                    let geom = if compact {
                        GraphGeom::compact(w, h)
                    } else {
                        GraphGeom::new(w, h)
                    };
                    let global = app.eq.settings.input_gain_db + app.eq.settings.output_gain_db;
                    let nearest = app
                        .eq
                        .settings
                        .bands
                        .iter()
                        .enumerate()
                        .map(|(index, band)| {
                            let dx = geom.x(band.freq_hz) - local_x;
                            let dy = geom.y(band.gain_db + global) - local_y;
                            (index, dx * dx + dy * dy)
                        })
                        .filter(|(_, distance)| *distance <= hit_radius * hit_radius)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(index, _)| index);
                    if let Some(index) = nearest {
                        app.eq.selected = Some(index);
                        app.eq.type_menu_open = false;
                        down_drag.set(true);
                        cx.notify();
                    } else if app.eq.settings.bands.len() < add_limit {
                        let freq = geom.freq_at(local_x);
                        let gain = geom.db_at(local_y) - global;
                        app.eq.settings.bands.push(chordal_player::EqBand::new(
                            freq,
                            gain.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB),
                            0.71,
                        ));
                        app.eq.selected = Some(app.eq.settings.bands.len() - 1);
                        app.eq.touch();
                        down_drag.set(true);
                        app.apply_eq(cx);
                    }
                });
            })
            .on_mouse_move(move |event, _, cx| {
                if !move_drag.get() || event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let bounds = move_bounds.get();
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if w <= 1.0 || h <= 1.0 {
                    return;
                }
                let local_x = f32::from(event.position.x - bounds.left());
                let local_y = f32::from(event.position.y - bounds.top());
                let _ = weak_move.update(cx, |app, cx| {
                    let Some(index) = app.eq.selected else { return };
                    let geom = if compact {
                        GraphGeom::compact(w, h)
                    } else {
                        GraphGeom::new(w, h)
                    };
                    let global = app.eq.settings.input_gain_db + app.eq.settings.output_gain_db;
                    if let Some(band) = app.eq.settings.bands.get_mut(index) {
                        band.freq_hz = geom.freq_at(local_x).clamp(20.0, 20_000.0);
                        band.gain_db =
                            (geom.db_at(local_y) - global).clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB);
                        app.eq.touch();
                        app.apply_eq(cx);
                    }
                });
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                up_drag.set(false);
            })
            .on_mouse_down(MouseButton::Right, move |event, _, cx| {
                let bounds = right_bounds.get();
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if w <= 1.0 || h <= 1.0 {
                    return;
                }
                let local_x = f32::from(event.position.x - bounds.left());
                let local_y = f32::from(event.position.y - bounds.top());
                let _ = weak_right.update(cx, |app, cx| {
                    let geom = if compact {
                        GraphGeom::compact(w, h)
                    } else {
                        GraphGeom::new(w, h)
                    };
                    let global = app.eq.settings.input_gain_db + app.eq.settings.output_gain_db;
                    let nearest = app
                        .eq
                        .settings
                        .bands
                        .iter()
                        .enumerate()
                        .map(|(index, band)| {
                            let dx = geom.x(band.freq_hz) - local_x;
                            let dy = geom.y(band.gain_db + global) - local_y;
                            (index, dx * dx + dy * dy)
                        })
                        .filter(|(_, distance)| *distance <= 20.0 * 20.0)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(index, _)| index);
                    if let Some(index) = nearest {
                        app.eq.settings.bands.remove(index);
                        app.eq.selected = if app.eq.settings.bands.is_empty() {
                            None
                        } else {
                            Some(index.min(app.eq.settings.bands.len() - 1))
                        };
                        app.eq.touch();
                        app.apply_eq(cx);
                    }
                });
            })
            .child(div().absolute().inset_0().child(canvas_el));

        // dB 标尺与频率轴。
        let db_ticks: &[f32] = if compact {
            &[24.0, 12.0, 0.0, -12.0, -24.0]
        } else {
            &[24.0, 18.0, 12.0, 6.0, 0.0, -6.0, -12.0, -18.0, -24.0]
        };
        for db in db_ticks {
            card = card.child(
                div()
                    .absolute()
                    .left(px(4.0))
                    .top(px(geom.y(*db) - 5.0))
                    .w(px(if compact { 24.0 } else { 28.0 }))
                    .text_size(px(8.0))
                    .font_family(pal.mono.clone())
                    .text_color(if *db == 0.0 {
                        pal.text_dim
                    } else {
                        pal.text_faint
                    })
                    .text_align(gpui::TextAlign::Right)
                    .child(SharedString::from(if *db > 0.0 {
                        format!("+{}", *db as i32)
                    } else {
                        format!("{}", *db as i32)
                    })),
            );
        }
        let freq_ticks: &[f32] = if compact {
            &[31.5, 125.0, 500.0, 2_000.0, 8_000.0, 16_000.0]
        } else {
            &[
                31.5, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
            ]
        };
        for freq in freq_ticks {
            card = card.child(
                div()
                    .absolute()
                    .left(px(geom.x(*freq) - 16.0))
                    .bottom(px(4.0))
                    .w(px(32.0))
                    .text_size(px(8.0))
                    .font_family(pal.mono.clone())
                    .text_color(pal.text_faint)
                    .text_center()
                    .child(SharedString::from(fmt_freq(*freq))),
            );
        }
        if !compact {
            card = card.child(
                div()
                    .absolute()
                    .left(px(4.0))
                    .top(px(2.0))
                    .text_size(px(7.5))
                    .text_color(pal.text_faint)
                    .child("dB"),
            );
        }

        if self.eq.settings.bands.is_empty() {
            card = card.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(5.0))
                    .child(icons::icon(Icon::Plus, 16.0, pal.accent))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(pal.text_dim)
                            .child("点按曲线添加频段"),
                    ),
            );
        }

        if let Some(index) = self.eq.selected
            && let Some(band) = self.eq.settings.bands.get(index)
        {
            let global = self.eq.settings.input_gain_db + self.eq.settings.output_gain_db;
            let x = geom.x(band.freq_hz);
            let y = geom.y(band.gain_db + global);
            let tip = band_summary(index, band);
            card = card.child(
                div()
                    .absolute()
                    .left(px((x - 82.0).clamp(4.0, (width - 168.0).max(4.0))))
                    .top(px((y - 32.0).max(3.0)))
                    .w(px(164.0))
                    .h(px(23.0))
                    .rounded(px(12.0))
                    .bg(pal.panel)
                    .border_1()
                    .border_color(pal.border_strong)
                    .shadow(vec![BoxShadow {
                        color: pal.shadow,
                        offset: point(px(0.0), px(6.0)),
                        blur_radius: px(16.0),
                        spread_radius: px(0.0),
                    }])
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(9.0))
                    .font_family(pal.mono.clone())
                    .text_color(pal.text)
                    .child(SharedString::from(tip)),
            );
        }
        card.into_any_element()
    }

    pub(super) fn eq_inspector_desktop(
        &self,
        pal: &EqPalette,
        main_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut body = div()
            .id("eq-inspector")
            .h(px(main_h))
            .flex_none()
            .w(px(INSPECTOR_W))
            .border_l_1()
            .border_color(pal.divider)
            .px(px(14.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .overflow_y_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll());

        let selected = self.eq.selected.and_then(|index| {
            self.eq
                .settings
                .bands
                .get(index)
                .map(|band| (index, band.clone()))
        });

        body = body.child(
            div()
                .h(px(18.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("滤波器检视器"),
                )
                .when_some(selected.as_ref().map(|(index, _)| *index), |d, index| {
                    let weak_toggle = weak.clone();
                    let weak_remove = weak.clone();
                    let enabled = selected
                        .as_ref()
                        .map(|(_, band)| band.enabled)
                        .unwrap_or(false);
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .id("eq-inspector-enabled")
                                    .h(px(20.0))
                                    .px(px(6.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(3.0))
                                    .cursor_pointer()
                                    .bg(if enabled { pal.accent_soft } else { pal.chip })
                                    .text_size(px(9.0))
                                    .text_color(if enabled { pal.accent } else { pal.text_faint })
                                    .hover(|s| s.bg(pal.chip_hover))
                                    .child(div().size(px(4.0)).rounded(px(2.0)).bg(if enabled {
                                        pal.accent
                                    } else {
                                        pal.text_faint
                                    }))
                                    .child(if enabled { "启用" } else { "旁路" })
                                    .on_click(move |_, _, cx| {
                                        let _ = weak_toggle.update(cx, |app, cx| {
                                            if let Some(band) = app.eq.settings.bands.get_mut(index)
                                            {
                                                band.enabled = !enabled;
                                                app.eq.touch();
                                                app.apply_eq(cx);
                                            }
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .id("eq-inspector-remove")
                                    .h(px(20.0))
                                    .px(px(6.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(3.0))
                                    .cursor_pointer()
                                    .text_size(px(9.0))
                                    .text_color(pal.danger)
                                    .hover(|s| s.bg(pal.danger_soft))
                                    .child(icons::icon(Icon::Close, 9.0, pal.danger))
                                    .child("移除")
                                    .on_click(move |_, _, cx| {
                                        let _ = weak_remove.update(cx, |app, cx| {
                                            app.eq.selected = Some(index);
                                            app.remove_eq_band(cx);
                                        });
                                    }),
                            ),
                    )
                }),
        );

        if let Some((index, band)) = selected.clone() {
            body = body.child(
                div()
                    .w_full()
                    .rounded(px(10.0))
                    .bg(pal.chip)
                    .border_1()
                    .border_color(pal.border)
                    .p(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(8.5))
                                    .text_color(pal.text_faint)
                                    .child(SharedString::from(format!("频段 {:02}", index + 1))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(3.0))
                                    .child(
                                        div()
                                            .text_size(px(20.0))
                                            .font_family(pal.mono.clone())
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(pal.text)
                                            .child(SharedString::from(fmt_freq(band.freq_hz))),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(8.5))
                                            .text_color(pal.text_faint)
                                            .child("Hz"),
                                    ),
                            ),
                    )
                    .child(div().w(px(1.0)).h(px(32.0)).bg(pal.divider))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(8.5))
                                    .text_color(pal.text_faint)
                                    .child("增益"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(3.0))
                                    .child(
                                        div()
                                            .text_size(px(20.0))
                                            .font_family(pal.mono.clone())
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(if band.enabled {
                                                pal.accent
                                            } else {
                                                pal.text_faint
                                            })
                                            .child(SharedString::from(if band.kind.uses_gain() {
                                                db_num(band.gain_db)
                                            } else {
                                                "—".into()
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(8.5))
                                            .text_color(pal.text_faint)
                                            .child("dB"),
                                    ),
                            ),
                    ),
            );

            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .child(
                        div()
                            .text_size(px(9.0))
                            .text_color(pal.text_faint)
                            .child("滤波器类型"),
                    )
                    .child(self.eq_kind_grid(pal, index, band.kind, cx)),
            );

            body = body.child(self.eq_params_block(pal, index, &band, false, cx));
            body = body.child(eq_divider(pal));
        } else {
            body = body.child(
                div()
                    .w_full()
                    .rounded(px(10.0))
                    .bg(pal.chip)
                    .border_1()
                    .border_color(pal.border)
                    .p(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(pal.text_dim)
                            .child("未选择频段"),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .line_height(px(15.0))
                            .text_color(pal.text_faint)
                            .child("在曲线或推子上点选一个频段；点击曲线空白处可以直接新建。"),
                    ),
            );
            body = body.child(eq_divider(pal));
        }

        body = body.child(div().flex_1());
        body = body.child(self.eq_master_block(pal, false, cx));
        body.into_any_element()
    }

    pub(super) fn eq_kind_grid(
        &self,
        pal: &EqPalette,
        _index: usize,
        active: FilterKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut row = div().w_full().flex().gap(px(4.0));
        for kind in FilterKind::ALL {
            let weak = weak.clone();
            let is_active = active == kind;
            row = row.child(
                div()
                    .id(SharedString::from(format!("eq-kind-{}", kind.label())))
                    .h(px(28.0))
                    .flex_1()
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(if is_active {
                        pal.accent_line
                    } else {
                        pal.border
                    })
                    .bg(if is_active { pal.accent_soft } else { pal.chip })
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_size(px(9.5))
                    .text_color(if is_active { pal.accent } else { pal.text_dim })
                    .hover(move |s| {
                        s.bg(if is_active {
                            pal.accent_soft
                        } else {
                            pal.chip_hover
                        })
                    })
                    .child(kind.label())
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.set_eq_filter_kind(kind, cx);
                        });
                    }),
            );
        }
        row.into_any_element()
    }

    pub(super) fn eq_params_block(
        &self,
        pal: &EqPalette,
        _index: usize,
        band: &chordal_player::EqBand,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let colors = pal.slider_colors();
        let slider_h = if compact { 30.0 } else { 26.0 };
        let mut block =
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(if compact { px(8.0) } else { px(6.0) });

        let freq_slider = {
            let weak = weak.clone();
            widgets::eq_knob_slider(
                freq_to_slider(band.freq_hz),
                colors,
                "eq-freq",
                move |value: f32, cx: &mut App| {
                    let freq = slider_to_freq(value);
                    let _ = weak.update(cx, |app, cx| app.set_eq_freq(freq, cx));
                },
            )
        };
        block = block.child(eq_param_block(
            pal,
            "频率",
            format!("{} Hz", fmt_freq(band.freq_hz)),
            freq_slider,
        ));

        if band.kind.uses_gain() {
            let gain = band.gain_db;
            let weak = weak.clone();
            let gain_slider = eq_bipolar_slider(
                ((gain + 24.0) / 48.0).clamp(0.0, 1.0),
                colors,
                pal.text_faint,
                slider_h,
                5.5,
                "eq-gain",
                move |value: f32, cx: &mut App| {
                    let db = value * 48.0 - 24.0;
                    let _ = weak.update(cx, |app, cx| app.set_eq_gain(db, cx));
                },
            );
            block = block.child(eq_param_block(
                pal,
                "增益",
                format!("{} dB", db_num(gain)),
                gain_slider,
            ));
        }

        let q = band.q;
        let weak_q = weak.clone();
        let q_slider = widgets::eq_knob_slider(
            q_to_slider(q),
            colors,
            "eq-q",
            move |value: f32, cx: &mut App| {
                let q = slider_to_q(value);
                let _ = weak_q.update(cx, |app, cx| app.set_eq_q(q, cx));
            },
        );
        block = block.child(eq_param_block(
            pal,
            "品质因数",
            format!("Q {q:.2}"),
            q_slider,
        ));
        block.into_any_element()
    }

    pub(super) fn eq_master_block(
        &self,
        pal: &EqPalette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let colors = pal.slider_colors();
        let slider_h = if compact { 30.0 } else { 26.0 };
        let mut block = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(if compact { px(8.0) } else { px(6.0) })
            .child(
                div()
                    .text_size(px(9.0))
                    .text_color(pal.text_faint)
                    .child("输入 / 输出增益"),
            );

        for (label, value_db, output, id) in [
            (
                "输入增益",
                self.eq.settings.input_gain_db,
                false,
                "eq-input-gain",
            ),
            (
                "输出增益",
                self.eq.settings.output_gain_db,
                true,
                "eq-output-gain",
            ),
        ] {
            let weak = weak.clone();
            let slider = eq_bipolar_slider(
                ((value_db + 24.0) / 48.0).clamp(0.0, 1.0),
                colors,
                pal.text_faint,
                slider_h,
                5.5,
                id,
                move |value: f32, cx: &mut App| {
                    let db = value * 48.0 - 24.0;
                    let _ = weak.update(cx, |app, cx| {
                        if output {
                            app.eq.settings.output_gain_db = db;
                        } else {
                            app.eq.settings.input_gain_db = db;
                        }
                        app.eq.touch();
                        app.apply_eq(cx);
                    });
                },
            );
            block = block.child(eq_param_block(
                pal,
                label,
                format!("{} dB", db_num(value_db)),
                slider,
            ));
        }
        block.into_any_element()
    }

    pub(super) fn eq_statusbar_desktop(&self, pal: &EqPalette) -> AnyElement {
        div()
            .h(px(30.0))
            .flex_none()
            .px(px(16.0))
            .flex()
            .items_center()
            .gap(px(7.0))
            .border_t_1()
            .border_color(pal.divider)
            .child(
                div()
                    .size(px(5.0))
                    .rounded(px(2.5))
                    .bg(if self.eq.settings.enabled {
                        pal.ok
                    } else {
                        pal.text_faint
                    }),
            )
            .child(div().text_size(px(9.0)).text_color(pal.text_dim).child(
                if self.eq.settings.enabled {
                    "实时生效"
                } else {
                    "已旁路"
                },
            ))
            .child(
                div()
                    .text_size(px(9.0))
                    .text_color(pal.text_faint)
                    .child("· 参数热更新，不中断播放"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(9.0))
                    .font_family(pal.mono.clone())
                    .text_color(pal.text_faint)
                    .child("Esc 关闭"),
            )
            .into_any_element()
    }
}
