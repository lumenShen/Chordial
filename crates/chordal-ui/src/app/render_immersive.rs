//! `ChordialApp` 的 render_immersive 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn render_immersive(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        let card = full.lerp(self.current_dock_rect(), p);
        let card_w = card.w.max(1.0);
        let card_h = card.h.max(1.0);
        // 显隐通道全部走 smoothstep：两端斜率为 0，控件不会在行程起止帧
        // 突然出现/消失。区间比原来更长，展开/收起时各处细节是「渐渐长出来」
        // 而不是「最后一帧凭空冒出来」。共享封面/曲名不参与淡入，由舞台接管。
        let content_opacity = ramp(p, 1.0, 0.52);
        // 主面板自带的 Dock 更早淡出：真实 Dock 就在卡片背后，晚淡出会
        // 看到两层图标叠加（黑边/重影）。
        let dock_opacity = ramp(p, 0.0, 0.38);
        // 靠近 Dock 时把卡片配色过渡回 Dock 的玻璃底与主题文字。这样
        // p = 1 的卡片与真实 Dock 视觉一致，退出/进入都不会闪一下明暗。
        let theme = Theme::get(cx).clone();
        let dock_blend = ramp(p, 0.72, 1.0);
        // 收口时换成 Dock 的圆角，否则卡片盖在 Dock 上会在四角露出两层边。
        let card_radius = 18.0 + (22.0 - 18.0) * dock_blend;
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

        let source_mode = self.source_stage_mode();
        let dock_stage_active =
            self.stage_request.as_ref().is_some_and(|request| {
                request.from_mode.is_source() || request.to_mode.is_source()
            }) || self
                .motion
                .stage
                .as_ref()
                .is_some_and(|spring| spring.active(now) && spring.touches_source());
        let dock_shared = p > 0.001 || dock_stage_active;
        // 页面始终按最终视口布局，卡片只负责裁剪；避免动画每帧重排，
        // 也让隐藏测量得到与静态页面逐像素一致的目标。
        let split_layout = self.stage_split_active(cx, width, height);
        let page = if split_layout {
            self.render_immersive_wide(width, height, dock_shared, dock_opacity, window, cx)
        } else {
            self.render_immersive_compact(width, height, dock_shared, dock_opacity, window, cx)
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
                .rounded(px(card_radius))
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

        // 共享舞台使用窗口坐标，不随卡片裁剪或位移，首尾直接交给真实布局。
        let shared_geometry = if let Some(request) = self.stage_request {
            (request.from_mode.is_source() || request.to_mode.is_source()).then_some(request.from)
        } else if dock_stage_active {
            self.motion.stage.as_ref().map(|spring| spring.sample(now))
        } else if p > 0.001 {
            let target = if self.motion.dock_collapsing {
                self.stage_measurements
                    .cached(source_mode)
                    .unwrap_or_else(|| immersive_geometry(source_mode, width, height))
            } else {
                let mode = self.immersive_stage_mode(cx, width, height);
                self.stage_measurements
                    .cached(mode)
                    .unwrap_or_else(|| immersive_geometry(mode, width, height))
            };
            let dock = self
                .stage_measurements
                .cached(source_mode)
                .unwrap_or_else(|| immersive_geometry(source_mode, width, height));
            Some(
                if self.sheet_drag.is_some() || self.motion.dock_collapsing {
                    target.lerp(dock, p)
                } else {
                    target
                },
            )
        } else {
            None
        }
        .filter(|geometry| geometry.is_valid());

        // 「更多」抽屉画在页面与共享舞台之上；布局 1 / 布局 2 共用。
        // 底栏按钮行与主沉浸区左右边界对齐；下拉菜单锚定在右边界上方。
        // 竖屏歌词态的底栏是 1em 内缩的全宽列，必须走同一套几何。
        let bottom_rect = self.immersive_view_menu_rect(width, height, split_layout, cx);
        let menu_right = (width - bottom_rect.x - bottom_rect.w).max(0.0);
        let menu_bottom = (height - bottom_rect.y + 8.0).max(0.0);

        // 指针移动/按下都会重置闲置计时；只在真正从隐藏恢复时 notify。
        let mut root = div()
            .id("immersive-root")
            .size_full()
            .relative()
            .on_mouse_move(cx.listener(|app, event: &MouseMoveEvent, _, cx| {
                app.note_stage_pointer(
                    f32::from(event.position.x),
                    f32::from(event.position.y),
                    cx,
                );
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, _: &MouseDownEvent, _, cx| {
                    app.note_stage_interaction(cx);
                }),
            )
            .child(card_el);
        if dock_shared && let Some(geometry) = shared_geometry {
            root =
                root.child(self.render_dock_shared_stage(geometry, title_color, artist_color, p));
        }
        if self.drawer_visible() {
            root = root.child(self.render_drawer(card_w, card_h, now, cx));
        }
        // 菜单展开时先铺一层透明遮挡：点菜单外即收起，菜单本身阻止冒泡。
        if self.stage_view_menu_open {
            root = root.child(
                div()
                    .id("stage-view-scrim")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|app, _: &MouseDownEvent, _, cx| {
                            app.stage_view_menu_open = false;
                            cx.notify();
                        }),
                    ),
            );
        }
        root = root
            .child(self.render_stage_view_menu(menu_right, menu_bottom, cx))
            .children(Self::resize_handles());

        // 拖拽期间卡片会缩小、把手不再跟随指针；用一张满窗捕获层接管
        // 后续移动与松手，保证手势连续。
        if self.sheet_drag.is_some() {
            root = root.child(
                div()
                    .id("immersive-drag-capture")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .cursor(CursorStyle::ClosedHand)
                    .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                        let y = f32::from(event.position.y);
                        if event.pressed_button == Some(gpui::MouseButton::Left) {
                            app.update_sheet_drag(y, cx);
                        } else if app
                            .sheet_drag
                            .as_ref()
                            .is_some_and(|drag| !drag.animating(Instant::now()))
                        {
                            app.release_sheet_drag(y, height, cx);
                        }
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(move |app, event: &MouseUpEvent, _, cx| {
                            if app.sheet_drag.is_some() {
                                app.release_sheet_drag(f32::from(event.position.y), height, cx);
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

    /// 给带 `occlude` 的控件行补发舞台交互通知。
    ///
    /// `occlude` 会截断命中链，沉浸根节点上的 `on_mouse_move` / `on_mouse_down`
    /// 收不到行内事件；闲置计时与控件唤出必须在这里补上，否则在控件上移动或
    /// 点击不会刷新 5s 闲置窗口，控件可能在操作中途淡出。
    fn stage_row_interaction(row: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        row.on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|app, _: &MouseDownEvent, _, cx| app.note_stage_interaction(cx)),
        )
        .on_mouse_move(cx.listener(|app, event: &MouseMoveEvent, _, cx| {
            app.note_stage_pointer(f32::from(event.position.x), f32::from(event.position.y), cx);
        }))
    }

    /// AMLL BouncingSlider 进度条 + 时间/音质标签行。
    ///
    /// 右侧时间默认显示剩余时长，点一下切到总时长（AMLL
    /// `showRemainingTimeAtom` 的交互）。音质标签是带 hairline 描边的
    /// 半透明白色胶囊，点击打开解码信息弹窗。
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
        let progress_bar = crate::widgets::amll_playback_slider(
            progress,
            duration,
            status.state == PlayerState::Playing,
            "seek",
            seek,
        );

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
                .justify_center()
                .h(px(20.0 * tag_scale))
                .px(px(8.0 * tag_scale))
                .rounded(px(10.0 * tag_scale))
                .border_1()
                .border_color(white.opacity(0.16))
                .bg(white.opacity(0.13))
                .text_size(px(10.5 * tag_scale))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(white.opacity(0.9))
                .cursor_pointer()
                // 半透明背景在封面渐变上容易被吃掉，用 hairline 描边固定轮廓；
                // 悬停/按下只改变填充与描边浓度，避免标签抢走歌词的视觉重心。
                .hover(|s| s.bg(white.opacity(0.22)).border_color(white.opacity(0.26)))
                .active(|s| s.bg(white.opacity(0.28)))
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

        // 进度条/时间标签压在歌词之上：整块 occlude，避免按下时同时命中
        // 背后歌词视图的「点字跳转」监听（否则点滑杆会先跳歌词时间轴）。
        Self::stage_row_interaction(
            div()
                .w(px(width))
                .flex()
                .flex_col()
                .occlude()
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
                ),
            cx,
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
                self.playback_action_label().into(),
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
        // 传输键同样压在歌词之上；occlude 保证点按钮/按钮间隙都不会把
        // 按下事件漏给歌词的点击跳转。
        Self::stage_row_interaction(
            div()
                .w(px(width))
                .flex()
                .items_center()
                .occlude()
                .child(shuffle)
                .child(sp())
                .child(prev)
                .child(sp())
                .child(play)
                .child(sp())
                .child(next)
                .child(sp())
                .child(repeat),
            cx,
        )
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
        Self::stage_row_interaction(
            div()
                .w(px(width + 20.0))
                .ml(px(-12.0))
                .mr(px(-8.0))
                .h(px(40.0))
                .flex()
                .items_center()
                .occlude()
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
                ),
            cx,
        )
        .into_any_element()
    }

    /// 布局 1 / 布局 2 共用的底部控制行。
    ///
    /// 左：全屏；右：一个「图标 + 文字 + 箭头」的视图选择器。四项视图共用
    /// 同一个右侧内容区；「更多」入口已收进曲目信息右侧的圆钮。
    /// 透明度、模糊与缩放由外层的 `StageChromeGroup` 统一拥有。
    pub(super) fn immersive_bottom_row(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let bubble_style = crate::bubble::BubbleStyle::on_dark(Theme::get(cx).font_family.clone());
        let fullscreen = {
            let weak = weak.clone();
            let trigger = div()
                .id("immersive-fullscreen")
                .size(px(40.0))
                .rounded(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .opacity(0.45)
                .hover(|s| s.opacity(1.0).bg(white.opacity(0.133)))
                .child(crate::icons::icon(crate::icons::Icon::Expand, 18.0, white))
                .into_any_element();
            crate::bubble::bubble_button(
                bubble_style,
                "immersive-fullscreen",
                "切换全屏",
                trigger,
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                    let allowed = weak
                        .update(cx, |app, cx| {
                            if app.stage_chrome_hidden {
                                return false;
                            }
                            app.note_stage_interaction(cx);
                            true
                        })
                        .unwrap_or(false);
                    if allowed {
                        window.toggle_fullscreen();
                    }
                },
            )
        };

        let view = self.visible_stage_view();
        let select = {
            let weak = weak.clone();
            div()
                .id("immersive-view-select")
                .h(px(40.0))
                .px(px(13.0))
                .rounded(px(20.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .bg(white.opacity(0.10))
                .border_1()
                .border_color(white.opacity(0.10))
                .hover(|s| s.bg(white.opacity(0.17)))
                .active(|s| s.bg(white.opacity(0.22)))
                .cursor_pointer()
                .child(crate::icons::icon(view.icon(), 16.0, white.opacity(0.92)))
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(white.opacity(0.92))
                        .child(SharedString::from(view.label())),
                )
                .child(crate::icons::icon(
                    crate::icons::Icon::ChevronDown,
                    11.0,
                    white.opacity(0.6),
                ))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        if app.stage_chrome_hidden {
                            return;
                        }
                        app.stage_view_menu_open = !app.stage_view_menu_open;
                        app.note_stage_interaction(cx);
                        cx.notify();
                    });
                })
        };

        // 歌词列一直铺到卡片底边，整条控制行压在歌词之上；occlude 防止
        // 点按钮同时命中背后的歌词「点字跳转时间轴」。
        div()
            .w(px(width))
            .h(px(40.0))
            .flex()
            .items_center()
            .justify_between()
            .occlude()
            .child(fullscreen)
            .child(select)
            .into_any_element()
    }

    /// 右下角选择器的下拉菜单：风格参照原生 Select（圆角卡片 + hairline
    /// 分隔 + 当前项勾选），锚定在视图按钮正上方。
    pub(super) fn render_stage_view_menu(
        &self,
        right: f32,
        bottom: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.stage_view_menu_open {
            return div().into_any_element();
        }
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let mut panel = div()
            .id("stage-view-menu")
            .absolute()
            .right(px(right))
            .bottom(px(bottom))
            .w(px(196.0))
            .rounded(px(13.0))
            .bg(theme.elevated)
            .border_1()
            .border_color(theme.border_strong)
            .shadow(vec![BoxShadow {
                color: gpui::black().opacity(0.35),
                offset: point(px(0.0), px(12.0)),
                blur_radius: px(32.0),
                spread_radius: px(0.0),
            }])
            .p(px(6.0))
            .flex()
            .flex_col()
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for (index, view) in [
            StageView::Queue,
            StageView::Lyrics,
            StageView::Audio,
            StageView::MainOnly,
        ]
        .into_iter()
        .enumerate()
        {
            if index > 0 {
                panel = panel.child(div().h(px(1.0)).mx(px(8.0)).bg(theme.border));
            }
            let weak = weak.clone();
            let active = self.visible_stage_view() == view;
            panel = panel.child(
                div()
                    .id(gpui::ElementId::named_usize("stage-view", index))
                    .h(px(38.0))
                    .px(px(9.0))
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.hover))
                    .hover(|d| d.bg(theme.hover))
                    .child(crate::icons::icon(
                        view.icon(),
                        15.0,
                        if active { theme.text } else { theme.text_dim },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.0))
                            .text_color(if active { theme.text } else { theme.text_dim })
                            .child(SharedString::from(view.label())),
                    )
                    .when(active, |d| {
                        d.child(crate::icons::icon(
                            crate::icons::Icon::Check,
                            13.0,
                            theme.accent,
                        ))
                    })
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.set_stage_view(view, cx);
                            cx.notify();
                        });
                    }),
            );
        }
        panel.into_any_element()
    }
}
