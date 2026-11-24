//! 设置中心：render_general_playback（由 settings.rs 拆分，行为不变）。

use super::*;

impl SettingsView {
    pub(super) fn render_general(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
    ) -> AnyElement {
        let theme_choice = {
            let mut items: Vec<(
                &'static str,
                bool,
                Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
            )> = Vec::new();
            for (mode, label) in [(Appearance::Light, "浅色"), (Appearance::Dark, "深色")] {
                let weak = weak.clone();
                let active = self.appearance == mode;
                items.push((
                    label,
                    active,
                    Box::new(move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.appearance = mode;
                            view.emit(SettingsAction::SetAppearance(mode), window, cx);
                            cx.notify();
                        });
                    }),
                ));
            }
            segmented(theme, "settings-theme", items)
        };
        let preference = {
            let mut items: Vec<(
                &'static str,
                bool,
                Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
            )> = Vec::new();
            for (pref, label) in [
                (SourcePreference::Auto, "音质优先"),
                (SourcePreference::PreferLocal, "本地优先"),
                (SourcePreference::PreferOnline, "在线优先"),
            ] {
                let weak = weak.clone();
                let active = self.source_preference == pref;
                items.push((
                    label,
                    active,
                    Box::new(move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.source_preference = pref;
                            view.emit(SettingsAction::SetSourcePreference(pref), window, cx);
                            cx.notify();
                        });
                    }),
                ));
            }
            segmented(theme, "settings-source-pref", items)
        };

        let smooth_scroll = {
            let weak = weak.clone();
            toggle(
                theme,
                self.smooth_scroll,
                "settings-smooth-scroll",
                gpui::rgb(0x3B82F6).into(),
                move |on, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.smooth_scroll = on;
                        view.emit(SettingsAction::SetSmoothScroll(on), window, cx);
                        cx.notify();
                    });
                },
            )
        };

        let perf_overlay = {
            let weak = weak.clone();
            toggle(
                theme,
                self.perf_overlay,
                "settings-perf-overlay",
                gpui::rgb(0x3FB950).into(),
                move |on, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.perf_overlay = on;
                        view.emit(SettingsAction::SetPerfOverlay(on), window, cx);
                        cx.notify();
                    });
                },
            )
        };

        section(
            theme,
            "外观",
            Some("主题即刻切换；两套配色共用同一套布局与字号。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "主题",
                    Some("浅色纸面 / 深色炭黑".into()),
                    theme_choice,
                )],
            )],
        )
        .child(section(
            theme,
            "播放来源",
            Some("本地与在线存在同曲时如何取舍。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "同曲优先",
                    Some("音质优先：本地规格更高才播本地；在线优先：除已下载外始终在线".into()),
                    preference,
                )],
            )],
        ))
        .child(section(
            theme,
            "滚动",
            Some("平滑滚动对滚轮/触控板生效；关闭时保留系统原生手感。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "平滑滚动",
                    Some("时间基缓动（45ms），默认关闭以避免与触控板惯性叠加".into()),
                    smooth_scroll,
                )],
            )],
        ))
        .child(section(
            theme,
            "诊断",
            Some("浮窗置顶悬浮于桌面，实时显示帧率、帧耗时与进程占用。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "画面性能浮窗",
                    Some(
                        "显示 FPS、1% Low、P50/P99 帧耗时、CPU 与内存；\
                         CPU 以单核满载为 100%，浮窗可拖动、右上角关闭"
                            .into(),
                    ),
                    perf_overlay,
                )],
            )],
        ))
        .into_any_element()
    }

    pub(super) fn render_playback(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let preset = |label: &'static str, low: f32, high: f32| {
            let weak = weak.clone();
            let active = self.beat_band == (low, high);
            chip(theme, label, active, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.beat_band = (low, high);
                    view.beat_low_field.set(format!("{low:.0}"), cx);
                    view.beat_high_field.set(format!("{high:.0}"), cx);
                    view.emit(SettingsAction::SetBeatBand(low, high), window, cx);
                    view.notice_ok(format!("背景鼓点频段 {low:.0}–{high:.0} Hz"), cx);
                });
            })
        };

        let band_controls = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(div().w(px(92.0)).child(self.beat_low_field.view.clone()))
            .child(hint(theme, "Hz"))
            .child(div().w(px(10.0)))
            .child(div().w(px(92.0)).child(self.beat_high_field.view.clone()))
            .child(hint(theme, "Hz"))
            .into_any_element();

        let preset_row = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(preset("低音 80–2k", 80.0, 2_000.0))
            .child(preset("鼓点 4–10k", 4_000.0, 10_000.0))
            .child(preset("高频 8–16k", 8_000.0, 16_000.0))
            .child(div().flex_1())
            .into_any_element();

        let strength_preset = |label: &'static str, value: f32| {
            let weak = weak.clone();
            let active = (self.beat_strength - value).abs() < 0.001;
            chip(theme, label, active, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.set_beat_strength_preset(value, window, cx);
                });
            })
        };

        let strength_controls = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .w(px(56.0))
                    .child(self.beat_strength_field.view.clone()),
            )
            .child(hint(theme, "%"))
            .child(div().w(px(8.0)))
            .child(strength_preset("轻柔 25%", 0.25))
            .child(strength_preset("标准 50%", 0.5))
            .child(strength_preset("明显 75%", 0.75))
            .child(strength_preset("强烈 100%", 1.0))
            .into_any_element();

        let divisor_preset = |label: &'static str, value: u32| {
            let weak = weak.clone();
            let active = self.background_render_divisor == value;
            chip(theme, label, active, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.set_background_divisor_preset(value, window, cx);
                });
            })
        };

        let divisor_controls = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .w(px(52.0))
                    .child(self.background_divisor_field.view.clone()),
            )
            .child(hint(theme, "N"))
            .child(div().w(px(8.0)))
            .child(divisor_preset("÷1", 1))
            .child(divisor_preset("÷2", 2))
            .child(divisor_preset("÷4", 4))
            .child(divisor_preset("÷8", 8))
            .child(divisor_preset("÷12", 12))
            .into_any_element();

        let sweep_preset = |label: &'static str, value: f32| {
            let weak = weak.clone();
            let active = (self.lyric_sweep_fade_em - value).abs() < 0.001;
            chip(theme, label, active, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.set_lyric_sweep_preset(value, window, cx);
                });
            })
        };

        let sweep_controls = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(div().w(px(64.0)).child(self.lyric_sweep_field.view.clone()))
            .child(hint(theme, "em"))
            .child(div().w(px(8.0)))
            .child(sweep_preset("0.6", 0.6))
            .child(sweep_preset("1.0", 1.0))
            .child(sweep_preset("1.5", 1.5))
            .child(sweep_preset("2.0", 2.0))
            .into_any_element();

        let eq_switch = {
            let weak = weak.clone();
            toggle(
                theme,
                self.eq_enabled,
                "settings-eq-enabled",
                gpui::rgb(0x3B82F6).into(),
                move |on, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.eq_enabled = on;
                        view.emit(SettingsAction::SetEqEnabled(on), window, cx);
                    });
                },
            )
        };
        let eq_open = {
            let weak = weak.clone();
            small_button(theme, "打开均衡器", true, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.emit(SettingsAction::OpenEqualizer, window, cx);
                });
            })
        };
        let eq_summary = if self.eq_bands == 0 {
            "尚未配置频段".to_string()
        } else {
            format!(
                "{} · {} 个频段 · {}",
                self.eq_name,
                self.eq_bands,
                if self.eq_enabled {
                    "已开启"
                } else {
                    "已关闭"
                }
            )
        };

        section(
            theme,
            "背景律动",
            Some("沉浸歌词背景的网格随该频段能量做呼吸/旋转/提亮；默认 4000–10000 Hz、强度 50%。"),
            vec![
                group(
                    theme,
                    vec![
                        setting_row(
                            theme,
                            "监听频段",
                            Some("支持 4000 / 4k / 10khz 写法，回车应用".into()),
                            band_controls,
                        ),
                        setting_row(
                            theme,
                            "效果强度",
                            Some("0–150%：呼吸/旋转/缩放/提亮的整体倍率，回车应用".into()),
                            strength_controls,
                        ),
                        setting_row(
                            theme,
                            "渲染分辨率除数",
                            Some(
                                "光栅分辨率 = 背景区域 ÷ N；N 越大越省性能、越模糊，默认 2（更接近真实流体细节），回车应用"
                                    .into(),
                            ),
                            divisor_controls,
                        ),
                    ],
                ),
                preset_row,
            ],
        )
        .child(section(
            theme,
            "歌词",
            Some("动态歌词的整行扫光是一条共享的渐变 mask 前沿；软边越宽，越容易跨过相邻的字/词。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "扫光渐变宽度",
                    Some("单位 em（相对主字号），默认 0.6，范围 0.1–4.0，回车应用".into()),
                    sweep_controls,
                )],
            )],
        ))
        .child(section(
            theme,
            "均衡器",
            Some("参数均衡器的开关与详细调节在独立页面完成。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "参数均衡器",
                    Some(eq_summary),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(eq_switch)
                        .child(eq_open)
                        .into_any_element(),
                )],
            )],
        ))
        .child(section(
            theme,
            "音频输出",
            Some("设备名取决于平台：Windows 为 WASAPI 端点 ID，Linux 为 ALSA 设备名。"),
            vec![group(
                theme,
                vec![setting_block(
                    theme,
                    "输出设备",
                    Some("留空使用系统默认设备；修改在下次启动时重建音频后端。".to_string()),
                    badge(theme, "重启生效"),
                    div()
                        .w_full()
                        .child(self.device_field.view.clone())
                        .into_any_element(),
                )],
            )],
        ))
        .into_any_element()
    }
}
