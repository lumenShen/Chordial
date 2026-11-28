//! `ChordialApp` 的 render_list_panel 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn search_kind_label(&self) -> &'static str {
        match self.search_kind {
            SearchKind::Track => "歌曲",
            SearchKind::Album => "专辑",
            SearchKind::Artist => "歌手",
            SearchKind::Playlist => "歌单",
        }
    }

    pub(super) fn render_list_panel(
        &self,
        width: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let phone = self.is_phone();

        if self.view_tab == ViewTab::Search && self.search_kind != SearchKind::Track {
            return self.render_collection_search_panel(width, &theme, cx);
        }

        let (tracks, list, title, subtitle): (Arc<Vec<Track>>, QueueList, String, String) =
            match self.view_tab {
                ViewTab::Search => {
                    let visible: Arc<Vec<Track>> = match &self.source_route {
                        Some(route) => {
                            let mut cache = self.search_route_cache.borrow_mut();
                            match cache.as_ref() {
                                Some((cached_tracks, cached_route, visible))
                                    if Arc::ptr_eq(cached_tracks, &self.search_tracks)
                                        && cached_route == &self.source_route =>
                                {
                                    visible.clone()
                                }
                                _ => {
                                    let visible = Arc::new(
                                        self.search_tracks
                                            .iter()
                                            .filter(|track| &track.id.source == route)
                                            .cloned()
                                            .collect::<Vec<_>>(),
                                    );
                                    *cache = Some((
                                        self.search_tracks.clone(),
                                        self.source_route.clone(),
                                        visible.clone(),
                                    ));
                                    visible
                                }
                            }
                        }
                        None => self.search_tracks.clone(),
                    };
                    let route_label = self.source_route.as_ref().map(|id| {
                        self.registry
                            .snapshot()
                            .into_iter()
                            .find(|s| &s.info.id == id)
                            .map(|s| s.info.display_name)
                            .unwrap_or_else(|| id.0.clone())
                    });
                    let failed_count = self
                        .search_reports
                        .iter()
                        .filter(|report| report.outcome.is_err())
                        .count();
                    let subtitle = if self.searching {
                        "正在向已加载的音源请求…".to_string()
                    } else if visible.is_empty() {
                        if self.search_input.read(cx).content().trim().is_empty() {
                            "输入关键词后回车搜索".to_string()
                        } else {
                            "没有找到结果".to_string()
                        }
                    } else {
                        let mut s =
                            format!("{} 项 · 聚合 {}ms", visible.len(), self.search_elapsed_ms);
                        if let Some(route) = route_label {
                            s.push_str(&format!(" · {route}"));
                        }
                        if failed_count > 0 {
                            s.push_str(&format!(" · {failed_count} 个音源失败"));
                        }
                        s
                    };
                    (visible, QueueList::Search, "搜索".into(), subtitle)
                }
                ViewTab::Library => {
                    let (tracks, list, title) = match self.library_filter {
                        LibraryFilter::All => (
                            self.library.clone(),
                            QueueList::Library,
                            "所有音乐".to_string(),
                        ),
                        LibraryFilter::Queue => {
                            (self.queue.clone(), QueueList::Queue, "播放队列".to_string())
                        }
                    };
                    let subtitle =
                        if self.library_scanning && self.library_filter == LibraryFilter::All {
                            "正在扫描本地库…".to_string()
                        } else if phone && self.library_filter == LibraryFilter::Queue {
                            format!("{} 首 · 拖动序号调整顺序", tracks.len())
                        } else {
                            format!("{} 首", tracks.len())
                        };
                    (tracks, list, title, subtitle)
                }
                ViewTab::Collection => {
                    let view = self.collection.as_ref();
                    let tracks = view
                        .map(|c| c.tracks.clone())
                        .unwrap_or_else(|| Arc::new(Vec::new()));
                    let title = view
                        .map(|c| c.title.clone())
                        .unwrap_or_else(|| "集合".into());
                    let subtitle = view
                        .map(|c| {
                            if let Some(error) = &c.error {
                                format!("加载失败：{error}")
                            } else {
                                c.subtitle.clone()
                            }
                        })
                        .unwrap_or_else(|| "未选择集合".into());
                    (tracks, QueueList::Collection, title, subtitle)
                }
            };

        let action_h = if phone { 40.0 } else { 26.0 };
        let mut actions = div().flex().items_center().gap(px(6.0));
        if self.view_tab == ViewTab::Collection {
            let weak = weak.clone();
            actions = actions.child(
                div()
                    .id("collection-back")
                    .h(px(action_h))
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
                        crate::icons::Icon::ChevronDown,
                        9.0,
                        theme.text_faint,
                    ))
                    .child("返回")
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.collection = None;
                            let old_cover = app.collection_cover.take();
                            app.retire_image(old_cover);
                            app.view_tab = ViewTab::Library;
                            cx.notify();
                        });
                    }),
            );
        }
        if self.source_route.is_some() {
            let weak = weak.clone();
            actions = actions.child(
                div()
                    .id("clear-route")
                    .h(px(action_h))
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
        if !tracks.is_empty() {
            let weak = weak.clone();
            actions = actions.child(crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Play, 13.0, theme.text_dim),
                if phone { 40.0 } else { 28.0 },
                "播放全部",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_from_list(list, 0, cx));
                },
            ));
        }
        if list == QueueList::Queue {
            // 队列专属：模式胶囊（点击循环）+ 清除。队列本来就在曲库页，
            // 不再单独做一层「队列页」，只把状态与操作补齐。
            let mode = self.play_mode;
            let weak_mode = weak.clone();
            actions = actions.child(
                div()
                    .id("queue-mode")
                    .h(px(action_h))
                    .px(px(9.0))
                    .rounded(px(13.0))
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .text_color(theme.text_dim)
                    .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                    .child(crate::icons::icon(
                        match mode {
                            PlayMode::Shuffle => crate::icons::Icon::Shuffle,
                            PlayMode::RepeatOne => crate::icons::Icon::RepeatOne,
                            PlayMode::RepeatAll => crate::icons::Icon::Repeat,
                            PlayMode::Sequential => crate::icons::Icon::Forward,
                        },
                        10.0,
                        theme.text_faint,
                    ))
                    .child(mode.short_label())
                    .on_click(move |_, _, cx| {
                        let _ = weak_mode.update(cx, |app, cx| app.cycle_play_mode(cx));
                    }),
            );
            if !self.queue.is_empty() {
                let weak = weak.clone();
                actions = actions.child(
                    div()
                        .id("queue-clear-header")
                        .h(px(action_h))
                        .px(px(8.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_size(px(10.5))
                        .text_color(if self.queue_clear_confirm {
                            theme.danger
                        } else {
                            theme.text_faint
                        })
                        .when(self.queue_clear_confirm, |d| {
                            d.border_color(theme.danger).bg(theme.danger.opacity(0.08))
                        })
                        .hover(|s| s.bg(theme.surface_alt).text_color(theme.danger))
                        .child(if self.queue_clear_confirm {
                            "确认清除"
                        } else {
                            "清除"
                        })
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                if !app.queue_clear_confirm {
                                    app.queue_clear_confirm = true;
                                    cx.notify();
                                    return;
                                }
                                app.stop_playback(cx);
                                app.set_queue(Arc::new(Vec::new()), None);
                                app.queue_pos = None;
                                app.selection = None;
                                cx.notify();
                            });
                        }),
                );
            }
        }
        let has_album = self
            .current_track
            .as_ref()
            .and_then(|t| t.album.as_ref())
            .and_then(|a| a.id.as_ref())
            .is_some();
        let has_artist = self
            .current_track
            .as_ref()
            .is_some_and(|t| t.artists.iter().any(|a| a.id.is_some()));
        if has_album && !phone {
            let weak = weak.clone();
            actions = actions.child(crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Layers, 13.0, theme.text_dim),
                28.0,
                "当前专辑",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.open_album_for_current(cx));
                },
            ));
        }
        if has_artist && !phone {
            let weak = weak.clone();
            actions = actions.child(crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Note, 13.0, theme.text_dim),
                28.0,
                "当前歌手",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.open_artist_for_current(cx));
                },
            ));
        }

        let body: AnyElement = if tracks.is_empty() {
            let (icon, empty_title, hint) = match self.view_tab {
                ViewTab::Search => {
                    if self.searching {
                        (
                            crate::icons::Icon::Refresh,
                            "正在搜索",
                            "正在向已加载的音源请求结果",
                        )
                    } else if self.search_input.read(cx).content().trim().is_empty() {
                        (
                            crate::icons::Icon::Search,
                            "搜索曲目",
                            "输入歌名、歌手或专辑，回车搜索",
                        )
                    } else {
                        (
                            crate::icons::Icon::Alert,
                            "没有找到结果",
                            "换个关键词，或到全部音乐中查找",
                        )
                    }
                }
                ViewTab::Library => {
                    if self.library_scanning && self.library_filter == LibraryFilter::All {
                        (
                            crate::icons::Icon::Refresh,
                            "正在扫描本地库",
                            "读取标签与同名歌词，请稍候",
                        )
                    } else if self.library_filter == LibraryFilter::Queue {
                        (
                            crate::icons::Icon::Queue,
                            "播放队列为空",
                            "从搜索结果或全部音乐里点一首开始",
                        )
                    } else {
                        (
                            crate::icons::Icon::Note,
                            "本地库为空",
                            "选择电脑上的音乐文件夹，或添加在线音源",
                        )
                    }
                }
                ViewTab::Collection => {
                    let (title, hint) = match self.collection.as_ref() {
                        Some(view) if view.loading => {
                            ("正在加载集合", "正在请求歌单/专辑/歌手信息")
                        }
                        Some(view) if view.error.is_some() => {
                            ("集合加载失败", "确认音源已启用且服务可用，然后重试")
                        }
                        Some(_) => ("集合为空", "这个歌单/专辑暂时没有曲目"),
                        None => ("未选择集合", "从侧栏歌单或曲目信息进入"),
                    };
                    (crate::icons::Icon::Layers, title, hint)
                }
            };
            {
                let mut empty = div()
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
                            .child(empty_title),
                    )
                    .child(
                        div()
                            .max_w(px(280.0))
                            .text_center()
                            .text_size(px(11.0))
                            .line_height(px(16.0))
                            .text_color(theme.text_faint)
                            .child(hint),
                    );
                if self.view_tab == ViewTab::Library
                    && self.library_filter == LibraryFilter::All
                    && !self.library_scanning
                {
                    let local_id = self
                        .source_configs
                        .iter()
                        .find(|source| source.is_local())
                        .map(|source| SourceId(source.id.clone()));
                    let target = local_id.clone();
                    let weak_choose = weak.clone();
                    empty = empty.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(8.0))
                            .mt(px(8.0))
                            .child(crate::widgets::text_button_with_height(
                                &theme,
                                "选择音乐文件夹",
                                true,
                                if phone { 40.0 } else { 30.0 },
                                move |_, _, cx| {
                                    let _ = weak_choose.update(cx, |app, cx| {
                                        if let Some(id) = target.clone() {
                                            app.open_source_edit(id, cx);
                                        } else {
                                            app.open_new_source(cx);
                                        }
                                    });
                                },
                            ))
                            .child({
                                let weak_manage = weak.clone();
                                crate::widgets::text_button_with_height(
                                    &theme,
                                    "管理音源",
                                    false,
                                    if phone { 40.0 } else { 30.0 },
                                    move |_, _, cx| {
                                        let _ = weak_manage.update(cx, |app, cx| {
                                            app.open_source_manager(cx);
                                        });
                                    },
                                )
                            }),
                    );
                } else if self.view_tab == ViewTab::Library
                    && self.library_filter == LibraryFilter::Queue
                {
                    let weak_search = weak.clone();
                    empty = empty.child(div().mt(px(8.0)).child(
                        crate::widgets::text_button_with_height(
                            &theme,
                            "搜索歌曲",
                            true,
                            if phone { 40.0 } else { 30.0 },
                            move |_, window, cx| {
                                let _ = weak_search.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Browse;
                                    app.view_tab = ViewTab::Search;
                                    app.source_route = None;
                                    let input = app.search_input.clone();
                                    cx.defer_in(window, move |_, window, cx| {
                                        focus_input(&input, window, cx);
                                    });
                                    cx.notify();
                                });
                            },
                        ),
                    ));
                } else if self.view_tab == ViewTab::Collection
                    && self
                        .collection
                        .as_ref()
                        .is_some_and(|view| view.error.is_some())
                {
                    let retry = self
                        .collection
                        .as_ref()
                        .map(|view| (view.id.clone(), view.title.clone()));
                    let weak_retry = weak.clone();
                    empty = empty.child(div().mt(px(8.0)).child(
                        crate::widgets::text_button_with_height(
                            &theme,
                            "重试",
                            true,
                            if phone { 40.0 } else { 30.0 },
                            move |_, _, cx| {
                                let _ = weak_retry.update(cx, |app, cx| {
                                    if let Some((id, title)) = &retry {
                                        app.open_collection(id.clone(), title.clone(), cx);
                                    }
                                });
                            },
                        ),
                    ));
                }
                empty.into_any_element()
            }
        } else {
            self.render_track_table(&theme, tracks, list, cx)
        };

        let failed_search_count = self
            .search_reports
            .iter()
            .filter(|report| report.outcome.is_err())
            .count();

        let header = if phone {
            self.render_phone_pane_header(
                &theme,
                title,
                subtitle,
                Some(actions.into_any_element()),
                cx,
            )
        } else {
            self.render_pane_header(&theme, title, subtitle, Some(actions.into_any_element()))
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
            .when(self.view_tab == ViewTab::Collection, |d| {
                d.child(self.render_collection_header(&theme, cx))
            })
            .when(self.view_tab == ViewTab::Search, |d| {
                d.child(self.render_search_tabs(&theme, cx))
            })
            .when(
                self.view_tab == ViewTab::Search && !self.searching && failed_search_count > 0,
                |d| {
                    let weak_retry = weak.clone();
                    d.child(
                        div()
                            .mx(px(12.0))
                            .mb(px(8.0))
                            .px(px(10.0))
                            .py(px(7.0))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(theme.warn.opacity(0.28))
                            .bg(theme.warn.opacity(0.07))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Alert,
                                14.0,
                                theme.warn,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_dim)
                                    .child(format!(
                                        "{failed_search_count} 个音源未响应，当前显示可用结果"
                                    )),
                            )
                            .child(crate::widgets::text_button(
                                &theme,
                                "重试",
                                false,
                                move |_, _, cx| {
                                    let _ = weak_retry.update(cx, |app, cx| {
                                        let query = app.search_input.read(cx).content().to_string();
                                        if !query.trim().is_empty() {
                                            app.start_search(query, cx);
                                        }
                                    });
                                },
                            )),
                    )
                },
            )
            // 播放队列是操作面板，不需要再占一行搜索框。
            .when(
                self.view_tab != ViewTab::Collection
                    && !(self.view_tab == ViewTab::Library
                        && self.library_filter == LibraryFilter::Queue),
                |d| {
                    d.child(
                        div()
                            .px(px(12.0))
                            .pb(px(8.0))
                            .child(self.search_input_view.clone()),
                    )
                },
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
