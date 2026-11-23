//! `ChordialApp` 的 render_immersive 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn render_immersive(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let bounds = window.bounds();
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let now = Instant::now();
        self.sync_progress_clock();

        // Dock→全屏共享布局：p = 1 时页面完全贴在 Dock 上，p = 0 时满窗。
        // 根容器保持满窗（输入命中区），真正缩放的是这张卡片。
        let p = self.immersive_dock_progress(now, height);
        let full = StageRect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        };
        let card = full.lerp(dock_rect(width, height), p);
        let card_w = card.w.max(1.0);
        let card_h = card.h.max(1.0);
        // 页面自身的控件在最后 1/3 的收缩距离内淡出（共享封面/曲名仍然可见）。
        let content_opacity = if p < 0.65 {
            1.0
        } else {
            ((1.0 - p) / 0.2).clamp(0.0, 1.0)
        };
        // 页面自带的 Dock 更早淡出：真实 Dock 就在卡片背后，晚淡出会
        // 看到两层图标叠加（黑边/重影）。
        let dock_opacity = (1.0 - p * 2.5).clamp(0.0, 1.0);
        // 靠近 Dock 时把卡片配色过渡回 Dock 的玻璃底与主题文字。这样
        // p = 1 的卡片与真实 Dock 视觉一致，退出/进入都不会闪一下明暗。
        let theme = Theme::get(cx).clone();
        let dock_blend = ((p - 0.72) / 0.28).clamp(0.0, 1.0);
        let clear_black = gpui::Hsla {
            a: 0.0,
            ..gpui::black()
        };
        let dock_glass = overlay_color(theme.bg, theme.glass);
        let dock_title = if self.current_track.is_some() {
            theme.text
        } else {
            theme.text_dim
        };
        // 卡片自身必须保持不透明，否则会透出下方真实 Dock 的文字/图标。
        let card_bg = overlay_color(theme.bg, mix_color(gpui::black(), dock_glass, dock_blend));
        let card_border = mix_color(clear_black, theme.glass_border, dock_blend);
        let title_color = mix_color(gpui::white().opacity(0.9), dock_title, dock_blend);
        let artist_color = mix_color(gpui::white().opacity(0.45), theme.text_faint, dock_blend);

        // 背景铺满卡片；光栅分辨率 = 卡片区域 ÷ 设置里的除数。
        let bg_scale = self.background_render_scale();
        self.lyric_view.update(cx, |view, _| {
            // 背景按窗口绝对坐标绘制，必须带上卡片原点；否则卡片下移时
            // 背景会被祖先裁掉一截，底部露出卡片的黑色底，形成“黑边”。
            view.set_background_bounds(Some(Bounds::new(
                point(px(card.x), px(card.y)),
                size(px(card_w), px(card_h)),
            )));
            view.set_background_render_scale(bg_scale);
        });

        let dock_shared = p > 0.001;
        // 布局 1 只在「横向窗口 + 歌词可见 + 播放页」时启用；其余情况
        // （竖屏、无歌词、主动收起歌词、队列子页）统一落到布局 2。
        let wide_layout =
            self.portrait_page == PortraitPage::Art && self.layout_one_active(cx, width, height);
        let page = if wide_layout {
            self.render_immersive_wide(card_w, card_h, dock_shared, dock_opacity, window, cx)
        } else {
            self.render_immersive_compact(card_w, card_h, dock_shared, dock_opacity, window, cx)
        };

        let mut card_el = div()
            .id("immersive-card")
            .absolute()
            .left(px(card.x))
            .top(px(card.y))
            .w(px(card_w))
            .h(px(card_h));
        // 满屏时不要圆角/边框/底色，避免在窗口边缘留下黑边或裁切；
        // 只有在 Dock↔全屏过渡期间卡片才需要这些装饰。
        if p > 0.0001 {
            card_el = card_el
                .overflow_hidden()
                .rounded(px(18.0))
                .border_1()
                .border_color(card_border)
                .bg(card_bg);
        }
        card_el = card_el.child(
            div()
                .absolute()
                .inset_0()
                .opacity(content_opacity)
                .child(page),
        );

        // 共享舞台（封面/曲名/艺人）独立于页面缩放，从 Dock 内的小元素
        // 连续飞到沉浸页对应位置；坐标从窗口空间平移进卡片空间。
        if dock_shared {
            let mode = self.immersive_stage_mode(cx, width, height);
            let page_geometry = immersive_geometry(mode, width, height);
            let dock_stage = dock_geometry(width, height);
            let shared = page_geometry
                .lerp(dock_stage, p)
                .translated(-card.x, -card.y);
            card_el =
                card_el.child(self.render_shared_stage_colored(shared, title_color, artist_color));
        }

        // 「更多」抽屉画在页面与共享舞台之上；布局 1 / 布局 2 共用。
        if self.drawer_visible() {
            card_el = card_el.child(self.render_drawer(card_w, card_h, now, cx));
        }

        let mut root = div()
            .id("immersive-root")
            .size_full()
            .relative()
            .child(card_el)
            .children(Self::resize_handles());

        // 拖拽期间卡片会缩小、把手不再跟随指针；用一张满窗捕获层接管
        // 后续移动与松手，保证手势连续。
        if self.sheet_drag.is_some() {
            root = root.child(
                div()
                    .id("immersive-drag-capture")
                    .absolute()
                    .inset_0()
                    .cursor(CursorStyle::ClosedHand)
                    .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(gpui::MouseButton::Left) {
                            app.update_sheet_drag(f32::from(event.position.y), cx);
                        } else if app.sheet_drag.is_some_and(|drag| !drag.animating()) {
                            app.release_sheet_drag(height, cx);
                        }
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(move |app, _: &MouseUpEvent, _, cx| {
                            if app.sheet_drag.is_some() {
                                app.release_sheet_drag(height, cx);
                            }
                        }),
                    ),
            );
        }
        root.into_any_element()
    }

    /// 窄窗沉浸：AMLL react-full `VerticalLayout` 的等价几何（宽 < 高）。
    ///
    /// 800×1000 实测：行 drag 30 / thumb 30 / main 1fr（gap 2）；
    /// 小封面 6em=96（宽 ≤ 480 时 4.5em），x = 3em，y = main_top + 1em；
    /// 曲目信息在封面右侧垂直居中（16px：name 500/op.9、artists op.45），
    /// 菜单钮 35×35 贴右缘 3em；歌词铺满其余高度并对齐 0.1/top，
    /// mask 只在顶部 10% 渐隐；歌词打开时不显示传输控件（与 AMLL 竖向一致）。
    // ---- AMLL 播放器控件（横向/竖向共用同一套写法） ----

    /// AMLL BouncingSlider 进度条 + 时间/音质标签行。
    ///
    /// 右侧时间默认显示剩余时长，点一下切到总时长（AMLL
    /// `showRemainingTimeAtom` 的交互）。音质标签是 20% 白色圆角胶囊，
    /// 点击打开解码信息弹窗。
    pub(super) fn amll_progress_block(
        &self,
        width: f32,
        label_font: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let status = &self.player_status;
        let duration = status.duration_ms.unwrap_or(0);
        let position = self.display_position_ms().min(duration.max(1));
        let remaining = duration.saturating_sub(position);
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };
        let quality = self
            .current_track
            .as_ref()
            .and_then(|t| t.quality.clone())
            .unwrap_or_default();
        let seek = {
            let player = self.player.clone();
            move |fraction: f32, _cx: &mut App| {
                if duration > 0 {
                    player.seek_ms((fraction.clamp(0.0, 1.0) * duration as f32) as u64);
                }
            }
        };
        let progress_bar = crate::widgets::amll_slider(progress, "seek", seek);

        // AMLL `.qualityTag`：高 ≤1080 时 scale=1，否则 1.4。
        let tag_scale = if height > 1080.0 { 1.4 } else { 1.0 };
        let quality_pill = if quality.is_empty() {
            div().into_any_element()
        } else {
            let weak = weak.clone();
            div()
                .id("amll-quality-tag")
                .flex()
                .items_center()
                .rounded(px(5.0 * tag_scale))
                .bg(white.opacity(0.2))
                .pl(px(1.1 * tag_scale))
                .pr(px(5.72 * tag_scale))
                .py(px(2.0 * tag_scale))
                .text_size(px(11.0 * tag_scale))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(white)
                .cursor_pointer()
                // AMLL whileHover/whileTap：scale 0.95 / 0.9。
                .hover(|s| s.bg(white.opacity(0.26)))
                .child(SharedString::from(quality.to_uppercase()))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.quality_dialog_open = true;
                        cx.notify();
                    });
                })
                .into_any_element()
        };

        let right_label = {
            let weak = weak.clone();
            let text = if self.show_remaining_time {
                format!("-{}", fmt_ms(remaining))
            } else {
                fmt_ms(duration)
            };
            div()
                .id("amll-time-toggle")
                .flex_1()
                .flex()
                .justify_end()
                .cursor_pointer()
                .child(SharedString::from(text))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.show_remaining_time = !app.show_remaining_time;
                        cx.notify();
                    });
                })
        };

        div()
            .w(px(width))
            .flex()
            .flex_col()
            .child(div().h(px(24.0)).child(progress_bar))
            .child(
                div()
                    .w_full()
                    .mt(px(4.0))
                    .flex()
                    .items_center()
                    .text_size(px(label_font))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(white.opacity(0.5))
                    .child(div().flex_1().child(SharedString::from(fmt_ms(position))))
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(quality_pill),
                    )
                    .child(right_label),
            )
            .into_any_element()
    }

    /// AMLL MediaButton 传输行：shuffle / prev / play / next / repeat。
    pub(super) fn amll_transport(
        &self,
        width: f32,
        btn: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let playing = self.player_status.state == PlayerState::Playing;
        let press_enter = Self::anim_progress(self.ui_anim_enter, Self::UI_ANIM_PRESS_MS);
        let press_play = Self::anim_progress(
            Self::latest_anim(self.ui_anim_enter, self.ui_anim_play),
            Self::UI_ANIM_PRESS_MS,
        );
        let press_shuffle = Self::anim_progress(
            Self::latest_anim(self.ui_anim_enter, self.ui_anim_shuffle),
            Self::UI_ANIM_PRESS_MS,
        );
        let press_repeat = Self::anim_progress(
            Self::latest_anim(self.ui_anim_enter, self.ui_anim_repeat),
            Self::UI_ANIM_PRESS_MS,
        );
        let arrows = Self::anim_progress(self.ui_anim_enter, Self::UI_ANIM_ARROWS_MS);
        // AMLL `.songMediaButton > svg` / `.songMediaPlayButton > svg` 的
        // 响应式 scale。svg intrinsic size 分别是 32 与 38；我们的 16 网格
        // 已按同一 viewBox 比例归一，所以画布 = intrinsic × scale。
        let media_scale = if height > 1080.0 {
            3.0
        } else if height > 768.0 {
            2.0
        } else if height > 512.0 {
            1.5
        } else {
            1.0
        };
        let play_scale = if height > 1080.0 {
            2.0
        } else if height > 768.0 {
            1.1
        } else if height > 512.0 {
            0.8
        } else {
            0.5
        };
        let icon_side = 20.8; // 1.3em
        let icon_skip = 32.0 * media_scale;
        let icon_play = 38.0 * play_scale;
        let shuffle_active = self.play_mode == PlayMode::Shuffle;
        let repeat_active = matches!(self.play_mode, PlayMode::RepeatAll | PlayMode::RepeatOne);
        // 按钮会切换模式，气泡描述「下一次点击会发生什么」，而不是重复图标名。
        let shuffle_label = if shuffle_active {
            "关闭随机播放"
        } else {
            "开启随机播放"
        };
        let repeat_label = match self.play_mode {
            PlayMode::RepeatAll => "切换为单曲循环",
            PlayMode::RepeatOne => "关闭循环播放",
            PlayMode::Shuffle | PlayMode::Sequential => "开启列表循环",
        };
        // AMLL 的 shuffle/repeat 激活态是整块白色圆角图标；没有 knockout
        // 能力时用 22% 白色底 + 白色字形近似，保持同样的尺寸与位置。
        let icon_chip = |active: bool| {
            div()
                .size(px(icon_side))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .justify_center()
                .when(active, |d| d.bg(white.opacity(0.22)))
        };
        let sp = || div().flex_1();
        let bubble_style = crate::bubble::BubbleStyle::on_dark(Theme::get(cx).font_family.clone());
        let media_btn = |id: &'static str,
                         label: SharedString,
                         child: AnyElement,
                         on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>|
         -> AnyElement {
            let trigger = div()
                .id(id)
                .size(px(btn))
                .rounded(px(btn / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.133)))
                .child(child)
                .into_any_element();
            crate::bubble::bubble_button(bubble_style.clone(), id, label, trigger, on_click)
        };

        let shuffle = {
            let weak = weak.clone();
            media_btn(
                "amll-shuffle",
                shuffle_label.into(),
                icon_chip(shuffle_active)
                    .child(crate::icons::icon_motion(
                        crate::icons::Icon::Shuffle,
                        icon_side,
                        white,
                        crate::icons::IconMotion {
                            press: press_shuffle,
                            ..Default::default()
                        },
                    ))
                    .into_any_element(),
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.toggle_amll_shuffle();
                        app.ui_anim_shuffle = Some(Instant::now());
                        app.ensure_ui_anim_ticker(cx);
                        cx.notify();
                    });
                }),
            )
        };
        let prev = {
            let weak = weak.clone();
            media_btn(
                "amll-prev",
                "上一首".into(),
                crate::icons::icon_motion(
                    crate::icons::Icon::Rewind,
                    icon_skip,
                    white,
                    crate::icons::IconMotion {
                        press: press_enter,
                        arrows,
                        ..Default::default()
                    },
                ),
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_prev(cx));
                }),
            )
        };
        let play = {
            let weak = weak.clone();
            media_btn(
                "amll-play",
                if playing { "暂停" } else { "播放" }.into(),
                crate::icons::icon_motion(
                    if playing {
                        crate::icons::Icon::PauseMedia
                    } else {
                        crate::icons::Icon::PlayMedia
                    },
                    icon_play,
                    white,
                    crate::icons::IconMotion {
                        press: press_play,
                        ..Default::default()
                    },
                ),
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_playback(cx));
                }),
            )
        };
        let next = {
            let weak = weak.clone();
            media_btn(
                "amll-next",
                "下一首".into(),
                crate::icons::icon_motion(
                    crate::icons::Icon::Forward,
                    icon_skip,
                    white,
                    crate::icons::IconMotion {
                        press: press_enter,
                        arrows,
                        ..Default::default()
                    },
                ),
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_next(cx));
                }),
            )
        };
        let repeat = {
            let weak = weak.clone();
            media_btn(
                "amll-repeat",
                repeat_label.into(),
                icon_chip(repeat_active)
                    .child(crate::icons::icon_motion(
                        if self.play_mode == PlayMode::RepeatOne {
                            crate::icons::Icon::RepeatOne
                        } else {
                            crate::icons::Icon::Repeat
                        },
                        icon_side,
                        white,
                        crate::icons::IconMotion {
                            press: press_repeat,
                            ..Default::default()
                        },
                    ))
                    .into_any_element(),
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.cycle_amll_repeat();
                        app.ui_anim_repeat = Some(Instant::now());
                        app.ensure_ui_anim_ticker(cx);
                        cx.notify();
                    });
                }),
            )
        };
        div()
            .w(px(width))
            .flex()
            .items_center()
            .child(shuffle)
            .child(sp())
            .child(prev)
            .child(sp())
            .child(play)
            .child(sp())
            .child(next)
            .child(sp())
            .child(repeat)
            .into_any_element()
    }

    /// AMLL VolumeControl 等价音量行。
    pub(super) fn amll_volume_row(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let speaker_progress = Self::anim_progress(
            self.ui_anim_volume.map(|(t, _)| t),
            Self::UI_ANIM_SPEAKER_MS,
        );
        let (speaker_low, speaker_high) = match self.ui_anim_volume {
            Some((_, true)) => (0.0, speaker_progress),
            Some((_, false)) => (speaker_progress, 0.0),
            None => (0.0, 0.0),
        };
        let volume_slider = {
            let player = self.player.clone();
            crate::widgets::amll_live_slider(
                self.player_status.volume.clamp(0.0, 1.0),
                "volume",
                move |fraction: f32, cx: &mut App| {
                    player.set_volume(fraction);
                    let _ = weak.update(cx, |app, cx| {
                        app.note_volume_input(fraction, cx);
                    });
                },
            )
        };
        // AMLL VolumeControl：行高 40（svg 布局高度），左右负边距 -12 / -8，
        // 喇叭的可见图形只占 svg 画布的一小部分。
        div()
            .w(px(width + 20.0))
            .ml(px(-12.0))
            .mr(px(-8.0))
            .h(px(40.0))
            .flex()
            .items_center()
            .child(
                div()
                    .w(px(32.0))
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::icons::icon_motion(
                        crate::icons::Icon::VolumeLow,
                        26.7,
                        white.opacity(0.5),
                        crate::icons::IconMotion {
                            speaker: speaker_low,
                            ..Default::default()
                        },
                    )),
            )
            .child(div().flex_1().child(volume_slider))
            .child(
                div()
                    .w(px(43.0))
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::icons::icon_motion(
                        crate::icons::Icon::VolumeHigh,
                        33.4,
                        white.opacity(0.5),
                        crate::icons::IconMotion {
                            speaker: speaker_high,
                            ..Default::default()
                        },
                    )),
            )
            .into_any_element()
    }

    /// 布局 1 / 布局 2 共用的底部控制行。
    ///
    /// 左：全屏；右：队列 ·（歌词）· 更多 · 关闭。两套布局只通过
    /// `show_lyrics` 决定是否露出直接歌词开关，其余按钮、尺寸、间距与
    /// 交互完全一致，不再维护两套 Dock。
    pub(super) fn immersive_bottom_row(
        &self,
        width: f32,
        show_lyrics: bool,
        lyrics_on: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let bubble_style = crate::bubble::BubbleStyle::on_dark(Theme::get(cx).font_family.clone());
        let btn =
            |icon: crate::icons::Icon,
             id: &'static str,
             label: &'static str,
             active: bool,
             on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>| {
                let trigger = div()
                    .id(id)
                    .size(px(40.0))
                    .rounded(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(if active { 1.0 } else { 0.45 })
                    .hover(|s| s.opacity(1.0).bg(white.opacity(0.133)))
                    .child(crate::icons::icon(icon, 18.0, white))
                    .into_any_element();
                crate::bubble::bubble_button(bubble_style.clone(), id, label, trigger, on_click)
            };

        let fullscreen = btn(
            crate::icons::Icon::Expand,
            "immersive-fullscreen",
            "切换全屏",
            false,
            Box::new(|_, window, _| window.toggle_fullscreen()),
        );
        let queue = {
            let weak = weak.clone();
            btn(
                crate::icons::Icon::Queue,
                "immersive-queue",
                "查看播放队列",
                self.portrait_page == PortraitPage::Queue,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.close_drawer(cx);
                        app.set_portrait_page(PortraitPage::Queue, cx);
                    });
                }),
            )
        };
        let lyrics = show_lyrics.then(|| {
            let weak = weak.clone();
            btn(
                crate::icons::Icon::Lyrics,
                "immersive-lyrics",
                if lyrics_on {
                    "隐藏歌词"
                } else {
                    "显示歌词"
                },
                lyrics_on,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_wide_lyrics(cx));
                }),
            )
        });
        let more = {
            let weak = weak.clone();
            btn(
                crate::icons::Icon::More,
                "immersive-more",
                if self.drawer_open {
                    "关闭更多选项"
                } else {
                    "更多播放选项"
                },
                self.drawer_open,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        if app.drawer_open {
                            app.close_drawer(cx);
                        } else {
                            app.open_drawer(cx);
                        }
                    });
                }),
            )
        };
        let close = {
            let weak = weak.clone();
            btn(
                crate::icons::Icon::Close,
                "immersive-close",
                "退出沉浸播放",
                false,
                Box::new(move |_, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.hide_drawer_now();
                        app.exit_immersive(cx);
                        window.focus(&app.focus_handle);
                        cx.notify();
                    });
                }),
            )
        };

        // 歌词列一直铺到卡片底边，整条控制行压在歌词之上。gpui 的普通
        // 命中区不会遮挡背后的命中区，不加 occlude 时点按钮会同时命中
        // 歌词视图的 on_mouse_down，误触「点歌词跳转时间轴」。这里把整条
        // 控制行声明为遮挡层：按钮照常可点，但背后的歌词不再接收点击。
        div()
            .w(px(width))
            .h(px(40.0))
            .flex()
            .items_center()
            .justify_between()
            .occlude()
            .child(fullscreen)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .child(queue)
                    .when_some(lyrics, |row, button| row.child(button))
                    .child(more)
                    .child(close),
            )
            .into_any_element()
    }
}
