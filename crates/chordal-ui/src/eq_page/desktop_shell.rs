//! EQ 页面区块：desktop_shell（由 eq_page.rs 拆分，行为不变）。

use super::*;
use super::controls::*;
use super::drawing::*;

impl ChordialApp {
    // ---------- 桌面：Studio 控制台 ----------

    pub(super) fn eq_render_desktop(&self, pal: &EqPalette, enter: f32, cx: &mut Context<Self>) -> AnyElement {
        let panel_w = (self.viewport_width - 56.0).clamp(700.0, 1180.0);
        let panel_h = (self.viewport_height - 56.0).clamp(520.0, 820.0);
        let main_h = panel_h - 60.0 - 30.0;
        let weak = cx.entity().downgrade();

        let backdrop = {
            let weak = weak.clone();
            div()
                .id("eq-backdrop")
                .absolute()
                .inset_0()
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_eq(cx));
                })
                .into_any_element()
        };

        let content: AnyElement = match self.eq.tab {
            EqTab::Edit => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .child(self.eq_editor_desktop(pal, panel_w, panel_h, cx))
                .child(self.eq_inspector_desktop(pal, main_h, cx))
                .into_any_element(),
            EqTab::V4 => self
                .eq_library_desktop(pal, v4_presets(), main_h, cx)
                .into_any_element(),
            EqTab::AutoEq => self.eq_autoeq_desktop(pal, main_h, cx).into_any_element(),
            EqTab::Builtin => self
                .eq_library_desktop(pal, builtin_presets(), main_h, cx)
                .into_any_element(),
            EqTab::Personal => self.eq_saved_desktop(pal, main_h, cx).into_any_element(),
        };
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.eq_desktop_rail(pal, cx))
            .child(content)
            .into_any_element();

        let panel = div()
            .id("eq-panel")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, _| {})
            .relative()
            .w(px(panel_w))
            .h(px(panel_h))
            .mt(px((1.0 - enter) * 12.0))
            .rounded(px(18.0))
            .font_family(pal.font.clone())
            .border_1()
            .border_color(pal.border_strong)
            .bg(linear_gradient(
                180.0,
                linear_color_stop(pal.bg_top, 0.0),
                linear_color_stop(pal.bg_bottom, 1.0),
            ))
            .shadow(vec![BoxShadow {
                color: pal.shadow,
                offset: point(px(0.0), px(26.0)),
                blur_radius: px(70.0),
                spread_radius: px(0.0),
            }])
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(self.eq_header_desktop(pal, panel_w, cx))
            .child(body)
            .child(self.eq_statusbar_desktop(pal));

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
            .items_center()
            .justify_center()
            .overflow_hidden()
            .bg(pal.scrim)
            .opacity(enter)
            .child(backdrop)
            .child(panel)
            .into_any_element()
    }

    pub(super) fn eq_header_desktop(
        &self,
        pal: &EqPalette,
        panel_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let title = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .size(px(30.0))
                    .rounded(px(9.0))
                    .bg(pal.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icons::icon(Icon::Equalizer, 16.0, pal.accent)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child("均衡器"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .child(
                                div()
                                    .max_w(px(220.0))
                                    .text_size(px(10.5))
                                    .text_color(if self.eq.dirty {
                                        pal.accent
                                    } else {
                                        pal.text_dim
                                    })
                                    .text_ellipsis()
                                    .child(SharedString::from(self.eq.display_name())),
                            )
                            .child(div().text_size(px(10.5)).text_color(pal.text_faint).child(
                                SharedString::from(format!(
                                    "· {} 个频段",
                                    self.eq.settings.bands.len()
                                )),
                            )),
                    ),
            );

        let save = if self.eq.dirty {
            let weak = weak.clone();
            div()
                .id("eq-save-as")
                .h(px(30.0))
                .px(px(11.0))
                .rounded(px(8.0))
                .bg(pal.accent_soft)
                .border_1()
                .border_color(pal.accent_line)
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(11.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(pal.accent)
                .hover(|s| s.bg(pal.accent_soft.opacity(0.9)))
                .child("另存为")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.save_eq_preset(cx));
                })
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let gains = if panel_w >= 900.0 {
            div()
                .h(px(28.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .bg(pal.chip)
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(8.5))
                        .text_color(pal.text_faint)
                        .child("IN"),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_dim)
                        .child(SharedString::from(fmt_db(self.eq.settings.input_gain_db))),
                )
                .child(div().w(px(1.0)).h(px(10.0)).bg(pal.divider))
                .child(
                    div()
                        .text_size(px(8.5))
                        .text_color(pal.text_faint)
                        .child("OUT"),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_dim)
                        .child(SharedString::from(fmt_db(self.eq.settings.output_gain_db))),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let switch = {
            let weak = weak.clone();
            widgets::switch(
                self.eq.settings.enabled,
                pal.accent,
                "eq-enabled",
                move |on, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_eq_enabled(on, cx));
                },
            )
        };
        let close = {
            let weak = weak.clone();
            eq_icon_button(
                pal,
                Icon::Close,
                30.0,
                "eq-close",
                "关闭均衡器",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_eq(cx));
                },
            )
        };

        div()
            .h(px(60.0))
            .flex_none()
            .px(px(16.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .border_b_1()
            .border_color(pal.divider)
            .child(title)
            .child(div().flex_1())
            .child(save)
            .child(gains)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(pal.text_dim)
                            .child("启用"),
                    )
                    .child(switch),
            )
            .child(close)
            .into_any_element()
    }

    pub(super) fn eq_desktop_rail(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let autoeq_count = (!self.eq.autoeq.entries.is_empty()).then_some(self.eq.autoeq.len());
        let counts = [
            (EqTab::Edit, None),
            (EqTab::V4, Some(v4_presets().len())),
            (EqTab::AutoEq, autoeq_count),
            (EqTab::Builtin, Some(builtin_presets().len())),
            (EqTab::Personal, Some(self.eq.saved.len())),
        ];
        let mut rail = div()
            .w(px(RAIL_W))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(pal.divider)
            .bg(pal.panel_alt)
            .px(px(12.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(3.0));

        rail = rail.child(eq_section_label(pal, "工作区"));
        for (tab, count) in counts {
            if tab == EqTab::V4 {
                rail = rail.child(
                    div()
                        .h(px(22.0))
                        .pt(px(8.0))
                        .child(eq_section_label(pal, "预设库")),
                );
            }
            let active = self.eq.tab == tab;
            let weak = weak.clone();
            let (title, _) = tab_meta(tab);
            let icon = tab_icon(tab);
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("eq-rail-{}", tab.label())))
                    .h(px(34.0))
                    .px(px(8.0))
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .cursor_pointer()
                    .bg(if active {
                        pal.accent_soft
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |s| {
                        s.bg(if active {
                            pal.accent_soft
                        } else {
                            pal.chip_hover
                        })
                    })
                    .child(match icon {
                        Some(icon) => {
                            icons::icon(icon, 15.0, if active { pal.accent } else { pal.text_dim })
                        }
                        None => div().into_any_element(),
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.0))
                            .font_weight(if active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(if active { pal.accent } else { pal.text })
                            .text_ellipsis()
                            .child(title),
                    )
                    .when_some(count, |d, count| {
                        d.child(
                            div()
                                .text_size(px(9.5))
                                .font_family(pal.mono.clone())
                                .text_color(if active { pal.accent } else { pal.text_faint })
                                .child(SharedString::from(count.to_string())),
                        )
                    })
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.select_eq_tab(tab, cx);
                        });
                    }),
            );
        }

        rail = rail.child(div().flex_1());
        rail = rail.child(
            div()
                .w_full()
                .rounded(px(10.0))
                .bg(pal.chip)
                .border_1()
                .border_color(pal.border)
                .p(px(10.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("当前配置"),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if self.eq.dirty { pal.accent } else { pal.text })
                        .text_ellipsis()
                        .child(SharedString::from(self.eq.preset_name.clone())),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_size(px(9.0)).text_color(pal.text_faint).child(
                            SharedString::from(format!(
                                "{} 个滤波器",
                                self.eq.settings.bands.len()
                            )),
                        ))
                        .child(
                            div()
                                .text_size(px(9.0))
                                .font_family(pal.mono.clone())
                                .text_color(pal.text_dim)
                                .child(SharedString::from(format!(
                                    "IN {}",
                                    db_num(self.eq.settings.input_gain_db)
                                ))),
                        ),
                ),
        );
        rail.child(
            div()
                .px(px(8.0))
                .pt(px(8.0))
                .text_size(px(9.0))
                .text_color(pal.text_faint)
                .child("F3 打开 / Esc 关闭"),
        )
        .into_any_element()
    }

    pub(super) fn eq_editor_desktop(
        &self,
        pal: &EqPalette,
        panel_w: f32,
        panel_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let main_w = (panel_w - RAIL_W - INSPECTOR_W).max(280.0);
        let graph_h = (panel_h - 340.0).clamp(220.0, 460.0);

        let toolbar = {
            let weak_add = weak.clone();
            let weak_remove = weak.clone();
            let has_selection = self.eq.selected.is_some();
            let full = self.eq.settings.bands.len() >= EQ_MAX_BANDS;
            div()
                .h(px(28.0))
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(pal.text)
                                .child(SharedString::from(self.eq.preset_name.clone())),
                        )
                        .when(self.eq.dirty, |d| {
                            d.child(div().size(px(5.0)).rounded(px(2.5)).bg(pal.accent))
                        })
                        .child(
                            div()
                                .text_size(px(9.5))
                                .font_family(pal.mono.clone())
                                .text_color(pal.text_faint)
                                .child(SharedString::from(format!(
                                    "{} / {}",
                                    self.eq.settings.bands.len(),
                                    EQ_MAX_BANDS
                                ))),
                        ),
                )
                .child(div().flex_1())
                .when(has_selection, |d| {
                    d.child(
                        div()
                            .id("eq-toolbar-remove")
                            .h(px(26.0))
                            .px(px(9.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_size(px(10.5))
                            .text_color(pal.text_dim)
                            .hover(|s| s.bg(pal.chip_hover))
                            .child("移除频段")
                            .on_click(move |_, _, cx| {
                                let _ = weak_remove.update(cx, |app, cx| app.remove_eq_band(cx));
                            }),
                    )
                })
                .child(
                    div()
                        .id("eq-toolbar-add")
                        .h(px(26.0))
                        .px(px(9.0))
                        .rounded(px(7.0))
                        .bg(if full { pal.chip } else { pal.accent_soft })
                        .border_1()
                        .border_color(if full { pal.border } else { pal.accent_line })
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .cursor(if full {
                            gpui::CursorStyle::Arrow
                        } else {
                            gpui::CursorStyle::PointingHand
                        })
                        .child(icons::icon(
                            Icon::Plus,
                            11.0,
                            if full { pal.text_faint } else { pal.accent },
                        ))
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(if full { pal.text_faint } else { pal.accent })
                                .child("添加频段"),
                        )
                        .when(!full, |d| {
                            d.on_click(move |_, _, cx| {
                                let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                            })
                        }),
                )
                .into_any_element()
        };

        let hint = div()
            .h(px(16.0))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(px(9.0))
            .text_color(pal.text_faint)
            .child("点击曲线空白处添加频段")
            .child("·")
            .child("拖动节点调整频率与增益")
            .child("·")
            .child("右键删除");

        div()
            .w(px(main_w))
            .flex_none()
            .h_full()
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(toolbar)
            .child(self.eq_graph(pal, main_w - 28.0, graph_h, false, cx))
            .child(self.eq_fader_bank(pal, cx))
            .child(hint)
            .into_any_element()
    }

    pub(super) fn eq_fader_bank(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut strip = div()
            .id("eq-fader-strip")
            .w_full()
            .h(px(142.0))
            .flex_none()
            .rounded(px(12.0))
            .bg(pal.inset)
            .border_1()
            .border_color(pal.border)
            .px(px(8.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .overflow_x_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll());

        if self.eq.settings.bands.is_empty() {
            let weak_add = weak.clone();
            strip = strip.child(
                div()
                    .id("eq-fader-empty")
                    .w_full()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(5.0))
                    .cursor_pointer()
                    .rounded(px(9.0))
                    .hover(|s| s.bg(pal.chip))
                    .child(icons::icon(Icon::Plus, 16.0, pal.accent))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(pal.text_dim)
                            .child("添加第一个频段"),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                    }),
            );
            return strip.into_any_element();
        }

        for (index, band) in self.eq.settings.bands.iter().enumerate() {
            let weak_select = weak.clone();
            let weak_change = weak.clone();
            let selected = self.eq.selected == Some(index);
            strip = strip.child(eq_band_fader(
                index,
                band.freq_hz,
                band.gain_db,
                band.enabled,
                band.kind.uses_gain(),
                band.kind.label(),
                selected,
                pal,
                move |cx| {
                    let _ = weak_select.update(cx, |app, cx| app.select_eq_band(index, cx));
                },
                move |db, cx| {
                    let _ = weak_change.update(cx, |app, cx| {
                        app.eq.selected = Some(index);
                        if let Some(band) = app.eq.settings.bands.get_mut(index) {
                            band.gain_db = db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB);
                            app.eq.touch();
                            app.apply_eq(cx);
                        }
                    });
                },
            ));
        }
        let weak_add = weak.clone();
        let full = self.eq.settings.bands.len() >= EQ_MAX_BANDS;
        if !full {
            strip = strip.child(
                div()
                    .id("eq-fader-add")
                    .w(px(48.0))
                    .h_full()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(3.0))
                    .rounded(px(9.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(pal.chip))
                    .child(icons::icon(Icon::Plus, 14.0, pal.accent))
                    .child(
                        div()
                            .text_size(px(8.5))
                            .text_color(pal.text_faint)
                            .child("添加"),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                    }),
            );
        }
        strip.into_any_element()
    }
}
