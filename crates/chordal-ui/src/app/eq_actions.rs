//! `ChordialApp` 的 eq_actions 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn open_eq(&mut self, cx: &mut Context<Self>) {
        self.eq_open = true;
        self.eq_open_at = Some(Instant::now());
        self.eq.settings = self.eq.settings.clone().sanitized();
        self.player.set_eq(self.eq.settings.clone());
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    pub(crate) fn close_eq(&mut self, cx: &mut Context<Self>) {
        self.eq_open = false;
        self.eq_save_at = None;
        self.persist_eq();
        cx.notify();
    }

    pub(crate) fn remove_saved_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.eq.saved.len() {
            self.eq.saved.remove(index);
            self.persist_eq();
            cx.notify();
        }
    }

    pub(super) fn eq_animating(&self) -> bool {
        // 只有打开时的 240ms 入场淡入需要连续帧；400ms 落盘防抖是后台
        // 一次性任务，不应把整个界面拖入持续重绘。
        self.eq_open
            && self
                .eq_open_at
                .is_some_and(|t| t.elapsed() < Duration::from_millis(260))
    }

    /// 把当前配置推给播放线程，并在 400ms 后落盘。
    pub(crate) fn apply_eq(&mut self, cx: &mut Context<Self>) {
        self.eq.settings = self.eq.settings.clone().sanitized();
        self.eq.selected = self
            .eq
            .selected
            .filter(|index| *index < self.eq.settings.bands.len());
        self.player.set_eq(self.eq.settings.clone());
        self.eq_save_at = Some(Instant::now());
        self.eq_save_gen = self.eq_save_gen.wrapping_add(1);
        let generation = self.eq_save_gen;
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(420))
                .await;
            let _ = weak.update(cx, |app, _| {
                if app.eq_save_gen == generation && app.eq_save_at.is_some() {
                    app.eq_save_at = None;
                    app.persist_eq();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn set_eq_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        self.eq.settings.enabled = on;
        self.eq.touch();
        self.apply_eq(cx);
        self.show_portrait_toast(
            if on {
                "均衡器已开启"
            } else {
                "均衡器已关闭"
            },
            cx,
        );
        cx.notify();
    }

    pub(crate) fn add_eq_band(&mut self, cx: &mut Context<Self>) {
        if self.eq.add_band().is_some() {
            self.apply_eq(cx);
        } else {
            self.show_portrait_toast(format!("最多 {EQ_MAX_BANDS} 个频段"), cx);
        }
    }

    pub(crate) fn remove_eq_band(&mut self, cx: &mut Context<Self>) {
        self.eq.remove_selected();
        self.apply_eq(cx);
    }

    pub(crate) fn select_eq_band(&mut self, index: usize, cx: &mut Context<Self>) {
        self.eq.selected = Some(index);
        self.eq.type_menu_open = false;
        self.bump_portrait(cx);
        cx.notify();
    }

    pub(crate) fn set_eq_filter_kind(&mut self, kind: FilterKind, cx: &mut Context<Self>) {
        if let Some(band) = self.eq.selected_band_mut() {
            band.kind = kind;
            self.eq.type_menu_open = false;
            self.eq.touch();
            self.apply_eq(cx);
        }
    }

    pub(crate) fn set_eq_freq(&mut self, freq_hz: f32, cx: &mut Context<Self>) {
        if let Some(band) = self.eq.selected_band_mut() {
            band.freq_hz = freq_hz.clamp(20.0, 20_000.0);
            self.eq.touch();
            self.apply_eq(cx);
        }
    }

    pub(crate) fn set_eq_gain(&mut self, gain_db: f32, cx: &mut Context<Self>) {
        if let Some(band) = self.eq.selected_band_mut() {
            band.gain_db = gain_db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB);
            self.eq.touch();
            self.apply_eq(cx);
        }
    }

    pub(crate) fn set_eq_q(&mut self, q: f32, cx: &mut Context<Self>) {
        if let Some(band) = self.eq.selected_band_mut() {
            band.q = q.clamp(chordal_player::EQ_MIN_Q, chordal_player::EQ_MAX_Q);
            self.eq.touch();
            self.apply_eq(cx);
        }
    }

    pub(crate) fn load_eq_preset(&mut self, preset: PresetEntry, cx: &mut Context<Self>) {
        self.eq.load_preset(&preset);
        self.apply_eq(cx);
        self.show_portrait_toast(format!("已应用「{}」", preset.name), cx);
        cx.notify();
    }

    // ---------- AutoEQ 耳机档案 ----------

    /// 切换 EQ 分类；进入 AutoEQ 时按需加载仓库索引。
    pub(crate) fn select_eq_tab(&mut self, tab: EqTab, cx: &mut Context<Self>) {
        self.eq.tab = tab;
        self.eq.type_menu_open = false;
        if tab == EqTab::AutoEq {
            self.ensure_autoeq(cx);
        }
        cx.notify();
    }

    /// 首次进入 AutoEQ：先读本地缓存（同步、几毫秒），过期或缺失时后台拉取。
    pub(crate) fn ensure_autoeq(&mut self, cx: &mut Context<Self>) {
        if self.eq.autoeq.status != AutoEqStatus::Idle || self.eq.autoeq.index_loading {
            return;
        }
        match autoeq::load_cached_index() {
            Some((entries, modified)) => {
                self.eq.autoeq.replace_entries(entries);
                self.eq.autoeq.status = AutoEqStatus::Ready;
                self.eq.autoeq.from_cache = true;
                self.eq.autoeq.error = None;
                cx.notify();
                if autoeq::index_cache_is_stale(modified) {
                    self.sync_autoeq(false, cx);
                }
            }
            None => self.sync_autoeq(false, cx),
        }
    }

    /// 从 AutoEq 仓库同步索引；已有缓存时列表保持可见，只更新状态。
    pub(crate) fn sync_autoeq(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.eq.autoeq.index_loading {
            return;
        }
        self.eq.autoeq.index_loading = true;
        if self.eq.autoeq.entries.is_empty() {
            self.eq.autoeq.status = AutoEqStatus::Loading;
        }
        self.eq.autoeq.error = None;
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async { autoeq::fetch_index_blocking() })
                .await;
            let _ = weak.update(cx, |app, cx| {
                app.eq.autoeq.index_loading = false;
                let toast = match result {
                    Ok(entries) => {
                        let count = entries.len();
                        app.eq.autoeq.replace_entries(entries);
                        app.eq.autoeq.status = AutoEqStatus::Ready;
                        app.eq.autoeq.from_cache = false;
                        app.eq.autoeq.error = None;
                        tracing::info!(count, "autoeq index synced");
                        manual.then(|| format!("AutoEQ 已同步：{count} 款耳机"))
                    }
                    Err(error) => {
                        if app.eq.autoeq.entries.is_empty() {
                            app.eq.autoeq.status = AutoEqStatus::Failed;
                        }
                        tracing::warn!(error = %error, "autoeq index sync failed");
                        app.eq.autoeq.error = Some(error.clone());
                        manual.then(|| format!("AutoEQ 同步失败：{error}"))
                    }
                };
                if let Some(message) = toast {
                    app.show_portrait_toast(message, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 预览（`apply = false`）或直接应用（`apply = true`）某个型号；
    /// 曲线尚未拉取时先读缓存，再按需下载，拉取完成后自动应用。
    pub(crate) fn select_autoeq_entry(
        &mut self,
        file: String,
        apply: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((name, loaded)) = self
            .eq
            .autoeq
            .find(&file)
            .map(|entry| (entry.name.clone(), entry.bands.is_some()))
        else {
            return;
        };
        // 新选择覆盖旧的「拉取完自动应用」请求：预览别的型号不应再触发
        // 上一次的自动应用。
        if self.eq.autoeq.pending_apply.as_deref() != Some(file.as_str()) {
            self.eq.autoeq.pending_apply = None;
        }
        self.eq.autoeq_preview = Some(file.clone());
        if loaded {
            if apply {
                self.apply_autoeq_entry(&file, cx);
            } else {
                cx.notify();
            }
            return;
        }
        if let Some((bands, gain)) = autoeq::load_cached_param(&file) {
            self.store_autoeq_param(&file, bands, gain);
            if apply {
                self.apply_autoeq_entry(&file, cx);
            } else {
                cx.notify();
            }
            return;
        }
        if apply {
            self.eq.autoeq.pending_apply = Some(file.clone());
            self.show_portrait_toast(format!("正在拉取「{name}」…"), cx);
        }
        let already_loading = self
            .eq
            .autoeq
            .find(&file)
            .is_some_and(|entry| entry.loading);
        if !already_loading && let Some(entry) = self.eq.autoeq.find_mut(&file) {
            entry.loading = true;
            entry.error = None;
        }
        cx.notify();
        if already_loading {
            return;
        }
        cx.spawn(async move |weak, cx| {
            let key = file.clone();
            let result = cx
                .background_executor()
                .spawn(async move { autoeq::fetch_param_blocking(&key) })
                .await;
            let _ = weak.update(cx, |app, cx| {
                let pending = app.eq.autoeq.pending_apply.clone();
                let mut apply_now = false;
                let mut failure = None;
                if let Some(entry) = app.eq.autoeq.find_mut(&file) {
                    entry.loading = false;
                    match result {
                        Ok((bands, gain)) => {
                            entry.bands = Some(bands);
                            entry.input_gain_db = gain;
                            entry.error = None;
                            apply_now = pending.as_deref() == Some(file.as_str());
                        }
                        Err(error) => {
                            entry.error = Some(error.clone());
                            failure = Some(error);
                        }
                    }
                }
                if let Some(error) = &failure {
                    tracing::warn!(file = %file, error = %error, "autoeq param fetch failed");
                }
                if apply_now {
                    app.eq.autoeq.pending_apply = None;
                    app.apply_autoeq_entry(&file, cx);
                } else if failure.is_some() {
                    app.eq.autoeq.pending_apply = None;
                }
                if let Some(error) = failure {
                    app.show_portrait_toast(format!("AutoEQ 拉取失败：{error}"), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn store_autoeq_param(&mut self, file: &str, bands: Vec<EqBand>, input_gain_db: f32) {
        if let Some(entry) = self.eq.autoeq.find_mut(file) {
            entry.bands = Some(bands);
            entry.input_gain_db = input_gain_db;
            entry.error = None;
            entry.loading = false;
        }
    }

    pub(crate) fn apply_autoeq_entry(&mut self, file: &str, cx: &mut Context<Self>) {
        let Some((name, settings)) = self.eq.autoeq.find(file).and_then(|entry| {
            entry
                .to_settings()
                .map(|settings| (entry.name.clone(), settings))
        }) else {
            return;
        };
        self.eq.load_settings(name.clone(), settings);
        self.eq.autoeq_preview = Some(file.to_string());
        self.apply_eq(cx);
        self.show_portrait_toast(format!("已应用「{name}」"), cx);
        cx.notify();
    }

    pub(crate) fn save_eq_preset(&mut self, cx: &mut Context<Self>) {
        if self.eq.settings.bands.is_empty() {
            self.show_portrait_toast("空配置无需保存", cx);
            return;
        }
        let name = self.eq.save_as();
        self.eq.tab = EqTab::Personal;
        self.eq_save_at = None;
        self.persist_eq();
        self.show_portrait_toast(format!("已保存「{name}」"), cx);
        cx.notify();
    }

    pub(crate) fn persist_eq(&mut self) {
        let mut cfg = crate::config::load_or_default();
        cfg.equalizer = self.eq.settings.clone();
        cfg.eq_saved_presets = self.eq.saved.clone();
        crate::config::save(&cfg);
    }
}
