//! 设置中心：render_about（由 settings.rs 拆分，行为不变）。

use super::*;

impl SettingsView {
    pub(super) fn render_about(&self, theme: &Theme) -> AnyElement {
        let path_row = |label: &'static str, value: String| {
            div()
                .w_full()
                .py(px(9.0))
                .flex()
                .items_center()
                .gap(px(16.0))
                .child(
                    div()
                        .w(px(88.0))
                        .flex_none()
                        .text_size(px(11.5))
                        .text_color(theme.text_dim)
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(theme.font_mono.clone())
                        .text_size(px(10.5))
                        .text_color(theme.text)
                        .text_ellipsis()
                        .child(SharedString::from(value)),
                )
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .child(
                        div()
                            .size(px(52.0))
                            .rounded(px(14.0))
                            .bg(theme.text)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icons::icon(Icon::Note, 24.0, theme.on_accent)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.text)
                                    .child("Chordial 2.0"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.text_faint)
                                    .child("GPUI + Rust · 多音源可载可卸 · AMLL 等价歌词"),
                            ),
                    )
                    .into_any_element(),
            )
            .child(group(
                theme,
                vec![
                    path_row("版本", env!("CARGO_PKG_VERSION").to_string()),
                    path_row(
                        "音频后端",
                        if self.backend.is_empty() {
                            "未知".to_string()
                        } else {
                            self.backend.clone()
                        },
                    ),
                    path_row("配置文件", self.config_path.display().to_string()),
                    path_row("缓存目录", self.cache_root.display().to_string()),
                    path_row("下载目录", self.download_root.display().to_string()),
                ],
            ))
            .into_any_element()
    }
}
