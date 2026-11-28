//! EQ 页面支撑代码：controls（由 eq_page.rs 拆分，行为不变）。

use super::drawing::*;
use super::*;

// ================= 基础控件 =================

pub(super) fn eq_icon_button(
    pal: &EqPalette,
    icon: Icon,
    size: f32,
    id: impl Into<SharedString>,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let trigger = div()
        .id(id.clone())
        .size(px(size))
        .rounded(px(size * 0.28))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.bg(pal.chip_hover))
        .active(|s| s.bg(pal.chip))
        .child(icons::icon(icon, size * 0.46, pal.text_dim))
        .into_any_element();
    // 均衡器页是独立深色面板：气泡沿用「文字色作底、页面底色作字」的反色，
    // 不直接套全局 Theme，避免在深色面板里变成一块同样深的气泡。
    crate::bubble::bubble_button(
        crate::bubble::BubbleStyle {
            bg: pal.text,
            fg: pal.bg_top,
            shadow: pal.shadow,
            font_family: pal.font.clone(),
        },
        id,
        label,
        trigger,
        on_click,
    )
}

pub(super) fn eq_section_label(pal: &EqPalette, text: &'static str) -> AnyElement {
    div()
        .px(px(8.0))
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(pal.text_faint)
        .child(text)
        .into_any_element()
}

pub(super) fn eq_divider(pal: &EqPalette) -> AnyElement {
    div()
        .w_full()
        .h(px(1.0))
        .flex_none()
        .bg(pal.divider)
        .into_any_element()
}

pub(super) fn eq_chip(
    pal: &EqPalette,
    label: &'static str,
    active: bool,
    id: impl Into<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id.into())
        .h(px(28.0))
        .px(px(11.0))
        .rounded(px(14.0))
        .border_1()
        .border_color(if active {
            pal.accent_line
        } else {
            gpui::transparent_black()
        })
        .bg(if active { pal.accent_soft } else { pal.chip })
        .flex()
        .items_center()
        .cursor_pointer()
        .text_size(px(10.5))
        .text_color(if active { pal.accent } else { pal.text_dim })
        .hover(move |s| {
            s.bg(if active {
                pal.accent_soft
            } else {
                pal.chip_hover
            })
        })
        .child(label)
        .on_click(on_click)
        .into_any_element()
}

pub(super) fn eq_search_box(
    pal: &EqPalette,
    view: gpui::Entity<crate::input::InputView>,
) -> AnyElement {
    div()
        .h(px(38.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(pal.border)
        .bg(pal.chip)
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(11.0))
        .child(icons::icon(Icon::Search, 14.0, pal.text_faint))
        .child(div().flex_1().min_w_0().child(view))
        .into_any_element()
}

pub(super) fn eq_param_block(
    pal: &EqPalette,
    label: &'static str,
    value: String,
    slider: AnyElement,
) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(1.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(pal.text_dim)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.accent)
                        .child(SharedString::from(value)),
                ),
        )
        .child(slider)
        .into_any_element()
}

pub(super) fn eq_stat(pal: &EqPalette, label: &'static str, value: String) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(1.0))
        .child(
            div()
                .text_size(px(9.0))
                .text_color(pal.text_faint)
                .child(label),
        )
        .child(
            div()
                .text_size(px(11.5))
                .font_family(pal.mono.clone())
                .text_color(pal.text)
                .child(SharedString::from(value)),
        )
        .into_any_element()
}
// ================= 预设行 / 预览 =================

#[allow(clippy::too_many_arguments)] // 行控件的回调/状态参数拆结构体会降低调用点可读性
pub(super) fn eq_preset_row(
    pal: &EqPalette,
    name: &str,
    subtitle: String,
    icon: Option<Icon>,
    badge: Option<String>,
    active: bool,
    previewed: bool,
    compact: bool,
    id: SharedString,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let height = if compact { 58.0 } else { 54.0 };
    div()
        .id(id)
        .w_full()
        .h(px(height))
        .flex_none()
        .rounded(px(11.0))
        .border_1()
        .border_color(if active {
            pal.accent_line
        } else if previewed {
            pal.accent_line.opacity(0.45)
        } else {
            pal.border
        })
        .bg(if active {
            pal.selected_bg
        } else if previewed {
            pal.accent_soft.opacity(0.45)
        } else {
            pal.panel
        })
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .cursor_pointer()
        .hover(|s| s.bg(pal.chip_hover))
        .child(
            div()
                .size(px(32.0))
                .rounded(px(9.0))
                .bg(if active { pal.accent_soft } else { pal.chip })
                .flex()
                .items_center()
                .justify_center()
                .child(match icon {
                    Some(icon) => {
                        icons::icon(icon, 15.0, if active { pal.accent } else { pal.text_dim })
                    }
                    None => div().into_any_element(),
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(if active { pal.accent } else { pal.text })
                                .text_ellipsis()
                                .child(SharedString::from(name.to_string())),
                        )
                        .when_some(badge, |d, badge| {
                            d.child(
                                div()
                                    .h(px(15.0))
                                    .px(px(5.0))
                                    .rounded(px(4.0))
                                    .bg(pal.chip)
                                    .flex()
                                    .items_center()
                                    .text_size(px(8.0))
                                    .text_color(pal.text_faint)
                                    .child(SharedString::from(badge)),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .text_ellipsis()
                        .child(SharedString::from(subtitle)),
                ),
        )
        .when(active, |d| {
            d.child(icons::icon(Icon::Check, 13.0, pal.accent))
        })
        .when(previewed && !active, |d| {
            d.child(
                div()
                    .h(px(16.0))
                    .px(px(6.0))
                    .rounded(px(8.0))
                    .bg(pal.accent_soft)
                    .flex()
                    .items_center()
                    .text_size(px(8.0))
                    .text_color(pal.accent)
                    .child("预览"),
            )
        })
        .on_click(on_click)
        .into_any_element()
}

/// AutoEQ 同步按钮：拉取中只显示状态，避免重复点击。
pub(super) fn eq_sync_chip(
    pal: &EqPalette,
    id: &'static str,
    busy: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let chip = div()
        .id(id)
        .h(px(26.0))
        .px(px(10.0))
        .rounded(px(13.0))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(5.0))
        .bg(if busy { pal.chip } else { pal.accent_soft })
        .child(icons::icon(
            Icon::Refresh,
            11.0,
            if busy { pal.text_faint } else { pal.accent },
        ))
        .child(
            div()
                .text_size(px(9.5))
                .text_color(if busy { pal.text_faint } else { pal.accent })
                .child(if busy { "同步中" } else { "同步" }),
        );
    if busy {
        chip.into_any_element()
    } else {
        chip.cursor_pointer()
            .hover(|s| s.bg(pal.accent_soft.opacity(0.8)))
            .on_click(on_click)
            .into_any_element()
    }
}

/// AutoEQ 的状态提示卡片：加载 / 失败 / 空列表共用。
pub(super) fn eq_notice(
    pal: &EqPalette,
    icon: Icon,
    title: &str,
    hint: &str,
    action: Option<(&str, Rc<dyn Fn(&mut App)>)>,
) -> AnyElement {
    let mut card = div()
        .w_full()
        .h_full()
        .min_h(px(260.0))
        .rounded(px(14.0))
        .border_1()
        .border_color(pal.border)
        .bg(pal.panel)
        .p(px(16.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(7.0))
        .child(icons::icon(icon, 20.0, pal.text_faint))
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(pal.text_dim)
                .text_center()
                .child(SharedString::from(title.to_string())),
        )
        .child(
            div()
                .max_w(px(250.0))
                .text_size(px(9.5))
                .text_color(pal.text_faint)
                .text_center()
                .child(SharedString::from(hint.to_string())),
        );
    if let Some((label, on_action)) = action {
        card = card.child(
            div()
                .id("eq-notice-action")
                .mt(px(2.0))
                .h(px(28.0))
                .px(px(14.0))
                .rounded(px(14.0))
                .bg(pal.accent_soft)
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(10.0))
                .text_color(pal.accent)
                .hover(|s| s.bg(pal.accent_soft.opacity(0.8)))
                .child(SharedString::from(label.to_string()))
                .on_click(move |_, _, cx| on_action(cx)),
        );
    }
    card.into_any_element()
}

pub(super) fn autoeq_status_text(lib: &AutoEqLibrary, filtered: usize) -> String {
    if lib.entries.is_empty() {
        if lib.index_loading || lib.status == AutoEqStatus::Loading {
            "正在拉取索引…".into()
        } else if lib.status == AutoEqStatus::Failed {
            "拉取失败".into()
        } else {
            "尚未拉取 · 来自 AutoEq 仓库".into()
        }
    } else if lib.index_loading {
        format!("{} 款 · 正在同步…", lib.entries.len())
    } else {
        format!(
            "{} / {} 款 · {}",
            filtered,
            lib.entries.len(),
            if lib.from_cache {
                "本地缓存"
            } else {
                "已同步"
            }
        )
    }
}

pub(super) fn eq_preview_empty(pal: &EqPalette, hint: &'static str) -> AnyElement {
    div()
        .w_full()
        .h_full()
        .min_h(px(280.0))
        .rounded(px(14.0))
        .border_1()
        .border_color(pal.border)
        .bg(pal.panel)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .child(icons::icon(Icon::Sliders, 20.0, pal.text_faint))
        .child(
            div()
                .text_size(px(10.5))
                .text_color(pal.text_faint)
                .child(hint),
        )
        .into_any_element()
}

pub(super) fn eq_preview_panel(
    pal: &EqPalette,
    settings: &EqSettings,
    name: &str,
    subtitle: &str,
    active: bool,
    on_apply: Rc<dyn Fn(&mut App)>,
    on_remove: Option<Rc<dyn Fn(&mut App)>>,
) -> AnyElement {
    let pal_paint = pal.clone();
    let paint_settings = settings.clone();
    let graph = div()
        .w_full()
        .h(px(148.0))
        .rounded(px(10.0))
        .bg(pal.inset)
        .border_1()
        .border_color(pal.border)
        .overflow_hidden()
        .child(
            canvas(
                |bounds, _, _| bounds,
                move |_bounds, bounds, window, _| {
                    paint_eq_graph(
                        bounds,
                        &paint_settings,
                        &[],
                        None,
                        true,
                        0,
                        &pal_paint,
                        window,
                    );
                },
            )
            .size_full(),
        );

    let band_count = settings.bands.len();
    let max_boost = settings
        .bands
        .iter()
        .filter(|band| band.enabled && band.kind.uses_gain())
        .map(|band| band.gain_db)
        .fold(0.0f32, f32::max);
    let max_cut = settings
        .bands
        .iter()
        .filter(|band| band.enabled && band.kind.uses_gain())
        .map(|band| band.gain_db)
        .fold(0.0f32, f32::min);

    let mut list = div().w_full().flex().flex_col().gap(px(4.0));
    for band in settings.bands.iter().take(4) {
        list = list.child(
            div()
                .w_full()
                .h(px(19.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .border_b_1()
                .border_color(pal.divider)
                .child(
                    div()
                        .w(px(28.0))
                        .text_size(px(8.0))
                        .text_color(pal.text_faint)
                        .child(band.kind.label()),
                )
                .child(
                    div()
                        .w(px(44.0))
                        .text_size(px(8.5))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_dim)
                        .child(SharedString::from(fmt_freq(band.freq_hz))),
                )
                .child(
                    div()
                        .w(px(44.0))
                        .text_size(px(8.5))
                        .font_family(pal.mono.clone())
                        .text_color(if band.gain_db.abs() > 0.05 {
                            pal.accent
                        } else {
                            pal.text_faint
                        })
                        .child(SharedString::from(format!("{} dB", db_num(band.gain_db)))),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(px(8.5))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .child(SharedString::from(format!("Q {:.2}", band.q))),
                ),
        );
    }
    if band_count > 4 {
        list = list.child(div().text_size(px(8.5)).text_color(pal.text_faint).child(
            SharedString::from(format!("另有 {} 个滤波器", band_count - 4)),
        ));
    }

    let apply = {
        let on_apply = on_apply.clone();
        div()
            .id(SharedString::from(format!("eq-apply-{}", name)))
            .h(px(36.0))
            .w_full()
            .rounded(px(9.0))
            .border_1()
            .border_color(if active {
                pal.accent_line
            } else {
                gpui::transparent_black()
            })
            .bg(if active { pal.accent_soft } else { pal.accent })
            .flex()
            .items_center()
            .justify_center()
            .gap(px(5.0))
            .cursor_pointer()
            .text_size(px(11.5))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(if active { pal.accent } else { pal.on_accent })
            .hover(move |s| {
                s.bg(if active {
                    pal.accent_soft.opacity(0.85)
                } else {
                    pal.accent_hover
                })
            })
            .when(active, |d| {
                d.child(icons::icon(Icon::Check, 12.0, pal.accent))
            })
            .child(if active {
                "当前使用"
            } else {
                "应用此预设"
            })
            .on_click(move |_, _, cx| {
                let on_apply = on_apply.clone();
                on_apply(cx);
            })
            .into_any_element()
    };

    let remove = match on_remove {
        Some(on_remove) => {
            let on_remove = on_remove.clone();
            div()
                .id("eq-preview-remove")
                .h(px(28.0))
                .w_full()
                .rounded(px(8.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(9.5))
                .text_color(pal.danger)
                .hover(|s| s.bg(pal.danger_soft))
                .child("删除这个配置")
                .on_click(move |_, _, cx| on_remove(cx))
                .into_any_element()
        }
        None => div().into_any_element(),
    };

    div()
        .w_full()
        .h_full()
        .min_h(px(460.0))
        .rounded(px(14.0))
        .border_1()
        .border_color(pal.border)
        .bg(pal.panel)
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(9.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("预览"),
                )
                .child(
                    div()
                        .h(px(16.0))
                        .px(px(6.0))
                        .rounded(px(8.0))
                        .bg(if active { pal.accent_soft } else { pal.chip })
                        .flex()
                        .items_center()
                        .text_size(px(8.0))
                        .text_color(if active { pal.accent } else { pal.text_faint })
                        .child(if active { "正在使用" } else { "未应用" }),
                ),
        )
        .child(graph)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(pal.text)
                        .text_ellipsis()
                        .child(SharedString::from(name.to_string())),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .text_ellipsis()
                        .child(SharedString::from(subtitle.to_string())),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .gap(px(8.0))
                .child(eq_stat(pal, "滤波器", band_count.to_string()))
                .child(eq_stat(
                    pal,
                    "最大提升",
                    format!("{} dB", db_num(max_boost)),
                ))
                .child(eq_stat(pal, "最大衰减", format!("{} dB", db_num(max_cut)))),
        )
        .child(eq_divider(pal))
        .child(list)
        .child(div().flex_1())
        .child(apply)
        .child(remove)
        .into_any_element()
}

// ================= 链路 / 预设库 =================

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PowerDot {
    Off,
    On,
    Bypassed,
}

/// 15px 的启用圆点：实心 = 启用，描边 + 小点 = 已加入但旁路，空环 = 未加入。
pub(super) fn eq_power_dot(
    pal: &EqPalette,
    state: PowerDot,
    id: SharedString,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let (bg, border, inner): (Hsla, Hsla, AnyElement) = match state {
        PowerDot::On => (
            pal.accent,
            pal.accent,
            icons::icon(Icon::Check, 8.0, pal.on_accent),
        ),
        PowerDot::Bypassed => (pal.accent_soft, pal.accent_line, {
            div()
                .size(px(4.0))
                .rounded(px(2.0))
                .bg(pal.text_faint)
                .into_any_element()
        }),
        PowerDot::Off => (
            gpui::transparent_black(),
            pal.border_strong,
            div().into_any_element(),
        ),
    };
    div()
        .id(id)
        .size(px(15.0))
        .flex_none()
        .rounded(px(7.5))
        .border_1()
        .border_color(border)
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.border_color(pal.accent_line))
        .child(inner)
        .on_click(on_click)
        .into_any_element()
}

pub(super) fn eq_mini_icon_button(
    _pal: &EqPalette,
    icon: Icon,
    size: f32,
    id: SharedString,
    color: Hsla,
    hover_bg: Hsla,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .size(px(size))
        .flex_none()
        .rounded(px(size * 0.28))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .child(icons::icon(icon, size * 0.54, color))
        .on_click(move |event, window, cx| {
            // 行内的删除 / 局部动作不应冒泡成「选中整行」。
            cx.stop_propagation();
            on_click(event, window, cx);
        })
        .into_any_element()
}

pub(super) fn eq_tag(
    _pal: &EqPalette,
    text: impl Into<SharedString>,
    color: Hsla,
    bg: Hsla,
) -> AnyElement {
    div()
        .h(px(15.0))
        .px(px(5.0))
        .rounded(px(4.0))
        .bg(bg)
        .flex()
        .flex_none()
        .items_center()
        .text_size(px(8.0))
        .text_color(color)
        .child(text.into())
        .into_any_element()
}

/// 链路卡片标题行：图标 + 标题 + 计数 + 右侧动作。
pub(super) fn eq_stage_header(
    pal: &EqPalette,
    icon: Icon,
    title: &'static str,
    count: usize,
    action: Option<AnyElement>,
) -> AnyElement {
    div()
        .h(px(22.0))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(icons::icon(icon, 12.0, pal.text_dim))
        .child(
            div()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(pal.text)
                .child(title),
        )
        .child(
            div()
                .text_size(px(8.5))
                .font_family(pal.mono.clone())
                .text_color(pal.text_faint)
                .child(SharedString::from(count.to_string())),
        )
        .child(div().flex_1())
        .when_some(action, |d, action| d.child(action))
        .into_any_element()
}

/// 链路中的内置预设行：开关 / 名称 / 增幅乘数 / 移除。
pub(super) fn eq_chain_block_row(
    pal: &EqPalette,
    preset: &'static BuiltinPreset,
    block: &PresetBlock,
    selected: bool,
    on_select: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_toggle: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_remove: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let power = if block.enabled {
        PowerDot::On
    } else {
        PowerDot::Bypassed
    };
    div()
        .id(SharedString::from(format!("eq-chain-block-{}", preset.id)))
        .h(px(30.0))
        .w_full()
        .flex_none()
        .px(px(4.0))
        .rounded(px(7.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .bg(if selected {
            pal.selected_bg
        } else {
            gpui::transparent_black()
        })
        .hover(|s| s.bg(pal.chip_hover))
        .child(eq_power_dot(
            pal,
            power,
            SharedString::from(format!("eq-chain-block-power-{}", preset.id)),
            move |event, window, cx| {
                cx.stop_propagation();
                on_toggle(event, window, cx);
            },
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(0.0))
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if selected { pal.accent } else { pal.text })
                        .text_ellipsis()
                        .child(preset.name),
                )
                .child(
                    div()
                        .text_size(px(8.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .text_ellipsis()
                        .child(SharedString::from(preset.gain_label())),
                ),
        )
        .child(
            div()
                .w(px(38.0))
                .text_right()
                .text_size(px(9.0))
                .font_family(pal.mono.clone())
                .text_color(if block.enabled {
                    pal.accent
                } else {
                    pal.text_faint
                })
                .child(SharedString::from(crate::eq::fmt_amount(block.amount))),
        )
        .child(eq_mini_icon_button(
            pal,
            Icon::Trash,
            20.0,
            SharedString::from(format!("eq-chain-block-remove-{}", preset.id)),
            pal.text_faint,
            pal.danger_soft,
            on_remove,
        ))
        .on_click(on_select)
        .into_any_element()
}

/// 链路中的自定义滤波器行：启用 / 频率 / 增益 / Q / 移除。
pub(super) fn eq_chain_custom_row(
    pal: &EqPalette,
    index: usize,
    band: &chordal_player::EqBand,
    selected: bool,
    on_select: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_toggle: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_remove: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let gain_color = if !band.enabled {
        pal.text_faint
    } else if band.gain_db.abs() > 0.05 {
        pal.accent
    } else {
        pal.text_dim
    };
    div()
        .id(SharedString::from(format!("eq-chain-custom-{index}")))
        .h(px(28.0))
        .w_full()
        .flex_none()
        .px(px(4.0))
        .rounded(px(7.0))
        .flex()
        .items_center()
        .gap(px(5.0))
        .cursor_pointer()
        .bg(if selected {
            pal.selected_bg
        } else {
            gpui::transparent_black()
        })
        .hover(|s| s.bg(pal.chip_hover))
        .child(eq_power_dot(
            pal,
            if band.enabled {
                PowerDot::On
            } else {
                PowerDot::Bypassed
            },
            SharedString::from(format!("eq-chain-custom-power-{index}")),
            move |event, window, cx| {
                cx.stop_propagation();
                on_toggle(event, window, cx);
            },
        ))
        .child(
            div()
                .w(px(52.0))
                .text_size(px(10.0))
                .font_family(pal.mono.clone())
                .text_color(if selected { pal.accent } else { pal.text })
                .child(SharedString::from(fmt_freq(band.freq_hz))),
        )
        .child(
            div()
                .w(px(42.0))
                .text_size(px(9.5))
                .font_family(pal.mono.clone())
                .text_color(gain_color)
                .child(SharedString::from(if band.kind.uses_gain() {
                    db_num(band.gain_db)
                } else {
                    "—".into()
                })),
        )
        .child(
            div()
                .w(px(34.0))
                .text_size(px(8.0))
                .font_family(pal.mono.clone())
                .text_color(pal.text_faint)
                .child(SharedString::from(format!("Q {:.1}", band.q))),
        )
        .child(div().flex_1())
        .child(eq_mini_icon_button(
            pal,
            Icon::Trash,
            20.0,
            SharedString::from(format!("eq-chain-custom-remove-{index}")),
            pal.text_faint,
            pal.danger_soft,
            on_remove,
        ))
        .on_click(on_select)
        .into_any_element()
}

/// 预设库行：左侧启用圆点、名称/描述、右侧 +/√ 与预览箭头。
///
/// + / √ 是最高优先级的成员状态：只要已加入编辑器就显示 √，与启用与否无关。
#[allow(clippy::too_many_arguments)] // 行控件的回调/状态参数拆结构体会降低调用点可读性
pub(super) fn eq_builtin_row(
    pal: &EqPalette,
    index: usize,
    preset: &'static BuiltinPreset,
    added: bool,
    enabled: bool,
    previewed: bool,
    compact: bool,
    on_preview: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_toggle_add: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_toggle_power: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let power = if !added {
        PowerDot::Off
    } else if enabled {
        PowerDot::On
    } else {
        PowerDot::Bypassed
    };
    let add_button = div()
        .id(SharedString::from(format!("eq-builtin-add-{}", preset.id)))
        .size(px(26.0))
        .flex_none()
        .rounded(px(8.0))
        .border_1()
        .border_color(if added { pal.accent_line } else { pal.border })
        .bg(if added { pal.accent_soft } else { pal.chip })
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |s| {
            s.bg(if added {
                pal.accent_soft.opacity(0.8)
            } else {
                pal.chip_hover
            })
        })
        .child(icons::icon(
            if added { Icon::Check } else { Icon::Plus },
            12.0,
            if added { pal.accent } else { pal.text_dim },
        ))
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            on_toggle_add(event, window, cx);
        });

    div()
        .id(SharedString::from(format!("eq-builtin-{index}")))
        .w_full()
        .h(px(if compact { 62.0 } else { 64.0 }))
        .flex_none()
        .rounded(px(11.0))
        .border_1()
        .border_color(if previewed {
            pal.accent_line
        } else if added {
            pal.accent_line.opacity(0.45)
        } else {
            pal.border
        })
        .bg(if previewed {
            pal.accent_soft.opacity(0.55)
        } else if added {
            pal.selected_bg.opacity(0.6)
        } else {
            pal.panel
        })
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(9.0))
        .cursor_pointer()
        .hover(|s| s.bg(pal.chip_hover))
        .child(eq_power_dot(
            pal,
            power,
            SharedString::from(format!("eq-builtin-power-{}", preset.id)),
            move |event, window, cx| {
                cx.stop_propagation();
                on_toggle_power(event, window, cx);
            },
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(
                            div()
                                .min_w_0()
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(if added { pal.accent } else { pal.text })
                                .text_ellipsis()
                                .child(preset.name),
                        )
                        .child(eq_tag(pal, preset.group.label(), pal.text_faint, pal.chip))
                        .child(eq_tag(pal, preset.gain_label(), pal.text_dim, pal.chip)),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .text_ellipsis()
                        .child(preset.description),
                ),
        )
        .child(add_button)
        .child(icons::icon(Icon::ChevronRight, 12.0, pal.text_faint))
        .on_click(on_preview)
        .into_any_element()
}

/// 内置预设的详细预览卡片：频谱、描述、参数与增幅乘数。
pub(super) fn eq_builtin_preview_panel(
    pal: &EqPalette,
    preset: &'static BuiltinPreset,
    block: Option<&PresetBlock>,
    compact: bool,
    on_toggle_add: Rc<dyn Fn(&mut App)>,
    on_toggle_power: Rc<dyn Fn(&mut App)>,
    on_amount: Rc<dyn Fn(f32, &mut App)>,
) -> AnyElement {
    let amount = block.map(|block| block.amount).unwrap_or(1.0);
    let enabled = block.map(|block| block.enabled).unwrap_or(false);
    let mut settings = preset.to_settings(amount);
    // 预览只看预设本身的形状；前级补偿在统计里单独展示。
    settings.input_gain_db = 0.0;
    let pal_paint = pal.clone();
    let paint_settings = settings.clone();
    let graph_h = if compact { 132.0 } else { 150.0 };
    let graph = div()
        .w_full()
        .h(px(graph_h))
        .flex_none()
        .rounded(px(10.0))
        .bg(pal.inset)
        .border_1()
        .border_color(pal.border)
        .overflow_hidden()
        .child(
            canvas(
                |bounds, _, _| bounds,
                move |_bounds, bounds, window, _| {
                    paint_eq_graph(
                        bounds,
                        &paint_settings,
                        &[],
                        None,
                        true,
                        0,
                        &pal_paint,
                        window,
                    );
                },
            )
            .size_full(),
        );

    let mut detail = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(pal.text)
                        .child(preset.name),
                )
                .child(eq_tag(pal, preset.group.label(), pal.text_dim, pal.chip))
                .when(block.is_some(), |d| {
                    d.child(eq_tag(
                        pal,
                        if enabled { "已启用" } else { "已旁路" },
                        if enabled { pal.accent } else { pal.text_faint },
                        if enabled { pal.accent_soft } else { pal.chip },
                    ))
                }),
        )
        .child(
            div()
                .text_size(px(9.5))
                .line_height(px(15.0))
                .text_color(pal.text_dim)
                .child(preset.description),
        )
        .child(
            div()
                .w_full()
                .flex()
                .gap(px(8.0))
                .child(eq_stat(
                    pal,
                    "中心频率",
                    preset
                        .bands
                        .first()
                        .map(|band| fmt_freq(band.0))
                        .unwrap_or_else(|| "—".into()),
                ))
                .child(eq_stat(
                    pal,
                    "基准增益",
                    preset
                        .bands
                        .first()
                        .map(|band| format!("{} dB", db_num(band.1)))
                        .unwrap_or_else(|| "—".into()),
                ))
                .child(eq_stat(
                    pal,
                    "前级补偿",
                    format!("{} dB", db_num(preset.input_gain_db)),
                )),
        );

    if let Some(block) = block {
        let on_amount = on_amount.clone();
        let amount_slider = widgets::eq_knob_slider(
            (block.amount / crate::eq::PRESET_AMOUNT_MAX).clamp(0.0, 1.0),
            pal.slider_colors(),
            "eq-builtin-amount",
            move |value: f32, cx: &mut App| {
                on_amount(value * crate::eq::PRESET_AMOUNT_MAX, cx);
            },
        );
        detail = detail.child(eq_param_block(
            pal,
            "增幅乘数（预设分贝 × 乘数）",
            format!(
                "{} · 实际 {} dB",
                crate::eq::fmt_amount(block.amount),
                db_num(preset.base_gain_db() * block.amount)
            ),
            amount_slider,
        ));
    }

    let membership = {
        let on_toggle_add = on_toggle_add.clone();
        let added = block.is_some();
        div()
            .id("eq-builtin-membership")
            .h(px(34.0))
            .w_full()
            .rounded(px(9.0))
            .border_1()
            .border_color(if added {
                pal.accent_line
            } else {
                gpui::transparent_black()
            })
            .bg(if added { pal.accent_soft } else { pal.accent })
            .flex()
            .items_center()
            .justify_center()
            .gap(px(5.0))
            .cursor_pointer()
            .text_size(px(11.0))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(if added { pal.accent } else { pal.on_accent })
            .hover(move |s| {
                s.bg(if added {
                    pal.accent_soft.opacity(0.8)
                } else {
                    pal.accent_hover
                })
            })
            .child(icons::icon(
                if added { Icon::Check } else { Icon::Plus },
                12.0,
                if added { pal.accent } else { pal.on_accent },
            ))
            .child(if added {
                "√ 已加入编辑器"
            } else {
                "＋ 加入编辑器"
            })
            .on_click(move |_, _, cx| on_toggle_add(cx))
            .into_any_element()
    };

    let power = if block.is_some() {
        let on_toggle_power = on_toggle_power.clone();
        div()
            .id("eq-builtin-power-toggle")
            .h(px(28.0))
            .w_full()
            .rounded(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(5.0))
            .cursor_pointer()
            .bg(if enabled { pal.chip } else { pal.accent_soft })
            .text_size(px(9.5))
            .text_color(if enabled { pal.text_dim } else { pal.accent })
            .hover(|s| s.bg(pal.chip_hover))
            .child(if enabled {
                "旁路这枚预设"
            } else {
                "重新启用这枚预设"
            })
            .on_click(move |_, _, cx| on_toggle_power(cx))
            .into_any_element()
    } else {
        div().into_any_element()
    };

    let list = preset
        .bands
        .first()
        .map(|&(freq, gain, q)| {
            div()
                .w_full()
                .h(px(20.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .border_t_1()
                .border_color(pal.divider)
                .child(eq_tag(pal, "峰值", pal.text_faint, pal.chip))
                .child(
                    div()
                        .w(px(48.0))
                        .text_size(px(9.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_dim)
                        .child(SharedString::from(fmt_freq(freq))),
                )
                .child(
                    div()
                        .w(px(50.0))
                        .text_size(px(9.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.accent)
                        .child(SharedString::from(format!("{} dB", db_num(gain * amount)))),
                )
                .child(
                    div()
                        .text_size(px(8.5))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .child(SharedString::from(format!("Q {q:.2}"))),
                )
                .into_any_element()
        })
        .unwrap_or_else(|| div().into_any_element());

    div()
        .w_full()
        .rounded(px(14.0))
        .border_1()
        .border_color(pal.border)
        .bg(pal.panel)
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(9.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("预设频谱"),
                )
                .child(
                    div()
                        .h(px(16.0))
                        .px(px(6.0))
                        .rounded(px(8.0))
                        .bg(pal.chip)
                        .flex()
                        .items_center()
                        .text_size(px(8.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .child(SharedString::from(if block.is_some() {
                            crate::eq::fmt_amount(amount)
                        } else {
                            "1×".into()
                        })),
                ),
        )
        .child(graph)
        .child(detail)
        .child(eq_divider(pal))
        .child(list)
        .child(div().flex_1())
        .child(power)
        .child(membership)
        .into_any_element()
}
