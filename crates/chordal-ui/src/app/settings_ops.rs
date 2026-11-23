//! `ChordialApp` 的 settings_ops 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = true;
        let stats = self.cache.stats();
        let runtime = SettingsRuntime {
            cache_root: stats.root,
            cache_used_bytes: stats.total_bytes,
            cache_limit_bytes: stats.limit_bytes,
            cache_entries: stats.entries,
            download_root: self.downloads.root().to_path_buf(),
            eq_name: self.eq.display_name(),
            eq_band_count: self.eq.settings.bands.len(),
            eq_enabled: self.eq.settings.enabled,
            backend: self.player.backend_name().to_string(),
        };
        self.settings_view.update(cx, |view, cx| {
            view.sync(runtime, cx);
            view.sync_memory(mem::snapshot(), cx);
        });

        // 设置页打开期间每秒刷新一次内存诊断（关闭/重新打开时旧循环自动退出）。
        // 只更新诊断快照，不重写输入框，因此不会打断用户编辑。
        self.settings_gen = self.settings_gen.wrapping_add(1);
        let generation = self.settings_gen;
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let keep = weak
                    .update(cx, |app, cx| {
                        if !app.settings_open || app.settings_gen != generation {
                            return false;
                        }
                        let snapshot = mem::snapshot();
                        app.settings_view
                            .update(cx, |view, cx| view.sync_memory(snapshot, cx));
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        cx.notify();
    }

    // ---------- 画面性能浮窗 ----------

    /// 设置页开关 → 持久化配置 + 开/关独立浮窗。
    pub(super) fn set_perf_overlay(&mut self, on: bool, cx: &mut Context<Self>) {
        self.perf_overlay_on = on;
        let mut cfg = crate::config::load_or_default();
        if cfg.perf_overlay != on {
            cfg.perf_overlay = on;
            crate::config::save(&cfg);
        }
        if on {
            self.open_perf_overlay(cx);
        } else {
            self.close_perf_overlay(cx);
        }
        cx.notify();
    }

    /// 打开性能浮窗。已经打开时幂等（不重复开窗）。
    pub(crate) fn open_perf_overlay(&mut self, cx: &mut Context<Self>) {
        if self.perf_window.is_some() {
            return;
        }
        let app = cx.entity().downgrade();
        let options = crate::perf_overlay::window_options(cx);
        match cx.open_window(options, move |window, cx| {
            cx.new(|cx| crate::perf_overlay::PerfOverlayView::new(app, window, cx))
        }) {
            Ok(handle) => {
                self.perf_window = Some(handle.into());
            }
            Err(error) => {
                tracing::warn!(%error, "perf overlay window open failed");
                self.perf_overlay_on = false;
                self.perf_window = None;
                let mut cfg = crate::config::load_or_default();
                if cfg.perf_overlay {
                    cfg.perf_overlay = false;
                    crate::config::save(&cfg);
                }
                self.settings_view
                    .update(cx, |view, cx| view.set_perf_overlay(false, cx));
                self.info_message = Some("无法打开性能浮窗".into());
            }
        }
        cx.notify();
    }

    /// 由设置开关关闭浮窗。句柄在 update 前取出，避免窗口关闭回调重入。
    pub(super) fn close_perf_overlay(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.perf_window.take() {
            let _ = handle.update(cx, |_view, window, _cx| window.remove_window());
        }
        crate::perf_overlay::set_enabled(false);
        self.perf_overlay_on = false;
        cx.notify();
    }

    /// 浮窗右上角 × / Alt+F4 关闭时回写宿主与配置。
    pub(crate) fn perf_overlay_closed(&mut self, cx: &mut Context<Self>) {
        self.perf_window = None;
        self.perf_overlay_on = false;
        crate::perf_overlay::set_enabled(false);
        let mut cfg = crate::config::load_or_default();
        if cfg.perf_overlay {
            cfg.perf_overlay = false;
            crate::config::save(&cfg);
        }
        self.settings_view
            .update(cx, |view, cx| view.set_perf_overlay(false, cx));
        cx.notify();
    }

    /// 设置中心的动作分发：能即时生效的在这里应用，只能下次启动生效的
    /// 只写配置，界面已用「重启生效」标注。
    pub(super) fn handle_settings_action(
        &mut self,
        action: SettingsAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            SettingsAction::Close => self.close_settings(cx),
            SettingsAction::SetAppearance(mode) => {
                if self.appearance != mode {
                    self.toggle_appearance(cx);
                }
            }
            SettingsAction::SetSmoothScroll(on) => {
                self.smooth_scroll = on;
                let mut cfg = crate::config::load_or_default();
                cfg.smooth_scroll = on;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetPerfOverlay(on) => self.set_perf_overlay(on, cx),
            SettingsAction::SetSourcePreference(pref) => {
                self.set_source_preference(pref, cx);
            }
            SettingsAction::SetBeatBand(low, high) => self.commit_beat_band(low, high, cx),
            SettingsAction::SetBeatStrength(value) => self.commit_beat_strength(value, cx),
            SettingsAction::SetBackgroundRenderDivisor(divisor) => {
                self.commit_background_render_divisor(divisor, cx)
            }
            SettingsAction::SetLyricSweepFade(value) => self.commit_lyric_sweep_fade(value, cx),
            SettingsAction::SetEqEnabled(on) => self.set_eq_enabled(on, cx),
            SettingsAction::OpenEqualizer => {
                self.settings_open = false;
                self.open_eq(cx);
            }
            SettingsAction::SetCacheDir(path) => {
                self.apply_cache_dir(path.display().to_string(), cx);
            }
            SettingsAction::SetCacheLimit(bytes) => self.set_cache_limit_bytes(bytes, cx),
            SettingsAction::ClearCache => self.clear_cache_now(cx),
            SettingsAction::SetDownloadDir(path) => {
                self.apply_download_dir(path.display().to_string(), cx);
            }
            SettingsAction::SetSearchTimeout(ms) => {
                self.search_timeout_ms = ms.clamp(500, 60_000);
                let mut cfg = crate::config::load_or_default();
                cfg.search_timeout_ms = self.search_timeout_ms;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetNeteaseMode(mode) => {
                let mut cfg = crate::config::load_or_default();
                cfg.netease.mode = mode;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetNeteaseQuality(level) => {
                let mut cfg = crate::config::load_or_default();
                cfg.netease.level = Some(level);
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetNeteaseBaseUrl(url) => {
                let mut cfg = crate::config::load_or_default();
                cfg.netease.base_url = url;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetPlayerDevice(device) => {
                let mut cfg = crate::config::load_or_default();
                cfg.player_device = device;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::AddLocalRoot(path) => {
                let mut cfg = crate::config::load_or_default();
                if !cfg.local_roots.contains(&path) {
                    cfg.local_roots.push(path);
                    crate::config::save(&cfg);
                }
                let local = self.local_source.clone();
                if let Some(local) = local {
                    local.set_roots(cfg.local_roots.clone());
                    self.spawn_library_scan(local, cx);
                }
            }
            SettingsAction::RemoveLocalRoot(index) => {
                let mut cfg = crate::config::load_or_default();
                if cfg.local_roots.len() > 1 && index < cfg.local_roots.len() {
                    cfg.local_roots.remove(index);
                    crate::config::save(&cfg);
                    let local = self.local_source.clone();
                    if let Some(local) = local {
                        local.set_roots(cfg.local_roots.clone());
                        self.spawn_library_scan(local, cx);
                    }
                }
            }
            SettingsAction::RescanLibrary => {
                let local = self.local_source.clone();
                if let Some(local) = local {
                    self.spawn_library_scan(local, cx);
                }
            }
        }
    }

    pub(super) fn apply_cache_dir(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let path = std::path::PathBuf::from(text);
        let mut cfg = crate::config::load_or_default();
        cfg.cache_dir = path.clone();
        crate::config::save(&cfg);
        let cache = self.cache.clone();
        let limit = cfg.cache_limit_bytes;
        let path_for_task = path.clone();
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .spawn(async move { cache.reconfigure(path_for_task, limit) })
                .await;
            let _ = weak.update(cx, |_app, cx| cx.notify());
        })
        .detach();
        self.info_message = Some(format!("缓存目录：{}", path.display()));
        cx.notify();
    }

    pub(super) fn apply_download_dir(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let path = std::path::PathBuf::from(text);
        let mut cfg = crate::config::load_or_default();
        cfg.download_dir = path.clone();
        crate::config::save(&cfg);
        self.downloads = Arc::new(DownloadStore::open(path.clone()));
        self.info_message = Some(format!("下载目录：{}", path.display()));
        cx.notify();
    }

    pub(super) fn set_cache_limit_bytes(&mut self, bytes: u64, cx: &mut Context<Self>) {
        let mut cfg = crate::config::load_or_default();
        cfg.cache_limit_bytes = bytes;
        crate::config::save(&cfg);
        let cache = self.cache.clone();
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .spawn(async move { cache.set_limit(bytes) })
                .await;
            let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            let _ = weak.update(cx, |app, cx| {
                app.info_message = Some(format!("缓存上限：{gb:.1} GB"));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn clear_cache_now(&mut self, cx: &mut Context<Self>) {
        let cache = self.cache.clone();
        let settings = self.settings_view.clone();
        self.info_message = Some("正在清空对象缓存…".into());
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let removed = cx
                .background_executor()
                .spawn(async move { cache.purge_all() })
                .await;
            let _ = weak.update(cx, |app, cx| {
                app.info_message = Some(format!("已清空对象缓存（{removed} 项）"));
                let stats = app.cache.stats();
                settings.update(cx, |view, cx| {
                    view.update_cache(stats.total_bytes, stats.limit_bytes, stats.entries, cx);
                    view.set_notice(format!("已清空对象缓存（{removed} 项）"), false, cx);
                });
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn set_source_preference(&mut self, pref: SourcePreference, cx: &mut Context<Self>) {
        self.source_preference = pref;
        let mut cfg = crate::config::load_or_default();
        cfg.source_preference = pref;
        crate::config::save(&cfg);
        self.info_message = Some(format!("播放来源：{}", pref.label()));
        cx.notify();
    }

    pub(super) fn cycle_source_preference(&mut self, cx: &mut Context<Self>) {
        self.set_source_preference(self.source_preference.next(), cx);
    }

    /// 设置面板提交：解析「下限 / 上限」两个输入框并应用频段。
    pub(super) fn apply_beat_band(&mut self, cx: &mut Context<Self>) {
        let low_text = self.settings_beat_low_input.read(cx).content().to_string();
        let high_text = self.settings_beat_high_input.read(cx).content().to_string();
        let invalid =
            || "鼓点频段无效：需满足 20 ≤ 下限 < 上限 ≤ 22050 Hz（支持 4k / 10khz）".to_string();
        let (Some(low), Some(high)) = (parse_hz_input(&low_text), parse_hz_input(&high_text))
        else {
            self.status_message = Some(invalid());
            cx.notify();
            return;
        };
        let in_range = |v: f32| (20.0..=22_050.0).contains(&v);
        if !in_range(low) || !in_range(high) || high <= low {
            self.status_message = Some(invalid());
            cx.notify();
            return;
        }
        self.commit_beat_band(low, high, cx);
    }

    pub(super) fn commit_beat_band(&mut self, low: f32, high: f32, cx: &mut Context<Self>) {
        let (low, high) = chordal_player::normalize_band(low, high);
        self.beat_band = (low, high);
        self.player.set_beat_band(low, high);
        let mut cfg = crate::config::load_or_default();
        cfg.beat_band_low_hz = low;
        cfg.beat_band_high_hz = high;
        crate::config::save(&cfg);
        self.settings_beat_low_input.update(cx, |state, cx| {
            state.set_content(format!("{low:.0}"), cx);
        });
        self.settings_beat_high_input.update(cx, |state, cx| {
            state.set_content(format!("{high:.0}"), cx);
        });
        self.info_message = Some(format!("背景鼓点频段：{low:.0}–{high:.0} Hz"));
        cx.notify();
    }

    pub(super) fn set_beat_band_preset(&mut self, low: f32, high: f32, cx: &mut Context<Self>) {
        self.commit_beat_band(low, high, cx);
    }

    /// 应用背景鼓点的视觉倍率（0..=1.5），立即更新渲染器并持久化。
    pub(super) fn commit_beat_strength(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(0.0, 1.5);
        self.lyric_view
            .update(cx, |view, _| view.set_background_beat_strength(value));
        let mut cfg = crate::config::load_or_default();
        cfg.beat_strength = value;
        crate::config::save(&cfg);
        self.info_message = Some(format!("背景鼓点强度：{:.0}%", value * 100.0));
        cx.notify();
    }

    /// 流体背景光栅缩放：输出分辨率 = 显示区域 ÷ 除数。
    pub(super) fn background_render_scale(&self) -> f32 {
        1.0 / self.background_render_divisor.max(1) as f32
    }

    /// 应用流体背景分辨率除数（1..=20），立即更新渲染器并持久化。
    pub(super) fn commit_background_render_divisor(&mut self, divisor: u32, cx: &mut Context<Self>) {
        let divisor = crate::config::clamp_background_render_divisor(divisor);
        self.background_render_divisor = divisor;
        let scale = self.background_render_scale();
        self.lyric_view
            .update(cx, |view, _| view.set_background_render_scale(scale));
        let mut cfg = crate::config::load_or_default();
        cfg.background_render_divisor = divisor;
        crate::config::save(&cfg);
        self.info_message = Some(format!("流体背景分辨率：1/{divisor}"));
        cx.notify();
    }

    /// 应用歌词整行扫光的渐变带宽度（em），立即重放当前模式的歌词样式并持久化。
    pub(super) fn commit_lyric_sweep_fade(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = crate::config::clamp_lyric_sweep_fade_em(value);
        self.lyric_sweep_fade_em = value;
        let theme = Theme::get(cx).clone();
        // 手机播放页把歌词视图当背景，使用沉浸样式；其余按当前是否沉浸选择。
        let style = match self.phone_backdrop {
            PhoneBackdrop::Lyrics => {
                phone_lyric_style(&theme, self.viewport_width, self.viewport_height, value)
            }
            PhoneBackdrop::Player => {
                immersive_lyric_style(self.viewport_width, self.viewport_height, value)
            }
            PhoneBackdrop::Off if self.immersive => {
                immersive_lyric_style(self.viewport_width, self.viewport_height, value)
            }
            PhoneBackdrop::Off => compact_lyric_style(&theme, value),
        };
        self.lyric_view
            .update(cx, |view, _| view.set_text_style(style));
        let mut cfg = crate::config::load_or_default();
        cfg.lyric_sweep_fade_em = value;
        crate::config::save(&cfg);
        self.info_message = Some(format!("歌词扫光宽度：{value:.2}em"));
        cx.notify();
    }
}
