//! `ChordialApp` 的 render_collection 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    /// 集合页头部：封面 + 创建者/简介 + 歌手页专辑快捷入口。
    pub(super) fn render_collection_header(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(view) = &self.collection else {
            return div().into_any_element();
        };
        let weak = cx.entity().downgrade();
        let size = 64.0;
        let placeholder = div()
            .size_full()
            .bg(theme.surface_alt)
            .flex()
            .items_center()
            .justify_center()
            .child(crate::icons::icon(
                crate::icons::Icon::Layers,
                20.0,
                theme.text_faint,
            ))
            .into_any_element();
        let cover = crate::widgets::cover_thumb(
            theme,
            self.collection_cover.clone(),
            size,
            1.0,
            placeholder,
        );
        let mut info = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(3.0))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_dim)
                    .text_ellipsis()
                    .child(SharedString::from(
                        view.creator
                            .clone()
                            .filter(|c| !c.is_empty())
                            .unwrap_or_else(|| view.id.kind.label().to_string()),
                    )),
            )
            .when_some(
                view.description
                    .clone()
                    .filter(|d| !d.trim().is_empty())
                    .map(|d| d.lines().next().unwrap_or("").to_string()),
                |d, line| {
                    d.child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(SharedString::from(line)),
                    )
                },
            )
            .child(
                div().flex().items_center().gap(px(5.0)).mt(px(3.0)).child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme.text_faint)
                        .child(SharedString::from(format!(
                            "{} 首 · {}",
                            view.tracks.len(),
                            view.id.kind.label()
                        ))),
                ),
            );

        if !view.related_albums.is_empty() {
            let mut chips = div().flex().flex_wrap().gap(px(4.0)).mt(px(4.0));
            for album in view.related_albums.iter().take(8) {
                let weak = weak.clone();
                let id = album.id.clone();
                let title = album.title.clone();
                chips = chips.child(
                    div()
                        .id(SharedString::from(format!("coll-album-{}", album.id.id)))
                        .h(px(20.0))
                        .px(px(6.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface_alt)
                        .flex()
                        .items_center()
                        .text_size(px(9.5))
                        .text_color(theme.text_dim)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.hover).text_color(theme.text))
                        .child(SharedString::from(album.title.clone()))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.open_collection(id.clone(), title.clone(), cx);
                            });
                        }),
                );
            }
            info = info.child(chips);
        }

        // 订阅入口只对“远程歌单”出现：本地歌单没有来源可订阅。
        let subscribe: Option<AnyElement> = if view.id.kind == CollectionKind::Playlist
            && view.error.is_none()
            && !view.loading
            && !view.tracks.is_empty()
            && self
                .source_config_by_id(&view.id.source.0)
                .is_some_and(|source| source.is_netease())
        {
            let subscribed = self.playlists.is_subscribed(&view.id);
            let weak = weak.clone();
            let trigger = div()
                .id("collection-subscribe-trigger")
                .h(px(24.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface_alt)
                .flex()
                .items_center()
                .gap(px(4.0))
                .text_size(px(9.5))
                .text_color(if subscribed { theme.ok } else { theme.text_dim })
                .hover(|s| s.bg(theme.hover).text_color(theme.text))
                .child(crate::icons::icon(
                    if subscribed {
                        crate::icons::Icon::Check
                    } else {
                        crate::icons::Icon::Plus
                    },
                    10.0,
                    if subscribed {
                        theme.ok
                    } else {
                        theme.text_faint
                    },
                ))
                .child(if subscribed { "已订阅" } else { "订阅" })
                .into_any_element();
            Some(crate::bubble::bubble_button(
                crate::bubble::BubbleStyle::for_theme(theme),
                "collection-subscribe",
                if subscribed {
                    "已在「我的歌单」中"
                } else {
                    "加入到我的歌单里"
                },
                trigger,
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        if subscribed {
                            app.info_message = Some("已经在「我的歌单」里".into());
                            cx.notify();
                        } else {
                            app.subscribe_current_collection(cx);
                        }
                    });
                },
            ))
        } else {
            None
        };

        div()
            .px(px(14.0))
            .pb(px(10.0))
            .flex()
            .items_start()
            .gap(px(10.0))
            .child(cover)
            .child(info)
            .children(subscribe)
            .into_any_element()
    }

    pub(super) fn render_search_tabs(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(12.0))
            .pb(px(6.0));
        for (kind, label) in [
            (SearchKind::Track, "歌曲"),
            (SearchKind::Album, "专辑"),
            (SearchKind::Artist, "歌手"),
            (SearchKind::Playlist, "歌单"),
        ] {
            let active = self.search_kind == kind;
            let weak = weak.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("search-kind-{label}")))
                    .h(px(24.0))
                    .px(px(9.0))
                    .rounded(px(5.0))
                    .bg(if active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .flex()
                    .items_center()
                    .text_size(px(10.5))
                    .font_weight(if active {
                        gpui::FontWeight::MEDIUM
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(if active { theme.text } else { theme.text_faint })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover).text_color(theme.text))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.set_search_kind(kind, cx));
                    }),
            );
        }
        row.into_any_element()
    }

    pub(super) fn render_collection_results(
        &self,
        theme: &Theme,
        collections: Arc<Vec<CollectionSummary>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme.clone();
        let weak = cx.entity().downgrade();
        let n = collections.len();
        let mut results = uniform_list("collection-results", n, move |range, _window, _cx| {
            range
                .filter_map(|i| {
                    let collection = collections.get(i)?;
                    let weak = weak.clone();
                    let source_label = match collection.id.source.as_str() {
                        "netease" => "网易云",
                        "local" => "本地",
                        other => other,
                    };
                    let meta = match (collection.creator.as_deref(), collection.track_count) {
                        (Some(creator), Some(count)) => {
                            format!("{} · {} · {count} 首", collection.kind.label(), creator)
                        }
                        (Some(creator), None) => {
                            format!("{} · {creator}", collection.kind.label())
                        }
                        (None, Some(count)) => {
                            format!("{} · {count} 首", collection.kind.label())
                        }
                        (None, None) => collection.kind.label().to_string(),
                    };
                    let id = collection.id.clone();
                    let title = collection.title.clone();
                    Some(
                        div()
                            .id(SharedString::from(format!(
                                "coll-{}-{:?}-{}",
                                collection.id.source, collection.id.kind, collection.id.id
                            )))
                            .w_full()
                            .h(px(crate::theme::size::ROW))
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .rounded(px(7.0))
                            .hover(|s| s.bg(theme.hover))
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.open_collection(id.clone(), title.clone(), cx);
                                });
                            })
                            .child(
                                div()
                                    .size(px(34.0))
                                    .rounded(px(5.0))
                                    .bg(theme.surface_alt)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(crate::icons::icon(
                                        crate::icons::Icon::Layers,
                                        14.0,
                                        theme.text_faint,
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
                                            .text_size(px(12.5))
                                            .text_color(theme.text)
                                            .text_ellipsis()
                                            .child(SharedString::from(collection.title.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(theme.text_faint)
                                            .text_ellipsis()
                                            .child(SharedString::from(meta)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(theme.text_faint)
                                    .child(SharedString::from(source_label.to_string())),
                            )
                            .into_any_element(),
                    )
                })
                .collect::<Vec<_>>()
        });
        // 同曲目列表：滚动偏移存 App 句柄，沉浸层卸载页面后仍能恢复。
        results = results.track_scroll(self.collection_scroll.clone());
        if self.smooth_scroll {
            results = results.smooth_scroll();
        }
        results.flex_1().min_h_0().into_any_element()
    }

    pub(super) fn render_collection_search_panel(
        &self,
        width: Option<f32>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let phone = self.is_phone();
        // 与曲目搜索结果不同，集合搜索此前每帧都会 filter + clone 整个列表。
        let visible: Arc<Vec<CollectionSummary>> = {
            let mut cache = self.collection_route_cache.borrow_mut();
            let fresh = cache.as_ref().is_some_and(|(source, route, _)| {
                Arc::ptr_eq(source, &self.search_collections) && route == &self.source_route
            });
            if !fresh {
                let filtered: Arc<Vec<CollectionSummary>> = match &self.source_route {
                    Some(route) => Arc::new(
                        self.search_collections
                            .iter()
                            .filter(|c| &c.id.source == route)
                            .cloned()
                            .collect(),
                    ),
                    None => self.search_collections.clone(),
                };
                *cache = Some((
                    self.search_collections.clone(),
                    self.source_route.clone(),
                    filtered,
                ));
            }
            cache.as_ref().expect("initialized above").2.clone()
        };
        let route_label = self.source_route.as_ref().map(|id| {
            self.source_config_by_id(&id.0)
                .map(|source| source.name.clone())
                .unwrap_or_else(|| id.0.clone())
        });
        let subtitle = if self.searching {
            "正在向已加载的音源请求…".to_string()
        } else if visible.is_empty() {
            if self.search_input.read(cx).content().trim().is_empty() {
                "选择类型后输入关键词回车搜索".to_string()
            } else {
                format!("没有找到{}", self.search_kind_label())
            }
        } else {
            let mut s = format!(
                "{} 个{} · 聚合 {}ms",
                visible.len(),
                self.search_kind_label(),
                self.search_elapsed_ms
            );
            if let Some(route) = route_label {
                s.push_str(&format!(" · {route}"));
            }
            s
        };

        let mut actions = div().flex().items_center().gap(px(6.0));
        if self.source_route.is_some() {
            let weak = weak.clone();
            actions = actions.child(
                div()
                    .id("clear-collection-route")
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_dim)
                    .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Close,
                        9.0,
                        theme.text_faint,
                    ))
                    .child("全部音源")
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.source_route = None;
                            cx.notify();
                        });
                    }),
            );
        }

        let body: AnyElement = if visible.is_empty() {
            let (icon, title, hint) = if self.searching {
                (
                    crate::icons::Icon::Refresh,
                    "正在搜索",
                    "正在向已加载的音源请求结果",
                )
            } else if self.search_input.read(cx).content().trim().is_empty() {
                (
                    crate::icons::Icon::Search,
                    "搜索收藏",
                    "按专辑、歌手或歌单查找，回车搜索",
                )
            } else {
                (
                    crate::icons::Icon::Alert,
                    "没有找到结果",
                    "换个关键词，或切换搜索类型",
                )
            };
            div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(7.0))
                .pb(px(40.0))
                .child(crate::icons::icon(icon, 22.0, theme.text_faint))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(title),
                )
                .child(
                    div()
                        .max_w(px(240.0))
                        .text_center()
                        .text_size(px(11.0))
                        .line_height(px(16.0))
                        .text_color(theme.text_faint)
                        .child(hint),
                )
                .into_any_element()
        } else {
            self.render_collection_results(theme, visible, cx)
        };

        let header = if phone {
            self.render_phone_pane_header(
                theme,
                "搜索",
                subtitle,
                Some(actions.into_any_element()),
                cx,
            )
        } else {
            self.render_pane_header(theme, "搜索", subtitle, Some(actions.into_any_element()))
        };

        div()
            .h_full()
            .when_some(width, |d, w| d.w(px(w)))
            .when(width.is_none(), |d| d.flex_1())
            .min_w(px(280.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.surface)
            .when(!phone, |d| d.border_r_1().border_color(theme.border))
            .child(header)
            .child(self.render_search_tabs(theme, cx))
            .child(
                div()
                    .px(px(12.0))
                    .pb(px(8.0))
                    .child(self.search_input_view.clone()),
            )
            .child(body)
            .child(
                div()
                    .h(px(if phone {
                        92.0
                    } else {
                        crate::theme::size::DOCK + crate::theme::size::DOCK_BOTTOM + 14.0
                    }))
                    .flex_none(),
            )
            .into_any_element()
    }
}
