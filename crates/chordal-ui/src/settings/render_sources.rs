//! 设置中心：render_sources（由 settings.rs 拆分，行为不变）。

use super::*;

impl SettingsView {
    pub(super) fn render_sources(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let mode = {
            let mut items: Vec<(
                &'static str,
                bool,
                Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
            )> = Vec::new();
            for (mode, label) in [
                (NeteaseMode::Auto, "自动"),
                (NeteaseMode::External, "外部服务"),
                (NeteaseMode::Managed, "托管"),
            ] {
                let weak = weak.clone();
                let active = self.netease_mode == mode;
                items.push((
                    label,
                    active,
                    Box::new(move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.netease_mode = mode;
                            view.emit(SettingsAction::SetNeteaseMode(mode), window, cx);
                            view.notice_ok(format!("网易云运行模式 {label} · 重启后生效"), cx);
                        });
                    }),
                ));
            }
            segmented(theme, "settings-netease-mode", items)
        };
        let quality = {
            let mut items: Vec<(
                &'static str,
                bool,
                Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
            )> = Vec::new();
            for level in [
                SongQualityLevel::Standard,
                SongQualityLevel::Higher,
                SongQualityLevel::ExHigh,
                SongQualityLevel::Lossless,
                SongQualityLevel::HireS,
            ] {
                let weak = weak.clone();
                let active = self.netease_quality == level;
                items.push((
                    level.label(),
                    active,
                    Box::new(move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.netease_quality = level;
                            view.emit(SettingsAction::SetNeteaseQuality(level), window, cx);
                            view.notice_ok(format!("音质档位 {} · 重启后生效", level.label()), cx);
                        });
                    }),
                ));
            }
            segmented(theme, "settings-quality", items)
        };

        let mut roots = div().w_full().flex().flex_col();
        for (index, root) in self.local_roots.iter().enumerate() {
            let weak_remove = weak.clone();
            roots = roots.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .py(px(6.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.5))
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(SharedString::from(root.display().to_string())),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("settings-root-remove-{index}")))
                            .h(px(24.0))
                            .px(px(8.0))
                            .rounded(px(6.0))
                            .flex()
                            .items_center()
                            .text_size(px(10.5))
                            .text_color(theme.danger)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.hover))
                            .child("移除")
                            .on_click(move |_, window, cx| {
                                let _ = weak_remove.update(cx, |view, cx| {
                                    view.remove_local_root(index, window, cx);
                                });
                            }),
                    ),
            );
        }
        let rescan = {
            let weak = weak.clone();
            small_button(theme, "重新扫描", false, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.emit(SettingsAction::RescanLibrary, window, cx);
                    view.notice_ok("正在重新扫描本地音乐库…", cx);
                });
            })
        };
        let add_root = {
            let weak = weak.clone();
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(self.add_root_field.view.clone()),
                )
                .child(small_button(
                    theme,
                    "添加",
                    false,
                    move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            let text = view.add_root_field.state.read(cx).content().to_string();
                            view.submit_add_root(text, window, cx);
                        });
                    },
                ))
                .child(rescan)
                .into_any_element()
        };

        let timeout = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .w(px(110.0))
                    .child(self.search_timeout_field.view.clone()),
            )
            .child(hint(theme, "ms"))
            .into_any_element();

        section(
            theme,
            "网易云音乐",
            Some("托管 api-enhanced 时地址由 sidecar 自动分配，外部服务模式才需要填写。"),
            vec![group(
                theme,
                vec![
                    setting_row(
                        theme,
                        "运行模式",
                        Some("自动：优先外部服务，缺失时托管；托管会启动/安装 api-enhanced".into()),
                        mode,
                    ),
                    setting_row(
                        theme,
                        "音质档位",
                        Some("请求直链时使用的 level；重启后生效".into()),
                        quality,
                    ),
                    setting_block(
                        theme,
                        "服务地址",
                        Some("外部服务填根地址，如 http://127.0.0.1:3000".into()),
                        badge(theme, "重启生效"),
                        div()
                            .w_full()
                            .child(self.netease_url_field.view.clone())
                            .into_any_element(),
                    ),
                ],
            )],
        )
        .child(section(
            theme,
            "聚合搜索",
            Some("给每个音源单独计时；超时的源不会拖慢整体搜索。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "每源超时",
                    Some("500–60000 ms，回车应用".into()),
                    timeout,
                )],
            )],
        ))
        .child(section(
            theme,
            "本地音乐",
            Some("目录变更会写入配置；点击重新扫描后建立索引。"),
            vec![group(
                theme,
                vec![
                    div()
                        .w_full()
                        .py(px(4.0))
                        .flex()
                        .flex_col()
                        .child(roots)
                        .child(div().h(px(6.0)))
                        .child(add_root)
                        .into_any_element(),
                ],
            )],
        ))
        .into_any_element()
    }
}
