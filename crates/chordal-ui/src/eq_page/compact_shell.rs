//! EQ 页面区块：compact_shell（由 eq_page.rs 拆分，行为不变）。

use super::controls::*;
use super::*;

impl ChordialApp {
    pub(super) fn eq_render_compact(
        &self,
        pal: &EqPalette,
        enter: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let side = if self.viewport_width < 560.0 {
            12.0
        } else {
            24.0
        };
        let content_w = (self.viewport_width - side * 2.0).min(COMPACT_MAX_W);
        let header = self.eq_header_compact(pal, cx);
        let nav = self.eq_nav_compact(pal, cx);
        let body: AnyElement = match self.eq.tab {
            EqTab::Edit => self.eq_editor_compact(pal, content_w, cx),
            EqTab::V4 => self.eq_library_compact(pal, v4_presets(), cx),
            EqTab::AutoEq => self.eq_autoeq_compact(pal, cx),
            EqTab::Builtin => self.eq_library_compact(pal, builtin_presets(), cx),
            EqTab::Personal => self.eq_saved_compact(pal, cx),
        };
        div()
            .id("eq-page")
            .key_context("ChordialEq")
            .track_focus(&self.focus_handle)
            .on_action(
                cx.listener(|app, _: &crate::actions::ExitImmersive, window, cx| {
                    app.close_eq(cx);
                    cx.stop_propagation();
                    window.focus(&app.focus_handle);
                }),
            )
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .overflow_hidden()
            .bg(linear_gradient(
                180.0,
                linear_color_stop(pal.bg_top, 0.0),
                linear_color_stop(pal.bg_bottom, 1.0),
            ))
            .opacity(enter)
            .child(
                div()
                    .w(px(content_w))
                    .h_full()
                    .font_family(pal.font.clone())
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "eq-compact-scroll-{}",
                                self.eq.tab.label()
                            )))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .when(self.smooth_scroll, |d| d.smooth_scroll())
                            .pb(px(16.0))
                            .child(body),
                    )
                    .child(nav),
            )
            .into_any_element()
    }

    pub(super) fn eq_header_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let back = {
            let weak = weak.clone();
            eq_icon_button(
                pal,
                Icon::ChevronLeft,
                32.0,
                "eq-back",
                "返回",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_eq(cx));
                },
            )
        };
        let switch = {
            let weak = weak.clone();
            widgets::switch(
                self.eq.settings.enabled,
                pal.accent,
                "eq-enabled-compact",
                move |on, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_eq_enabled(on, cx));
                },
            )
        };
        div()
            .h(px(58.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(back)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child("均衡器"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .max_w(px(200.0))
                                    .text_size(px(10.0))
                                    .text_color(if self.eq.dirty {
                                        pal.accent
                                    } else {
                                        pal.text_dim
                                    })
                                    .text_ellipsis()
                                    .child(SharedString::from(self.eq.display_name())),
                            )
                            .child(div().text_size(px(10.0)).text_color(pal.text_faint).child(
                                SharedString::from(format!(
                                    "· {} 个频段",
                                    self.eq.settings.bands.len()
                                )),
                            )),
                    ),
            )
            .child(
                div()
                    .text_size(px(9.5))
                    .text_color(pal.text_dim)
                    .child("启用"),
            )
            .child(switch)
            .into_any_element()
    }

    pub(super) fn eq_editor_compact(
        &self,
        pal: &EqPalette,
        content_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let graph_h = (self.viewport_height * 0.27).clamp(190.0, 300.0);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(self.eq_graph(pal, content_w, graph_h, true, cx))
            .child(self.eq_band_chips(pal, cx))
            .child(self.eq_inspector_compact(pal, cx))
            .into_any_element()
    }

    pub(super) fn eq_band_chips(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        if self.eq.settings.bands.is_empty() {
            let weak_add = weak.clone();
            return div()
                .id("eq-chips-empty")
                .w_full()
                .h(px(66.0))
                .rounded(px(12.0))
                .border_1()
                .border_color(pal.accent_line)
                .bg(pal.accent_soft)
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                })
                .child(icons::icon(Icon::Plus, 15.0, pal.accent))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(pal.accent)
                        .child("点按曲线或此处添加频段"),
                )
                .into_any_element();
        }

        let mut row = div()
            .id("eq-chips")
            .w_full()
            .h(px(66.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .overflow_x_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll());
        for (index, band) in self.eq.settings.bands.iter().enumerate() {
            let weak_select = weak.clone();
            let selected = self.eq.selected == Some(index);
            row = row.child(
                div()
                    .id(SharedString::from(format!("eq-chip-band-{index}")))
                    .w(px(64.0))
                    .h(px(64.0))
                    .flex_none()
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(if selected {
                        pal.accent_line
                    } else {
                        pal.border
                    })
                    .bg(if selected { pal.selected_bg } else { pal.panel })
                    .p(px(9.0))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = weak_select.update(cx, |app, cx| app.select_eq_band(index, cx));
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(8.0))
                                    .font_family(pal.mono.clone())
                                    .text_color(if selected { pal.accent } else { pal.text_faint })
                                    .child(SharedString::from(format!("{:02}", index + 1))),
                            )
                            .child(div().size(px(4.0)).rounded(px(2.0)).bg(if band.enabled {
                                pal.accent
                            } else {
                                pal.text_faint
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_family(pal.mono.clone())
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(pal.text)
                                    .child(SharedString::from(fmt_freq(band.freq_hz))),
                            )
                            .child(
                                div()
                                    .text_size(px(9.0))
                                    .font_family(pal.mono.clone())
                                    .text_color(if band.kind.uses_gain() && band.enabled {
                                        pal.accent
                                    } else {
                                        pal.text_faint
                                    })
                                    .child(SharedString::from(if band.kind.uses_gain() {
                                        db_num(band.gain_db)
                                    } else {
                                        band.kind.label().to_string()
                                    })),
                            ),
                    ),
            );
        }
        let weak_add = weak.clone();
        let full = self.eq.settings.bands.len() >= EQ_MAX_BANDS;
        row = row.child(
            div()
                .id("eq-chip-add")
                .w(px(64.0))
                .h(px(64.0))
                .flex_none()
                .rounded(px(12.0))
                .border_1()
                .border_color(pal.border)
                .bg(pal.chip)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(2.0))
                .cursor(if full {
                    gpui::CursorStyle::Arrow
                } else {
                    gpui::CursorStyle::PointingHand
                })
                .when(!full, |d| {
                    d.on_click(move |_, _, cx| {
                        let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                    })
                })
                .child(icons::icon(
                    Icon::Plus,
                    15.0,
                    if full { pal.text_faint } else { pal.accent },
                ))
                .child(
                    div()
                        .text_size(px(8.0))
                        .text_color(pal.text_faint)
                        .child("添加"),
                ),
        );
        row.into_any_element()
    }
}
