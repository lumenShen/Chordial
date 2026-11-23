//! 设置中心：render_shell（由 settings.rs 拆分，行为不变）。

use super::*;

impl SettingsView {
    pub(super) fn render_sidebar(&self, theme: &Theme, weak: &gpui::WeakEntity<Self>) -> AnyElement {
        let mut nav = div().flex().flex_col().gap(px(3.0));
        for tab in SettingsTab::ALL {
            let weak = weak.clone();
            let active = self.tab == tab;
            let fg = if active { theme.text } else { theme.text_dim };
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("settings-nav-{}", tab.label())))
                    .h(px(36.0))
                    .w_full()
                    .px(px(10.0))
                    .rounded(px(8.0))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .bg(if active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .text_size(px(12.5))
                    .font_weight(if active {
                        gpui::FontWeight::MEDIUM
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(fg)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover).text_color(theme.text))
                    .when(active, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left(px(0.0))
                                .top(px(9.0))
                                .w(px(2.0))
                                .h(px(18.0))
                                .rounded(px(1.0))
                                .bg(theme.text),
                        )
                    })
                    .child(icons::icon(tab.icon(), 15.0, fg))
                    .child(tab.label())
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |view, cx| view.set_tab(tab, cx));
                    }),
            );
        }

        let close = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                theme,
                icons::icon(Icon::Close, 12.0, theme.text_dim),
                26.0,
                "关闭设置",
                move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.emit(SettingsAction::Close, window, cx);
                    });
                },
            )
        };

        div()
            .w(px(NAV_W))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.surface_alt)
            .p(px(16.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb(px(18.0))
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("设置"),
                    )
                    .child(close),
            )
            .child(nav)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .pt(px(12.0))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .child("Chordial 2.0"),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(theme.text_faint)
                            .child(format!(
                                "音频后端 · {}",
                                if self.backend.is_empty() {
                                    "未知"
                                } else {
                                    self.backend.as_str()
                                }
                            )),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_content(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
        cx: &mut Context<Self>,
        width: Option<f32>,
        pad: f32,
    ) -> AnyElement {
        let body: AnyElement = match self.tab {
            SettingsTab::General => self.render_general(theme, weak),
            SettingsTab::Playback => self.render_playback(theme, weak, cx),
            SettingsTab::Sources => self.render_sources(theme, weak, cx),
            SettingsTab::Storage => self.render_storage(theme, weak, cx),
            SettingsTab::About => self.render_about(theme),
        };

        // 桌面固定为「卡片宽 − 导航宽 − 分隔线」；手机/窄窗由外层给
        // `None`，内容随窗口伸缩，避免 800px 卡片把窗口撑出可视区。
        div()
            .when_some(width, |d, w| d.w(px(w)).flex_none())
            .when(width.is_none(), |d| d.w_full().flex_1())
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(pad))
                    .pt(px(22.0))
                    .pb(px(14.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(self.tab.label()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .child(self.tab.subtitle()),
                    ),
            )
            .child(
                div()
                    .id("settings-body-scroll")
                    .w_full()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(self.smooth_scroll, |d| d.smooth_scroll())
                    .px(px(pad))
                    .pb(px(14.0))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(14.0))
                            .pb(px(6.0))
                            .child(body),
                    ),
            )
            .child(self.render_footer(theme, pad))
            .into_any_element()
    }

    pub(super) fn render_footer(&self, theme: &Theme, pad: f32) -> AnyElement {
        let (text, color, icon) = match &self.notice {
            Some(notice) if notice.error => (notice.text.clone(), theme.danger, Some(Icon::Alert)),
            Some(notice) => (notice.text.clone(), theme.text_dim, Some(Icon::Check)),
            None => (
                SharedString::from("回车应用文本修改 · 开关与分段选项即时生效"),
                theme.text_faint,
                None,
            ),
        };
        div()
            .h(px(34.0))
            .flex_none()
            .px(px(pad))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(div().h(px(1.0)).flex_1().bg(theme.border))
            .when_some(icon, |d, icon| d.child(icons::icon(icon, 11.0, color)))
            .child(div().text_size(px(10.0)).text_color(color).child(text))
            .into_any_element()
    }
}
