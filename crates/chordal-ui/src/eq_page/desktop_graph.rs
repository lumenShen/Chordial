//! EQ 页面区块：desktop_graph（曲线交互 + 右侧检视器）。

use super::controls::*;
use super::drawing::*;
use super::*;
use crate::eq::EqState;

/// 曲线图的保留版本：覆盖所有会影响布局/绘制/命中框的输入。
fn eq_graph_version(eq: &EqState, pal: &EqPalette, width: f32, height: f32, compact: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    eq_curve_key(&eq.settings).hash(&mut hasher);
    eq.selected
        .map(|selected| match selected {
            EqSelection::Profile => (0u8, 0usize),
            EqSelection::Block(index) => (1, index),
            EqSelection::Custom(index) => (2, index),
        })
        .hash(&mut hasher);
    for origin in &eq.origins {
        match *origin {
            BandOrigin::Profile(index) => (0u8, index),
            BandOrigin::Block { block, band } => (1, block ^ (band << 16)),
            BandOrigin::Custom(index) => (2, index),
        }
        .hash(&mut hasher);
    }
    eq.profile
        .as_ref()
        .map(|profile| (profile.name.as_str(), profile.source.as_str()))
        .hash(&mut hasher);
    for block in &eq.blocks {
        (
            block.preset_id.as_str(),
            block.enabled,
            block.amount.to_bits(),
        )
            .hash(&mut hasher);
    }
    width.to_bits().hash(&mut hasher);
    height.to_bits().hash(&mut hasher);
    compact.hash(&mut hasher);
    for color in [
        pal.inset,
        pal.border,
        pal.text,
        pal.text_dim,
        pal.text_faint,
        pal.accent,
        pal.panel,
        pal.border_strong,
        pal.grid,
        pal.grid_major,
        pal.shadow,
    ] {
        color.h.to_bits().hash(&mut hasher);
        color.s.to_bits().hash(&mut hasher);
        color.l.to_bits().hash(&mut hasher);
        color.a.to_bits().hash(&mut hasher);
    }
    (pal.font.as_ref(), pal.mono.as_ref()).hash(&mut hasher);
    hasher.finish()
}

/// 检视器版本：参数、选中、滚动、悬停与主题任一变化都重建。
fn eq_inspector_version(
    eq: &EqState,
    pal: &EqPalette,
    height: f32,
    scroll: gpui::Point<gpui::Pixels>,
    mouse: gpui::Point<gpui::Pixels>,
    smooth_scroll: bool,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    eq_curve_key(&eq.settings).hash(&mut hasher);
    eq.selected
        .map(|selected| match selected {
            EqSelection::Profile => (0u8, 0usize),
            EqSelection::Block(index) => (1, index),
            EqSelection::Custom(index) => (2, index),
        })
        .hash(&mut hasher);
    for origin in &eq.origins {
        match *origin {
            BandOrigin::Profile(index) => (0u8, index),
            BandOrigin::Block { block, band } => (1, block ^ (band << 16)),
            BandOrigin::Custom(index) => (2, index),
        }
        .hash(&mut hasher);
    }
    eq.profile
        .as_ref()
        .map(|profile| {
            (
                profile.name.as_str(),
                profile.source.as_str(),
                profile.band_count(),
            )
        })
        .hash(&mut hasher);
    for block in &eq.blocks {
        (
            block.preset_id.as_str(),
            block.enabled,
            block.amount.to_bits(),
        )
            .hash(&mut hasher);
    }
    (eq.preset_name.as_str(), eq.dirty, eq.type_menu_open).hash(&mut hasher);
    smooth_scroll.hash(&mut hasher);
    (eq.preview_builtin, eq.preview_saved).hash(&mut hasher);
    height.to_bits().hash(&mut hasher);
    f32::from(scroll.x).to_bits().hash(&mut hasher);
    f32::from(scroll.y).to_bits().hash(&mut hasher);
    f32::from(mouse.x).to_bits().hash(&mut hasher);
    f32::from(mouse.y).to_bits().hash(&mut hasher);
    for color in [
        pal.divider,
        pal.text,
        pal.text_dim,
        pal.text_faint,
        pal.accent,
        pal.accent_soft,
        pal.accent_line,
        pal.chip,
        pal.chip_hover,
        pal.selected_bg,
        pal.danger,
        pal.danger_soft,
        pal.ok,
    ] {
        color.h.to_bits().hash(&mut hasher);
        color.s.to_bits().hash(&mut hasher);
        color.l.to_bits().hash(&mut hasher);
        color.a.to_bits().hash(&mut hasher);
    }
    (pal.font.as_ref(), pal.mono.as_ref()).hash(&mut hasher);
    hasher.finish()
}

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
        let paint_origins = self.eq.origins.clone();
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
                    &paint_origins,
                    paint_selected,
                    compact,
                    0,
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
                        .origins
                        .iter()
                        .copied()
                        .zip(app.eq.settings.bands.iter())
                        .map(|(origin, band)| {
                            let dx = geom.x(band.freq_hz) - local_x;
                            let dy = geom.y(band.gain_db + global) - local_y;
                            (origin, dx * dx + dy * dy)
                        })
                        .filter(|(_, distance)| *distance <= hit_radius * hit_radius)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(origin, _)| origin);
                    if let Some(origin) = nearest {
                        // 点选只负责选中；参数调整集中在右侧检视器。
                        app.eq.selected = Some(EqSelection::from_origin(origin));
                        app.eq.type_menu_open = false;
                        down_drag.set(matches!(origin, BandOrigin::Custom(_)));
                        cx.notify();
                    } else if app.eq.add_custom_band().is_some() {
                        // 曲线空白处直接新建一个自定义频段并进入拖拽。
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
                    let Some(EqSelection::Custom(index)) = app.eq.selected else {
                        return;
                    };
                    let geom = if compact {
                        GraphGeom::compact(w, h)
                    } else {
                        GraphGeom::new(w, h)
                    };
                    let global = app.eq.settings.input_gain_db + app.eq.settings.output_gain_db;
                    if let Some(band) = app.eq.custom.get_mut(index) {
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
                        .origins
                        .iter()
                        .copied()
                        .zip(app.eq.settings.bands.iter())
                        .map(|(origin, band)| {
                            let dx = geom.x(band.freq_hz) - local_x;
                            let dy = geom.y(band.gain_db + global) - local_y;
                            (origin, dx * dx + dy * dy)
                        })
                        .filter(|(_, distance)| *distance <= 20.0 * 20.0)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(origin, _)| origin);
                    match nearest {
                        Some(BandOrigin::Custom(index)) => {
                            app.eq.remove_custom(index);
                            app.apply_eq(cx);
                        }
                        Some(BandOrigin::Block { block, .. }) => {
                            app.eq.remove_block(block);
                            app.apply_eq(cx);
                        }
                        _ => {}
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

        // 图例：档案 / 预设 / 自定义三层节点颜色。
        if !compact {
            card = card.child(
                div()
                    .absolute()
                    .right(px(8.0))
                    .top(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(self.eq_legend_item(pal, "档案", pal.text_dim))
                    .child(self.eq_legend_item(pal, "预设", pal.accent.opacity(0.72)))
                    .child(self.eq_legend_item(pal, "自定义", pal.accent)),
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
                            .child("点按曲线添加自定义频段"),
                    ),
            );
        }

        if let Some(index) = self.eq.selected_band_index()
            && let Some(band) = self.eq.settings.bands.get(index)
            && let Some(origin) = self.eq.origins.get(index)
        {
            let global = self.eq.settings.input_gain_db + self.eq.settings.output_gain_db;
            let x = geom.x(band.freq_hz);
            let y = geom.y(band.gain_db + global);
            let tip = match origin {
                BandOrigin::Profile(_) => match &self.eq.profile {
                    Some(profile) => format!("档案 · {}", profile.name),
                    None => "耳机档案".into(),
                },
                BandOrigin::Block { block, .. } => {
                    let preset = self.eq.block_preset(*block);
                    let amount = self
                        .eq
                        .blocks
                        .get(*block)
                        .map(|block| crate::eq::fmt_amount(block.amount))
                        .unwrap_or_default();
                    match preset {
                        Some(preset) => format!("预设 · {} {}", preset.name, amount),
                        None => "内置预设".into(),
                    }
                }
                BandOrigin::Custom(index) => band_summary(*index, band),
            };
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

        // 曲线、网格、刻度与节点只在设置/选中/几何/主题变化时重建。
        // 静态动画期间直接复用上一帧的 prepaint/paint，避免 260 点曲线与
        // 20 个节点的路径、文本和圆角几何每帧重新进入场景。
        if std::env::var_os("CHORDIAL_NO_RETAINED").is_some() {
            return card.into_any_element();
        }
        let version = eq_graph_version(&self.eq, pal, width, height, compact);
        let mut refinement = gpui::StyleRefinement::default();
        refinement = refinement.w_full().h(px(height)).flex_none();
        let mut style = gpui::Style::default();
        style.refine(&refinement);
        gpui::Retained::new("eq-graph-retained", version, style, move |_window, _cx| {
            card.into_any_element()
        })
        .into_any_element()
    }

    fn eq_legend_item(&self, pal: &EqPalette, label: &'static str, color: Hsla) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(3.0))
            .child(div().size(px(5.0)).rounded(px(2.5)).bg(color))
            .child(
                div()
                    .text_size(px(7.5))
                    .text_color(pal.text_faint)
                    .child(label),
            )
            .into_any_element()
    }

    pub(super) fn eq_inspector_desktop(
        &self,
        pal: &EqPalette,
        main_h: f32,
        mouse: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .track_scroll(&self.eq_inspector_scroll)
            .overflow_y_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll());

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
                .child(
                    div()
                        .h(px(15.0))
                        .px(px(5.0))
                        .rounded(px(4.0))
                        .bg(pal.chip)
                        .flex()
                        .items_center()
                        .text_size(px(8.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .child(SharedString::from(format!(
                            "{} / {}",
                            self.eq.total_bands(),
                            EQ_MAX_BANDS
                        ))),
                ),
        );

        match self.eq.selected {
            Some(EqSelection::Custom(index)) => {
                if let Some(band) = self.eq.custom.get(index).cloned() {
                    body = body.child(self.eq_custom_inspector(pal, index, &band, false, cx));
                    body = body.child(eq_divider(pal));
                }
            }
            Some(EqSelection::Block(index)) => {
                if let Some(block) = self.eq.blocks.get(index).cloned()
                    && let Some(preset) = self.eq.block_preset(index)
                {
                    body = body.child(self.eq_block_inspector(pal, index, preset, &block, cx));
                    body = body.child(eq_divider(pal));
                }
            }
            Some(EqSelection::Profile) => {
                if let Some(profile) = self.eq.profile.clone() {
                    body = body.child(self.eq_profile_inspector(pal, &profile, cx));
                    body = body.child(eq_divider(pal));
                }
            }
            None => {
                body = body.child(self.eq_chain_overview(pal));
                body = body.child(eq_divider(pal));
            }
        }

        body = body.child(div().flex_1());
        body = body.child(self.eq_master_block(pal, false, cx));

        // 检视器只在参数/选中/悬停/滚动/主题变化时重建。滚动底部信号链
        // 不会使它失效，避免两个独立面板互相拖慢。
        if std::env::var_os("CHORDIAL_NO_RETAINED").is_some() {
            return body.into_any_element();
        }
        let version = eq_inspector_version(
            &self.eq,
            pal,
            main_h,
            self.eq_inspector_scroll.offset(),
            mouse,
            self.smooth_scroll,
        );
        let mut refinement = gpui::StyleRefinement::default();
        refinement = refinement.w(px(INSPECTOR_W)).h(px(main_h)).flex_none();
        let mut style = gpui::Style::default();
        style.refine(&refinement);
        gpui::Retained::new(
            "eq-inspector-retained",
            version,
            style,
            move |_window, _cx| body.into_any_element(),
        )
        .into_any_element()
    }

    /// 自定义频段的检视器主体。
    pub(super) fn eq_custom_inspector(
        &self,
        pal: &EqPalette,
        index: usize,
        band: &chordal_player::EqBand,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut block = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(if compact { 10.0 } else { 8.0 }));

        block = block.child(
            div()
                .w_full()
                .rounded(px(10.0))
                .bg(pal.chip)
                .border_1()
                .border_color(pal.border)
                .p(px(10.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.0))
                                .child(div().text_size(px(8.5)).text_color(pal.text_faint).child(
                                    SharedString::from(format!("自定义 · 频段 {:02}", index + 1)),
                                ))
                                .child(eq_tag(pal, "手调", pal.accent, pal.accent_soft)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(3.0))
                                .child(
                                    div()
                                        .text_size(px(if compact { 24.0 } else { 20.0 }))
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
                                        .text_size(px(if compact { 24.0 } else { 20.0 }))
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

        // 启用 / 移除。
        {
            let weak_toggle_dot = weak.clone();
            let weak_toggle_row = weak.clone();
            let weak_remove = weak.clone();
            let enabled = band.enabled;
            block = block.child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(
                        div()
                            .id("eq-inspector-enabled")
                            .h(px(24.0))
                            .flex_1()
                            .rounded(px(6.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.0))
                            .cursor_pointer()
                            .bg(if enabled { pal.accent_soft } else { pal.chip })
                            .text_size(px(9.5))
                            .text_color(if enabled { pal.accent } else { pal.text_faint })
                            .hover(|s| s.bg(pal.chip_hover))
                            .child(eq_power_dot(
                                pal,
                                if enabled {
                                    PowerDot::On
                                } else {
                                    PowerDot::Bypassed
                                },
                                SharedString::from("eq-inspector-enabled-dot"),
                                move |_, _, cx| {
                                    cx.stop_propagation();
                                    let _ = weak_toggle_dot.update(cx, |app, cx| {
                                        app.set_eq_band_enabled(index, !enabled, cx);
                                    });
                                },
                            ))
                            .child(if enabled { "启用中" } else { "已旁路" })
                            .on_click(move |_, _, cx| {
                                let _ = weak_toggle_row.update(cx, |app, cx| {
                                    app.set_eq_band_enabled(index, !enabled, cx);
                                });
                            }),
                    )
                    .child(
                        div()
                            .id("eq-inspector-remove")
                            .h(px(24.0))
                            .px(px(8.0))
                            .rounded(px(6.0))
                            .flex()
                            .items_center()
                            .gap(px(3.0))
                            .cursor_pointer()
                            .text_size(px(9.5))
                            .text_color(pal.danger)
                            .hover(|s| s.bg(pal.danger_soft))
                            .child(icons::icon(Icon::Trash, 10.0, pal.danger))
                            .child("移除")
                            .on_click(move |_, _, cx| {
                                let _ = weak_remove.update(cx, |app, cx| {
                                    app.eq.remove_custom(index);
                                    app.apply_eq(cx);
                                });
                            }),
                    ),
            );
        }

        block = block.child(
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

        block = block.child(self.eq_params_block(pal, index, band, compact, cx));

        let response = EqSettings {
            enabled: true,
            bands: vec![band.clone()],
            ..EqSettings::default()
        };
        block = block.child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("该频段响应"),
                )
                .child(self.eq_mini_response(pal, &response, if compact { 108.0 } else { 96.0 })),
        );

        block.into_any_element()
    }

    /// 内置预设整块的检视器：幅度乘数、启用状态与频谱。
    pub(super) fn eq_block_inspector(
        &self,
        pal: &EqPalette,
        index: usize,
        preset: &'static BuiltinPreset,
        block: &PresetBlock,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut body = div().w_full().flex().flex_col().gap(px(8.0));

        body = body.child(
            div()
                .w_full()
                .rounded(px(10.0))
                .bg(pal.chip)
                .border_1()
                .border_color(pal.border)
                .p(px(10.0))
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(
                            div()
                                .text_size(px(8.5))
                                .text_color(pal.text_faint)
                                .child(SharedString::from(format!("链路预设 {:02}", index + 1))),
                        )
                        .child(eq_tag(pal, preset.group.label(), pal.text_dim, pal.panel))
                        .child(eq_tag(
                            pal,
                            if block.enabled { "启用" } else { "旁路" },
                            if block.enabled {
                                pal.accent
                            } else {
                                pal.text_faint
                            },
                            if block.enabled {
                                pal.accent_soft
                            } else {
                                pal.panel
                            },
                        )),
                )
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(pal.text)
                        .child(preset.name),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .line_height(px(14.0))
                        .text_color(pal.text_faint)
                        .child(preset.description),
                ),
        );

        {
            let weak_amount = weak.clone();
            let amount = block.amount;
            let slider = widgets::eq_knob_slider(
                (amount / crate::eq::PRESET_AMOUNT_MAX).clamp(0.0, 1.0),
                pal.slider_colors(),
                "eq-block-amount",
                move |value: f32, cx: &mut App| {
                    let amount = value * crate::eq::PRESET_AMOUNT_MAX;
                    let _ = weak_amount.update(cx, |app, cx| {
                        app.set_eq_block_amount(index, amount, cx);
                    });
                },
            );
            body = body.child(eq_param_block(
                pal,
                "增幅乘数",
                format!(
                    "{} · 实际 {} dB",
                    crate::eq::fmt_amount(amount),
                    db_num(preset.base_gain_db() * amount)
                ),
                slider,
            ));
        }

        body = body.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child(
                    div()
                        .id("eq-block-enabled")
                        .h(px(26.0))
                        .flex_1()
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(4.0))
                        .cursor_pointer()
                        .bg(if block.enabled {
                            pal.accent_soft
                        } else {
                            pal.chip
                        })
                        .text_size(px(9.5))
                        .text_color(if block.enabled {
                            pal.accent
                        } else {
                            pal.text_faint
                        })
                        .hover(|s| s.bg(pal.chip_hover))
                        .child(if block.enabled {
                            "旁路这枚预设"
                        } else {
                            "重新启用"
                        })
                        .on_click({
                            let weak = weak.clone();
                            let enabled = block.enabled;
                            move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.set_eq_block_enabled(index, !enabled, cx);
                                });
                            }
                        }),
                )
                .child(
                    div()
                        .id("eq-block-remove")
                        .h(px(26.0))
                        .px(px(8.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .cursor_pointer()
                        .text_size(px(9.5))
                        .text_color(pal.danger)
                        .hover(|s| s.bg(pal.danger_soft))
                        .child(icons::icon(Icon::Trash, 10.0, pal.danger))
                        .child("移出链路")
                        .on_click({
                            let weak = weak.clone();
                            move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.remove_eq_block(index, cx);
                                });
                            }
                        }),
                ),
        );

        let mut visual = preset.to_settings(block.amount);
        // 预览只看预设本身的形状；前级补偿在统计里单独展示。
        visual.input_gain_db = 0.0;
        body = body.child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("预设频谱"),
                )
                .child(self.eq_mini_response(pal, &visual, 100.0)),
        );

        body = body.child(
            div()
                .w_full()
                .flex()
                .gap(px(8.0))
                .child(eq_stat(
                    pal,
                    "中心频率",
                    preset
                        .bands
                        .first()
                        .map(|band| fmt_freq(band.0))
                        .unwrap_or_else(|| "—".into()),
                ))
                .child(eq_stat(
                    pal,
                    "基准增益",
                    preset
                        .bands
                        .first()
                        .map(|band| format!("{} dB", db_num(band.1)))
                        .unwrap_or_else(|| "—".into()),
                ))
                .child(eq_stat(
                    pal,
                    "前级补偿",
                    format!("{} dB", db_num(preset.input_gain_db * block.amount)),
                )),
        );

        body.child(
            div()
                .text_size(px(8.5))
                .line_height(px(13.0))
                .text_color(pal.text_faint)
                .child("调大增幅乘数会把预设分贝按比例放大；0× 等同于旁路。"),
        )
        .into_any_element()
    }

    pub(super) fn eq_profile_inspector(
        &self,
        pal: &EqPalette,
        profile: &EqProfile,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut body = div().w_full().flex().flex_col().gap(px(8.0));

        body = body.child(
            div()
                .w_full()
                .rounded(px(10.0))
                .bg(pal.chip)
                .border_1()
                .border_color(pal.border)
                .p(px(10.0))
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(
                            div()
                                .text_size(px(8.5))
                                .text_color(pal.text_faint)
                                .child("耳机档案 · 链路第一层"),
                        )
                        .when(!profile.source.is_empty(), |d| {
                            d.child(eq_tag(
                                pal,
                                profile.source.clone(),
                                pal.accent,
                                pal.accent_soft,
                            ))
                        }),
                )
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(pal.text)
                        .text_ellipsis()
                        .child(profile.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child(SharedString::from(profile.subtitle())),
                ),
        );

        let mut visual = profile.settings.clone();
        visual.input_gain_db = 0.0;
        body = body.child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("档案频谱"),
                )
                .child(self.eq_mini_response(pal, &visual, 110.0)),
        );

        body = body.child(
            div()
                .w_full()
                .flex()
                .gap(px(8.0))
                .child(eq_stat(pal, "频段", profile.band_count().to_string()))
                .child(eq_stat(
                    pal,
                    "前级",
                    format!("{} dB", db_num(profile.settings.input_gain_db)),
                ))
                .child(eq_stat(
                    pal,
                    "启用节点",
                    profile
                        .settings
                        .bands
                        .iter()
                        .filter(|band| band.enabled)
                        .count()
                        .to_string(),
                )),
        );

        body = body.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(5.0))
                .child({
                    let weak = weak.clone();
                    div()
                        .id("eq-profile-change")
                        .h(px(30.0))
                        .flex_1()
                        .rounded(px(7.0))
                        .bg(pal.accent_soft)
                        .border_1()
                        .border_color(pal.accent_line)
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(4.0))
                        .cursor_pointer()
                        .text_size(px(9.5))
                        .text_color(pal.accent)
                        .hover(|s| s.bg(pal.accent_soft.opacity(0.8)))
                        .child("更换耳机档案")
                        .child(icons::icon(Icon::ChevronRight, 10.0, pal.accent))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.select_eq_tab(EqTab::AutoEq, cx);
                            });
                        })
                })
                .child({
                    let weak = weak.clone();
                    div()
                        .id("eq-profile-clear")
                        .h(px(30.0))
                        .px(px(8.0))
                        .rounded(px(7.0))
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .cursor_pointer()
                        .text_size(px(9.5))
                        .text_color(pal.danger)
                        .hover(|s| s.bg(pal.danger_soft))
                        .child(icons::icon(Icon::Trash, 10.0, pal.danger))
                        .child("清除")
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.clear_eq_profile(cx);
                            });
                        })
                }),
        );

        body.child(
            div()
                .text_size(px(8.5))
                .line_height(px(13.0))
                .text_color(pal.text_faint)
                .child("耳机档案与内置预设、自定义频段是叠加关系；更换档案不会清空你手调的节点。"),
        )
        .into_any_element()
    }

    pub(super) fn eq_chain_overview(&self, pal: &EqPalette) -> AnyElement {
        let total = self.eq.total_bands();
        let ratio = (total as f32 / EQ_MAX_BANDS as f32).clamp(0.0, 1.0);
        div()
            .w_full()
            .rounded(px(10.0))
            .bg(pal.chip)
            .border_1()
            .border_color(pal.border)
            .p(px(10.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(10.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(pal.text_dim)
                    .child("未选择节点"),
            )
            .child(
                div()
                    .text_size(px(9.0))
                    .line_height(px(14.0))
                    .text_color(pal.text_faint)
                    .child("点选曲线节点，或点选下方信号链里的档案 / 预设 / 自定义滤波器。"),
            )
            .child(
                div()
                    .w_full()
                    .h(px(4.0))
                    .rounded(px(2.0))
                    .bg(pal.fader_track)
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative(ratio))
                            .rounded(px(2.0))
                            .bg(pal.accent),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(eq_stat(
                        pal,
                        "档案",
                        self.eq.profile_band_count().to_string(),
                    ))
                    .child(eq_stat(pal, "预设", self.eq.blocks.len().to_string()))
                    .child(eq_stat(pal, "自定义", self.eq.custom.len().to_string()))
                    .child(eq_stat(pal, "节点", format!("{total} / {EQ_MAX_BANDS}"))),
            )
            .into_any_element()
    }

    /// 检视器里的迷你频谱：复用主曲线绘制（独立缓存槽位）。
    pub(super) fn eq_mini_response(
        &self,
        pal: &EqPalette,
        settings: &EqSettings,
        height: f32,
    ) -> AnyElement {
        let pal_paint = pal.clone();
        let paint_settings = settings.clone();
        div()
            .w_full()
            .h(px(height))
            .flex_none()
            .rounded(px(9.0))
            .bg(pal.inset)
            .border_1()
            .border_color(pal.border)
            .overflow_hidden()
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |_bounds, bounds, window, _| {
                        paint_eq_graph(
                            bounds,
                            &paint_settings,
                            &[],
                            None,
                            true,
                            1,
                            &pal_paint,
                            window,
                        );
                    },
                )
                .size_full(),
            )
            .into_any_element()
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
                        app.set_eq_master_gain(output, db, cx);
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
                    .child(SharedString::from(format!(
                        "剩余 {} 个节点",
                        self.eq.remaining_bands()
                    ))),
            )
            .child(div().w(px(1.0)).h(px(10.0)).bg(pal.divider))
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
