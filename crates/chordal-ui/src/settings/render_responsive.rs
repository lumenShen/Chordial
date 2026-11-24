//! 设置中心：render_responsive（由 settings.rs 拆分，行为不变）。

use super::*;

impl SettingsView {
    /// 桌面设置：居中固定卡片，左导航 / 右内容。
    pub(super) fn render_desktop(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sidebar = self.render_sidebar(theme, weak);
        let content = self.render_content(theme, weak, cx, Some(CARD_W - NAV_W - 1.0), 24.0);

        let overlay_weak = weak.clone();
        div()
            .id("settings-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            // 整层遮罩既是关闭区域，也是主界面的命中边界。
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000059))
            .on_click(move |_, window, cx| {
                let _ = overlay_weak.update(cx, |view, cx| {
                    view.emit(SettingsAction::Close, window, cx);
                });
            })
            .child(
                div()
                    .id("settings-card")
                    // 卡片内部的空白、分隔行和滚动留白都不能穿透到下面的
                    // 曲目列表 / Dock；子控件仍正常接收点击。
                    .occlude()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    // 固定宽度，避免内部长路径/分段控件把卡片撑宽。
                    .w(px(CARD_W))
                    .min_w(px(CARD_W))
                    .max_w(px(CARD_W))
                    .h(px(CARD_H))
                    .flex_none()
                    .rounded(px(16.0))
                    .border_1()
                    .border_color(theme.border_strong)
                    .bg(theme.surface)
                    .overflow_hidden()
                    .shadow(vec![BoxShadow {
                        color: theme.shadow_strong,
                        offset: point(px(0.0), px(18.0)),
                        blur_radius: px(48.0),
                        spread_radius: px(0.0),
                    }])
                    .flex()
                    .flex_row()
                    .child(sidebar)
                    .child(div().w(px(1.0)).h_full().flex_none().bg(theme.border))
                    .child(content),
            )
            .into_any_element()
    }

    /// 手机/窄窗设置：整窗页面，顶部标题 + 横向导航；不再有卡片阴影，
    /// 避免遮罩在 360px 宽时只剩一条缝。
    pub(super) fn render_phone(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
        cx: &mut Context<Self>,
        vw: f32,
        vh: f32,
    ) -> AnyElement {
        let close = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                theme,
                icons::icon(Icon::Close, 12.0, theme.text_dim),
                32.0,
                "关闭设置",
                move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.emit(SettingsAction::Close, window, cx);
                    });
                },
            )
        };
        let header = div()
            .h(px(54.0))
            .flex_none()
            .px(px(14.0))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("设置"),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .child("Chordial 2.0"),
                    ),
            )
            .child(close);
        let nav = self.render_phone_nav(theme, weak);
        let content = self.render_content(theme, weak, cx, None, 16.0);

        div()
            .id("settings-overlay")
            .absolute()
            .top_0()
            .left_0()
            .w(px(vw))
            .h(px(vh))
            // 窄窗设置铺满窗口，也必须阻断底层主界面的命中。
            .occlude()
            .bg(theme.surface)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(nav)
            .child(div().h(px(1.0)).flex_none().bg(theme.border))
            .child(content)
            .into_any_element()
    }

    /// 手机导航：横向滚动的分段条；比左栏更节省垂直空间。
    pub(super) fn render_phone_nav(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
    ) -> AnyElement {
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(12.0))
            .py(px(6.0));
        for tab in SettingsTab::ALL {
            let weak = weak.clone();
            let active = self.tab == tab;
            let fg = if active { theme.text } else { theme.text_faint };
            row = row.child(
                div()
                    .id(SharedString::from(format!(
                        "settings-phone-nav-{}",
                        tab.label()
                    )))
                    .h(px(32.0))
                    .px(px(10.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .bg(if active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .text_size(px(11.5))
                    .font_weight(if active {
                        gpui::FontWeight::MEDIUM
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(fg)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover).text_color(theme.text))
                    .child(icons::icon(tab.icon(), 14.0, fg))
                    .child(tab.label())
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |view, cx| view.set_tab(tab, cx));
                    }),
            );
        }
        div()
            .id("settings-phone-nav-scroll")
            .w_full()
            .flex_none()
            .overflow_x_scroll()
            .child(row)
            .into_any_element()
    }
}
