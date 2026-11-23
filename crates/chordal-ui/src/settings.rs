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

use chordal_netease::{NeteaseMode, SongQualityLevel};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Context, Entity, Hsla, IntoElement, Render,
    SharedString, Window, div, point, px,
};

use crate::config::SourcePreference;
use crate::icons::{self, Icon};
use crate::input::{InputState, InputView};
use crate::theme::{Appearance, Theme};

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
    SetBeatBand(f32, f32),
    /// 鼓点视觉效果倍率（0..=1.5，1.0 = 原始幅度）。
    SetBeatStrength(f32),
    /// 流体背景光栅分辨率除数（1..=20，1 = 原始分辨率）。
    SetBackgroundRenderDivisor(u32),
    /// 动态歌词整行扫光渐变带宽度（em，0.1..=4.0）。
    SetLyricSweepFade(f32),
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
    beat_band: (f32, f32),
    beat_strength: f32,
    background_render_divisor: u32,
    lyric_sweep_fade_em: f32,
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

    // ---- 输入框 ----
    cache_dir_field: TextField,
    download_dir_field: TextField,
    beat_low_field: TextField,
    beat_high_field: TextField,
    beat_strength_field: TextField,
    background_divisor_field: TextField,
    lyric_sweep_field: TextField,
    search_timeout_field: TextField,
    netease_url_field: TextField,
    device_field: TextField,
    add_root_field: TextField,

    notice: Option<Notice>,
}

impl SettingsView {
    pub fn new(
        handler: impl Fn(SettingsAction, &mut Window, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let cfg = crate::config::load_or_default();
        let beat_band =
            chordal_player::normalize_band(cfg.beat_band_low_hz, cfg.beat_band_high_hz);
        let beat_strength = cfg.beat_strength.clamp(0.0, 1.5);
        let lyric_sweep_fade_em = crate::config::clamp_lyric_sweep_fade_em(cfg.lyric_sweep_fade_em);
        let background_render_divisor =
            crate::config::clamp_background_render_divisor(cfg.background_render_divisor);
        let quality = cfg.netease.level.unwrap_or_default();
        let weak = cx.entity().downgrade();

        let cache_dir_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-cache-dir",
                "对象缓存目录",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_cache_dir(text, window, cx));
                },
                cx,
            )
        };
        let download_dir_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-download-dir",
                "下载目录",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_download_dir(text, window, cx));
                },
                cx,
            )
        };
        let beat_low_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-beat-low",
                "4000",
                move |_text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_beat_band(window, cx));
                },
                cx,
            )
        };
        let beat_high_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-beat-high",
                "10000",
                move |_text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_beat_band(window, cx));
                },
                cx,
            )
        };
        let beat_strength_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-beat-strength",
                "50",
                move |text, window, cx| {
                    let _ =
                        weak.update(cx, |view, cx| view.submit_beat_strength(text, window, cx));
                },
                cx,
            )
        };
        let background_divisor_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-background-divisor",
                "4",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.submit_background_divisor(text, window, cx)
                    });
                },
                cx,
            )
        };
        let lyric_sweep_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-lyric-sweep-fade",
                "0.6",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_lyric_sweep(text, window, cx));
                },
                cx,
            )
        };
        let search_timeout_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-search-timeout",
                "6000",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_search_timeout(text, window, cx));
                },
                cx,
            )
        };
        let netease_url_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-netease-url",
                "http://127.0.0.1:3000",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_netease_url(text, window, cx));
                },
                cx,
            )
        };
        let device_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-player-device",
                "留空使用系统默认设备",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_device(text, window, cx));
                },
                cx,
            )
        };
        let add_root_field = {
            let weak = weak.clone();
            TextField::new(
                "settings-add-local-root",
                "添加音乐文件夹路径",
                move |text, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_add_root(text, window, cx));
                },
                cx,
            )
        };

        let view = Self {
            handler: Rc::new(handler),
            tab: initial_tab(),
            appearance: cfg.appearance,
            source_preference: cfg.source_preference,
            smooth_scroll: cfg.smooth_scroll,
            beat_band,
            beat_strength,
            background_render_divisor,
            lyric_sweep_fade_em,
            cache_dir: cfg.cache_dir,
            download_dir: cfg.download_dir,
            local_roots: cfg.local_roots,
            search_timeout_ms: cfg.search_timeout_ms,
            netease_base_url: cfg.netease.base_url,
            netease_mode: cfg.netease.mode,
            netease_quality: quality,
            player_device: cfg.player_device,
            cache_root: PathBuf::new(),
            cache_used: 0,
            cache_limit: cfg.cache_limit_bytes,
            cache_entries: 0,
            download_root: PathBuf::new(),
            eq_name: String::new(),
            eq_bands: 0,
            eq_enabled: false,
            backend: String::new(),
            config_path: crate::config::config_path(),
            cache_dir_field,
            download_dir_field,
            beat_low_field,
            beat_high_field,
            beat_strength_field,
            background_divisor_field,
            lyric_sweep_field,
            search_timeout_field,
            netease_url_field,
            device_field,
            add_root_field,
            notice: None,
        };
        view.write_inputs(cx);
        view
    }

    /// 打开设置时同步配置与运行时统计。
    pub fn sync(&mut self, runtime: SettingsRuntime, cx: &mut Context<Self>) {
        let cfg = crate::config::load_or_default();
        self.appearance = cfg.appearance;
        self.source_preference = cfg.source_preference;
        self.smooth_scroll = cfg.smooth_scroll;
        self.beat_band =
            chordal_player::normalize_band(cfg.beat_band_low_hz, cfg.beat_band_high_hz);
        self.beat_strength = cfg.beat_strength.clamp(0.0, 1.5);
        self.background_render_divisor =
            crate::config::clamp_background_render_divisor(cfg.background_render_divisor);
        self.lyric_sweep_fade_em =
            crate::config::clamp_lyric_sweep_fade_em(cfg.lyric_sweep_fade_em);
        self.cache_dir = cfg.cache_dir.clone();
        self.download_dir = cfg.download_dir.clone();
        self.local_roots = cfg.local_roots.clone();
        self.search_timeout_ms = cfg.search_timeout_ms;
        self.netease_base_url = cfg.netease.base_url.clone();
        self.netease_mode = cfg.netease.mode;
        self.netease_quality = cfg.netease.level.unwrap_or_default();
        self.player_device = cfg.player_device.clone();

        self.cache_root = runtime.cache_root;
        self.cache_used = runtime.cache_used_bytes;
        self.cache_limit = runtime.cache_limit_bytes;
        self.cache_entries = runtime.cache_entries;
        self.download_root = runtime.download_root;
        self.eq_name = runtime.eq_name;
        self.eq_bands = runtime.eq_band_count;
        self.eq_enabled = runtime.eq_enabled;
        self.backend = runtime.backend;

        self.write_inputs(cx);
        self.notice = None;
        cx.notify();
    }

    /// 宿主异步完成后回写提示（清缓存、切换目录等）。
    pub fn set_notice(
        &mut self,
        text: impl Into<SharedString>,
        error: bool,
        cx: &mut Context<Self>,
    ) {
        self.notice = Some(Notice {
            text: text.into(),
            error,
        });
        cx.notify();
    }

    /// 清空缓存后刷新占用条。
    pub fn update_cache(
        &mut self,
        used: u64,
        limit: u64,
        entries: u64,
        cx: &mut Context<Self>,
    ) {
        self.cache_used = used;
        self.cache_limit = limit;
        self.cache_entries = entries;
        cx.notify();
    }

    fn write_inputs(&self, cx: &mut Context<Self>) {
        self.cache_dir_field
            .set(self.cache_dir.display().to_string(), cx);
        self.download_dir_field
            .set(self.download_dir.display().to_string(), cx);
        self.beat_low_field
            .set(format!("{:.0}", self.beat_band.0), cx);
        self.beat_high_field
            .set(format!("{:.0}", self.beat_band.1), cx);
        self.beat_strength_field
            .set(format!("{:.0}", self.beat_strength * 100.0), cx);
        self.background_divisor_field
            .set(self.background_render_divisor.to_string(), cx);
        self.lyric_sweep_field
            .set(format!("{:.2}", self.lyric_sweep_fade_em), cx);
        self.search_timeout_field
            .set(self.search_timeout_ms.to_string(), cx);
        self.netease_url_field
            .set(self.netease_base_url.clone(), cx);
        self.device_field
            .set(self.player_device.clone().unwrap_or_default(), cx);
    }

    fn set_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    fn emit(&self, action: SettingsAction, window: &mut Window, cx: &mut Context<Self>) {
        (self.handler)(action, window, cx);
    }

    fn notice_ok(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: false,
        });
        cx.notify();
    }

    fn notice_error(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(Notice {
            text: text.into(),
            error: true,
        });
        cx.notify();
    }

    // ---------- 输入提交 ----------

    fn submit_cache_dir(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.notice_error("缓存目录不能为空", cx);
            return;
        }
        let path = PathBuf::from(&text);
        self.cache_dir = path.clone();
        self.emit(SettingsAction::SetCacheDir(path), window, cx);
        self.notice_ok(format!("缓存目录 {text}"), cx);
    }

    fn submit_download_dir(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            self.notice_error("下载目录不能为空", cx);
            return;
        }
        let path = PathBuf::from(&text);
        self.download_dir = path.clone();
        self.emit(SettingsAction::SetDownloadDir(path), window, cx);
        self.notice_ok(format!("下载目录 {text}"), cx);
    }

    fn submit_beat_band(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let low_text = self.beat_low_field.state.read(cx).content().to_string();
        let high_text = self.beat_high_field.state.read(cx).content().to_string();
        let (Some(low), Some(high)) = (parse_hz(&low_text), parse_hz(&high_text)) else {
            self.notice_error("频段格式无效，可填 4000 / 4k / 10khz", cx);
            return;
        };
        let in_range = |v: f32| (20.0..=22_050.0).contains(&v);
        if !in_range(low) || !in_range(high) || high <= low {
            self.notice_error("需满足 20 ≤ 下限 < 上限 ≤ 22050 Hz", cx);
            return;
        }
        let (low, high) = chordal_player::normalize_band(low, high);
        self.beat_band = (low, high);
        self.beat_low_field.set(format!("{low:.0}"), cx);
        self.beat_high_field.set(format!("{high:.0}"), cx);
        self.emit(SettingsAction::SetBeatBand(low, high), window, cx);
        self.notice_ok(format!("背景鼓点频段 {low:.0}–{high:.0} Hz"), cx);
    }

    /// 提交“效果强度”（百分比，0–150）。
    fn submit_beat_strength(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().trim_end_matches('%').trim();
        let Ok(percent) = text.parse::<f32>() else {
            self.notice_error("效果强度需为 0–150 的百分比", cx);
            return;
        };
        if !(0.0..=150.0).contains(&percent) {
            self.notice_error("效果强度范围 0–150%", cx);
            return;
        }
        self.apply_beat_strength(percent / 100.0, window, cx);
    }

    fn set_beat_strength_preset(
        &mut self,
        value: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_beat_strength(value, window, cx);
    }

    fn apply_beat_strength(&mut self, value: f32, window: &mut Window, cx: &mut Context<Self>) {
        let value = value.clamp(0.0, 1.5);
        self.beat_strength = value;
        self.beat_strength_field
            .set(format!("{:.0}", value * 100.0), cx);
        self.emit(SettingsAction::SetBeatStrength(value), window, cx);
        self.notice_ok(format!("背景鼓点强度 {:.0}%", value * 100.0), cx);
    }

    /// 提交流体背景分辨率除数（整数，1–20）。
    fn submit_background_divisor(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim().trim_start_matches('÷').trim();
        let Ok(divisor) = text.parse::<u32>() else {
            self.notice_error("分辨率除数需为 1–20 的整数", cx);
            return;
        };
        if !(crate::config::BACKGROUND_RENDER_DIVISOR_MIN
            ..=crate::config::BACKGROUND_RENDER_DIVISOR_MAX)
            .contains(&divisor)
        {
            self.notice_error("分辨率除数范围 1–20", cx);
            return;
        }
        self.apply_background_divisor(divisor, window, cx);
    }

    fn set_background_divisor_preset(
        &mut self,
        divisor: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_background_divisor(divisor, window, cx);
    }

    fn apply_background_divisor(
        &mut self,
        divisor: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let divisor = crate::config::clamp_background_render_divisor(divisor);
        self.background_render_divisor = divisor;
        self.background_divisor_field.set(divisor.to_string(), cx);
        self.emit(SettingsAction::SetBackgroundRenderDivisor(divisor), window, cx);
        self.notice_ok(format!("流体背景分辨率 1/{divisor}"), cx);
    }

    /// 提交整行扫光渐变带宽度（em，0.1–4.0）。
    fn submit_lyric_sweep(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim().trim_end_matches("em").trim();
        let Ok(value) = text.parse::<f32>() else {
            self.notice_error("扫光宽度需为 0.1–4.0 的数字（单位 em）", cx);
            return;
        };
        if !(crate::config::LYRIC_SWEEP_FADE_EM_MIN..=crate::config::LYRIC_SWEEP_FADE_EM_MAX)
            .contains(&value)
        {
            self.notice_error("扫光宽度范围 0.1–4.0 em", cx);
            return;
        }
        self.apply_lyric_sweep(value, window, cx);
    }

    fn set_lyric_sweep_preset(&mut self, value: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_lyric_sweep(value, window, cx);
    }

    fn apply_lyric_sweep(&mut self, value: f32, window: &mut Window, cx: &mut Context<Self>) {
        let value = crate::config::clamp_lyric_sweep_fade_em(value);
        self.lyric_sweep_fade_em = value;
        self.lyric_sweep_field.set(format!("{value:.2}"), cx);
        self.emit(SettingsAction::SetLyricSweepFade(value), window, cx);
        self.notice_ok(format!("歌词扫光宽度 {value:.2}em"), cx);
    }

    fn submit_search_timeout(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(ms) = text.trim().parse::<u64>() else {
            self.notice_error("搜索超时必须是毫秒数", cx);
            return;
        };
        if !(500..=60_000).contains(&ms) {
            self.notice_error("搜索超时范围 500–60000 ms", cx);
            return;
        }
        self.search_timeout_ms = ms;
        self.emit(SettingsAction::SetSearchTimeout(ms), window, cx);
        self.notice_ok(format!("每源搜索超时 {ms} ms"), cx);
    }

    fn submit_netease_url(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if !(text.starts_with("http://") || text.starts_with("https://")) {
            self.notice_error("服务地址需以 http:// 或 https:// 开头", cx);
            return;
        }
        self.netease_base_url = text.clone();
        self.emit(SettingsAction::SetNeteaseBaseUrl(text), window, cx);
        self.notice_ok("服务地址已保存 · 重启后生效", cx);
    }

    fn submit_device(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        let device = (!text.is_empty()).then(|| text.clone());
        self.player_device = device.clone();
        self.emit(SettingsAction::SetPlayerDevice(device), window, cx);
        self.notice_ok(
            if text.is_empty() {
                "输出设备已恢复系统默认 · 重启后生效".to_string()
            } else {
                format!("输出设备 {text} · 重启后生效")
            },
            cx,
        );
    }

    fn submit_add_root(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let path = PathBuf::from(&text);
        if self.local_roots.iter().any(|root| root == &path) {
            self.notice_error("该目录已在本地音乐库中", cx);
            return;
        }
        self.local_roots.push(path.clone());
        self.emit(SettingsAction::AddLocalRoot(path), window, cx);
        self.add_root_field.set(String::new(), cx);
        self.notice_ok("目录已添加 · 正在重新扫描本地库", cx);
    }

    fn remove_local_root(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.local_roots.len() || self.local_roots.len() <= 1 {
            self.notice_error("至少保留一个音乐目录", cx);
            return;
        }
        self.local_roots.remove(index);
        self.emit(SettingsAction::RemoveLocalRoot(index), window, cx);
        self.notice_ok("目录已移除 · 重新扫描后生效", cx);
    }

    // ================= 渲染 =================

    fn render_sidebar(&self, theme: &Theme, weak: &gpui::WeakEntity<Self>) -> AnyElement {
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

    fn render_content(
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

    fn render_footer(&self, theme: &Theme, pad: f32) -> AnyElement {
        let (text, color, icon) = match &self.notice {
            Some(notice) if notice.error => {
                (notice.text.clone(), theme.danger, Some(Icon::Alert))
            }
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
            .child(
                div()
                    .h(px(1.0))
                    .flex_1()
                    .bg(theme.border),
            )
            .when_some(icon, |d, icon| {
                d.child(icons::icon(icon, 11.0, color))
            })
            .child(div().text_size(px(10.0)).text_color(color).child(text))
            .into_any_element()
    }

    fn render_general(&self, theme: &Theme, weak: &gpui::WeakEntity<Self>) -> AnyElement {
        let theme_choice = {
            let mut items: Vec<(
                &'static str,
                bool,
                Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
            )> = Vec::new();
            for (mode, label) in [
                (Appearance::Light, "浅色"),
                (Appearance::Dark, "深色"),
            ] {
                let weak = weak.clone();
                let active = self.appearance == mode;
                items.push((
                    label,
                    active,
                    Box::new(move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.appearance = mode;
                            view.emit(SettingsAction::SetAppearance(mode), window, cx);
                            cx.notify();
                        });
                    }),
                ));
            }
            segmented(theme, "settings-theme", items)
        };
        let preference = {
            let mut items: Vec<(
                &'static str,
                bool,
                Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
            )> = Vec::new();
            for (pref, label) in [
                (SourcePreference::Auto, "音质优先"),
                (SourcePreference::PreferLocal, "本地优先"),
                (SourcePreference::PreferOnline, "在线优先"),
            ] {
                let weak = weak.clone();
                let active = self.source_preference == pref;
                items.push((
                    label,
                    active,
                    Box::new(move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.source_preference = pref;
                            view.emit(SettingsAction::SetSourcePreference(pref), window, cx);
                            cx.notify();
                        });
                    }),
                ));
            }
            segmented(theme, "settings-source-pref", items)
        };

        let smooth_scroll = {
            let weak = weak.clone();
            toggle(
                theme,
                self.smooth_scroll,
                "settings-smooth-scroll",
                gpui::rgb(0x3B82F6).into(),
                move |on, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.smooth_scroll = on;
                        view.emit(SettingsAction::SetSmoothScroll(on), window, cx);
                        cx.notify();
                    });
                },
            )
        };

        section(
            theme,
            "外观",
            Some("主题即刻切换；两套配色共用同一套布局与字号。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "主题",
                    Some("浅色纸面 / 深色炭黑".into()),
                    theme_choice,
                )],
            )],
        )
        .child(section(
            theme,
            "播放来源",
            Some("本地与在线存在同曲时如何取舍。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "同曲优先",
                    Some("音质优先：本地规格更高才播本地；在线优先：除已下载外始终在线".into()),
                    preference,
                )],
            )],
        ))
        .child(section(
            theme,
            "滚动",
            Some("平滑滚动对滚轮/触控板生效；关闭时保留系统原生手感。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "平滑滚动",
                    Some("时间基缓动（45ms），默认关闭以避免与触控板惯性叠加".into()),
                    smooth_scroll,
                )],
            )],
        ))
        .into_any_element()
    }

    fn render_playback(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let preset = |label: &'static str, low: f32, high: f32| {
            let weak = weak.clone();
            let active = self.beat_band == (low, high);
            chip(theme, label, active, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.beat_band = (low, high);
                    view.beat_low_field.set(format!("{low:.0}"), cx);
                    view.beat_high_field.set(format!("{high:.0}"), cx);
                    view.emit(SettingsAction::SetBeatBand(low, high), window, cx);
                    view.notice_ok(format!("背景鼓点频段 {low:.0}–{high:.0} Hz"), cx);
                });
            })
        };

        let band_controls = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .w(px(92.0))
                    .child(self.beat_low_field.view.clone()),
            )
            .child(hint(theme, "Hz"))
            .child(div().w(px(10.0)))
            .child(
                div()
                    .w(px(92.0))
                    .child(self.beat_high_field.view.clone()),
            )
            .child(hint(theme, "Hz"))
            .into_any_element();

        let preset_row = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(preset("低音 80–2k", 80.0, 2_000.0))
            .child(preset("鼓点 4–10k", 4_000.0, 10_000.0))
            .child(preset("高频 8–16k", 8_000.0, 16_000.0))
            .child(div().flex_1())
            .into_any_element();

        let strength_preset = |label: &'static str, value: f32| {
            let weak = weak.clone();
            let active = (self.beat_strength - value).abs() < 0.001;
            chip(theme, label, active, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.set_beat_strength_preset(value, window, cx);
                });
            })
        };

        let strength_controls = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .w(px(56.0))
                    .child(self.beat_strength_field.view.clone()),
            )
            .child(hint(theme, "%"))
            .child(div().w(px(8.0)))
            .child(strength_preset("轻柔 25%", 0.25))
            .child(strength_preset("标准 50%", 0.5))
            .child(strength_preset("明显 75%", 0.75))
            .child(strength_preset("强烈 100%", 1.0))
            .into_any_element();

        let divisor_preset = |label: &'static str, value: u32| {
            let weak = weak.clone();
            let active = self.background_render_divisor == value;
            chip(theme, label, active, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.set_background_divisor_preset(value, window, cx);
                });
            })
        };

        let divisor_controls = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .w(px(52.0))
                    .child(self.background_divisor_field.view.clone()),
            )
            .child(hint(theme, "N"))
            .child(div().w(px(8.0)))
            .child(divisor_preset("÷1", 1))
            .child(divisor_preset("÷2", 2))
            .child(divisor_preset("÷4", 4))
            .child(divisor_preset("÷8", 8))
            .child(divisor_preset("÷12", 12))
            .into_any_element();

        let sweep_preset = |label: &'static str, value: f32| {
            let weak = weak.clone();
            let active = (self.lyric_sweep_fade_em - value).abs() < 0.001;
            chip(theme, label, active, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.set_lyric_sweep_preset(value, window, cx);
                });
            })
        };

        let sweep_controls = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .w(px(64.0))
                    .child(self.lyric_sweep_field.view.clone()),
            )
            .child(hint(theme, "em"))
            .child(div().w(px(8.0)))
            .child(sweep_preset("0.6", 0.6))
            .child(sweep_preset("1.0", 1.0))
            .child(sweep_preset("1.5", 1.5))
            .child(sweep_preset("2.0", 2.0))
            .into_any_element();

        let eq_switch = {
            let weak = weak.clone();
            toggle(
                theme,
                self.eq_enabled,
                "settings-eq-enabled",
                gpui::rgb(0x3B82F6).into(),
                move |on, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.eq_enabled = on;
                        view.emit(SettingsAction::SetEqEnabled(on), window, cx);
                    });
                },
            )
        };
        let eq_open = {
            let weak = weak.clone();
            small_button(theme, "打开均衡器", true, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.emit(SettingsAction::OpenEqualizer, window, cx);
                });
            })
        };
        let eq_summary = if self.eq_bands == 0 {
            "尚未配置频段".to_string()
        } else {
            format!(
                "{} · {} 个频段 · {}",
                self.eq_name,
                self.eq_bands,
                if self.eq_enabled { "已开启" } else { "已关闭" }
            )
        };

        section(
            theme,
            "背景律动",
            Some("沉浸歌词背景的网格随该频段能量做呼吸/旋转/提亮；默认 4000–10000 Hz、强度 50%。"),
            vec![
                group(
                    theme,
                    vec![
                        setting_row(
                            theme,
                            "监听频段",
                            Some("支持 4000 / 4k / 10khz 写法，回车应用".into()),
                            band_controls,
                        ),
                        setting_row(
                            theme,
                            "效果强度",
                            Some("0–150%：呼吸/旋转/缩放/提亮的整体倍率，回车应用".into()),
                            strength_controls,
                        ),
                        setting_row(
                            theme,
                            "渲染分辨率除数",
                            Some(
                                "光栅分辨率 = 背景区域 ÷ N；N 越大越省性能、越模糊，默认 4，回车应用"
                                    .into(),
                            ),
                            divisor_controls,
                        ),
                    ],
                ),
                preset_row,
            ],
        )
        .child(section(
            theme,
            "歌词",
            Some("动态歌词的整行扫光是一条共享的渐变 mask 前沿；软边越宽，越容易跨过相邻的字/词。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "扫光渐变宽度",
                    Some("单位 em（相对主字号），默认 0.6，范围 0.1–4.0，回车应用".into()),
                    sweep_controls,
                )],
            )],
        ))
        .child(section(
            theme,
            "均衡器",
            Some("参数均衡器的开关与详细调节在独立页面完成。"),
            vec![group(
                theme,
                vec![setting_row(
                    theme,
                    "参数均衡器",
                    Some(eq_summary),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(eq_switch)
                        .child(eq_open)
                        .into_any_element(),
                )],
            )],
        ))
        .child(section(
            theme,
            "音频输出",
            Some("设备名取决于平台：Windows 为 WASAPI 端点 ID，Linux 为 ALSA 设备名。"),
            vec![group(
                theme,
                vec![setting_block(
                    theme,
                    "输出设备",
                    Some("留空使用系统默认设备；修改在下次启动时重建音频后端。".to_string()),
                    badge(theme, "重启生效"),
                    div()
                        .w_full()
                        .child(self.device_field.view.clone())
                        .into_any_element(),
                )],
            )],
        ))
        .into_any_element()
    }

    fn render_sources(
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
                            view.notice_ok(
                                format!("音质档位 {} · 重启后生效", level.label()),
                                cx,
                            );
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
                            let text = view
                                .add_root_field
                                .state
                                .read(cx)
                                .content()
                                .to_string();
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
                vec![div()
                    .w_full()
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .child(roots)
                    .child(div().h(px(6.0)))
                    .child(add_root)
                    .into_any_element()],
            )],
        ))
        .into_any_element()
    }

    fn render_storage(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let ratio = if self.cache_limit == 0 {
            0.0
        } else {
            (self.cache_used as f32 / self.cache_limit as f32).clamp(0.0, 1.0)
        };
        let bar_w = 236.0_f32;
        let usage = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_dim)
                    .child(format!(
                        "已用 {} / {} · {} 项",
                        format_bytes(self.cache_used),
                        format_bytes(self.cache_limit),
                        self.cache_entries
                    )),
            )
            .child(
                div()
                    .w(px(bar_w))
                    .h(px(6.0))
                    .rounded(px(3.0))
                    .bg(theme.border)
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(px((bar_w * ratio).max(if self.cache_used > 0 {
                                2.0
                            } else {
                                0.0
                            })))
                            .rounded(px(3.0))
                            .bg(if ratio > 0.9 {
                                theme.danger
                            } else {
                                theme.text
                            }),
                    ),
            )
            .into_any_element();

        let presets = {
            let weak = weak.clone();
            let mut row = div().flex().items_center().gap(px(6.0));
            for (label, bytes) in [
                ("1 GB", 1024_u64 * 1024 * 1024),
                ("5 GB", 5 * 1024 * 1024 * 1024),
                ("10 GB", 10 * 1024 * 1024 * 1024),
                ("20 GB", 20 * 1024 * 1024 * 1024),
            ] {
                let weak = weak.clone();
                let active = self.cache_limit == bytes;
                row = row.child(chip(
                    theme,
                    label,
                    active,
                    move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.cache_limit = bytes;
                            view.emit(SettingsAction::SetCacheLimit(bytes), window, cx);
                            let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                            view.notice_ok(format!("缓存上限 {gb:.0} GB"), cx);
                        });
                    },
                ));
            }
            row = row
                .child(step_button(theme, "−", {
                    let weak = weak.clone();
                    move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            let next = view
                                .cache_limit
                                .saturating_sub(1024 * 1024 * 1024)
                                .clamp(256 * 1024 * 1024, 100 * 1024 * 1024 * 1024);
                            view.cache_limit = next;
                            view.emit(SettingsAction::SetCacheLimit(next), window, cx);
                        });
                    }
                }))
                .child(step_button(theme, "+", {
                    let weak = weak.clone();
                    move |_, window, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            let next = view
                                .cache_limit
                                .saturating_add(1024 * 1024 * 1024)
                                .clamp(256 * 1024 * 1024, 100 * 1024 * 1024 * 1024);
                            view.cache_limit = next;
                            view.emit(SettingsAction::SetCacheLimit(next), window, cx);
                        });
                    }
                }));
            row.into_any_element()
        };

        let clear = {
            let weak = weak.clone();
            small_button(theme, "清空缓存", false, move |_, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.emit(SettingsAction::ClearCache, window, cx);
                    view.notice_ok("正在清空对象缓存…", cx);
                });
            })
        };

        let clear = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(clear)
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(theme.text_faint)
                    .child("缓存重建后播放不受影响"),
            )
            .into_any_element();

        section(
            theme,
            "对象缓存",
            Some("封面、歌词、详情与直链的磁盘池；超出上限按 LRU 回收。"),
            vec![
                group(
                    theme,
                    vec![
                        setting_row(
                            theme,
                            "占用",
                            Some(format!("实际目录：{}", self.cache_root.display())),
                            usage,
                        ),
                        setting_row(
                            theme,
                            "上限",
                            Some("预设或按 1 GB 步进微调".into()),
                            presets,
                        ),
                        setting_block(
                            theme,
                            "缓存目录",
                            Some("按回车切换；目录不可写时自动回退临时目录".into()),
                            div().into_any_element(),
                            div()
                                .w_full()
                                .child(self.cache_dir_field.view.clone())
                                .into_any_element(),
                        ),
                    ],
                ),
                clear,
            ],
        )
        .child(section(
            theme,
            "下载",
            Some("显式下载的文件不占对象缓存配额，播放时优先使用。"),
            vec![group(
                theme,
                vec![setting_block(
                    theme,
                    "下载目录",
                    Some(format!("实际目录：{}", self.download_root.display())),
                    div().into_any_element(),
                    div()
                        .w_full()
                        .child(self.download_dir_field.view.clone())
                        .into_any_element(),
                )],
            )],
        ))
        .into_any_element()
    }

    fn render_about(&self, theme: &Theme) -> AnyElement {
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

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let viewport = window.viewport_size();
        let vw = f32::from(viewport.width);
        let vh = f32::from(viewport.height);
        // 800×600 卡片需要四周留出 24px；窗口装不下时改成整窗设置页，
        // 导航从左栏变成顶部横向分段，内容宽度跟随窗口。
        let compact = vw < CARD_W + 48.0 || vh < CARD_H + 48.0;
        if compact {
            self.render_phone(&theme, &weak, cx, vw, vh)
                .into_any_element()
        } else {
            self.render_desktop(&theme, &weak, cx)
                .into_any_element()
        }
    }
}

impl SettingsView {
    /// 桌面设置：居中固定卡片，左导航 / 右内容。
    fn render_desktop(
        &self,
        theme: &Theme,
        weak: &gpui::WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sidebar = self.render_sidebar(theme, weak);
        let content = self.render_content(
            theme,
            weak,
            cx,
            Some(CARD_W - NAV_W - 1.0),
            24.0,
        );

        let overlay_weak = weak.clone();
        div()
            .id("settings-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
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
    fn render_phone(
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
    fn render_phone_nav(
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
                    .id(SharedString::from(format!("settings-phone-nav-{}", tab.label())))
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
    items: Vec<(&'static str, bool, Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>)>,
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
