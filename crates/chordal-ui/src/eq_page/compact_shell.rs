//! EQ 页面区块：compact_shell（竖屏 / 窄窗触控布局）。

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
            EqTab::AutoEq => self.eq_autoeq_compact(pal, cx),
            EqTab::Builtin => self.eq_builtin_compact(pal, cx),
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
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(16.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(pal.text)
                                    .child("均衡器"),
                            )
                            .child(
                                div()
                                    .h(px(16.0))
                                    .px(px(6.0))
                                    .rounded(px(8.0))
                                    .bg(pal.chip)
                                    .flex()
                                    .items_center()
                                    .text_size(px(8.0))
                                    .font_family(pal.mono.clone())
                                    .text_color(pal.text_faint)
                                    .child(SharedString::from(format!(
                                        "{} 档案 · {} 预设 · {} 自定义",
                                        self.eq.profile_band_count(),
                                        self.eq.enabled_block_count(),
                                        self.eq.custom.len()
                                    ))),
                            ),
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
                                SharedString::from(format!("· {} 个节点", self.eq.total_bands())),
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
            .child(self.eq_chain_compact(pal, cx))
            .child(self.eq_inspector_compact(pal, cx))
            .into_any_element()
    }

    /// 紧凑布局的信号链：与桌面同一套语义，纵向铺开。
    pub(super) fn eq_chain_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut panel = div()
            .w_full()
            .rounded(px(16.0))
            .bg(pal.panel)
            .border_1()
            .border_color(pal.border_strong)
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(5.0))
            .child(
                div()
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(pal.text_dim)
                            .child("信号链"),
                    )
                    .child(
                        div()
                            .text_size(px(8.5))
                            .font_family(pal.mono.clone())
                            .text_color(pal.text_faint)
                            .child(SharedString::from(format!(
                                "{} / {} 节点",
                                self.eq.total_bands(),
                                EQ_MAX_BANDS
                            ))),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(8.5))
                            .text_color(pal.text_faint)
                            .child("点选后在下方调整"),
                    ),
            );

        // ---- 耳机档案 ----
        {
            let weak_open = weak.clone();
            panel = panel.child(eq_stage_header(
                pal,
                Icon::Headphones,
                "耳机档案",
                self.eq.profile_band_count(),
                Some(
                    div()
                        .id("eq-chain-c-profile-open")
                        .h(px(22.0))
                        .px(px(7.0))
                        .rounded(px(6.0))
                        .bg(pal.chip)
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .cursor_pointer()
                        .text_size(px(8.5))
                        .text_color(pal.text_dim)
                        .child("更换")
                        .child(icons::icon(Icon::ChevronRight, 10.0, pal.text_faint))
                        .on_click(move |_, _, cx| {
                            let _ = weak_open.update(cx, |app, cx| {
                                app.select_eq_tab(EqTab::AutoEq, cx);
                            });
                        })
                        .into_any_element(),
                ),
            ));
            match self.eq.profile.clone() {
                Some(profile) => {
                    let weak_select = weak.clone();
                    let weak_clear = weak.clone();
                    let selected = self.eq.selected == Some(EqSelection::Profile);
                    panel = panel.child(
                        div()
                            .id("eq-chain-c-profile")
                            .h(px(40.0))
                            .w_full()
                            .px(px(8.0))
                            .rounded(px(9.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .cursor_pointer()
                            .bg(if selected { pal.selected_bg } else { pal.chip })
                            .child(icons::icon(Icon::Headphones, 14.0, pal.accent))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.0))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(if selected {
                                                pal.accent
                                            } else {
                                                pal.text
                                            })
                                            .text_ellipsis()
                                            .child(profile.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.0))
                                            .text_color(pal.text_faint)
                                            .child(SharedString::from(profile.subtitle())),
                                    ),
                            )
                            .child(eq_mini_icon_button(
                                pal,
                                Icon::Trash,
                                26.0,
                                SharedString::from("eq-chain-c-profile-remove"),
                                pal.text_faint,
                                pal.danger_soft,
                                move |_, _, cx| {
                                    let _ = weak_clear.update(cx, |app, cx| {
                                        app.clear_eq_profile(cx);
                                    });
                                },
                            ))
                            .on_click(move |_, _, cx| {
                                let _ = weak_select.update(cx, |app, cx| {
                                    app.select_eq_profile(cx);
                                });
                            }),
                    );
                }
                None => {
                    let weak_open = weak.clone();
                    panel = panel.child(
                        div()
                            .id("eq-chain-c-profile-empty")
                            .h(px(36.0))
                            .w_full()
                            .px(px(8.0))
                            .rounded(px(9.0))
                            .border_1()
                            .border_color(pal.border)
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .cursor_pointer()
                            .child(icons::icon(Icon::Headphones, 13.0, pal.text_faint))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(10.0))
                                    .text_color(pal.text_faint)
                                    .child("未加载耳机档案"),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(pal.accent)
                                    .child("去选择 >"),
                            )
                            .on_click(move |_, _, cx| {
                                let _ = weak_open.update(cx, |app, cx| {
                                    app.select_eq_tab(EqTab::AutoEq, cx);
                                });
                            }),
                    );
                }
            }
            panel = panel.child(eq_divider(pal));
        }

        // ---- 内置预设 ----
        {
            let weak_add = weak.clone();
            panel = panel.child(eq_stage_header(
                pal,
                Icon::Layers,
                "内置预设",
                self.eq.blocks.len(),
                Some(
                    div()
                        .id("eq-chain-c-block-add")
                        .h(px(22.0))
                        .px(px(7.0))
                        .rounded(px(6.0))
                        .bg(pal.chip)
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .cursor_pointer()
                        .text_size(px(8.5))
                        .text_color(pal.text_dim)
                        .child("添加")
                        .child(icons::icon(Icon::Plus, 10.0, pal.text_faint))
                        .on_click(move |_, _, cx| {
                            let _ = weak_add.update(cx, |app, cx| {
                                app.select_eq_tab(EqTab::Builtin, cx);
                            });
                        })
                        .into_any_element(),
                ),
            ));
            if self.eq.blocks.is_empty() {
                panel = panel.child(
                    div()
                        .px(px(6.0))
                        .py(px(5.0))
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("还没有启用预设 · 从「内置预设」加入"),
                );
            }
            for index in 0..self.eq.blocks.len() {
                let Some(preset) = self.eq.block_preset(index) else {
                    continue;
                };
                let block = self.eq.blocks[index].clone();
                let selected = self.eq.selected == Some(EqSelection::Block(index));
                let weak_select = weak.clone();
                let weak_toggle = weak.clone();
                let weak_remove = weak.clone();
                panel = panel.child(eq_chain_block_row(
                    pal,
                    preset,
                    &block,
                    selected,
                    move |_, _, cx| {
                        let _ = weak_select.update(cx, |app, cx| {
                            app.select_eq_block(index, cx);
                        });
                    },
                    move |_, _, cx| {
                        let _ = weak_toggle.update(cx, |app, cx| {
                            app.set_eq_block_enabled(index, !block.enabled, cx);
                        });
                    },
                    move |_, _, cx| {
                        let _ = weak_remove.update(cx, |app, cx| {
                            app.remove_eq_block(index, cx);
                        });
                    },
                ));
            }
            panel = panel.child(eq_divider(pal));
        }

        // ---- 自定义滤波器 ----
        {
            let weak_add = weak.clone();
            panel = panel.child(eq_stage_header(
                pal,
                Icon::Sliders,
                "自定义滤波器",
                self.eq.custom.len(),
                Some(
                    div()
                        .id("eq-chain-c-custom-add")
                        .h(px(22.0))
                        .px(px(7.0))
                        .rounded(px(6.0))
                        .bg(pal.chip)
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .cursor_pointer()
                        .text_size(px(8.5))
                        .text_color(pal.text_dim)
                        .child("添加")
                        .child(icons::icon(Icon::Plus, 10.0, pal.text_faint))
                        .on_click(move |_, _, cx| {
                            let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                        })
                        .into_any_element(),
                ),
            ));
            if self.eq.custom.is_empty() {
                panel = panel.child(
                    div()
                        .px(px(6.0))
                        .py(px(5.0))
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("点按曲线空白处添加第一个自定义频段"),
                );
            }
            for (index, band) in self.eq.custom.iter().enumerate() {
                let band = band.clone();
                let selected = self.eq.selected == Some(EqSelection::Custom(index));
                let weak_select = weak.clone();
                let weak_toggle = weak.clone();
                let weak_remove = weak.clone();
                panel = panel.child(eq_chain_custom_row(
                    pal,
                    index,
                    &band,
                    selected,
                    move |_, _, cx| {
                        let _ = weak_select.update(cx, |app, cx| {
                            app.select_eq_band(index, cx);
                        });
                    },
                    move |_, _, cx| {
                        let _ = weak_toggle.update(cx, |app, cx| {
                            app.set_eq_band_enabled(index, !band.enabled, cx);
                        });
                    },
                    move |_, _, cx| {
                        let _ = weak_remove.update(cx, |app, cx| {
                            app.eq.remove_custom(index);
                            app.apply_eq(cx);
                        });
                    },
                ));
            }
        }
        panel.into_any_element()
    }
}
