//! EQ 页面区块：compact_panels（由 eq_page.rs 拆分，行为不变）。

use super::controls::*;
use super::*;

impl ChordialApp {
    pub(super) fn eq_inspector_compact(
        &self,
        pal: &EqPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let selected = self.eq.selected.and_then(|index| {
            self.eq
                .settings
                .bands
                .get(index)
                .map(|band| (index, band.clone()))
        });
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

        match selected {
            Some((index, band)) => {
                panel = panel.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(1.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(5.0))
                                        .child(
                                            div()
                                                .text_size(px(9.0))
                                                .text_color(pal.text_faint)
                                                .child(SharedString::from(format!(
                                                    "频段 {:02} / {:02}",
                                                    index + 1,
                                                    self.eq.settings.bands.len()
                                                ))),
                                        )
                                        .child({
                                            let weak_toggle = weak.clone();
                                            let enabled = band.enabled;
                                            div()
                                                .id("eq-band-enabled-compact")
                                                .h(px(24.0))
                                                .px(px(8.0))
                                                .rounded(px(12.0))
                                                .bg(if enabled {
                                                    pal.accent_soft
                                                } else {
                                                    pal.chip
                                                })
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(9.0))
                                                .text_color(if enabled {
                                                    pal.accent
                                                } else {
                                                    pal.text_faint
                                                })
                                                .child(if enabled { "启用" } else { "旁路" })
                                                .on_click(move |_, _, cx| {
                                                    let _ = weak_toggle.update(cx, |app, cx| {
                                                        if let Some(band) =
                                                            app.eq.settings.bands.get_mut(index)
                                                        {
                                                            band.enabled = !enabled;
                                                            app.eq.touch();
                                                            app.apply_eq(cx);
                                                        }
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .gap(px(4.0))
                                        .child(
                                            div()
                                                .text_size(px(22.0))
                                                .font_family(pal.mono.clone())
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(pal.text)
                                                .child(SharedString::from(fmt_freq(band.freq_hz))),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(9.0))
                                                .text_color(pal.text_faint)
                                                .child("Hz"),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(13.0))
                                                .font_family(pal.mono.clone())
                                                .text_color(pal.accent)
                                                .child(SharedString::from(format!(
                                                    "  {} dB",
                                                    if band.kind.uses_gain() {
                                                        db_num(band.gain_db)
                                                    } else {
                                                        "—".into()
                                                    }
                                                ))),
                                        ),
                                ),
                        )
                        .child({
                            let weak = weak.clone();
                            eq_icon_button(
                                pal,
                                Icon::Close,
                                32.0,
                                "eq-remove-compact",
                                "删除频段",
                                move |_, _, cx| {
                                    let _ = weak.update(cx, |app, cx| {
                                        app.eq.selected = Some(index);
                                        app.remove_eq_band(cx);
                                    });
                                },
                            )
                        }),
                );

                panel = panel.child(self.eq_kind_row_compact(pal, index, band.kind, cx));
                panel = panel.child(self.eq_params_block(pal, index, &band, true, cx));
                panel = panel.child(eq_divider(pal));
            }
            None => {
                panel = panel.child(
                    div()
                        .w_full()
                        .py(px(14.0))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(pal.text_dim)
                                .child("还没有频段"),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(pal.text_faint)
                                .child("点按上方曲线即可添加第一个频段"),
                        ),
                );
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
                    .child("调整会自动保存；需要复用曲线时点「另存为」"),
            );
        }
        panel.into_any_element()
    }

    pub(super) fn eq_kind_row_compact(
        &self,
        pal: &EqPalette,
        _index: usize,
        active: FilterKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut row = div().w_full().flex().gap(px(5.0));
        for kind in FilterKind::ALL {
            let weak = weak.clone();
            let is_active = active == kind;
            row = row.child(
                div()
                    .id(SharedString::from(format!("eq-kind-c-{}", kind.label())))
                    .h(px(32.0))
                    .flex_1()
                    .rounded(px(8.0))
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
                    .text_size(px(10.5))
                    .text_color(if is_active { pal.accent } else { pal.text_dim })
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.set_eq_filter_kind(kind, cx));
                    })
                    .child(kind.label()),
            );
        }
        row.into_any_element()
    }

    // ---------- 紧凑：预设库 ----------

    pub(super) fn eq_library_compact(
        &self,
        pal: &EqPalette,
        entries: Vec<PresetEntry>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, subtitle) = tab_meta(self.eq.tab);
        let filtered: Vec<PresetEntry> = entries;

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
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(pal.text_faint)
                                .child(subtitle),
                        ),
                )
                .child(
                    div()
                        .text_size(px(9.5))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .child(SharedString::from(format!("{} 项", filtered.len()))),
                ),
        );

        if filtered.is_empty() {
            body = body.child(
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
            );
        }

        for (index, entry) in filtered.iter().enumerate() {
            let weak = weak.clone();
            let entry = *entry;
            let active = !self.eq.dirty && self.eq.preset_name == entry.name;
            body = body.child(eq_preset_row(
                pal,
                entry.name,
                entry.subtitle(),
                Some(Icon::Equalizer),
                None,
                active,
                false,
                false,
                SharedString::from(format!("eq-preset-c-{index}")),
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.load_eq_preset(entry, cx);
                        app.show_portrait_toast(format!("已应用「{}」", entry.name), cx);
                        cx.notify();
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
        let active_name = self.eq.preset_name.clone();
        let dirty = self.eq.dirty;
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
            let active = !dirty && active_name == entry.name;
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
                                .child("「另存为」得到的个人预设"),
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
                            .child("在编辑器里调好曲线后点「另存为个人预设」"),
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
                            let preset = app.eq.saved.get(index).cloned();
                            if let Some(preset) = preset {
                                app.eq.load_saved(&preset);
                                app.eq.preview_saved(index);
                                app.apply_eq(cx);
                                app.show_portrait_toast(format!("已应用「{}」", preset.name), cx);
                                cx.notify();
                            }
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
                            .child(icons::icon(Icon::Close, 13.0, pal.danger))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = weak_remove.update(cx, |app, cx| {
                                    app.remove_saved_preset(index, cx);
                                    app.eq.preview_saved = None;
                                });
                            }),
                    ),
            );
        }
        body.into_any_element()
    }

    pub(super) fn eq_nav_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let tabs = [
            EqTab::Edit,
            EqTab::V4,
            EqTab::AutoEq,
            EqTab::Builtin,
            EqTab::Personal,
        ];
        let mut row = div()
            .h(px(60.0))
            .w_full()
            .flex_none()
            .border_t_1()
            .border_color(pal.divider)
            .bg(pal.panel)
            .flex()
            .items_center();
        for tab in tabs {
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
