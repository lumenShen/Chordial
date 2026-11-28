//! `ChordialApp` 的 render_settings_login 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let _ = cx;
        // 设置页是独立视图：未通知（无点击/输入/滚轮/内存采样刷新）时，
        // 复用上一帧的布局与绘制记录。否则即使主界面只是在跑动画，GPUI
        // 仍会每帧重排整棵设置静态树（300Hz 实测 prepaint ~1.6ms）。
        // 鼠标悬停/点击会通过 window.refresh 或实体 notify 自动绕过缓存。
        if std::env::var_os("CHORDIAL_NO_VIEW_CACHE").is_some() {
            return self.settings_view.clone().into_any_element();
        }
        let style = gpui::StyleRefinement::default()
            .absolute()
            .top_0()
            .left_0()
            .size_full();
        let view: gpui::AnyView = self.settings_view.clone().into();
        view.cached(style).into_any_element()
    }

    /// 旧版设置面板：已由 `settings.rs` 的独立实体取代，保留一版用于对照，
    /// 待确认新版无回归后删除。
    #[allow(dead_code)]
    pub(super) fn render_settings_legacy(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let stats = self.cache.stats();
        let used = format_bytes(stats.total_bytes);
        let limit = format_bytes(stats.limit_bytes);
        let entries = stats.entries;
        let root = stats.root.display().to_string();
        let download_root = self.downloads.root().display().to_string();

        let section = |title: &'static str| {
            div()
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_faint)
                .child(title)
                .into_any_element()
        };
        let hint = |text: String| {
            div()
                .text_size(px(10.0))
                .text_color(theme.text_faint)
                .child(SharedString::from(text))
                .into_any_element()
        };

        let preset_btn = |label: &'static str, bytes: u64| {
            let weak = weak.clone();
            let active = stats.limit_bytes == bytes;
            div()
                .id(SharedString::from(format!("cache-limit-{label}")))
                .h(px(26.0))
                .px(px(9.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(if active { theme.text } else { theme.border })
                .bg(if active {
                    theme.active
                } else {
                    gpui::transparent_black()
                })
                .flex()
                .items_center()
                .text_size(px(10.5))
                .text_color(if active { theme.text } else { theme.text_dim })
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .child(label)
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_cache_limit_bytes(bytes, cx));
                })
        };
        let adjust_btn = |label: &'static str, delta: i64| {
            let weak = weak.clone();
            let current = stats.limit_bytes as i64;
            let next = (current + delta).clamp(256 * 1024 * 1024, 100 * 1024 * 1024 * 1024) as u64;
            div()
                .id(SharedString::from(format!("cache-adjust-{label}")))
                .size(px(26.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.border)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.0))
                .text_color(theme.text_dim)
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .child(label)
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_cache_limit_bytes(next, cx));
                })
        };

        let pref_btn = |pref: SourcePreference| {
            let weak = weak.clone();
            let active = self.source_preference == pref;
            div()
                .id(SharedString::from(format!("pref-{}", pref.label())))
                .h(px(28.0))
                .px(px(10.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(if active { theme.text } else { theme.border })
                .bg(if active {
                    theme.active
                } else {
                    gpui::transparent_black()
                })
                .flex()
                .items_center()
                .text_size(px(11.0))
                .text_color(if active { theme.text } else { theme.text_dim })
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .child(pref.label())
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_source_preference(pref, cx));
                })
        };

        let clear_btn = {
            let weak = weak.clone();
            div()
                .id("cache-clear")
                .h(px(26.0))
                .px(px(9.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.border)
                .flex()
                .items_center()
                .text_size(px(10.5))
                .text_color(theme.danger)
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .child("清空缓存")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.clear_cache_now(cx));
                })
        };
        let close_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Close, 12.0, theme.text_dim),
                26.0,
                "关闭设置",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_settings(cx));
                },
            )
        };

        let band_rows: Vec<AnyElement> = AudioBandKind::ALL
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                let range = self.audio_band_ranges.get(kind);
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(hint(format!(
                        "{} · 当前 {:.0}–{:.0} Hz",
                        kind.label(),
                        range.low_hz,
                        range.high_hz
                    )))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(hint("频率".into()))
                            .child(
                                div()
                                    .w(px(110.0))
                                    .child(self.settings_audio_low_views[index].clone()),
                            )
                            .child(hint("–".into()))
                            .child(
                                div()
                                    .w(px(110.0))
                                    .child(self.settings_audio_high_views[index].clone()),
                            )
                            .child(hint("Hz".into())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(hint("控制点".into()))
                            .child(
                                div()
                                    .w(px(260.0))
                                    .child(self.settings_audio_control_views[index].clone()),
                            )
                            .child(hint("1–25，仅内部点".into())),
                    )
                    .into_any_element()
            })
            .collect();

        let card = div()
            .id("settings-card")
            .w(px(620.0))
            .max_h(px(640.0))
            .overflow_y_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll())
            .rounded(px(14.0))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.surface)
            .shadow(vec![BoxShadow {
                color: theme.shadow_strong,
                offset: point(px(0.0), px(12.0)),
                blur_radius: px(36.0),
                spread_radius: px(0.0),
            }])
            .p(px(20.0))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("设置"),
                    )
                    .child(close_btn),
            )
            .child(section("对象缓存"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(hint(format!(
                        "已用 {used} / {limit} · {entries} 项 · 目录（回车应用）"
                    )))
                    .child(self.settings_cache_view.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(preset_btn("1 GB", 1024 * 1024 * 1024))
                    .child(preset_btn("5 GB", 5 * 1024 * 1024 * 1024))
                    .child(preset_btn("10 GB", 10 * 1024 * 1024 * 1024))
                    .child(preset_btn("20 GB", 20 * 1024 * 1024 * 1024))
                    .child(adjust_btn("−", -(1024i64 * 1024 * 1024)))
                    .child(adjust_btn("+", 1024 * 1024 * 1024))
                    .child(div().flex_1())
                    .child(clear_btn),
            )
            .child(hint(format!("实际缓存根目录：{root}")))
            .child(section("显式下载"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(hint(
                        "下载文件直接写入该目录，不计入缓存配额；播放时优先使用。回车应用。".into(),
                    ))
                    .child(self.settings_download_view.clone()),
            )
            .child(hint(format!("实际下载目录：{download_root}")))
            .child(section("播放来源（本地同曲 vs 在线）"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(pref_btn(SourcePreference::Auto))
                    .child(pref_btn(SourcePreference::PreferLocal))
                    .child(pref_btn(SourcePreference::PreferOnline))
                    .child(div().flex_1())
                    .child(hint(
                        "音质优先：仅当本地更高时才播本地；在线优先：除已下载外始终在线".into(),
                    )),
            )
            .child(section("背景音频反应"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(hint(
                        "五路频率可分别编辑；控制点每行 5 个：1–5 / 6–10 / 11–15 / 16–20 / 21–25，仅内部点可分配。".into(),
                    ))
                    .children(band_rows),
            )
            .child(section("均衡器"))
            .child({
                let blue: gpui::Hsla = gpui::rgb(0x3B82F6).into();
                let weak_eq = weak.clone();
                let weak_toggle = weak.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(hint(format!(
                        "{} · {} 个频段 · {}",
                        self.eq.display_name(),
                        self.eq.settings.bands.len(),
                        if self.eq.settings.enabled {
                            "已开启"
                        } else {
                            "已关闭"
                        }
                    )))
                    .child(div().flex_1())
                    .child(crate::widgets::switch(
                        self.eq.settings.enabled,
                        blue,
                        "settings-eq-enabled",
                        move |on, cx| {
                            let _ = weak_toggle.update(cx, |app, cx| {
                                app.set_eq_enabled(on, cx);
                            });
                        },
                    ))
                    .child(
                        div()
                            .id("settings-open-eq")
                            .h(px(28.0))
                            .px(px(10.0))
                            .rounded(px(7.0))
                            .bg(theme.text)
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_size(px(11.0))
                            .text_color(theme.on_accent)
                            .hover(|s| s.bg(theme.accent_hover))
                            .child("打开均衡器")
                            .on_click(move |_, _, cx| {
                                let _ = weak_eq.update(cx, |app, cx| {
                                    app.settings_open = false;
                                    app.open_eq(cx);
                                });
                            }),
                    )
            });

        let weak_overlay = weak.clone();
        div()
            .id("settings-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000066))
            .on_click(move |_, _, cx| {
                let _ = weak_overlay.update(cx, |app, cx| app.close_settings(cx));
            })
            .child(
                div()
                    .id("settings-card-wrap")
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(card),
            )
            .into_any_element()
    }

    // ================= 登录面板 =================

    pub(super) fn render_login(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let tab = self.login_tab;
        let source_name = self
            .login_source_id()
            .and_then(|id| self.source_config_by_id(&id.0))
            .map(|source| source.name.clone())
            .unwrap_or_else(|| "音源".into());
        let phone = self.is_phone();
        let status = self.login_status.clone().unwrap_or_else(|| match tab {
            LoginTab::Qr => "等待扫码".into(),
            LoginTab::Phone => "输入手机号并获取验证码".into(),
        });
        let close_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Close, 12.0, theme.text_dim),
                if phone { 44.0 } else { 26.0 },
                "关闭登录",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_login(cx));
                },
            )
        };
        let tab_btn = |target: LoginTab, label: &'static str| {
            let weak = weak.clone();
            let active = tab == target;
            div()
                .id(SharedString::from(format!("login-tab-{label}")))
                .h(px(if phone { 44.0 } else { 26.0 }))
                .px(px(10.0))
                .rounded(px(6.0))
                .bg(if active {
                    theme.active
                } else {
                    gpui::transparent_black()
                })
                .flex()
                .items_center()
                .text_size(px(11.0))
                .font_weight(if active {
                    gpui::FontWeight::MEDIUM
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(if active { theme.text } else { theme.text_faint })
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover).text_color(theme.text))
                .child(label)
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.switch_login_tab(target, cx));
                })
        };

        let body: AnyElement = match tab {
            LoginTab::Qr => {
                let placeholder = div()
                    .size_full()
                    .bg(theme.surface_alt)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::icons::icon(
                        crate::icons::Icon::Refresh,
                        28.0,
                        theme.text_faint,
                    ))
                    .into_any_element();
                let qr = crate::widgets::cover_thumb(
                    &theme,
                    self.login_qr.clone(),
                    if phone { 180.0 } else { 216.0 },
                    10.0,
                    placeholder,
                );
                let refresh_btn = {
                    let weak = weak.clone();
                    crate::widgets::text_button_with_height(
                        &theme,
                        "刷新二维码",
                        false,
                        if phone { 40.0 } else { 30.0 },
                        move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.refresh_login(cx));
                        },
                    )
                };
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.0))
                    .child(qr)
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.text_dim)
                            .child(SharedString::from(status)),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .text_center()
                            .child("按音源提示完成扫码登录；登录状态只保存在本机"),
                    )
                    .child(refresh_btn)
                    .into_any_element()
            }
            LoginTab::Phone => {
                let cooldown = self
                    .captcha_cooldown_until
                    .is_some_and(|until| until > Instant::now());
                let send_btn = {
                    let weak = weak.clone();
                    let label = if cooldown {
                        "已发送"
                    } else {
                        "发送验证码"
                    };
                    crate::widgets::text_button_with_height(
                        &theme,
                        label,
                        false,
                        if phone { 40.0 } else { 30.0 },
                        move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.send_login_captcha(cx));
                        },
                    )
                };
                let login_btn = {
                    let weak = weak.clone();
                    crate::widgets::text_button_with_height(
                        &theme,
                        "登录",
                        true,
                        if phone { 40.0 } else { 30.0 },
                        move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.submit_phone_login(cx));
                        },
                    )
                };
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(self.login_phone_view.clone())
                    .child(self.login_captcha_view.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(send_btn)
                            .child(login_btn),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_dim)
                            .child(SharedString::from(status)),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .child("验证码 5 分钟内有效；登录状态只保存在本机"),
                    )
                    .into_any_element()
            }
        };

        let card_w = (self.viewport_width - 24.0).clamp(300.0, 380.0);
        let card_pad = if self.viewport_width < 420.0 {
            16.0
        } else {
            20.0
        };
        let card = div()
            .id("login-card")
            .w(px(card_w))
            .rounded(px(14.0))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.surface)
            .shadow(vec![BoxShadow {
                color: theme.shadow_strong,
                offset: point(px(0.0), px(12.0)),
                blur_radius: px(36.0),
                spread_radius: px(0.0),
            }])
            .p(px(card_pad))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(format!("登录{}", source_name)),
                    )
                    .child(close_btn),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(tab_btn(LoginTab::Qr, "扫码登录"))
                    .child(tab_btn(LoginTab::Phone, "手机号登录")),
            )
            .child(body);

        let weak_overlay = weak.clone();
        div()
            .id("login-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000066))
            .on_click(move |_, _, cx| {
                let _ = weak_overlay.update(cx, |app, cx| app.close_login(cx));
            })
            .child(
                div()
                    .id("login-card-wrap")
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(card),
            )
            .into_any_element()
    }
}
