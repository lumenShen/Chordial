//! EQ 页面区块：desktop_shell（桌面 Studio 控制台）。

use super::controls::*;
use super::*;
use crate::eq::EqState;

/// 链路区块右上角的小动作（更换 / 添加）。
fn chain_action(
    pal: &EqPalette,
    id: &'static str,
    label: &'static str,
    icon: Icon,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .h(px(22.0))
        .px(px(7.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(3.0))
        .cursor_pointer()
        .bg(pal.chip)
        .hover(|s| s.bg(pal.chip_hover))
        .child(
            div()
                .text_size(px(8.5))
                .text_color(pal.text_dim)
                .child(label),
        )
        .child(icons::icon(icon, 10.0, pal.text_faint))
        .on_click(on_click)
        .into_any_element()
}

impl ChordialApp {
    // ---------- 桌面：Studio 控制台 ----------

    pub(super) fn eq_render_desktop(
        &self,
        pal: &EqPalette,
        enter: f32,
        mouse: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            EqTab::Edit => {
                let start = Instant::now();
                let editor = if eq_probe("editor") {
                    div().flex_1().into_any_element()
                } else {
                    self.eq_editor_desktop(pal, panel_w, panel_h, mouse, cx)
                };
                crate::frame_trace::add("eq.editor", start.elapsed());
                let start = Instant::now();
                let inspector = if eq_probe("inspector") {
                    div().into_any_element()
                } else {
                    self.eq_inspector_desktop(pal, main_h, mouse, cx)
                };
                crate::frame_trace::add("eq.inspector", start.elapsed());
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .child(editor)
                    .child(inspector)
                    .into_any_element()
            }
            EqTab::AutoEq => self.eq_autoeq_desktop(pal, main_h, cx).into_any_element(),
            EqTab::Builtin => self.eq_builtin_desktop(pal, main_h, cx).into_any_element(),
            EqTab::Personal => self.eq_saved_desktop(pal, main_h, cx).into_any_element(),
        };
        let body = if eq_probe("rail") {
            div().flex_1().min_h_0().into_any_element()
        } else {
            let start = Instant::now();
            let rail = self.eq_desktop_rail(pal, cx);
            crate::frame_trace::add("eq.rail", start.elapsed());
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(rail)
                .child(content)
                .into_any_element()
        };

        let start = Instant::now();
        let header = if eq_probe("header") {
            div().into_any_element()
        } else {
            self.eq_header_desktop(pal, panel_w, cx)
        };
        crate::frame_trace::add("eq.header", start.elapsed());
        let start = Instant::now();
        let statusbar = if eq_probe("statusbar") {
            div().into_any_element()
        } else {
            self.eq_statusbar_desktop(pal)
        };
        crate::frame_trace::add("eq.statusbar", start.elapsed());

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
            .child(header)
            .child(body)
            .child(statusbar);

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
        let chain = format!(
            "{} 档案 · {} 预设 · {} 自定义",
            self.eq.profile_band_count(),
            self.eq.enabled_block_count(),
            self.eq.custom.len()
        );
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
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(15.0))
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
                                    .child(SharedString::from(chain)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .child(
                                div()
                                    .max_w(px(240.0))
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
                                SharedString::from(format!("· {} 个节点", self.eq.total_bands())),
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
        for (index, (tab, count)) in counts.into_iter().enumerate() {
            if index == 1 {
                rail = rail.child(
                    div()
                        .h(px(24.0))
                        .pt(px(10.0))
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
                        .child("当前链路"),
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
                                "{} 档案 · {} 预设 · {} 自定义",
                                self.eq.profile_band_count(),
                                self.eq.blocks.len(),
                                self.eq.custom.len()
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
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(9.0))
                                .font_family(pal.mono.clone())
                                .text_color(pal.text_faint)
                                .child(SharedString::from(format!(
                                    "{} / {} 节点",
                                    self.eq.total_bands(),
                                    EQ_MAX_BANDS
                                ))),
                        )
                        .child(
                            div()
                                .text_size(px(9.0))
                                .text_color(if self.eq.settings.enabled {
                                    pal.ok
                                } else {
                                    pal.text_faint
                                })
                                .child(if self.eq.settings.enabled {
                                    "实时生效"
                                } else {
                                    "已旁路"
                                }),
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
        mouse: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let main_w = (panel_w - RAIL_W - INSPECTOR_W).max(280.0);
        // 工具栏 28 + 提示 16 + 3 个 10px gap + 上下 14px padding。
        let remaining = (panel_h - 192.0).max(200.0);
        let chain_h = (remaining * 0.45).clamp(180.0, 300.0);
        let graph_h = (remaining - chain_h).max(140.0);

        let toolbar = {
            if eq_probe("toolbar") {
                div().h(px(28.0)).w_full().flex_none().into_any_element()
            } else {
                let weak_add = weak.clone();
                let weak_remove = weak.clone();
                let selection = self.eq.selected;
                let full = self.eq.remaining_bands() == 0;
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
                                        "{} / {} 节点",
                                        self.eq.total_bands(),
                                        EQ_MAX_BANDS
                                    ))),
                            ),
                    )
                    .child(div().flex_1())
                    .when(selection.is_some(), |d| {
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
                                .child("移除节点")
                                .on_click(move |_, _, cx| {
                                    let _ =
                                        weak_remove.update(cx, |app, cx| app.remove_eq_band(cx));
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
                                    .child("自定义滤波器"),
                            )
                            .when(!full, |d| {
                                d.on_click(move |_, _, cx| {
                                    let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                                })
                            }),
                    )
                    .into_any_element()
            }
        };

        let hint = if eq_probe("hint") {
            div().h(px(16.0)).w_full().flex_none().into_any_element()
        } else {
            div()
                .h(px(16.0))
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(9.0))
                .text_color(pal.text_faint)
                .child("底部链路点选节点，在右侧精调")
                .child("·")
                .child("曲线空白处添加自定义频段")
                .child("·")
                .child("拖动自定义节点改频率/增益")
                .child("·")
                .child("右键删除节点")
                .into_any_element()
        };

        let graph_start = Instant::now();
        let graph = if eq_probe("graph") {
            div().w_full().h(px(graph_h)).into_any_element()
        } else {
            self.eq_graph(pal, main_w - 28.0, graph_h, false, cx)
        };
        crate::frame_trace::add("eq.graph", graph_start.elapsed());
        let chain_start = Instant::now();
        let chain = if eq_probe("chain") {
            div().w_full().h(px(chain_h)).into_any_element()
        } else {
            self.eq_chain_desktop(pal, chain_h, mouse, cx)
        };
        crate::frame_trace::add("eq.chain", chain_start.elapsed());

        div()
            .w(px(main_w))
            .flex_none()
            .h_full()
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(toolbar)
            .child(graph)
            .child(chain)
            .child(hint)
            .into_any_element()
    }

    /// 编辑器底部链路：耳机档案 → 内置预设 → 自定义滤波器。
    ///
    /// 每行只负责“选中”，参数统一在右侧检视器里调整；点击行内圆点可
    /// 快速旁路，垃圾桶直接移除节点。
    pub(super) fn eq_chain_desktop(
        &self,
        pal: &EqPalette,
        height: f32,
        mouse: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut body = div()
            .id("eq-chain-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .track_scroll(&self.eq_chain_scroll)
            .overflow_y_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll())
            .flex()
            .flex_col()
            .gap(px(2.0))
            .pr(px(3.0));

        // ---- 耳机档案 ----
        {
            let weak_open = weak.clone();
            let action = chain_action(
                pal,
                "eq-chain-profile-open",
                "更换",
                Icon::ChevronRight,
                move |_, _, cx| {
                    let _ = weak_open.update(cx, |app, cx| {
                        app.select_eq_tab(EqTab::AutoEq, cx);
                    });
                },
            );
            body = body.child(eq_stage_header(
                pal,
                Icon::Headphones,
                "耳机档案",
                self.eq.profile_band_count(),
                Some(action),
            ));
            match &self.eq.profile {
                Some(profile) => {
                    let weak_select = weak.clone();
                    let weak_clear = weak.clone();
                    let selected = self.eq.selected == Some(EqSelection::Profile);
                    body = body.child(
                        div()
                            .id("eq-chain-profile")
                            .h(px(36.0))
                            .w_full()
                            .flex_none()
                            .px(px(6.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .gap(px(7.0))
                            .cursor_pointer()
                            .bg(if selected {
                                pal.selected_bg
                            } else {
                                gpui::transparent_black()
                            })
                            .hover(|s| s.bg(pal.chip_hover))
                            .child(
                                div()
                                    .size(px(22.0))
                                    .rounded(px(6.0))
                                    .bg(pal.accent_soft)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icons::icon(Icon::Headphones, 12.0, pal.accent)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(0.0))
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(if selected {
                                                pal.accent
                                            } else {
                                                pal.text
                                            })
                                            .text_ellipsis()
                                            .child(SharedString::from(profile.name.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(8.5))
                                            .text_color(pal.text_faint)
                                            .text_ellipsis()
                                            .child(SharedString::from(profile.subtitle())),
                                    ),
                            )
                            .child(eq_mini_icon_button(
                                pal,
                                Icon::Trash,
                                22.0,
                                SharedString::from("eq-chain-profile-remove"),
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
                    body = body.child(
                        div()
                            .id("eq-chain-profile-empty")
                            .h(px(32.0))
                            .w_full()
                            .flex_none()
                            .px(px(8.0))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(pal.border)
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(pal.chip_hover))
                            .child(icons::icon(Icon::Headphones, 12.0, pal.text_faint))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(9.5))
                                    .text_color(pal.text_faint)
                                    .child("未加载耳机档案"),
                            )
                            .child(
                                div()
                                    .text_size(px(9.0))
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
            body = body.child(eq_divider(pal));
        }

        // ---- 内置预设 ----
        {
            let weak_add = weak.clone();
            let action = chain_action(
                pal,
                "eq-chain-block-add",
                "添加",
                Icon::Plus,
                move |_, _, cx| {
                    let _ = weak_add.update(cx, |app, cx| {
                        app.select_eq_tab(EqTab::Builtin, cx);
                    });
                },
            );
            body = body.child(eq_stage_header(
                pal,
                Icon::Layers,
                "内置预设",
                self.eq.blocks.len(),
                Some(action),
            ));
            if self.eq.blocks.is_empty() {
                body = body.child(
                    div()
                        .px(px(8.0))
                        .py(px(6.0))
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("还没有启用预设 · 从「内置预设」加入单点增益"),
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
                body = body.child(eq_chain_block_row(
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
            body = body.child(eq_divider(pal));
        }

        // ---- 自定义滤波器 ----
        {
            let weak_add = weak.clone();
            let full = self.eq.remaining_bands() == 0;
            let action = chain_action(
                pal,
                "eq-chain-custom-add",
                "添加",
                Icon::Plus,
                move |_, _, cx| {
                    if full {
                        return;
                    }
                    let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                },
            );
            body = body.child(eq_stage_header(
                pal,
                Icon::Sliders,
                "自定义滤波器",
                self.eq.custom.len(),
                Some(action),
            ));
            if self.eq.custom.is_empty() {
                body = body.child(
                    div()
                        .px(px(8.0))
                        .py(px(6.0))
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("点按曲线空白处，或点右上角「添加」新建频段"),
                );
            }
            for (index, band) in self.eq.custom.iter().enumerate() {
                let band = band.clone();
                let selected = self.eq.selected == Some(EqSelection::Custom(index));
                let weak_select = weak.clone();
                let weak_toggle = weak.clone();
                let weak_remove = weak.clone();
                body = body.child(eq_chain_custom_row(
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
                            app.eq.selected = Some(EqSelection::Custom(index));
                            app.remove_eq_band(cx);
                        });
                    },
                ));
            }
        }

        let card = div()
            .id("eq-chain")
            .w_full()
            .h(px(height))
            .flex_none()
            .rounded(px(12.0))
            .bg(pal.inset)
            .border_1()
            .border_color(pal.border)
            .px(px(8.0))
            .py(px(6.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .h(px(18.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(8.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(pal.text_faint)
                            .child("信号链"),
                    )
                    .child(
                        div()
                            .text_size(px(8.0))
                            .font_family(pal.mono.clone())
                            .text_color(pal.text_faint)
                            .child(SharedString::from(format!(
                                "{} 预设 · {} 自定义 · {} / {} 节点",
                                self.eq.blocks.len(),
                                self.eq.custom.len(),
                                self.eq.total_bands(),
                                EQ_MAX_BANDS
                            ))),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(8.0))
                            .text_color(pal.text_faint)
                            .child("点选后在右侧调整 · 列表可滚动"),
                    ),
            )
            .child(body);

        if std::env::var_os("CHORDIAL_NO_RETAINED").is_some() {
            return card.into_any_element();
        }
        let version = eq_chain_version(
            &self.eq,
            pal,
            height,
            self.eq_chain_scroll.offset(),
            mouse,
            self.smooth_scroll,
        );
        let mut refinement = gpui::StyleRefinement::default();
        refinement = refinement.w_full().h(px(height)).flex_none();
        let mut style = gpui::Style::default();
        style.refine(&refinement);
        gpui::Retained::new("eq-chain-retained", version, style, move |_window, _cx| {
            card.into_any_element()
        })
        .into_any_element()
    }
}

/// 信号链的保留版本：设置、选择、滚动、鼠标悬停、主题与高度任一变化都重建。
fn eq_chain_version(
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
    (eq.preset_name.as_str(), eq.dirty).hash(&mut hasher);
    smooth_scroll.hash(&mut hasher);
    height.to_bits().hash(&mut hasher);
    f32::from(scroll.x).to_bits().hash(&mut hasher);
    f32::from(scroll.y).to_bits().hash(&mut hasher);
    f32::from(mouse.x).to_bits().hash(&mut hasher);
    f32::from(mouse.y).to_bits().hash(&mut hasher);
    for color in [
        pal.inset,
        pal.border,
        pal.text,
        pal.text_dim,
        pal.text_faint,
        pal.accent,
        pal.accent_soft,
        pal.selected_bg,
        pal.chip_hover,
        pal.danger,
        pal.danger_soft,
        pal.divider,
    ] {
        color.h.to_bits().hash(&mut hasher);
        color.s.to_bits().hash(&mut hasher);
        color.l.to_bits().hash(&mut hasher);
        color.a.to_bits().hash(&mut hasher);
    }
    (pal.font.as_ref(), pal.mono.as_ref()).hash(&mut hasher);
    hasher.finish()
}
