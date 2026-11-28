//! `ChordialApp` 的 render_now_playing 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn now_playing_text(&self) -> Arc<NowPlayingText> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Some(track) = &self.current_track {
            track.title.hash(&mut hasher);
            for artist in &track.artists {
                artist.name.hash(&mut hasher);
            }
            if let Some(album) = &track.album {
                album.name.hash(&mut hasher);
            }
            track.quality.hash(&mut hasher);
            match &track.origin {
                TrackOrigin::Local { .. } => 0u8.hash(&mut hasher),
                TrackOrigin::Remote { source } => {
                    1u8.hash(&mut hasher);
                    source.0.hash(&mut hasher);
                }
            }
        }
        self.player_status.title.hash(&mut hasher);
        let key = hasher.finish();
        if let Some((cached, text)) = self.now_playing_text.borrow().as_ref()
            && *cached == key
        {
            return text.clone();
        }
        let text = Arc::new(NowPlayingText::new(
            self.current_track.as_ref(),
            self.player_status.title.as_deref(),
        ));
        *self.now_playing_text.borrow_mut() = Some((key, text.clone()));
        text
    }

    pub(super) fn render_now_playing(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let playing = self.player_status.state == PlayerState::Playing;
        let has_track = self.current_track.is_some();
        let text = self.now_playing_text();
        let title = text.title.clone();
        let artists = text.artists.clone();
        let album = text.album.clone();
        let source_label = text.source_label.clone();
        let quality = text.quality.clone();

        let panel_w = (self.viewport_width - self.sidebar_width() - self.list_width()).max(360.0);
        let content_w = (panel_w - 56.0).clamp(240.0, 440.0);
        let compact_h = self.viewport_height < 780.0;
        let art = (self.viewport_height * 0.28).clamp(160.0, 260.0);
        let info_mt = if compact_h { 14.0 } else { 20.0 };
        let lyrics_mt = if compact_h { 14.0 } else { 24.0 };
        let lyrics_pt = if compact_h { 10.0 } else { 14.0 };
        let preview_count = if compact_h { 5 } else { 7 };

        let artwork: AnyElement = {
            let placeholder_bg = if theme.is_dark() {
                linear_gradient(
                    135.0,
                    linear_color_stop(crate::theme::hex(0x2C2C33), 0.0),
                    linear_color_stop(crate::theme::hex(0x131316), 1.0),
                )
            } else {
                linear_gradient(
                    135.0,
                    linear_color_stop(crate::theme::hex(0xF2F2F5), 0.0),
                    linear_color_stop(crate::theme::hex(0xD8D8DF), 1.0),
                )
            };
            let placeholder = div()
                .size_full()
                .bg(placeholder_bg)
                .flex()
                .items_center()
                .justify_center()
                .child(crate::icons::icon(
                    crate::icons::Icon::Note,
                    art * 0.26,
                    theme.text_faint,
                ))
                .into_any_element();
            let cover = crate::widgets::cover_plain(
                &theme,
                self.cover_image.clone(),
                art,
                1.0,
                placeholder,
            );
            let wrapper = div()
                .rounded(px(1.0))
                .shadow(vec![BoxShadow {
                    color: theme.shadow_strong,
                    offset: point(px(0.0), px(14.0)),
                    blur_radius: px(30.0),
                    spread_radius: px(0.0),
                }])
                .child(cover);
            if has_track {
                let weak = weak.clone();
                wrapper
                    .id("hero-cover")
                    .cursor_pointer()
                    .on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.open_portrait(cx);
                            window.focus(&app.focus_handle);
                            cx.notify();
                        });
                    })
                    .into_any_element()
            } else {
                wrapper.into_any_element()
            }
        };

        let artist_line = if has_track {
            if artists.is_empty() {
                SharedString::from("未知艺人")
            } else {
                artists
            }
        } else {
            SharedString::from("从曲库或搜索结果中选择一首歌")
        };

        let pill = |text: SharedString| {
            div()
                .h(px(20.0))
                .px(px(7.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface_alt)
                .flex()
                .items_center()
                .text_size(px(9.5))
                .text_color(theme.text_faint)
                .child(text)
                .into_any_element()
        };

        let mut info = div()
            .w(px(content_w))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(3.0))
            .mt(px(info_mt))
            .child(
                div()
                    .w_full()
                    .text_size(px(21.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .text_ellipsis()
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(13.0))
                    .text_color(theme.text_dim)
                    .text_ellipsis()
                    .child(SharedString::from(artist_line)),
            );
        if !album.is_empty() {
            info = info.child(
                div()
                    .w_full()
                    .text_size(px(10.5))
                    .text_color(theme.text_faint)
                    .text_ellipsis()
                    .child(album.clone()),
            );
        }
        if has_track {
            let mut meta = div().flex().items_center().gap(px(6.0)).mt(px(9.0));
            if !source_label.is_empty() {
                meta = meta.child(pill(source_label.clone()));
            }
            if !quality.is_empty() {
                meta = meta.child(pill(quality.clone()));
            }
            info = info.child(meta);

            let album_link = self
                .current_track
                .as_ref()
                .and_then(|t| t.album.as_ref())
                .and_then(|a| a.id.as_ref())
                .is_some();
            let artist_link = self
                .current_track
                .as_ref()
                .is_some_and(|t| t.artists.iter().any(|a| a.id.is_some()));
            if album_link || artist_link {
                let mut links = div().flex().items_center().gap(px(6.0)).mt(px(3.0));
                if album_link {
                    let weak = weak.clone();
                    links = links.child(
                        div()
                            .id("goto-album")
                            .h(px(22.0))
                            .px(px(7.0))
                            .rounded(px(5.0))
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(px(10.0))
                            .text_color(theme.text_dim)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Layers,
                                11.0,
                                theme.text_faint,
                            ))
                            .child("专辑")
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.open_album_for_current(cx);
                                });
                            }),
                    );
                }
                if artist_link {
                    let weak = weak.clone();
                    links = links.child(
                        div()
                            .id("goto-artist")
                            .h(px(22.0))
                            .px(px(7.0))
                            .rounded(px(5.0))
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(px(10.0))
                            .text_color(theme.text_dim)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Note,
                                11.0,
                                theme.text_faint,
                            ))
                            .child("歌手")
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.open_artist_for_current(cx);
                                });
                            }),
                    );
                }
                info = info.child(links);
            }
        } else {
            let weak_play = weak.clone();
            let weak_search = weak.clone();
            let weak_source = weak.clone();
            let mut actions = div()
                .w(px(content_w))
                .flex()
                .flex_col()
                .items_start()
                .gap(px(8.0))
                .mt(px(18.0));
            if !self.library.is_empty() {
                actions = actions.child(crate::widgets::text_button(
                    &theme,
                    "播放全部",
                    true,
                    move |_, _, cx| {
                        let _ = weak_play.update(cx, |app, cx| {
                            app.play_from_list(QueueList::Library, 0, cx);
                        });
                    },
                ));
            } else {
                actions = actions.child(crate::widgets::text_button(
                    &theme,
                    "添加音乐",
                    true,
                    move |_, _, cx| {
                        let _ = weak_source.update(cx, |app, cx| app.open_source_manager(cx));
                    },
                ));
            }
            actions = actions.child(crate::widgets::text_button(
                &theme,
                "搜索歌曲",
                false,
                move |_, window, cx| {
                    let _ = weak_search.update(cx, |app, cx| {
                        app.view_tab = ViewTab::Search;
                        let input = app.search_input.clone();
                        cx.defer_in(window, move |_, window, cx| {
                            focus_input(&input, window, cx);
                        });
                        cx.notify();
                    });
                },
            ));
            info = info.child(actions);
        }

        // 歌词预览：跟随播放位置，当前行高亮。
        let position = self.player_status.position_ms;
        let preview = self.lyric_preview.as_ref();
        let current_idx = if preview.is_empty() {
            None
        } else {
            Some(
                preview
                    .partition_point(|l| l.start_ms <= position)
                    .saturating_sub(1),
            )
        };
        let start = current_idx.map(|i| i.saturating_sub(2)).unwrap_or(0);
        let end = (start + preview_count).min(preview.len());
        let mut lines = div().w(px(content_w)).flex().flex_col().gap(px(1.0));
        if preview.is_empty() {
            lines = lines.child(
                div()
                    .py(px(10.0))
                    .text_size(px(11.5))
                    .line_height(px(17.0))
                    .text_color(theme.text_faint)
                    .child(SharedString::from(
                        self.lyrics_status.clone().unwrap_or_else(|| {
                            if has_track {
                                "暂无歌词".to_string()
                            } else {
                                "播放曲目后在这里同步显示歌词".to_string()
                            }
                        }),
                    )),
            );
        } else {
            for i in start..end {
                let line = &preview[i];
                let active = current_idx == Some(i) && position > 0;
                let (main, sub) = if line.text.is_empty() {
                    (line.translation.clone(), SharedString::default())
                } else {
                    (line.text.clone(), line.translation.clone())
                };
                lines = lines.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .py(px(3.0))
                        .child(
                            div()
                                .text_size(px(if active { 13.5 } else { 12.0 }))
                                .font_weight(if active {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(if active { theme.text } else { theme.text_faint })
                                .text_ellipsis()
                                .child(main),
                        )
                        .when(!sub.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(theme.text_faint)
                                    .text_ellipsis()
                                    .child(sub),
                            )
                        }),
                );
            }
        }

        let immersive_btn = {
            let weak = weak.clone();
            div()
                .id("open-immersive")
                .h(px(24.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .text_size(px(10.5))
                .text_color(theme.text_dim)
                .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                .child(crate::icons::icon(
                    crate::icons::Icon::Expand,
                    11.0,
                    theme.text_faint,
                ))
                .child("沉浸")
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.force_portrait = false;
                        app.enter_immersive(cx);
                        window.focus(&app.focus_handle);
                        cx.notify();
                    });
                })
        };

        let lyrics_block = div()
            .w(px(content_w))
            .mt(px(lyrics_mt))
            .pt(px(lyrics_pt))
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(6.0))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text_faint)
                            .child("歌词"),
                    )
                    .when(has_track, |d| d.child(immersive_btn)),
            )
            .child(lines);

        div()
            .flex_1()
            .min_w(px(320.0))
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .border_l_1()
            .border_color(theme.border)
            .child(
                div()
                    .h(px(52.0))
                    .px(px(22.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("正在播放"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(crate::widgets::equalizer(&theme, playing))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(
                                        if matches!(
                                            self.player_status.state,
                                            PlayerState::Failed(_)
                                        ) {
                                            theme.danger
                                        } else {
                                            theme.text_faint
                                        },
                                    )
                                    .child(self.playback_status_label()),
                            ),
                    ),
            )
            .child(
                div()
                    .id("now-scroll")
                    .flex_1()
                    .min_h_0()
                    .track_scroll(&self.now_scroll)
                    .overflow_y_scroll()
                    .when(self.smooth_scroll, |d| d.smooth_scroll())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .px(px(28.0))
                            .pt(px(18.0))
                            .pb(px(crate::theme::size::DOCK
                                + crate::theme::size::DOCK_BOTTOM
                                + 14.0))
                            .child(artwork)
                            .child(info)
                            .child(lyrics_block),
                    ),
            )
            .into_any_element()
    }
}
