//! `ChordialApp` 的 render_menus 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    /// 曲目行右键 / 「⋮」菜单。窗口坐标进来，渲染时夹回视口。
    pub(super) fn render_track_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let Some(menu) = &self.track_menu else {
            return div().into_any_element();
        };
        let width = self.viewport_width;
        let height = self.viewport_height;
        let panel_w = 238.0;
        let panel_h = 414.0;
        let x = menu.x.min((width - panel_w - 8.0).max(8.0)).max(8.0);
        let y = menu.y.min((height - panel_h - 12.0).max(44.0)).max(44.0);

        let item =
            |icon: crate::icons::Icon,
             label: &'static str,
             danger: bool,
             on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>| {
                let color = if danger { theme.danger } else { theme.text_dim };
                div()
                    .id(SharedString::from(format!("track-menu-{label}")))
                    .h(px(34.0))
                    .w_full()
                    .px(px(10.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .cursor_pointer()
                    .hover(|s| {
                        s.bg(if danger {
                            theme.danger.opacity(0.12)
                        } else {
                            theme.hover
                        })
                    })
                    .child(crate::icons::icon(icon, 14.0, color))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.0))
                            .text_color(if danger { theme.danger } else { theme.text })
                            .child(label),
                    )
                    .on_click(on_click)
                    .into_any_element()
            };

        let title = menu.track.title.clone();
        let artist = menu
            .track
            .artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        let (list, index) = (menu.list, menu.index);
        let menu_is_current = menu.is_current;
        let mut rows: Vec<AnyElement> = Vec::new();
        {
            let weak = weak.clone();
            let playing = self.player_status.state == PlayerState::Playing;
            let (icon, label) = if menu.is_current && playing {
                (crate::icons::Icon::Pause, "暂停")
            } else {
                (crate::icons::Icon::Play, "播放")
            };
            rows.push(item(
                icon,
                label,
                false,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.track_menu = None;
                        if app.current_track.is_some() && menu_is_current {
                            app.toggle_playback(cx);
                        } else {
                            app.play_from_list(list, index, cx);
                        }
                    });
                }),
            ));
        }
        {
            let weak = weak.clone();
            let track = menu.track.clone();
            rows.push(item(
                crate::icons::Icon::Forward,
                "下一首播放",
                false,
                Box::new(move |_, _, cx| {
                    let track = track.clone();
                    let _ = weak.update(cx, |app, cx| {
                        app.track_menu = None;
                        app.queue_play_next(track, cx);
                    });
                }),
            ));
        }
        {
            let weak = weak.clone();
            let track = menu.track.clone();
            rows.push(item(
                crate::icons::Icon::Plus,
                "添加到播放队列",
                false,
                Box::new(move |_, _, cx| {
                    let track = track.clone();
                    let _ = weak.update(cx, |app, cx| {
                        app.track_menu = None;
                        app.queue_append(track, cx);
                    });
                }),
            ));
        }
        {
            let weak = weak.clone();
            let track = menu.track.clone();
            let (x, y) = (menu.x, menu.y);
            rows.push(item(
                crate::icons::Icon::Layers,
                "添加到歌单",
                false,
                Box::new(move |_, _, cx| {
                    let track = track.clone();
                    let _ = weak.update(cx, |app, cx| {
                        app.open_playlist_picker(track, x, y, cx);
                    });
                }),
            ));
        }
        if menu.has_album {
            let weak = weak.clone();
            let track = menu.track.clone();
            rows.push(item(
                crate::icons::Icon::Layers,
                "查看专辑",
                false,
                Box::new(move |_, _, cx| {
                    let track = track.clone();
                    let _ = weak.update(cx, |app, cx| {
                        app.track_menu = None;
                        app.open_album_for_track(&track, cx);
                    });
                }),
            ));
        }
        if menu.has_artist {
            let weak = weak.clone();
            let track = menu.track.clone();
            rows.push(item(
                crate::icons::Icon::Person,
                "查看歌手",
                false,
                Box::new(move |_, _, cx| {
                    let track = track.clone();
                    let _ = weak.update(cx, |app, cx| {
                        app.track_menu = None;
                        app.open_artist_for_track(&track, cx);
                    });
                }),
            ));
        }
        {
            let weak = weak.clone();
            rows.push(item(
                crate::icons::Icon::Check,
                "多选",
                false,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.track_menu = None;
                        app.enter_selection(list, index, cx);
                    });
                }),
            ));
        }
        if menu.in_queue {
            let weak = weak.clone();
            let id = menu.track.id.clone();
            rows.push(item(
                crate::icons::Icon::Close,
                "从播放队列移除",
                true,
                Box::new(move |_, _, cx| {
                    let id = id.clone();
                    let _ = weak.update(cx, |app, cx| {
                        app.track_menu = None;
                        app.remove_track_from_queue(&id, cx);
                    });
                }),
            ));
        }

        div()
            .id("track-menu-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, _, _, cx| app.close_track_menu(cx)),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|app, _, _, cx| app.close_track_menu(cx)),
            )
            .child(
                div()
                    .id("track-menu-panel")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(panel_w))
                    .rounded(px(12.0))
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(vec![BoxShadow {
                        color: gpui::black().opacity(0.42),
                        blur_radius: px(28.0),
                        spread_radius: px(0.0),
                        offset: point(px(0.0), px(10.0)),
                    }])
                    .p(px(6.0))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .px(px(6.0))
                            .pt(px(4.0))
                            .pb(px(8.0))
                            .child(
                                div()
                                    .size(px(34.0))
                                    .rounded(px(6.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(theme.surface_alt)
                                    .child(crate::icons::icon(
                                        crate::icons::Icon::Note,
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
                                    .gap(px(1.0))
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .text_ellipsis()
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.0))
                                            .text_color(theme.text_faint)
                                            .text_ellipsis()
                                            .child(SharedString::from(artist)),
                                    ),
                            ),
                    )
                    .child(div().h(px(1.0)).mx(px(4.0)).my(px(4.0)).bg(theme.border))
                    .children(rows)
                    .child(div().h(px(1.0)).mx(px(4.0)).my(px(4.0)).bg(theme.border))
                    .child(item(
                        crate::icons::Icon::Close,
                        "取消",
                        false,
                        Box::new({
                            let weak = weak.clone();
                            move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| app.close_track_menu(cx));
                            }
                        }),
                    )),
            )
            .into_any_element()
    }

    /// Dock 上方的队列概览 / 更多操作。歌词没有浮层，曲目区直接进沉浸页。
    pub(super) fn render_dock_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let Some(menu) = self.dock_menu else {
            return div().into_any_element();
        };
        let width = self.viewport_width;
        let dock_w = self.dock_width();
        let dock_left = ((width - dock_w) / 2.0).max(8.0);
        let dock_right = (dock_left + dock_w).min(width);
        // 右缘对齐 Dock 的 18px 内边距，和右下角按钮同一列。
        let anchor_right = (width - dock_right + 18.0).max(8.0);
        let bottom = crate::theme::size::DOCK_BOTTOM + crate::theme::size::DOCK + 10.0;

        let (panel_w, panel): (f32, AnyElement) = match menu {
            DockMenu::Queue => {
                // 随机模式下“接下来播放”必须展示真实随机顺序，而不是
                // 队列里的物理下一项。
                let upcoming: Vec<(usize, Track)> = self
                    .upcoming_queue_indices(3)
                    .into_iter()
                    .filter_map(|index| self.queue.get(index).cloned().map(|track| (index, track)))
                    .collect();
                let mut rows = div().flex().flex_col().gap(px(1.0));
                if upcoming.is_empty() {
                    rows = rows.child(
                        div()
                            .px(px(9.0))
                            .py(px(14.0))
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .child(if self.queue.is_empty() {
                                "播放队列为空"
                            } else if self.play_mode == PlayMode::Shuffle {
                                "随机播放中"
                            } else {
                                "已经是队列最后一首"
                            }),
                    );
                }
                for (index, track) in upcoming {
                    let cover_image = track_cover_key(&track).and_then(|key| {
                        self.track_covers
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .images
                            .get(&key)
                            .cloned()
                    });
                    let title = SharedString::from(track.title.clone());
                    let artist = SharedString::from(
                        track
                            .artists
                            .iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" / "),
                    );
                    let duration = SharedString::from(fmt_ms(track.duration_ms.unwrap_or(0)));
                    let weak_row = weak.clone();
                    rows = rows.child(
                        div()
                            .id(gpui::ElementId::named_usize("dock-queue-row", index))
                            .h(px(38.0))
                            .px(px(8.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.hover))
                            .child(crate::widgets::cover_thumb(
                                &theme,
                                cover_image,
                                30.0,
                                1.0,
                                crate::icons::icon(
                                    crate::icons::Icon::Note,
                                    12.0,
                                    theme.text_faint,
                                ),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.0))
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .text_ellipsis()
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.0))
                                            .text_color(theme.text_faint)
                                            .text_ellipsis()
                                            .child(artist),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(30.0))
                                    .flex_none()
                                    .text_size(px(9.0))
                                    .text_color(theme.text_faint)
                                    .text_align(gpui::TextAlign::Right)
                                    .child(duration),
                            )
                            .on_click(move |_, _, cx| {
                                let _ = weak_row.update(cx, |app, cx| {
                                    app.dock_menu = None;
                                    app.play_from_list(QueueList::Queue, index, cx);
                                });
                            }),
                    );
                }

                let open_queue = {
                    let weak = weak.clone();
                    div()
                        .id("dock-open-queue")
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .cursor_pointer()
                        .text_size(px(9.5))
                        .text_color(theme.text_dim)
                        .hover(|s| s.text_color(theme.text))
                        .child("打开播放队列")
                        .child(crate::icons::icon(
                            crate::icons::Icon::ChevronRight,
                            10.0,
                            theme.text_faint,
                        ))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.open_queue_list(cx));
                        })
                };
                let body = div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(8.0))
                            .pt(px(5.0))
                            .pb(px(7.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.text)
                                    .child("接下来播放"),
                            )
                            .child(
                                div().text_size(px(9.5)).text_color(theme.text_faint).child(
                                    SharedString::from(format!("队列 · {}", self.queue.len())),
                                ),
                            ),
                    )
                    .child(rows)
                    .child(
                        div()
                            .mt(px(4.0))
                            .pt(px(8.0))
                            .pb(px(5.0))
                            .px(px(8.0))
                            .border_t_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(9.0))
                                    .text_color(theme.text_faint)
                                    .child("按顺序播放"),
                            )
                            .child(open_queue),
                    )
                    .into_any_element();
                (320.0, body)
            }
            DockMenu::More => {
                let item = |icon: crate::icons::Icon,
                            label: &'static str,
                            meta: Option<&'static str>,
                            id: &'static str,
                            on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>| {
                    div()
                        .id(SharedString::from(format!("dock-more-{id}")))
                        .h(px(34.0))
                        .px(px(9.0))
                        .rounded(px(7.0))
                        .flex()
                        .items_center()
                        .gap(px(9.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.hover))
                        .child(crate::icons::icon(icon, 15.0, theme.text_dim))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(11.5))
                                .text_color(theme.text)
                                .text_ellipsis()
                                .child(label),
                        )
                        .when_some(meta, |d, meta| {
                            d.child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(theme.text_faint)
                                    .child(meta),
                            )
                        })
                        .on_click(on_click)
                };

                let preference = {
                    let weak = weak.clone();
                    item(
                        crate::icons::Icon::Sliders,
                        self.source_preference.label(),
                        Some("切换"),
                        "preference",
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.cycle_source_preference(cx));
                        }),
                    )
                };
                let download = {
                    let weak = weak.clone();
                    item(
                        crate::icons::Icon::Download,
                        "下载当前曲目",
                        None,
                        "download",
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.dock_menu = None;
                                app.download_current(cx);
                            });
                        }),
                    )
                };
                let equalizer = {
                    let weak = weak.clone();
                    item(
                        crate::icons::Icon::Equalizer,
                        "打开均衡器",
                        None,
                        "eq",
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.dock_menu = None;
                                app.open_eq(cx);
                            });
                        }),
                    )
                };

                let body = div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(9.0))
                            .pt(px(6.0))
                            .pb(px(6.0))
                            .text_size(px(9.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text_faint)
                            .child("播放来源"),
                    )
                    .child(preference)
                    .child(div().h(px(1.0)).mx(px(7.0)).my(px(4.0)).bg(theme.border))
                    .child(download)
                    .child(equalizer)
                    .into_any_element();
                (232.0, body)
            }
        };

        div()
            .id("dock-menu-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, _, _, cx| app.close_dock_menu(cx)),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|app, _, _, cx| app.close_dock_menu(cx)),
            )
            .child(
                div()
                    .id("dock-menu-panel")
                    .absolute()
                    .right(px(anchor_right))
                    .bottom(px(bottom))
                    .w(px(panel_w))
                    .rounded(px(12.0))
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(vec![BoxShadow {
                        color: theme.shadow_strong,
                        offset: point(px(0.0), px(12.0)),
                        blur_radius: px(30.0),
                        spread_radius: px(0.0),
                    }])
                    .p(px(6.0))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(panel),
            )
            .into_any_element()
    }

    /// 多选态的顶部栏：返回 / 已选数量 / 全选。保留红绿灯。
    pub(super) fn render_phone_selection_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let count = self.selection.as_ref().map(|s| s.count()).unwrap_or(0);
        let all = self
            .selection
            .as_ref()
            .is_some_and(|s| !s.selected.is_empty() && s.selected.iter().all(|v| *v));
        let back = {
            let weak = weak.clone();
            div()
                .id("selection-back")
                .size(px(28.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .child(crate::icons::icon(
                    crate::icons::Icon::ChevronLeft,
                    16.0,
                    theme.text_dim,
                ))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.exit_selection(cx));
                })
        };
        let select_all = {
            let weak = weak.clone();
            div()
                .id("selection-all")
                .h(px(28.0))
                .px(px(10.0))
                .rounded(px(7.0))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(11.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text_dim)
                .hover(|s| s.bg(theme.hover).text_color(theme.text))
                .child(if all { "取消全选" } else { "全选" })
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.select_all_visible(cx));
                })
        };
        div()
            .h(px(40.0))
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
            .child(back)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(13.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .child(SharedString::from(format!("已选 {count} 首"))),
            )
            .child(select_all)
            .into_any_element()
    }

    /// 多选态的底部批量操作条。
    pub(super) fn render_phone_selection_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let empty = self.selection.as_ref().map(|s| s.count()).unwrap_or(0) == 0;
        let action = |icon: crate::icons::Icon,
                      label: &'static str,
                      id: &'static str,
                      action: SelectionAction| {
            let weak = weak.clone();
            div()
                .id(id)
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(3.0))
                .rounded(px(12.0))
                .cursor_pointer()
                .opacity(if empty { 0.35 } else { 1.0 })
                .hover(|s| s.bg(theme.hover))
                .child(crate::icons::icon(icon, 16.0, theme.text_dim))
                .child(
                    div()
                        .text_size(px(9.5))
                        .text_color(theme.text_faint)
                        .child(label),
                )
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        if app.selection.as_ref().is_some_and(|s| s.count() > 0) {
                            app.apply_selection(action, cx);
                        }
                    });
                })
        };
        div()
            .id("phone-selection-actions")
            .absolute()
            .left(px(12.0))
            .right(px(12.0))
            .bottom(px(12.0))
            .h(px(56.0))
            .rounded(px(16.0))
            .bg(theme.surface.opacity(0.97))
            .border_1()
            .border_color(theme.border)
            .shadow(vec![BoxShadow {
                color: gpui::black().opacity(0.32),
                blur_radius: px(24.0),
                spread_radius: px(0.0),
                offset: point(px(0.0), px(-4.0)),
            }])
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .child(action(
                crate::icons::Icon::Play,
                "播放",
                "selection-play",
                SelectionAction::Play,
            ))
            .child(action(
                crate::icons::Icon::Forward,
                "下一首",
                "selection-next",
                SelectionAction::PlayNext,
            ))
            .child(action(
                crate::icons::Icon::Plus,
                "加入队列",
                "selection-append",
                SelectionAction::Append,
            ))
            .child(action(
                crate::icons::Icon::Close,
                "移出队列",
                "selection-remove",
                SelectionAction::Remove,
            ))
            .into_any_element()
    }

    /// 手机标题栏：播放页/歌词页为透明覆盖层（顶部轻压暗），曲库页为
    /// 实心 sidebar 色。红绿灯与拖拽区保留，否则无边框窗口会失去
    /// 关闭/移动能力；歌词页额外提供返回播放。
    pub(super) fn render_phone_titlebar(
        &self,
        overlay: bool,
        back: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let fg = if overlay {
            gpui::white().opacity(0.78)
        } else {
            theme.text_dim
        };
        let menu = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Menu, 16.0, fg),
                40.0,
                "菜单",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        if app.drawer_open {
                            app.close_drawer(cx);
                        } else {
                            app.open_drawer(cx);
                        }
                    });
                },
            )
        };
        let back_btn = back.then(|| {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::ChevronLeft, 16.0, fg),
                40.0,
                "返回播放",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.open_phone_sheet(cx);
                    });
                },
            )
        });
        let center = {
            let pill = (overlay && !back).then(|| {
                div()
                    .w(px(50.0))
                    .h(px(8.0))
                    .rounded(px(4.0))
                    .bg(gpui::white().opacity(0.176))
                    .into_any_element()
            });
            let title = back.then(|| {
                div()
                    .max_w(px(210.0))
                    .text_size(px(12.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(gpui::white().opacity(0.88))
                    .text_ellipsis()
                    .child(SharedString::from(self.current_title_text()))
                    .into_any_element()
            });
            let brand = (!overlay).then(|| {
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
                    )
                    .into_any_element()
            });
            let base = div()
                .id(if overlay {
                    "phone-titlebar-overlay"
                } else {
                    "phone-titlebar-drag"
                })
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor(if overlay {
                    CursorStyle::Arrow
                } else {
                    CursorStyle::OpenHand
                });
            // 播放页/歌词页的标题栏上叠加了把手、收起与返回按钮。Windows
            // 的原生 HTCAPTION 命中会吞掉这些子元素，所以覆盖态不再把整条
            // 标题栏注册成窗口拖动区，而是在两侧留出窄拖动带。
            let center = if overlay {
                base
            } else {
                Self::window_drag_area(base)
            };
            center.children(pill).children(title).children(brand)
        };

        let bar = div()
            .h(px(40.0))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(14.0))
            .when(overlay, |d| {
                d.absolute().top_0().left_0().right_0().bg(linear_gradient(
                    180.0,
                    linear_color_stop(gpui::black().opacity(0.30), 0.0),
                    linear_color_stop(gpui::black().opacity(0.0), 1.0),
                ))
            })
            .when(!overlay, |d| {
                d.bg(theme.sidebar).border_b_1().border_color(theme.border)
            })
            .child(Self::traffic_lights())
            .children(back_btn)
            .child(center)
            .child(menu);
        bar.into_any_element()
    }

    /// 手机播放页底部与沉浸层共用同一组全屏 / 视图选择器。
    pub(super) fn phone_bottom_row(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        self.immersive_bottom_row(width, cx)
    }

    /// 手机壳底部导航高度。曲库页常驻一条，mini player 悬浮在它上方。
    pub(super) const PHONE_NAV_H: f32 = crate::layout_morph::PHONE_NAV_H;

    /// 手机壳底部导航：曲库 / 队列 / 搜索 / 音源。
    ///
    /// 一级导航必须常驻可见，不能只藏在标题栏的汉堡菜单里；抽屉保留给
    /// 睡眠定时、均衡器、音频信息等次级操作，以及播放页/歌词页的跳转。
    pub(super) fn render_phone_bottom_nav(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();

        let browsing = self.phone_page == PhonePage::Browse;
        let library_active = browsing
            && (matches!(self.page, Some(MainPage::Playlist(_)))
                || self.view_tab == ViewTab::Collection
                || (self.view_tab == ViewTab::Library
                    && self.library_filter == LibraryFilter::All));
        let queue_active = browsing
            && self.page.is_none()
            && self.view_tab == ViewTab::Library
            && self.library_filter == LibraryFilter::Queue;
        let search_active = browsing && self.page.is_none() && self.view_tab == ViewTab::Search;
        let sources_active = browsing
            && matches!(
                self.page,
                Some(
                    MainPage::Source(_)
                        | MainPage::ManageSources
                        | MainPage::NewSource
                        | MainPage::EditSource(_)
                )
            );

        let tab =
            |icon: crate::icons::Icon,
             label: &'static str,
             id: &'static str,
             active: bool,
             on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>| {
                let fg = if active { theme.text } else { theme.text_faint };
                div()
                    .id(id)
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(3.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover))
                    .child(crate::icons::icon(icon, 18.0, fg))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .font_weight(if active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(fg)
                            .child(label),
                    )
                    .on_click(on_click)
            };

        let library = {
            let weak = weak.clone();
            tab(
                crate::icons::Icon::Note,
                "曲库",
                "phone-nav-library",
                library_active,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Browse;
                        app.page = None;
                        app.view_tab = ViewTab::Library;
                        app.library_filter = LibraryFilter::All;
                        app.source_route = None;
                        cx.notify();
                    });
                }),
            )
        };
        let queue = {
            let weak = weak.clone();
            tab(
                crate::icons::Icon::Queue,
                "队列",
                "phone-nav-queue",
                queue_active,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Browse;
                        app.page = None;
                        app.view_tab = ViewTab::Library;
                        app.library_filter = LibraryFilter::Queue;
                        cx.notify();
                    });
                }),
            )
        };
        let search = {
            let weak = weak.clone();
            tab(
                crate::icons::Icon::Search,
                "搜索",
                "phone-nav-search",
                search_active,
                Box::new(move |_, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Browse;
                        app.page = None;
                        app.view_tab = ViewTab::Search;
                        app.source_route = None;
                        // 搜索框在切页后的下一帧才挂载，聚焦必须延后一拍。
                        let input = app.search_input.clone();
                        cx.defer_in(window, move |_, window, cx| {
                            focus_input(&input, window, cx);
                        });
                        cx.notify();
                    });
                }),
            )
        };
        let sources = {
            let weak = weak.clone();
            tab(
                crate::icons::Icon::Layers,
                "音源",
                "phone-nav-sources",
                sources_active,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Browse;
                        app.open_source_manager(cx);
                    });
                }),
            )
        };

        div()
            .id("phone-bottom-nav")
            .h(px(Self::PHONE_NAV_H))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(library)
            .child(queue)
            .child(search)
            .child(sources)
            .into_any_element()
    }

    /// 无边框窗口的三颗红绿灯。桌面标题栏与手机标题栏共用。
    pub(super) fn traffic_lights() -> AnyElement {
        let light = |id: &'static str, color: gpui::Hsla| {
            div()
                .id(id)
                .size(px(12.0))
                .rounded(px(6.0))
                .bg(color)
                .cursor_pointer()
        };
        let close = light("tl-close", crate::theme::hex(0xFF5F57)).on_click(|_, _, cx| cx.quit());
        let minimize = light("tl-min", crate::theme::hex(0xFEBC2E))
            .on_click(|_, window, _| window.minimize_window());
        let maximize = light("tl-max", crate::theme::hex(0x28C840))
            .on_click(|_, window, _| window.zoom_window());
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(close)
            .child(minimize)
            .child(maximize)
            .into_any_element()
    }
}
