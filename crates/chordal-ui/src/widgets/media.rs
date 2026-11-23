//! widgets：media（由 widgets.rs 拆分，行为不变）。

use super::*;

pub fn cover_thumb(
    theme: &Theme,
    image: Option<Arc<RenderImage>>,
    size: f32,
    radius: f32,
    placeholder: AnyElement,
) -> AnyElement {
    let inner: AnyElement = match image {
        Some(image) => img(ImageSource::Render(image))
            .size(px(size))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => placeholder,
    };
    div()
        .size(px(size))
        .rounded(px(radius))
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .flex()
        .items_center()
        .justify_center()
        .child(inner)
        .into_any_element()
}

/// 无边框封面（沉浸层大图，避免像相框）。
pub fn cover_plain(
    theme: &Theme,
    image: Option<Arc<RenderImage>>,
    size: f32,
    radius: f32,
    placeholder: AnyElement,
) -> AnyElement {
    let inner: AnyElement = match image {
        Some(image) => img(ImageSource::Render(image))
            .size(px(size))
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => placeholder,
    };
    div()
        .size(px(size))
        .rounded(px(radius))
        .overflow_hidden()
        .bg(theme.surface_alt)
        .flex()
        .items_center()
        .justify_center()
        .child(inner)
        .into_any_element()
}

pub fn equalizer(theme: &Theme, playing: bool) -> AnyElement {
    let color = if playing {
        theme.playing
    } else {
        theme.text_faint
    };
    div()
        .flex()
        .items_end()
        .gap(px(2.0))
        .h(px(11.0))
        .children(
            [5.0, 10.0, 7.0]
                .into_iter()
                .map(move |h| div().w(px(2.0)).h(px(h)).rounded(px(1.0)).bg(color)),
        )
        .into_any_element()
}

pub fn empty_state(theme: &Theme, icon: AnyElement, title: &str, hint: &str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .pt(px(40.0))
        .child(icon)
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(SharedString::from(title.to_string())),
        )
        .child(
            div()
                .max_w(px(360.0))
                .text_center()
                .text_size(px(12.0))
                .line_height(px(18.0))
                .text_color(theme.text_faint)
                .child(SharedString::from(hint.to_string())),
        )
        .into_any_element()
}

/// 开/关开关（均衡器启用、频段启用）。点击时停止冒泡，避免卡片同时被选中。
pub fn switch(
    on: bool,
    accent: Hsla,
    id: impl Into<SharedString>,
    on_change: impl Fn(bool, &mut App) + 'static,
) -> AnyElement {
    let knob = if on { 20.0 } else { 2.0 };
    let id: SharedString = id.into();
    let element_id = SharedString::from(format!("switch-{}", id.as_ref()));
    div()
        .id(element_id)
        .w(px(42.0))
        .h(px(22.0))
        .rounded(px(11.0))
        .bg(if on {
            accent
        } else {
            gpui::white().opacity(0.18)
        })
        .relative()
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            on_change(!on, cx);
            cx.refresh_windows();
        })
        .child(
            div()
                .absolute()
                .top(px(2.0))
                .left(px(knob))
                .size(px(18.0))
                .rounded(px(9.0))
                .bg(gpui::white()),
        )
        .into_any_element()
}
