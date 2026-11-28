//! `ChordialApp` 的 render_sidebar 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn sidebar_width(&self) -> f32 {
        if self.viewport_width < 1180.0 {
            208.0
        } else {
            crate::theme::size::SIDEBAR
        }
    }

    pub(super) fn list_width(&self) -> f32 {
        if self.viewport_width < 1180.0 {
            320.0
        } else {
            crate::theme::size::LIST
        }
    }

    pub(super) fn dock_width(&self) -> f32 {
        // 与沉浸层展开/收起动画共用同一条宽度公式。
        crate::layout_morph::dock_width(self.viewport_width)
    }

    // ================= macOS 标题栏 =================

    pub(super) fn render_titlebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let dark = theme.is_dark();
        // 中等宽度下状态消息最容易把标题栏挤爆；消息仍会通过列表/提示出现，
        // 标题栏只保留品牌与窗口操作。
        let show_messages = self.viewport_width >= 1180.0;

        let theme_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(
                    if dark {
                        crate::icons::Icon::Sun
                    } else {
                        crate::icons::Icon::Moon
                    },
                    14.0,
                    theme.text_dim,
                ),
                26.0,
                if dark { "切换浅色" } else { "切换深色" },
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_appearance(cx));
                },
            )
        };

        let settings_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Settings, 14.0, theme.text_dim),
                26.0,
                "设置",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.open_settings(cx));
                },
            )
        };

        let center = Self::window_drag_area(
            div()
                .id("titlebar-drag")
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::OpenHand),
        );
        // Windows 的双击最大化由 DefWindowProc（HTCAPTION）原生处理，
        // 此处只给其余平台补上。
        #[cfg(not(target_os = "windows"))]
        let center = center.on_click(|event, window, _| {
            if event.click_count() == 2 {
                window.zoom_window();
            }
        });
        let center = center.child(
            div()
                .flex()
                .items_center()
                .gap(px(7.0))
                .child(crate::icons::icon(
                    crate::icons::Icon::Note,
                    12.0,
                    theme.accent,
                ))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_dim)
                        .child("Chordial"),
                ),
        );

        div()
            .h(px(crate::theme::size::TITLEBAR))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(14.0))
            .bg(theme.sidebar)
            .border_b_1()
            .border_color(theme.border)
            .child(Self::traffic_lights())
            .child(center)
            .when(show_messages, |d| {
                d.children(self.status_message.as_ref().map(|msg| {
                    div()
                        .max_w(px(360.0))
                        .text_size(px(10.5))
                        .text_color(theme.danger)
                        .text_ellipsis()
                        .child(SharedString::from(msg.clone()))
                        .into_any_element()
                }))
                .children(self.info_message.as_ref().map(|msg| {
                    div()
                        .max_w(px(360.0))
                        .text_size(px(10.5))
                        .text_color(theme.text_faint)
                        .text_ellipsis()
                        .child(SharedString::from(msg.clone()))
                        .into_any_element()
                }))
            })
            .child(settings_btn)
            .child(theme_btn)
            .into_any_element()
    }

    // ================= 左侧边栏 =================
    pub(super) fn render_sidebar(
        &self,
        _mouse: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let width = self.sidebar_width();
        let dock_left = ((self.viewport_width - self.dock_width()) / 2.0).max(8.0);
        let bottom_pad: f32 = if dock_left < width + 12.0 {
            crate::theme::size::DOCK + crate::theme::size::DOCK_BOTTOM + 8.0
        } else {
            10.0
        };

        let active_source = match &self.page {
            Some(MainPage::Source(id)) | Some(MainPage::EditSource(id)) => Some(id.clone()),
            _ => None,
        };
        let active_playlist = match &self.page {
            Some(MainPage::Playlist(id)) => Some(id.clone()),
            _ => None,
        };
        let manage_active = matches!(
            self.page,
            Some(MainPage::ManageSources)
                | Some(MainPage::NewSource)
                | Some(MainPage::EditSource(_))
        );

        let section_label = |label: &'static str, meta: Option<String>| {
            div()
                .h(px(27.0))
                .px(px(9.0))
                .pb(px(6.0))
                .flex()
                .items_end()
                .justify_between()
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_faint)
                        .child(label),
                )
                .children(meta.map(|meta| {
                    div()
                        .text_size(px(9.5))
                        .text_color(theme.text_faint)
                        .child(SharedString::from(meta))
                }))
                .into_any_element()
        };

        let switch = |on: bool, id: SourceId, weak: gpui::WeakEntity<ChordialApp>| {
            let switch_bg = if on { theme.accent } else { theme.active };
            let knob_bg = if on { theme.on_accent } else { theme.surface };
            div()
                .id(SharedString::from(format!("source-switch-{}", id.0)))
                .w(px(30.0))
                .h(px(17.0))
                .flex_none()
                .rounded(px(9.0))
                .border_1()
                .border_color(if on {
                    theme.accent
                } else {
                    theme.border_strong
                })
                .bg(switch_bg)
                .relative()
                .cursor_pointer()
                .hover(|s| s.border_color(theme.text_faint))
                .child(
                    div()
                        .absolute()
                        .top(px(1.0))
                        .left(px(if on { 13.0 } else { 1.0 }))
                        .size(px(13.0))
                        .rounded(px(7.0))
                        .bg(knob_bg),
                )
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let id = id.clone();
                    let _ = weak.update(cx, |app, cx| {
                        let enabled = app.source_is_enabled(&id);
                        app.set_source_enabled(&id, !enabled, cx);
                    });
                })
        };

        let mut list = div().flex().flex_col().gap(px(1.0));

        // ---------- 音乐 ----------
        list = list
            .child(section_label("音乐", None))
            .child({
                let active = self.page.is_none()
                    && self.view_tab == ViewTab::Library
                    && self.library_filter == LibraryFilter::All;
                let weak = weak.clone();
                div()
                    .id("nav-all")
                    .h(px(32.0))
                    .px(px(9.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .bg(if active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|s| s.bg(theme.hover))
                    .cursor_pointer()
                    .child(crate::icons::icon(
                        crate::icons::Icon::Note,
                        14.0,
                        theme.text,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.5))
                            .font_weight(if active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child("所有音乐"),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .child(SharedString::from(self.library.len().to_string())),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.page = None;
                            app.view_tab = ViewTab::Library;
                            app.library_filter = LibraryFilter::All;
                            app.source_route = None;
                            cx.notify();
                        });
                    })
            })
            .child({
                let active = self.page.is_none()
                    && self.view_tab == ViewTab::Library
                    && self.library_filter == LibraryFilter::Queue;
                let weak = weak.clone();
                div()
                    .id("nav-queue")
                    .h(px(32.0))
                    .px(px(9.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .bg(if active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|s| s.bg(theme.hover))
                    .cursor_pointer()
                    .child(crate::icons::icon(
                        crate::icons::Icon::Queue,
                        14.0,
                        theme.text_dim,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.5))
                            .font_weight(if active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child("播放队列"),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .child(SharedString::from(self.queue.len().to_string())),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.page = None;
                            app.view_tab = ViewTab::Library;
                            app.library_filter = LibraryFilter::Queue;
                            app.source_route = None;
                            cx.notify();
                        });
                    })
            });

        // ---------- 音源 ----------
        let source_slots = self.source_slots();
        let enabled_sources = self.source_configs.iter().filter(|s| s.enabled).count();
        list = list.child(div().h(px(10.0))).child(section_label(
            "音源",
            Some(format!(
                "{enabled_sources} / {} 开启",
                self.source_configs.len()
            )),
        ));

        if self.source_configs.is_empty() {
            list = list.child(
                div()
                    .px(px(9.0))
                    .py(px(5.0))
                    .text_size(px(10.0))
                    .text_color(theme.text_faint)
                    .child("还没有音源，点下方「管理音源」添加"),
            );
        }

        for config in &self.source_configs {
            let id = SourceId::new(config.id.clone());
            let enabled = config.enabled;
            let slot = source_slots.get(&id);
            let loaded = slot.is_some_and(|slot| slot.status == SourceStatus::Loaded);
            let failed = slot.is_some_and(|slot| matches!(slot.status, SourceStatus::Failed(_)));
            let active = active_source.as_ref() == Some(&id);
            let is_local = config.is_local();
            let icon = if is_local {
                crate::icons::Icon::Drive
            } else {
                crate::icons::Icon::Cloud
            };
            let dot = if !enabled {
                theme.text_faint
            } else if failed {
                theme.danger
            } else if loaded {
                theme.ok
            } else {
                theme.warn
            };

            let status_text = if !enabled {
                "已关闭 · 保留设置".to_string()
            } else if failed {
                "加载失败 · 打开查看".to_string()
            } else if is_local {
                let tracks = cx
                    .global::<Globals>()
                    .local_sources
                    .get(&id)
                    .map(|local| local.stats().tracks)
                    .unwrap_or(0);
                if loaded {
                    format!("已就绪 · {tracks} 首")
                } else {
                    "未加载".to_string()
                }
            } else if loaded {
                let account = self
                    .registry
                    .get(&id)
                    .and_then(|source| source.account().map(|account| account.account_state()));
                match account {
                    Some(AccountState::LoggedIn(profile)) => {
                        format!("已连接 · {}", profile.nickname)
                    }
                    Some(AccountState::LoggedOut) => "已连接 · 未登录".to_string(),
                    Some(AccountState::Unknown) => "已连接 · 校验中".to_string(),
                    _ => "已连接".to_string(),
                }
            } else {
                "未加载".to_string()
            };

            let weak_row = weak.clone();
            let weak_switch = weak.clone();
            let row_switch = switch(enabled, id.clone(), weak_switch);
            list = list.child(
                div()
                    .id(SharedString::from(format!("source-{}", config.id)))
                    .h(px(45.0))
                    .px(px(9.0))
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .bg(if active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|s| s.bg(theme.hover))
                    .cursor_pointer()
                    .on_click({
                        let id = id.clone();
                        move |_, _, cx| {
                            let _ = weak_row.update(cx, |app, cx| {
                                app.open_source_page(id.clone(), cx);
                            });
                        }
                    })
                    .child(
                        div()
                            .size(px(28.0))
                            .flex_none()
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.surface_alt)
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(if enabled { 1.0 } else { 0.55 })
                            .child(crate::icons::icon(
                                icon,
                                14.0,
                                if active { theme.text } else { theme.text_dim },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .font_weight(if active {
                                        gpui::FontWeight::MEDIUM
                                    } else {
                                        gpui::FontWeight::NORMAL
                                    })
                                    .text_color(if enabled {
                                        theme.text
                                    } else {
                                        theme.text_faint
                                    })
                                    .text_ellipsis()
                                    .child(SharedString::from(config.name.clone())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.0))
                                    .child(div().size(px(5.0)).rounded(px(3.0)).bg(dot))
                                    .child(
                                        div()
                                            .text_size(px(9.5))
                                            .text_color(theme.text_faint)
                                            .text_ellipsis()
                                            .child(SharedString::from(status_text)),
                                    ),
                            ),
                    )
                    .child(row_switch),
            );
        }

        // 管理音源：端点状态 → 二级编辑。
        {
            let weak = weak.clone();
            list = list.child(
                div()
                    .id("manage-sources")
                    .h(px(32.0))
                    .px(px(9.0))
                    .mt(px(2.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .bg(if manage_active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|s| s.bg(theme.hover))
                    .cursor_pointer()
                    .child(crate::icons::icon(
                        crate::icons::Icon::Sliders,
                        14.0,
                        theme.text_dim,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.0))
                            .text_color(theme.text_dim)
                            .child("管理音源"),
                    )
                    .child(crate::icons::icon(
                        crate::icons::Icon::ChevronRight,
                        11.0,
                        theme.text_faint,
                    ))
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.open_source_manager(cx));
                    }),
            );
        }

        // ---------- 我的歌单（本地 + 订阅） ----------
        {
            let weak_plus = weak.clone();
            list = list.child(div().h(px(12.0))).child(
                div()
                    .h(px(27.0))
                    .px(px(9.0))
                    .pb(px(6.0))
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text_faint)
                            .child("我的歌单"),
                    )
                    .child(
                        div()
                            .id("playlist-create")
                            .size(px(19.0))
                            .rounded(px(5.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.hover))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Plus,
                                12.0,
                                theme.text_faint,
                            ))
                            .on_click(move |_, _, cx| {
                                let _ =
                                    weak_plus.update(cx, |app, cx| app.create_local_playlist(cx));
                            }),
                    ),
            );
        }

        let summaries = self.playlist_summaries();
        if summaries.is_empty() {
            list = list.child(
                div()
                    .px(px(9.0))
                    .py(px(5.0))
                    .text_size(px(10.0))
                    .line_height(px(15.0))
                    .text_color(theme.text_faint)
                    .child("还没有本地歌单；也可以在歌单页订阅来源歌单"),
            );
        }
        for summary in summaries.iter() {
            let active = active_playlist.as_ref() == Some(&summary.id);
            let subscribed = summary.is_subscribed();
            let weak = weak.clone();
            let id = summary.id.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("playlist-{}", summary.id.0)))
                    .h(px(30.0))
                    .px(px(9.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .bg(if active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|s| s.bg(theme.hover))
                    .cursor_pointer()
                    .child(crate::icons::icon(
                        if subscribed {
                            crate::icons::Icon::Cloud
                        } else {
                            crate::icons::Icon::Layers
                        },
                        13.0,
                        if active { theme.text } else { theme.text_faint },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.0))
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(SharedString::from(summary.title.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(theme.text_faint)
                            .child(SharedString::from(summary.count.to_string())),
                    )
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        let _ = weak.update(cx, |app, cx| app.open_playlist_page(id.clone(), cx));
                    }),
            );
        }
        {
            let weak = weak.clone();
            list = list.child(
                div()
                    .id("playlist-create-row")
                    .h(px(30.0))
                    .px(px(9.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Plus,
                        13.0,
                        theme.text_faint,
                    ))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.text_faint)
                            .child("新建歌单"),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.create_local_playlist(cx));
                    }),
            );
        }

        div()
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .track_scroll(&self.sidebar_scroll)
                    .overflow_y_scroll()
                    .when(self.smooth_scroll, |d| d.smooth_scroll())
                    .p(px(10.0))
                    .pb(px(bottom_pad.max(10.0)))
                    .child(list),
            )
            .into_any_element()
    }
}
