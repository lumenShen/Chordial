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

// ---- 按职责拆分出的子模块（纯代码搬移） ----
mod compact_panels;
mod compact_shell;
mod controls;
mod desktop_graph;
mod desktop_library;
mod desktop_shell;
mod drawing;

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

    // ---------- 桌面：预设库 ----------

    // ---------- 紧凑：触控主界面 ----------
}
