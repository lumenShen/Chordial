//! `ChordialApp` 的 render_dock 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn render_dock(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let status = &self.player_status;
        let text = self.now_playing_text();
        let title = text.title.clone();
        let playing = status.state == PlayerState::Playing;
        let duration = status.duration_ms.unwrap_or(0);
        let position = status.position_ms.min(duration.max(1));
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };
        let artist = if self.current_track.is_some() {
            if !text.artists.is_empty() {
                text.artists.clone()
            } else if !text.album.is_empty() {
                text.album.clone()
            } else {
                SharedString::from("未知艺人")
            }
        } else {
            SharedString::from("选择一首歌开始")
        };

        let width = self.dock_width();
        let left_x = ((self.viewport_width - width) / 2.0).max(8.0);
        let show_volume = self.viewport_width >= 1120.0;
        let compact = width < 720.0;

        let cover = crate::widgets::cover_thumb(
            &theme,
            self.cover_image.clone(),
            42.0,
            7.0,
            crate::icons::icon(crate::icons::Icon::Note, 16.0, theme.text_faint),
        );
        let open_immersive = {
            let weak = weak.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |app, cx| {
                    app.open_portrait(cx);
                    window.focus(&app.focus_handle);
                    cx.notify();
                });
            }
        };
        let left = div()
            .id("dock-track")
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(10.0))
            .w(px(if compact { 180.0 } else { 232.0 }))
            .min_w_0()
            .child(cover)
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
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(if self.current_track.is_some() {
                                theme.text
                            } else {
                                theme.text_dim
                            })
                            .text_ellipsis()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(artist),
                    ),
            )
            .on_click(open_immersive);

        let prev = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Prev, 14.0, theme.text_dim),
                28.0,
                "上一首",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_prev(cx));
                },
            )
        };
        let next = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Next, 14.0, theme.text_dim),
                28.0,
                "下一首",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_next(cx));
                },
            )
        };
        let play_icon = if playing {
            crate::icons::icon(crate::icons::Icon::Pause, 15.0, theme.on_accent)
        } else {
            crate::icons::icon(crate::icons::Icon::Play, 15.0, theme.on_accent)
        };
        let play = {
            let weak = weak.clone();
            div()
                .id("dock-play")
                .size(px(36.0))
                .rounded(px(18.0))
                .bg(theme.accent)
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(theme.accent_hover))
                .child(play_icon)
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_playback(cx));
                })
        };
        let transport = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(prev)
            .child(play)
            .child(next);

        let seek = {
            let player = self.player.clone();
            move |fraction: f32| {
                if duration > 0 {
                    let target = (fraction.clamp(0.0, 1.0) * duration as f32) as u64;
                    player.seek_ms(target);
                }
            }
        };
        let progress_slider = crate::widgets::slider(
            progress,
            crate::widgets::SliderColors {
                track: theme.border_strong,
                fill: theme.accent,
                thumb: theme.accent,
            },
            "seek",
            seek,
        );
        let progress_group = div()
            .flex_1()
            .min_w(px(120.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(36.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_dim)
                    .text_align(gpui::TextAlign::Right)
                    .child(SharedString::from(fmt_ms(position))),
            )
            .child(div().flex_1().child(progress_slider))
            .child(
                div()
                    .w(px(36.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_faint)
                    .child(SharedString::from(if duration > 0 {
                        fmt_ms(duration)
                    } else {
                        "--:--".into()
                    })),
            );

        let volume_btn = {
            let weak = weak.clone();
            let muted = status.volume <= 0.001;
            crate::widgets::icon_button_with_tooltip(
                &theme,
                crate::icons::icon(
                    if muted {
                        crate::icons::Icon::VolumeMute
                    } else {
                        crate::icons::Icon::Volume
                    },
                    15.0,
                    theme.text_dim,
                ),
                26.0,
                "dock-volume",
                if muted { "取消静音" } else { "静音" },
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, _| {
                        let v = if app.player_status.volume > 0.001 {
                            0.0
                        } else {
                            0.9
                        };
                        app.player.set_volume(v);
                        app.player_status.volume = v;
                    });
                },
            )
        };
        let volume_slider = {
            let player = self.player.clone();
            crate::widgets::slider(
                status.volume.clamp(0.0, 1.0),
                crate::widgets::SliderColors {
                    track: theme.border_strong,
                    fill: theme.text_dim,
                    thumb: theme.text,
                },
                "volume",
                move |fraction: f32| player.set_volume(fraction),
            )
        };
        let preference_btn = {
            let weak = weak.clone();
            let label = self.source_preference.label();
            div()
                .id("dock-preference")
                .h(px(26.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .cursor_pointer()
                .hover(|s| s.bg(theme.surface_alt))
                .child(crate::icons::icon(
                    crate::icons::Icon::Sliders,
                    13.0,
                    theme.text_faint,
                ))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme.text_dim)
                        .child(SharedString::from(label)),
                )
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.cycle_source_preference(cx));
                })
        };
        let download_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Download, 15.0, theme.text_dim),
                26.0,
                "下载当前曲目",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.download_current(cx));
                },
            )
        };
        let lyrics_btn = {
            let weak = weak.clone();
            let trigger = div()
                .id("dock-lyrics")
                .size(px(28.0))
                .rounded(px(7.0))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(theme.surface_alt))
                .child(crate::icons::icon(
                    crate::icons::Icon::Lyrics,
                    16.0,
                    theme.text_dim,
                ))
                .into_any_element();
            crate::bubble::bubble_button(
                crate::bubble::BubbleStyle::for_theme(&theme),
                "dock-lyrics",
                "打开沉浸歌词",
                trigger,
                move |_, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.open_lyrics_immersive(cx);
                        window.focus(&app.focus_handle);
                        cx.notify();
                    });
                },
            )
        };
        let right = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(volume_btn)
            .when(show_volume, |d| {
                d.child(div().w(px(72.0)).child(volume_slider))
            })
            .child(div().w(px(1.0)).h(px(18.0)).bg(theme.border))
            .child(preference_btn)
            .child(download_btn)
            .child(div().w(px(1.0)).h(px(18.0)).bg(theme.border))
            .child(lyrics_btn);

        div()
            .absolute()
            .left(px(left_x))
            .bottom(px(crate::theme::size::DOCK_BOTTOM))
            .w(px(width))
            .h(px(crate::theme::size::DOCK))
            .rounded(px(18.0))
            .bg(theme.glass)
            .border_1()
            .border_color(theme.glass_border)
            .shadow(vec![BoxShadow {
                color: theme.shadow_strong,
                offset: point(px(0.0), px(10.0)),
                blur_radius: px(28.0),
                spread_radius: px(0.0),
            }])
            .px(px(14.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(left)
            .child(transport)
            .child(progress_group)
            .child(right)
            .into_any_element()
    }

    /// 播放线程的位置快照约 33ms 更新一次；沉浸页在两次快照之间按真实
    /// 帧间隔外推，让进度条在 300Hz 下仍然连续（上限 80ms，避免卡顿后漂移）。
    pub(super) fn sync_progress_clock(&mut self) {
        let (position, playing) = self.player.clock();
        let (last_position, last_playing, _) = self.progress_clock;
        if position != last_position || playing != last_playing {
            self.progress_clock = (position, playing, Instant::now());
        }
    }

    pub(super) fn display_position_ms(&self) -> u64 {
        let (position, playing, since) = self.progress_clock;
        if playing {
            position.saturating_add(since.elapsed().as_millis().min(80) as u64)
        } else {
            position
        }
    }
}
