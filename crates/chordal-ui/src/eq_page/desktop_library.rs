//! EQ 页面区块：预设库（内置预设 / AutoEQ 耳机档案 / 个人配置）。

use super::controls::*;
use super::*;

impl ChordialApp {
    // ---------- 桌面：内置预设 ----------

    pub(super) fn eq_builtin_desktop(
        &self,
        pal: &EqPalette,
        main_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, subtitle) = tab_meta(EqTab::Builtin);
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
            })
            .or_else(|| filtered.first().map(|&index| &entries[index]));

        let added_count = self.eq.blocks.len();
        let head = div()
            .h(px(38.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
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
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child(subtitle),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .h(px(24.0))
                    .px(px(9.0))
                    .rounded(px(12.0))
                    .bg(if added_count > 0 {
                        pal.accent_soft
                    } else {
                        pal.chip
                    })
                    .flex()
                    .items_center()
                    .text_size(px(9.5))
                    .font_family(pal.mono.clone())
                    .text_color(if added_count > 0 {
                        pal.accent
                    } else {
                        pal.text_dim
                    })
                    .child(SharedString::from(format!(
                        "{} / {} 已加入",
                        added_count,
                        entries.len()
                    ))),
            );

        let mut chips = div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0));
        let weak_all = weak.clone();
        chips = chips.child(eq_chip(
            pal,
            "全部",
            self.eq.builtin_group.is_none(),
            "eq-builtin-group-all",
            move |_, _, cx| {
                let _ = weak_all.update(cx, |app, cx| {
                    app.eq.builtin_group = None;
                    cx.notify();
                });
            },
        ));
        for group in PresetGroup::ALL {
            let weak = weak.clone();
            let active = self.eq.builtin_group == Some(group);
            chips = chips.child(eq_chip(
                pal,
                group.label(),
                active,
                SharedString::from(format!("eq-builtin-group-{}", group.label())),
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.eq.builtin_group = Some(group);
                        cx.notify();
                    });
                },
            ));
        }
        chips = chips.child(div().flex_1()).child(
            div()
                .text_size(px(9.0))
                .font_family(pal.mono.clone())
                .text_color(pal.text_faint)
                .child(SharedString::from(format!("{} 项", filtered.len()))),
        );

        let list: AnyElement = if filtered.is_empty() {
            eq_notice(
                pal,
                Icon::Layers,
                "这个分组还没有预设",
                "切换到「全部」查看完整的单点增益列表。",
                None,
            )
        } else {
            let count = filtered.len();
            let pal_list = pal.clone();
            let weak_list = weak.clone();
            let preview_id = preview.map(|preset| preset.id);
            let blocks = self.eq.blocks.clone();
            let mut list =
                gpui::uniform_list("eq-builtin-list", count, move |range, _window, _cx| {
                    range
                        .map(|row| {
                            let index = filtered[row];
                            let preset = &entries[index];
                            let block = blocks
                                .iter()
                                .find(|block| block.preset_id == preset.id)
                                .cloned();
                            let added = block.is_some();
                            let enabled = block.as_ref().is_some_and(|block| block.enabled);
                            let previewed = preview_id == Some(preset.id);
                            let weak_preview = weak_list.clone();
                            let weak_add = weak_list.clone();
                            let weak_power = weak_list.clone();
                            let preset_id = preset.id;
                            div()
                                .h(px(70.0))
                                .pb(px(6.0))
                                .child(eq_builtin_row(
                                    &pal_list,
                                    index,
                                    preset,
                                    added,
                                    enabled,
                                    previewed,
                                    false,
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
                                ))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                });
            if self.smooth_scroll {
                list = list.smooth_scroll();
            }
            list.flex_1()
                .min_w_0()
                .h_full()
                .pr(px(4.0))
                .into_any_element()
        };

        let preview: AnyElement = match preview {
            Some(preset) => {
                let block = self
                    .eq
                    .blocks
                    .iter()
                    .find(|block| block.preset_id == preset.id)
                    .cloned();
                let preset_id = preset.id;
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
                eq_builtin_preview_panel(
                    pal,
                    preset,
                    block.as_ref(),
                    false,
                    on_add,
                    on_power,
                    on_amount,
                )
            }
            None => eq_preview_empty(pal, "选择一个内置预设"),
        };

        let _ = main_h;
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(px(14.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(head)
            .child(chips)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(12.0))
                    .child(list)
                    .child(
                        div()
                            .id("eq-builtin-preview-pane")
                            .w(px(300.0))
                            .h_full()
                            .flex_none()
                            .overflow_y_scroll()
                            .when(self.smooth_scroll, |d| d.smooth_scroll())
                            .child(preview),
                    ),
            )
            .into_any_element()
    }

    // ---------- 桌面：AutoEQ 耳机档案 ----------

    pub(super) fn eq_autoeq_desktop(
        &self,
        pal: &EqPalette,
        main_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, subtitle) = tab_meta(EqTab::AutoEq);
        let lib = &self.eq.autoeq;
        let entries = lib.entries.clone();
        let total = entries.len();
        let filtered = lib.visible_indices(&self.eq.category, &self.eq.search);
        let filtered_count = filtered.len();
        let profile_name = self.eq.profile.as_ref().map(|profile| profile.name.clone());

        let preview_index = self
            .eq
            .autoeq_preview
            .as_deref()
            .and_then(|file| {
                filtered
                    .iter()
                    .copied()
                    .find(|&index| entries[index].file == file)
            })
            .or_else(|| {
                profile_name.as_ref().and_then(|name| {
                    filtered
                        .iter()
                        .copied()
                        .find(|&index| &entries[index].name == name)
                })
            })
            .or_else(|| filtered.first().copied());
        let preview_file = preview_index.map(|index| entries[index].file.clone());

        let mut head = div()
            .h(px(38.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
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
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child(subtitle),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .w(px(230.0))
                    .child(eq_search_box(pal, self.eq_search_view.clone())),
            );
        let weak_sync = weak.clone();
        head = head.child(eq_sync_chip(
            pal,
            "eq-autoeq-sync",
            lib.index_loading,
            move |_, _, cx| {
                let _ = weak_sync.update(cx, |app, cx| app.sync_autoeq(true, cx));
            },
        ));

        let mut chips = div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0));
        for label in ["全部", "头戴式", "入耳式", "耳塞式"] {
            let weak = weak.clone();
            let active = self.eq.category == label;
            chips = chips.child(eq_chip(
                pal,
                label,
                active,
                SharedString::from(format!("eq-cat-{label}")),
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.eq.category = label.to_string();
                        cx.notify();
                    });
                },
            ));
        }
        chips = chips.child(div().flex_1()).child(
            div()
                .text_size(px(9.0))
                .font_family(pal.mono.clone())
                .text_color(pal.text_faint)
                .child(SharedString::from(autoeq_status_text(lib, filtered_count))),
        );

        let loading = lib.index_loading || lib.status == AutoEqStatus::Loading;
        let content: AnyElement = if total == 0 {
            if loading {
                eq_notice(
                    pal,
                    Icon::Refresh,
                    "正在从 AutoEQ 仓库拉取…",
                    "索引约 500 KB，首次加载需要几秒；完成后会缓存到本地。",
                    None,
                )
            } else {
                let hint = lib.error.clone().unwrap_or_else(|| {
                    "从 github.com/jaakkopasanen/AutoEq 拉取 6000+ 款耳机的校准曲线，之后离线可用。"
                        .to_string()
                });
                let weak_retry = weak.clone();
                let action: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
                    let _ = weak_retry.update(cx, |app, cx| app.sync_autoeq(true, cx));
                });
                eq_notice(
                    pal,
                    Icon::Cloud,
                    if lib.error.is_some() {
                        "AutoEQ 索引拉取失败"
                    } else {
                        "尚未拉取 AutoEQ 耳机档案"
                    },
                    &hint,
                    Some(("拉取耳机档案", action)),
                )
            }
        } else if filtered.is_empty() {
            eq_notice(
                pal,
                Icon::Headphones,
                "没有匹配的耳机",
                "换个分类，或输入型号关键词后按回车搜索。",
                None,
            )
        } else {
            // visible_indices 已返回 Arc，直接复用（不再二次包装 / 复制 6000 项）。
            let indices = filtered;
            let count = indices.len();
            let rows_entries = entries.clone();
            let pal_rows = pal.clone();
            let weak_rows = weak.clone();
            let rows_preview = preview_file.clone();
            let rows_profile = profile_name.clone();
            let mut list =
                gpui::uniform_list("eq-autoeq-list", count, move |range, _window, _cx| {
                    range
                        .map(|row| {
                            if eq_probe("rows") {
                                return div().h(px(60.0)).into_any_element();
                            }
                            let index = indices[row];
                            let entry = &rows_entries[index];
                            let file = entry.file.clone();
                            let name = entry.name.clone();
                            let subtitle = entry.subtitle();
                            let previewed = rows_preview.as_deref() == Some(entry.file.as_str());
                            let active = rows_profile.as_deref() == Some(entry.name.as_str());
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
                            let weak_row = weak_rows.clone();
                            div()
                                .h(px(60.0))
                                .pb(px(6.0))
                                .child(eq_preset_row(
                                    &pal_rows,
                                    &name,
                                    subtitle,
                                    Some(Icon::Headphones),
                                    badge,
                                    active,
                                    previewed,
                                    false,
                                    SharedString::from(format!("eq-autoeq-{index}")),
                                    move |_, _, cx| {
                                        let _ = weak_row.update(cx, |app, cx| {
                                            app.select_autoeq_entry(file.clone(), false, cx);
                                        });
                                    },
                                ))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                });
            if self.smooth_scroll {
                list = list.smooth_scroll();
            }
            let list = list
                .flex_1()
                .min_w_0()
                .h_full()
                .pr(px(4.0))
                .into_any_element();

            let preview: AnyElement = match preview_index {
                Some(index) => {
                    let entry = &entries[index];
                    let active = profile_name.as_deref() == Some(entry.name.as_str());
                    match entry.to_settings() {
                        Some(settings) => {
                            let weak_apply = weak.clone();
                            let file = entry.file.clone();
                            let on_apply: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
                                let _ = weak_apply
                                    .update(cx, |app, cx| app.apply_autoeq_entry(&file, cx));
                            });
                            eq_preview_panel(
                                pal,
                                &settings,
                                &entry.name,
                                &entry.subtitle(),
                                active,
                                on_apply,
                                None,
                            )
                        }
                        None if entry.loading => eq_notice(
                            pal,
                            Icon::Refresh,
                            "正在拉取校准曲线…",
                            "从 AutoEQ 仓库下载这个型号的 ParametricEQ.txt。",
                            None,
                        ),
                        None => {
                            let file = entry.file.clone();
                            let weak_action = weak.clone();
                            let action: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
                                let _ = weak_action.update(cx, |app, cx| {
                                    app.select_autoeq_entry(file.clone(), false, cx);
                                });
                            });
                            match &entry.error {
                                Some(error) => eq_notice(
                                    pal,
                                    Icon::Alert,
                                    "拉取失败",
                                    error,
                                    Some(("重试", action)),
                                ),
                                None => eq_notice(
                                    pal,
                                    Icon::Cloud,
                                    "按需拉取",
                                    "点按左侧耳机型号下载曲线并预览；应用时不会重复下载。",
                                    Some(("拉取曲线", action)),
                                ),
                            }
                        }
                    }
                }
                None => eq_preview_empty(pal, "选择一个耳机型号"),
            };

            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(px(12.0))
                .child(list)
                .child(
                    div()
                        .id("eq-autoeq-preview-pane")
                        .w(px(300.0))
                        .h_full()
                        .flex_none()
                        .overflow_y_scroll()
                        .when(self.smooth_scroll, |d| d.smooth_scroll())
                        .child(preview),
                )
                .into_any_element()
        };

        let mut body = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(px(14.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(head)
            .child(chips);
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
        body = body.child(content);
        let _ = main_h;
        body.into_any_element()
    }

    pub(super) fn eq_saved_desktop(
        &self,
        pal: &EqPalette,
        main_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, subtitle) = tab_meta(EqTab::Personal);
        let preview_index = self
            .eq
            .preview_saved
            .filter(|index| *index < self.eq.saved.len())
            .or_else(|| {
                (!self.eq.dirty)
                    .then(|| {
                        self.eq
                            .saved
                            .iter()
                            .position(|preset| preset.name == self.eq.preset_name)
                    })
                    .flatten()
            })
            .or_else(|| (!self.eq.saved.is_empty()).then_some(0));

        let head = div()
            .h(px(38.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
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
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child(subtitle),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .h(px(24.0))
                    .px(px(9.0))
                    .rounded(px(12.0))
                    .bg(pal.chip)
                    .flex()
                    .items_center()
                    .text_size(px(9.5))
                    .font_family(pal.mono.clone())
                    .text_color(pal.text_dim)
                    .child(SharedString::from(format!(
                        "{} 个配置",
                        self.eq.saved.len()
                    ))),
            );

        let mut list = div()
            .id("eq-saved-list")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll())
            .flex()
            .flex_col()
            .gap(px(6.0))
            .pr(px(4.0));

        if self.eq.saved.is_empty() {
            list = list.child(
                div()
                    .w_full()
                    .h(px(180.0))
                    .rounded(px(12.0))
                    .bg(pal.panel)
                    .border_1()
                    .border_color(pal.border)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .child(icons::icon(Icon::Person, 20.0, pal.text_faint))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(pal.text_dim)
                            .child("还没有个人配置"),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child("在编辑器里调好链路，然后点右上角「另存为」"),
                    ),
            );
        }

        for (index, preset) in self.eq.saved.iter().enumerate() {
            let weak_preview = weak.clone();
            let name = preset.name.clone();
            let subtitle = format!(
                "{} 个滤波器 · 输入 {}",
                preset.settings.bands.len(),
                fmt_db(preset.settings.input_gain_db)
            );
            let active = !self.eq.dirty && self.eq.preset_name == name;
            let previewed = preview_index == Some(index);
            list = list.child(eq_preset_row(
                pal,
                &name,
                subtitle,
                Some(Icon::Person),
                None,
                active,
                previewed,
                false,
                SharedString::from(format!("eq-saved-{index}")),
                move |_, _, cx| {
                    let _ = weak_preview.update(cx, |app, cx| {
                        app.eq.preview_saved(index);
                        cx.notify();
                    });
                },
            ));
        }

        let preview = match preview_index.and_then(|index| {
            self.eq
                .saved
                .get(index)
                .map(|preset| (index, preset.clone()))
        }) {
            Some((index, preset)) => {
                let active = !self.eq.dirty && self.eq.preset_name == preset.name;
                let weak_apply = weak.clone();
                let on_apply: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                    let _ = weak_apply.update(cx, |app, cx| {
                        app.apply_saved_eq_preset(index, cx);
                    });
                });
                let weak_remove = weak.clone();
                let on_remove: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                    let _ = weak_remove.update(cx, |app, cx| {
                        app.remove_saved_preset(index, cx);
                    });
                });
                eq_preview_panel(
                    pal,
                    &preset.settings,
                    &preset.name,
                    "个人配置 · 完整链路",
                    active,
                    on_apply,
                    Some(on_remove),
                )
            }
            None => eq_preview_empty(pal, "还没有可预览的个人配置"),
        };

        let _ = main_h;
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(px(14.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(head)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(12.0))
                    .child(list)
                    .child(
                        div()
                            .id("eq-preview-pane-saved")
                            .w(px(300.0))
                            .h_full()
                            .flex_none()
                            .overflow_y_scroll()
                            .when(self.smooth_scroll, |d| d.smooth_scroll())
                            .child(preview),
                    ),
            )
            .into_any_element()
    }
}
