//! `ChordialApp` 的 render_dock 行为。
//!
//! Dock 是两层的：上层负责曲目 / 传输 / 工具，下层是横贯整条 Dock 的
//! 时间轴。曲目区本身就是沉浸歌词入口，不再保留独立的歌词按钮。

use super::*;

impl ChordialApp {
    pub(super) fn render_dock(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let status = &self.player_status;
        // 与沉浸层共享舞台共用同一份文案，动画收口时不会换成另一串字。
        let (title, artist_line) = self.dock_track_text();
        let playing = status.state == PlayerState::Playing;
        let failed = matches!(status.state, PlayerState::Failed(_));
        let loading = status.state == PlayerState::Loading;
        // 未开始播放时播放器还没有 duration；退回曲目元数据，Dock 也能显示
        // 真实总时长，而不是一条空轨 + “--:--”。
        let duration = status
            .duration_ms
            .filter(|duration| *duration > 0)
            .or_else(|| {
                self.current_track
                    .as_ref()
                    .and_then(|track| track.duration_ms)
            })
            .unwrap_or(0);
        let position = status.position_ms.min(duration.max(1));
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };

        let width = self.dock_width();
        let left_x = ((self.viewport_width - width) / 2.0).max(8.0);
        let show_volume = self.viewport_width >= 1120.0;

        let cover = crate::widgets::cover_thumb(
            &theme,
            self.cover_image.clone(),
            DOCK_COVER,
            DOCK_COVER_RADIUS,
            crate::icons::icon(crate::icons::Icon::Note, 16.0, theme.text_faint),
        );
        let open_immersive = {
            let weak = weak.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |app, cx| {
                    app.open_lyrics_immersive(cx);
                    window.focus(&app.focus_handle);
                    cx.notify();
                });
            }
        };
        let track = self
            .stage_measurements
            .observe(
                StageMode::Dock,
                div()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .px(px(DOCK_TRACK_PAD_X))
                    .rounded(px(11.0))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(DOCK_COVER_GAP))
                    .hover(|s| s.bg(theme.hover))
                    .child(cover)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap(px(DOCK_LINE_GAP))
                            .child(
                                div()
                                    .text_size(px(DOCK_TITLE_SIZE))
                                    // 与共享舞台同一行高，动画终点逐像素重合。
                                    .line_height(px(DOCK_TITLE_SIZE * INFO_LINE_RATIO))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
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
                                    .min_w_0()
                                    .text_size(px(DOCK_ARTIST_SIZE))
                                    .line_height(px(DOCK_ARTIST_SIZE * INFO_LINE_RATIO))
                                    .text_color(theme.text_faint)
                                    .text_ellipsis()
                                    .child(artist_line),
                            ),
                    ),
                &[(0, MeasurePart::Cover), (1, MeasurePart::Info)],
            )
            .id("dock-track")
            .on_click(open_immersive);

        let prev = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Prev, 16.0, theme.text_dim),
                DOCK_TRANSPORT_BTN,
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
                crate::icons::icon(crate::icons::Icon::Next, 16.0, theme.text_dim),
                DOCK_TRANSPORT_BTN,
                "下一首",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_next(cx));
                },
            )
        };
        let play_icon = if playing {
            crate::icons::icon(crate::icons::Icon::Pause, 17.0, theme.on_accent)
        } else {
            crate::icons::icon(
                crate::icons::Icon::Play,
                17.0,
                if failed {
                    gpui::white()
                } else if loading {
                    theme.disabled_fg
                } else {
                    theme.on_accent
                },
            )
        };
        let play = {
            let weak = weak.clone();
            div()
                .id("dock-play")
                .size(px(DOCK_PLAY_BTN))
                .rounded(px(DOCK_PLAY_BTN * 0.5))
                .bg(if failed {
                    theme.danger
                } else if loading {
                    theme.disabled_bg
                } else {
                    theme.accent
                })
                .cursor(if loading {
                    CursorStyle::Arrow
                } else {
                    CursorStyle::PointingHand
                })
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| {
                    if failed {
                        s.bg(theme.danger.opacity(0.85))
                    } else if loading {
                        s
                    } else {
                        s.bg(theme.accent_hover)
                    }
                })
                .child(play_icon)
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_playback(cx));
                })
        };
        let transport = div()
            .flex_none()
            .px(px(DOCK_TRANSPORT_PAD_X))
            .flex()
            .items_center()
            .gap(px(DOCK_TRANSPORT_GAP))
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
        let progress_slider = crate::widgets::timeline_slider(
            progress,
            crate::widgets::SliderColors {
                track: theme.text_faint.opacity(0.42),
                fill: theme.accent,
                thumb: theme.accent,
            },
            theme.surface,
            "dock-seek",
            seek,
        );

        let time_label = |text: String, color: gpui::Hsla, align_right: bool| {
            div()
                .w(px(38.0))
                .flex_none()
                .text_size(px(10.0))
                .text_color(color)
                .when(align_right, |d| d.text_align(gpui::TextAlign::Right))
                .child(SharedString::from(text))
        };
        let timeline = div()
            .h(px(DOCK_TIMELINE_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(9.0))
            .child(time_label(fmt_ms(position), theme.text_dim, true))
            .child(div().flex_1().min_w_0().child(progress_slider))
            .child(time_label(
                if duration > 0 {
                    fmt_ms(duration)
                } else {
                    "--:--".into()
                },
                theme.text_faint,
                false,
            ));

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
                    16.0,
                    theme.text_dim,
                ),
                28.0,
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
                "dock-volume-slider",
                move |fraction: f32| player.set_volume(fraction),
            )
        };

        let queue_btn = Self::dock_menu_button(
            &theme,
            weak.clone(),
            crate::icons::Icon::Queue,
            "dock-queue",
            "播放队列",
            DockMenu::Queue,
            self.dock_menu == Some(DockMenu::Queue),
        );
        let more_btn = Self::dock_menu_button(
            &theme,
            weak.clone(),
            crate::icons::Icon::More,
            "dock-more",
            "更多",
            DockMenu::More,
            self.dock_menu == Some(DockMenu::More),
        );

        let tools = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(2.0))
            .child(volume_btn)
            .when(show_volume, |d| {
                d.child(div().w(px(66.0)).h(px(16.0)).child(volume_slider))
            })
            .child(
                div()
                    .w(px(1.0))
                    .h(px(17.0))
                    .mx(px(6.0))
                    .flex_none()
                    .bg(theme.border),
            )
            .child(queue_btn)
            .child(more_btn);

        let upper = div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .gap(px(DOCK_UPPER_GAP))
            .child(track)
            .child(transport)
            .child(tools);

        div()
            .absolute()
            // Dock 的整块玻璃表面都是命中边界；空白/内边距区域不能穿透到
            // 下面的曲目行或右栏按钮。occlude 会在 Dock hover 时注册一个
            // BlockMouse hitbox，子控件仍按正常冒泡/点击处理。
            .occlude()
            .left(px(left_x))
            .bottom(px(crate::theme::size::DOCK_BOTTOM))
            .w(px(width))
            .h(px(crate::theme::size::DOCK))
            .rounded(px(22.0))
            .bg(theme.glass)
            .border_1()
            .border_color(theme.glass_border)
            .shadow(vec![BoxShadow {
                color: theme.shadow_strong,
                offset: point(px(0.0), px(12.0)),
                blur_radius: px(32.0),
                spread_radius: px(0.0),
            }])
            .px(px(DOCK_PAD_X))
            .pt(px(DOCK_PAD_TOP))
            .pb(px(DOCK_PAD_BOTTOM))
            .flex()
            .flex_col()
            .gap(px(DOCK_GAP))
            .child(upper)
            .child(timeline)
            .into_any_element()
    }

    /// Dock 曲目区与沉浸层共享舞台共用的文案。
    ///
    /// 展开/收起动画收口时卡片会盖住真实 Dock；两端文案不同就会在最后一帧
    /// 看到一次文字跳变，因此标题与副标题都从这里取。
    pub(super) fn dock_track_text(&self) -> (SharedString, SharedString) {
        let text = self.now_playing_text();
        let title = text.title.clone();
        if matches!(self.player_status.state, PlayerState::Failed(_)) {
            return (title, SharedString::from("播放失败 · 点击重试"));
        }
        if self.player_status.state == PlayerState::Loading {
            return (title, SharedString::from("正在加载…"));
        }
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
        let source = text.source_label.clone();
        // 艺人 + 来源合成一个可省略的文本行。分开布局时来源会被 flex_1
        // 推到 Dock 中部，形成难看的“空白峡谷”；这里保持单一基线。
        let artist_line = if self.current_track.is_some() && !source.is_empty() {
            SharedString::from(format!("{artist} · {source}"))
        } else {
            artist
        };
        (title, artist_line)
    }

    fn dock_menu_button(
        theme: &Theme,
        weak: gpui::WeakEntity<ChordialApp>,
        icon: crate::icons::Icon,
        id: &'static str,
        tooltip: &'static str,
        menu: DockMenu,
        active: bool,
    ) -> AnyElement {
        let trigger = div()
            .id(SharedString::from(format!("dock-menu-trigger-{id}")))
            .size(px(30.0))
            .rounded(px(7.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(active, |d| d.bg(theme.hover))
            .hover(|s| s.bg(theme.hover))
            .child(crate::icons::icon(icon, 17.0, theme.text_dim))
            .into_any_element();
        if active {
            // 菜单已打开时不再保留悬停气泡，否则气泡会压在菜单右上角。
            return trigger;
        }
        crate::bubble::bubble_button(
            crate::bubble::BubbleStyle::for_theme(theme),
            id,
            tooltip,
            trigger,
            move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.toggle_dock_menu(menu, cx));
            },
        )
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
