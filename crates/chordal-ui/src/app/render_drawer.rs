//! `ChordialApp` 的 render_drawer 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    /// 底部「更多」抽屉：布局 1 / 布局 2 共用。
    ///
    /// 一级页收纳定时、均衡器、歌词开关、队列、音频信息与艺术家；
    /// 定时选项是抽屉内的二级页，不再从 Dock 弹浮层。
    pub(super) fn render_drawer(
        &self,
        width: f32,
        height: f32,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let phone_main = self.phone_shell_active();
        let progress = self.drawer_progress(now);
        let panel_w = (width - 24.0).clamp(280.0, 520.0).min(width.max(1.0));
        let panel_h = ((height * 0.68).clamp(280.0, 540.0)).min((height - 12.0).max(180.0));
        let left = ((width - panel_w) * 0.5).max(0.0);
        let top = height - panel_h * progress;

        let row =
            |icon: crate::icons::Icon,
             id: SharedString,
             title: SharedString,
             detail: String,
             active: bool,
             on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>| {
                let mut labels = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .max_w_full()
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(title),
                    );
                if !detail.is_empty() {
                    labels = labels.child(
                        div()
                            .max_w_full()
                            .text_size(px(11.0))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(SharedString::from(detail)),
                    );
                }
                div()
                    .id(id)
                    .h(px(if phone_main { 52.0 } else { 46.0 }))
                    .w_full()
                    .flex_none()
                    .rounded(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(10.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover))
                    .child(crate::icons::icon(icon, 18.0, theme.text_dim))
                    .child(labels)
                    .when(active, |d| {
                        d.child(crate::icons::icon(
                            crate::icons::Icon::Check,
                            14.0,
                            theme.text,
                        ))
                    })
                    .on_click(on_click)
                    .into_any_element()
            };

        let (panel_title, back_row) = match self.drawer_page {
            DrawerPage::Root => (if phone_main { "菜单" } else { "更多" }, None),
            DrawerPage::Sleep => {
                let weak = weak.clone();
                let back = row(
                    crate::icons::Icon::ChevronLeft,
                    "drawer-back".into(),
                    "返回".into(),
                    String::new(),
                    false,
                    Box::new(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.drawer_page = DrawerPage::Root;
                            cx.notify();
                        });
                    }),
                );
                ("睡眠定时", Some(back))
            }
        };

        let mut body = div().w_full().flex().flex_col().gap(px(2.0));
        if let Some(back) = back_row {
            body = body.child(back);
        }
        match self.drawer_page {
            DrawerPage::Root => {
                // 一级导航在曲库页底部常驻；抽屉负责播放页/歌词页上的
                // 跳转，以及主题等次级入口。
                if phone_main {
                    let section = div()
                        .px(px(10.0))
                        .pt(px(4.0))
                        .pb(px(2.0))
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_faint)
                        .child("导航");
                    let current = self
                        .current_track
                        .as_ref()
                        .map(|t| t.title.clone())
                        .unwrap_or_else(|| "未在播放".into());
                    let player = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Play,
                            "drawer-phone-player".into(),
                            "正在播放".into(),
                            current,
                            self.phone_page == PhonePage::Player,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.close_drawer(cx);
                                    app.open_phone_sheet(cx);
                                    cx.notify();
                                });
                            }),
                        )
                    };
                    let lyrics = self.current_track.is_some().then(|| {
                        let weak = weak.clone();
                        let has_lyrics = self.lyric_view.read(cx).has_lyrics();
                        row(
                            crate::icons::Icon::Lyrics,
                            "drawer-phone-lyrics".into(),
                            "歌词".into(),
                            if has_lyrics {
                                "全屏滚动歌词".into()
                            } else {
                                "暂无歌词".into()
                            },
                            false,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.close_drawer(cx);
                                    app.open_lyrics_immersive(cx);
                                });
                            }),
                        )
                    });
                    let library = {
                        let weak = weak.clone();
                        let active = self.phone_page == PhonePage::Browse
                            && self.view_tab == ViewTab::Library
                            && self.library_filter == LibraryFilter::All;
                        row(
                            crate::icons::Icon::Note,
                            "drawer-phone-library".into(),
                            "所有音乐".into(),
                            format!("{} 首", self.library.len()),
                            active,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Browse;
                                    app.page = None;
                                    app.view_tab = ViewTab::Library;
                                    app.library_filter = LibraryFilter::All;
                                    app.source_route = None;
                                    app.close_drawer(cx);
                                    cx.notify();
                                });
                            }),
                        )
                    };
                    let search = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Search,
                            "drawer-phone-search".into(),
                            "搜索".into(),
                            "歌曲 / 专辑 / 歌手 / 歌单".into(),
                            self.phone_page == PhonePage::Browse
                                && self.view_tab == ViewTab::Search,
                            Box::new(move |_, window, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Browse;
                                    app.page = None;
                                    app.view_tab = ViewTab::Search;
                                    app.close_drawer(cx);
                                    focus_input(&app.search_input, window, cx);
                                    cx.notify();
                                });
                            }),
                        )
                    };
                    let settings = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Settings,
                            "drawer-phone-settings".into(),
                            "设置".into(),
                            "外观、播放、音源与存储".into(),
                            self.settings_open,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.hide_drawer_now();
                                    app.open_settings(cx);
                                });
                            }),
                        )
                    };
                    let theme_row = {
                        let weak = weak.clone();
                        let dark = Theme::get(cx).is_dark();
                        row(
                            if dark {
                                crate::icons::Icon::Sun
                            } else {
                                crate::icons::Icon::Moon
                            },
                            "drawer-phone-theme".into(),
                            if dark {
                                "切换到浅色".into()
                            } else {
                                "切换到深色".into()
                            },
                            String::new(),
                            false,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.toggle_appearance(cx);
                                    cx.notify();
                                });
                            }),
                        )
                    };
                    body = body.child(section).child(player);
                    if let Some(lyrics) = lyrics {
                        body = body.child(lyrics);
                    }
                    let source_section = div()
                        .px(px(10.0))
                        .pt(px(8.0))
                        .pb(px(2.0))
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_faint)
                        .child("音源");
                    let manage_sources = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Sliders,
                            "drawer-manage-sources".into(),
                            "管理音源".into(),
                            format!("{} 个实例", self.source_configs.len()),
                            matches!(self.page, Some(MainPage::ManageSources)),
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Browse;
                                    app.close_drawer(cx);
                                    app.open_source_manager(cx);
                                });
                            }),
                        )
                    };
                    body = body
                        .child(library)
                        .child(search)
                        .child(source_section)
                        .child(manage_sources);
                    for config in &self.source_configs {
                        let id = SourceId::new(config.id.clone());
                        let detail = if !config.enabled {
                            "已关闭".to_string()
                        } else if self.registry.get(&id).is_some() {
                            if config.is_local() {
                                format!(
                                    "本地库 · {} 个目录",
                                    config
                                        .local
                                        .as_ref()
                                        .map(|local| local.roots.len())
                                        .unwrap_or(0)
                                )
                            } else {
                                "已启用".to_string()
                            }
                        } else {
                            "未加载".to_string()
                        };
                        let weak_source = weak.clone();
                        let source_id = id.clone();
                        body = body.child(row(
                            if config.is_local() {
                                crate::icons::Icon::Drive
                            } else {
                                crate::icons::Icon::Cloud
                            },
                            SharedString::from(format!("drawer-source-{}", config.id)),
                            config.name.clone().into(),
                            detail,
                            matches!(self.page, Some(MainPage::Source(ref active)) if active == &id),
                            Box::new(move |_, _, cx| {
                                let _ = weak_source.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Browse;
                                    app.close_drawer(cx);
                                    app.open_source_page(source_id.clone(), cx);
                                });
                            }),
                        ));
                    }
                    let playlist_section = div()
                        .px(px(10.0))
                        .pt(px(8.0))
                        .pb(px(2.0))
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_faint)
                        .child("我的歌单");
                    body = body.child(playlist_section);
                    for summary in self.playlists.summaries() {
                        let weak_playlist = weak.clone();
                        let playlist_id = summary.id.clone();
                        body = body.child(row(
                            crate::icons::Icon::Layers,
                            SharedString::from(format!("drawer-playlist-{}", summary.id.0)),
                            summary.title.clone().into(),
                            format!("{} 首", summary.count),
                            matches!(self.page, Some(MainPage::Playlist(ref active)) if active == &summary.id),
                            Box::new(move |_, _, cx| {
                                let _ = weak_playlist.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Browse;
                                    app.page = Some(MainPage::Playlist(playlist_id.clone()));
                                    app.close_drawer(cx);
                                    cx.notify();
                                });
                            }),
                        ));
                    }
                    let create_playlist = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Plus,
                            "drawer-create-playlist".into(),
                            "新建歌单".into(),
                            "创建本地歌单".into(),
                            false,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Browse;
                                    app.close_drawer(cx);
                                    app.create_local_playlist(cx);
                                });
                            }),
                        )
                    };
                    body = body
                        .child(create_playlist)
                        .child(settings)
                        .child(theme_row)
                        .child(div().h(px(1.0)).my(px(4.0)).bg(theme.border));
                }
                let sleep_detail = self
                    .sleep_minutes
                    .map(|m| format!("{m} 分钟后暂停"))
                    .unwrap_or_else(|| "未设置".into());
                let sleep = {
                    let weak = weak.clone();
                    row(
                        crate::icons::Icon::Clock,
                        "drawer-sleep".into(),
                        "睡眠定时".into(),
                        sleep_detail,
                        self.sleep_minutes.is_some(),
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.drawer_page = DrawerPage::Sleep;
                                cx.notify();
                            });
                        }),
                    )
                };
                let eq_detail = format!(
                    "{} · {}",
                    self.eq.preset_name,
                    if self.eq.settings.enabled {
                        "已开启"
                    } else {
                        "已关闭"
                    }
                );
                let eq = {
                    let weak = weak.clone();
                    row(
                        crate::icons::Icon::Equalizer,
                        "drawer-eq".into(),
                        "均衡器".into(),
                        eq_detail,
                        self.eq.settings.enabled,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.hide_drawer_now();
                                app.open_eq(cx);
                            });
                        }),
                    )
                };
                let queue = {
                    let weak = weak.clone();
                    let active = phone_main
                        && self.phone_page == PhonePage::Browse
                        && self.view_tab == ViewTab::Library
                        && self.library_filter == LibraryFilter::Queue;
                    row(
                        crate::icons::Icon::Queue,
                        "drawer-queue".into(),
                        "播放队列".into(),
                        format!("{} 首", self.queue.len()),
                        active,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.hide_drawer_now();
                                if app.phone_shell_active() {
                                    app.phone_page = PhonePage::Browse;
                                    app.view_tab = ViewTab::Library;
                                    app.library_filter = LibraryFilter::Queue;
                                } else if app.immersive {
                                    // 队列与歌词共用右侧内容区，不再整页替换播放页。
                                    app.set_stage_view(StageView::Queue, cx);
                                } else {
                                    app.set_portrait_page(PortraitPage::Queue, cx);
                                }
                                cx.notify();
                            });
                        }),
                    )
                };
                body = body.child(sleep).child(eq);
                // 歌词入口与右下角按钮同源：只在沉浸层出现，窄窗/强制竖屏
                // 也显示，确保抽屉不是「收起歌词后唯一的恢复入口」失效。
                if self.immersive && self.lyric_view.read(cx).has_lyrics() {
                    let weak = weak.clone();
                    let visible = self.immersive_lyrics_visible(cx);
                    body = body.child(row(
                        crate::icons::Icon::Lyrics,
                        "drawer-lyrics".into(),
                        "歌词".into(),
                        if visible {
                            "显示中 · 点击收起".into()
                        } else {
                            "已收起 · 点击显示".into()
                        },
                        visible,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.close_drawer(cx);
                                app.toggle_immersive_lyrics(cx);
                            });
                        }),
                    ));
                }
                body = body.child(queue);

                let quality = self
                    .current_track
                    .as_ref()
                    .and_then(|t| t.quality.clone())
                    .unwrap_or_default();
                let source = self
                    .current_track
                    .as_ref()
                    .map(|t| match &t.origin {
                        TrackOrigin::Local { .. } => "本地音乐".to_string(),
                        TrackOrigin::Remote { source } => self
                            .source_config_by_id(&source.0)
                            .map(|config| config.name.clone())
                            .unwrap_or_else(|| source.0.clone()),
                    })
                    .unwrap_or_else(|| "—".into());
                body = body.child(
                    div()
                        .w_full()
                        .flex_none()
                        .rounded(px(10.0))
                        .bg(theme.surface_alt)
                        .border_1()
                        .border_color(theme.border)
                        .p(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child("音频信息"),
                        )
                        .child(div().text_size(px(11.0)).text_color(theme.text_dim).child(
                            SharedString::from(if quality.is_empty() {
                                "音质信息未知".to_string()
                            } else {
                                quality
                            }),
                        ))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.text_dim)
                                .child(SharedString::from(format!("音源：{source}"))),
                        ),
                );

                // 出自专辑 / 沉浸播放页：与参考视频「…」面板里的信息卡
                // 对齐，让曲目信息也能直接跳转。
                if let Some(album) = self
                    .current_track
                    .as_ref()
                    .and_then(|t| t.album.as_ref())
                    .filter(|a| a.id.is_some())
                {
                    let weak = weak.clone();
                    let name = album.name.clone();
                    body = body.child(row(
                        crate::icons::Icon::Layers,
                        "drawer-album".into(),
                        "出自专辑".into(),
                        name,
                        false,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.hide_drawer_now();
                                app.open_album_for_current(cx);
                            });
                        }),
                    ));
                }
                if self.phone_shell_active() {
                    let weak = weak.clone();
                    body = body.child(row(
                        crate::icons::Icon::Expand,
                        "drawer-immersive".into(),
                        "沉浸播放页".into(),
                        "全屏歌词与播放队列".into(),
                        false,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.hide_drawer_now();
                                app.open_portrait(cx);
                            });
                        }),
                    ));
                }

                let artists = self
                    .current_track
                    .as_ref()
                    .map(|t| t.artists.clone())
                    .unwrap_or_default();
                if !artists.is_empty() {
                    let mut card = div()
                        .w_full()
                        .flex_none()
                        .rounded(px(10.0))
                        .bg(theme.surface_alt)
                        .border_1()
                        .border_color(theme.border)
                        .p(px(10.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .px(px(2.0))
                                .pb(px(2.0))
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child("参与创作的艺术家"),
                        );
                    for artist in artists {
                        let weak_artist = weak.clone();
                        let linked = artist.id.is_some();
                        card = card.child(
                            div()
                                .id(SharedString::from(format!("drawer-artist-{}", artist.name)))
                                .h(px(if phone_main { 44.0 } else { 34.0 }))
                                .flex()
                                .items_center()
                                .gap(px(9.0))
                                .px(px(2.0))
                                .rounded(px(7.0))
                                .when(linked, |d| {
                                    d.cursor_pointer().hover(|s| s.bg(theme.surface_alt))
                                })
                                .child(
                                    div()
                                        .size(px(24.0))
                                        .rounded(px(12.0))
                                        .bg(theme.active)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(crate::icons::icon(
                                            crate::icons::Icon::Person,
                                            12.0,
                                            theme.text_faint,
                                        )),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(12.0))
                                        .text_color(theme.text)
                                        .text_ellipsis()
                                        .child(SharedString::from(artist.name.clone())),
                                )
                                .when(linked, |d| {
                                    d.on_click(move |_, window, cx| {
                                        let _ = weak_artist.update(cx, |app, cx| {
                                            app.hide_drawer_now();
                                            app.exit_immersive(cx);
                                            app.open_artist_for_current(cx);
                                            window.focus(&app.focus_handle);
                                            cx.notify();
                                        });
                                    })
                                }),
                        );
                    }
                    body = body.child(card);
                }

                // 「退出播放页」只属于沉浸模态；手机壳的曲库页由返回键
                // 和底部曲库按钮处理，不能再出现一个名字不符的入口。
                if self.immersive {
                    let exit = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Close,
                            "drawer-exit".into(),
                            "退出播放页".into(),
                            "Esc".into(),
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
                    body = body.child(exit);
                }
            }
            DrawerPage::Sleep => {
                for (label, minutes) in [
                    ("15 分钟", Some(15u32)),
                    ("30 分钟", Some(30u32)),
                    ("60 分钟", Some(60u32)),
                    ("关闭定时", None),
                ] {
                    let weak = weak.clone();
                    let active = self.sleep_minutes == minutes;
                    body = body.child(row(
                        crate::icons::Icon::Clock,
                        match minutes {
                            Some(15) => SharedString::from("drawer-sleep-15"),
                            Some(30) => SharedString::from("drawer-sleep-30"),
                            Some(60) => SharedString::from("drawer-sleep-60"),
                            _ => SharedString::from("drawer-sleep-off"),
                        },
                        SharedString::from(label),
                        if active {
                            "当前已启用".into()
                        } else {
                            String::new()
                        },
                        active,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.set_sleep_minutes(minutes, cx);
                                app.close_drawer(cx);
                            });
                        }),
                    ));
                }
            }
        }

        let header_close = {
            let weak = weak.clone();
            div()
                .id("drawer-close")
                .size(px(if phone_main { 40.0 } else { 28.0 }))
                .rounded(px(if phone_main { 20.0 } else { 14.0 }))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(theme.hover))
                .child(crate::icons::icon(
                    crate::icons::Icon::Close,
                    14.0,
                    theme.text_dim,
                ))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_drawer(cx));
                })
        };

        let panel = div()
            .id("immersive-drawer-panel")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(panel_w))
            .h(px(panel_h))
            .rounded(px(20.0))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.surface)
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div().w_full().pt(px(8.0)).flex().justify_center().child(
                    div()
                        .w(px(38.0))
                        .h(px(4.0))
                        .rounded(px(2.0))
                        .bg(theme.border_strong),
                ),
            )
            .child(
                div()
                    .w_full()
                    .px(px(16.0))
                    .pt(px(4.0))
                    .pb(px(8.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(panel_title),
                    )
                    .child(header_close),
            )
            .child(
                div()
                    .id("drawer-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(self.smooth_scroll, |d| d.smooth_scroll())
                    .px(px(10.0))
                    .pb(px(12.0))
                    .child(body),
            );

        let scrim = {
            let weak = weak.clone();
            div()
                .id("immersive-drawer-scrim")
                .absolute()
                .inset_0()
                .bg(gpui::black().opacity(0.38 * progress))
                .occlude()
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_drawer(cx));
                })
        };

        scrim.child(panel).into_any_element()
    }
}
