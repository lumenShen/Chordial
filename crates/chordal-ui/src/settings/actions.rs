//! 设置中心：actions（由 settings.rs 拆分，行为不变）。

use super::*;

impl SettingsView {
    pub fn new(
        handler: impl Fn(SettingsAction, &mut Window, &mut App) + 'static,
        cfg: AppConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let audio_band_ranges = cfg.audio_band_ranges.sanitized();
        let audio_band_controls = cfg.audio_band_controls.sanitized();
        let beat_strength = cfg.beat_strength.clamp(0.0, 1.5);
        let lyric_sweep_fade_em = crate::config::clamp_lyric_sweep_fade_em(cfg.lyric_sweep_fade_em);
        let background_render_divisor =
            crate::config::clamp_background_render_divisor(cfg.background_render_divisor);
        let animation_duration_multiplier =
            crate::config::clamp_animation_duration_multiplier(cfg.animation_duration_multiplier);
        let animation_overshoot_multiplier =
            crate::config::clamp_animation_overshoot_multiplier(cfg.animation_overshoot_multiplier);
        let quality = cfg.netease.level.unwrap_or_default();
        let weak = cx.entity().downgrade();

        let cache_dir_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-cache-dir",
                "对象缓存目录",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_cache_dir(text, window, cx));
                },
                cx,
            )
        };
        let download_dir_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-download-dir",
                "下载目录",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_download_dir(text, window, cx));
                },
                cx,
            )
        };
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
        let mut audio_band_low_fields = Vec::with_capacity(5);
        let mut audio_band_high_fields = Vec::with_capacity(5);
        let mut audio_band_control_fields = Vec::with_capacity(5);
        for index in 0..5 {
            let weak_low = weak.clone();
            audio_band_low_fields.push(TextField::new(
                BAND_LOW_IDS[index],
                "下限 Hz",
                move |_text, window, cx| {
                    let _ =
                        weak_low.update(cx, |view, cx| view.submit_audio_band(index, window, cx));
                },
                cx,
            ));
            let weak_high = weak.clone();
            audio_band_high_fields.push(TextField::new(
                BAND_HIGH_IDS[index],
                "上限 Hz",
                move |_text, window, cx| {
                    let _ =
                        weak_high.update(cx, |view, cx| view.submit_audio_band(index, window, cx));
                },
                cx,
            ));
            let weak_controls = weak.clone();
            audio_band_control_fields.push(TextField::new(
                BAND_CONTROL_IDS[index],
                "如 7,8,12,13",
                move |_text, window, cx| {
                    let _ = weak_controls.update(cx, |view, cx| {
                        view.submit_audio_band_controls(index, window, cx)
                    });
                },
                cx,
            ));
        }
        let beat_strength_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-beat-strength",
                "50",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_beat_strength(text, window, cx));
                },
                cx,
            )
        };
        let background_divisor_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-background-divisor",
                "4",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.submit_background_divisor(text, window, cx)
                    });
                },
                cx,
            )
        };
        let lyric_sweep_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-lyric-sweep-fade",
                "0.6",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_lyric_sweep(text, window, cx));
                },
                cx,
            )
        };
        let motion_duration_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-motion-duration",
                "1.00",
                move |text, window, cx| {
                    let _ =
                        weak.update(cx, |view, cx| view.submit_motion_duration(text, window, cx));
                },
                cx,
            )
        };
        let motion_overshoot_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-motion-overshoot",
                "1.00",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.submit_motion_overshoot(text, window, cx)
                    });
                },
                cx,
            )
        };
        let search_timeout_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-search-timeout",
                "6000",
                move |text, window, cx| {
                    let _ =
                        weak.update(cx, |view, cx| view.submit_search_timeout(text, window, cx));
                },
                cx,
            )
        };
        let netease_url_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-netease-url",
                "http://127.0.0.1:3000",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_netease_url(text, window, cx));
                },
                cx,
            )
        };
        let device_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-player-device",
                "留空使用系统默认设备",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_device(text, window, cx));
                },
                cx,
            )
        };
        let add_root_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-add-local-root",
                "添加音乐文件夹路径",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_add_root(text, window, cx));
                },
                cx,
            )
        };

        let view = Self {
            handler: Rc::new(handler),
            tab: initial_tab(),
            appearance: cfg.appearance,
            source_preference: cfg.source_preference,
            smooth_scroll: cfg.smooth_scroll,
            audio_band_ranges,
            audio_band_controls,
            beat_strength,
            background_render_divisor,
            lyric_sweep_fade_em,
            motion_preset: cfg.motion_preset,
            animation_duration_multiplier,
            animation_overshoot_multiplier,
            perf_overlay: cfg.perf_overlay,
            cache_dir: cfg.cache_dir,
            download_dir: cfg.download_dir,
            local_roots: cfg.local_roots,
            search_timeout_ms: cfg.search_timeout_ms,
            netease_base_url: cfg.netease.base_url,
            netease_mode: cfg.netease.mode,
            netease_quality: quality,
            player_device: cfg.player_device,
            cache_root: PathBuf::new(),
            cache_used: 0,
            cache_limit: cfg.cache_limit_bytes,
            cache_entries: 0,
            download_root: PathBuf::new(),
            eq_name: String::new(),
            eq_bands: 0,
            eq_enabled: false,
            backend: String::new(),
            config_path: crate::config::config_path(),
            mem: chordal_core::mem::snapshot(),
            cache_dir_field,
            download_dir_field,
            audio_band_low_fields,
            audio_band_high_fields,
            audio_band_control_fields,
            beat_strength_field,
            background_divisor_field,
            lyric_sweep_field,
            motion_duration_field,
            motion_overshoot_field,
            search_timeout_field,
            netease_url_field,
            device_field,
            add_root_field,
            notice: None,
        };
        view.write_inputs(cx);
        view
    }

    /// 打开设置时同步配置与运行时统计。
    pub fn sync(&mut self, cfg: &AppConfig, runtime: SettingsRuntime, cx: &mut Context<Self>) {
        self.appearance = cfg.appearance;
        self.source_preference = cfg.source_preference;
        self.smooth_scroll = cfg.smooth_scroll;
        self.audio_band_ranges = cfg.audio_band_ranges.sanitized();
        self.audio_band_controls = cfg.audio_band_controls.sanitized();
        self.beat_strength = cfg.beat_strength.clamp(0.0, 1.5);
        self.background_render_divisor =
            crate::config::clamp_background_render_divisor(cfg.background_render_divisor);
        self.lyric_sweep_fade_em =
            crate::config::clamp_lyric_sweep_fade_em(cfg.lyric_sweep_fade_em);
        self.motion_preset = cfg.motion_preset;
        self.animation_duration_multiplier =
            crate::config::clamp_animation_duration_multiplier(cfg.animation_duration_multiplier);
        self.animation_overshoot_multiplier =
            crate::config::clamp_animation_overshoot_multiplier(cfg.animation_overshoot_multiplier);
        self.perf_overlay = cfg.perf_overlay;
        self.cache_dir = cfg.cache_dir.clone();
        self.download_dir = cfg.download_dir.clone();
        self.local_roots = cfg.local_roots.clone();
        self.search_timeout_ms = cfg.search_timeout_ms;
        self.netease_base_url = cfg.netease.base_url.clone();
        self.netease_mode = cfg.netease.mode;
        self.netease_quality = cfg.netease.level.unwrap_or_default();
        self.player_device = cfg.player_device.clone();

        self.cache_root = runtime.cache_root;
        self.cache_used = runtime.cache_used_bytes;
        self.cache_limit = runtime.cache_limit_bytes;
        self.cache_entries = runtime.cache_entries;
        self.download_root = runtime.download_root;
        self.eq_name = runtime.eq_name;
        self.eq_bands = runtime.eq_band_count;
        self.eq_enabled = runtime.eq_enabled;
        self.backend = runtime.backend;

        self.write_inputs(cx);
        self.notice = None;
        cx.notify();
    }

    /// 只刷新内存诊断区（不重写输入框，避免覆盖正在编辑的文本）。
    pub fn sync_memory(
        &mut self,
        snapshot: chordal_core::mem::MemSnapshot,
        cx: &mut Context<Self>,
    ) {
        if self.mem == snapshot {
            return;
        }
        self.mem = snapshot;
        cx.notify();
    }

    /// 浮窗在界面上被直接关闭（右上角 × / Alt+F4）时同步开关状态。
    pub fn set_perf_overlay(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.perf_overlay != on {
            self.perf_overlay = on;
            cx.notify();
        }
    }

    /// 宿主异步完成后回写提示（清缓存、切换目录等）。
    pub fn set_notice(
        &mut self,
        text: impl Into<SharedString>,
        error: bool,
        cx: &mut Context<Self>,
    ) {
        self.notice = Some(Notice {
            text: text.into(),
            error,
        });
        cx.notify();
    }

    /// 清空缓存后刷新占用条。
    pub fn update_cache(&mut self, used: u64, limit: u64, entries: u64, cx: &mut Context<Self>) {
        self.cache_used = used;
        self.cache_limit = limit;
        self.cache_entries = entries;
        cx.notify();
    }

    pub(super) fn write_inputs(&self, cx: &mut Context<Self>) {
        self.cache_dir_field
            .set(self.cache_dir.display().to_string(), cx);
        self.download_dir_field
            .set(self.download_dir.display().to_string(), cx);
        for (index, kind) in AudioBandKind::ALL.into_iter().enumerate() {
            let range = self.audio_band_ranges.get(kind);
            self.audio_band_low_fields[index].set(format!("{:.0}", range.low_hz), cx);
            self.audio_band_high_fields[index].set(format!("{:.0}", range.high_hz), cx);
            self.audio_band_control_fields[index]
                .set(self.audio_band_controls.get(kind).display(), cx);
        }
        self.beat_strength_field
            .set(format!("{:.0}", self.beat_strength * 100.0), cx);
        self.background_divisor_field
            .set(self.background_render_divisor.to_string(), cx);
        self.lyric_sweep_field
            .set(format!("{:.2}", self.lyric_sweep_fade_em), cx);
        self.motion_duration_field
            .set(format!("{:.2}", self.animation_duration_multiplier), cx);
        self.motion_overshoot_field
            .set(format!("{:.2}", self.animation_overshoot_multiplier), cx);
        self.search_timeout_field
            .set(self.search_timeout_ms.to_string(), cx);
        self.netease_url_field
            .set(self.netease_base_url.clone(), cx);
        self.device_field
            .set(self.player_device.clone().unwrap_or_default(), cx);
    }

    pub(super) fn set_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    pub(super) fn emit(&self, action: SettingsAction, window: &mut Window, cx: &mut Context<Self>) {
        (self.handler)(action, window, cx);
    }

    pub(super) fn notice_ok(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: false,
        });
        cx.notify();
    }

    pub(super) fn notice_error(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: true,
        });
        cx.notify();
    }

    // ---------- 输入提交 ----------

    pub(super) fn submit_cache_dir(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.notice_error("缓存目录不能为空", cx);
            return;
        }
        let path = PathBuf::from(&text);
        self.cache_dir = path.clone();
        self.emit(SettingsAction::SetCacheDir(path), window, cx);
        self.notice_ok(format!("缓存目录 {text}"), cx);
    }

    pub(super) fn submit_download_dir(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.notice_error("下载目录不能为空", cx);
            return;
        }
        let path = PathBuf::from(&text);
        self.download_dir = path.clone();
        self.emit(SettingsAction::SetDownloadDir(path), window, cx);
        self.notice_ok(format!("下载目录 {text}"), cx);
    }

    pub(super) fn submit_audio_band(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = AudioBandKind::ALL.get(index).copied() else {
            return;
        };
        let low_text = self.audio_band_low_fields[index]
            .state
            .read(cx)
            .content()
            .to_string();
        let high_text = self.audio_band_high_fields[index]
            .state
            .read(cx)
            .content()
            .to_string();
        let (Some(low), Some(high)) = (parse_hz(&low_text), parse_hz(&high_text)) else {
            self.notice_error("频段格式无效，可填 4000 / 4k / 10khz", cx);
            return;
        };
        if !(20.0..=22_050.0).contains(&low) || !(20.0..=22_050.0).contains(&high) || high <= low {
            self.notice_error("需满足 20 ≤ 下限 < 上限 ≤ 22050 Hz", cx);
            return;
        }
        self.audio_band_ranges.set(
            kind,
            chordal_core::FrequencyRange {
                low_hz: low,
                high_hz: high,
            },
        );
        self.audio_band_low_fields[index].set(format!("{low:.0}"), cx);
        self.audio_band_high_fields[index].set(format!("{high:.0}"), cx);
        self.emit(
            SettingsAction::SetAudioBandRanges(self.audio_band_ranges),
            window,
            cx,
        );
        self.notice_ok(format!("{} {low:.0}–{high:.0} Hz", kind.label()), cx);
    }

    pub(super) fn submit_audio_band_controls(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = AudioBandKind::ALL.get(index).copied() else {
            return;
        };
        let text = self.audio_band_control_fields[index]
            .state
            .read(cx)
            .content()
            .to_string();
        let controls = match chordal_core::parse_control_point_set(&text) {
            Ok(controls) => controls,
            Err(chordal_core::ControlPointParseError::InvalidNumber) => {
                self.notice_error("控制点编号需用逗号分隔，如 7,8,12,13", cx);
                return;
            }
            Err(chordal_core::ControlPointParseError::BoundaryOrOutOfRange(_)) => {
                self.notice_error("仅可分配内部点 7,8,9,12,13,14,17,18,19；边界点保持固定", cx);
                return;
            }
        };
        self.audio_band_controls.set(kind, controls);
        let display = controls.display();
        self.audio_band_control_fields[index].set(display.clone(), cx);
        self.emit(
            SettingsAction::SetAudioBandControls(self.audio_band_controls),
            window,
            cx,
        );
        self.notice_ok(
            format!(
                "{} → 控制点 {}",
                kind.label(),
                if controls.is_empty() { "无" } else { &display }
            ),
            cx,
        );
    }

    /// 提交“效果强度”（百分比，0–150）。
    pub(super) fn submit_beat_strength(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().trim_end_matches('%').trim();
        let Ok(percent) = text.parse::<f32>() else {
            self.notice_error("效果强度需为 0–150 的百分比", cx);
            return;
        };
        if !(0.0..=150.0).contains(&percent) {
            self.notice_error("效果强度范围 0–150%", cx);
            return;
        }
        self.apply_beat_strength(percent / 100.0, window, cx);
    }

    pub(super) fn set_beat_strength_preset(
        &mut self,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_beat_strength(value, window, cx);
    }

    pub(super) fn apply_beat_strength(
        &mut self,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = value.clamp(0.0, 1.5);
        self.beat_strength = value;
        self.beat_strength_field
            .set(format!("{:.0}", value * 100.0), cx);
        self.emit(SettingsAction::SetBeatStrength(value), window, cx);
        self.notice_ok(format!("背景音频反应强度 {:.0}%", value * 100.0), cx);
    }

    /// 提交流体背景分辨率除数（整数，1–20）。
    pub(super) fn submit_background_divisor(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().trim_start_matches('÷').trim();
        let Ok(divisor) = text.parse::<u32>() else {
            self.notice_error("分辨率除数需为 1–20 的整数", cx);
            return;
        };
        if !(crate::config::BACKGROUND_RENDER_DIVISOR_MIN
            ..=crate::config::BACKGROUND_RENDER_DIVISOR_MAX)
            .contains(&divisor)
        {
            self.notice_error("分辨率除数范围 1–20", cx);
            return;
        }
        self.apply_background_divisor(divisor, window, cx);
    }

    pub(super) fn set_background_divisor_preset(
        &mut self,
        divisor: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_background_divisor(divisor, window, cx);
    }

    pub(super) fn apply_background_divisor(
        &mut self,
        divisor: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let divisor = crate::config::clamp_background_render_divisor(divisor);
        self.background_render_divisor = divisor;
        self.background_divisor_field.set(divisor.to_string(), cx);
        self.emit(
            SettingsAction::SetBackgroundRenderDivisor(divisor),
            window,
            cx,
        );
        self.notice_ok(format!("流体背景分辨率 1/{divisor}"), cx);
    }

    /// 提交整行扫光渐变带宽度（em，0.1–4.0）。
    pub(super) fn submit_lyric_sweep(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().trim_end_matches("em").trim();
        let Ok(value) = text.parse::<f32>() else {
            self.notice_error("扫光宽度需为 0.1–4.0 的数字（单位 em）", cx);
            return;
        };
        if !(crate::config::LYRIC_SWEEP_FADE_EM_MIN..=crate::config::LYRIC_SWEEP_FADE_EM_MAX)
            .contains(&value)
        {
            self.notice_error("扫光宽度范围 0.1–4.0 em", cx);
            return;
        }
        self.apply_lyric_sweep(value, window, cx);
    }

    pub(super) fn set_lyric_sweep_preset(
        &mut self,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_lyric_sweep(value, window, cx);
    }

    pub(super) fn apply_lyric_sweep(
        &mut self,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = crate::config::clamp_lyric_sweep_fade_em(value);
        self.lyric_sweep_fade_em = value;
        self.lyric_sweep_field.set(format!("{value:.2}"), cx);
        self.emit(SettingsAction::SetLyricSweepFade(value), window, cx);
        self.notice_ok(format!("歌词扫光宽度 {value:.2}em"), cx);
    }

    /// 切换弹簧预设：即刻影响新动画，已开始的动画保持原参数。
    pub(super) fn apply_motion_preset(
        &mut self,
        preset: MotionPreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.motion_preset = preset;
        self.emit(SettingsAction::SetMotionPreset(preset), window, cx);
        self.notice_ok(format!("动画预设 {}", preset.label()), cx);
    }

    /// 提交动画时长倍率（0.25–3.0，> 1 慢放）。
    pub(super) fn submit_motion_duration(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(value) = text.trim().parse::<f32>() else {
            self.notice_error("时长倍率需为 0.25–3.0 的数字", cx);
            return;
        };
        if !value.is_finite()
            || !(crate::config::ANIMATION_DURATION_MULTIPLIER_MIN
                ..=crate::config::ANIMATION_DURATION_MULTIPLIER_MAX)
                .contains(&value)
        {
            self.notice_error("时长倍率范围 0.25–3.0", cx);
            return;
        }
        self.apply_motion_duration(value, window, cx);
    }

    pub(super) fn set_motion_duration_preset(
        &mut self,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_motion_duration(value, window, cx);
    }

    pub(super) fn apply_motion_duration(
        &mut self,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = crate::config::clamp_animation_duration_multiplier(value);
        self.animation_duration_multiplier = value;
        self.motion_duration_field.set(format!("{value:.2}"), cx);
        self.emit(SettingsAction::SetMotionDuration(value), window, cx);
        self.notice_ok(format!("动画时长 {value:.2}×（越大越慢放）"), cx);
    }

    /// 提交动画回弹倍率（0.25–2.0，> 1 回弹更明显）。
    pub(super) fn submit_motion_overshoot(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(value) = text.trim().parse::<f32>() else {
            self.notice_error("回弹倍率需为 0.25–2.0 的数字", cx);
            return;
        };
        if !value.is_finite()
            || !(crate::config::ANIMATION_OVERSHOOT_MULTIPLIER_MIN
                ..=crate::config::ANIMATION_OVERSHOOT_MULTIPLIER_MAX)
                .contains(&value)
        {
            self.notice_error("回弹倍率范围 0.25–2.0", cx);
            return;
        }
        self.apply_motion_overshoot(value, window, cx);
    }

    pub(super) fn set_motion_overshoot_preset(
        &mut self,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_motion_overshoot(value, window, cx);
    }

    pub(super) fn apply_motion_overshoot(
        &mut self,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = crate::config::clamp_animation_overshoot_multiplier(value);
        self.animation_overshoot_multiplier = value;
        self.motion_overshoot_field.set(format!("{value:.2}"), cx);
        self.emit(SettingsAction::SetMotionOvershoot(value), window, cx);
        self.notice_ok(format!("动画回弹 {value:.2}×（越大回弹越明显）"), cx);
    }

    pub(super) fn submit_search_timeout(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(ms) = text.trim().parse::<u64>() else {
            self.notice_error("搜索超时必须是毫秒数", cx);
            return;
        };
        if !(500..=60_000).contains(&ms) {
            self.notice_error("搜索超时范围 500–60000 ms", cx);
            return;
        }
        self.search_timeout_ms = ms;
        self.emit(SettingsAction::SetSearchTimeout(ms), window, cx);
        self.notice_ok(format!("每源搜索超时 {ms} ms"), cx);
    }

    pub(super) fn submit_netease_url(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().to_string();
        if !(text.starts_with("http://") || text.starts_with("https://")) {
            self.notice_error("服务地址需以 http:// 或 https:// 开头", cx);
            return;
        }
        self.netease_base_url = text.clone();
        self.emit(SettingsAction::SetNeteaseBaseUrl(text), window, cx);
        self.notice_ok("服务地址已保存 · 重启后生效", cx);
    }

    pub(super) fn submit_device(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().to_string();
        let device = (!text.is_empty()).then(|| text.clone());
        self.player_device = device.clone();
        self.emit(SettingsAction::SetPlayerDevice(device), window, cx);
        self.notice_ok(
            if text.is_empty() {
                "输出设备已恢复系统默认 · 重启后生效".to_string()
            } else {
                format!("输出设备 {text} · 重启后生效")
            },
            cx,
        );
    }

    pub(super) fn submit_add_root(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let path = PathBuf::from(&text);
        if self.local_roots.iter().any(|root| root == &path) {
            self.notice_error("该目录已在本地音乐库中", cx);
            return;
        }
        self.local_roots.push(path.clone());
        self.emit(SettingsAction::AddLocalRoot(path), window, cx);
        self.add_root_field.set(String::new(), cx);
        self.notice_ok("目录已添加 · 正在重新扫描本地库", cx);
    }

    pub(super) fn remove_local_root(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.local_roots.len() || self.local_roots.len() <= 1 {
            self.notice_error("至少保留一个音乐目录", cx);
            return;
        }
        self.local_roots.remove(index);
        self.emit(SettingsAction::RemoveLocalRoot(index), window, cx);
        self.notice_ok("目录已移除 · 重新扫描后生效", cx);
    }
}
