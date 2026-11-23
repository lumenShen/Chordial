//! `ChordialApp` 的 render_stage 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    /// 沉浸层统一的曲名文案。共享元素与页面占位共用同一份文本，
    /// 保证补间结束交还静态布局时不会发生文字替换。
    pub(super) fn current_title_text(&self) -> String {
        self.current_track
            .as_ref()
            .map(|t| t.title.clone())
            .or_else(|| self.player_status.title.clone())
            .unwrap_or_else(|| "未在播放".into())
    }

    /// 沉浸层统一的艺人文案（AMLL 语义：`A, B & C`，无艺人时回退专辑）。
    pub(super) fn current_artist_text(&self) -> String {
        match self.current_track.as_ref() {
            Some(track) => {
                let names: Vec<&str> = track
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .filter(|n| !n.is_empty())
                    .collect();
                match names.len() {
                    0 => track
                        .album
                        .as_ref()
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| "未知艺人".into()),
                    1 => names[0].to_string(),
                    n => format!("{} & {}", names[..n - 1].join(", "), names[n - 1]),
                }
            }
            None => "选择一首歌开始".into(),
        }
    }

    /// 只有曲目信息与右侧圆钮（封面态放在大封面下方）。
    pub(super) fn amll_info_row(
        &self,
        width: f32,
        font: f32,
        height: f32,
        back: bool,
        interactive: bool,
        hide_text: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let title = self.current_title_text();
        let artist = self.current_artist_text();
        // AMLL MenuButton：width 3.5vh；窄屏改用 max(2em, min(5vw, 4vh))。
        let action_side = if self.viewport_width <= 1000.0 {
            ((self.viewport_width * 0.05).min(height * 0.04)).max(32.0)
        } else {
            height * 0.035
        };
        let action = {
            let trigger = div()
                .id("amll-info-action")
                .size(px(action_side))
                .rounded(px(action_side * 0.5))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .ml(px(16.0))
                .bg(white.opacity(0.082))
                .child(crate::icons::icon(
                    if back {
                        crate::icons::Icon::ChevronLeft
                    } else {
                        crate::icons::Icon::MoreDot
                    },
                    action_side * 0.72,
                    white,
                ))
                .into_any_element();
            if interactive {
                let weak = weak.clone();
                crate::bubble::bubble_button(
                    crate::bubble::BubbleStyle::on_dark(Theme::get(cx).font_family.clone()),
                    "amll-info-action",
                    if back {
                        "返回播放页"
                    } else {
                        "更多播放选项"
                    },
                    trigger,
                    move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            if back {
                                app.set_portrait_page(PortraitPage::Art, cx);
                            } else {
                                app.open_drawer(cx);
                            }
                        });
                    },
                )
            } else {
                trigger
            }
        };
        let available = (width - action_side - 16.0).max(1.0);
        let title_w = measure_text_width(window, &title, font, gpui::FontWeight::MEDIUM);
        let artist_w = measure_text_width(window, &artist, font, gpui::FontWeight::NORMAL);
        let now = Instant::now();
        let marquee = self.info_marquee;
        let make_line = |line: u8,
                         text: String,
                         measured: f32,
                         weight: gpui::FontWeight,
                         color: gpui::Hsla|
         -> AnyElement {
            // AMLL TextMarquee：只有当文字宽度超过容器 95% 时，悬停才滚动。
            let overflowing = measured > available * 0.95;
            let distance = (measured - available * 0.95).max(0.0);
            let duration_ms = (distance * 2.0 / 64.0 * 1000.0).max(1.0);
            let active = marquee.filter(|m| m.line == line);
            let animating = active.is_some_and(|m| m.active(now));
            let offset = active.map(|m| m.offset(now)).unwrap_or(0.0);
            let hover_weak = weak.clone();
            let mut outer = div()
                .id(if line == 0 {
                    "info-line-title"
                } else {
                    "info-line-artist"
                })
                .w_full()
                .overflow_hidden()
                .on_hover(move |hovered, _, cx| {
                    if !overflowing {
                        return;
                    }
                    let _ = hover_weak.update(cx, |app, cx| {
                        if *hovered {
                            app.info_marquee = Some(InfoMarquee {
                                line,
                                start: Instant::now(),
                                distance,
                                duration_ms,
                            });
                        } else if app.info_marquee.is_some_and(|m| m.line == line) {
                            app.info_marquee = None;
                        }
                        cx.notify();
                    });
                });
            if animating {
                outer = outer.child(
                    div()
                        .w(px(measured + 2.0))
                        .ml(px(-offset))
                        .text_size(px(font))
                        .line_height(px(font * INFO_LINE_RATIO))
                        .font_weight(weight)
                        .text_color(color)
                        .whitespace_nowrap()
                        .child(SharedString::from(text)),
                );
            } else {
                outer = outer.child(
                    div()
                        .w_full()
                        .text_size(px(font))
                        .line_height(px(font * INFO_LINE_RATIO))
                        .font_weight(weight)
                        .text_color(color)
                        .text_ellipsis()
                        .child(SharedString::from(text)),
                );
            }
            outer.into_any_element()
        };
        div()
            .w(px(width))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .opacity(if hide_text { 0.0 } else { 1.0 })
                    .child(make_line(
                        0,
                        title,
                        title_w,
                        gpui::FontWeight::MEDIUM,
                        white.opacity(0.9),
                    ))
                    .child(make_line(
                        1,
                        artist,
                        artist_w,
                        gpui::FontWeight::NORMAL,
                        white.opacity(0.45),
                    )),
            )
            .child(action)
            .into_any_element()
    }

    /// 桌面宽屏与竖屏共用的 AMLL 播放器列。
    ///
    /// 封面、信息、进度、传输、音量共用同一个列宽；歌词关闭/无歌词时由调用方
    /// 把这整列水平居中。这样两端只有定位不同，控件本身不再各写一套。
    pub(super) fn amll_player_column(
        &self,
        column_w: f32,
        height: f32,
        controls_h: f32,
        chrome_o: f32,
        menu_interactive: bool,
        shared_hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let theme = Theme::dark();
        // AMLL Cover：半径 max(2%, 0.7vh)；整块带
        // `0 1em 1.2em rgba(0,0,0,.19)` 投影。暂停时整体缩到 0.75。
        let cover_scale = self.cover_scale_now(Instant::now()).clamp(0.5, 1.1);
        let cover_side = column_w * cover_scale;
        let cover_radius = (column_w * 0.02).max(height * 0.007) * cover_scale;
        let cover = div()
            .w(px(column_w))
            .h(px(column_w))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(cover_side))
                    .rounded(px(cover_radius))
                    .shadow(vec![BoxShadow {
                        color: gpui::black().opacity(0.19),
                        offset: point(px(0.0), px(16.0 * cover_scale)),
                        blur_radius: px(19.2 * cover_scale),
                        spread_radius: px(0.0),
                    }])
                    .child(crate::widgets::cover_plain(
                        &theme,
                        self.cover_image.clone(),
                        cover_side,
                        cover_radius,
                        crate::icons::icon(
                            crate::icons::Icon::Note,
                            (cover_side * 0.1).clamp(28.0, 40.0),
                            white.opacity(0.4),
                        ),
                    )),
            );
        let info_font = (height * 0.02).max(16.0);
        let info = div()
            .w(px(column_w))
            .opacity(chrome_o)
            .child(self.amll_info_row(
                column_w,
                info_font,
                height,
                false,
                menu_interactive,
                shared_hidden,
                window,
                cx,
            ));
        let label_font = (height * 0.012).max(12.8);
        let progress = div()
            .w(px(column_w))
            .h(px(45.2))
            .opacity(chrome_o)
            .child(self.amll_progress_block(column_w, label_font, height, cx));
        // 手机最小宽度时封面列只有 ~184px；40px 的按钮会让 5 个传输键
        // 互相挤压。低于 240 时收到 34px，图标仍保持可点击尺寸。
        let btn = (column_w * 0.18).clamp(if column_w < 240.0 { 34.0 } else { 40.0 }, 88.0);
        let transport = self.amll_transport(column_w, btn, height, cx);
        let volume = self.amll_volume_row(column_w, cx);
        let controls = div()
            .w(px(column_w))
            .h(px(controls_h.max(0.0)))
            .flex()
            .flex_col()
            .items_center()
            .justify_between()
            .child(info)
            .child(progress)
            .child(transport)
            .child(volume);
        div()
            .w(px(column_w))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .opacity(if shared_hidden { 0.0 } else { 1.0 })
                    .child(cover),
            )
            // AMLL：封面与 music-info 行之间是 8px grid gap，控件列再有
            // `margin-top: calc(-8px + 1.75em)`，合起来 28px（1em = 16px）。
            .child(div().h(px(28.0)))
            .child(controls)
            .into_any_element()
    }

    /// 共享元素舞台：一帧内只画一份封面 + 曲名/艺人，用插值几何定位。
    ///
    /// 调用方已在页面里把同名元素设为透明（保留占位），因此这里不做交叉
    /// 淡入——两端文案一致，字号、行高、位置连续变化即可。
    pub(super) fn render_shared_stage(&self, geometry: StageGeometry) -> AnyElement {
        let white = gpui::white();
        self.render_shared_stage_colored(geometry, white.opacity(0.9), white.opacity(0.45))
    }

    /// 带文字的共享舞台；Dock 收尾时文字颜色会过渡回主面板主题色。
    pub(super) fn render_shared_stage_colored(
        &self,
        geometry: StageGeometry,
        title_color: gpui::Hsla,
        artist_color: gpui::Hsla,
    ) -> AnyElement {
        let white = gpui::white();
        let theme = Theme::dark();
        let title = self.current_title_text();
        let artist = self.current_artist_text();
        let cover_size = geometry.cover.w.max(1.0);
        let cover = crate::widgets::cover_plain(
            &theme,
            self.cover_image.clone(),
            cover_size,
            geometry.cover_radius,
            crate::icons::icon(
                crate::icons::Icon::Note,
                (cover_size * 0.22).clamp(14.0, 40.0),
                white.opacity(0.35),
            ),
        );
        div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .left(px(geometry.cover.x))
                    .top(px(geometry.cover.y))
                    .child(cover),
            )
            .child(
                div()
                    .absolute()
                    .left(px(geometry.info.x))
                    .top(px(geometry.info.y))
                    .w(px(geometry.info.w.max(1.0)))
                    .flex()
                    .flex_col()
                    .gap(px(geometry.line_gap))
                    .child(
                        div()
                            .max_w_full()
                            .text_size(px(geometry.title_size))
                            .line_height(px(geometry.title_size * INFO_LINE_RATIO))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(title_color)
                            .text_ellipsis()
                            .child(SharedString::from(title)),
                    )
                    .child(
                        div()
                            .max_w_full()
                            .text_size(px(geometry.artist_size))
                            .line_height(px(geometry.artist_size * INFO_LINE_RATIO))
                            .text_color(artist_color)
                            .text_ellipsis()
                            .child(SharedString::from(artist)),
                    ),
            )
            .into_any_element()
    }

    /// AMLL 顶部拖拽 pill（50×8，白 17.6%）。
    ///
    /// 它只是把手的视觉标识；按下命中区由外层 dismiss-handle 承担，
    /// 拖动是「下拉关闭沉浸页」而不是移动窗口。
    pub(super) fn amll_pill(&self, left: f32, top: f32, id: &'static str) -> AnyElement {
        div()
            .id(id)
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(50.0))
            .h(px(8.0))
            .rounded(px(4.0))
            .bg(gpui::white().opacity(0.176))
            .into_any_element()
    }

    /// 竖屏播放页：AMLL 竖向布局 + Salt Player 的子页面/自动隐藏逻辑。
    pub(super) fn render_immersive_compact(
        &mut self,
        width: f32,
        height: f32,
        dock_shared: bool,
        dock_opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let now = Instant::now();
        // 同一列宽供封面、信息、进度、传输、音量与 Dock 使用。
        let content_w = compact_content_w(width, height);
        // AMLL 竖向布局的控件常驻：红框内的信息/进度与 Dock 不再淡出，
        // 避免停手 3.2 秒后中间留下一大片空白。
        let chrome_o = 1.0;
        let enter = self
            .portrait_enter
            .map(|t| (t.elapsed().as_secs_f32() / 0.26).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let enter = 1.0 - (1.0 - enter).powi(3);
        let page = self.portrait_page;
        let transition = self.portrait_transition.filter(|t| !t.done(now));
        // 共享元素补间：页面滑入/滑出时，封面与曲目信息走独立的 FLIP 路径，
        // 页面内的同名元素暂时透明，只保留占位。
        let stage = self
            .stage_motion
            .filter(|motion| motion.active(now) && motion.touches_compact())
            .map(|motion| motion.geometry(now))
            .filter(|geometry| geometry.is_valid());
        let shared_hidden = stage.is_some() || dock_shared;
        // 竖屏页不显示歌词：歌词视图退化为全窗 Mesh 背景。
        self.lyric_align = 0.1;
        self.lyric_view.update(cx, |view, _| {
            view.set_text_visible(false);
            view.set_click_seek_enabled(false);
            view.set_player_config(LyricPlayerConfig {
                align_anchor: AlignAnchor::Top,
                align_position: 0.1,
                enable_blur: !self.no_blur,
                ..LyricPlayerConfig::default()
            });
        });
        self.ensure_portrait_idle_ticker(cx);

        let lyric_view_el = div().absolute().inset_0().child(self.lyric_view.clone());

        // 顶部拖动带：播放页的整条页头都可拖动窗口；pill 附近保留
        // 下拉关闭手势。队列/更多页的页头有自己的可点元素，不铺拖动带。
        // Dock→全屏的位移由外层卡片承担，页面内部不再二次偏移。
        let sheet_offset = 0.0;
        let art_page = page == PortraitPage::Art && transition.is_none() && !self.eq_open;
        let drag_band = art_page.then(|| {
            Self::window_drag_area(
                div()
                    .id("immersive-drag-band-compact")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(COMPACT_TOP))
                    .cursor(CursorStyle::OpenHand),
            )
        });
        let handle_w = 120.0_f32.min(width);
        let handle_top = 0.0;
        let dismiss_handle = art_page.then(|| {
            div()
                .id("immersive-dismiss-handle-compact")
                .absolute()
                .top(px(handle_top))
                .left(px((width - handle_w) / 2.0))
                .w(px(handle_w))
                .h(px(56.0))
                .cursor(CursorStyle::OpenHand)
                .occlude()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|app, event: &MouseDownEvent, _, cx| {
                        app.begin_sheet_drag(f32::from(event.position.y), cx);
                    }),
                )
                .child(self.amll_pill(
                    handle_w / 2.0 - 25.0,
                    20.0 - handle_top,
                    "immersive-pill-compact",
                ))
        });

        // 页面本体：下拉时整块下移，露出背后的主面板。
        let mut sheet = div()
            .id("immersive-sheet-compact")
            .absolute()
            .left_0()
            .top(px(sheet_offset))
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(gpui::black())
            .opacity(enter)
            .child(lyric_view_el)
            .when_some(drag_band, |sheet, band| sheet.child(band))
            .when_some(dismiss_handle, |sheet, handle| sheet.child(handle));

        let overlay = div()
            .id("immersive-compact-overlay")
            .key_context("ChordialImmersive")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
                if app.quality_dialog_open {
                    app.quality_dialog_open = false;
                } else if app.drawer_open {
                    app.close_drawer(cx);
                } else if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.exit_immersive(cx);
                }
                window.focus(&app.focus_handle);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &TogglePortrait, _, cx| {
                app.exit_immersive(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &ToggleEqualizer, _, cx| {
                if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.open_eq(cx);
                }
            }))
            .on_action(cx.listener(|app, _: &PlayPause, _, cx| app.toggle_playback(cx)))
            .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
            .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
            .on_action(cx.listener(|app, _: &PlayFirst, _, cx| app.play_first(cx)))
            .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                app.bump_portrait(cx);
                let y = f32::from(event.position.y);
                match event.pressed_button {
                    Some(gpui::MouseButton::Left) => app.update_sheet_drag(y, cx),
                    None => {
                        if app.sheet_drag.is_some_and(|drag| !drag.animating()) {
                            app.release_sheet_drag(height, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |app, _: &MouseUpEvent, _, cx| {
                    if app.sheet_drag.is_some() {
                        app.release_sheet_drag(height, cx);
                    }
                }),
            )
            .on_click(cx.listener(|app, _, _, cx| app.bump_portrait(cx)))
            .size_full()
            .relative()
            .overflow_hidden();

        if let Some(tr) = transition {
            let (mut from_off, from_o, mut to_off, to_o) = tr.frames(now, width, height);
            // 从队列页返回播放页时方向相反：子页退出、播放页归位。
            if matches!(tr.from, PortraitPage::Queue) {
                from_off = -from_off;
                to_off = -to_off;
            }
            let (fx, fy) = match tr.kind {
                crate::player_page::TransitionKind::Vertical => (0.0, from_off),
                crate::player_page::TransitionKind::Fade => (0.0, 0.0),
            };
            let (tx, ty) = match tr.kind {
                crate::player_page::TransitionKind::Vertical => (0.0, to_off),
                crate::player_page::TransitionKind::Fade => (0.0, 0.0),
            };
            let from = gpui::batch_layer(self.render_compact_page(
                tr.from,
                content_w,
                height,
                width,
                chrome_o,
                dock_opacity,
                shared_hidden,
                window,
                cx,
            ));
            let to = gpui::batch_layer(self.render_compact_page(
                tr.to,
                content_w,
                height,
                width,
                chrome_o,
                dock_opacity,
                shared_hidden,
                window,
                cx,
            ));
            sheet = sheet
                .child(
                    div()
                        .absolute()
                        .left(px(fx))
                        .top(px(fy))
                        .size_full()
                        .opacity(from_o)
                        .child(from),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(tx))
                        .top(px(ty))
                        .size_full()
                        .opacity(to_o)
                        .child(to),
                );
            self.ensure_ui_anim_ticker(cx);
        } else {
            if self.portrait_transition.is_some() {
                self.portrait_transition = None;
            }
            sheet = sheet.child(gpui::batch_layer(self.render_compact_page(
                page,
                content_w,
                height,
                width,
                chrome_o,
                dock_opacity,
                shared_hidden,
                window,
                cx,
            )));
        }

        // 共享元素舞台画在页面之上：封面从大图连续缩到队列页头，
        // 曲名与艺人同步移动、缩放，而不是随页面一起硬切。
        if let Some(geometry) = stage {
            sheet = sheet.child(self.render_shared_stage(geometry));
        }

        if let Some((text, started)) = self.portrait_toast.clone() {
            if started.elapsed() < Duration::from_millis(1800) {
                sheet = sheet.child(
                    div()
                        .absolute()
                        .bottom(px(118.0))
                        .left(px((width - 300.0) / 2.0))
                        .w(px(300.0))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .rounded(px(16.0))
                                .bg(gpui::rgba(0x00000099))
                                .px(px(14.0))
                                .py(px(7.0))
                                .text_size(px(11.5))
                                .text_color(white.opacity(0.92))
                                .child(SharedString::from(text)),
                        ),
                );
            }
        }
        overlay.child(sheet).into_any_element()
    }

    pub(super) fn render_compact_page(
        &self,
        page: PortraitPage,
        content_w: f32,
        height: f32,
        width: f32,
        chrome_o: f32,
        dock_opacity: f32,
        shared_hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match page {
            PortraitPage::Art => self.render_compact_art(
                width,
                height,
                chrome_o,
                dock_opacity,
                shared_hidden,
                window,
                cx,
            ),
            PortraitPage::Queue => self.render_portrait_queue(content_w, shared_hidden, cx),
        }
    }

    /// 竖屏播放页：与桌面宽屏同一根 `amll_player_column`，只是把整列水平居中。
    /// 页面不渲染歌词文本；歌词视图仅作为全窗 Mesh 背景。
    pub(super) fn render_compact_art(
        &self,
        width: f32,
        height: f32,
        chrome_o: f32,
        dock_opacity: f32,
        shared_hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cover = compact_cover_size(width, height);
        let available = (height
            - COMPACT_TOP
            - crate::layout_morph::COMPACT_BOTTOM
            - crate::layout_morph::COMPACT_DOCK_H
            - 8.0)
            .max(0.0);
        let controls_h = (available - cover - COMPACT_COVER_GAP).max(0.0);
        // 从布局 1 / 队列页切换过来时，播放列跟随共享封面的 x 一起滑动，
        // 而不是整列硬切到居中位置。
        let now = Instant::now();
        let motion_x = self
            .stage_motion
            .filter(|motion| motion.active(now) && motion.touches_compact())
            .map(|motion| motion.geometry(now).cover.x);
        let left = motion_x.unwrap_or_else(|| ((width - cover) / 2.0).max(0.0));
        div()
            .size_full()
            .relative()
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(COMPACT_TOP))
                    .w(px(cover))
                    .child(self.amll_player_column(
                        cover,
                        height,
                        controls_h,
                        chrome_o,
                        true,
                        shared_hidden,
                        window,
                        cx,
                    )),
            )
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .bottom(px(crate::layout_morph::COMPACT_BOTTOM))
                    .w(px(cover))
                    .opacity(dock_opacity)
                    .child(self.immersive_bottom_row(cover, false, false, cx)),
            )
            .into_any_element()
    }

    /// 宽屏沉浸：AMLL react-full `HorizontalLayout` 的等价几何。
    ///
    /// 1600×1000 实测：列 info 0.45W / player 0.55W（gap 8px）；行
    /// drag = 0.12fr / thumb 40 / cover / music-info = 0.8fr / buttom 40 / tail = 0.08fr，
    /// fr = H − 120 − cover；封面 = min(45vh, 38vw)（H ≤ 1000，否则 50vh）；
    /// 歌词列右 padding 8%（H > 1000 时 15%）；歌词对齐封面中心（AMLL 用
    /// ResizeObserver 计算 cover 中线占比后作为 `alignPosition`）。
    pub(super) fn render_immersive_wide(
        &mut self,
        width: f32,
        height: f32,
        dock_shared: bool,
        dock_opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = Instant::now();
        // 布局 1 只在歌词可见时启用；这里仍读一次状态，避免未来入口漂移。
        let lyrics_on = self.lyric_view.read(cx).has_lyrics() && !self.wide_lyrics_hidden;
        self.lyric_view.update(cx, |view, _| {
            view.set_text_visible(lyrics_on);
            view.set_click_seek_enabled(lyrics_on);
        });

        // ---- AMLL horizontal 几何 ----
        let cover = wide_cover_size(width, height);
        let gap = 8.0;
        // 剩余 fr 高度按 AMLL 的 6 行 grid 计算：thumb 2×2vh=36，
        // 5 道 8px gap。
        let fr = (height - cover - 76.0).max(0.0);
        let drag = fr * 0.12;
        let cover_top = wide_cover_top(width, height);
        // 底部控制行与布局 2 使用同一个底边距（16px）；音量行必须停在
        // 控制行上方，信息区用剩余高度做 space-between。
        let bottom_row_top = height - 16.0 - 40.0;
        let controls_top = cover_top + cover + COMPACT_COVER_GAP;
        let controls_h = (bottom_row_top - gap - controls_top).max(0.0);
        let info_w = width * 0.45;
        let lyric_left = info_w + gap;
        let lyric_col_w = (width - lyric_left).max(0.0);
        let lyric_pad_right = if height <= 1000.0 {
            lyric_col_w * 0.08
        } else {
            lyric_col_w * 0.15
        };

        // 布局 1 固定为「左播放列 + 右歌词」；歌词开关切换时会立即落到
        // 布局 2，因此这里不再维护居中分支。
        let split_x = wide_column_x(width, height);
        let wide_motion = self
            .stage_motion
            .filter(|motion| motion.active(now) && motion.touches_wide())
            .filter(|motion| motion.from.is_valid() && motion.to.is_valid());
        let stage = wide_motion.map(|motion| motion.geometry(now));
        let mut column_x = split_x;
        if let Some(geometry) = stage {
            column_x = geometry.cover.x;
        }
        let shared_hidden = stage.is_some() || dock_shared;

        let align = ((cover_top + cover * 0.5) / height).clamp(0.05, 0.95);
        if lyrics_on && (self.lyric_align - align).abs() > 0.0005 {
            self.lyric_align = align;
            self.lyric_view.update(cx, |view, _| {
                view.set_player_config(LyricPlayerConfig {
                    align_anchor: AlignAnchor::Center,
                    align_position: align,
                    enable_blur: !self.no_blur,
                    ..LyricPlayerConfig::default()
                });
            });
        }
        // —— 顶部窗口拖动带：整条空白头部都可拖动窗口（AMLL drag-area）。
        //    pill 只保留一个小的下拉关闭命中区，其余宽度全部用来移动窗口。
        // Dock→全屏的位移由外层卡片承担，页面内部不再二次偏移。
        let sheet_offset = 0.0;
        let pill_top = drag + gap + 16.0;
        let drag_band = (!self.eq_open).then(|| {
            Self::window_drag_area(
                div()
                    .id("immersive-drag-band")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(pill_top + 8.0))
                    .cursor(CursorStyle::OpenHand),
            )
        });

        // 下拉关闭把手：与 pill 等宽的窄条，occlude 掉下层的窗口拖动带。
        // EQ 覆盖层打开时不启用，避免顶部的 EQ 控件被手势抢走。
        let handle_w = 160.0_f32.min(width);
        let handle_top = (pill_top - 20.0).max(0.0);
        let dismiss_handle = (!self.eq_open).then(|| {
            div()
                .id("immersive-dismiss-handle")
                .absolute()
                .top(px(handle_top))
                .left(px(column_x + cover * 0.5 - handle_w / 2.0))
                .w(px(handle_w))
                .h(px(56.0))
                .cursor(CursorStyle::OpenHand)
                .occlude()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|app, event: &MouseDownEvent, _, cx| {
                        app.begin_sheet_drag(f32::from(event.position.y), cx);
                    }),
                )
                .child(self.amll_pill(
                    handle_w / 2.0 - 25.0,
                    pill_top - handle_top,
                    "immersive-pill",
                ))
        });

        // —— 共用 AMLL 播放器列：封面 / 信息 / 进度 / 传输 / 音量 ——
        let info_column = div()
            .absolute()
            .left(px(column_x))
            .top(px(cover_top))
            .w(px(cover))
            .child(self.amll_player_column(
                cover,
                height,
                controls_h,
                1.0,
                false,
                shared_hidden,
                window,
                cx,
            ));

        // —— 歌词开启时占右列；关闭/无歌词时整窗只保留 Mesh 背景 ——
        let lyric_area = if lyrics_on {
            div()
                .absolute()
                .left(px(lyric_left))
                .top_0()
                .bottom_0()
                .right_0()
                .pr(px(lyric_pad_right))
                .child(self.lyric_view.clone())
        } else {
            div().absolute().inset_0().child(self.lyric_view.clone())
        };

        // —— 统一的底部控制行：与布局 2 共用同一个构建函数 ——
        let bottom_row = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(bottom_row_top))
            .h(px(40.0))
            .px(px(32.0))
            .opacity(dock_opacity)
            .child(self.immersive_bottom_row((width - 64.0).max(1.0), true, lyrics_on, cx));

        let stage_el = stage.map(|geometry| self.render_shared_stage(geometry));

        // 页面本体：下拉时整块下移，露出背后的主面板。拖动带在下、pill
        // 把手在上（occlude），因此只有 pill 附近是下拉手势，其余顶部拖窗口。
        let sheet = div()
            .id("immersive-sheet")
            .absolute()
            .left_0()
            .top(px(sheet_offset))
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(gpui::black())
            .child(lyric_area)
            .child(gpui::batch_layer(info_column))
            .when_some(stage_el, |sheet, stage| sheet.child(stage))
            .child(gpui::batch_layer(bottom_row))
            .when_some(drag_band, |sheet, band| sheet.child(band))
            .when_some(dismiss_handle, |sheet, handle| sheet.child(handle));

        div()
            .key_context("ChordialImmersive")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
                if app.quality_dialog_open {
                    app.quality_dialog_open = false;
                } else if app.drawer_open {
                    app.close_drawer(cx);
                } else if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.exit_immersive(cx);
                }
                window.focus(&app.focus_handle);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &TogglePortrait, _, cx| {
                app.exit_immersive(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &ToggleEqualizer, _, cx| {
                if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.open_eq(cx);
                }
            }))
            .on_action(cx.listener(|app, _: &PlayPause, _, cx| app.toggle_playback(cx)))
            .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
            .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
            .on_action(cx.listener(|app, _: &PlayFirst, _, cx| app.play_first(cx)))
            .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                let y = f32::from(event.position.y);
                match event.pressed_button {
                    Some(gpui::MouseButton::Left) => app.update_sheet_drag(y, cx),
                    None => {
                        if app.sheet_drag.is_some_and(|drag| !drag.animating()) {
                            app.release_sheet_drag(height, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |app, _: &MouseUpEvent, _, cx| {
                    if app.sheet_drag.is_some() {
                        app.release_sheet_drag(height, cx);
                    }
                }),
            )
            .size_full()
            .relative()
            .overflow_hidden()
            .child(sheet)
            .into_any_element()
    }
}
