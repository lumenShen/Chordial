//! `ChordialApp` 的 lifecycle 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        registry: Arc<SourceRegistry>,
        player: Arc<Player>,
        local_source: Option<Arc<chordal_local::LocalSource>>,
        cache: Arc<ObjectCache>,
        search_timeout_ms: u64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let cfg = crate::config::load_or_default();
        let search_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("搜索歌曲、艺人或专辑");
            s
        });

        // 输入框视图是独立 entity：observe 输入状态，提交回调走根视图。
        let weak_self = cx.entity().downgrade();
        let weak_search = weak_self.clone();
        let search_input_view = cx.new(|cx| {
            InputView::new(search_input.clone(), cx).on_submit(move |text, _window, cx| {
                let _ = weak_search.update(cx, |app, cx| app.start_search(text, cx));
            })
        });

        // 设置面板的路径输入框：回车即应用。
        let settings_cache_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("对象缓存目录");
            s.set_content(cfg.cache_dir.display().to_string(), cx);
            s
        });
        let weak_cache_dir = weak_self.clone();
        let settings_cache_view = cx.new(|cx| {
            InputView::new(settings_cache_input.clone(), cx)
                .with_id("settings-cache-dir")
                .on_submit(move |text, _window, cx| {
                    let _ = weak_cache_dir.update(cx, |app, cx| app.apply_cache_dir(text, cx));
                })
        });
        let settings_download_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("下载目录");
            s.set_content(cfg.download_dir.display().to_string(), cx);
            s
        });
        let weak_download_dir = weak_self.clone();
        let settings_download_view = cx.new(|cx| {
            InputView::new(settings_download_input.clone(), cx)
                .with_id("settings-download-dir")
                .on_submit(move |text, _window, cx| {
                    let _ =
                        weak_download_dir.update(cx, |app, cx| app.apply_download_dir(text, cx));
                })
        });

        // 五路频段与内部控制点：每个频段两个频率输入和一个编号列表。
        let audio_band_ranges = cfg.audio_band_ranges.sanitized();
        let audio_band_controls = cfg.audio_band_controls.sanitized();
        player.set_audio_band_ranges(audio_band_ranges);
        const BAND_LOW_IDS: [&str; 5] = [
            "settings-band-rumble-low",
            "settings-band-kick-low",
            "settings-band-vocals-low",
            "settings-band-instruments-low",
            "settings-band-air-low",
        ];
        const BAND_HIGH_IDS: [&str; 5] = [
            "settings-band-rumble-high",
            "settings-band-kick-high",
            "settings-band-vocals-high",
            "settings-band-instruments-high",
            "settings-band-air-high",
        ];
        const BAND_CONTROL_IDS: [&str; 5] = [
            "settings-band-rumble-controls",
            "settings-band-kick-controls",
            "settings-band-vocals-controls",
            "settings-band-instruments-controls",
            "settings-band-air-controls",
        ];
        let mut settings_audio_low_inputs = Vec::with_capacity(5);
        let mut settings_audio_low_views = Vec::with_capacity(5);
        let mut settings_audio_high_inputs = Vec::with_capacity(5);
        let mut settings_audio_high_views = Vec::with_capacity(5);
        let mut settings_audio_control_inputs = Vec::with_capacity(5);
        let mut settings_audio_control_views = Vec::with_capacity(5);
        for (index, kind) in AudioBandKind::ALL.into_iter().enumerate() {
            let range = audio_band_ranges.get(kind);
            let low_input = cx.new(|cx| {
                let mut state = InputState::new(cx);
                state.set_placeholder("下限 Hz");
                state.set_content(format!("{:.0}", range.low_hz), cx);
                state
            });
            let weak_low = weak_self.clone();
            let low_view = cx.new(|cx| {
                InputView::new(low_input.clone(), cx)
                    .with_id(BAND_LOW_IDS[index])
                    .on_submit(move |_text, _window, cx| {
                        let _ = weak_low.update(cx, |app, cx| app.apply_audio_band(index, cx));
                    })
            });
            let high_input = cx.new(|cx| {
                let mut state = InputState::new(cx);
                state.set_placeholder("上限 Hz");
                state.set_content(format!("{:.0}", range.high_hz), cx);
                state
            });
            let weak_high = weak_self.clone();
            let high_view = cx.new(|cx| {
                InputView::new(high_input.clone(), cx)
                    .with_id(BAND_HIGH_IDS[index])
                    .on_submit(move |_text, _window, cx| {
                        let _ = weak_high.update(cx, |app, cx| app.apply_audio_band(index, cx));
                    })
            });
            let control_input = cx.new(|cx| {
                let mut state = InputState::new(cx);
                state.set_placeholder("如 7,8,12,13");
                state.set_content(audio_band_controls.get(kind).display(), cx);
                state
            });
            let weak_controls = weak_self.clone();
            let control_view = cx.new(|cx| {
                InputView::new(control_input.clone(), cx)
                    .with_id(BAND_CONTROL_IDS[index])
                    .on_submit(move |_text, _window, cx| {
                        let _ = weak_controls
                            .update(cx, |app, cx| app.apply_audio_band_controls(index, cx));
                    })
            });
            settings_audio_low_inputs.push(low_input);
            settings_audio_low_views.push(low_view);
            settings_audio_high_inputs.push(high_input);
            settings_audio_high_views.push(high_view);
            settings_audio_control_inputs.push(control_input);
            settings_audio_control_views.push(control_view);
        }
        let settings_audio_low_inputs = settings_audio_low_inputs.try_into().unwrap();
        let settings_audio_low_views = settings_audio_low_views.try_into().unwrap();
        let settings_audio_high_inputs = settings_audio_high_inputs.try_into().unwrap();
        let settings_audio_high_views = settings_audio_high_views.try_into().unwrap();
        let settings_audio_control_inputs = settings_audio_control_inputs.try_into().unwrap();
        let settings_audio_control_views = settings_audio_control_views.try_into().unwrap();

        // 音源管理二级编辑页的输入框。
        let source_edit_name_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("音源名称");
            s
        });
        let weak_source_name = weak_self.clone();
        let source_edit_name_view = cx.new(|cx| {
            InputView::new(source_edit_name_input.clone(), cx)
                .with_id("source-edit-name")
                .on_submit(move |text, _window, cx| {
                    let _ = weak_source_name.update(cx, |app, cx| app.apply_source_name(text, cx));
                })
        });
        let source_edit_url_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("http://127.0.0.1:3000");
            s
        });
        let weak_source_url = weak_self.clone();
        let source_edit_url_view = cx.new(|cx| {
            InputView::new(source_edit_url_input.clone(), cx)
                .with_id("source-edit-url")
                .on_submit(move |text, _window, cx| {
                    let _ = weak_source_url.update(cx, |app, cx| app.apply_source_url(text, cx));
                })
        });
        let source_edit_root_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("添加音乐目录，如 D:\\Music");
            s
        });
        let weak_source_root = weak_self.clone();
        let source_edit_root_view = cx.new(|cx| {
            InputView::new(source_edit_root_input.clone(), cx)
                .with_id("source-edit-root")
                .on_submit(move |text, _window, cx| {
                    let _ =
                        weak_source_root.update(cx, |app, cx| app.apply_source_add_root(text, cx));
                })
        });

        let source_playlist_rename_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("歌单名称");
            s
        });
        let weak_playlist_rename = weak_self.clone();
        let source_playlist_rename_view = cx.new(|cx| {
            InputView::new(source_playlist_rename_input.clone(), cx)
                .with_id("playlist-rename")
                .on_submit(move |text, _window, cx| {
                    let _ = weak_playlist_rename
                        .update(cx, |app, cx| app.apply_playlist_rename(text, cx));
                })
        });

        // 新版设置中心：独立实体，动作回到根视图统一应用。
        let settings_view = {
            let weak_settings = weak_self.clone();
            cx.new(|cx| {
                SettingsView::new(
                    move |action, window, cx| {
                        let _ = weak_settings.update(cx, |app, cx| {
                            app.handle_settings_action(action, window, cx);
                        });
                    },
                    cfg.clone(),
                    cx,
                )
            })
        };

        let login_phone_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("手机号");
            s
        });
        let weak_phone = weak_self.clone();
        let login_phone_view = cx.new(|cx| {
            InputView::new(login_phone_input.clone(), cx)
                .with_id("login-phone")
                .on_submit(move |_text, _window, cx| {
                    let _ = weak_phone.update(cx, |app, cx| app.send_login_captcha(cx));
                })
        });
        let login_captcha_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("短信验证码");
            s
        });
        let weak_captcha = weak_self.clone();
        let login_captcha_view = cx.new(|cx| {
            InputView::new(login_captcha_input.clone(), cx)
                .with_id("login-captcha")
                .on_submit(move |_text, _window, cx| {
                    let _ = weak_captcha.update(cx, |app, cx| app.submit_phone_login(cx));
                })
        });

        let theme = Theme::get(cx).clone();
        let appearance = theme.mode;
        let background_render_divisor =
            crate::config::clamp_background_render_divisor(cfg.background_render_divisor);
        let initial_bg_scale = 1.0 / background_render_divisor as f32;
        let clock: Arc<dyn PlaybackClock> = Arc::new(PlayerClock(player.clone()));
        let lyric_view = cx.new(|cx| {
            let mut view = LyricPlayerView::new(cx);
            view.set_clock(Some(clock));
            {
                let player = player.clone();
                view.set_seek_handler(Some(Arc::new(move |ms: u64| {
                    player.seek_ms(ms.saturating_add(50));
                })));
            }
            {
                // 五频段反应：视图每帧只读取一次整份快照，再分发给不同控制点。
                let player = player.clone();
                view.set_audio_source(Some(Arc::new(move || player.audio_bands())));
                view.set_audio_control_mapping(audio_band_controls);
            }
            view.set_colors(LyricRenderColors {
                text: theme.text,
                translation: theme.text_dim,
                overlay: gpui::rgba(0x00000066).into(),
                interlude_dot: theme.text,
                fallback_background: theme.bg,
                glow: theme.playing,
            });
            view.set_text_style(compact_lyric_style(&theme, cfg.lyric_sweep_fade_em));
            view.set_player_config(LyricPlayerConfig {
                align_position: 0.38,
                base_font_size: 44.0,
                non_dynamic_opacity: 0.60,
                ..Default::default()
            });
            view.set_background_fps(24);
            view.set_background_render_scale(initial_bg_scale);
            view.set_background_flow_speed(2.0);
            view.set_background_beat_strength(cfg.beat_strength);
            // 浅色主题用纸面纯色，避免深色 Mesh 与深色文字对比不足。
            if !theme.is_dark() {
                view.set_background_kind(BackgroundRendererKind::Css);
            }
            view.set_background_color(theme.bg);
            view
        });

        let eq_search_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("搜索耳机");
            s
        });
        let weak_eq = weak_self.clone();
        let eq_search_view = cx.new(|cx| {
            InputView::new(eq_search_input.clone(), cx)
                .with_id("eq-search")
                .on_submit(move |text, _window, cx| {
                    let _ = weak_eq.update(cx, |app: &mut ChordialApp, cx| {
                        app.eq.search = text.trim().to_string();
                        cx.notify();
                    });
                })
        });
        let eq = EqState::new(
            cfg.equalizer.clone(),
            cfg.eq_saved_presets.clone(),
            cfg.eq_recipe.clone(),
        );
        player.set_eq(eq.settings.clone());

        let persistence = crate::storage::JsonWriter::new();
        let playlists = PlaylistStore::load_default_with_writer(Some(persistence.clone()));
        let mut app = Self {
            registry,
            player,
            local_source: local_source.clone(),
            search_timeout_ms,
            cache,
            config: cfg.clone(),
            persistence,
            downloads: Arc::new(DownloadStore::open(cfg.download_dir.clone())),
            source_preference: cfg.source_preference,
            source_configs: cfg.sources.clone(),
            playlists,
            page: None,
            smooth_scroll: cfg.smooth_scroll,
            lyric_sweep_fade_em: crate::config::clamp_lyric_sweep_fade_em(cfg.lyric_sweep_fade_em),
            background_render_divisor,
            perf_overlay_on: cfg.perf_overlay,
            perf_window: None,
            settings_open: false,
            settings_gen: 0,
            settings_view,
            settings_cache_view,
            settings_download_view,
            settings_audio_low_inputs,
            settings_audio_low_views,
            settings_audio_high_inputs,
            settings_audio_high_views,
            settings_audio_control_inputs,
            settings_audio_control_views,
            source_edit_name_input,
            source_edit_name_view,
            source_edit_url_input,
            source_edit_url_view,
            source_edit_root_input,
            source_edit_root_view,
            source_playlist_rename_input,
            source_playlist_rename_view,
            source_user_playlists: HashMap::new(),
            source_user_playlists_loading: HashSet::new(),
            source_feeds: HashMap::new(),
            source_feed_detail: None,
            source_delete_confirm: None,
            playlist_delete_confirm: None,
            playlist_page_cache: RefCell::new(None),
            audio_band_ranges,
            audio_band_controls,
            account_state: AccountState::Unknown,
            account_avatar: None,
            user_playlists: Arc::new(Vec::new()),
            playlists_loading: false,
            login_open: false,
            login_source: None,
            login_tab: LoginTab::Qr,
            login_gen: 0,
            login_challenge: None,
            login_qr: None,
            login_status: None,
            login_phone_input,
            login_phone_view,
            login_captcha_input,
            login_captcha_view,
            captcha_cooldown_until: None,
            collection: None,
            collection_cover: None,
            collection_gen: 0,
            view_tab: ViewTab::Library,
            sidebar_open: true,
            search_input,
            search_input_view,
            search_kind: SearchKind::Track,
            search_collections: Arc::new(Vec::new()),
            search_gen: 0,
            searching: false,
            search_tracks: Arc::new(Vec::new()),
            search_reports: Vec::new(),
            search_elapsed_ms: 0,
            source_route: None,
            library: Arc::new(Vec::new()),
            library_scanning: false,
            library_scans: HashMap::new(),
            library_scan_epoch: 0,
            library_source_tracks: HashMap::new(),
            library_merge_gen: 0,
            library_filter: LibraryFilter::All,
            play_mode: PlayMode::Sequential,
            last_repeat_mode: PlayMode::Sequential,
            queue: Arc::new(Vec::new()),
            queue_pos: None,
            shuffle_plan: None,
            queue_refill: None,
            queue_gen: 0,
            queue_clear_confirm: false,
            player_status: chordal_player::Status::default(),
            status_message: None,
            info_message: None,
            focus_handle,
            lyric_view,
            current_track: None,
            lyrics_gen: 0,
            lyrics_status: None,
            play_gen: 0,
            appearance,
            cover_image: None,
            retired_images: Vec::new(),
            lyric_preview: Arc::new(Vec::new()),
            activated: false,
            viewport_width: 1280.0,
            viewport_height: 800.0,
            viewport_class: ViewportClass::Wide,
            phone_page: PhonePage::Browse,
            phone_backdrop: PhoneBackdrop::Off,
            phone_sheet: None,
            phone_sheet_anchor: None,
            phone_edge_swipe: None,
            phone_swipe_suppress_click: None,
            track_menu: None,
            playlist_picker: None,
            dock_menu: None,
            selection: None,
            long_press: None,
            long_press_pos: None,
            long_press_gen: 0,
            row_click_suppress_until: None,
            queue_drag: None,
            lyric_align: 0.35,
            no_blur: std::env::var_os("CHORDIAL_NO_BLUR").is_some(),
            no_bg: std::env::var_os("CHORDIAL_NO_BG").is_some(),
            immersive: false,
            sheet_drag: None,
            motion: crate::motion::MotionSystem::default(),
            stage_measurements: crate::app::stage_measure::StageMeasurements::default(),
            stage_request: None,
            motion_tuning: crate::motion::current_tuning(cx),
            immersive_hint_at: None,
            ui_anim_enter: None,
            ui_anim_play: None,
            ui_anim_shuffle: None,
            ui_anim_repeat: None,
            ui_anim_volume: None,
            last_volume_input: 0.9,
            show_remaining_time: true,
            quality_dialog_open: false,
            bench_anim: std::env::var_os("CHORDIAL_BENCH_ANIM").is_some(),
            force_portrait: false,
            portrait_page: PortraitPage::Art,
            portrait_transition: None,
            portrait_toast: None,
            drawer_open: false,
            drawer_page: DrawerPage::Root,
            wide_lyrics_hidden: false,
            compact_lyrics_open: false,
            stage_view: StageView::Lyrics,
            stage_view_menu_open: false,
            stage_last_input_at: Instant::now(),
            stage_last_pointer: None,
            stage_chrome_hidden: false,
            // 起点放在过去，避免应用刚启动的 260ms 内控件误淡入。
            stage_chrome_at: Instant::now() - Duration::from_secs(1),
            info_marquee: None,
            queue_scroll: gpui::UniformListScrollHandle::new(),
            track_list_scroll: gpui::UniformListScrollHandle::new(),
            playlist_scroll: gpui::UniformListScrollHandle::new(),
            collection_scroll: gpui::UniformListScrollHandle::new(),
            main_page_scroll: gpui::ScrollHandle::new(),
            now_scroll: gpui::ScrollHandle::new(),
            search_route_cache: RefCell::new(None),
            search_source_counts: RefCell::new(None),
            collection_route_cache: RefCell::new(None),
            row_text_cache: RefCell::new(None),
            track_covers: Arc::new(std::sync::Mutex::new(TrackCovers::new())),
            now_playing_text: RefCell::new(None),
            playlist_summaries: RefCell::new(None),
            source_snapshot: RefCell::new(None),
            sidebar_scroll: gpui::ScrollHandle::new(),
            sleep_minutes: None,
            sleep_until: None,
            sleep_gen: 0,
            eq_open: false,
            eq,
            eq_search_view,
            eq_chain_scroll: gpui::ScrollHandle::new(),
            eq_inspector_scroll: gpui::ScrollHandle::new(),
            progress_clock: (0, false, Instant::now()),
            eq_save_at: None,
            eq_save_gen: 0,
        };

        let _ = local_source;
        // 所有已启用的本地实例并发扫描，结果按源 ID 合并、按身份键去重。
        app.spawn_library_scan_all(cx);
        app.spawn_netease_warmup(cx);
        app.spawn_account_bootstrap(cx);
        app.spawn_status_poll(cx);
        if std::env::var_os("CHORDIAL_START_IMMERSIVE").is_some() {
            app.enter_immersive(cx);
        }
        // 诊断 / 截图：启动即打开设置中心。
        if std::env::var_os("CHORDIAL_START_SETTINGS").is_some() {
            app.open_settings(cx);
        }
        // 诊断 / 截图：启动即打开均衡器。
        if std::env::var_os("CHORDIAL_START_EQ").is_some() {
            app.open_eq(cx);
        }
        app
    }

    // ---------- 后台任务 ----------

    pub fn spawn_library_scan(
        &mut self,
        local: Arc<chordal_local::LocalSource>,
        cx: &mut Context<Self>,
    ) {
        self.spawn_library_scan_source(local, cx);
    }

    /// 播放状态轮询（200ms）：读快照、弹事件、必要时刷新 UI。
    /// 实体销毁（窗口关闭）后循环自动退出。
    pub(super) fn spawn_status_poll(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let alive = weak
                    .update(cx, |app, cx| {
                        let mut changed = false;
                        while let Some(ev) = app.player.try_pop_event() {
                            match ev {
                                PlayerEvent::TrackEnded(_) => {
                                    if !matches!(app.player.status().state, PlayerState::Failed(_))
                                    {
                                        app.on_track_ended(cx);
                                    }
                                    changed = true;
                                }
                                PlayerEvent::StreamError(msg) => {
                                    app.status_message = Some(format!("播放失败：{msg}"));
                                    changed = true;
                                }
                                PlayerEvent::SinkError(msg) => {
                                    app.status_message = Some(format!("输出设备错误：{msg}"));
                                    changed = true;
                                }
                            }
                        }
                        // 快照与上次不一致才重绘（避免空转重渲染）。
                        let status = app.player.status();
                        if status != app.player_status {
                            let was_playing = app.player_status.state == PlayerState::Playing;
                            let is_playing = status.state == PlayerState::Playing;
                            let cover_from = app.cover_scale_now(Instant::now());
                            app.player_status = status;
                            // AMLL 的播放/暂停 SVG 在状态切换时重新挂载，因此
                            // pressed 回弹与图标交换必须同帧发生。
                            if was_playing != is_playing {
                                app.start_cover_scale_anim(cover_from, is_playing);
                                if app.immersive {
                                    app.ui_anim_play = Some(Instant::now());
                                }
                                app.ensure_ui_anim_ticker(cx);
                            }
                            changed = true;
                        }
                        // 闲置收起复用这条 200ms 轮询，不再开第二条常驻定时循环。
                        changed |= app.tick_stage_chrome(cx);
                        if changed {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !alive {
                    return;
                }
            }
        })
        .detach();
    }

    /// 播放/暂停并立刻刷新状态与按钮回弹，不等待 200ms 轮询。
    pub(super) fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if matches!(self.player_status.state, PlayerState::Failed(_)) {
            self.resolve_and_play(cx);
            return;
        }
        if self.player_status.state == PlayerState::Loading {
            return;
        }
        let was_playing = self.player_status.state == PlayerState::Playing;
        self.player.toggle();
        let status = self.player.status();
        let is_playing = status.state == PlayerState::Playing;
        let cover_from = self.cover_scale_now(Instant::now());
        self.player_status = status;
        if was_playing != is_playing {
            self.start_cover_scale_anim(cover_from, is_playing);
            if self.immersive {
                // AMLL 的播放/暂停 SVG 在状态切换时重新挂载，pressed 回弹与
                // 图标交换必须在同一帧发生。
                self.ui_anim_play = Some(Instant::now());
            }
        }
        cx.notify();
    }

    // ---------- 搜索 ----------

    pub fn start_search(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.search_gen += 1;
        self.searching = true;
        self.view_tab = ViewTab::Search;
        self.show_phone_browse();
        cx.notify();

        let registry = self.registry.clone();
        let timeout = self.search_timeout_ms;
        let generation = self.search_gen;
        let kind = self.search_kind;
        let query = SearchQuery {
            text,
            limit: 100,
            offset: 0,
            kind,
        };
        if kind == SearchKind::Track {
            cx.spawn(async move |weak, cx| {
                let agg = match ChordialApp::run_background(async move {
                    registry.search_all(&query, timeout).await
                })
                .await
                {
                    Ok(agg) => agg,
                    Err(_) => {
                        let _ = weak.update(cx, |app, cx| {
                            if app.search_gen == generation {
                                app.searching = false;
                                app.status_message = Some("后台任务繁忙，请稍后重试搜索".into());
                                cx.notify();
                            }
                        });
                        return;
                    }
                };
                let _ = weak.update(cx, |app, cx| {
                    if app.search_gen != generation {
                        return; // 过期搜索，丢弃
                    }
                    app.search_tracks = Arc::new(agg.tracks);
                    app.search_collections = Arc::new(Vec::new());
                    app.search_reports = agg.per_source;
                    app.search_elapsed_ms = agg.total_elapsed_ms;
                    app.searching = false;
                    cx.notify();
                });
            })
            .detach();
        } else {
            cx.spawn(async move |weak, cx| {
                let agg = match ChordialApp::run_background(async move {
                    registry.search_collections_all(&query, timeout).await
                })
                .await
                {
                    Ok(agg) => agg,
                    Err(_) => {
                        let _ = weak.update(cx, |app, cx| {
                            if app.search_gen == generation {
                                app.searching = false;
                                app.status_message = Some("后台任务繁忙，请稍后重试搜索".into());
                                cx.notify();
                            }
                        });
                        return;
                    }
                };
                let _ = weak.update(cx, |app, cx| {
                    if app.search_gen != generation {
                        return;
                    }
                    app.search_tracks = Arc::new(Vec::new());
                    app.search_collections = Arc::new(agg.collections);
                    app.search_reports = Vec::new();
                    app.search_elapsed_ms = agg.elapsed_ms;
                    app.searching = false;
                    cx.notify();
                });
            })
            .detach();
        }
    }

    pub(super) fn set_search_kind(&mut self, kind: SearchKind, cx: &mut Context<Self>) {
        if self.search_kind == kind {
            return;
        }
        self.search_kind = kind;
        self.search_tracks = Arc::new(Vec::new());
        self.search_collections = Arc::new(Vec::new());
        self.search_reports.clear();
        self.search_elapsed_ms = 0;
        self.search_gen = self.search_gen.wrapping_add(1);
        let text = self.search_input.read(cx).content().trim().to_string();
        if !text.is_empty() {
            self.start_search(text, cx);
        } else {
            self.searching = false;
            self.view_tab = ViewTab::Search;
            self.show_phone_browse();
            cx.notify();
        }
    }

    /// 无边框窗口的拖动热区。
    ///
    /// Linux（Wayland/X11）由 gpui 向 compositor 发 move 请求；Windows 有两条
    /// 路径，同时保留：
    /// - `WindowControlArea::Drag` 让 `WM_NCHITTEST` 返回 `HTCAPTION`，由
    ///   DefWindowProc 接管拖动（贴边分屏、双击最大化都是原生行为）；
    /// - 鼠标按下时再调用 `start_window_move` 兜底。命中测试依赖上一帧的
    ///   `mouse_hit_test`，在刚切换页面/DPI 变化等时序下可能拿不到
    ///   HTCAPTION，此时客户区仍会收到 mouse-down，兜底路径保证一定能拖。
    ///   两条路径不会同时触发：返回 HTCAPTION 时客户区收不到 mouse-down。
    #[cfg(target_os = "windows")]
    pub(super) fn window_drag_area<E: gpui::InteractiveElement>(element: E) -> E {
        element
            .window_control_area(gpui::WindowControlArea::Drag)
            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                window.start_window_move();
            })
    }

    #[cfg(not(target_os = "windows"))]
    pub(super) fn window_drag_area<E: gpui::InteractiveElement>(element: E) -> E {
        element.on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
            window.start_window_move();
        })
    }

    /// 无边框窗口的 8 个 resize 热区；Wayland 下由 compositor 执行交互缩放。
    pub(super) fn resize_handles() -> Vec<AnyElement> {
        let edge = |id: &'static str,
                    edge: ResizeEdge,
                    style: CursorStyle,
                    top: Option<f32>,
                    bottom: Option<f32>,
                    left: Option<f32>,
                    right: Option<f32>,
                    w: Option<f32>,
                    h: Option<f32>| {
            let mut el = div().id(id).absolute().cursor(style);
            if let Some(v) = top {
                el = el.top(px(v));
            }
            if let Some(v) = bottom {
                el = el.bottom(px(v));
            }
            if let Some(v) = left {
                el = el.left(px(v));
            }
            if let Some(v) = right {
                el = el.right(px(v));
            }
            if let Some(v) = w {
                el = el.w(px(v));
            }
            if let Some(v) = h {
                el = el.h(px(v));
            }
            el.occlude()
                .on_mouse_down(gpui::MouseButton::Left, move |_, window, _| {
                    window.start_window_resize(edge);
                })
                .into_any_element()
        };
        vec![
            edge(
                "rz-top",
                ResizeEdge::Top,
                CursorStyle::ResizeUp,
                Some(0.0),
                None,
                Some(0.0),
                Some(0.0),
                None,
                Some(6.0),
            ),
            edge(
                "rz-bottom",
                ResizeEdge::Bottom,
                CursorStyle::ResizeDown,
                None,
                Some(0.0),
                Some(0.0),
                Some(0.0),
                None,
                Some(6.0),
            ),
            edge(
                "rz-left",
                ResizeEdge::Left,
                CursorStyle::ResizeLeft,
                Some(0.0),
                Some(0.0),
                Some(0.0),
                None,
                Some(6.0),
                None,
            ),
            edge(
                "rz-right",
                ResizeEdge::Right,
                CursorStyle::ResizeRight,
                Some(0.0),
                Some(0.0),
                None,
                Some(0.0),
                Some(6.0),
                None,
            ),
            edge(
                "rz-tl",
                ResizeEdge::TopLeft,
                CursorStyle::ResizeUpLeftDownRight,
                Some(0.0),
                None,
                Some(0.0),
                None,
                Some(10.0),
                Some(10.0),
            ),
            edge(
                "rz-tr",
                ResizeEdge::TopRight,
                CursorStyle::ResizeUpRightDownLeft,
                Some(0.0),
                None,
                None,
                Some(0.0),
                Some(10.0),
                Some(10.0),
            ),
            edge(
                "rz-bl",
                ResizeEdge::BottomLeft,
                CursorStyle::ResizeUpRightDownLeft,
                None,
                Some(0.0),
                Some(0.0),
                None,
                Some(10.0),
                Some(10.0),
            ),
            edge(
                "rz-br",
                ResizeEdge::BottomRight,
                CursorStyle::ResizeUpLeftDownRight,
                None,
                Some(0.0),
                None,
                Some(0.0),
                Some(10.0),
                Some(10.0),
            ),
        ]
    }
    pub(super) fn toggle_appearance(&mut self, cx: &mut Context<Self>) {
        let mode = match self.appearance {
            Appearance::Dark => Appearance::Light,
            Appearance::Light => Appearance::Dark,
        };
        self.appearance = mode;
        Theme::for_mode(mode).set_global(cx);
        // 歌词 view 的颜色/字体在构造时缓存，切换主题后需要重放。
        if self.phone_backdrop != PhoneBackdrop::Off {
            // 手机播放页的背景与文字是「封面驱动 + 白色」，不跟随明暗
            // 主题；重放主题样式会把它变回面板色，必须重新套用手机样式。
            self.phone_backdrop = PhoneBackdrop::Off;
            self.sync_phone_backdrop(cx);
        } else {
            let theme = Theme::get(cx).clone();
            // 沉浸层的字号/字重与面板不同；主题切换时必须按当前模式重放，
            // 否则沉浸层会被换成面板样式（字体高度/行高看起来会突然变化）。
            let style = if self.immersive {
                immersive_lyric_style(
                    self.viewport_width,
                    self.viewport_height,
                    self.lyric_sweep_fade_em,
                )
            } else {
                compact_lyric_style(&theme, self.lyric_sweep_fade_em)
            };
            let view = self.lyric_view.clone();
            view.update(cx, |view, _| {
                view.set_colors(LyricRenderColors {
                    text: theme.text,
                    translation: theme.text_dim,
                    overlay: theme.overlay,
                    interlude_dot: theme.text,
                    fallback_background: theme.bg,
                    glow: theme.playing,
                });
                view.set_text_style(style);
                view.set_background_kind(if theme.is_dark() {
                    BackgroundRendererKind::MeshGradient
                } else {
                    BackgroundRendererKind::Css
                });
                view.set_background_color(theme.bg);
            });
        }
        let mut cfg = self.config.clone();
        cfg.appearance = mode;
        self.commit_config(&cfg);
        cx.notify();
    }
}
