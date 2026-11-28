//! 设置中心（独立实体）。
//!
//! 左侧分页导航 + 右侧内容区，取代原先一整列长滚动：
//! - 纯展示/输入状态由本实体持有，`AppConfig` 的读写统一交给宿主回调；
//! - 需要运行时生效的动作（缓存重定向、主题、均衡器…）通过
//!   [`SettingsAction`] 回传给 `ChordialApp`，宿主只做分发；
//! - 音源地址/音质/输出设备等只能在下次启动时重建的对象，界面上明确
//!   标注「重启生效」，不做假承诺。

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use chordal_core::mem::Gauge;
use chordal_core::{AudioBandKind, AudioBandRanges, AudioControlMapping};
use chordal_netease::{NeteaseMode, SongQualityLevel};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Context, Entity, Hsla, IntoElement, Render,
    SharedString, Window, div, point, px,
};

use crate::config::{AppConfig, SourcePreference};
use crate::icons::{self, Icon};
use crate::input::{InputState, InputView};
use crate::motion::MotionPreset;
use crate::theme::{Appearance, Theme};

// ---- 按职责拆分出的子模块（纯代码搬移） ----
mod actions;
mod render_about;
mod render_general_playback;
mod render_responsive;
mod render_shell;
mod render_sources;
mod render_storage;

const CARD_W: f32 = 800.0;
const CARD_H: f32 = 600.0;
const NAV_W: f32 = 210.0;

// ================= 数据模型 =================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Playback,
    Sources,
    Storage,
    About,
}

impl SettingsTab {
    const ALL: [SettingsTab; 5] = [
        SettingsTab::General,
        SettingsTab::Playback,
        SettingsTab::Sources,
        SettingsTab::Storage,
        SettingsTab::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Playback => "播放",
            Self::Sources => "音源",
            Self::Storage => "存储",
            Self::About => "关于",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Self::General => "外观与默认播放行为",
            Self::Playback => "输出、律动、歌词与均衡器",
            Self::Sources => "网易云、搜索与本地音乐库",
            Self::Storage => "对象缓存与下载目录",
            Self::About => "版本、后端与路径",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::General => Icon::Sliders,
            Self::Playback => Icon::Headphones,
            Self::Sources => Icon::Cloud,
            Self::Storage => Icon::Drive,
            Self::About => Icon::Note,
        }
    }
}

/// 由宿主在打开设置时补齐的运行时快照（缓存统计、均衡器与后端信息）。
#[derive(Debug, Clone, Default)]
pub struct SettingsRuntime {
    pub cache_root: PathBuf,
    pub cache_used_bytes: u64,
    pub cache_limit_bytes: u64,
    pub cache_entries: u64,
    pub download_root: PathBuf,
    pub eq_name: String,
    pub eq_band_count: usize,
    pub eq_enabled: bool,
    pub backend: String,
}

/// 设置面板向宿主发出的动作。
#[derive(Debug, Clone)]
pub enum SettingsAction {
    Close,
    SetAppearance(Appearance),
    SetSourcePreference(SourcePreference),
    SetSmoothScroll(bool),
    /// 一次更新五路独立监听频段。
    SetAudioBandRanges(AudioBandRanges),
    /// 一次更新五路频段到内部控制点的映射。
    SetAudioBandControls(AudioControlMapping),
    /// 鼓点视觉效果倍率（0..=1.5，1.0 = 原始幅度）。
    SetBeatStrength(f32),
    /// 流体背景光栅分辨率除数（1..=20，1 = 原始分辨率）。
    SetBackgroundRenderDivisor(u32),
    /// 动态歌词整行扫光渐变带宽度（em，0.1..=4.0）。
    SetLyricSweepFade(f32),
    /// 统一弹簧预设（紧凑/均衡/柔和/弹性）。
    SetMotionPreset(MotionPreset),
    /// 动画时长倍率（0.25..=3.0，> 1 慢放）。
    SetMotionDuration(f32),
    /// 动画回弹倍率（0.25..=2.0，> 1 回弹更明显）。
    SetMotionOvershoot(f32),
    /// 画面性能诊断浮窗开关。
    SetPerfOverlay(bool),
    SetEqEnabled(bool),
    OpenEqualizer,
    SetCacheDir(PathBuf),
    SetCacheLimit(u64),
    ClearCache,
    SetDownloadDir(PathBuf),
    SetSearchTimeout(u64),
    SetNeteaseMode(NeteaseMode),
    SetNeteaseQuality(SongQualityLevel),
    SetNeteaseBaseUrl(String),
    SetPlayerDevice(Option<String>),
    AddLocalRoot(PathBuf),
    RemoveLocalRoot(usize),
    RescanLibrary,
}

type Handler = Rc<dyn Fn(SettingsAction, &mut Window, &mut App)>;

#[derive(Debug, Clone)]
struct Notice {
    text: SharedString,
    error: bool,
}

/// 一个输入框 = 状态实体 + 视图实体（视图负责 IME / 回车提交）。
struct TextField {
    state: Entity<InputState>,
    view: Entity<InputView>,
}

impl TextField {
    fn new(
        id: &'static str,
        placeholder: &'static str,
        submit: impl Fn(String, &mut Window, &mut App) + 'static,
        cx: &mut Context<SettingsView>,
    ) -> Self {
        let state = cx.new(|cx| {
            let mut state = InputState::new(cx);
            state.set_placeholder(placeholder);
            state
        });
        let view = cx.new(|cx| {
            InputView::new(state.clone(), cx)
                .with_id(id)
                .on_submit(submit)
        });
        Self { state, view }
    }

    fn set(&self, text: impl Into<String>, cx: &mut Context<SettingsView>) {
        self.state
            .update(cx, |state, cx| state.set_content(text, cx));
    }
}

pub struct SettingsView {
    handler: Handler,
    tab: SettingsTab,

    // ---- 配置快照 ----
    appearance: Appearance,
    source_preference: SourcePreference,
    smooth_scroll: bool,
    audio_band_ranges: AudioBandRanges,
    audio_band_controls: AudioControlMapping,
    beat_strength: f32,
    background_render_divisor: u32,
    lyric_sweep_fade_em: f32,
    motion_preset: MotionPreset,
    animation_duration_multiplier: f32,
    animation_overshoot_multiplier: f32,
    perf_overlay: bool,
    cache_dir: PathBuf,
    download_dir: PathBuf,
    local_roots: Vec<PathBuf>,
    search_timeout_ms: u64,
    netease_base_url: String,
    netease_mode: NeteaseMode,
    netease_quality: SongQualityLevel,
    player_device: Option<String>,

    // ---- 运行时快照 ----
    cache_root: PathBuf,
    cache_used: u64,
    cache_limit: u64,
    cache_entries: u64,
    download_root: PathBuf,
    eq_name: String,
    eq_bands: usize,
    eq_enabled: bool,
    backend: String,
    config_path: PathBuf,
    /// 最近一次内存快照；由宿主在设置页打开时每秒刷新。
    mem: chordal_core::mem::MemSnapshot,

    // ---- 输入框 ----
    cache_dir_field: TextField,
    download_dir_field: TextField,
    audio_band_low_fields: Vec<TextField>,
    audio_band_high_fields: Vec<TextField>,
    audio_band_control_fields: Vec<TextField>,
    beat_strength_field: TextField,
    background_divisor_field: TextField,
    lyric_sweep_field: TextField,
    motion_duration_field: TextField,
    motion_overshoot_field: TextField,
    search_timeout_field: TextField,
    netease_url_field: TextField,
    device_field: TextField,
    add_root_field: TextField,

    notice: Option<Notice>,
}

impl SettingsView {
    // ================= 渲染 =================
}
impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let start = Instant::now();
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let viewport = window.viewport_size();
        let vw = f32::from(viewport.width);
        let vh = f32::from(viewport.height);
        // 800×600 卡片需要四周留出 24px；窗口装不下时改成整窗设置页，
        // 导航从左栏变成顶部横向分段，内容宽度跟随窗口。
        let compact = vw < CARD_W + 48.0 || vh < CARD_H + 48.0;
        let view = if compact {
            self.render_phone(&theme, &weak, cx, vw, vh)
                .into_any_element()
        } else {
            self.render_desktop(&theme, &weak, cx).into_any_element()
        };
        crate::frame_trace::add("settings.total", start.elapsed());
        view
    }
}

impl SettingsView {}
// ================= 基础控件 =================

fn section(
    theme: &Theme,
    title: &'static str,
    desc: Option<&'static str>,
    children: Vec<AnyElement>,
) -> gpui::Div {
    let mut body = div().flex().flex_col().gap(px(10.0));
    for child in children {
        body = body.child(child);
    }
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_dim)
                        .child(title),
                )
                .when_some(desc, |d, desc| {
                    d.child(
                        div()
                            .text_size(px(10.0))
                            .line_height(px(15.0))
                            .text_color(theme.text_faint)
                            .child(desc),
                    )
                }),
        )
        .child(body)
}

fn group(theme: &Theme, rows: Vec<AnyElement>) -> AnyElement {
    let mut col = div()
        .w_full()
        .rounded(px(10.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .px(px(14.0))
        .flex()
        .flex_col();
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            col = col.child(div().w_full().h(px(1.0)).bg(theme.border));
        }
        col = col.child(row);
    }
    col.into_any_element()
}

fn setting_row(
    theme: &Theme,
    title: &'static str,
    hint: Option<String>,
    control: AnyElement,
) -> AnyElement {
    div()
        .w_full()
        .min_h(px(50.0))
        .py(px(8.0))
        .flex()
        .items_center()
        .gap(px(18.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text)
                        .child(title),
                )
                .when_some(hint, |d, hint| {
                    d.child(
                        div()
                            .text_size(px(10.0))
                            .line_height(px(15.0))
                            .text_color(theme.text_faint)
                            .child(hint),
                    )
                }),
        )
        .child(control)
        .into_any_element()
}

fn setting_block(
    theme: &Theme,
    title: &'static str,
    hint: Option<String>,
    badge: AnyElement,
    control: AnyElement,
) -> AnyElement {
    div()
        .w_full()
        .py(px(6.0))
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.text)
                        .child(title),
                )
                .child(badge),
        )
        .when_some(hint, |d, hint| {
            d.child(
                div()
                    .text_size(px(10.0))
                    .line_height(px(15.0))
                    .text_color(theme.text_faint)
                    .child(hint),
            )
        })
        .child(control)
        .into_any_element()
}

fn hint(theme: &Theme, text: &'static str) -> AnyElement {
    div()
        .text_size(px(10.0))
        .text_color(theme.text_faint)
        .child(text)
        .into_any_element()
}

fn badge(theme: &Theme, text: &'static str) -> AnyElement {
    div()
        .h(px(18.0))
        .px(px(6.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(theme.border)
        .flex()
        .items_center()
        .text_size(px(9.0))
        .text_color(theme.text_faint)
        .child(text)
        .into_any_element()
}

fn segmented(
    theme: &Theme,
    id: &'static str,
    items: Vec<(
        &'static str,
        bool,
        Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
    )>,
) -> AnyElement {
    let mut bar = div()
        .id(id)
        .h(px(32.0))
        .p(px(2.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .flex()
        .items_center()
        .gap(px(2.0));
    for (label, active, on_click) in items {
        bar = bar.child(
            div()
                .id(SharedString::from(format!("{id}-{label}")))
                .h_full()
                .px(px(10.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(if active {
                    theme.border
                } else {
                    gpui::transparent_black()
                })
                .bg(if active {
                    theme.surface
                } else {
                    gpui::transparent_black()
                })
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.0))
                .font_weight(if active {
                    gpui::FontWeight::MEDIUM
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(if active { theme.text } else { theme.text_dim })
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover).text_color(theme.text))
                .child(label)
                .on_click(on_click),
        );
    }
    bar.into_any_element()
}

fn chip(
    theme: &Theme,
    label: &'static str,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("settings-chip-{label}")))
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
        .on_click(on_click)
        .into_any_element()
}

fn small_button(
    theme: &Theme,
    label: &'static str,
    primary: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let base = div()
        .id(SharedString::from(format!("settings-button-{label}")))
        .h(px(28.0))
        .px(px(10.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .cursor_pointer();
    let base = if primary {
        base.bg(theme.text)
            .text_color(theme.on_accent)
            .hover(|s| s.bg(theme.accent_hover))
    } else {
        base.border_1()
            .border_color(theme.border)
            .text_color(theme.text)
            .hover(|s| s.bg(theme.hover))
    };
    base.child(label).on_click(on_click).into_any_element()
}

fn step_button(
    theme: &Theme,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("settings-step-{label}")))
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
        .on_click(on_click)
        .into_any_element()
}

fn toggle(
    theme: &Theme,
    on: bool,
    id: &'static str,
    on_color: Hsla,
    on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let knob = if on { 20.0 } else { 2.0 };
    div()
        .id(SharedString::from(format!("settings-toggle-{id}")))
        .w(px(42.0))
        .h(px(22.0))
        .rounded(px(11.0))
        .bg(if on { on_color } else { theme.disabled_bg })
        .border_1()
        .border_color(theme.border)
        .relative()
        .cursor_pointer()
        .on_click(move |_, window, cx| on_change(!on, window, cx))
        .child(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(knob))
                .size(px(18.0))
                .rounded(px(9.0))
                .bg(gpui::white()),
        )
        .into_any_element()
}

/// 诊断 / 截图：`CHORDIAL_SETTINGS_TAB=playback|sources|storage|about`
/// 指定启动时打开的设置分页。
fn initial_tab() -> SettingsTab {
    match std::env::var("CHORDIAL_SETTINGS_TAB").as_deref() {
        Ok("playback") => SettingsTab::Playback,
        Ok("sources") => SettingsTab::Sources,
        Ok("storage") => SettingsTab::Storage,
        Ok("about") => SettingsTab::About,
        _ => SettingsTab::General,
    }
}

// ================= 解析 / 格式化 =================

/// 解析频段输入：`4000` / `4k` / `10khz` / `22,050`。
fn parse_hz(text: &str) -> Option<f32> {
    let cleaned = text.trim().to_ascii_lowercase().replace([',', ' '], "");
    if cleaned.is_empty() {
        return None;
    }
    let (number, scale) = if let Some(rest) = cleaned.strip_suffix("khz") {
        (rest, 1_000.0)
    } else if let Some(rest) = cleaned.strip_suffix("k") {
        (rest, 1_000.0)
    } else if let Some(rest) = cleaned.strip_suffix("hz") {
        (rest, 1.0)
    } else {
        (cleaned.as_str(), 1.0)
    };
    number
        .parse::<f32>()
        .ok()
        .map(|value| value * scale)
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.1} GB", value / GIB)
    } else if value >= MIB {
        format!("{:.0} MB", value / MIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, parse_hz};

    #[test]
    fn parses_hz_variants() {
        assert_eq!(parse_hz("4000"), Some(4_000.0));
        assert_eq!(parse_hz(" 4k "), Some(4_000.0));
        assert_eq!(parse_hz("10kHz"), Some(10_000.0));
        assert_eq!(parse_hz("22,050"), Some(22_050.0));
        assert_eq!(parse_hz("10000hz"), Some(10_000.0));
        assert_eq!(parse_hz(""), None);
        assert_eq!(parse_hz("abc"), None);
        assert_eq!(parse_hz("0"), None);
    }

    #[test]
    fn formats_cache_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5 MB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }
}
