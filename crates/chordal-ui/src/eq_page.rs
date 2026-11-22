//! 均衡器页面：桌面 Studio 控制台 / 紧凑触控两种布局。
//!
//! 桌面端把「浏览预设 → 预览曲线 → 精确调参」放在同一屏：左侧工作区导航，
//! 中间是频响曲线与频段推子，右侧是滤波器检视器。紧凑布局（竖屏 / 窄窗）
//! 则把曲线放大成主画布，频段做成横向芯片，参数收进底部触控面板。
//!
//! 两套布局共用同一份 [`EqState`] 与绘制器，曲线和播放线程使用同一组
//! biquad 系数，因此视觉上没有「预览曲线」与「实际声音」的偏差。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use chordal_player::{
    EQ_MAX_BANDS, EQ_MAX_GAIN_DB, EQ_MIN_GAIN_DB, EqSettings, FilterKind, fmt_db, fmt_freq,
};
use gpui::StatefulInteractiveElement;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, BoxShadow, Component, Context, Hsla, IntoElement, MouseButton,
    MouseMoveEvent, PathBuilder, RenderOnce, SharedString, Window, canvas, div, fill,
    linear_color_stop, linear_gradient, point, px, size,
};

use crate::app::ChordialApp;
use crate::autoeq::{AutoEqLibrary, AutoEqStatus};
use crate::eq::{
    EqTab, GraphGeom, PresetEntry, band_summary, builtin_presets, freq_to_slider, q_to_slider,
    slider_to_freq, slider_to_q, v4_presets,
};
use crate::icons::{self, Icon};
use crate::theme::Theme;
use crate::widgets::{self, SliderColors};

const RAIL_W: f32 = 196.0;
const INSPECTOR_W: f32 = 286.0;
const COMPACT_MAX_W: f32 = 560.0;
/// 紧凑布局一次最多铺多少行 AutoEQ；完整列表有 6000+ 条，
/// 依赖搜索缩小范围，避免把整份索引塞进一次布局。
const COMPACT_AUTOEQ_ROWS: usize = 80;

/// 诊断开关：`CHORDIAL_EQ_PROBE=rows` / `preview` 把对应区域替换为空块，
/// 用于把 EQ 帧成本拆分到具体区块（仅压测，不影响正常渲染）。
fn eq_probe(flag: &str) -> bool {
    use std::sync::OnceLock;
    static PROBE: OnceLock<String> = OnceLock::new();
    PROBE
        .get_or_init(|| std::env::var("CHORDIAL_EQ_PROBE").unwrap_or_default())
        .contains(flag)
}

/// 频响曲线采样缓存：`response_db` 每个采样点都要重算全部 biquad 系数
/// （三角函数/幂运算），260 点 × 频段数在 300Hz 下是主要 CPU 成本。
/// 曲线只随设置与画布几何变化，缓存后滚动/悬停帧直接复用像素坐标。
struct CurveCache {
    key: u64,
    width_bits: u32,
    height_bits: u32,
    left_bits: u32,
    top_bits: u32,
    compact: bool,
    steps: usize,
    points: Rc<Vec<(f32, f32)>>,
}

thread_local! {
    static CURVE_CACHE: std::cell::RefCell<Option<CurveCache>> =
        const { std::cell::RefCell::new(None) };
}

fn eq_curve_key(settings: &EqSettings) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    settings.enabled.hash(&mut hasher);
    settings.input_gain_db.to_bits().hash(&mut hasher);
    settings.output_gain_db.to_bits().hash(&mut hasher);
    settings.bands.len().hash(&mut hasher);
    for band in &settings.bands {
        (band.kind as u8).hash(&mut hasher);
        band.enabled.hash(&mut hasher);
        band.freq_hz.to_bits().hash(&mut hasher);
        band.gain_db.to_bits().hash(&mut hasher);
        band.q.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

fn eq_curve_points(
    settings: &EqSettings,
    geom: &GraphGeom,
    ox: f32,
    oy: f32,
    width: f32,
    height: f32,
    compact: bool,
) -> Rc<Vec<(f32, f32)>> {
    let key = eq_curve_key(settings);
    let width_bits = width.to_bits();
    let height_bits = height.to_bits();
    let left_bits = ox.to_bits();
    let top_bits = oy.to_bits();
    let steps = if compact { 150 } else { 260 };
    CURVE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(cached) = cache.as_ref()
            && cached.key == key
            && cached.width_bits == width_bits
            && cached.height_bits == height_bits
            && cached.left_bits == left_bits
            && cached.top_bits == top_bits
            && cached.compact == compact
            && cached.steps == steps
        {
            return cached.points.clone();
        }
        let mut points: Vec<(f32, f32)> = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let norm = i as f32 / steps as f32;
            let freq = chordal_player::norm_to_freq(norm);
            let db = settings.response_db(freq, chordal_player::EQ_REFERENCE_RATE);
            points.push((ox + geom.x(freq), oy + geom.y(db)));
        }
        let points = Rc::new(points);
        *cache = Some(CurveCache {
            key,
            width_bits,
            height_bits,
            left_bits,
            top_bits,
            compact,
            steps,
            points: points.clone(),
        });
        points
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EqLayout {
    Desktop,
    Compact,
}

fn eq_layout(width: f32, height: f32, force_portrait: bool) -> EqLayout {
    if force_portrait || width < 820.0 || height > width {
        EqLayout::Compact
    } else {
        EqLayout::Desktop
    }
}

/// 均衡器专用色板：跟随应用明暗，但始终保持「设备面板」的层次。
#[derive(Clone)]
struct EqPalette {
    scrim: Hsla,
    bg_top: Hsla,
    bg_bottom: Hsla,
    panel: Hsla,
    panel_alt: Hsla,
    inset: Hsla,
    border: Hsla,
    border_strong: Hsla,
    divider: Hsla,
    text: Hsla,
    text_dim: Hsla,
    text_faint: Hsla,
    accent: Hsla,
    accent_hover: Hsla,
    accent_soft: Hsla,
    accent_line: Hsla,
    on_accent: Hsla,
    danger: Hsla,
    danger_soft: Hsla,
    ok: Hsla,
    grid: Hsla,
    grid_major: Hsla,
    chip: Hsla,
    chip_hover: Hsla,
    selected_bg: Hsla,
    fader_track: Hsla,
    thumb: Hsla,
    shadow: Hsla,
    font: SharedString,
    mono: SharedString,
}

impl EqPalette {
    fn new(theme: &Theme) -> Self {
        let dark = theme.is_dark();
        if dark {
            Self {
                scrim: crate::theme::alpha(0x050508C4),
                bg_top: crate::theme::hex(0x111218),
                bg_bottom: crate::theme::hex(0x08090C),
                panel: crate::theme::hex(0x131419),
                panel_alt: crate::theme::hex(0x0D0E12),
                inset: crate::theme::hex(0x0A0B0E),
                border: crate::theme::alpha(0xFFFFFF14),
                border_strong: crate::theme::alpha(0xFFFFFF26),
                divider: crate::theme::alpha(0xFFFFFF0D),
                text: crate::theme::hex(0xF4F4F7),
                text_dim: crate::theme::hex(0xA5A6B0),
                text_faint: crate::theme::hex(0x6C6D77),
                accent: crate::theme::hex(0x4D9CFF),
                accent_hover: crate::theme::hex(0x6DAEFF),
                accent_soft: crate::theme::alpha(0x4D9CFF24),
                accent_line: crate::theme::alpha(0x4D9CFF70),
                on_accent: crate::theme::hex(0x061020),
                danger: crate::theme::hex(0xFF6F6A),
                danger_soft: crate::theme::alpha(0xFF6F6A1E),
                ok: crate::theme::hex(0x37D67A),
                grid: crate::theme::alpha(0xFFFFFF0A),
                grid_major: crate::theme::alpha(0xFFFFFF1B),
                chip: crate::theme::alpha(0xFFFFFF0D),
                chip_hover: crate::theme::alpha(0xFFFFFF17),
                selected_bg: crate::theme::alpha(0x4D9CFF18),
                fader_track: crate::theme::alpha(0xFFFFFF20),
                thumb: crate::theme::hex(0xFFFFFF),
                shadow: crate::theme::alpha(0x000000B8),
                font: theme.font_family.clone(),
                mono: theme.font_mono.clone(),
            }
        } else {
            Self {
                scrim: crate::theme::alpha(0x0A0A1080),
                bg_top: crate::theme::hex(0xEFF0F4),
                bg_bottom: crate::theme::hex(0xE4E5EA),
                panel: crate::theme::hex(0xFFFFFF),
                panel_alt: crate::theme::hex(0xF4F4F7),
                inset: crate::theme::hex(0xFAFAFC),
                border: crate::theme::alpha(0x0A0A101A),
                border_strong: crate::theme::alpha(0x0A0A102E),
                divider: crate::theme::alpha(0x0A0A100F),
                text: crate::theme::hex(0x1C1C1F),
                text_dim: crate::theme::hex(0x595962),
                text_faint: crate::theme::hex(0x8C8C94),
                accent: crate::theme::hex(0x0B6BFF),
                accent_hover: crate::theme::hex(0x0057DB),
                accent_soft: crate::theme::alpha(0x0B6BFF16),
                accent_line: crate::theme::alpha(0x0B6BFF5C),
                on_accent: crate::theme::hex(0xFFFFFF),
                danger: crate::theme::hex(0xC0443A),
                danger_soft: crate::theme::alpha(0xC0443A16),
                ok: crate::theme::hex(0x248A3D),
                grid: crate::theme::alpha(0x0A0A100D),
                grid_major: crate::theme::alpha(0x0A0A101F),
                chip: crate::theme::alpha(0x0A0A100A),
                chip_hover: crate::theme::alpha(0x0A0A1016),
                selected_bg: crate::theme::alpha(0x0B6BFF0F),
                fader_track: crate::theme::alpha(0x0A0A1029),
                thumb: crate::theme::hex(0x1C1C1F),
                shadow: crate::theme::alpha(0x0B0B1240),
                font: theme.font_family.clone(),
                mono: theme.font_mono.clone(),
            }
        }
    }

    fn slider_colors(&self) -> SliderColors {
        SliderColors {
            track: self.fader_track,
            fill: self.accent,
            thumb: self.thumb,
        }
    }
}

fn db_num(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0".into()
    } else {
        format!("{db:+.1}")
    }
}

fn tab_meta(tab: EqTab) -> (&'static str, &'static str) {
    match tab {
        EqTab::Edit => ("编辑器", "点按曲线添加，拖动节点调整"),
        EqTab::V4 => ("V4 预设", "Moriafly / Syaran 风格配置"),
        EqTab::AutoEq => ("AutoEQ 耳机档案", "jaakkopasanen/AutoEq · 按测量曲线校准"),
        EqTab::Builtin => ("内置预设", "按听感调校的房间与风格"),
        EqTab::Personal => ("我的配置", "「另存为」得到的个人预设"),
    }
}

fn tab_icon(tab: EqTab) -> Option<Icon> {
    match tab {
        EqTab::Edit => Some(Icon::Sliders),
        EqTab::V4 => Some(Icon::Equalizer),
        EqTab::AutoEq => Some(Icon::Headphones),
        EqTab::Builtin => Some(Icon::Layers),
        EqTab::Personal => Some(Icon::Person),
    }
}

// ================= 绘制 =================

fn eq_rect(window: &mut Window, x: f32, y: f32, w: f32, h: f32, color: Hsla) {
    window.paint_quad(fill(
        Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
        color,
    ));
}

fn eq_circle(window: &mut Window, cx: f32, cy: f32, r: f32, color: Hsla) {
    // 圆点用圆角 quad 表示：与路径相比不会触发全屏 MSAA 路径通道。
    window.paint_quad(
        fill(
            Bounds::new(point(px(cx - r), px(cy - r)), size(px(r * 2.0), px(r * 2.0))),
            color,
        )
        .corner_radii(px(r)),
    );
}

fn eq_polyline(window: &mut Window, points: &[(f32, f32)], thickness: f32, color: Hsla) {
    if points.len() < 2 {
        return;
    }
    let mut path = PathBuilder::stroke(px(thickness.max(0.5)));
    path.move_to(point(px(points[0].0), px(points[0].1)));
    for &(x, y) in &points[1..] {
        path.line_to(point(px(x), px(y)));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// 频响网格、曲线与频段控制点。
fn paint_eq_graph(
    bounds: Bounds<gpui::Pixels>,
    settings: &EqSettings,
    selected: Option<usize>,
    compact: bool,
    pal: &EqPalette,
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    if width <= 2.0 || height <= 2.0 {
        return;
    }
    let ox = f32::from(bounds.left());
    let oy = f32::from(bounds.top());
    let geom = if compact {
        GraphGeom::compact(width, height)
    } else {
        GraphGeom::new(width, height)
    };

    for step in 0..=8 {
        let db = 24.0 - step as f32 * 6.0;
        let y = oy + geom.y(db);
        eq_rect(
            window,
            ox + geom.left,
            y,
            geom.width,
            1.0,
            if step == 4 { pal.grid_major } else { pal.grid },
        );
    }
    for freq in [
        31.5f32, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
    ] {
        let x = ox + geom.x(freq);
        eq_rect(
            window,
            x,
            oy + 10.0,
            1.0,
            (f32::from(bounds.size.height) - 30.0).max(1.0),
            if (freq - 1_000.0).abs() < 1.0 {
                pal.grid_major
            } else {
                pal.grid
            },
        );
    }

    let signal = if settings.enabled {
        pal.accent
    } else {
        pal.text_faint
    };
    let global = settings.input_gain_db + settings.output_gain_db;
    let points = eq_curve_points(settings, &geom, ox, oy, width, height, compact);
    // 一条折线就是整条曲线：之前每个采样点建 4 边形 + 一次 paint_path，
    // 260 点 × 3 层 = 780 条路径，每帧都会触发多次全屏 MSAA 路径通道。
    eq_polyline(window, points.as_slice(), 1.8, signal);
    // 曲线辉光：先宽后窄，让蓝色在深色面板上有「发光」的层次而不是硬描边。
    eq_polyline(window, points.as_slice(), 6.0, signal.opacity(0.07));
    eq_polyline(window, points.as_slice(), 3.0, signal.opacity(0.16));

    let handle = if compact { 17.0 } else { 14.0 };
    for (index, band) in settings.bands.iter().enumerate() {
        let x = ox + geom.x(band.freq_hz);
        let y = oy + geom.y(band.gain_db + global);
        let color = if band.enabled { signal } else { pal.text_faint };
        // 控制点到 0 dB 的细线，帮助读取提升/衰减幅度。
        if band.enabled {
            let base_y = oy + geom.y(global);
            eq_rect(
                window,
                x - 0.5,
                y.min(base_y),
                1.0,
                (base_y - y).abs().max(1.0),
                pal.grid,
            );
        }
        if selected == Some(index) {
            eq_circle(window, x, y, handle * 0.5 + 5.0, pal.text.opacity(0.16));
            eq_circle(window, x, y, handle * 0.5 + 2.0, pal.text.opacity(0.85));
        }
        eq_circle(window, x, y, handle * 0.5, pal.inset);
        eq_circle(window, x, y, handle * 0.5 - 4.0, color);
    }
    let _ = width;
}

// ================= 滑杆 =================

struct SliderState {
    bounds: Cell<Bounds<gpui::Pixels>>,
    dragging: Cell<bool>,
    view: Cell<Option<gpui::EntityId>>,
}

fn notify_slider_view(state: &SliderState, cx: &mut App) {
    if let Some(view) = state.view.get() {
        cx.notify(view);
    }
}

fn eq_fraction_at(
    bounds: Bounds<gpui::Pixels>,
    position: gpui::Point<gpui::Pixels>,
) -> Option<f32> {
    let w = f32::from(bounds.size.width);
    if w <= 0.0 {
        return None;
    }
    Some((f32::from(position.x - bounds.left()) / w).clamp(0.0, 1.0))
}

/// 双极性滑杆（增益）：填充从 0 点向两侧展开，避免负增益看起来像「剩余量」。
struct BipolarSlider {
    value: f32,
    colors: SliderColors,
    tick: Hsla,
    height: f32,
    thumb_r: f32,
    id: &'static str,
    on_change: Rc<std::cell::RefCell<Box<dyn FnMut(f32, &mut App)>>>,
}

fn eq_bipolar_slider(
    value: f32,
    colors: SliderColors,
    tick: Hsla,
    height: f32,
    thumb_r: f32,
    id: &'static str,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    Component::new(BipolarSlider {
        value: value.clamp(0.0, 1.0),
        colors,
        tick,
        height,
        thumb_r,
        id,
        on_change: Rc::new(std::cell::RefCell::new(Box::new(on_change))),
    })
    .into_any_element()
}

impl RenderOnce for BipolarSlider {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let state = Rc::new(SliderState {
            bounds: Cell::new(Bounds::new(point(px(0.0), px(0.0)), size(px(0.0), px(0.0)))),
            dragging: Cell::new(false),
            view: Cell::new(None),
        });
        let on_change = self.on_change;
        let value = self.value;
        let colors = self.colors;
        let tick = self.tick;
        let height = self.height;
        let thumb_r = self.thumb_r;
        let id = self.id;

        let state_down = state.clone();
        let change_down = on_change.clone();
        let state_move = state.clone();
        let change_move = on_change.clone();
        let state_up = state.clone();
        let state_paint_bounds = state.clone();
        let state_paint = state.clone();

        div()
            .id(SharedString::from(format!("eq-bipolar-{id}")))
            .h(px(height))
            .w_full()
            .relative()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                state_down.dragging.set(true);
                if let Some(f) = eq_fraction_at(state_down.bounds.get(), event.position) {
                    (change_down.borrow_mut())(f, cx);
                    notify_slider_view(&state_down, cx);
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if state_move.dragging.get()
                    && event.pressed_button == Some(MouseButton::Left)
                    && let Some(f) = eq_fraction_at(state_move.bounds.get(), event.position)
                {
                    (change_move.borrow_mut())(f, cx);
                    notify_slider_view(&state_move, cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                state_up.dragging.set(false);
            })
            .child(
                canvas(
                    move |bounds, _, _| {
                        state_paint_bounds.bounds.set(bounds);
                        bounds
                    },
                    move |_bounds, bounds, window, _| {
                        paint_bipolar(
                            bounds,
                            value,
                            colors,
                            tick,
                            state_paint.dragging.get(),
                            thumb_r,
                            window,
                        );
                    },
                )
                .size_full(),
            )
    }
}

fn paint_bipolar(
    bounds: Bounds<gpui::Pixels>,
    value: f32,
    colors: SliderColors,
    tick: Hsla,
    active: bool,
    thumb_r: f32,
    window: &mut Window,
) {
    let cy = bounds.center().y;
    let thickness = if active { 6.0 } else { 4.0 };
    let track = Bounds::new(
        point(bounds.left(), cy - px(thickness / 2.0)),
        size(bounds.size.width, px(thickness)),
    );
    window.paint_quad(fill(track, colors.track).corner_radii(px(thickness / 2.0)));

    let width = f32::from(bounds.size.width);
    let center_x = f32::from(bounds.left()) + width / 2.0;
    eq_rect(
        window,
        center_x - 0.5,
        f32::from(cy) - 6.0,
        1.0,
        12.0,
        tick.opacity(0.55),
    );

    let value_x = f32::from(bounds.left()) + width * value.clamp(0.0, 1.0);
    let (x0, x1) = if value_x >= center_x {
        (center_x, value_x)
    } else {
        (value_x, center_x)
    };
    if x1 - x0 > 0.5 {
        let fill_rect = Bounds::new(
            point(px(x0), cy - px(thickness / 2.0)),
            size(px(x1 - x0), px(thickness)),
        );
        window.paint_quad(fill(fill_rect, colors.fill).corner_radii(px(thickness / 2.0)));
    }
    eq_circle(window, value_x, f32::from(cy), thumb_r + 1.5, colors.track);
    eq_circle(window, value_x, f32::from(cy), thumb_r, colors.thumb);
}

// ================= 频段推子 =================

struct FaderState {
    bounds: Cell<Bounds<gpui::Pixels>>,
    dragging: Cell<bool>,
    view: Cell<Option<gpui::EntityId>>,
}

fn notify_fader_view(state: &FaderState, cx: &mut App) {
    if let Some(view) = state.view.get() {
        cx.notify(view);
    }
}

/// 桌面端竖向频段推子：中点 0 dB，向上提升、向下衰减。
fn eq_band_fader(
    index: usize,
    freq_hz: f32,
    gain_db: f32,
    enabled: bool,
    uses_gain: bool,
    kind_label: &'static str,
    selected: bool,
    pal: &EqPalette,
    on_select: impl Fn(&mut App) + 'static,
    on_change: impl FnMut(f32, &mut App) + 'static,
) -> AnyElement {
    let state = Rc::new(FaderState {
        bounds: Cell::new(Bounds::new(point(px(0.0), px(0.0)), size(px(0.0), px(0.0)))),
        dragging: Cell::new(false),
        view: Cell::new(None),
    });
    let change = Rc::new(std::cell::RefCell::new(
        Box::new(on_change) as Box<dyn FnMut(f32, &mut App)>
    ));
    let select = Rc::new(on_select);
    let shown = Rc::new(Cell::new(gain_db));
    let pal_paint = pal.clone();

    let down_state = state.clone();
    let down_select = select.clone();
    let down_change = change.clone();
    let down_shown = shown.clone();
    let move_state = state.clone();
    let move_change = change.clone();
    let move_shown = shown.clone();
    let up_state = state.clone();
    let paint_state = state.clone();
    let paint_state2 = state.clone();
    let paint_shown = shown.clone();

    let cursor = if uses_gain {
        gpui::CursorStyle::ResizeUpDown
    } else {
        gpui::CursorStyle::PointingHand
    };

    div()
        .id(SharedString::from(format!("eq-fader-{index}")))
        .w(px(50.0))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(3.0))
        .rounded(px(9.0))
        .cursor(cursor)
        .hover(|s| s.bg(pal.chip))
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            (down_select)(cx);
            if uses_gain {
                if let Some(db) = fader_db_at(down_state.bounds.get(), event.position) {
                    down_state.dragging.set(true);
                    down_shown.set(db);
                    (down_change.borrow_mut())(db, cx);
                    notify_fader_view(&down_state, cx);
                }
            }
        })
        .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
            if !move_state.dragging.get()
                || !uses_gain
                || event.pressed_button != Some(MouseButton::Left)
            {
                return;
            }
            if let Some(db) = fader_db_at(move_state.bounds.get(), event.position) {
                move_shown.set(db);
                (move_change.borrow_mut())(db, cx);
                notify_fader_view(&move_state, cx);
            }
        })
        .on_mouse_up(MouseButton::Left, move |_, _, _| {
            up_state.dragging.set(false);
        })
        .child(
            div()
                .pt(px(5.0))
                .text_size(px(9.0))
                .font_family(pal.mono.clone())
                .text_color(if selected { pal.accent } else { pal.text_dim })
                .child(SharedString::from(fmt_freq(freq_hz))),
        )
        .child(
            div().w(px(38.0)).flex_1().min_h_0().child(
                canvas(
                    move |bounds, window, _| {
                        paint_state.bounds.set(bounds);
                        paint_state.view.set(Some(window.current_view()));
                        bounds
                    },
                    move |_bounds, bounds, window, _| {
                        paint_fader(
                            bounds,
                            paint_shown.get(),
                            selected,
                            enabled,
                            uses_gain,
                            &pal_paint,
                            paint_state2.dragging.get(),
                            window,
                        );
                    },
                )
                .size_full(),
            ),
        )
        .child(
            div()
                .pb(px(5.0))
                .text_size(px(9.5))
                .font_family(pal.mono.clone())
                .text_color(if !uses_gain || !enabled {
                    pal.text_faint
                } else if selected {
                    pal.accent
                } else {
                    pal.text_dim
                })
                .child(SharedString::from(if uses_gain {
                    db_num(shown.get())
                } else {
                    kind_label.to_string()
                })),
        )
        .into_any_element()
}

fn fader_db_at(bounds: Bounds<gpui::Pixels>, position: gpui::Point<gpui::Pixels>) -> Option<f32> {
    let h = f32::from(bounds.size.height);
    if h <= 12.0 {
        return None;
    }
    let center = f32::from(bounds.top()) + h / 2.0;
    let half = (h / 2.0 - 8.0).max(1.0);
    let db = (center - f32::from(position.y)) / half * 24.0;
    Some(db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB))
}

fn paint_fader(
    bounds: Bounds<gpui::Pixels>,
    gain_db: f32,
    selected: bool,
    enabled: bool,
    uses_gain: bool,
    pal: &EqPalette,
    dragging: bool,
    window: &mut Window,
) {
    let h = f32::from(bounds.size.height);
    if h <= 12.0 {
        return;
    }
    let x = f32::from(bounds.center().x);
    let top = f32::from(bounds.top()) + 6.0;
    let bottom = f32::from(bounds.bottom()) - 6.0;
    let center = (top + bottom) / 2.0;
    let alpha = if enabled { 1.0 } else { 0.38 };
    let track_color = pal
        .fader_track
        .opacity(if selected { 1.0 } else { 0.75 } * alpha);
    eq_rect(window, x - 1.0, top, 2.0, bottom - top, track_color);
    eq_rect(
        window,
        x - 10.0,
        center - 0.5,
        20.0,
        1.0,
        pal.grid_major.opacity(alpha),
    );

    let db = gain_db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB);
    let half = (h / 2.0 - 8.0).max(1.0);
    let y = center - db / 24.0 * half;
    if uses_gain {
        let (y0, y1) = if y <= center {
            (y, center)
        } else {
            (center, y)
        };
        if y1 - y0 > 0.5 {
            eq_rect(
                window,
                x - 1.5,
                y0,
                3.0,
                y1 - y0,
                if enabled {
                    pal.accent.opacity(0.85)
                } else {
                    pal.text_faint.opacity(0.4)
                },
            );
        }
        if selected || dragging {
            eq_circle(window, x, y, 8.0, pal.accent.opacity(0.16 * alpha));
        }
        eq_circle(
            window,
            x,
            y,
            5.0,
            if enabled { pal.accent } else { pal.text_faint },
        );
        if selected {
            eq_circle(window, x, y, 2.0, pal.inset);
        }
    } else {
        eq_circle(
            window,
            x,
            center,
            7.0,
            pal.fader_track.opacity(0.55 * alpha),
        );
        eq_circle(window, x, center, 3.0, pal.text_faint.opacity(alpha));
    }
}

// ================= 基础控件 =================

fn eq_icon_button(
    pal: &EqPalette,
    icon: Icon,
    size: f32,
    id: impl Into<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id.into())
        .size(px(size))
        .rounded(px(size * 0.28))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.bg(pal.chip_hover))
        .active(|s| s.bg(pal.chip))
        .child(icons::icon(icon, size * 0.46, pal.text_dim))
        .on_click(on_click)
        .into_any_element()
}

fn eq_section_label(pal: &EqPalette, text: &'static str) -> AnyElement {
    div()
        .px(px(8.0))
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(pal.text_faint)
        .child(text)
        .into_any_element()
}

fn eq_divider(pal: &EqPalette) -> AnyElement {
    div()
        .w_full()
        .h(px(1.0))
        .flex_none()
        .bg(pal.divider)
        .into_any_element()
}

fn eq_chip(
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

fn eq_search_box(pal: &EqPalette, view: gpui::Entity<crate::input::InputView>) -> AnyElement {
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

fn eq_param_block(
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

fn eq_stat(pal: &EqPalette, label: &'static str, value: String) -> AnyElement {
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

// ================= 页面 =================

impl ChordialApp {
    pub(crate) fn render_eq(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let pal = EqPalette::new(&theme);
        let enter = self
            .eq_open_at
            .map(|t| (t.elapsed().as_secs_f32() / 0.24).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let enter: f32 = 1.0 - (1.0 - enter).powi(3);
        match eq_layout(
            self.viewport_width,
            self.viewport_height,
            self.force_portrait,
        ) {
            EqLayout::Desktop => self.eq_render_desktop(&pal, enter, cx),
            EqLayout::Compact => self.eq_render_compact(&pal, enter, cx),
        }
    }

    // ---------- 桌面：Studio 控制台 ----------

    fn eq_render_desktop(&self, pal: &EqPalette, enter: f32, cx: &mut Context<Self>) -> AnyElement {
        let panel_w = (self.viewport_width - 56.0).clamp(700.0, 1180.0);
        let panel_h = (self.viewport_height - 56.0).clamp(520.0, 820.0);
        let main_h = panel_h - 60.0 - 30.0;
        let weak = cx.entity().downgrade();

        let backdrop = {
            let weak = weak.clone();
            div()
                .id("eq-backdrop")
                .absolute()
                .inset_0()
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_eq(cx));
                })
                .into_any_element()
        };

        let content: AnyElement = match self.eq.tab {
            EqTab::Edit => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .child(self.eq_editor_desktop(pal, panel_w, panel_h, cx))
                .child(self.eq_inspector_desktop(pal, main_h, cx))
                .into_any_element(),
            EqTab::V4 => self
                .eq_library_desktop(pal, v4_presets(), main_h, cx)
                .into_any_element(),
            EqTab::AutoEq => self.eq_autoeq_desktop(pal, main_h, cx).into_any_element(),
            EqTab::Builtin => self
                .eq_library_desktop(pal, builtin_presets(), main_h, cx)
                .into_any_element(),
            EqTab::Personal => self.eq_saved_desktop(pal, main_h, cx).into_any_element(),
        };
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.eq_desktop_rail(pal, cx))
            .child(content)
            .into_any_element();

        let panel = div()
            .id("eq-panel")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, _| {})
            .relative()
            .w(px(panel_w))
            .h(px(panel_h))
            .mt(px((1.0 - enter) * 12.0))
            .rounded(px(18.0))
            .font_family(pal.font.clone())
            .border_1()
            .border_color(pal.border_strong)
            .bg(linear_gradient(
                180.0,
                linear_color_stop(pal.bg_top, 0.0),
                linear_color_stop(pal.bg_bottom, 1.0),
            ))
            .shadow(vec![BoxShadow {
                color: pal.shadow,
                offset: point(px(0.0), px(26.0)),
                blur_radius: px(70.0),
                spread_radius: px(0.0),
            }])
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(self.eq_header_desktop(pal, panel_w, cx))
            .child(body)
            .child(self.eq_statusbar_desktop(pal));

        div()
            .id("eq-page")
            .key_context("ChordialEq")
            .track_focus(&self.focus_handle)
            .on_action(
                cx.listener(|app, _: &crate::actions::ExitImmersive, window, cx| {
                    app.close_eq(cx);
                    cx.stop_propagation();
                    window.focus(&app.focus_handle);
                }),
            )
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .bg(pal.scrim)
            .opacity(enter)
            .child(backdrop)
            .child(panel)
            .into_any_element()
    }

    fn eq_header_desktop(
        &self,
        pal: &EqPalette,
        panel_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let title = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .size(px(30.0))
                    .rounded(px(9.0))
                    .bg(pal.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icons::icon(Icon::Equalizer, 16.0, pal.accent)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child("均衡器"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .child(
                                div()
                                    .max_w(px(220.0))
                                    .text_size(px(10.5))
                                    .text_color(if self.eq.dirty {
                                        pal.accent
                                    } else {
                                        pal.text_dim
                                    })
                                    .text_ellipsis()
                                    .child(SharedString::from(self.eq.display_name())),
                            )
                            .child(div().text_size(px(10.5)).text_color(pal.text_faint).child(
                                SharedString::from(format!(
                                    "· {} 个频段",
                                    self.eq.settings.bands.len()
                                )),
                            )),
                    ),
            );

        let save = if self.eq.dirty {
            let weak = weak.clone();
            div()
                .id("eq-save-as")
                .h(px(30.0))
                .px(px(11.0))
                .rounded(px(8.0))
                .bg(pal.accent_soft)
                .border_1()
                .border_color(pal.accent_line)
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(11.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(pal.accent)
                .hover(|s| s.bg(pal.accent_soft.opacity(0.9)))
                .child("另存为")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.save_eq_preset(cx));
                })
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let gains = if panel_w >= 900.0 {
            div()
                .h(px(28.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .bg(pal.chip)
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(8.5))
                        .text_color(pal.text_faint)
                        .child("IN"),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_dim)
                        .child(SharedString::from(fmt_db(self.eq.settings.input_gain_db))),
                )
                .child(div().w(px(1.0)).h(px(10.0)).bg(pal.divider))
                .child(
                    div()
                        .text_size(px(8.5))
                        .text_color(pal.text_faint)
                        .child("OUT"),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_dim)
                        .child(SharedString::from(fmt_db(self.eq.settings.output_gain_db))),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let switch = {
            let weak = weak.clone();
            widgets::switch(
                self.eq.settings.enabled,
                pal.accent,
                "eq-enabled",
                move |on, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_eq_enabled(on, cx));
                },
            )
        };
        let close = {
            let weak = weak.clone();
            eq_icon_button(pal, Icon::Close, 30.0, "eq-close", move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.close_eq(cx));
            })
        };

        div()
            .h(px(60.0))
            .flex_none()
            .px(px(16.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .border_b_1()
            .border_color(pal.divider)
            .child(title)
            .child(div().flex_1())
            .child(save)
            .child(gains)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(pal.text_dim)
                            .child("启用"),
                    )
                    .child(switch),
            )
            .child(close)
            .into_any_element()
    }

    fn eq_desktop_rail(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let autoeq_count = (!self.eq.autoeq.entries.is_empty()).then_some(self.eq.autoeq.len());
        let counts = [
            (EqTab::Edit, None),
            (EqTab::V4, Some(v4_presets().len())),
            (EqTab::AutoEq, autoeq_count),
            (EqTab::Builtin, Some(builtin_presets().len())),
            (EqTab::Personal, Some(self.eq.saved.len())),
        ];
        let mut rail = div()
            .w(px(RAIL_W))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(pal.divider)
            .bg(pal.panel_alt)
            .px(px(12.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(3.0));

        rail = rail.child(eq_section_label(pal, "工作区"));
        for (tab, count) in counts {
            if tab == EqTab::V4 {
                rail = rail.child(
                    div()
                        .h(px(22.0))
                        .pt(px(8.0))
                        .child(eq_section_label(pal, "预设库")),
                );
            }
            let active = self.eq.tab == tab;
            let weak = weak.clone();
            let (title, _) = tab_meta(tab);
            let icon = tab_icon(tab);
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("eq-rail-{}", tab.label())))
                    .h(px(34.0))
                    .px(px(8.0))
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .cursor_pointer()
                    .bg(if active {
                        pal.accent_soft
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |s| {
                        s.bg(if active {
                            pal.accent_soft
                        } else {
                            pal.chip_hover
                        })
                    })
                    .child(match icon {
                        Some(icon) => {
                            icons::icon(icon, 15.0, if active { pal.accent } else { pal.text_dim })
                        }
                        None => div().into_any_element(),
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.0))
                            .font_weight(if active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(if active { pal.accent } else { pal.text })
                            .text_ellipsis()
                            .child(title),
                    )
                    .when_some(count, |d, count| {
                        d.child(
                            div()
                                .text_size(px(9.5))
                                .font_family(pal.mono.clone())
                                .text_color(if active { pal.accent } else { pal.text_faint })
                                .child(SharedString::from(count.to_string())),
                        )
                    })
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.select_eq_tab(tab, cx);
                        });
                    }),
            );
        }

        rail = rail.child(div().flex_1());
        rail = rail.child(
            div()
                .w_full()
                .rounded(px(10.0))
                .bg(pal.chip)
                .border_1()
                .border_color(pal.border)
                .p(px(10.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("当前配置"),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if self.eq.dirty { pal.accent } else { pal.text })
                        .text_ellipsis()
                        .child(SharedString::from(self.eq.preset_name.clone())),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_size(px(9.0)).text_color(pal.text_faint).child(
                            SharedString::from(format!(
                                "{} 个滤波器",
                                self.eq.settings.bands.len()
                            )),
                        ))
                        .child(
                            div()
                                .text_size(px(9.0))
                                .font_family(pal.mono.clone())
                                .text_color(pal.text_dim)
                                .child(SharedString::from(format!(
                                    "IN {}",
                                    db_num(self.eq.settings.input_gain_db)
                                ))),
                        ),
                ),
        );
        rail.child(
            div()
                .px(px(8.0))
                .pt(px(8.0))
                .text_size(px(9.0))
                .text_color(pal.text_faint)
                .child("F3 打开 / Esc 关闭"),
        )
        .into_any_element()
    }

    fn eq_editor_desktop(
        &self,
        pal: &EqPalette,
        panel_w: f32,
        panel_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let main_w = (panel_w - RAIL_W - INSPECTOR_W).max(280.0);
        let graph_h = (panel_h - 340.0).clamp(220.0, 460.0);

        let toolbar = {
            let weak_add = weak.clone();
            let weak_remove = weak.clone();
            let has_selection = self.eq.selected.is_some();
            let full = self.eq.settings.bands.len() >= EQ_MAX_BANDS;
            div()
                .h(px(28.0))
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(pal.text)
                                .child(SharedString::from(self.eq.preset_name.clone())),
                        )
                        .when(self.eq.dirty, |d| {
                            d.child(div().size(px(5.0)).rounded(px(2.5)).bg(pal.accent))
                        })
                        .child(
                            div()
                                .text_size(px(9.5))
                                .font_family(pal.mono.clone())
                                .text_color(pal.text_faint)
                                .child(SharedString::from(format!(
                                    "{} / {}",
                                    self.eq.settings.bands.len(),
                                    EQ_MAX_BANDS
                                ))),
                        ),
                )
                .child(div().flex_1())
                .when(has_selection, |d| {
                    d.child(
                        div()
                            .id("eq-toolbar-remove")
                            .h(px(26.0))
                            .px(px(9.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_size(px(10.5))
                            .text_color(pal.text_dim)
                            .hover(|s| s.bg(pal.chip_hover))
                            .child("移除频段")
                            .on_click(move |_, _, cx| {
                                let _ = weak_remove.update(cx, |app, cx| app.remove_eq_band(cx));
                            }),
                    )
                })
                .child(
                    div()
                        .id("eq-toolbar-add")
                        .h(px(26.0))
                        .px(px(9.0))
                        .rounded(px(7.0))
                        .bg(if full { pal.chip } else { pal.accent_soft })
                        .border_1()
                        .border_color(if full { pal.border } else { pal.accent_line })
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .cursor(if full {
                            gpui::CursorStyle::Arrow
                        } else {
                            gpui::CursorStyle::PointingHand
                        })
                        .child(icons::icon(
                            Icon::Plus,
                            11.0,
                            if full { pal.text_faint } else { pal.accent },
                        ))
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(if full { pal.text_faint } else { pal.accent })
                                .child("添加频段"),
                        )
                        .when(!full, |d| {
                            d.on_click(move |_, _, cx| {
                                let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                            })
                        }),
                )
                .into_any_element()
        };

        let hint = div()
            .h(px(16.0))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(px(9.0))
            .text_color(pal.text_faint)
            .child("点击曲线空白处添加频段")
            .child("·")
            .child("拖动节点调整频率与增益")
            .child("·")
            .child("右键删除");

        div()
            .w(px(main_w))
            .flex_none()
            .h_full()
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(toolbar)
            .child(self.eq_graph(pal, main_w - 28.0, graph_h, false, cx))
            .child(self.eq_fader_bank(pal, cx))
            .child(hint)
            .into_any_element()
    }

    fn eq_fader_bank(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut strip = div()
            .id("eq-fader-strip")
            .w_full()
            .h(px(142.0))
            .flex_none()
            .rounded(px(12.0))
            .bg(pal.inset)
            .border_1()
            .border_color(pal.border)
            .px(px(8.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .overflow_x_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll());

        if self.eq.settings.bands.is_empty() {
            let weak_add = weak.clone();
            strip = strip.child(
                div()
                    .id("eq-fader-empty")
                    .w_full()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(5.0))
                    .cursor_pointer()
                    .rounded(px(9.0))
                    .hover(|s| s.bg(pal.chip))
                    .child(icons::icon(Icon::Plus, 16.0, pal.accent))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(pal.text_dim)
                            .child("添加第一个频段"),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                    }),
            );
            return strip.into_any_element();
        }

        for (index, band) in self.eq.settings.bands.iter().enumerate() {
            let weak_select = weak.clone();
            let weak_change = weak.clone();
            let selected = self.eq.selected == Some(index);
            strip = strip.child(eq_band_fader(
                index,
                band.freq_hz,
                band.gain_db,
                band.enabled,
                band.kind.uses_gain(),
                band.kind.label(),
                selected,
                pal,
                move |cx| {
                    let _ = weak_select.update(cx, |app, cx| app.select_eq_band(index, cx));
                },
                move |db, cx| {
                    let _ = weak_change.update(cx, |app, cx| {
                        app.eq.selected = Some(index);
                        if let Some(band) = app.eq.settings.bands.get_mut(index) {
                            band.gain_db = db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB);
                            app.eq.touch();
                            app.apply_eq(cx);
                        }
                    });
                },
            ));
        }
        let weak_add = weak.clone();
        let full = self.eq.settings.bands.len() >= EQ_MAX_BANDS;
        if !full {
            strip = strip.child(
                div()
                    .id("eq-fader-add")
                    .w(px(48.0))
                    .h_full()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(3.0))
                    .rounded(px(9.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(pal.chip))
                    .child(icons::icon(Icon::Plus, 14.0, pal.accent))
                    .child(
                        div()
                            .text_size(px(8.5))
                            .text_color(pal.text_faint)
                            .child("添加"),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                    }),
            );
        }
        strip.into_any_element()
    }

    fn eq_graph(
        &self,
        pal: &EqPalette,
        width: f32,
        height: f32,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let geom = if compact {
            GraphGeom::compact(width, height)
        } else {
            GraphGeom::new(width, height)
        };
        let bounds_cell = Rc::new(Cell::new(Bounds::new(
            point(px(0.0), px(0.0)),
            size(px(0.0), px(0.0)),
        )));
        let dragging = Rc::new(Cell::new(false));
        let paint_settings = self.eq.settings.clone();
        let paint_selected = self.eq.selected;
        let paint_pal = pal.clone();
        let canvas_bounds = bounds_cell.clone();
        let canvas_el = canvas(
            move |bounds, _, _| {
                canvas_bounds.set(bounds);
                bounds
            },
            move |_bounds, bounds, window, _| {
                paint_eq_graph(
                    bounds,
                    &paint_settings,
                    paint_selected,
                    compact,
                    &paint_pal,
                    window,
                );
            },
        )
        .size_full();

        let weak = cx.entity().downgrade();
        let down_bounds = bounds_cell.clone();
        let down_drag = dragging.clone();
        let move_bounds = bounds_cell.clone();
        let move_drag = dragging.clone();
        let up_drag = dragging.clone();
        let right_bounds = bounds_cell.clone();
        let weak_down = weak.clone();
        let weak_move = weak.clone();
        let weak_right = weak.clone();
        let hit_radius = if compact { 28.0 } else { 18.0 };
        let add_limit = EQ_MAX_BANDS;

        let mut card = div()
            .id("eq-graph")
            .relative()
            .w_full()
            .h(px(height))
            .flex_none()
            .rounded(px(14.0))
            .bg(pal.inset)
            .border_1()
            .border_color(pal.border)
            .overflow_hidden()
            .cursor(gpui::CursorStyle::Crosshair)
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                let bounds = down_bounds.get();
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if w <= 1.0 || h <= 1.0 {
                    return;
                }
                let local_x = f32::from(event.position.x - bounds.left());
                let local_y = f32::from(event.position.y - bounds.top());
                let _ = weak_down.update(cx, |app, cx| {
                    let geom = if compact {
                        GraphGeom::compact(w, h)
                    } else {
                        GraphGeom::new(w, h)
                    };
                    let global = app.eq.settings.input_gain_db + app.eq.settings.output_gain_db;
                    let nearest = app
                        .eq
                        .settings
                        .bands
                        .iter()
                        .enumerate()
                        .map(|(index, band)| {
                            let dx = geom.x(band.freq_hz) - local_x;
                            let dy = geom.y(band.gain_db + global) - local_y;
                            (index, dx * dx + dy * dy)
                        })
                        .filter(|(_, distance)| *distance <= hit_radius * hit_radius)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(index, _)| index);
                    if let Some(index) = nearest {
                        app.eq.selected = Some(index);
                        app.eq.type_menu_open = false;
                        down_drag.set(true);
                        cx.notify();
                    } else if app.eq.settings.bands.len() < add_limit {
                        let freq = geom.freq_at(local_x);
                        let gain = geom.db_at(local_y) - global;
                        app.eq.settings.bands.push(chordal_player::EqBand::new(
                            freq,
                            gain.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB),
                            0.71,
                        ));
                        app.eq.selected = Some(app.eq.settings.bands.len() - 1);
                        app.eq.touch();
                        down_drag.set(true);
                        app.apply_eq(cx);
                    }
                });
            })
            .on_mouse_move(move |event, _, cx| {
                if !move_drag.get() || event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let bounds = move_bounds.get();
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if w <= 1.0 || h <= 1.0 {
                    return;
                }
                let local_x = f32::from(event.position.x - bounds.left());
                let local_y = f32::from(event.position.y - bounds.top());
                let _ = weak_move.update(cx, |app, cx| {
                    let Some(index) = app.eq.selected else { return };
                    let geom = if compact {
                        GraphGeom::compact(w, h)
                    } else {
                        GraphGeom::new(w, h)
                    };
                    let global = app.eq.settings.input_gain_db + app.eq.settings.output_gain_db;
                    if let Some(band) = app.eq.settings.bands.get_mut(index) {
                        band.freq_hz = geom.freq_at(local_x).clamp(20.0, 20_000.0);
                        band.gain_db =
                            (geom.db_at(local_y) - global).clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB);
                        app.eq.touch();
                        app.apply_eq(cx);
                    }
                });
            })
            .on_mouse_up(MouseButton::Left, move |_, _, _| {
                up_drag.set(false);
            })
            .on_mouse_down(MouseButton::Right, move |event, _, cx| {
                let bounds = right_bounds.get();
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if w <= 1.0 || h <= 1.0 {
                    return;
                }
                let local_x = f32::from(event.position.x - bounds.left());
                let local_y = f32::from(event.position.y - bounds.top());
                let _ = weak_right.update(cx, |app, cx| {
                    let geom = if compact {
                        GraphGeom::compact(w, h)
                    } else {
                        GraphGeom::new(w, h)
                    };
                    let global = app.eq.settings.input_gain_db + app.eq.settings.output_gain_db;
                    let nearest = app
                        .eq
                        .settings
                        .bands
                        .iter()
                        .enumerate()
                        .map(|(index, band)| {
                            let dx = geom.x(band.freq_hz) - local_x;
                            let dy = geom.y(band.gain_db + global) - local_y;
                            (index, dx * dx + dy * dy)
                        })
                        .filter(|(_, distance)| *distance <= 20.0 * 20.0)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(index, _)| index);
                    if let Some(index) = nearest {
                        app.eq.settings.bands.remove(index);
                        app.eq.selected = if app.eq.settings.bands.is_empty() {
                            None
                        } else {
                            Some(index.min(app.eq.settings.bands.len() - 1))
                        };
                        app.eq.touch();
                        app.apply_eq(cx);
                    }
                });
            })
            .child(div().absolute().inset_0().child(canvas_el));

        // dB 标尺与频率轴。
        let db_ticks: &[f32] = if compact {
            &[24.0, 12.0, 0.0, -12.0, -24.0]
        } else {
            &[24.0, 18.0, 12.0, 6.0, 0.0, -6.0, -12.0, -18.0, -24.0]
        };
        for db in db_ticks {
            card = card.child(
                div()
                    .absolute()
                    .left(px(4.0))
                    .top(px(geom.y(*db) - 5.0))
                    .w(px(if compact { 24.0 } else { 28.0 }))
                    .text_size(px(8.0))
                    .font_family(pal.mono.clone())
                    .text_color(if *db == 0.0 {
                        pal.text_dim
                    } else {
                        pal.text_faint
                    })
                    .text_align(gpui::TextAlign::Right)
                    .child(SharedString::from(if *db > 0.0 {
                        format!("+{}", *db as i32)
                    } else {
                        format!("{}", *db as i32)
                    })),
            );
        }
        let freq_ticks: &[f32] = if compact {
            &[31.5, 125.0, 500.0, 2_000.0, 8_000.0, 16_000.0]
        } else {
            &[
                31.5, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
            ]
        };
        for freq in freq_ticks {
            card = card.child(
                div()
                    .absolute()
                    .left(px(geom.x(*freq) - 16.0))
                    .bottom(px(4.0))
                    .w(px(32.0))
                    .text_size(px(8.0))
                    .font_family(pal.mono.clone())
                    .text_color(pal.text_faint)
                    .text_center()
                    .child(SharedString::from(fmt_freq(*freq))),
            );
        }
        if !compact {
            card = card.child(
                div()
                    .absolute()
                    .left(px(4.0))
                    .top(px(2.0))
                    .text_size(px(7.5))
                    .text_color(pal.text_faint)
                    .child("dB"),
            );
        }

        if self.eq.settings.bands.is_empty() {
            card = card.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(5.0))
                    .child(icons::icon(Icon::Plus, 16.0, pal.accent))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(pal.text_dim)
                            .child("点按曲线添加频段"),
                    ),
            );
        }

        if let Some(index) = self.eq.selected
            && let Some(band) = self.eq.settings.bands.get(index)
        {
            let global = self.eq.settings.input_gain_db + self.eq.settings.output_gain_db;
            let x = geom.x(band.freq_hz);
            let y = geom.y(band.gain_db + global);
            let tip = band_summary(index, band);
            card = card.child(
                div()
                    .absolute()
                    .left(px((x - 82.0).clamp(4.0, (width - 168.0).max(4.0))))
                    .top(px((y - 32.0).max(3.0)))
                    .w(px(164.0))
                    .h(px(23.0))
                    .rounded(px(12.0))
                    .bg(pal.panel)
                    .border_1()
                    .border_color(pal.border_strong)
                    .shadow(vec![BoxShadow {
                        color: pal.shadow,
                        offset: point(px(0.0), px(6.0)),
                        blur_radius: px(16.0),
                        spread_radius: px(0.0),
                    }])
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(9.0))
                    .font_family(pal.mono.clone())
                    .text_color(pal.text)
                    .child(SharedString::from(tip)),
            );
        }
        card.into_any_element()
    }

    fn eq_inspector_desktop(
        &self,
        pal: &EqPalette,
        main_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut body = div()
            .id("eq-inspector")
            .h(px(main_h))
            .flex_none()
            .w(px(INSPECTOR_W))
            .border_l_1()
            .border_color(pal.divider)
            .px(px(14.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .overflow_y_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll());

        let selected = self.eq.selected.and_then(|index| {
            self.eq
                .settings
                .bands
                .get(index)
                .map(|band| (index, band.clone()))
        });

        body = body.child(
            div()
                .h(px(18.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(pal.text_faint)
                        .child("滤波器检视器"),
                )
                .when_some(selected.as_ref().map(|(index, _)| *index), |d, index| {
                    let weak_toggle = weak.clone();
                    let weak_remove = weak.clone();
                    let enabled = selected
                        .as_ref()
                        .map(|(_, band)| band.enabled)
                        .unwrap_or(false);
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .id("eq-inspector-enabled")
                                    .h(px(20.0))
                                    .px(px(6.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(3.0))
                                    .cursor_pointer()
                                    .bg(if enabled { pal.accent_soft } else { pal.chip })
                                    .text_size(px(9.0))
                                    .text_color(if enabled { pal.accent } else { pal.text_faint })
                                    .hover(|s| s.bg(pal.chip_hover))
                                    .child(div().size(px(4.0)).rounded(px(2.0)).bg(if enabled {
                                        pal.accent
                                    } else {
                                        pal.text_faint
                                    }))
                                    .child(if enabled { "启用" } else { "旁路" })
                                    .on_click(move |_, _, cx| {
                                        let _ = weak_toggle.update(cx, |app, cx| {
                                            if let Some(band) = app.eq.settings.bands.get_mut(index)
                                            {
                                                band.enabled = !enabled;
                                                app.eq.touch();
                                                app.apply_eq(cx);
                                            }
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .id("eq-inspector-remove")
                                    .h(px(20.0))
                                    .px(px(6.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(3.0))
                                    .cursor_pointer()
                                    .text_size(px(9.0))
                                    .text_color(pal.danger)
                                    .hover(|s| s.bg(pal.danger_soft))
                                    .child(icons::icon(Icon::Close, 9.0, pal.danger))
                                    .child("移除")
                                    .on_click(move |_, _, cx| {
                                        let _ = weak_remove.update(cx, |app, cx| {
                                            app.eq.selected = Some(index);
                                            app.remove_eq_band(cx);
                                        });
                                    }),
                            ),
                    )
                }),
        );

        if let Some((index, band)) = selected.clone() {
            body = body.child(
                div()
                    .w_full()
                    .rounded(px(10.0))
                    .bg(pal.chip)
                    .border_1()
                    .border_color(pal.border)
                    .p(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(8.5))
                                    .text_color(pal.text_faint)
                                    .child(SharedString::from(format!("频段 {:02}", index + 1))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(3.0))
                                    .child(
                                        div()
                                            .text_size(px(20.0))
                                            .font_family(pal.mono.clone())
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(pal.text)
                                            .child(SharedString::from(fmt_freq(band.freq_hz))),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(8.5))
                                            .text_color(pal.text_faint)
                                            .child("Hz"),
                                    ),
                            ),
                    )
                    .child(div().w(px(1.0)).h(px(32.0)).bg(pal.divider))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(8.5))
                                    .text_color(pal.text_faint)
                                    .child("增益"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(3.0))
                                    .child(
                                        div()
                                            .text_size(px(20.0))
                                            .font_family(pal.mono.clone())
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(if band.enabled {
                                                pal.accent
                                            } else {
                                                pal.text_faint
                                            })
                                            .child(SharedString::from(if band.kind.uses_gain() {
                                                db_num(band.gain_db)
                                            } else {
                                                "—".into()
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(8.5))
                                            .text_color(pal.text_faint)
                                            .child("dB"),
                                    ),
                            ),
                    ),
            );

            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .child(
                        div()
                            .text_size(px(9.0))
                            .text_color(pal.text_faint)
                            .child("滤波器类型"),
                    )
                    .child(self.eq_kind_grid(pal, index, band.kind, cx)),
            );

            body = body.child(self.eq_params_block(pal, index, &band, false, cx));
            body = body.child(eq_divider(pal));
        } else {
            body = body.child(
                div()
                    .w_full()
                    .rounded(px(10.0))
                    .bg(pal.chip)
                    .border_1()
                    .border_color(pal.border)
                    .p(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(pal.text_dim)
                            .child("未选择频段"),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .line_height(px(15.0))
                            .text_color(pal.text_faint)
                            .child("在曲线或推子上点选一个频段；点击曲线空白处可以直接新建。"),
                    ),
            );
            body = body.child(eq_divider(pal));
        }

        body = body.child(div().flex_1());
        body = body.child(self.eq_master_block(pal, false, cx));
        body.into_any_element()
    }

    fn eq_kind_grid(
        &self,
        pal: &EqPalette,
        _index: usize,
        active: FilterKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut row = div().w_full().flex().gap(px(4.0));
        for kind in FilterKind::ALL {
            let weak = weak.clone();
            let is_active = active == kind;
            row = row.child(
                div()
                    .id(SharedString::from(format!("eq-kind-{}", kind.label())))
                    .h(px(28.0))
                    .flex_1()
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(if is_active {
                        pal.accent_line
                    } else {
                        pal.border
                    })
                    .bg(if is_active { pal.accent_soft } else { pal.chip })
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_size(px(9.5))
                    .text_color(if is_active { pal.accent } else { pal.text_dim })
                    .hover(move |s| {
                        s.bg(if is_active {
                            pal.accent_soft
                        } else {
                            pal.chip_hover
                        })
                    })
                    .child(kind.label())
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.set_eq_filter_kind(kind, cx);
                        });
                    }),
            );
        }
        row.into_any_element()
    }

    fn eq_params_block(
        &self,
        pal: &EqPalette,
        _index: usize,
        band: &chordal_player::EqBand,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let colors = pal.slider_colors();
        let slider_h = if compact { 30.0 } else { 26.0 };
        let mut block =
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(if compact { px(8.0) } else { px(6.0) });

        let freq_slider = {
            let weak = weak.clone();
            widgets::eq_knob_slider(
                freq_to_slider(band.freq_hz),
                colors,
                "eq-freq",
                move |value: f32, cx: &mut App| {
                    let freq = slider_to_freq(value);
                    let _ = weak.update(cx, |app, cx| app.set_eq_freq(freq, cx));
                },
            )
        };
        block = block.child(eq_param_block(
            pal,
            "频率",
            format!("{} Hz", fmt_freq(band.freq_hz)),
            freq_slider,
        ));

        if band.kind.uses_gain() {
            let gain = band.gain_db;
            let weak = weak.clone();
            let gain_slider = eq_bipolar_slider(
                ((gain + 24.0) / 48.0).clamp(0.0, 1.0),
                colors,
                pal.text_faint,
                slider_h,
                5.5,
                "eq-gain",
                move |value: f32, cx: &mut App| {
                    let db = value * 48.0 - 24.0;
                    let _ = weak.update(cx, |app, cx| app.set_eq_gain(db, cx));
                },
            );
            block = block.child(eq_param_block(
                pal,
                "增益",
                format!("{} dB", db_num(gain)),
                gain_slider,
            ));
        }

        let q = band.q;
        let weak_q = weak.clone();
        let q_slider = widgets::eq_knob_slider(
            q_to_slider(q),
            colors,
            "eq-q",
            move |value: f32, cx: &mut App| {
                let q = slider_to_q(value);
                let _ = weak_q.update(cx, |app, cx| app.set_eq_q(q, cx));
            },
        );
        block = block.child(eq_param_block(
            pal,
            "品质因数",
            format!("Q {q:.2}"),
            q_slider,
        ));
        block.into_any_element()
    }

    fn eq_master_block(
        &self,
        pal: &EqPalette,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let colors = pal.slider_colors();
        let slider_h = if compact { 30.0 } else { 26.0 };
        let mut block = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(if compact { px(8.0) } else { px(6.0) })
            .child(
                div()
                    .text_size(px(9.0))
                    .text_color(pal.text_faint)
                    .child("输入 / 输出增益"),
            );

        for (label, value_db, output, id) in [
            (
                "输入增益",
                self.eq.settings.input_gain_db,
                false,
                "eq-input-gain",
            ),
            (
                "输出增益",
                self.eq.settings.output_gain_db,
                true,
                "eq-output-gain",
            ),
        ] {
            let weak = weak.clone();
            let slider = eq_bipolar_slider(
                ((value_db + 24.0) / 48.0).clamp(0.0, 1.0),
                colors,
                pal.text_faint,
                slider_h,
                5.5,
                id,
                move |value: f32, cx: &mut App| {
                    let db = value * 48.0 - 24.0;
                    let _ = weak.update(cx, |app, cx| {
                        if output {
                            app.eq.settings.output_gain_db = db;
                        } else {
                            app.eq.settings.input_gain_db = db;
                        }
                        app.eq.touch();
                        app.apply_eq(cx);
                    });
                },
            );
            block = block.child(eq_param_block(
                pal,
                label,
                format!("{} dB", db_num(value_db)),
                slider,
            ));
        }
        block.into_any_element()
    }

    fn eq_statusbar_desktop(&self, pal: &EqPalette) -> AnyElement {
        div()
            .h(px(30.0))
            .flex_none()
            .px(px(16.0))
            .flex()
            .items_center()
            .gap(px(7.0))
            .border_t_1()
            .border_color(pal.divider)
            .child(
                div()
                    .size(px(5.0))
                    .rounded(px(2.5))
                    .bg(if self.eq.settings.enabled {
                        pal.ok
                    } else {
                        pal.text_faint
                    }),
            )
            .child(div().text_size(px(9.0)).text_color(pal.text_dim).child(
                if self.eq.settings.enabled {
                    "实时生效"
                } else {
                    "已旁路"
                },
            ))
            .child(
                div()
                    .text_size(px(9.0))
                    .text_color(pal.text_faint)
                    .child("· 参数热更新，不中断播放"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(9.0))
                    .font_family(pal.mono.clone())
                    .text_color(pal.text_faint)
                    .child("Esc 关闭"),
            )
            .into_any_element()
    }

    // ---------- 桌面：预设库 ----------

    fn eq_library_desktop(
        &self,
        pal: &EqPalette,
        entries: Vec<PresetEntry>,
        main_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, subtitle) = tab_meta(self.eq.tab);
        let filtered: Vec<PresetEntry> = entries;

        let active_name = self.eq.preset_name.clone();
        let preview_entry = self
            .eq
            .preview
            .filter(|entry| {
                filtered
                    .iter()
                    .any(|candidate| candidate.name == entry.name)
            })
            .or_else(|| {
                filtered
                    .iter()
                    .find(|entry| entry.name == active_name && !self.eq.dirty)
                    .copied()
            })
            .or_else(|| filtered.first().copied());

        let mut head = div()
            .h(px(38.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child(subtitle),
                    ),
            )
            .child(div().flex_1());
        head = head.child(
            div()
                .h(px(24.0))
                .px(px(9.0))
                .rounded(px(12.0))
                .bg(pal.chip)
                .flex()
                .items_center()
                .text_size(px(9.5))
                .font_family(pal.mono.clone())
                .text_color(pal.text_dim)
                .child(SharedString::from(format!("{} 个配置", filtered.len()))),
        );

        // 预设列表按需渲染可视行：内置/V4 条目在 EQ 打开、滚动时
        // 不再全部进入布局与预绘制。
        let list: AnyElement = if filtered.is_empty() {
            div()
                .id("eq-library-list")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .when(self.smooth_scroll, |d| d.smooth_scroll())
                .pr(px(4.0))
                .child(
                    div()
                        .w_full()
                        .h(px(140.0))
                        .rounded(px(12.0))
                        .bg(pal.panel)
                        .border_1()
                        .border_color(pal.border)
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(5.0))
                        .child(icons::icon(Icon::Headphones, 20.0, pal.text_faint))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(pal.text_dim)
                                .child("没有匹配的配置"),
                        ),
                )
                .into_any_element()
        } else {
            let count = filtered.len();
            let entries = filtered;
            let pal_list = pal.clone();
            let weak_list = weak.clone();
            let dirty = self.eq.dirty;
            let active_name = self.eq.preset_name.clone();
            let preview_name = preview_entry.map(|entry| entry.name);
            let icon = Icon::Equalizer;
            let mut list =
                gpui::uniform_list("eq-library-list", count, move |range, _window, _cx| {
                    range
                        .map(|index| {
                            if eq_probe("rows") {
                                return div().h(px(60.0)).into_any_element();
                            }
                            let entry = entries[index];
                            let weak_preview = weak_list.clone();
                            let previewed = preview_name == Some(entry.name);
                            let active = !dirty && active_name == entry.name;
                            div()
                                .h(px(60.0))
                                .pb(px(6.0))
                                .child(eq_preset_row(
                                    &pal_list,
                                    entry.name,
                                    entry.subtitle(),
                                    Some(icon),
                                    None,
                                    active,
                                    previewed,
                                    false,
                                    SharedString::from(format!("eq-preset-{index}")),
                                    move |_, _, cx| {
                                        let _ = weak_preview.update(cx, |app, cx| {
                                            app.eq.preview_preset(entry);
                                            cx.notify();
                                        });
                                    },
                                ))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                });
            if self.smooth_scroll {
                list = list.smooth_scroll();
            }
            list.flex_1()
                .min_w_0()
                .h_full()
                .pr(px(4.0))
                .into_any_element()
        };

        let preview = if eq_probe("preview") {
            div().w(px(292.0)).h_full().flex_none().into_any_element()
        } else {
            match preview_entry {
                Some(entry) => {
                    let settings = entry.to_settings();
                    let active = !self.eq.dirty && self.eq.preset_name == entry.name;
                    let weak_apply = weak.clone();
                    let on_apply: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                        let _ = weak_apply.update(cx, |app, cx| {
                            app.load_eq_preset(entry, cx);
                            cx.notify();
                        });
                    });
                    eq_preview_panel(
                        pal,
                        &settings,
                        entry.name,
                        &entry.subtitle(),
                        active,
                        on_apply,
                        None,
                    )
                }
                None => eq_preview_empty(pal, "选择一个配置查看预览"),
            }
        };

        let mut body = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(px(14.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(head);
        body = body.child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(px(12.0))
                .child(list)
                .child(
                    div()
                        .id("eq-preview-pane-presets")
                        .w(px(292.0))
                        .h_full()
                        .flex_none()
                        .overflow_y_scroll()
                        .when(self.smooth_scroll, |d| d.smooth_scroll())
                        .child(preview),
                ),
        );
        let _ = main_h;
        body.into_any_element()
    }

    // ---------- 桌面：AutoEQ 耳机档案 ----------

    fn eq_autoeq_desktop(
        &self,
        pal: &EqPalette,
        main_h: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, subtitle) = tab_meta(EqTab::AutoEq);
        let lib = &self.eq.autoeq;
        let entries = lib.entries.clone();
        let total = entries.len();
        let filtered = lib.visible_indices(&self.eq.category, &self.eq.search);
        let filtered_count = filtered.len();
        let active_name = self.eq.preset_name.clone();
        let dirty = self.eq.dirty;

        let preview_index = self
            .eq
            .autoeq_preview
            .as_deref()
            .and_then(|file| filtered.iter().copied().find(|&index| entries[index].file == file))
            .or_else(|| {
                (!dirty)
                    .then(|| {
                        filtered
                            .iter()
                            .copied()
                            .find(|&index| entries[index].name == active_name)
                    })
                    .flatten()
            })
            .or_else(|| filtered.first().copied());
        let preview_file = preview_index.map(|index| entries[index].file.clone());

        let mut head = div()
            .h(px(38.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child(subtitle),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .w(px(230.0))
                    .child(eq_search_box(pal, self.eq_search_view.clone())),
            );
        let weak_sync = weak.clone();
        head = head.child(eq_sync_chip(
            pal,
            "eq-autoeq-sync",
            lib.index_loading,
            move |_, _, cx| {
                let _ = weak_sync.update(cx, |app, cx| app.sync_autoeq(true, cx));
            },
        ));

        let mut chips = div().h(px(28.0)).flex().items_center().gap(px(6.0));
        for label in ["全部", "头戴式", "入耳式", "耳塞式"] {
            let weak = weak.clone();
            let active = self.eq.category == label;
            chips = chips.child(eq_chip(
                pal,
                label,
                active,
                SharedString::from(format!("eq-cat-{label}")),
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.eq.category = label.to_string();
                        cx.notify();
                    });
                },
            ));
        }
        chips = chips.child(div().flex_1()).child(
            div()
                .text_size(px(9.0))
                .font_family(pal.mono.clone())
                .text_color(pal.text_faint)
                .child(SharedString::from(autoeq_status_text(lib, filtered_count))),
        );

        let loading = lib.index_loading || lib.status == AutoEqStatus::Loading;
        let content: AnyElement = if total == 0 {
            if loading {
                eq_notice(
                    pal,
                    Icon::Refresh,
                    "正在从 AutoEq 仓库拉取…",
                    "索引约 500 KB，首次加载需要几秒；完成后会缓存到本地。",
                    None,
                )
            } else {
                let hint = lib.error.clone().unwrap_or_else(|| {
                    "从 github.com/jaakkopasanen/AutoEq 拉取 6000+ 款耳机的校准曲线，之后离线可用。"
                        .to_string()
                });
                let weak_retry = weak.clone();
                let action: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
                    let _ = weak_retry.update(cx, |app, cx| app.sync_autoeq(true, cx));
                });
                eq_notice(
                    pal,
                    Icon::Cloud,
                    if lib.error.is_some() {
                        "AutoEQ 索引拉取失败"
                    } else {
                        "尚未拉取 AutoEQ 耳机档案"
                    },
                    &hint,
                    Some(("拉取耳机档案", action)),
                )
            }
        } else if filtered.is_empty() {
            eq_notice(
                pal,
                Icon::Headphones,
                "没有匹配的耳机",
                "换个分类，或输入型号关键词后按回车搜索。",
                None,
            )
        } else {
            let indices = Arc::new(filtered);
            let count = indices.len();
            let rows_entries = entries.clone();
            let pal_rows = pal.clone();
            let weak_rows = weak.clone();
            let rows_preview = preview_file.clone();
            let rows_active = active_name.clone();
            let mut list = gpui::uniform_list("eq-autoeq-list", count, move |range, _window, _cx| {
                range
                    .map(|row| {
                        if eq_probe("rows") {
                            return div().h(px(60.0)).into_any_element();
                        }
                        let index = indices[row];
                        let entry = &rows_entries[index];
                        let file = entry.file.clone();
                        let name = entry.name.clone();
                        let subtitle = entry.subtitle();
                        let previewed = rows_preview.as_deref() == Some(entry.file.as_str());
                        let active = !dirty && rows_active == entry.name;
                        let badge = if entry.loading {
                            Some("拉取中".to_string())
                        } else if entry.error.is_some() {
                            Some("失败".to_string())
                        } else {
                            entry
                                .bands
                                .as_ref()
                                .map(|bands| format!("{} 段", bands.len()))
                        };
                        let weak_row = weak_rows.clone();
                        div()
                            .h(px(60.0))
                            .pb(px(6.0))
                            .child(eq_preset_row(
                                &pal_rows,
                                &name,
                                subtitle,
                                Some(Icon::Headphones),
                                badge,
                                active,
                                previewed,
                                false,
                                SharedString::from(format!("eq-autoeq-{index}")),
                                move |_, _, cx| {
                                    let _ = weak_row.update(cx, |app, cx| {
                                        app.select_autoeq_entry(file.clone(), false, cx);
                                    });
                                },
                            ))
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            });
            if self.smooth_scroll {
                list = list.smooth_scroll();
            }
            let list = list
                .flex_1()
                .min_w_0()
                .h_full()
                .pr(px(4.0))
                .into_any_element();

            let preview: AnyElement = match preview_index {
                Some(index) => {
                    let entry = &entries[index];
                    let active = !dirty && active_name == entry.name;
                    match entry.to_settings() {
                        Some(settings) => {
                            let weak_apply = weak.clone();
                            let file = entry.file.clone();
                            let on_apply: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
                                let _ = weak_apply
                                    .update(cx, |app, cx| app.apply_autoeq_entry(&file, cx));
                            });
                            eq_preview_panel(
                                pal,
                                &settings,
                                &entry.name,
                                &entry.subtitle(),
                                active,
                                on_apply,
                                None,
                            )
                        }
                        None if entry.loading => eq_notice(
                            pal,
                            Icon::Refresh,
                            "正在拉取校准曲线…",
                            "从 AutoEq 仓库下载这个型号的 ParametricEQ.txt。",
                            None,
                        ),
                        None => {
                            let file = entry.file.clone();
                            let weak_action = weak.clone();
                            let action: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
                                let _ = weak_action.update(cx, |app, cx| {
                                    app.select_autoeq_entry(file.clone(), false, cx);
                                });
                            });
                            match &entry.error {
                                Some(error) => eq_notice(
                                    pal,
                                    Icon::Alert,
                                    "拉取失败",
                                    error,
                                    Some(("重试", action)),
                                ),
                                None => eq_notice(
                                    pal,
                                    Icon::Cloud,
                                    "按需拉取",
                                    "点按左侧耳机型号下载曲线并预览；应用时不会重复下载。",
                                    Some(("拉取曲线", action)),
                                ),
                            }
                        }
                    }
                }
                None => eq_preview_empty(pal, "选择一个耳机型号"),
            };

            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(px(12.0))
                .child(list)
                .child(
                    div()
                        .id("eq-autoeq-preview-pane")
                        .w(px(292.0))
                        .h_full()
                        .flex_none()
                        .overflow_y_scroll()
                        .when(self.smooth_scroll, |d| d.smooth_scroll())
                        .child(preview),
                )
                .into_any_element()
        };

        let mut body = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(px(14.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(head)
            .child(chips);
        if let Some(error) = &self.eq.autoeq.error
            && total > 0
        {
            body = body.child(
                div()
                    .text_size(px(9.0))
                    .text_color(pal.danger)
                    .text_ellipsis()
                    .child(SharedString::from(format!("同步失败：{error}"))),
            );
        }
        body = body.child(content);
        let _ = main_h;
        body.into_any_element()
    }

    fn eq_saved_desktop(&self, pal: &EqPalette, main_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, subtitle) = tab_meta(EqTab::Personal);
        let preview_index = self
            .eq
            .preview_saved
            .filter(|index| *index < self.eq.saved.len())
            .or_else(|| {
                (!self.eq.dirty)
                    .then(|| {
                        self.eq
                            .saved
                            .iter()
                            .position(|preset| preset.name == self.eq.preset_name)
                    })
                    .flatten()
            })
            .or_else(|| (!self.eq.saved.is_empty()).then_some(0));

        let head = div()
            .h(px(38.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child(subtitle),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .h(px(24.0))
                    .px(px(9.0))
                    .rounded(px(12.0))
                    .bg(pal.chip)
                    .flex()
                    .items_center()
                    .text_size(px(9.5))
                    .font_family(pal.mono.clone())
                    .text_color(pal.text_dim)
                    .child(SharedString::from(format!(
                        "{} 个配置",
                        self.eq.saved.len()
                    ))),
            );

        let mut list = div()
            .id("eq-saved-list")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll())
            .flex()
            .flex_col()
            .gap(px(6.0))
            .pr(px(4.0));

        if self.eq.saved.is_empty() {
            list = list.child(
                div()
                    .w_full()
                    .h(px(180.0))
                    .rounded(px(12.0))
                    .bg(pal.panel)
                    .border_1()
                    .border_color(pal.border)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .child(icons::icon(Icon::Person, 20.0, pal.text_faint))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(pal.text_dim)
                            .child("还没有个人配置"),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child("在编辑器里调好曲线，然后点右上角「另存为」"),
                    ),
            );
        }

        for (index, preset) in self.eq.saved.iter().enumerate() {
            let weak_preview = weak.clone();
            let name = preset.name.clone();
            let subtitle = format!(
                "{} 个滤波器 · 输入 {}",
                preset.settings.bands.len(),
                fmt_db(preset.settings.input_gain_db)
            );
            let active = !self.eq.dirty && self.eq.preset_name == name;
            let previewed = preview_index == Some(index);
            list = list.child(eq_preset_row(
                pal,
                &name,
                subtitle,
                Some(Icon::Person),
                None,
                active,
                previewed,
                false,
                SharedString::from(format!("eq-saved-{index}")),
                move |_, _, cx| {
                    let _ = weak_preview.update(cx, |app, cx| {
                        app.eq.preview_saved(index);
                        cx.notify();
                    });
                },
            ));
        }

        let preview = match preview_index.and_then(|index| {
            self.eq
                .saved
                .get(index)
                .map(|preset| (index, preset.clone()))
        }) {
            Some((index, preset)) => {
                let active = !self.eq.dirty && self.eq.preset_name == preset.name;
                let weak_apply = weak.clone();
                let apply_preset = preset.clone();
                let on_apply: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                    let _ = weak_apply.update(cx, |app, cx| {
                        app.eq.load_saved(&apply_preset);
                        app.eq.preview_saved(index);
                        app.apply_eq(cx);
                        cx.notify();
                    });
                });
                let weak_remove = weak.clone();
                let on_remove: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                    let _ = weak_remove.update(cx, |app, cx| {
                        app.remove_saved_preset(index, cx);
                        app.eq.preview_saved = None;
                    });
                });
                eq_preview_panel(
                    pal,
                    &preset.settings,
                    &preset.name,
                    "个人配置",
                    active,
                    on_apply,
                    Some(on_remove),
                )
            }
            None => eq_preview_empty(pal, "还没有可预览的个人配置"),
        };

        let _ = main_h;
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(px(14.0))
            .py(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(head)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(12.0))
                    .child(list)
                    .child(
                        div()
                            .id("eq-preview-pane-saved")
                            .w(px(292.0))
                            .h_full()
                            .flex_none()
                            .overflow_y_scroll()
                            .when(self.smooth_scroll, |d| d.smooth_scroll())
                            .child(preview),
                    ),
            )
            .into_any_element()
    }

    // ---------- 紧凑：触控主界面 ----------

    fn eq_render_compact(&self, pal: &EqPalette, enter: f32, cx: &mut Context<Self>) -> AnyElement {
        let side = if self.viewport_width < 560.0 {
            12.0
        } else {
            24.0
        };
        let content_w = (self.viewport_width - side * 2.0).min(COMPACT_MAX_W);
        let header = self.eq_header_compact(pal, cx);
        let nav = self.eq_nav_compact(pal, cx);
        let body: AnyElement = match self.eq.tab {
            EqTab::Edit => self.eq_editor_compact(pal, content_w, cx),
            EqTab::V4 => self.eq_library_compact(pal, v4_presets(), cx),
            EqTab::AutoEq => self.eq_autoeq_compact(pal, cx),
            EqTab::Builtin => self.eq_library_compact(pal, builtin_presets(), cx),
            EqTab::Personal => self.eq_saved_compact(pal, cx),
        };
        div()
            .id("eq-page")
            .key_context("ChordialEq")
            .track_focus(&self.focus_handle)
            .on_action(
                cx.listener(|app, _: &crate::actions::ExitImmersive, window, cx| {
                    app.close_eq(cx);
                    cx.stop_propagation();
                    window.focus(&app.focus_handle);
                }),
            )
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .overflow_hidden()
            .bg(linear_gradient(
                180.0,
                linear_color_stop(pal.bg_top, 0.0),
                linear_color_stop(pal.bg_bottom, 1.0),
            ))
            .opacity(enter)
            .child(
                div()
                    .w(px(content_w))
                    .h_full()
                    .font_family(pal.font.clone())
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "eq-compact-scroll-{}",
                                self.eq.tab.label()
                            )))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .when(self.smooth_scroll, |d| d.smooth_scroll())
                            .pb(px(16.0))
                            .child(body),
                    )
                    .child(nav),
            )
            .into_any_element()
    }

    fn eq_header_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let back = {
            let weak = weak.clone();
            eq_icon_button(pal, Icon::ChevronLeft, 32.0, "eq-back", move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.close_eq(cx));
            })
        };
        let switch = {
            let weak = weak.clone();
            widgets::switch(
                self.eq.settings.enabled,
                pal.accent,
                "eq-enabled-compact",
                move |on, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_eq_enabled(on, cx));
                },
            )
        };
        div()
            .h(px(58.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(back)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child("均衡器"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .max_w(px(200.0))
                                    .text_size(px(10.0))
                                    .text_color(if self.eq.dirty {
                                        pal.accent
                                    } else {
                                        pal.text_dim
                                    })
                                    .text_ellipsis()
                                    .child(SharedString::from(self.eq.display_name())),
                            )
                            .child(div().text_size(px(10.0)).text_color(pal.text_faint).child(
                                SharedString::from(format!(
                                    "· {} 个频段",
                                    self.eq.settings.bands.len()
                                )),
                            )),
                    ),
            )
            .child(
                div()
                    .text_size(px(9.5))
                    .text_color(pal.text_dim)
                    .child("启用"),
            )
            .child(switch)
            .into_any_element()
    }

    fn eq_editor_compact(
        &self,
        pal: &EqPalette,
        content_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let graph_h = (self.viewport_height * 0.27).clamp(190.0, 300.0);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(self.eq_graph(pal, content_w, graph_h, true, cx))
            .child(self.eq_band_chips(pal, cx))
            .child(self.eq_inspector_compact(pal, cx))
            .into_any_element()
    }

    fn eq_band_chips(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        if self.eq.settings.bands.is_empty() {
            let weak_add = weak.clone();
            return div()
                .id("eq-chips-empty")
                .w_full()
                .h(px(66.0))
                .rounded(px(12.0))
                .border_1()
                .border_color(pal.accent_line)
                .bg(pal.accent_soft)
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                })
                .child(icons::icon(Icon::Plus, 15.0, pal.accent))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(pal.accent)
                        .child("点按曲线或此处添加频段"),
                )
                .into_any_element();
        }

        let mut row = div()
            .id("eq-chips")
            .w_full()
            .h(px(66.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .overflow_x_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll());
        for (index, band) in self.eq.settings.bands.iter().enumerate() {
            let weak_select = weak.clone();
            let selected = self.eq.selected == Some(index);
            row = row.child(
                div()
                    .id(SharedString::from(format!("eq-chip-band-{index}")))
                    .w(px(64.0))
                    .h(px(64.0))
                    .flex_none()
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(if selected {
                        pal.accent_line
                    } else {
                        pal.border
                    })
                    .bg(if selected { pal.selected_bg } else { pal.panel })
                    .p(px(9.0))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = weak_select.update(cx, |app, cx| app.select_eq_band(index, cx));
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(8.0))
                                    .font_family(pal.mono.clone())
                                    .text_color(if selected { pal.accent } else { pal.text_faint })
                                    .child(SharedString::from(format!("{:02}", index + 1))),
                            )
                            .child(div().size(px(4.0)).rounded(px(2.0)).bg(if band.enabled {
                                pal.accent
                            } else {
                                pal.text_faint
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_family(pal.mono.clone())
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(pal.text)
                                    .child(SharedString::from(fmt_freq(band.freq_hz))),
                            )
                            .child(
                                div()
                                    .text_size(px(9.0))
                                    .font_family(pal.mono.clone())
                                    .text_color(if band.kind.uses_gain() && band.enabled {
                                        pal.accent
                                    } else {
                                        pal.text_faint
                                    })
                                    .child(SharedString::from(if band.kind.uses_gain() {
                                        db_num(band.gain_db)
                                    } else {
                                        band.kind.label().to_string()
                                    })),
                            ),
                    ),
            );
        }
        let weak_add = weak.clone();
        let full = self.eq.settings.bands.len() >= EQ_MAX_BANDS;
        row = row.child(
            div()
                .id("eq-chip-add")
                .w(px(64.0))
                .h(px(64.0))
                .flex_none()
                .rounded(px(12.0))
                .border_1()
                .border_color(pal.border)
                .bg(pal.chip)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(2.0))
                .cursor(if full {
                    gpui::CursorStyle::Arrow
                } else {
                    gpui::CursorStyle::PointingHand
                })
                .when(!full, |d| {
                    d.on_click(move |_, _, cx| {
                        let _ = weak_add.update(cx, |app, cx| app.add_eq_band(cx));
                    })
                })
                .child(icons::icon(
                    Icon::Plus,
                    15.0,
                    if full { pal.text_faint } else { pal.accent },
                ))
                .child(
                    div()
                        .text_size(px(8.0))
                        .text_color(pal.text_faint)
                        .child("添加"),
                ),
        );
        row.into_any_element()
    }

    fn eq_inspector_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let selected = self.eq.selected.and_then(|index| {
            self.eq
                .settings
                .bands
                .get(index)
                .map(|band| (index, band.clone()))
        });
        let mut panel = div()
            .w_full()
            .rounded(px(16.0))
            .bg(pal.panel)
            .border_1()
            .border_color(pal.border_strong)
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(12.0));

        match selected {
            Some((index, band)) => {
                panel = panel.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex_1()
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
                                                .text_size(px(9.0))
                                                .text_color(pal.text_faint)
                                                .child(SharedString::from(format!(
                                                    "频段 {:02} / {:02}",
                                                    index + 1,
                                                    self.eq.settings.bands.len()
                                                ))),
                                        )
                                        .child({
                                            let weak_toggle = weak.clone();
                                            let enabled = band.enabled;
                                            div()
                                                .id("eq-band-enabled-compact")
                                                .h(px(24.0))
                                                .px(px(8.0))
                                                .rounded(px(12.0))
                                                .bg(if enabled {
                                                    pal.accent_soft
                                                } else {
                                                    pal.chip
                                                })
                                                .flex()
                                                .items_center()
                                                .cursor_pointer()
                                                .text_size(px(9.0))
                                                .text_color(if enabled {
                                                    pal.accent
                                                } else {
                                                    pal.text_faint
                                                })
                                                .child(if enabled { "启用" } else { "旁路" })
                                                .on_click(move |_, _, cx| {
                                                    let _ = weak_toggle.update(cx, |app, cx| {
                                                        if let Some(band) =
                                                            app.eq.settings.bands.get_mut(index)
                                                        {
                                                            band.enabled = !enabled;
                                                            app.eq.touch();
                                                            app.apply_eq(cx);
                                                        }
                                                    });
                                                })
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_baseline()
                                        .gap(px(4.0))
                                        .child(
                                            div()
                                                .text_size(px(22.0))
                                                .font_family(pal.mono.clone())
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(pal.text)
                                                .child(SharedString::from(fmt_freq(band.freq_hz))),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(9.0))
                                                .text_color(pal.text_faint)
                                                .child("Hz"),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(13.0))
                                                .font_family(pal.mono.clone())
                                                .text_color(pal.accent)
                                                .child(SharedString::from(format!(
                                                    "  {} dB",
                                                    if band.kind.uses_gain() {
                                                        db_num(band.gain_db)
                                                    } else {
                                                        "—".into()
                                                    }
                                                ))),
                                        ),
                                ),
                        )
                        .child({
                            let weak = weak.clone();
                            eq_icon_button(
                                pal,
                                Icon::Close,
                                32.0,
                                "eq-remove-compact",
                                move |_, _, cx| {
                                    let _ = weak.update(cx, |app, cx| {
                                        app.eq.selected = Some(index);
                                        app.remove_eq_band(cx);
                                    });
                                },
                            )
                        }),
                );

                panel = panel.child(self.eq_kind_row_compact(pal, index, band.kind, cx));
                panel = panel.child(self.eq_params_block(pal, index, &band, true, cx));
                panel = panel.child(eq_divider(pal));
            }
            None => {
                panel = panel.child(
                    div()
                        .w_full()
                        .py(px(14.0))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(pal.text_dim)
                                .child("还没有频段"),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(pal.text_faint)
                                .child("点按上方曲线即可添加第一个频段"),
                        ),
                );
            }
        }

        panel = panel.child(self.eq_master_block(pal, true, cx));

        if self.eq.dirty {
            let weak = weak.clone();
            panel = panel.child(
                div()
                    .id("eq-save-compact")
                    .h(px(42.0))
                    .w_full()
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(pal.accent_line)
                    .bg(pal.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(pal.accent)
                    .hover(|s| s.bg(pal.accent_soft.opacity(0.8)))
                    .child(icons::icon(Icon::Plus, 12.0, pal.accent))
                    .child("另存为个人预设")
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.save_eq_preset(cx));
                    }),
            );
        } else {
            panel = panel.child(
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(9.0))
                    .text_color(pal.text_faint)
                    .child("调整会自动保存；需要复用曲线时点「另存为」"),
            );
        }
        panel.into_any_element()
    }

    fn eq_kind_row_compact(
        &self,
        pal: &EqPalette,
        _index: usize,
        active: FilterKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut row = div().w_full().flex().gap(px(5.0));
        for kind in FilterKind::ALL {
            let weak = weak.clone();
            let is_active = active == kind;
            row = row.child(
                div()
                    .id(SharedString::from(format!("eq-kind-c-{}", kind.label())))
                    .h(px(32.0))
                    .flex_1()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(if is_active {
                        pal.accent_line
                    } else {
                        pal.border
                    })
                    .bg(if is_active { pal.accent_soft } else { pal.chip })
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .text_color(if is_active { pal.accent } else { pal.text_dim })
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.set_eq_filter_kind(kind, cx));
                    })
                    .child(kind.label()),
            );
        }
        row.into_any_element()
    }

    // ---------- 紧凑：预设库 ----------

    fn eq_library_compact(
        &self,
        pal: &EqPalette,
        entries: Vec<PresetEntry>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, subtitle) = tab_meta(self.eq.tab);
        let filtered: Vec<PresetEntry> = entries;

        let mut body = div().w_full().flex().flex_col().gap(px(8.0)).child(
            div()
                .pt(px(2.0))
                .flex()
                .items_end()
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
                                .text_color(pal.text)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(pal.text_faint)
                                .child(subtitle),
                        ),
                )
                .child(
                    div()
                        .text_size(px(9.5))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .child(SharedString::from(format!("{} 项", filtered.len()))),
                ),
        );

        if filtered.is_empty() {
            body = body.child(
                div()
                    .w_full()
                    .h(px(140.0))
                    .rounded(px(12.0))
                    .bg(pal.panel)
                    .border_1()
                    .border_color(pal.border)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(5.0))
                    .child(icons::icon(Icon::Headphones, 20.0, pal.text_faint))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(pal.text_dim)
                            .child("没有匹配的配置"),
                    ),
            );
        }

        for (index, entry) in filtered.iter().enumerate() {
            let weak = weak.clone();
            let entry = *entry;
            let active = !self.eq.dirty && self.eq.preset_name == entry.name;
            body = body.child(eq_preset_row(
                pal,
                entry.name,
                entry.subtitle(),
                Some(Icon::Equalizer),
                None,
                active,
                false,
                false,
                SharedString::from(format!("eq-preset-c-{index}")),
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.load_eq_preset(entry, cx);
                        app.show_portrait_toast(format!("已应用「{}」", entry.name), cx);
                        cx.notify();
                    });
                },
            ));
        }
        body.into_any_element()
    }

    // ---------- 紧凑：AutoEQ 耳机档案 ----------

    fn eq_autoeq_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let (title, _subtitle) = tab_meta(EqTab::AutoEq);
        let lib = &self.eq.autoeq;
        let entries = lib.entries.clone();
        let total = entries.len();
        let filtered = lib.visible_indices(&self.eq.category, &self.eq.search);
        let filtered_count = filtered.len();
        let active_name = self.eq.preset_name.clone();
        let dirty = self.eq.dirty;
        let loading = lib.index_loading || lib.status == AutoEqStatus::Loading;
        let mut body = div().w_full().flex().flex_col().gap(px(8.0)).child(
            div()
                .pt(px(2.0))
                .flex()
                .items_end()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .text_size(px(17.0))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(pal.text)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(pal.text_faint)
                                .text_ellipsis()
                                .child(SharedString::from(autoeq_status_text(lib, filtered_count))),
                        ),
                )
                .child({
                    let weak_sync = weak.clone();
                    eq_sync_chip(pal, "eq-autoeq-sync-c", lib.index_loading, move |_, _, cx| {
                        let _ = weak_sync.update(cx, |app, cx| app.sync_autoeq(true, cx));
                    })
                }),
        );

        body = body.child(eq_search_box(pal, self.eq_search_view.clone()));
        body = body.child({
            let mut chips = div()
                .id("eq-autoeq-chips")
                .flex()
                .items_center()
                .gap(px(6.0))
                .overflow_x_scroll()
                .when(self.smooth_scroll, |d| d.smooth_scroll());
            for label in ["全部", "头戴式", "入耳式", "耳塞式"] {
                let weak = weak.clone();
                let active = self.eq.category == label;
                chips = chips.child(eq_chip(
                    pal,
                    label,
                    active,
                    SharedString::from(format!("eq-cat-c-{label}")),
                    move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.eq.category = label.to_string();
                            cx.notify();
                        });
                    },
                ));
            }
            chips.into_any_element()
        });

        if let Some(error) = &self.eq.autoeq.error
            && total > 0
        {
            body = body.child(
                div()
                    .text_size(px(9.0))
                    .text_color(pal.danger)
                    .text_ellipsis()
                    .child(SharedString::from(format!("同步失败：{error}"))),
            );
        }

        if total == 0 {
            if loading {
                return body
                    .child(eq_notice(
                        pal,
                        Icon::Refresh,
                        "正在从 AutoEQ 仓库拉取…",
                        "索引约 500 KB，首次加载需要几秒；完成后会缓存到本地。",
                        None,
                    ))
                    .into_any_element();
            }
            let hint = lib.error.clone().unwrap_or_else(|| {
                "从 github.com/jaakkopasanen/AutoEq 拉取 6000+ 款耳机的校准曲线，之后离线可用。"
                    .to_string()
            });
            let weak_retry = weak.clone();
            let action: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
                let _ = weak_retry.update(cx, |app, cx| app.sync_autoeq(true, cx));
            });
            return body
                .child(eq_notice(
                    pal,
                    Icon::Cloud,
                    if lib.error.is_some() {
                        "AutoEQ 索引拉取失败"
                    } else {
                        "尚未拉取 AutoEQ 耳机档案"
                    },
                    &hint,
                    Some(("拉取耳机档案", action)),
                ))
                .into_any_element();
        }

        if filtered.is_empty() {
            return body
                .child(eq_notice(
                    pal,
                    Icon::Headphones,
                    "没有匹配的耳机",
                    "换个分类，或输入型号关键词后按回车搜索。",
                    None,
                ))
                .into_any_element();
        }

        if filtered_count > COMPACT_AUTOEQ_ROWS {
            body = body.child(
                div()
                    .text_size(px(9.0))
                    .text_color(pal.text_faint)
                    .text_center()
                    .child(SharedString::from(format!(
                        "共 {filtered_count} 项，仅显示前 {COMPACT_AUTOEQ_ROWS} 项；输入关键词后按回车缩小范围"
                    ))),
            );
        }

        for &index in filtered.iter().take(COMPACT_AUTOEQ_ROWS) {
            let entry = &entries[index];
            let weak_row = weak.clone();
            let file = entry.file.clone();
            let active = !dirty && active_name == entry.name;
            let previewed = self.eq.autoeq_preview.as_deref() == Some(entry.file.as_str());
            let badge = if entry.loading {
                Some("拉取中".to_string())
            } else if entry.error.is_some() {
                Some("失败".to_string())
            } else {
                entry
                    .bands
                    .as_ref()
                    .map(|bands| format!("{} 段", bands.len()))
            };
            body = body.child(eq_preset_row(
                pal,
                &entry.name,
                entry.subtitle(),
                Some(Icon::Headphones),
                badge,
                active,
                previewed,
                true,
                SharedString::from(format!("eq-autoeq-c-{index}")),
                move |_, _, cx| {
                    let _ = weak_row.update(cx, |app, cx| {
                        app.select_autoeq_entry(file.clone(), true, cx);
                    });
                },
            ));
        }
        body.into_any_element()
    }

    fn eq_saved_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut body = div().w_full().flex().flex_col().gap(px(8.0)).child(
            div()
                .pt(px(2.0))
                .flex()
                .items_end()
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
                                .text_color(pal.text)
                                .child("我的配置"),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(pal.text_faint)
                                .child("「另存为」得到的个人预设"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(9.5))
                        .font_family(pal.mono.clone())
                        .text_color(pal.text_faint)
                        .child(SharedString::from(format!("{} 项", self.eq.saved.len()))),
                ),
        );

        if self.eq.saved.is_empty() {
            body = body.child(
                div()
                    .w_full()
                    .h(px(200.0))
                    .rounded(px(14.0))
                    .bg(pal.panel)
                    .border_1()
                    .border_color(pal.border)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .child(icons::icon(Icon::Person, 22.0, pal.text_faint))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(pal.text_dim)
                            .child("还没有个人配置"),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_faint)
                            .child("在编辑器里调好曲线后点「另存为个人预设」"),
                    ),
            );
            return body.into_any_element();
        }

        for (index, preset) in self.eq.saved.iter().enumerate() {
            let weak_load = weak.clone();
            let weak_remove = weak.clone();
            let name = preset.name.clone();
            let description = format!(
                "{} 个滤波器 · 输入 {}",
                preset.settings.bands.len(),
                fmt_db(preset.settings.input_gain_db)
            );
            let active = !self.eq.dirty && self.eq.preset_name == name;
            body = body.child(
                div()
                    .id(SharedString::from(format!("eq-saved-c-{index}")))
                    .w_full()
                    .h(px(60.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(if active { pal.accent_line } else { pal.border })
                    .bg(if active { pal.selected_bg } else { pal.panel })
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = weak_load.update(cx, |app, cx| {
                            let preset = app.eq.saved.get(index).cloned();
                            if let Some(preset) = preset {
                                app.eq.load_saved(&preset);
                                app.eq.preview_saved(index);
                                app.apply_eq(cx);
                                app.show_portrait_toast(format!("已应用「{}」", preset.name), cx);
                                cx.notify();
                            }
                        });
                    })
                    .child(
                        div()
                            .size(px(32.0))
                            .rounded(px(9.0))
                            .bg(pal.chip)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icons::icon(Icon::Person, 15.0, pal.text_dim)),
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
                                    .text_size(px(12.5))
                                    .text_color(if active { pal.accent } else { pal.text })
                                    .text_ellipsis()
                                    .child(SharedString::from(name)),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(pal.text_faint)
                                    .text_ellipsis()
                                    .child(SharedString::from(description)),
                            ),
                    )
                    .when(active, |d| {
                        d.child(icons::icon(Icon::Check, 13.0, pal.accent))
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("eq-saved-c-remove-{index}")))
                            .size(px(30.0))
                            .rounded(px(8.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(pal.danger_soft))
                            .child(icons::icon(Icon::Close, 13.0, pal.danger))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = weak_remove.update(cx, |app, cx| {
                                    app.remove_saved_preset(index, cx);
                                    app.eq.preview_saved = None;
                                });
                            }),
                    ),
            );
        }
        body.into_any_element()
    }

    fn eq_nav_compact(&self, pal: &EqPalette, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let tabs = [
            EqTab::Edit,
            EqTab::V4,
            EqTab::AutoEq,
            EqTab::Builtin,
            EqTab::Personal,
        ];
        let mut row = div()
            .h(px(60.0))
            .w_full()
            .flex_none()
            .border_t_1()
            .border_color(pal.divider)
            .bg(pal.panel)
            .flex()
            .items_center();
        for tab in tabs {
            let weak = weak.clone();
            let active = self.eq.tab == tab;
            let icon = tab_icon(tab);
            row = row.child(
                div()
                    .id(SharedString::from(format!("eq-nav-{}", tab.label())))
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(2.0))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.select_eq_tab(tab, cx);
                        });
                    })
                    .child(
                        div()
                            .h(px(22.0))
                            .px(px(9.0))
                            .rounded(px(7.0))
                            .bg(if active {
                                pal.accent_soft
                            } else {
                                gpui::transparent_black()
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(match icon {
                                Some(icon) => icons::icon(
                                    icon,
                                    16.0,
                                    if active { pal.accent } else { pal.text_faint },
                                ),
                                None => div().into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(9.0))
                            .text_color(if active { pal.accent } else { pal.text_faint })
                            .child(tab.label()),
                    ),
            );
        }
        row.into_any_element()
    }
}

// ================= 预设行 / 预览 =================

fn eq_preset_row(
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
fn eq_sync_chip(
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
fn eq_notice(
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

fn autoeq_status_text(lib: &AutoEqLibrary, filtered: usize) -> String {
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
            if lib.from_cache { "本地缓存" } else { "已同步" }
        )
    }
}

fn eq_preview_empty(pal: &EqPalette, hint: &'static str) -> AnyElement {
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

fn eq_preview_panel(
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
                    paint_eq_graph(bounds, &paint_settings, None, true, &pal_paint, window);
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
