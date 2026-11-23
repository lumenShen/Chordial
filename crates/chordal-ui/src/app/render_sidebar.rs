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
        (self.viewport_width - 32.0).min(840.0).max(520.0)
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

    pub(super) fn playlist_rows(&self) -> Arc<Vec<PlaylistRowText>> {
        let mut cache = self.playlist_rows.borrow_mut();
        match cache.as_ref() {
            Some((cached, rows)) if Arc::ptr_eq(cached, &self.user_playlists) => rows.clone(),
            _ => {
                let rows = Arc::new(
                    self.user_playlists
                        .iter()
                        .map(PlaylistRowText::new)
                        .collect::<Vec<_>>(),
                );
                *cache = Some((self.user_playlists.clone(), rows.clone()));
                rows
            }
        }
    }

    pub(super) fn source_snapshot(&self) -> Arc<Vec<SourceSlotSnapshot>> {
        let version = self.registry.version();
        let mut cache = self.source_snapshot.borrow_mut();
        match cache.as_ref() {
            Some((cached, slots)) if *cached == version => slots.clone(),
            _ => {
                let slots = Arc::new(self.registry.snapshot());
                *cache = Some((version, slots.clone()));
                slots
            }
        }
    }

    /// 侧栏歌单保留缓存的输入版本。任何可能改变歌单行内容、位置或 hover
    /// 状态的输入都必须进入哈希，否则缓存的绘制范围会过期。
    pub(super) fn sidebar_playlist_version(&self, mouse: gpui::Point<gpui::Pixels>) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (matches!(self.appearance, crate::theme::Appearance::Dark) as u8).hash(&mut hasher);
        self.user_playlists.as_ptr().hash(&mut hasher);
        self.playlists_loading.hash(&mut hasher);
        self.account_avatar
            .as_ref()
            .map(|image| Arc::as_ptr(image) as usize)
            .hash(&mut hasher);
        match &self.account_state {
            chordal_core::model::AccountState::Unsupported => 0u8.hash(&mut hasher),
            chordal_core::model::AccountState::LoggedOut => 1u8.hash(&mut hasher),
            chordal_core::model::AccountState::Unknown => 3u8.hash(&mut hasher),
            chordal_core::model::AccountState::LoggedIn(profile) => {
                2u8.hash(&mut hasher);
                profile.uid.hash(&mut hasher);
                profile.nickname.hash(&mut hasher);
                profile.avatar.hash(&mut hasher);
                profile.vip.hash(&mut hasher);
            }
        }
        if let Some(collection) = &self.collection {
            collection.id.hash(&mut hasher);
        }
        self.viewport_width.to_bits().hash(&mut hasher);
        self.viewport_height.to_bits().hash(&mut hasher);
        self.sidebar_width().to_bits().hash(&mut hasher);
        f32::from(self.sidebar_scroll.offset().y)
            .to_bits()
            .hash(&mut hasher);
        f32::from(mouse.x).to_bits().hash(&mut hasher);
        f32::from(mouse.y).to_bits().hash(&mut hasher);
        hasher.finish()
    }

    pub(super) fn render_sidebar(
        &self,
        mouse: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let slots = self.source_snapshot();
        let loaded = slots
            .iter()
            .filter(|s| s.status == SourceStatus::Loaded)
            .count();
        let width = self.sidebar_width();
        let netease_loaded = slots
            .iter()
            .any(|s| s.info.id.as_str() == "netease" && s.status == SourceStatus::Loaded);
        let dock_left = ((self.viewport_width - self.dock_width()) / 2.0).max(8.0);
        let bottom_pad: f32 = if dock_left < width + 12.0 { 88.0 } else { 10.0 };

        let section = |label: &'static str| {
            div()
                .h(px(28.0))
                .px(px(10.0))
                .pb(px(6.0))
                .flex()
                .items_end()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_faint)
                .child(label)
                .into_any_element()
        };

        let nav_row = |id: &'static str,
                       label: &'static str,
                       count: String,
                       active: bool,
                       icon: crate::icons::Icon,
                       weak: gpui::WeakEntity<ChordialApp>,
                       target: LibraryFilter| {
            let fg = theme.text;
            div()
                .id(id)
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
                .child(crate::icons::icon(icon, 14.0, fg))
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
                        .text_color(fg)
                        .text_ellipsis()
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(if active {
                            theme.text_dim
                        } else {
                            theme.text_faint
                        })
                        .child(count),
                )
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.library_filter = target;
                        app.source_route = None;
                        app.view_tab = ViewTab::Library;
                        cx.notify();
                    });
                })
        };

        let mut list = div().flex().flex_col().gap(px(2.0));
        list = list
            .child(section("音乐"))
            .child(nav_row(
                "nav-all",
                "所有音乐",
                self.library.len().to_string(),
                self.library_filter == LibraryFilter::All && self.view_tab == ViewTab::Library,
                crate::icons::Icon::Note,
                weak.clone(),
                LibraryFilter::All,
            ))
            .child(nav_row(
                "nav-queue",
                "播放队列",
                self.queue.len().to_string(),
                self.library_filter == LibraryFilter::Queue && self.view_tab == ViewTab::Library,
                crate::icons::Icon::Queue,
                weak.clone(),
                LibraryFilter::Queue,
            ))
            .child(div().h(px(6.0)))
            .child(section("音乐源"));

        // 全部音源路由。
        {
            let active = self.source_route.is_none() && self.view_tab == ViewTab::Search;
            let weak = weak.clone();
            list = list.child(
                div()
                    .id("route-all")
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
                    .child(crate::icons::icon(
                        crate::icons::Icon::Search,
                        14.0,
                        if active { theme.text } else { theme.text_dim },
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
                            .child("全部音源"),
                    )
                    .on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.source_route = None;
                            app.view_tab = ViewTab::Search;
                            focus_input(&app.search_input, window, cx);
                            cx.notify();
                        });
                    }),
            );
        }

        // 搜索结果的按源计数：只在 Arc 快照变化时重算一次。
        // 旧实现在下面的音源循环里对每个源各扫一遍 search_tracks
        // （O(源数 × 结果数)，每帧重复），搜索结果多时是确定性瓶颈。
        let search_ptr = Arc::as_ptr(&self.search_tracks) as usize;
        {
            let mut cache = self.search_source_counts.borrow_mut();
            let stale = cache.as_ref().is_none_or(|(ptr, _)| *ptr != search_ptr);
            if stale {
                let mut counts = HashMap::new();
                for track in self.search_tracks.iter() {
                    *counts.entry(track.id.source.clone()).or_insert(0usize) += 1;
                }
                *cache = Some((search_ptr, counts));
            }
        }
        let source_counts = self.search_source_counts.borrow();
        let source_counts = source_counts.as_ref().expect("initialized above");

        for slot in slots.iter() {
            let source_id = slot.info.id.clone();
            let route_active =
                self.view_tab == ViewTab::Search && self.source_route.as_ref() == Some(&source_id);
            let loaded_now = slot.status == SourceStatus::Loaded;
            let local = source_id.0 == "local";
            let dot = match &slot.status {
                SourceStatus::Loaded => theme.ok,
                SourceStatus::Registered => theme.text_faint,
                SourceStatus::Failed(_) => theme.danger,
            };
            let count = if local {
                self.library.len()
            } else {
                *source_counts.1.get(&source_id).unwrap_or(&0)
            };
            let weak_route = weak.clone();
            let weak_toggle = weak.clone();
            let route_id = source_id.clone();
            let toggle_id = source_id.clone();
            let icon = if local {
                crate::icons::Icon::Drive
            } else {
                crate::icons::Icon::Cloud
            };

            list = list.child(
                div()
                    .id(SharedString::from(format!("src-{}", source_id)))
                    .h(px(32.0))
                    .px(px(9.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .bg(if route_active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|s| s.bg(theme.hover))
                    .on_click(move |_, window, cx| {
                        let id = route_id.clone();
                        let _ = weak_route.update(cx, |app, cx| {
                            app.source_route = Some(id);
                            app.view_tab = ViewTab::Search;
                            focus_input(&app.search_input, window, cx);
                            cx.notify();
                        });
                    })
                    .child(crate::icons::icon(
                        icon,
                        14.0,
                        if route_active {
                            theme.text
                        } else {
                            theme.text_dim
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.5))
                            .font_weight(if route_active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(SharedString::from(slot.info.display_name.clone())),
                    )
                    .when(count > 0, |d| {
                        d.child(
                            div()
                                .text_size(px(9.5))
                                .text_color(theme.text_faint)
                                .child(SharedString::from(count.to_string())),
                        )
                    })
                    .child(div().size(px(5.0)).rounded(px(3.0)).bg(dot))
                    .child(
                        div()
                            .id(SharedString::from(format!("src-toggle-{}", toggle_id)))
                            .h(px(18.0))
                            .px(px(5.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .text_size(px(9.5))
                            .text_color(theme.text_faint)
                            .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                            .child(if loaded_now { "卸载" } else { "加载" })
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = weak_toggle
                                    .update(cx, |app, cx| app.toggle_source(&toggle_id, cx));
                            }),
                    ),
            );
        }

        // 账号 / 我的歌单（只在网易云源已加载时出现）。
        if netease_loaded {
            list = list.child(div().h(px(6.0))).child(section("网易云账号"));
            match self.account_state.clone() {
                AccountState::LoggedIn(profile) => {
                    let weak_logout = weak.clone();
                    let avatar_el: AnyElement = match &self.account_avatar {
                        Some(image) => {
                            let placeholder = div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(theme.surface_alt)
                                .child(crate::icons::icon(
                                    crate::icons::Icon::Cloud,
                                    12.0,
                                    theme.text_faint,
                                ))
                                .into_any_element();
                            crate::widgets::cover_thumb(
                                &theme,
                                Some(image.clone()),
                                28.0,
                                14.0,
                                placeholder,
                            )
                        }
                        None => crate::icons::icon(crate::icons::Icon::Cloud, 14.0, theme.ok),
                    };
                    list = list.child(
                        div()
                            .id("account-row")
                            .h(px(34.0))
                            .px(px(9.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(avatar_el)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .text_ellipsis()
                                            .child(SharedString::from(profile.nickname.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.5))
                                            .text_color(theme.text_faint)
                                            .child(SharedString::from(if profile.vip {
                                                "VIP · 已登录"
                                            } else {
                                                "已登录"
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("account-logout")
                                    .h(px(20.0))
                                    .px(px(6.0))
                                    .rounded(px(4.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .flex()
                                    .items_center()
                                    .text_size(px(9.5))
                                    .text_color(theme.text_faint)
                                    .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                                    .child("退出")
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        let _ = weak_logout
                                            .update(cx, |app, cx| app.logout_account(cx));
                                    }),
                            ),
                    );
                    list = list.child(div().h(px(4.0))).child(section("我的歌单"));
                    if self.playlists_loading {
                        list = list.child(
                            div()
                                .px(px(9.0))
                                .py(px(4.0))
                                .text_size(px(10.0))
                                .text_color(theme.text_faint)
                                .child("正在加载歌单…"),
                        );
                    } else if self.user_playlists.is_empty() {
                        list = list.child(
                            div()
                                .px(px(9.0))
                                .py(px(4.0))
                                .text_size(px(10.0))
                                .text_color(theme.text_faint)
                                .child("暂无歌单"),
                        );
                    } else {
                        let playlist_rows = self.playlist_rows();
                        let row_count = playlist_rows.len().min(30);
                        let mut style = gpui::Style::default();
                        style.size.width = gpui::relative(1.0).into();
                        style.size.height = px((row_count as f32) * 28.0).into();
                        style.flex_shrink = 0.0;

                        let version = self.sidebar_playlist_version(mouse);
                        let rows = playlist_rows;
                        let theme_rows = theme.clone();
                        let weak_rows = weak.clone();
                        let active_id = self
                            .collection
                            .as_ref()
                            .map(|collection| collection.id.clone());
                        list = list.child(gpui::Retained::new(
                            "sidebar-playlists",
                            version,
                            style,
                            move |_window, _cx| {
                                let mut column = div().w_full().flex().flex_col();
                                for row in rows.iter().take(row_count) {
                                    let weak = weak_rows.clone();
                                    let active = active_id.as_ref() == Some(&row.id);
                                    column = column.child(
                                        div()
                                            .id(row.element_id.clone())
                                            .h(px(28.0))
                                            .px(px(9.0))
                                            .rounded(px(6.0))
                                            .flex()
                                            .items_center()
                                            .gap(px(7.0))
                                            .bg(if active {
                                                theme_rows.active
                                            } else {
                                                gpui::transparent_black()
                                            })
                                            .hover(|s| s.bg(theme_rows.hover))
                                            .cursor_pointer()
                                            .child(crate::icons::icon(
                                                crate::icons::Icon::Layers,
                                                12.0,
                                                if active {
                                                    theme_rows.text
                                                } else {
                                                    theme_rows.text_faint
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_size(px(11.5))
                                                    .text_color(theme_rows.text)
                                                    .text_ellipsis()
                                                    .child(row.title.clone()),
                                            )
                                            .when(!row.count.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .text_size(px(9.5))
                                                        .text_color(theme_rows.text_faint)
                                                        .child(row.count.clone()),
                                                )
                                            })
                                            .on_click({
                                                let id = row.id.clone();
                                                let title = row.title.to_string();
                                                move |_, _, cx| {
                                                    let _ = weak.update(cx, |app, cx| {
                                                        app.open_collection(
                                                            id.clone(),
                                                            title.clone(),
                                                            cx,
                                                        );
                                                    });
                                                }
                                            }),
                                    );
                                }
                                column.into_any_element()
                            },
                        ));
                    }
                }
                AccountState::Unknown => {
                    let weak = weak.clone();
                    list = list.child(
                        div()
                            .id("account-retry")
                            .px(px(9.0))
                            .py(px(4.0))
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.text))
                            .child("正在校验账号 · 点击重试")
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.spawn_account_bootstrap(cx);
                                    cx.notify();
                                });
                            }),
                    );
                }
                _ => {
                    let weak = weak.clone();
                    list = list.child(
                        div()
                            .id("account-login")
                            .h(px(32.0))
                            .px(px(9.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.hover))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Cloud,
                                14.0,
                                theme.text_dim,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.5))
                                    .text_color(theme.text)
                                    .child("登录网易云"),
                            )
                            .child(crate::icons::icon(
                                crate::icons::Icon::ChevronDown,
                                10.0,
                                theme.text_faint,
                            ))
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| app.open_login(cx));
                            }),
                    );
                }
            }
        }

        // 播放模式。
        list = list.child(div().h(px(6.0))).child(section("播放模式"));
        for (mode, label, icon) in [
            (PlayMode::Sequential, "顺序播放", crate::icons::Icon::Menu),
            (PlayMode::RepeatAll, "列表循环", crate::icons::Icon::Repeat),
            (PlayMode::Shuffle, "随机播放", crate::icons::Icon::Shuffle),
            (PlayMode::RepeatOne, "单曲循环", crate::icons::Icon::Repeat),
        ] {
            let active = self.play_mode == mode;
            let weak = weak.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("mode-{label}")))
                    .h(px(32.0))
                    .px(px(9.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .hover(|s| s.bg(theme.hover))
                    .child(crate::icons::icon(
                        icon,
                        14.0,
                        if active {
                            theme.text_dim
                        } else {
                            theme.text_faint
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .font_weight(if active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(if active { theme.text } else { theme.text_dim })
                            .child(label),
                    )
                    .when(active, |d| {
                        d.child(crate::icons::icon(
                            crate::icons::Icon::Check,
                            11.0,
                            theme.text_dim,
                        ))
                    })
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.play_mode = mode;
                            cx.notify();
                        });
                    }),
            );
        }

        let footer_note = if self.library_scanning {
            "正在扫描本地库…".to_string()
        } else {
            format!("{loaded} 个音源已加载 · Ctrl K 搜索")
        };

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
                    .child(list),
            )
            .child(
                div()
                    .mx(px(10.0))
                    .mb(px(bottom_pad.max(10.0)))
                    .p(px(10.0))
                    .rounded(px(9.0))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface_alt)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.0))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Drive,
                                13.0,
                                theme.text_dim,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(11.5))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child("本地库"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.text_faint)
                                    .child(SharedString::from(format!(
                                        "{} 首",
                                        self.library.len()
                                    ))),
                            )
                            .child({
                                let weak = weak.clone();
                                crate::widgets::icon_button(
                                    &theme,
                                    crate::icons::icon(
                                        crate::icons::Icon::Settings,
                                        13.0,
                                        theme.text_dim,
                                    ),
                                    22.0,
                                    "设置",
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |app, cx| app.open_settings(cx));
                                    },
                                )
                            }),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(SharedString::from(footer_note)),
                    ),
            )
            .into_any_element()
    }
}
