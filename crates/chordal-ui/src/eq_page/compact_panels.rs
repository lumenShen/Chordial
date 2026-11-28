//! EQ 页面区块：compact_panels（紧凑布局的检视器与预设库）。

use super::controls::*;
use super::*;

impl ChordialApp {
    pub(super) fn eq_inspector_compact(
        &self,
        pal: &EqPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut panel = div()
            .w_full()
            .rounded(px(16.0))
            .bg(pal.panel)
            .border_1()
            .border_color(pal.border_strong)
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(12.0));

        match self.eq.selected {
            Some(EqSelection::Custom(index)) => {
                if let Some(band) = self.eq.custom.get(index).cloned() {
                    panel = panel.child(self.eq_custom_inspector(pal, index, &band, true, cx));
                    panel = panel.child(eq_divider(pal));
                }
            }
            Some(EqSelection::Block(index)) => {
                if let Some(block) = self.eq.blocks.get(index).cloned()
                    && let Some(preset) = self.eq.block_preset(index)
                {
                    panel = panel.child(self.eq_block_inspector(pal, index, preset, &block, cx));
                    panel = panel.child(eq_divider(pal));
                }
            }
            Some(EqSelection::Profile) => {
                if let Some(profile) = self.eq.profile.clone() {
                    panel = panel.child(self.eq_profile_inspector(pal, &profile, cx));
                    panel = panel.child(eq_divider(pal));
                }
            }
            None => {
                panel = panel.child(self.eq_chain_overview(pal));
                panel = panel.child(eq_divider(pal));
            }
        }

        panel = panel.child(self.eq_master_block(pal, true, cx));

        if self.eq.dirty {
            let weak = weak.clone();
            panel = panel.child(
                div()
                    .id("eq-save-compact")
                    .h(px(42.0))
                    .w_full()
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(pal.accent_line)
                    .bg(pal.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(pal.accent)
                    .hover(|s| s.bg(pal.accent_soft.opacity(0.8)))
                    .child(icons::icon(Icon::Plus, 12.0, pal.accent))
                    .child("另存为个人预设")
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.save_eq_preset(cx));
                    }),
            );
        } else {
            panel = panel.child(
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(9.0))
                    .text_color(pal.text_faint)
                    .child("调整会自动保存；需要复用整条链路时点「另存为」"),
            );
        }
        panel.into_any_element()
    }

    // ---------- 紧凑：内置预设库 ----------

    pub(super) fn eq_builtin_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let entries = builtin_presets();
        let filtered: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, preset)| {
                self.eq
                    .builtin_group
                    .is_none_or(|group| preset.group == group)
            })
            .map(|(index, _)| index)
            .collect();
        let preview = self
            .eq
            .preview_builtin
            .and_then(builtin_preset)
            .filter(|preset| {
                self.eq
                    .builtin_group
                    .is_none_or(|group| preset.group == group)
            });

        let mut body = div().w_full().flex().flex_col().gap(px(8.0)).child(
            div()
                .pt(px(2.0))
                .flex()
                .items_end()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .text_size(px(17.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(pal.text)
                                .child("内置预设"),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(pal.text_faint)
                                .child("单点增益 · 可叠加、可调幅度"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(9.5))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .child(SharedString::from(format!(
                            "{} / {} 已加入",
                            self.eq.blocks.len(),
                            entries.len()
                        ))),
                ),
        );

        let mut chips = div()
            .id("eq-builtin-chips")
            .flex()
            .items_center()
            .gap(px(6.0))
            .overflow_x_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll());
        {
            let weak = weak.clone();
            chips = chips.child(eq_chip(
                pal,
                "全部",
                self.eq.builtin_group.is_none(),
                "eq-builtin-group-c-all",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.eq.builtin_group = None;
                        cx.notify();
                    });
                },
            ));
        }
        for group in PresetGroup::ALL {
            let weak = weak.clone();
            let active = self.eq.builtin_group == Some(group);
            chips = chips.child(eq_chip(
                pal,
                group.label(),
                active,
                SharedString::from(format!("eq-builtin-group-c-{}", group.label())),
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.eq.builtin_group = Some(group);
                        cx.notify();
                    });
                },
            ));
        }
        body = body.child(chips);

        if let Some(preset) = preview {
            let preset_id = preset.id;
            let block = self
                .eq
                .blocks
                .iter()
                .find(|block| block.preset_id == preset.id)
                .cloned();
            let weak_add = weak.clone();
            let on_add: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                let _ = weak_add.update(cx, |app, cx| {
                    app.toggle_builtin_membership(preset_id, cx);
                });
            });
            let weak_power = weak.clone();
            let on_power: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                let _ = weak_power.update(cx, |app, cx| {
                    app.toggle_builtin_power(preset_id, cx);
                });
            });
            let weak_amount = weak.clone();
            let on_amount: Rc<dyn Fn(f32, &mut App)> = Rc::new(move |amount, cx: &mut App| {
                let _ = weak_amount.update(cx, |app, cx| {
                    if let Some(index) = app.eq.block_index(preset_id) {
                        app.set_eq_block_amount(index, amount, cx);
                    }
                });
            });
            let weak_close = weak.clone();
            body = body.child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(9.0))
                                    .text_color(pal.text_faint)
                                    .child("预设预览"),
                            )
                            .child(
                                div()
                                    .id("eq-builtin-preview-close")
                                    .h(px(20.0))
                                    .px(px(7.0))
                                    .rounded(px(10.0))
                                    .bg(pal.chip)
                                    .flex()
                                    .items_center()
                                    .gap(px(3.0))
                                    .cursor_pointer()
                                    .text_size(px(8.5))
                                    .text_color(pal.text_faint)
                                    .child(icons::icon(Icon::Close, 9.0, pal.text_faint))
                                    .child("收起")
                                    .on_click(move |_, _, cx| {
                                        let _ = weak_close.update(cx, |app, cx| {
                                            app.eq.preview_builtin = None;
                                            cx.notify();
                                        });
                                    }),
                            ),
                    )
                    .child(eq_builtin_preview_panel(
                        pal,
                        preset,
                        block.as_ref(),
                        true,
                        on_add,
                        on_power,
                        on_amount,
                    )),
            );
        }

        if filtered.is_empty() {
            return body
                .child(eq_notice(
                    pal,
                    Icon::Layers,
                    "这个分组还没有预设",
                    "切换到「全部」查看完整的单点增益列表。",
                    None,
                ))
                .into_any_element();
        }

        for (row, &index) in filtered.iter().enumerate() {
            let preset = &entries[index];
            let block = self
                .eq
                .blocks
                .iter()
                .find(|block| block.preset_id == preset.id)
                .cloned();
            let added = block.is_some();
            let enabled = block.as_ref().is_some_and(|block| block.enabled);
            let previewed = self.eq.preview_builtin == Some(preset.id);
            let preset_id = preset.id;
            let weak_preview = weak.clone();
            let weak_add = weak.clone();
            let weak_power = weak.clone();
            body = body.child(eq_builtin_row(
                pal,
                row,
                preset,
                added,
                enabled,
                previewed,
                true,
                move |_, _, cx| {
                    let _ = weak_preview.update(cx, |app, cx| {
                        app.eq.preview_builtin = Some(preset_id);
                        cx.notify();
                    });
                },
                move |_, _, cx| {
                    let _ = weak_add.update(cx, |app, cx| {
                        app.toggle_builtin_membership(preset_id, cx);
                    });
                },
                move |_, _, cx| {
                    let _ = weak_power.update(cx, |app, cx| {
                        app.toggle_builtin_power(preset_id, cx);
                    });
                },
            ));
        }
        body.into_any_element()
    }

    // ---------- 紧凑：AutoEQ 耳机档案 ----------

    pub(super) fn eq_autoeq_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, _subtitle) = tab_meta(EqTab::AutoEq);
        let lib = &self.eq.autoeq;
        let entries = lib.entries.clone();
        let total = entries.len();
        let filtered = lib.visible_indices(&self.eq.category, &self.eq.search);
        let filtered_count = filtered.len();
        let profile_name = self.eq.profile.as_ref().map(|profile| profile.name.clone());
        let loading = lib.index_loading || lib.status == AutoEqStatus::Loading;
        let mut body = div().w_full().flex().flex_col().gap(px(8.0)).child(
            div()
                .pt(px(2.0))
                .flex()
                .items_end()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .text_size(px(17.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(pal.text)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(pal.text_faint)
                                .text_ellipsis()
                                .child(SharedString::from(autoeq_status_text(lib, filtered_count))),
                        ),
                )
                .child({
                    let weak_sync = weak.clone();
                    eq_sync_chip(
                        pal,
                        "eq-autoeq-sync-c",
                        lib.index_loading,
                        move |_, _, cx| {
                            let _ = weak_sync.update(cx, |app, cx| app.sync_autoeq(true, cx));
                        },
                    )
                }),
        );

        body = body.child(eq_search_box(pal, self.eq_search_view.clone()));
        body = body.child({
            let mut chips = div()
                .id("eq-autoeq-chips")
                .flex()
                .items_center()
                .gap(px(6.0))
                .overflow_x_scroll()
                .when(self.smooth_scroll, |d| d.smooth_scroll());
            for label in ["全部", "头戴式", "入耳式", "耳塞式"] {
                let weak = weak.clone();
                let active = self.eq.category == label;
                chips = chips.child(eq_chip(
                    pal,
                    label,
                    active,
                    SharedString::from(format!("eq-cat-c-{label}")),
                    move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.eq.category = label.to_string();
                            cx.notify();
                        });
                    },
                ));
            }
            chips.into_any_element()
        });

        if let Some(error) = &self.eq.autoeq.error
            && total > 0
        {
            body = body.child(
                div()
                    .text_size(px(9.0))
                    .text_color(pal.danger)
                    .text_ellipsis()
                    .child(SharedString::from(format!("同步失败：{error}"))),
            );
        }

        if total == 0 {
            if loading {
                return body
                    .child(eq_notice(
                        pal,
                        Icon::Refresh,
                        "正在从 AutoEQ 仓库拉取…",
                        "索引约 500 KB，首次加载需要几秒；完成后会缓存到本地。",
                        None,
                    ))
                    .into_any_element();
            }
            let hint = lib.error.clone().unwrap_or_else(|| {
                "从 github.com/jaakkopasanen/AutoEq 拉取 6000+ 款耳机的校准曲线，之后离线可用。"
                    .to_string()
            });
            let weak_retry = weak.clone();
            let action: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
                let _ = weak_retry.update(cx, |app, cx| app.sync_autoeq(true, cx));
            });
            return body
                .child(eq_notice(
                    pal,
                    Icon::Cloud,
                    if lib.error.is_some() {
                        "AutoEQ 索引拉取失败"
                    } else {
                        "尚未拉取 AutoEQ 耳机档案"
                    },
                    &hint,
                    Some(("拉取耳机档案", action)),
                ))
                .into_any_element();
        }

        if filtered.is_empty() {
            return body
                .child(eq_notice(
                    pal,
                    Icon::Headphones,
                    "没有匹配的耳机",
                    "换个分类，或输入型号关键词后按回车搜索。",
                    None,
                ))
                .into_any_element();
        }

        if filtered_count > COMPACT_AUTOEQ_ROWS {
            body = body.child(
                div()
                    .text_size(px(9.0))
                    .text_color(pal.text_faint)
                    .text_center()
                    .child(SharedString::from(format!(
                        "共 {filtered_count} 项，仅显示前 {COMPACT_AUTOEQ_ROWS} 项；输入关键词后按回车缩小范围"
                    ))),
            );
        }

        for &index in filtered.iter().take(COMPACT_AUTOEQ_ROWS) {
            let entry = &entries[index];
            let weak_row = weak.clone();
            let file = entry.file.clone();
            let active = profile_name.as_deref() == Some(entry.name.as_str());
            let previewed = self.eq.autoeq_preview.as_deref() == Some(entry.file.as_str());
            let badge = if entry.loading {
                Some("拉取中".to_string())
            } else if entry.error.is_some() {
                Some("失败".to_string())
            } else {
                entry
                    .bands
                    .as_ref()
                    .map(|bands| format!("{} 段", bands.len()))
            };
            body = body.child(eq_preset_row(
                pal,
                &entry.name,
                entry.subtitle(),
                Some(Icon::Headphones),
                badge,
                active,
                previewed,
                true,
                SharedString::from(format!("eq-autoeq-c-{index}")),
                move |_, _, cx| {
                    let _ = weak_row.update(cx, |app, cx| {
                        app.select_autoeq_entry(file.clone(), true, cx);
                    });
                },
            ));
        }
        body.into_any_element()
    }

    pub(super) fn eq_saved_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut body = div().w_full().flex().flex_col().gap(px(8.0)).child(
            div()
                .pt(px(2.0))
                .flex()
                .items_end()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .text_size(px(17.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(pal.text)
                                .child("我的配置"),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(pal.text_faint)
                                .child("「另存为」保存的完整链路"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(9.5))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .child(SharedString::from(format!("{} 项", self.eq.saved.len()))),
                ),
        );

        if self.eq.saved.is_empty() {
            body = body.child(
                div()
                    .w_full()
                    .h(px(200.0))
                    .rounded(px(14.0))
                    .bg(pal.panel)
                    .border_1()
                    .border_color(pal.border)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .child(icons::icon(Icon::Person, 22.0, pal.text_faint))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(pal.text_dim)
                            .child("还没有个人配置"),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child("在编辑器里调好链路后点「另存为个人预设」"),
                    ),
            );
            return body.into_any_element();
        }

        for (index, preset) in self.eq.saved.iter().enumerate() {
            let weak_load = weak.clone();
            let weak_remove = weak.clone();
            let name = preset.name.clone();
            let description = format!(
                "{} 个滤波器 · 输入 {}",
                preset.settings.bands.len(),
                fmt_db(preset.settings.input_gain_db)
            );
            let active = !self.eq.dirty && self.eq.preset_name == name;
            body = body.child(
                div()
                    .id(SharedString::from(format!("eq-saved-c-{index}")))
                    .w_full()
                    .h(px(60.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(if active { pal.accent_line } else { pal.border })
                    .bg(if active { pal.selected_bg } else { pal.panel })
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = weak_load.update(cx, |app, cx| {
                            app.apply_saved_eq_preset(index, cx);
                        });
                    })
                    .child(
                        div()
                            .size(px(32.0))
                            .rounded(px(9.0))
                            .bg(pal.chip)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icons::icon(Icon::Person, 15.0, pal.text_dim)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(if active { pal.accent } else { pal.text })
                                    .text_ellipsis()
                                    .child(SharedString::from(name)),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(pal.text_faint)
                                    .text_ellipsis()
                                    .child(SharedString::from(description)),
                            ),
                    )
                    .when(active, |d| {
                        d.child(icons::icon(Icon::Check, 13.0, pal.accent))
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("eq-saved-c-remove-{index}")))
                            .size(px(30.0))
                            .rounded(px(8.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(pal.danger_soft))
                            .child(icons::icon(Icon::Trash, 13.0, pal.danger))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = weak_remove.update(cx, |app, cx| {
                                    app.remove_saved_preset(index, cx);
                                });
                            }),
                    ),
            );
        }
        body.into_any_element()
    }

    pub(super) fn eq_nav_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut row = div()
            .h(px(60.0))
            .w_full()
            .flex_none()
            .border_t_1()
            .border_color(pal.divider)
            .bg(pal.panel)
            .flex()
            .items_center();
        for tab in EqTab::ALL {
            let weak = weak.clone();
            let active = self.eq.tab == tab;
            let icon = tab_icon(tab);
            row = row.child(
                div()
                    .id(SharedString::from(format!("eq-nav-{}", tab.label())))
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(2.0))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.select_eq_tab(tab, cx);
                        });
                    })
                    .child(
                        div()
                            .h(px(22.0))
                            .px(px(9.0))
                            .rounded(px(7.0))
                            .bg(if active {
                                pal.accent_soft
                            } else {
                                gpui::transparent_black()
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(match icon {
                                Some(icon) => icons::icon(
                                    icon,
                                    16.0,
                                    if active { pal.accent } else { pal.text_faint },
                                ),
                                None => div().into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(9.0))
                            .text_color(if active { pal.accent } else { pal.text_faint })
                            .child(tab.label()),
                    ),
            );
        }
        row.into_any_element()
    }
}
