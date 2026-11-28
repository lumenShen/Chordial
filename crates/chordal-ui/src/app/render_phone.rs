//! `ChordialApp` 的 render_phone 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    /// 主面板的根节点：三个布局（宽屏 / 中等 / 手机）共用同一套快捷键、
    /// 焦点上下文与边框。只负责“外壳”，页面内容由调用方追加。
    pub(super) fn main_root(&mut self, background: bool, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::get(cx).clone();
        let mut root = div();
        if !background {
            root = root
                .key_context("ChordialMain")
                .track_focus(&self.focus_handle);
        }
        root.on_action(cx.listener(|app, _: &PlayPause, _window, cx| {
            app.toggle_playback(cx);
        }))
        .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
        .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
        .on_action(cx.listener(|app, _: &PlayFirst, _, cx| app.play_first(cx)))
        .on_action(cx.listener(|app, _: &FocusSearch, window, cx| {
            if app.is_phone() && !app.immersive {
                // 手机壳的搜索框在曲库页。切页与聚焦必须分两拍：本帧
                // 输入框还没挂载，focus 会被下一帧的布局丢掉。
                app.phone_page = PhonePage::Browse;
                app.view_tab = ViewTab::Search;
                let input = app.search_input.clone();
                cx.defer_in(window, move |_, window, cx| {
                    focus_input(&input, window, cx);
                });
                cx.notify();
            } else {
                focus_input(&app.search_input, window, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &RescanLibrary, _, cx| {
            app.spawn_library_scan_all(cx);
        }))
        .on_action(cx.listener(|app, _: &ToggleSidebar, _, cx| {
            // 手机壳没有侧栏；Ctrl+B 改成开合手机抽屉（同样的“导航”语义）。
            match (app.is_phone() && !app.immersive, app.drawer_open) {
                (true, true) => app.close_drawer(cx),
                (true, false) => app.open_drawer(cx),
                (false, _) => {
                    app.sidebar_open = !app.sidebar_open;
                    cx.notify();
                }
            }
        }))
        .on_action(cx.listener(|app, _: &OpenSettings, _, cx| {
            app.open_settings(cx);
        }))
        .on_action(cx.listener(|app, _: &CloseSettings, _, cx| {
            // Esc 在手机壳里兼作「关抽屉 / 返回播放」，与系统返回键同义。
            if app.quality_dialog_open {
                app.quality_dialog_open = false;
            } else if app.stage_view_menu_open {
                app.stage_view_menu_open = false;
            } else if app.settings_open {
                app.close_settings(cx);
            } else if app.drawer_open {
                app.close_drawer(cx);
            } else if app.dock_menu.is_some() {
                app.close_dock_menu(cx);
            } else if app.playlist_picker.is_some() {
                app.close_playlist_picker(cx);
            } else if app.track_menu.is_some() {
                app.close_track_menu(cx);
            } else if app.page.is_some() {
                app.close_main_page(cx);
            } else if app.selection.is_some() {
                app.exit_selection(cx);
            } else if app.is_phone() && app.phone_page == PhonePage::Player {
                app.close_phone_sheet(cx);
            } else if app.is_phone()
                && app.phone_page == PhonePage::Browse
                && app.current_track.is_some()
            {
                app.open_phone_sheet(cx);
                cx.notify();
            }
        }))
        .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
            if app.eq_open {
                app.close_eq(cx);
            } else if app.immersive {
                app.close_portrait(cx);
            }
            window.focus(&app.focus_handle);
            cx.notify();
        }))
        .on_action(cx.listener(|app, _: &TogglePortrait, window, cx| {
            if app.immersive {
                app.close_portrait(cx);
            } else {
                app.open_portrait(cx);
                window.focus(&app.focus_handle);
            }
            cx.notify();
        }))
        .on_action(cx.listener(|app, _: &ToggleEqualizer, window, cx| {
            if app.eq_open {
                app.close_eq(cx);
            } else {
                app.open_eq(cx);
                window.focus(&app.focus_handle);
            }
            cx.notify();
        }))
        .on_action(cx.listener(|app, _: &Quit, _, cx| {
            app.flush_persistence();
            app.cache.flush();
            // 先停掉托管 sidecar（不清理缓存、不注销账号），再退出。
            let registry = app.registry.clone();
            cx.spawn(async move |weak, cx| {
                for info in registry.loaded_sources() {
                    if let Some(source) = registry.get(&info.id) {
                        source.stop_services().await;
                    }
                }
                let _ = weak.update(cx, |_, cx| cx.quit());
            })
            .detach();
        }))
        .size_full()
        .relative()
        .flex()
        .flex_col()
        .bg(theme.bg)
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text)
    }

    /// 主面板（非沉浸层）。沉浸页下拉关闭时也会画在 sheet 背后，
    /// 让页面下移时露出熟悉的列表与播放条。
    ///
    /// `background = true` 时只作为静态背景：不注册键位上下文与焦点节点，
    /// 避免与同时渲染的沉浸层抢同一个 `focus_handle`。
    pub(super) fn render_main(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        background: bool,
    ) -> AnyElement {
        if !self.activated {
            self.activated = true;
            window.activate_window();
        }
        let scope = PerfScope::enter("ui.render.pass");
        let show_now = self.viewport_width >= crate::responsive::WIDE_MIN_W;
        let list_width = show_now.then(|| self.list_width());
        // 均衡器面板几乎铺满窗口：列表/正在播放完全落在面板背后，跳过
        // 它们的布局与绘制，EQ 列表滚动/预览时不再为不可见内容付费。
        // 侧栏左侧一窄条仍可见，保留。
        let covered_by_eq = self.eq_open
            && !self.settings_open
            && !self.login_open
            && !self.immersive
            && !background;

        let middle = div()
            .flex()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .when(self.sidebar_open, |d| {
                let start = Instant::now();
                let sidebar = self.render_sidebar(window.mouse_position(), cx);
                crate::frame_trace::add("main.sidebar", start.elapsed());
                d.child(gpui::batch_layer(sidebar))
            })
            .when(!covered_by_eq, |d| {
                let start = Instant::now();
                if self.page.is_some() {
                    // 音源页 / 管理音源 / 本地歌单：整块主内容区。
                    let page = self.render_main_page(cx);
                    crate::frame_trace::add("main.page", start.elapsed());
                    d.child(gpui::batch_layer(page))
                } else {
                    let list = self.render_list_panel(list_width, cx);
                    crate::frame_trace::add("main.list", start.elapsed());
                    d.child(gpui::batch_layer(list)).when(show_now, |d| {
                        let start = Instant::now();
                        let now_playing = self.render_now_playing(cx);
                        crate::frame_trace::add("main.now_playing", start.elapsed());
                        d.child(gpui::batch_layer(now_playing))
                    })
                }
            });

        let titlebar_start = Instant::now();
        let titlebar = self.render_titlebar(cx);
        crate::frame_trace::add("main.titlebar", titlebar_start.elapsed());
        let dock_start = Instant::now();
        let dock = self.render_dock(cx);
        crate::frame_trace::add("main.dock", dock_start.elapsed());
        let view = self
            .main_root(background, cx)
            .child(titlebar)
            .child(middle)
            .child(gpui::batch_layer(dock))
            .when(self.dock_menu.is_some(), |d| {
                d.child(self.render_dock_menu(cx))
            })
            .when(self.settings_open, |d| {
                let start = Instant::now();
                let settings = self.render_settings(cx);
                crate::frame_trace::add("main.settings", start.elapsed());
                d.child(settings)
            })
            .when(self.login_open, |d| d.child(self.render_login(cx)))
            .when(self.eq_open, |d| {
                let start = Instant::now();
                let eq = self.render_eq(window, cx);
                crate::frame_trace::add("main.eq", start.elapsed());
                d.child(gpui::batch_layer(eq))
            })
            .when(self.track_menu.is_some(), |d| {
                d.child(self.render_track_menu(cx))
            })
            .when(self.playlist_picker.is_some(), |d| {
                d.child(self.render_playlist_picker(cx))
            })
            .when(self.queue_drag.is_some(), |d| {
                d.child(
                    div()
                        .id("queue-drag-capture-wide")
                        .absolute()
                        .inset_0()
                        .occlude()
                        .cursor(CursorStyle::ClosedHand)
                        .on_mouse_move(cx.listener(|app, event: &MouseMoveEvent, _, cx| {
                            if event.pressed_button == Some(gpui::MouseButton::Left) {
                                app.update_queue_drag(f32::from(event.position.y), cx);
                            }
                        }))
                        .on_mouse_up(
                            gpui::MouseButton::Left,
                            cx.listener(|app, _: &MouseUpEvent, _, cx| {
                                app.release_queue_drag(cx);
                            }),
                        ),
                )
            })
            .children(Self::resize_handles());
        scope.finish(1);
        self.stage_measurements
            .observe(StageMode::Dock, view, &[(2, MeasurePart::Card)])
            .into_any_element()
    }

    // ================= 手机壳（单列） =================

    /// 手机壳：没有侧栏/右栏/Dock，播放页 / 歌词页 / 曲库页三选一。
    /// 曲库页底部有常驻一级导航；抽屉承载次级入口与播放页/歌词页跳转。
    pub(super) fn render_phone_main(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        background: bool,
    ) -> AnyElement {
        if !self.activated && !background {
            self.activated = true;
            window.activate_window();
        }
        let scope = PerfScope::enter("ui.render.pass");
        let now = Instant::now();
        self.sync_phone_backdrop(cx);
        let width = self.viewport_width;
        let height = self.viewport_height;
        let theme = Theme::get(cx).clone();
        let progress = self.phone_sheet_progress(now);

        // 播放页是覆盖在曲库页上的卡片：拖动时曲库页留下做背景，
        // 松手后完全展开/收起。歌词页仍是独立整页（共用同一个
        // lyric_view，不能和播放页背景同时挂载）。
        let mut content = div().size_full().relative().overflow_hidden();
        if self.page.is_some() {
            content = content.child(self.render_phone_full_page(cx));
        } else {
            match self.phone_page {
                PhonePage::Player if self.current_track.is_some() => {
                    if progress < 0.999 {
                        content = content.child(self.render_phone_browse(cx)).child(
                            div()
                                .absolute()
                                .inset_0()
                                .occlude()
                                .bg(gpui::black().opacity(0.42 * progress)),
                        );
                    }
                    content = content.child(self.render_phone_player(progress, window, cx));
                }
                _ => {
                    content = content.child(self.render_phone_browse(cx));
                }
            }
        }

        // 手势捕获层：拖动开始后指针可能离开封面/把手，必须在整窗范围
        // 内继续接收 move/up，才能保证跟手与松手判定连续。
        if self.phone_sheet.is_some() || self.phone_edge_swipe.is_some() {
            content = content.child(
                div()
                    .id("phone-gesture-capture")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .cursor(CursorStyle::ClosedHand)
                    .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                        if event.pressed_button != Some(gpui::MouseButton::Left) {
                            return;
                        }
                        let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
                        if app.phone_sheet.is_some() {
                            app.update_phone_sheet_drag(y, cx);
                        } else if app.phone_edge_swipe.is_some() {
                            app.update_phone_edge_swipe(x, y, cx);
                        }
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(move |app, _: &MouseUpEvent, _, cx| {
                            if app.phone_sheet.is_some() {
                                app.release_phone_sheet_drag(cx);
                            }
                            if app.phone_edge_swipe.is_some() {
                                app.release_phone_edge_swipe(cx);
                            }
                        }),
                    ),
            );
        }

        // 队列拖动排序：整窗捕获，行本身只负责起手。
        if self.queue_drag.is_some() {
            content = content.child(
                div()
                    .id("queue-drag-capture")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .cursor(CursorStyle::ClosedHand)
                    .on_mouse_move(cx.listener(|app, event: &MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(gpui::MouseButton::Left) {
                            app.update_queue_drag(f32::from(event.position.y), cx);
                        }
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|app, _: &MouseUpEvent, _, cx| {
                            app.release_queue_drag(cx);
                        }),
                    ),
            );
        }

        let phone_message = self
            .status_message
            .clone()
            .or_else(|| self.info_message.clone());
        if let Some(message) = phone_message {
            let weak_dismiss = cx.entity().downgrade();
            content = content.child(
                div()
                    .absolute()
                    .top(px(48.0))
                    .left(px(12.0))
                    .right(px(12.0))
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(theme.border_strong)
                    .bg(theme.surface_alt)
                    .shadow(vec![BoxShadow {
                        color: theme.shadow,
                        offset: point(px(0.0), px(4.0)),
                        blur_radius: px(14.0),
                        spread_radius: px(0.0),
                    }])
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Alert,
                        14.0,
                        theme.text_dim,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.5))
                            .text_color(theme.text)
                            .child(message),
                    )
                    .child(
                        div()
                            .id("dismiss-phone-message")
                            .size(px(32.0))
                            .flex_none()
                            .rounded(px(6.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.hover))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Close,
                                12.0,
                                theme.text_dim,
                            ))
                            .on_click(move |_, _, cx| {
                                let _ = weak_dismiss.update(cx, |app, cx| {
                                    app.status_message = None;
                                    app.info_message = None;
                                    cx.notify();
                                });
                            }),
                    ),
            );
        }

        if let Some((text, started)) = self.portrait_toast.clone()
            && started.elapsed() < Duration::from_millis(1800)
        {
            content = content.child(
                div()
                    .absolute()
                    .bottom(px(126.0))
                    .left(px(((width - 300.0) / 2.0).max(12.0)))
                    .w(px(300.0_f32.min(width - 24.0)))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .rounded(px(16.0))
                            .bg(theme.surface_alt)
                            .border_1()
                            .border_color(theme.border_strong)
                            .px(px(14.0))
                            .py(px(7.0))
                            .text_size(px(11.5))
                            .text_color(theme.text)
                            .child(SharedString::from(text)),
                    ),
            );
        }

        if self.page.is_none()
            && self.phone_page == PhonePage::Player
            && self.current_track.is_some()
        {
            let sheet_index = if progress < 0.999 { 2 } else { 0 };
            content = self.stage_measurements.observe(
                StageMode::PhonePlayer,
                content,
                &[(sheet_index, MeasurePart::Card)],
            );
        }
        let mut view = self
            .main_root(background, cx)
            .child(div().flex_1().min_h_0().overflow_hidden().child(content))
            .when(!background && self.drawer_visible(), |d| {
                d.child(self.render_drawer(width, height, now, cx))
            })
            .when(!background && self.settings_open, |d| {
                d.child(self.render_settings(cx))
            })
            .when(!background && self.login_open, |d| {
                d.child(self.render_login(cx))
            })
            .when(!background && self.eq_open, |d| {
                d.child(gpui::batch_layer(self.render_eq(window, cx)))
            })
            .when(!background && self.track_menu.is_some(), |d| {
                d.child(self.render_track_menu(cx))
            })
            .children(Self::resize_handles());
        if !background && self.stage_view_menu_open {
            let bottom_rect =
                stage_bottom_rect(width, height, false, stable_lyric_pad(width, height));
            let menu_right = (width - bottom_rect.x - bottom_rect.w).max(0.0);
            let menu_bottom = (height - bottom_rect.y + 8.0).max(0.0);
            view = view
                .child(
                    div()
                        .id("phone-stage-view-scrim")
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|app, _, _, cx| {
                                app.stage_view_menu_open = false;
                                cx.notify();
                            }),
                        ),
                )
                .child(self.render_stage_view_menu(menu_right, menu_bottom, cx));
        }
        scope.finish(1);
        view.into_any_element()
    }

    /// 手机播放页卡片。`progress` 0 = 收进 mini player，1 = 全屏。
    pub(super) fn render_phone_player(
        &mut self,
        progress: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_progress_clock();
        let width = self.viewport_width;
        let viewport_h = self.viewport_height;
        let p = progress.clamp(0.0, 1.0);
        let top = ((1.0 - p) * viewport_h).max(0.0);
        let inset = (1.0 - p) * 10.0;
        let sheet_width = (width - inset * 2.0).max(1.0);
        let height = (viewport_h - top).max(1.0);
        let radius = 22.0 * (1.0 - p);

        let cover = compact_cover_size(sheet_width, height);
        let left = ((sheet_width - cover) * 0.5).max(0.0);
        let visual = crate::stage_chrome::StageChromeVisual::from_progress(1.0);
        let label_font = (height * 0.012).max(12.8);
        let btn = stage_button_size(cover);
        let bottom_rect = StageRect {
            x: left,
            y: height - crate::layout_morph::COMPACT_BOTTOM - 44.0,
            w: cover,
            h: 44.0,
        };
        let controls_top = COMPACT_TOP + cover + COMPACT_COVER_GAP;
        let controls_h = (bottom_rect.y - controls_top).max(0.0);
        let volume_h = if stage_volume_visible(height) {
            STAGE_VOLUME_H
        } else {
            0.0
        };
        let gap_count = if volume_h > 0.0 { 4 } else { 3 };
        let controls_layout = render_stage::stage_controls_layout(
            controls_top,
            controls_h,
            amll_info_height(self.viewport_width, height),
            gap_count,
            left,
            cover,
            btn,
            volume_h,
            bottom_rect,
        );
        let bottom_row = div()
            .w(px(bottom_rect.w))
            .h(px(bottom_rect.h))
            .child(self.phone_bottom_row(cover, cx))
            .into_any_element();
        let controls_group = self.stage_controls_group(
            controls_layout,
            height,
            label_font,
            btn,
            visual,
            bottom_row,
            cx,
        );

        // 不在手机播放/歌词态时，绝不能再把同一个 lyric_view 塞进第二
        // 棵树里。用静态渐变兜底，等沉浸层交还后再切回 Mesh。
        let backdrop: AnyElement = if !self.no_bg && self.phone_backdrop != PhoneBackdrop::Off {
            div()
                .absolute()
                .inset_0()
                .bg(gpui::black())
                .child(self.lyric_view.clone())
                .into_any_element()
        } else {
            div()
                .absolute()
                .inset_0()
                .bg(linear_gradient(
                    165.0,
                    linear_color_stop(crate::theme::hex(0x232329), 0.0),
                    linear_color_stop(crate::theme::hex(0x08080A), 1.0),
                ))
                .into_any_element()
        };

        let now = Instant::now();
        let swipe_offset = self
            .phone_edge_swipe
            .as_ref()
            .map(|swipe| swipe.offset(now))
            .unwrap_or(0.0);
        let suppress_click = self.phone_swipe_click_suppressed();

        let sheet =
            div()
                .absolute()
                .top(px(top))
                .left(px(inset))
                .right(px(inset))
                .bottom(px(0.0))
                .rounded_t(px(radius))
                .overflow_hidden()
                .shadow(vec![BoxShadow {
                    color: gpui::black().opacity(0.35 * (1.0 - p)),
                    blur_radius: px(28.0),
                    spread_radius: px(0.0),
                    offset: point(px(0.0), px(-8.0)),
                }])
                .child(backdrop)
                .child(self.render_phone_titlebar(true, false, cx))
                // 标题栏不再整条注册窗口拖动，改由把手两侧的窄带承担；
                // 这样把手、收起按钮和菜单都能收到真实点击。
                .child(Self::window_drag_area(
                    div()
                        .id("phone-sheet-window-drag-right")
                        .absolute()
                        .top(px(0.0))
                        .left(px((sheet_width + 140.0) * 0.5 + 6.0))
                        .right(px(44.0))
                        .h(px(36.0))
                        .cursor(CursorStyle::OpenHand),
                ))
                .children(((sheet_width - 140.0) * 0.5 > 116.0).then(|| {
                    Self::window_drag_area(
                        div()
                            .id("phone-sheet-window-drag-left")
                            .absolute()
                            .top(px(0.0))
                            .left(px(104.0))
                            .w(px((sheet_width - 140.0) * 0.5 - 108.0))
                            .h(px(36.0))
                            .cursor(CursorStyle::OpenHand),
                    )
                }))
                // 收起播放页：把手之外再给一个明确的按钮，触屏/鼠标都能用。
                .child(
                    div()
                        .id("phone-sheet-collapse")
                        .absolute()
                        .left(px(66.0))
                        .top(px(0.0))
                        .size(px(44.0))
                        .rounded(px(22.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .opacity(0.75)
                        .hover(|s| s.bg(gpui::white().opacity(0.12)).opacity(1.0))
                        .child(crate::icons::icon(
                            crate::icons::Icon::ChevronDown,
                            16.0,
                            gpui::white(),
                        ))
                        .on_click(cx.listener(|app, _, _, cx| app.close_phone_sheet(cx))),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(left + swipe_offset))
                        .top(px(COMPACT_TOP))
                        .w(px(cover))
                        .child(self.amll_player_column(
                            cover, height, true, visual, false, false, window, cx,
                        )),
                )
                .child(controls_group)
                // 封面是手机播放页最自然的「打开歌词」入口；横向拖动则切换
                // 封面 ⇄ 歌词（与参考视频的横滑一致）。
                .child(
                    div()
                        .id("phone-cover-lyrics")
                        .absolute()
                        .left(px(left + swipe_offset))
                        .top(px(COMPACT_TOP))
                        .w(px(cover))
                        .h(px(cover))
                        .cursor_pointer()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|app, event: &MouseDownEvent, _, cx| {
                                app.begin_phone_edge_swipe(
                                    f32::from(event.position.x),
                                    f32::from(event.position.y),
                                    cx,
                                );
                            }),
                        )
                        .on_mouse_up(
                            gpui::MouseButton::Left,
                            cx.listener(|app, _: &MouseUpEvent, _, cx| {
                                if app.phone_edge_swipe.is_some() {
                                    app.release_phone_edge_swipe(cx);
                                }
                            }),
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if suppress_click || app.phone_swipe_click_suppressed() {
                                return;
                            }
                            app.open_lyrics_immersive(cx);
                        })),
                )
                // 顶部把手：只接管一条 140×36 的命中区，避免和红绿灯、
                // Windows 原生窗口拖动区打架。
                .child(
                    div()
                        .id("phone-sheet-handle")
                        .absolute()
                        .top(px(2.0))
                        .left(px((sheet_width - 140.0) * 0.5))
                        .w(px(140.0))
                        .h(px(36.0))
                        .flex()
                        .justify_center()
                        .cursor(CursorStyle::OpenHand)
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|app, event: &MouseDownEvent, _, cx| {
                                app.begin_phone_sheet_drag(f32::from(event.position.y), cx);
                            }),
                        )
                        .on_mouse_up(
                            gpui::MouseButton::Left,
                            cx.listener(|app, _: &MouseUpEvent, _, cx| {
                                if app.phone_sheet.is_some() {
                                    app.release_phone_sheet_drag(cx);
                                }
                            }),
                        ),
                );

        sheet.into_any_element()
    }

    /// 手机曲库页：桌面曲目板直接铺满，外层只负责标题栏与返回播放。
    /// 底部常驻一级导航条；mini player 悬浮在导航条上方。多选时换成批量操作条。
    pub(super) fn render_phone_browse(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let selecting = self.selection.is_some();
        let bar = if selecting {
            self.render_phone_selection_bar(cx)
        } else {
            self.render_phone_titlebar(false, false, cx)
        };
        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .child(bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_list_panel(None, cx)),
            );
        if selecting {
            root = root.child(self.render_phone_selection_actions(cx));
        } else {
            if self.current_track.is_some() {
                root = root.child(self.render_phone_mini_player(cx));
            }
            // 一级导航常驻：曲库 / 队列 / 搜索 / 设置。
            root = root.child(self.render_phone_bottom_nav(cx));
        }
        // mini player 上按住后，指针可能很快离开小条；从这里开始整窗
        // 接管，越过死区才真正展开播放页。
        if self.phone_sheet_anchor.is_some() {
            root = root.child(
                div()
                    .id("phone-mini-drag-capture")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .cursor(CursorStyle::ClosedHand)
                    .on_mouse_move(cx.listener(|app, event: &MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(gpui::MouseButton::Left) {
                            app.drag_phone_mini_player(f32::from(event.position.y), cx);
                        }
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|app, _: &MouseUpEvent, _, cx| {
                            app.release_phone_mini_player(cx);
                        }),
                    ),
            );
        }
        if !selecting && self.current_track.is_some() {
            root = self.stage_measurements.observe(
                StageMode::MiniPlayer,
                root,
                &[(2, MeasurePart::Card)],
            );
        }
        root.into_any_element()
    }

    fn render_phone_full_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .child(self.render_phone_titlebar(false, false, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_main_page(cx)),
            )
            .when(self.current_track.is_some(), |d| {
                d.child(self.render_phone_mini_player(cx))
            })
            .child(self.render_phone_bottom_nav(cx));
        if self.current_track.is_some() {
            self.stage_measurements
                .observe(StageMode::MiniPlayer, root, &[(2, MeasurePart::Card)])
                .into_any_element()
        } else {
            root.into_any_element()
        }
    }

    /// 手机壳底部 mini player。上滑/点击展开播放页，右键打开曲目菜单。
    pub(super) fn render_phone_mini_player(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let (title, artist) = self.dock_track_text();
        let playing = self.player_status.state == PlayerState::Playing;
        let failed = matches!(self.player_status.state, PlayerState::Failed(_));
        let loading = self.player_status.state == PlayerState::Loading;
        let duration = self.player_status.duration_ms.unwrap_or(0);
        let position = self.display_position_ms().min(duration.max(1));
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };
        let cover = crate::widgets::cover_thumb(
            &theme,
            self.cover_image.clone(),
            42.0,
            1.0,
            crate::icons::icon(crate::icons::Icon::Note, 16.0, theme.text_faint),
        );
        let current_index = self.queue_pos.filter(|pos| *pos < self.queue.len());
        let open_index = current_index.or_else(|| (!self.queue.is_empty()).then_some(0));
        let body = {
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(cover)
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
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .text_ellipsis()
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(if failed {
                                    theme.danger
                                } else {
                                    theme.text_faint
                                })
                                .text_ellipsis()
                                .child(artist),
                        ),
                )
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|app, event: &MouseDownEvent, _, cx| {
                        app.press_phone_mini_player(f32::from(event.position.y), cx);
                    }),
                )
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|app, _: &MouseUpEvent, _, cx| {
                        app.release_phone_mini_player(cx);
                    }),
                )
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |app, event: &MouseDownEvent, _, cx| {
                        if let Some(index) = open_index {
                            app.open_track_menu(
                                QueueList::Queue,
                                index,
                                f32::from(event.position.x),
                                f32::from(event.position.y),
                                cx,
                            );
                        }
                    }),
                )
        };
        let body = self.stage_measurements.observe(
            StageMode::MiniPlayer,
            body,
            &[(0, MeasurePart::Cover), (1, MeasurePart::Info)],
        );
        let play = {
            let weak = weak.clone();
            div()
                .id("phone-mini-play")
                .size(px(44.0))
                .rounded(px(22.0))
                .when(failed, |d| d.bg(theme.danger))
                .when(loading, |d| d.bg(theme.disabled_bg))
                .flex()
                .items_center()
                .justify_center()
                .cursor(if loading {
                    CursorStyle::Arrow
                } else {
                    CursorStyle::PointingHand
                })
                .hover(|s| {
                    if failed {
                        s.bg(theme.danger.opacity(0.85))
                    } else if loading {
                        s
                    } else {
                        s.bg(theme.hover)
                    }
                })
                .child(crate::icons::icon(
                    if playing {
                        crate::icons::Icon::Pause
                    } else {
                        crate::icons::Icon::Play
                    },
                    14.0,
                    if failed {
                        gpui::white()
                    } else if loading {
                        theme.disabled_fg
                    } else {
                        theme.text
                    },
                ))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = weak.update(cx, |app, cx| app.toggle_playback(cx));
                })
        };
        let queue = {
            let weak = weak.clone();
            div()
                .id("phone-mini-queue")
                .size(px(44.0))
                .flex_none()
                .rounded(px(10.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .child(crate::icons::icon(
                    crate::icons::Icon::Queue,
                    15.0,
                    theme.text_dim,
                ))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Browse;
                        app.view_tab = ViewTab::Library;
                        app.library_filter = LibraryFilter::Queue;
                        app.phone_sheet = None;
                        app.motion.phone_sheet = None;
                        cx.notify();
                    });
                })
        };
        let more = {
            div()
                .id("phone-mini-more")
                .size(px(44.0))
                .flex_none()
                .rounded(px(10.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .child(crate::icons::icon(
                    crate::icons::Icon::More,
                    15.0,
                    theme.text_dim,
                ))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |app, event: &MouseDownEvent, _, cx| {
                        if let Some(index) = open_index {
                            app.open_track_menu(
                                QueueList::Queue,
                                index,
                                f32::from(event.position.x),
                                f32::from(event.position.y),
                                cx,
                            );
                        }
                    }),
                )
                .on_click(|_, _, cx| cx.stop_propagation())
        };
        div()
            .id("phone-mini-player")
            .absolute()
            .left(px(12.0))
            .right(px(12.0))
            .bottom(px(Self::PHONE_NAV_H + 8.0))
            .h(px(62.0))
            .rounded(px(16.0))
            .overflow_hidden()
            .bg(theme.surface.opacity(0.96))
            .border_1()
            .border_color(theme.border)
            .shadow(vec![BoxShadow {
                color: theme.shadow_strong,
                blur_radius: px(24.0),
                spread_radius: px(0.0),
                offset: point(px(0.0), px(-4.0)),
            }])
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(8.0))
            .pr(px(8.0))
            .child(body)
            .child(play)
            .child(queue)
            .child(more)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(2.0))
                    .bg(theme.border)
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative(progress.clamp(0.0, 1.0)))
                            .bg(theme.accent.opacity(0.72)),
                    ),
            )
            .on_click(cx.listener(|app, _, _, cx| app.open_phone_sheet(cx)))
            .into_any_element()
    }
}
