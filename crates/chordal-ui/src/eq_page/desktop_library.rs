//! EQ 页面区块：desktop_library（由 eq_page.rs 拆分，行为不变）。

use super::*;
use super::controls::*;

impl ChordialApp {
    pub(super) fn eq_library_desktop(
        &self,
        pal: &EqPalette,
        entries: Vec<PresetEntry>,
        main_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, subtitle) = tab_meta(self.eq.tab);
        let filtered: Vec<PresetEntry> = entries;

        let active_name = self.eq.preset_name.clone();
        let preview_entry = self
            .eq
            .preview
            .filter(|entry| {
                filtered
                    .iter()
                    .any(|candidate| candidate.name == entry.name)
            })
            .or_else(|| {
                filtered
                    .iter()
                    .find(|entry| entry.name == active_name && !self.eq.dirty)
                    .copied()
            })
            .or_else(|| filtered.first().copied());

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
            .child(div().flex_1());
        head = head.child(
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
                .child(SharedString::from(format!("{} 个配置", filtered.len()))),
        );

        // 预设列表按需渲染可视行：内置/V4 条目在 EQ 打开、滚动时
        // 不再全部进入布局与预绘制。
        let list: AnyElement = if filtered.is_empty() {
            div()
                .id("eq-library-list")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .when(self.smooth_scroll, |d| d.smooth_scroll())
                .pr(px(4.0))
                .child(
                    div()
                        .w_full()
                        .h(px(140.0))
                        .rounded(px(12.0))
                        .bg(pal.panel)
                        .border_1()
                        .border_color(pal.border)
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(5.0))
                        .child(icons::icon(Icon::Headphones, 20.0, pal.text_faint))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(pal.text_dim)
                                .child("没有匹配的配置"),
                        ),
                )
                .into_any_element()
        } else {
            let count = filtered.len();
            let entries = filtered;
            let pal_list = pal.clone();
            let weak_list = weak.clone();
            let dirty = self.eq.dirty;
            let active_name = self.eq.preset_name.clone();
            let preview_name = preview_entry.map(|entry| entry.name);
            let icon = Icon::Equalizer;
            let mut list =
                gpui::uniform_list("eq-library-list", count, move |range, _window, _cx| {
                    range
                        .map(|index| {
                            if eq_probe("rows") {
                                return div().h(px(60.0)).into_any_element();
                            }
                            let entry = entries[index];
                            let weak_preview = weak_list.clone();
                            let previewed = preview_name == Some(entry.name);
                            let active = !dirty && active_name == entry.name;
                            div()
                                .h(px(60.0))
                                .pb(px(6.0))
                                .child(eq_preset_row(
                                    &pal_list,
                                    entry.name,
                                    entry.subtitle(),
                                    Some(icon),
                                    None,
                                    active,
                                    previewed,
                                    false,
                                    SharedString::from(format!("eq-preset-{index}")),
                                    move |_, _, cx| {
                                        let _ = weak_preview.update(cx, |app, cx| {
                                            app.eq.preview_preset(entry);
                                            cx.notify();
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

        let preview = if eq_probe("preview") {
            div().w(px(292.0)).h_full().flex_none().into_any_element()
        } else {
            match preview_entry {
                Some(entry) => {
                    let settings = entry.to_settings();
                    let active = !self.eq.dirty && self.eq.preset_name == entry.name;
                    let weak_apply = weak.clone();
                    let on_apply: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                        let _ = weak_apply.update(cx, |app, cx| {
                            app.load_eq_preset(entry, cx);
                            cx.notify();
                        });
                    });
                    eq_preview_panel(
                        pal,
                        &settings,
                        entry.name,
                        &entry.subtitle(),
                        active,
                        on_apply,
                        None,
                    )
                }
                None => eq_preview_empty(pal, "选择一个配置查看预览"),
            }
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
            .child(head);
        body = body.child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(px(12.0))
                .child(list)
                .child(
                    div()
                        .id("eq-preview-pane-presets")
                        .w(px(292.0))
                        .h_full()
                        .flex_none()
                        .overflow_y_scroll()
                        .when(self.smooth_scroll, |d| d.smooth_scroll())
                        .child(preview),
                ),
        );
        let _ = main_h;
        body.into_any_element()
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
        let active_name = self.eq.preset_name.clone();
        let dirty = self.eq.dirty;

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
                (!dirty)
                    .then(|| {
                        filtered
                            .iter()
                            .copied()
                            .find(|&index| entries[index].name == active_name)
                    })
                    .flatten()
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

        let mut chips = div().h(px(28.0)).flex().items_center().gap(px(6.0));
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
                    "正在从 AutoEq 仓库拉取…",
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
            let rows_active = active_name.clone();
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
                            let active = !dirty && rows_active == entry.name;
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
                    let active = !dirty && active_name == entry.name;
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
                            "从 AutoEq 仓库下载这个型号的 ParametricEQ.txt。",
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
                        .w(px(292.0))
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

    pub(super) fn eq_saved_desktop(&self, pal: &EqPalette, main_h: f32, cx: &mut Context<Self>) -> AnyElement {
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
                            .child("在编辑器里调好曲线，然后点右上角「另存为」"),
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
                let apply_preset = preset.clone();
                let on_apply: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                    let _ = weak_apply.update(cx, |app, cx| {
                        app.eq.load_saved(&apply_preset);
                        app.eq.preview_saved(index);
                        app.apply_eq(cx);
                        cx.notify();
                    });
                });
                let weak_remove = weak.clone();
                let on_remove: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                    let _ = weak_remove.update(cx, |app, cx| {
                        app.remove_saved_preset(index, cx);
                        app.eq.preview_saved = None;
                    });
                });
                eq_preview_panel(
                    pal,
                    &preset.settings,
                    &preset.name,
                    "个人配置",
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
                            .w(px(292.0))
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
