//! `ChordialApp` 的 render_track_list 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    /// 手机曲库页的页头：返回播放 + 标题 + 动作。
    /// 桌面页头保留 17px 标题；手机没有右栏，页头必须让出宽度给返回键。
    pub(super) fn render_phone_pane_header(
        &self,
        theme: &Theme,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        right: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        // 没有当前曲目时播放页是空的；此时不摆一个按了没反应的返回键。
        // 多选态已经有顶部「返回」，列表头不再重复。
        let back: AnyElement = if self.current_track.is_some() && self.selection.is_none() {
            let weak = weak.clone();
            crate::widgets::icon_button(
                theme,
                crate::icons::icon(crate::icons::Icon::ChevronLeft, 15.0, theme.text_dim),
                30.0,
                "返回播放",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.open_phone_sheet(cx);
                    });
                },
            )
        } else {
            div().w(px(30.0)).h(px(30.0)).into_any_element()
        };
        div()
            .h(px(54.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(back)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(15.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(title.into()),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(subtitle.into()),
                    ),
            )
            .children(right)
            .into_any_element()
    }

    pub(super) fn render_pane_header(
        &self,
        theme: &Theme,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        right: Option<AnyElement>,
    ) -> AnyElement {
        div()
            .h(px(crate::theme::size::PANE_HEAD))
            .px(px(16.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(title.into()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(subtitle.into()),
                    ),
            )
            .children(right)
            .into_any_element()
    }

    // ================= 曲目列表（窄栏） =================

    pub(super) fn render_track_row(
        theme: &Theme,
        index: usize,
        row: &TrackRowText,
        track_covers: &std::sync::Mutex<TrackCovers>,
        list: QueueList,
        queue_pos: Option<usize>,
        queue_current_id: Option<&chordal_core::model::TrackId>,
        weak: gpui::WeakEntity<ChordialApp>,
        tracks: Arc<Vec<Track>>,
        phone: bool,
        select_mode: bool,
        selected: bool,
        drag_from: Option<usize>,
        drag_to: Option<usize>,
        style: &TrackRowStyle,
        trailing: Option<AnyElement>,
    ) -> AnyElement {
        let is_current = queue_pos == Some(index) && queue_current_id == Some(&row.id);
        let drag_source = drag_from == Some(index);
        let drag_target = drag_to == Some(index) && drag_from != Some(index);
        let number: AnyElement = if select_mode {
            div()
                .size(px(17.0))
                .rounded(px(9.0))
                .border_1()
                .border_color(if selected {
                    theme.accent
                } else {
                    theme.border_strong
                })
                .bg(if selected {
                    theme.accent
                } else {
                    gpui::transparent_black()
                })
                .flex()
                .items_center()
                .justify_center()
                .child(crate::icons::icon(
                    crate::icons::Icon::Check,
                    10.0,
                    theme.on_accent,
                ))
                .into_any_element()
        } else if is_current {
            crate::widgets::equalizer(&theme, true)
        } else {
            div()
                .text_size(px(10.5))
                .text_color(theme.text_faint)
                .child(row.number.clone())
                .into_any_element()
        };
        let mut subtitle = match list {
            QueueList::Search => row.subtitle_search.clone(),
            _ => row.subtitle_default.clone(),
        };
        if let Some(source) = &style.source_label {
            subtitle = SharedString::from(format!("{subtitle}  ·  {source}"));
        }
        if style.unavailable {
            subtitle = SharedString::from(format!("{subtitle}  ·  无法拉取"));
        } else if style.local_substitute {
            subtitle = SharedString::from(format!("{subtitle}  ·  本地替代"));
        }
        // 只读取当前可见行的缓存图片；未加载/无封面时回落到音符占位。
        let cover_image = row.cover_key.as_ref().and_then(|key| {
            track_covers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .images
                .get(key)
                .cloned()
        });

        // 队列列表的序号是拖拽把手；其它列表没有排序语义。
        let mut leading = div()
            .w(px(18.0))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(number);
        if list == QueueList::Queue && !select_mode {
            leading =
                leading
                    .cursor(CursorStyle::OpenHand)
                    .on_mouse_down(gpui::MouseButton::Left, {
                        let weak = weak.clone();
                        move |event: &MouseDownEvent, _, cx: &mut App| {
                            cx.stop_propagation();
                            let _ = weak.update(cx, |app, cx| {
                                app.begin_queue_drag(index, f32::from(event.position.y), cx);
                            });
                        }
                    });
        }

        let more = (!select_mode && style.show_menu).then(|| {
            let weak = weak.clone();
            let list = list.clone();
            div()
                .id(gpui::ElementId::named_usize("track-more", index))
                .size(px(24.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .opacity(0.55)
                .hover(|s| s.bg(theme.surface_alt).opacity(1.0))
                .child(crate::icons::icon(
                    crate::icons::Icon::More,
                    13.0,
                    theme.text_faint,
                ))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    move |event: &MouseDownEvent, _, cx: &mut App| {
                        cx.stop_propagation();
                        let _ = weak.update(cx, |app, cx| {
                            app.open_track_menu(
                                list.clone(),
                                index,
                                f32::from(event.position.x),
                                f32::from(event.position.y),
                                cx,
                            );
                        });
                    },
                )
                .into_any_element()
        });
        let add = (phone && !select_mode && !style.compact).then(|| {
            let weak = weak.clone();
            let track = tracks.get(index).cloned();
            div()
                .id(gpui::ElementId::named_usize("track-add", index))
                .size(px(24.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .opacity(0.55)
                .hover(|s| s.bg(theme.surface_alt).opacity(1.0))
                .child(crate::icons::icon(
                    crate::icons::Icon::Plus,
                    13.0,
                    theme.text_faint,
                ))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    if let Some(track) = track.clone() {
                        let _ = weak.update(cx, |app, cx| app.queue_append(track, cx));
                    }
                })
                .into_any_element()
        });

        div()
            .id(gpui::ElementId::named_usize("track", index))
            .w_full()
            .h(px(style.height))
            .px(px(if style.compact { 8.0 } else { 12.0 }))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(7.0))
            .bg(if selected {
                theme.selection
            } else if is_current {
                theme.selection
            } else if drag_target {
                theme.surface_alt
            } else if drag_source {
                theme.hover
            } else {
                gpui::transparent_black()
            })
            .opacity(if drag_source { 0.55 } else { 1.0 })
            .hover(|s| s.bg(theme.hover))
            .on_mouse_down(gpui::MouseButton::Left, {
                let weak = weak.clone();
                let list = list.clone();
                move |event: &MouseDownEvent, _, cx: &mut App| {
                    if phone && !select_mode {
                        let _ = weak.update(cx, |app, cx| {
                            app.start_long_press(
                                list.clone(),
                                index,
                                f32::from(event.position.x),
                                f32::from(event.position.y),
                                cx,
                            );
                        });
                    }
                }
            })
            .on_mouse_up(gpui::MouseButton::Left, {
                let weak = weak.clone();
                move |_: &MouseUpEvent, _, cx: &mut App| {
                    let _ = weak.update(cx, |app, _| app.cancel_long_press());
                }
            })
            .on_mouse_move({
                let weak = weak.clone();
                move |event: &MouseMoveEvent, _, cx: &mut App| {
                    if event.pressed_button == Some(gpui::MouseButton::Left) {
                        let _ = weak.update(cx, |app, _| {
                            app.cancel_long_press_on_move(
                                f32::from(event.position.x),
                                f32::from(event.position.y),
                            );
                        });
                    }
                }
            })
            .on_mouse_down(gpui::MouseButton::Right, {
                let weak = weak.clone();
                let list = list.clone();
                move |event: &MouseDownEvent, _, cx: &mut App| {
                    let _ = weak.update(cx, |app, cx| {
                        app.cancel_long_press();
                        app.open_track_menu(
                            list.clone(),
                            index,
                            f32::from(event.position.x),
                            f32::from(event.position.y),
                            cx,
                        );
                    });
                }
            })
            .on_click({
                let list = list.clone();
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        if app.row_click_suppressed() {
                            return;
                        }
                        if app.selection.is_some() {
                            app.toggle_selection(index, cx);
                        } else {
                            app.play_from_list(&list, index, cx);
                        }
                    });
                }
            })
            .child(leading)
            .child(crate::widgets::cover_thumb(
                theme,
                cover_image,
                style.cover,
                1.0,
                crate::icons::icon(
                    crate::icons::Icon::Note,
                    14.0,
                    if is_current {
                        theme.text_dim
                    } else {
                        theme.text_faint
                    },
                )
                .into_any_element(),
            ))
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
                            .font_weight(if is_current {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(row.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(subtitle),
                    ),
            )
            .when(!select_mode && style.show_duration, |d| {
                d.child(
                    div()
                        .w(px(40.0))
                        .text_size(px(10.5))
                        .text_color(theme.text_faint)
                        .text_align(gpui::TextAlign::Right)
                        .child(row.duration.clone()),
                )
            })
            .children(trailing)
            .children(add)
            .children(more)
            .into_any_element()
    }

    pub(super) fn render_track_header(theme: &Theme, phone: bool) -> AnyElement {
        div()
            .w_full()
            .h(px(26.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .border_b_1()
            .border_color(theme.border)
            .child(div().w(px(18.0)))
            .child(div().w(px(34.0)))
            .child(
                div()
                    .flex_1()
                    .text_size(px(10.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text_faint)
                    .child("标题"),
            )
            .child(
                div()
                    .w(px(40.0))
                    .text_size(px(10.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text_faint)
                    .text_align(gpui::TextAlign::Right)
                    .child("时长"),
            )
            // 行尾的「+ / ⋮」入口占位，保证表头与行内容对齐。
            .child(div().w(px(if phone { 56.0 } else { 28.0 })))
            .into_any_element()
    }

    pub(super) fn render_track_table(
        &self,
        theme: &Theme,
        tracks: Arc<Vec<Track>>,
        list: QueueList,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let n = tracks.len();
        let queue_pos = self.queue_pos;
        let queue = self.queue.clone();
        let current_id = queue_pos.and_then(|i| queue.get(i).map(|t| t.id.clone()));
        let rows = {
            let mut cache = self.row_text_cache.borrow_mut();
            match cache.as_ref() {
                Some((cached_tracks, rows)) if Arc::ptr_eq(cached_tracks, &tracks) => rows.clone(),
                _ => {
                    let rows = Arc::new(
                        tracks
                            .iter()
                            .enumerate()
                            .map(|(index, track)| TrackRowText::new(index, track))
                            .collect::<Vec<_>>(),
                    );
                    *cache = Some((tracks.clone(), rows.clone()));
                    rows
                }
            }
        };
        let weak = cx.entity().downgrade();
        let phone = self.is_phone();
        let select_mode = self.selection.as_ref().is_some_and(|s| s.list == list);
        let selection = self
            .selection
            .as_ref()
            .filter(|s| s.list == list)
            .map(|s| Arc::new(s.selected.clone()));
        let drag = self.queue_drag.filter(|_| list == QueueList::Queue);
        let tracks_arc = tracks.clone();
        let row_theme = theme.clone();
        let covers = self.track_covers.clone();
        let cover_weak = weak.clone();
        let cover_tracks = tracks_arc.clone();
        let mut body = uniform_list("track-list", n, move |range, _window, cx| {
            let visible: Vec<usize> = range.collect();
            // 只为真正滚到屏上的行启动下载：列表可能有几千首曲目。
            let _ = cover_weak.update(cx, |app, cx| {
                app.request_track_covers(&visible, &cover_tracks, cx);
            });
            visible
                .into_iter()
                .filter_map(|i| {
                    rows.get(i).map(|row| {
                        let selected = selection
                            .as_ref()
                            .and_then(|chosen| chosen.get(i).copied())
                            .unwrap_or(false);
                        Self::render_track_row(
                            &row_theme,
                            i,
                            row,
                            &covers,
                            list.clone(),
                            queue_pos,
                            current_id.as_ref(),
                            weak.clone(),
                            tracks_arc.clone(),
                            phone,
                            select_mode,
                            selected,
                            drag.map(|d| d.from),
                            drag.map(|d| d.to),
                            &TrackRowStyle::full(),
                            None,
                        )
                    })
                })
                .collect::<Vec<_>>()
        });
        // 偏移存在 App 的句柄里：沉浸层会卸载整个主页面，若只靠元素状态
        // 保存滚动位置，退出沉浸后列表会回到顶部。
        body = body.track_scroll(self.track_list_scroll.clone());
        if self.smooth_scroll {
            body = body.smooth_scroll();
        }
        let body: AnyElement = body.flex_1().min_h_0().into_any_element();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(Self::render_track_header(theme, self.is_phone()))
            .child(body)
            .into_any_element()
    }
}
