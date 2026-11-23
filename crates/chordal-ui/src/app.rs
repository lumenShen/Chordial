//! 根视图：源面板 / 搜索 / 本地库 / 播放条。
//!
//! 渲染性能纪律：
//! - 列表一律 [`uniform_list`] 虚拟化（只渲染可视行）；
//! - 结果集以 `Arc<Vec<Track>>` 快照持有，render 期间零深拷贝；
//! - 行回调通过 `WeakEntity` 捕获，行元素按 index 建 id；
//! - 播放状态 200ms 轮询 + 变化才 `cx.notify()`；
//! - 渲染 pass 带 `ui.render.pass` 检查点（300Hz 预算 3ms，超预算告警）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chordal_cache::ObjectCache;
use chordal_core::model::{
    AccountState, CollectionId, CollectionKind, CollectionSummary, LoginChallenge, LoginMethod,
    LoginProgress, SearchKind, SearchQuery, SourceId, StreamHandle, Track, TrackOrigin,
};
use chordal_core::perf::PerfScope;
use chordal_core::registry::{SourceRegistry, SourceSlotSnapshot, SourceStatus};
use chordal_core::source::MusicSource;
use chordal_player::{
    EQ_MAX_BANDS, EQ_MAX_GAIN_DB, EQ_MIN_GAIN_DB, EqBand, Event as PlayerEvent, FilterKind, Player,
    PlayerState,
};
use chordial_amll_gpui::background::BackgroundRendererKind;
use chordial_amll_gpui::render::{LyricPlayerView, LyricRenderColors, PlaybackClock};
use chordial_amll_gpui::{AlignAnchor, LyricPlayerConfig, LyricTextStyle};
use gpui::StatefulInteractiveElement;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, BoxShadow, Context, CursorStyle, Entity, FocusHandle, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, RenderImage, ResizeEdge, SharedString, TextRun, Window, div,
    linear_color_stop, linear_gradient, point, px, size, uniform_list,
};

use crate::actions::{
    CloseSettings, ExitImmersive, FocusSearch, NextTrack, OpenSettings, PlayFirst, PlayPause,
    PrevTrack, Quit, RescanLibrary, ToggleEqualizer, TogglePortrait, ToggleSidebar,
};
use crate::autoeq::{self, AutoEqStatus};
use crate::config::SourcePreference;
use crate::downloads::{DownloadExtras, DownloadStore, lyrics_to_lrc};
use crate::eq::{EqState, EqTab, PresetEntry};
use crate::input::{InputState, InputView, focus_input};
use crate::layout_morph::{
    COMPACT_COVER_GAP, COMPACT_TOP, INFO_LINE_RATIO, LayoutMotion, StageGeometry, StageMode,
    StageRect, compact_content_w, compact_cover_size, dock_geometry, dock_rect,
    immersive_geometry, wide_column_x, wide_cover_size, wide_cover_top,
};
use crate::lyrics::{cover_url, decode_data_url_image, fetch_cover_blocking, to_amll_lyrics};
use crate::main_state::Globals;
use crate::player_page::{PortraitPage, Transition};
use crate::responsive::{MIN_WIDE_SPLIT_W, ViewportClass};
use crate::settings::{SettingsAction, SettingsRuntime, SettingsView};
use crate::sheet_drag::{SheetDrag, SheetDragOutcome, SpringSettle};
use crate::theme::{Appearance, Theme, fmt_ms};

/// 线性混合两个颜色（RGB + alpha）；用于 Dock↔沉浸页收尾阶段的
/// 背景/文字配色过渡（黑底白字 → Dock 的玻璃底与主题文字）。
fn mix_color(a: gpui::Hsla, b: gpui::Hsla, t: f32) -> gpui::Hsla {
    let t = t.clamp(0.0, 1.0);
    let a: gpui::Rgba = a.into();
    let b: gpui::Rgba = b.into();
    gpui::Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
    .into()
}

/// 把无背景的玻璃色预乘到主背景上，得到不透明等效色——卡片落座时
/// 用它盖住真实 Dock 的内容，避免两层文字/图标重叠。
fn overlay_color(bg: gpui::Hsla, fg: gpui::Hsla) -> gpui::Hsla {
    let bg: gpui::Rgba = bg.into();
    let fg: gpui::Rgba = fg.into();
    let a = fg.a.clamp(0.0, 1.0);
    gpui::Rgba {
        r: fg.r * a + bg.r * (1.0 - a),
        g: fg.g * a + bg.g * (1.0 - a),
        b: fg.b * a + bg.b * (1.0 - a),
        a: 1.0,
    }
    .into()
}

/// 量一行文字的宽度（与 gpui 默认 `.SystemUIFont` 的排版一致），
/// 用于判断 AMLL TextMarquee 是否需要滚动以及滚动距离。
fn measure_text_width(
    window: &mut Window,
    text: &str,
    font_size: f32,
    weight: gpui::FontWeight,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let mut font = gpui::font(".SystemUIFont");
    font.weight = weight;
    let run = TextRun {
        len: text.len(),
        font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(text.to_string()),
        px(font_size),
        std::slice::from_ref(&run),
        None,
    );
    f32::from(line.width)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewTab {
    Search,
    Library,
    Collection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueList {
    Search,
    Library,
    Queue,
    Collection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LibraryFilter {
    All,
    Queue,
}

/// 底部「更多」抽屉的二级页。抽屉本身不改变播放页布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawerPage {
    Root,
    Sleep,
}

/// 手机壳（单列）当前页面。宽屏没有这个概念：侧栏/曲目板/右栏同时存在。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhonePage {
    /// 播放页：封面 + 曲目信息 + 进度 + 传输 + 音量。
    Player,
    /// 歌词页：封面驱动的 Mesh + 全屏滚动歌词。
    Lyrics,
    /// 曲库页：复用桌面曲目板（本地库 / 队列 / 搜索结果 / 集合）。
    Browse,
}

/// 手机壳与 `lyric_view` 的共享状态。
///
/// 播放页把歌词视图当背景（隐藏文字）；歌词页显示文字。两者共用同一个
/// 实体，必须显式区分，不能只用一个 bool 表示“占用”。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhoneBackdrop {
    Off,
    Player,
    Lyrics,
}

/// 收窄到手机宽度时默认落在哪一页。
///
/// 用户正在搜索/看集合/看队列时，保留他的浏览上下文；只有在普通曲库
/// 或播放中时才落到播放页——否则“拉窄窗口”会把正在找歌的人踢回播放器。
fn phone_default_page(
    view_tab: ViewTab,
    filter: LibraryFilter,
    has_track: bool,
) -> PhonePage {
    let browsing = match view_tab {
        ViewTab::Search | ViewTab::Collection => true,
        ViewTab::Library => filter == LibraryFilter::Queue,
    };
    if browsing || !has_track {
        PhonePage::Browse
    } else {
        PhonePage::Player
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayMode {
    Sequential,
    /// 列表循环：播到队尾回绕到第一首。
    RepeatAll,
    Shuffle,
    RepeatOne,
}

impl PlayMode {
    fn full_label(self) -> &'static str {
        match self {
            Self::Sequential => "顺序播放模式",
            Self::RepeatAll => "列表循环播放模式",
            Self::Shuffle => "随机播放模式",
            Self::RepeatOne => "单曲循环模式",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Sequential => Self::RepeatAll,
            Self::RepeatAll => Self::RepeatOne,
            Self::RepeatOne => Self::Shuffle,
            Self::Shuffle => Self::Sequential,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginTab {
    Qr,
    Phone,
}

/// AMLL `TextMarquee` 的悬停滚动状态。
#[derive(Clone, Copy)]
struct InfoMarquee {
    /// 0 = 曲名，1 = 艺人。
    line: u8,
    start: Instant,
    /// 需要滚动的距离（px）。
    distance: f32,
    /// 单程时长（ms）；AMLL 的公式是 distance*2/64*1000。
    duration_ms: f32,
}

impl InfoMarquee {
    fn active(self, now: Instant) -> bool {
        now.saturating_duration_since(self.start).as_secs_f32() * 1000.0
            < self.duration_ms * 2.0
    }

    fn offset(self, now: Instant) -> f32 {
        let dt = now.saturating_duration_since(self.start).as_secs_f32() * 1000.0;
        let t = dt / self.duration_ms.max(1.0);
        if t >= 2.0 {
            0.0
        } else if t <= 1.0 {
            self.distance * t
        } else {
            self.distance * (2.0 - t)
        }
    }
}

/// 右栏歌词预览的一行（已压平成纯文本 + 译文）。
#[derive(Clone)]
struct PreviewLyric {
    start_ms: u64,
    text: SharedString,
    translation: SharedString,
}

/// 当前打开的集合页（歌单 / 专辑 / 歌手）。
#[derive(Clone)]
struct CollectionView {
    id: CollectionId,
    title: String,
    subtitle: String,
    creator: Option<String>,
    description: Option<String>,
    tracks: Arc<Vec<Track>>,
    related_albums: Arc<Vec<CollectionSummary>>,
    loading: bool,
    error: Option<String>,
}

struct PlayerClock(Arc<Player>);

impl PlaybackClock for PlayerClock {
    fn position_ms(&self) -> u64 {
        self.0.clock().0
    }

    fn is_playing(&self) -> bool {
        self.0.clock().1
    }

    fn snapshot(&self) -> (u64, bool) {
        self.0.clock()
    }
}

/// 曲目行的预计算显示文本。列表快照变化时一次性构建，render 期间只做
/// `SharedString` 克隆（引用计数），不再 `format!`/`join`/`to_string`。
struct TrackRowText {
    id: chordal_core::model::TrackId,
    number: SharedString,
    title: SharedString,
    subtitle_search: SharedString,
    subtitle_default: SharedString,
    duration: SharedString,
}

impl TrackRowText {
    fn new(index: usize, track: &Track) -> Self {
        let artists = track
            .artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        let album = track.album.as_ref().map(|a| a.name.as_str()).unwrap_or("");
        let source_label = match &track.origin {
            TrackOrigin::Local { .. } => "本地",
            TrackOrigin::Remote { source } => match source.0.as_str() {
                "netease" => "网易云",
                other => other,
            },
        };
        let subtitle_search = if artists.is_empty() {
            source_label.to_string()
        } else {
            format!("{artists}  ·  {source_label}")
        };
        let subtitle_default = if !artists.is_empty() && !album.is_empty() {
            format!("{artists}  ·  {album}")
        } else if !artists.is_empty() {
            artists
        } else {
            album.to_string()
        };
        let duration = track
            .duration_ms
            .map(fmt_ms)
            .unwrap_or_else(|| "--:--".into());
        Self {
            id: track.id.clone(),
            number: SharedString::from((index + 1).to_string()),
            title: SharedString::from(track.title.clone()),
            subtitle_search: SharedString::from(subtitle_search),
            subtitle_default: SharedString::from(subtitle_default),
            duration: SharedString::from(duration),
        }
    }
}

/// 「正在播放」面板的预计算文本，曲目变化时才重建。
struct NowPlayingText {
    title: SharedString,
    artists: SharedString,
    album: SharedString,
    source_label: SharedString,
    quality: SharedString,
}

impl NowPlayingText {
    fn new(track: Option<&Track>, fallback_title: Option<&str>) -> Self {
        let title = track
            .map(|t| t.title.as_str())
            .or(fallback_title)
            .unwrap_or("未在播放");
        let artists = track
            .map(|t| {
                t.artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        let album = track
            .and_then(|t| t.album.as_ref())
            .map(|a| a.name.as_str())
            .unwrap_or("");
        let source_label = track
            .map(|t| match &t.origin {
                TrackOrigin::Local { .. } => "本地",
                TrackOrigin::Remote { source } => match source.0.as_str() {
                    "netease" => "网易云",
                    other => other,
                },
            })
            .unwrap_or("");
        let quality = track.and_then(|t| t.quality.as_deref()).unwrap_or("");
        Self {
            title: SharedString::from(title.to_string()),
            artists: SharedString::from(artists),
            album: SharedString::from(album.to_string()),
            source_label: SharedString::from(source_label.to_string()),
            quality: SharedString::from(quality.to_string()),
        }
    }
}

/// 侧栏「我的歌单」行的预计算文本/ID。
struct PlaylistRowText {
    id: CollectionId,
    element_id: SharedString,
    title: SharedString,
    count: SharedString,
}

impl PlaylistRowText {
    fn new(playlist: &CollectionSummary) -> Self {
        Self {
            id: playlist.id.clone(),
            element_id: SharedString::from(format!("user-playlist-{}", playlist.id.id)),
            title: SharedString::from(playlist.title.clone()),
            count: SharedString::from(
                playlist
                    .track_count
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
            ),
        }
    }
}

pub struct ChordialApp {
    registry: Arc<SourceRegistry>,
    player: Arc<Player>,
    local_source: Option<Arc<chordal_local::LocalSource>>,
    search_timeout_ms: u64,
    /// 进程级对象缓存（封面/歌词/详情/直链），设置面板可重定向/限容。
    cache: Arc<ObjectCache>,
    /// 显式下载目录，独立于缓存配额。
    downloads: Arc<DownloadStore>,
    source_preference: SourcePreference,
    /// 平滑滚动开关（配置项，默认 false）。EQ 页面等子模块也读取它。
    pub(crate) smooth_scroll: bool,
    /// 动态歌词整行扫光的渐变带宽度（em）；设置 → 播放可调。
    lyric_sweep_fade_em: f32,
    /// 流体背景光栅分辨率除数（1..=20）：scale = 1 / N；设置 → 播放可调。
    background_render_divisor: u32,
    settings_open: bool,
    /// 新版设置中心（独立实体，见 `settings.rs`）。
    settings_view: Entity<SettingsView>,
    settings_cache_view: Entity<InputView>,
    settings_download_view: Entity<InputView>,
    settings_beat_low_input: Entity<InputState>,
    settings_beat_low_view: Entity<InputView>,
    settings_beat_high_input: Entity<InputState>,
    settings_beat_high_view: Entity<InputView>,
    /// 当前背景鼓点监听频段（Hz）；与播放器/配置保持一致。
    beat_band: (f32, f32),

    // ---- 网易云账号 / 集合页 ----
    account_state: AccountState,
    account_avatar: Option<Arc<RenderImage>>,
    user_playlists: Arc<Vec<CollectionSummary>>,
    playlists_loading: bool,
    login_open: bool,
    login_tab: LoginTab,
    login_gen: u64,
    login_challenge: Option<LoginChallenge>,
    login_qr: Option<Arc<RenderImage>>,
    login_status: Option<String>,
    login_phone_input: Entity<InputState>,
    login_phone_view: Entity<InputView>,
    login_captcha_input: Entity<InputState>,
    login_captcha_view: Entity<InputView>,
    captcha_cooldown_until: Option<Instant>,
    collection: Option<CollectionView>,
    collection_cover: Option<Arc<RenderImage>>,
    collection_gen: u64,

    view_tab: ViewTab,
    sidebar_open: bool,
    search_input: Entity<InputState>,
    search_input_view: Entity<InputView>,
    search_kind: SearchKind,
    search_collections: Arc<Vec<CollectionSummary>>,
    search_gen: u64,
    searching: bool,
    search_tracks: Arc<Vec<Track>>,
    search_reports: Vec<chordal_core::registry::SourceSearchReport>,
    search_elapsed_ms: u64,
    /// 搜索路由：None = 全部音源；Some = 只看该源结果。
    source_route: Option<SourceId>,

    library: Arc<Vec<Track>>,
    library_scanning: bool,
    library_filter: LibraryFilter,
    play_mode: PlayMode,
    /// 关闭随机播放时恢复的循环模式（AMLL 的 shuffle 与 repeat 是两个
    /// 独立按钮；Chordial 用单字段建模时靠它记住上一次的 repeat 档位）。
    last_repeat_mode: PlayMode,

    queue: Arc<Vec<Track>>,
    queue_pos: Option<usize>,
    player_status: chordal_player::Status,
    status_message: Option<String>,
    /// 非错误提示（下载完成、音质选择说明等）。
    info_message: Option<String>,
    pub(crate) focus_handle: FocusHandle,

    // ---- 歌词页 ----
    lyric_view: Entity<LyricPlayerView>,
    current_track: Option<Track>,
    lyrics_gen: u64,
    lyrics_status: Option<String>,
    /// 播放请求代号：网络流可能乱序返回，旧请求不得覆盖新曲目。
    play_gen: u64,
    appearance: Appearance,
    cover_image: Option<Arc<RenderImage>>,
    /// 已被替换/清空的图片。`RenderImage` 上传到 gpui sprite atlas 后不会随
    /// `Arc` 释放自动移除；这里收集起来，在下一帧 `render` 里调用
    /// `App::drop_image`，否则快速切歌会把图集（只增不减）撑爆并黑屏。
    retired_images: Vec<Arc<RenderImage>>,
    /// 当前曲目歌词压平后的预览行（右栏用，异步加载完成后整体替换）。
    lyric_preview: Arc<Vec<PreviewLyric>>,
    activated: bool,
    /// 当前客户区尺寸；用于列表/播放条/封面断点。
    pub(crate) viewport_width: f32,
    pub(crate) viewport_height: f32,
    /// 当前断点。跨断点时只跑一次状态迁移（关闭抽屉、重置背景等）。
    viewport_class: ViewportClass,
    /// 手机壳当前页；宽屏时保留但不参与渲染。
    phone_page: PhonePage,
    /// 手机播放页/歌词页与 `lyric_view` 的共享状态。
    /// 与沉浸层共用同一个 `lyric_view`，必须显式跟踪才能正确交还样式。
    phone_backdrop: PhoneBackdrop,
    /// 宽屏布局下缓存的歌词对齐比例（封面中心占比）。
    lyric_align: f32,
    /// 诊断开关：禁用歌词模糊（CHORDIAL_NO_BLUR=1，性能隔离用）。
    no_blur: bool,
    /// 诊断开关：禁用沉浸背景（CHORDIAL_NO_BG=1，性能隔离用）。
    no_bg: bool,
    immersive: bool,
    /// 沉浸页顶部拖拽条的下拉关闭手势；None = 当前没有手势。
    sheet_drag: Option<SheetDrag>,
    /// 进入沉浸层时 Dock→全屏共享布局的展开弹簧。
    immersive_expand: Option<SpringSettle>,

    // ---- 沉浸层控件动画（AMLL MediaButton / VolumeControl 等价） ----
    /// 进入沉浸页的时刻：所有 MediaButton 播放 pressed 回弹，上一首/下一首
    /// 播放 AMLL 的双箭头合并入场动画。
    ui_anim_enter: Option<Instant>,
    /// 播放状态切换（轮询观察到）时，播放/暂停图标重新挂载的 pressed 回弹。
    ui_anim_play: Option<Instant>,
    ui_anim_shuffle: Option<Instant>,
    ui_anim_repeat: Option<Instant>,
    /// 音量滑杆到达 0 / 1 时喇叭弹跳；bool = 右侧（高音量）。
    ui_anim_volume: Option<(Instant, bool)>,
    /// 上一次音量输入值（用于检测跨越 0/1 边界）。
    last_volume_input: f32,
    /// AMLL 进度条右侧时间标签：true 显示剩余（默认），false 显示总时长。
    show_remaining_time: bool,
    /// 音质标签点击后的解码信息弹窗。
    quality_dialog_open: bool,
    /// 封面暂停缩放：AMLL Cover 在暂停时以 0.75 倍显示。
    /// (开始时刻, 起始缩放, 目标缩放, 时长秒)。
    cover_scale_anim: Option<(Instant, f32, f32, f32)>,
    /// 诊断：持续请求动画帧（CHORDIAL_BENCH_ANIM=1），用于 300Hz 压测。
    bench_anim: bool,

    // ---- 竖屏播放页 / 沉浸层子页面 ----
    /// true = 强制竖屏布局（即使窗口是横向）；false = 按窗口宽高自动选择。
    pub(crate) force_portrait: bool,
    portrait_page: PortraitPage,
    portrait_transition: Option<Transition>,
    portrait_enter: Option<Instant>,
    portrait_last_input: Instant,
    portrait_chrome_hidden: bool,
    portrait_chrome_at: Option<Instant>,
    portrait_idle_ticker: bool,
    portrait_toast: Option<(String, Instant)>,
    /// 底部「更多」抽屉：打开状态、动画起点、当前二级页。
    /// 布局 1 / 布局 2 共用同一只抽屉与同一组入口。
    drawer_open: bool,
    drawer_at: Option<Instant>,
    drawer_page: DrawerPage,
    /// 宽屏沉浸层里用户主动关闭歌词的状态；无歌词或关闭歌词时使用布局 2。
    wide_lyrics_hidden: bool,
    /// 曲名/艺人行的悬停滚动（AMLL TextMarquee）。
    info_marquee: Option<InfoMarquee>,
    /// 共享元素（封面 / 曲名 / 艺人）的布局补间。
    /// 竖屏页面切换与宽屏歌词开/关共用同一套 First→Last 几何。
    stage_motion: Option<LayoutMotion>,
    /// 竖屏播放队列的虚拟列表滚动句柄（进入队列时定位当前曲目）。
    queue_scroll: gpui::UniformListScrollHandle,
    /// 搜索路由过滤结果缓存：避免每帧对搜索结果做 O(n) 克隆。
    search_route_cache: RefCell<Option<(Arc<Vec<Track>>, Option<SourceId>, Arc<Vec<Track>>)>>,
    /// 曲目行显示文本缓存：列表快照不变时 render 期间零字符串分配。
    row_text_cache: RefCell<Option<(Arc<Vec<Track>>, Arc<Vec<TrackRowText>>)>>,
    /// 「正在播放」文本缓存。
    now_playing_text: RefCell<Option<(u64, Arc<NowPlayingText>)>>,
    /// 侧栏歌单行缓存。
    playlist_rows: RefCell<Option<(Arc<Vec<CollectionSummary>>, Arc<Vec<PlaylistRowText>>)>>,
    /// 音源状态快照缓存（按 registry.version 失效）。
    source_snapshot: RefCell<Option<(u64, Arc<Vec<SourceSlotSnapshot>>)>>,
    /// 侧栏滚动偏移：参与歌单区保留缓存的版本哈希。
    sidebar_scroll: gpui::ScrollHandle,

    // ---- 睡眠定时 ----
    sleep_minutes: Option<u32>,
    sleep_until: Option<Instant>,
    sleep_gen: u64,

    // ---- 参数均衡器 ----
    eq_open: bool,
    pub(crate) eq_open_at: Option<Instant>,
    pub(crate) eq: EqState,
    /// 持有输入框实体；视图通过 `eq_search_view` 使用。
    #[allow(dead_code)]
    eq_search_input: Entity<InputState>,
    pub(crate) eq_search_view: Entity<InputView>,
    /// 沉浸页进度条：播放线程位置快照 + 快照时刻，用于帧间外推。
    progress_clock: (u64, bool, Instant),
    /// 编辑后 400ms 落盘，拖动滑杆时不会每个事件都写配置。
    eq_save_at: Option<Instant>,
    /// 防抖落盘代号：连续编辑时只允许最新一次定时器落盘。
    eq_save_gen: u64,
}

// Builders 类型别名（main.rs 组装时使用）。
pub type SourceBuilder = Arc<
    dyn Fn(
            &chordal_core::source::SourceInfo,
        ) -> chordal_core::error::CoreResult<Arc<dyn chordal_core::source::MusicSource>>
        + Send
        + Sync,
>;

#[allow(dead_code)]
pub fn builder_map() -> HashMap<SourceId, SourceBuilder> {
    HashMap::new()
}

/// 小窗「正在播放」面板的歌词样式。
///
/// 构造、退出沉浸层、切换主题都必须用同一份样式；否则面板歌词的字重/
/// 行高/padding 会在这些路径上被重置成 `Default`，视觉上像换了一套字体。
fn compact_lyric_style(theme: &Theme, sweep_fade_em: f32) -> LyricTextStyle {
    LyricTextStyle {
        font_family: theme.font_family.clone(),
        font_weight: gpui::FontWeight::MEDIUM,
        base_font_size: 44.0,
        line_height_ratio: 1.35,
        sub_font_size_ratio: 0.54,
        sub_line_height_ratio: 1.45,
        padding_x_em: 1.1,
        padding_y_ratio: 0.03,
        sweep_fade_em,
        ..Default::default()
    }
}

/// 手机歌词页字号：按宽度缩放，上限 34px；字重与隐藏式播放页一致，
/// 但使用主题字体栈，保证中文 fallback 正常。
fn phone_lyric_style(
    theme: &Theme,
    width: f32,
    height: f32,
    sweep_fade_em: f32,
) -> LyricTextStyle {
    let font = (width * 0.075).clamp(18.0, 34.0);
    let vertical = width < height;
    LyricTextStyle {
        font_family: theme.font_family.clone(),
        font_weight: gpui::FontWeight::SEMIBOLD,
        base_font_size: font,
        line_height_ratio: 1.3,
        sub_font_size_ratio: 0.55,
        sub_line_height_ratio: 1.45,
        padding_x_em: 1.0,
        padding_y_ratio: 0.04,
        edge_fade_top: 0.1,
        edge_fade_bottom: if vertical { 0.08 } else { 0.1 },
        sweep_fade_em,
        ..Default::default()
    }
}

/// AMLL 等价字号：Medium 档 `max(max(5vh, 2.5vw), 14px)`；宽 ≤768 用
/// `max(8vw, 12px)`。是否竖向由宽高比决定（影响底部 mask 渐隐）。
fn immersive_lyric_style(width: f32, height: f32, sweep_fade_em: f32) -> LyricTextStyle {
    let font = if width <= 768.0 {
        (width * 0.08).max(12.0)
    } else {
        (height * 0.05).max(width * 0.025).max(14.0)
    };
    let vertical = width < height;
    LyricTextStyle {
        // 字体栈的 Windows 端是 Segoe UI。
        font_family: "Segoe UI".into(),
        font_weight: gpui::FontWeight::SEMIBOLD,
        base_font_size: font,
        line_height_ratio: 1.2,
        sub_font_size_ratio: 0.5,
        sub_line_height_ratio: 1.5,
        padding_x_em: 1.0,
        padding_y_ratio: 0.02,
        // AMLL 歌词容器 mask：横向上下各 10%，竖向只有顶部 10%。
        edge_fade_bottom: if vertical { 0.0 } else { 0.1 },
        sweep_fade_em,
        ..Default::default()
    }
}

impl ChordialApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        registry: Arc<SourceRegistry>,
        player: Arc<Player>,
        local_source: Option<Arc<chordal_local::LocalSource>>,
        cache: Arc<ObjectCache>,
        search_timeout_ms: u64,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let cfg = crate::config::load_or_default();
        let search_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("搜索歌曲、艺人或专辑");
            s
        });

        // 输入框视图是独立 entity：observe 输入状态，提交回调走根视图。
        let weak_self = cx.entity().downgrade();
        let weak_search = weak_self.clone();
        let search_input_view = cx.new(|cx| {
            InputView::new(search_input.clone(), cx).on_submit(move |text, _window, cx| {
                let _ = weak_search.update(cx, |app, cx| app.start_search(text, cx));
            })
        });

        // 设置面板的路径输入框：回车即应用。
        let settings_cache_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("对象缓存目录");
            s.set_content(cfg.cache_dir.display().to_string(), cx);
            s
        });
        let weak_cache_dir = weak_self.clone();
        let settings_cache_view = cx.new(|cx| {
            InputView::new(settings_cache_input.clone(), cx)
                .with_id("settings-cache-dir")
                .on_submit(move |text, _window, cx| {
                    let _ = weak_cache_dir.update(cx, |app, cx| app.apply_cache_dir(text, cx));
                })
        });
        let settings_download_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("下载目录");
            s.set_content(cfg.download_dir.display().to_string(), cx);
            s
        });
        let weak_download_dir = weak_self.clone();
        let settings_download_view = cx.new(|cx| {
            InputView::new(settings_download_input.clone(), cx)
                .with_id("settings-download-dir")
                .on_submit(move |text, _window, cx| {
                    let _ =
                        weak_download_dir.update(cx, |app, cx| app.apply_download_dir(text, cx));
                })
        });

        // 背景鼓点频段：两个数字输入框，任一框回车都提交整组频段。
        let beat_band = chordal_player::normalize_band(cfg.beat_band_low_hz, cfg.beat_band_high_hz);
        player.set_beat_band(beat_band.0, beat_band.1);
        let settings_beat_low_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("4000");
            s.set_content(format!("{:.0}", beat_band.0), cx);
            s
        });
        let weak_beat_low = weak_self.clone();
        let settings_beat_low_view = cx.new(|cx| {
            InputView::new(settings_beat_low_input.clone(), cx)
                .with_id("settings-beat-low")
                .on_submit(move |_text, _window, cx| {
                    let _ = weak_beat_low.update(cx, |app, cx| app.apply_beat_band(cx));
                })
        });
        let settings_beat_high_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("10000");
            s.set_content(format!("{:.0}", beat_band.1), cx);
            s
        });
        let weak_beat_high = weak_self.clone();
        let settings_beat_high_view = cx.new(|cx| {
            InputView::new(settings_beat_high_input.clone(), cx)
                .with_id("settings-beat-high")
                .on_submit(move |_text, _window, cx| {
                    let _ = weak_beat_high.update(cx, |app, cx| app.apply_beat_band(cx));
                })
        });

        // 新版设置中心：独立实体，动作回到根视图统一应用。
        let settings_view = {
            let weak_settings = weak_self.clone();
            cx.new(|cx| {
                SettingsView::new(
                    move |action, window, cx| {
                        let _ = weak_settings.update(cx, |app, cx| {
                            app.handle_settings_action(action, window, cx);
                        });
                    },
                    cx,
                )
            })
        };

        let login_phone_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("手机号");
            s
        });
        let weak_phone = weak_self.clone();
        let login_phone_view = cx.new(|cx| {
            InputView::new(login_phone_input.clone(), cx)
                .with_id("login-phone")
                .on_submit(move |_text, _window, cx| {
                    let _ = weak_phone.update(cx, |app, cx| app.send_login_captcha(cx));
                })
        });
        let login_captcha_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("短信验证码");
            s
        });
        let weak_captcha = weak_self.clone();
        let login_captcha_view = cx.new(|cx| {
            InputView::new(login_captcha_input.clone(), cx)
                .with_id("login-captcha")
                .on_submit(move |_text, _window, cx| {
                    let _ = weak_captcha.update(cx, |app, cx| app.submit_phone_login(cx));
                })
        });

        let theme = Theme::get(cx).clone();
        let appearance = theme.mode;
        let background_render_divisor =
            crate::config::clamp_background_render_divisor(cfg.background_render_divisor);
        let initial_bg_scale = 1.0 / background_render_divisor as f32;
        let clock: Arc<dyn PlaybackClock> = Arc::new(PlayerClock(player.clone()));
        let lyric_view = cx.new(|cx| {
            let mut view = LyricPlayerView::new(cx);
            view.set_clock(Some(clock));
            {
                let player = player.clone();
                view.set_seek_handler(Some(Arc::new(move |ms: u64| {
                    player.seek_ms(ms.saturating_add(50));
                })));
            }
            {
                // 背景鼓点：视图每帧读取播放线程算好的频段包络。
                let player = player.clone();
                view.set_beat_source(Some(Arc::new(move || player.beat_level())));
            }
            view.set_colors(LyricRenderColors {
                text: theme.text,
                translation: theme.text_dim,
                overlay: gpui::rgba(0x00000066).into(),
                interlude_dot: theme.text,
                fallback_background: theme.bg,
                glow: theme.playing,
            });
            view.set_text_style(compact_lyric_style(&theme, cfg.lyric_sweep_fade_em));
            view.set_player_config(LyricPlayerConfig {
                align_position: 0.38,
                base_font_size: 44.0,
                non_dynamic_opacity: 0.60,
                ..Default::default()
            });
            view.set_background_fps(24);
            view.set_background_render_scale(initial_bg_scale);
            view.set_background_flow_speed(2.0);
            view.set_background_beat_strength(cfg.beat_strength);
            // 浅色主题用纸面纯色，避免深色 Mesh 与深色文字对比不足。
            if !theme.is_dark() {
                view.set_background_kind(BackgroundRendererKind::Css);
            }
            view.set_background_color(theme.bg);
            view
        });

        let eq_search_input = cx.new(|cx| {
            let mut s = InputState::new(cx);
            s.set_placeholder("搜索耳机");
            s
        });
        let weak_eq = weak_self.clone();
        let eq_search_view = cx.new(|cx| {
            InputView::new(eq_search_input.clone(), cx)
                .with_id("eq-search")
                .on_submit(move |text, _window, cx| {
                    let _ = weak_eq.update(cx, |app: &mut ChordialApp, cx| {
                        app.eq.search = text.trim().to_string();
                        cx.notify();
                    });
                })
        });
        let eq = EqState::new(cfg.equalizer.clone(), cfg.eq_saved_presets.clone());
        player.set_eq(eq.settings.clone());

        let mut app = Self {
            registry,
            player,
            local_source: local_source.clone(),
            search_timeout_ms,
            cache,
            downloads: Arc::new(DownloadStore::open(cfg.download_dir.clone())),
            source_preference: cfg.source_preference,
            smooth_scroll: cfg.smooth_scroll,
            lyric_sweep_fade_em: crate::config::clamp_lyric_sweep_fade_em(
                cfg.lyric_sweep_fade_em,
            ),
            background_render_divisor,
            settings_open: false,
            settings_view,
            settings_cache_view,
            settings_download_view,
            settings_beat_low_input,
            settings_beat_low_view,
            settings_beat_high_input,
            settings_beat_high_view,
            beat_band,
            account_state: AccountState::Unknown,
            account_avatar: None,
            user_playlists: Arc::new(Vec::new()),
            playlists_loading: false,
            login_open: false,
            login_tab: LoginTab::Qr,
            login_gen: 0,
            login_challenge: None,
            login_qr: None,
            login_status: None,
            login_phone_input,
            login_phone_view,
            login_captcha_input,
            login_captcha_view,
            captcha_cooldown_until: None,
            collection: None,
            collection_cover: None,
            collection_gen: 0,
            view_tab: ViewTab::Library,
            sidebar_open: true,
            search_input,
            search_input_view,
            search_kind: SearchKind::Track,
            search_collections: Arc::new(Vec::new()),
            search_gen: 0,
            searching: false,
            search_tracks: Arc::new(Vec::new()),
            search_reports: Vec::new(),
            search_elapsed_ms: 0,
            source_route: None,
            library: Arc::new(Vec::new()),
            library_scanning: false,
            library_filter: LibraryFilter::All,
            play_mode: PlayMode::Sequential,
            last_repeat_mode: PlayMode::Sequential,
            queue: Arc::new(Vec::new()),
            queue_pos: None,
            player_status: chordal_player::Status::default(),
            status_message: None,
            info_message: None,
            focus_handle,
            lyric_view,
            current_track: None,
            lyrics_gen: 0,
            lyrics_status: None,
            play_gen: 0,
            appearance,
            cover_image: None,
            retired_images: Vec::new(),
            lyric_preview: Arc::new(Vec::new()),
            activated: false,
            viewport_width: 1280.0,
            viewport_height: 800.0,
            viewport_class: ViewportClass::Wide,
            phone_page: PhonePage::Browse,
            phone_backdrop: PhoneBackdrop::Off,
            lyric_align: 0.35,
            no_blur: std::env::var_os("CHORDIAL_NO_BLUR").is_some(),
            no_bg: std::env::var_os("CHORDIAL_NO_BG").is_some(),
            immersive: false,
            sheet_drag: None,
            immersive_expand: None,
            ui_anim_enter: None,
            ui_anim_play: None,
            ui_anim_shuffle: None,
            ui_anim_repeat: None,
            ui_anim_volume: None,
            last_volume_input: 0.9,
            show_remaining_time: true,
            quality_dialog_open: false,
            cover_scale_anim: None,
            bench_anim: std::env::var_os("CHORDIAL_BENCH_ANIM").is_some(),
            force_portrait: false,
            portrait_page: PortraitPage::Art,
            portrait_transition: None,
            portrait_enter: None,
            portrait_last_input: Instant::now(),
            portrait_chrome_hidden: false,
            portrait_chrome_at: None,
            portrait_idle_ticker: false,
            portrait_toast: None,
            drawer_open: false,
            drawer_at: None,
            drawer_page: DrawerPage::Root,
            wide_lyrics_hidden: false,
            info_marquee: None,
            stage_motion: None,
            queue_scroll: gpui::UniformListScrollHandle::new(),
            search_route_cache: RefCell::new(None),
            row_text_cache: RefCell::new(None),
            now_playing_text: RefCell::new(None),
            playlist_rows: RefCell::new(None),
            source_snapshot: RefCell::new(None),
            sidebar_scroll: gpui::ScrollHandle::new(),
            sleep_minutes: None,
            sleep_until: None,
            sleep_gen: 0,
            eq_open: false,
            eq_open_at: None,
            eq,
            eq_search_input,
            eq_search_view,
            progress_clock: (0, false, Instant::now()),
            eq_save_at: None,
            eq_save_gen: 0,
        };

        if let Some(local) = local_source {
            app.spawn_library_scan(local, cx);
        }
        app.spawn_netease_warmup(cx);
        app.spawn_account_bootstrap(cx);
        app.spawn_status_poll(cx);
        if std::env::var_os("CHORDIAL_START_IMMERSIVE").is_some() {
            app.enter_immersive(cx);
        }
        // 诊断 / 截图：启动即打开设置中心。
        if std::env::var_os("CHORDIAL_START_SETTINGS").is_some() {
            app.open_settings(cx);
        }
        // 诊断 / 截图：启动即打开均衡器。
        if std::env::var_os("CHORDIAL_START_EQ").is_some() {
            app.open_eq(cx);
        }
        app
    }

    // ---------- 后台任务 ----------

    pub fn spawn_library_scan(
        &mut self,
        local: Arc<chordal_local::LocalSource>,
        cx: &mut Context<Self>,
    ) {
        self.library_scanning = true;
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let tracks = cx
                .background_executor()
                .spawn(async move {
                    let summary = local.scan_blocking();
                    let tracks = Arc::new(local.all_tracks());
                    (summary.tracks, tracks)
                })
                .await;
            let _ = weak.update(cx, |app, cx| {
                app.library = tracks.1;
                app.library_scanning = false;
                tracing::info!(tracks = tracks.0, "library scan finished");
                // 诊断：扫描完成后自动播放第一首并进入沉浸页，便于 300Hz
                // 歌词/播放路径压测（CHORDIAL_AUTOPLAY=1）。
                if std::env::var_os("CHORDIAL_AUTOPLAY").is_some() && !app.library.is_empty() {
                    app.play_first(cx);
                    app.enter_immersive(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 播放状态轮询（200ms）：读快照、弹事件、必要时刷新 UI。
    /// 实体销毁（窗口关闭）后循环自动退出。
    fn spawn_status_poll(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let alive = weak
                    .update(cx, |app, cx| {
                        let mut changed = false;
                        while let Some(ev) = app.player.try_pop_event() {
                            match ev {
                                PlayerEvent::TrackEnded(_) => {
                                    app.on_track_ended(cx);
                                    changed = true;
                                }
                                PlayerEvent::StreamError(msg) => {
                                    app.status_message = Some(format!("播放失败：{msg}"));
                                    changed = true;
                                }
                                PlayerEvent::SinkError(msg) => {
                                    app.status_message = Some(format!("输出设备错误：{msg}"));
                                    changed = true;
                                }
                            }
                        }
                        // 快照与上次不一致才重绘（避免空转重渲染）。
                        let status = app.player.status();
                        if status != app.player_status {
                            let was_playing = app.player_status.state == PlayerState::Playing;
                            let is_playing = status.state == PlayerState::Playing;
                            let cover_from = app.cover_scale_now(Instant::now());
                            app.player_status = status;
                            // AMLL 的播放/暂停 SVG 在状态切换时重新挂载，因此
                            // pressed 回弹与图标交换必须同帧发生。
                            if was_playing != is_playing {
                                app.start_cover_scale_anim(cover_from, is_playing);
                                if app.immersive {
                                    app.ui_anim_play = Some(Instant::now());
                                }
                                app.ensure_ui_anim_ticker(cx);
                            }
                            changed = true;
                        }
                        if changed {
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !alive {
                    return;
                }
            }
        })
        .detach();
    }

    /// 播放/暂停并立刻刷新状态与按钮回弹，不等待 200ms 轮询。
    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        let was_playing = self.player_status.state == PlayerState::Playing;
        self.player.toggle();
        let status = self.player.status();
        let is_playing = status.state == PlayerState::Playing;
        let cover_from = self.cover_scale_now(Instant::now());
        self.player_status = status;
        if was_playing != is_playing {
            self.start_cover_scale_anim(cover_from, is_playing);
            if self.immersive {
                // AMLL 的播放/暂停 SVG 在状态切换时重新挂载，pressed 回弹与
                // 图标交换必须在同一帧发生。
                self.ui_anim_play = Some(Instant::now());
            }
        }
        cx.notify();
    }

    // ---------- 搜索 ----------

    pub fn start_search(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.search_gen += 1;
        self.searching = true;
        self.view_tab = ViewTab::Search;
        self.show_phone_browse();
        cx.notify();

        let registry = self.registry.clone();
        let timeout = self.search_timeout_ms;
        let generation = self.search_gen;
        let kind = self.search_kind;
        let query = SearchQuery {
            text,
            limit: 100,
            offset: 0,
            kind,
        };
        if kind == SearchKind::Track {
            cx.spawn(async move |weak, cx| {
                let agg = registry.search_all(&query, timeout).await;
                let _ = weak.update(cx, |app, cx| {
                    if app.search_gen != generation {
                        return; // 过期搜索，丢弃
                    }
                    app.search_tracks = Arc::new(agg.tracks);
                    app.search_collections = Arc::new(Vec::new());
                    app.search_reports = agg.per_source;
                    app.search_elapsed_ms = agg.total_elapsed_ms;
                    app.searching = false;
                    cx.notify();
                });
            })
            .detach();
        } else {
            cx.spawn(async move |weak, cx| {
                let agg = registry.search_collections_all(&query, timeout).await;
                let _ = weak.update(cx, |app, cx| {
                    if app.search_gen != generation {
                        return;
                    }
                    app.search_tracks = Arc::new(Vec::new());
                    app.search_collections = Arc::new(agg.collections);
                    app.search_reports = Vec::new();
                    app.search_elapsed_ms = agg.elapsed_ms;
                    app.searching = false;
                    cx.notify();
                });
            })
            .detach();
        }
    }

    fn set_search_kind(&mut self, kind: SearchKind, cx: &mut Context<Self>) {
        if self.search_kind == kind {
            return;
        }
        self.search_kind = kind;
        self.search_tracks = Arc::new(Vec::new());
        self.search_collections = Arc::new(Vec::new());
        self.search_reports.clear();
        self.search_elapsed_ms = 0;
        self.search_gen = self.search_gen.wrapping_add(1);
        let text = self.search_input.read(cx).content().trim().to_string();
        if !text.is_empty() {
            self.start_search(text, cx);
        } else {
            self.searching = false;
            self.view_tab = ViewTab::Search;
            self.show_phone_browse();
            cx.notify();
        }
    }

    /// 无边框窗口的拖动热区。
    ///
    /// Linux（Wayland/X11）由 gpui 向 compositor 发 move 请求；Windows 有两条
    /// 路径，同时保留：
    /// - `WindowControlArea::Drag` 让 `WM_NCHITTEST` 返回 `HTCAPTION`，由
    ///   DefWindowProc 接管拖动（贴边分屏、双击最大化都是原生行为）；
    /// - 鼠标按下时再调用 `start_window_move` 兜底。命中测试依赖上一帧的
    ///   `mouse_hit_test`，在刚切换页面/DPI 变化等时序下可能拿不到
    ///   HTCAPTION，此时客户区仍会收到 mouse-down，兜底路径保证一定能拖。
    ///   两条路径不会同时触发：返回 HTCAPTION 时客户区收不到 mouse-down。
    #[cfg(target_os = "windows")]
    fn window_drag_area<E: gpui::InteractiveElement>(element: E) -> E {
        element
            .window_control_area(gpui::WindowControlArea::Drag)
            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                window.start_window_move();
            })
    }

    #[cfg(not(target_os = "windows"))]
    fn window_drag_area<E: gpui::InteractiveElement>(element: E) -> E {
        element.on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
            window.start_window_move();
        })
    }

    /// 无边框窗口的 8 个 resize 热区；Wayland 下由 compositor 执行交互缩放。
    fn resize_handles() -> Vec<AnyElement> {
        let edge = |id: &'static str,
                    edge: ResizeEdge,
                    style: CursorStyle,
                    top: Option<f32>,
                    bottom: Option<f32>,
                    left: Option<f32>,
                    right: Option<f32>,
                    w: Option<f32>,
                    h: Option<f32>| {
            let mut el = div().id(id).absolute().cursor(style);
            if let Some(v) = top {
                el = el.top(px(v));
            }
            if let Some(v) = bottom {
                el = el.bottom(px(v));
            }
            if let Some(v) = left {
                el = el.left(px(v));
            }
            if let Some(v) = right {
                el = el.right(px(v));
            }
            if let Some(v) = w {
                el = el.w(px(v));
            }
            if let Some(v) = h {
                el = el.h(px(v));
            }
            el.occlude()
                .on_mouse_down(gpui::MouseButton::Left, move |_, window, _| {
                    window.start_window_resize(edge);
                })
                .into_any_element()
        };
        vec![
            edge(
                "rz-top",
                ResizeEdge::Top,
                CursorStyle::ResizeUp,
                Some(0.0),
                None,
                Some(0.0),
                Some(0.0),
                None,
                Some(6.0),
            ),
            edge(
                "rz-bottom",
                ResizeEdge::Bottom,
                CursorStyle::ResizeDown,
                None,
                Some(0.0),
                Some(0.0),
                Some(0.0),
                None,
                Some(6.0),
            ),
            edge(
                "rz-left",
                ResizeEdge::Left,
                CursorStyle::ResizeLeft,
                Some(0.0),
                Some(0.0),
                Some(0.0),
                None,
                Some(6.0),
                None,
            ),
            edge(
                "rz-right",
                ResizeEdge::Right,
                CursorStyle::ResizeRight,
                Some(0.0),
                Some(0.0),
                None,
                Some(0.0),
                Some(6.0),
                None,
            ),
            edge(
                "rz-tl",
                ResizeEdge::TopLeft,
                CursorStyle::ResizeUpLeftDownRight,
                Some(0.0),
                None,
                Some(0.0),
                None,
                Some(10.0),
                Some(10.0),
            ),
            edge(
                "rz-tr",
                ResizeEdge::TopRight,
                CursorStyle::ResizeUpRightDownLeft,
                Some(0.0),
                None,
                None,
                Some(0.0),
                Some(10.0),
                Some(10.0),
            ),
            edge(
                "rz-bl",
                ResizeEdge::BottomLeft,
                CursorStyle::ResizeUpRightDownLeft,
                None,
                Some(0.0),
                Some(0.0),
                None,
                Some(10.0),
                Some(10.0),
            ),
            edge(
                "rz-br",
                ResizeEdge::BottomRight,
                CursorStyle::ResizeUpLeftDownRight,
                None,
                Some(0.0),
                None,
                Some(0.0),
                Some(10.0),
                Some(10.0),
            ),
        ]
    }

    fn enter_immersive(&mut self, cx: &mut Context<Self>) {
        self.immersive = true;
        self.sheet_drag = None;
        // Dock→全屏：第一帧就是 Dock 的条状卡片，随后与共享封面/曲名
        // 一起放大到满窗。
        self.immersive_expand = Some(SpringSettle::new(
            1.0,
            0.0,
            0.0,
            Instant::now(),
            Self::EXPAND_DAMPING,
            Self::EXPAND_STIFFNESS,
        ));
        self.portrait_page = PortraitPage::Art;
        self.portrait_transition = None;
        self.portrait_enter = Some(Instant::now());
        self.portrait_last_input = Instant::now();
        self.portrait_chrome_hidden = false;
        self.portrait_chrome_at = None;
        self.portrait_toast = None;
        self.hide_drawer_now();
        self.wide_lyrics_hidden = false;
        self.stage_motion = None;
        self.ensure_portrait_idle_ticker(cx);
        // AMLL：页面打开时所有 MediaButton 的 SVG 内容重新挂载 → pressed 回弹；
        // 上一首/下一首的双箭头同时播放一次合并入场动画。
        self.ui_anim_enter = Some(Instant::now());
        self.ui_anim_play = None;
        self.ui_anim_shuffle = None;
        self.ui_anim_repeat = None;
        self.ui_anim_volume = None;
        self.last_volume_input = self.player_status.volume;
        self.quality_dialog_open = false;
        self.cover_scale_anim = None;
        self.ensure_ui_anim_ticker(cx);
        // 沉浸层会接管 `lyric_view` 的样式与背景；先撤销手机壳的 Mesh
        // 背景标记，退出沉浸后下一帧才能重新套用手机播放页样式。
        self.phone_backdrop = PhoneBackdrop::Off;
        let wide = !self.force_portrait
            && self.viewport_width >= self.viewport_height
            && self.viewport_width >= MIN_WIDE_SPLIT_W;
        let white = gpui::Hsla {
            h: 0.0,
            s: 0.0,
            l: 1.0,
            a: 1.0,
        };
        self.lyric_view.update(cx, |view, _| {
            // 沉浸层背景 = AMLL 的 Mesh Gradient（动画），铺满整个窗口：
            // 光栅按低分辨率计算，由 GPU 双线性放大到屏幕。
            view.set_text_visible(wide);
            view.set_click_seek_enabled(wide);
            // 浅色主题在主面板用纯色，但沉浸层必须用封面驱动的 Mesh，
            // 否则会得到一块纯黑/纯纸色背景。
            view.set_background_kind(BackgroundRendererKind::MeshGradient);
            view.set_background_visible(!self.no_bg);
            view.set_background_color(gpui::rgb(0x111111).into());
            view.set_background_fps(30);
            view.set_background_flow_speed(2.0);
            view.set_colors(LyricRenderColors {
                text: white,
                // AMLL lyricSubLine 的 0.3 透明度由渲染层统一乘，这里给纯白。
                translation: white,
                overlay: gpui::Hsla {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                    a: 0.0,
                },
                interlude_dot: white,
                fallback_background: gpui::Hsla {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                    a: 1.0,
                },
                glow: white,
            });
        });
    }

    // ---------- 播放页（AMLL 沉浸层 + 竖屏子页面） ----------

    /// 兼容旧入口：统一进入 AMLL 沉浸层；竖向窗口使用竖屏布局。
    fn open_portrait(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.login_open = false;
        self.eq_open = false;
        self.force_portrait = true;
        self.enter_immersive(cx);
        cx.notify();
    }

    /// 主面板的歌词入口：宽屏沉浸层直接展示右侧歌词（关闭状态会被重置）；
    /// 竖屏窗口不承载歌词，只打开同一套播放页。
    fn open_lyrics_immersive(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.login_open = false;
        self.eq_open = false;
        self.force_portrait = false;
        self.enter_immersive(cx);
        cx.notify();
    }

    fn close_portrait(&mut self, cx: &mut Context<Self>) {
        self.exit_immersive(cx);
        cx.notify();
    }

    /// 任意输入都会重置自动隐藏计时；隐藏状态下则立刻淡入。
    fn bump_portrait(&mut self, cx: &mut Context<Self>) {
        self.portrait_last_input = Instant::now();
        if self.portrait_chrome_hidden {
            self.portrait_chrome_hidden = false;
            self.portrait_chrome_at = Some(Instant::now());
            self.ensure_ui_anim_ticker(cx);
        }
        self.ensure_portrait_idle_ticker(cx);
    }

    fn set_portrait_page(&mut self, page: PortraitPage, cx: &mut Context<Self>) {
        let from = self
            .portrait_transition
            .map(|t| t.to)
            .unwrap_or(self.portrait_page);
        if from == page {
            return;
        }
        let now = Instant::now();
        // FLIP：先记录当前屏幕上的共享元素几何（可能正处于上一段补间中），
        // 再切换页面，最后补间到新页面的页头/大封面几何。播放页在宽屏
        // 歌词可见时是 WideSplit，因此不能只看 `portrait_page` 的默认模态。
        let art_mode =
            self.immersive_stage_mode(cx, self.viewport_width, self.viewport_height);
        let current_mode = if self.portrait_page == PortraitPage::Art {
            art_mode
        } else {
            Self::stage_mode_for_page(self.portrait_page)
        };
        let target_mode = if page == PortraitPage::Art {
            art_mode
        } else {
            Self::stage_mode_for_page(page)
        };
        // 队列是覆盖在播放页上的子页面，整页纵向位移。
        let moving_page = from != page;
        if moving_page {
            let transition = Transition::new(from, page);
            self.stage_motion = Some(self.layout_motion_to(
                target_mode,
                current_mode,
                now,
                transition.duration_ms,
            ));
            self.portrait_transition = Some(transition);
        } else {
            self.portrait_transition = None;
        }
        self.portrait_page = page;
        if page == PortraitPage::Queue
            && let Some(position) = self.queue_pos
        {
            // 虚拟列表只渲染可视行；进入队列时把当前曲目滚到视口中心。
            self.queue_scroll
                .scroll_to_item(position, gpui::ScrollStrategy::Center);
        }
        self.portrait_last_input = Instant::now();
        self.ensure_ui_anim_ticker(cx);
        self.ensure_portrait_idle_ticker(cx);
        cx.notify();
    }

    fn stage_mode_for_page(page: PortraitPage) -> StageMode {
        match page {
            PortraitPage::Art => StageMode::CompactArt,
            PortraitPage::Queue => StageMode::CompactQueue,
        }
    }

    /// 构造一段 First→Last 补间。`fallback` 是当前静止模态；若上一段补间
    /// 尚未结束，则从它的当前插值几何继续，避免连续点击时画面跳变。
    fn layout_motion_to(
        &self,
        to_mode: StageMode,
        fallback: StageMode,
        now: Instant,
        duration_ms: f32,
    ) -> LayoutMotion {
        let from = self.current_stage_geometry(fallback, now);
        let to = immersive_geometry(to_mode, self.viewport_width, self.viewport_height);
        LayoutMotion::starting_at(fallback, from, to_mode, to, now, duration_ms)
    }

    /// 当前屏幕上的共享元素几何：补间进行中就用插值结果。
    fn current_stage_geometry(&self, fallback: StageMode, now: Instant) -> StageGeometry {
        if let Some(motion) = self.stage_motion.filter(|motion| motion.active(now)) {
            return motion.geometry(now);
        }
        immersive_geometry(fallback, self.viewport_width, self.viewport_height)
    }

    /// 当前是否使用布局 1（宽屏/类平板 + 歌词可见）。
    ///
    /// 布局 1 的唯一判据是「横向窗口且歌词可见」；竖屏、无歌词、用户收起歌词
    /// 都回落到布局 2。这样布局不再由两套页面分别决定，避免再次漂移。
    fn layout_one_active(&self, cx: &App, width: f32, height: f32) -> bool {
        !self.force_portrait
            && width >= height
            && width >= MIN_WIDE_SPLIT_W
            && self.lyric_view.read(cx).has_lyrics()
            && !self.wide_lyrics_hidden
    }

    /// 宽屏沉浸层：歌词开/关在「布局 1」与「布局 2」之间切换，
    /// 播放器列与歌词列走同一套共享元素 FLIP。
    fn toggle_wide_lyrics(&mut self, cx: &mut Context<Self>) {
        if !self.layout_can_toggle_lyrics(cx) {
            return;
        }
        let now = Instant::now();
        let current_mode = if self.wide_lyrics_hidden {
            StageMode::CompactArt
        } else {
            StageMode::WideSplit
        };
        self.wide_lyrics_hidden = !self.wide_lyrics_hidden;
        let target_mode = if self.wide_lyrics_hidden {
            StageMode::CompactArt
        } else {
            StageMode::WideSplit
        };
        self.stage_motion = Some(self.layout_motion_to(target_mode, current_mode, now, 320.0));
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    /// 歌词开关只对「横向窗口 + 当前有歌词」有意义。
    fn layout_can_toggle_lyrics(&self, cx: &App) -> bool {
        !self.force_portrait
            && self.viewport_width >= self.viewport_height
            && self.viewport_width >= MIN_WIDE_SPLIT_W
            && self.lyric_view.read(cx).has_lyrics()
    }

    // ---------- 断点与手机壳状态 ----------

    fn is_phone(&self) -> bool {
        self.viewport_class.is_phone()
    }

    /// 手机壳（非沉浸层）才有「播放页 / 曲库页」的切换。
    fn phone_shell_active(&self) -> bool {
        self.is_phone() && !self.immersive
    }

    fn show_phone_browse(&mut self) {
        if self.is_phone() {
            self.phone_page = PhonePage::Browse;
        }
    }

    /// 从手机曲库选中曲目后回到播放页；播放解析是异步的，不能等
    /// `current_track` 落定才切页，否则首次播放会停在曲库页。
    fn show_phone_player(&mut self) {
        if self.is_phone() {
            self.phone_page = PhonePage::Player;
        }
    }

    /// 跨断点时收敛一次跨模块状态。
    ///
    /// 需要处理的只有「哪些视图属于当前布局」：手机壳没有 Dock/侧栏，
    /// 离开手机壳时抽屉与手机背景必须交还给桌面/沉浸层，否则会留下
    /// 一个无入口可关的浮层或一块黑色 Mesh 背景。
    fn handle_viewport_class_change(
        &mut self,
        next: ViewportClass,
        cx: &mut Context<Self>,
    ) {
        let previous = self.viewport_class;
        if previous == next {
            return;
        }
        self.viewport_class = next;
        match (previous, next) {
            (_, ViewportClass::Phone) => {
                self.hide_drawer_now();
                // 首次收窄：保留搜索/集合/队列的浏览上下文；普通曲库
                // 且有曲目时直接落在播放页（参考手机播放器）。
                self.phone_page = phone_default_page(
                    self.view_tab,
                    self.library_filter,
                    self.current_track.is_some(),
                );
                // 不重置 `force_portrait`：它是用户主动进入竖屏播放页的
                // 选择，跨断点后仍应保留，交给沉浸层自己判断布局。
            }
            (ViewportClass::Phone, _) => {
                self.hide_drawer_now();
                self.phone_page = PhonePage::Player;
            }
            _ => {}
        }
        if !self.immersive {
            self.sync_phone_backdrop(cx);
        }
        cx.notify();
    }

    /// 手机壳与沉浸层共用 `lyric_view`。这里把「播放页背景 / 歌词页 /
    /// 不占用」收敛成一次显式迁移，避免每帧重置或两个页面同时渲染。
    fn sync_phone_backdrop(&mut self, cx: &mut Context<Self>) {
        let mode = if self.phone_shell_active() && self.current_track.is_some() {
            match self.phone_page {
                PhonePage::Player => PhoneBackdrop::Player,
                PhonePage::Lyrics => PhoneBackdrop::Lyrics,
                PhonePage::Browse => PhoneBackdrop::Off,
            }
        } else {
            PhoneBackdrop::Off
        };
        if mode == self.phone_backdrop {
            return;
        }
        self.phone_backdrop = mode;
        if mode != PhoneBackdrop::Off {
            let (width, height) = (self.viewport_width, self.viewport_height);
            let scale = self.background_render_scale();
            let no_bg = self.no_bg;
            let no_blur = self.no_blur;
            let sweep = self.lyric_sweep_fade_em;
            let theme = Theme::get(cx).clone();
            let show_text = mode == PhoneBackdrop::Lyrics;
            let style = if show_text {
                phone_lyric_style(&theme, width, height, sweep)
            } else {
                immersive_lyric_style(width, height, sweep)
            };
            let white = gpui::white();
            self.lyric_view.update(cx, |view, _| {
                view.set_text_visible(show_text);
                view.set_click_seek_enabled(show_text);
                view.set_player_config(LyricPlayerConfig {
                    align_anchor: if show_text {
                        AlignAnchor::Center
                    } else {
                        AlignAnchor::Top
                    },
                    align_position: if show_text { 0.5 } else { 0.1 },
                    enable_blur: !no_blur,
                    ..LyricPlayerConfig::default()
                });
                view.set_background_kind(BackgroundRendererKind::MeshGradient);
                view.set_background_visible(!no_bg);
                view.set_background_bounds(None);
                view.set_background_render_scale(scale);
                view.set_background_color(gpui::rgb(0x111111).into());
                view.set_background_fps(30);
                view.set_background_flow_speed(2.0);
                view.set_colors(LyricRenderColors {
                    text: white,
                    translation: white,
                    overlay: gpui::Hsla {
                        h: 0.0,
                        s: 0.0,
                        l: 0.0,
                        a: 0.0,
                    },
                    interlude_dot: white,
                    // 没有封面时 Mesh 会退化成纯黑；给手机壳一个
                    // 偏蓝的近黑底，至少和参考图的“暗色玻璃”一致。
                    fallback_background: gpui::rgb(0x171820).into(),
                    glow: white,
                });
                view.set_text_style(style);
            });
        } else if !self.immersive {
            let theme = Theme::get(cx).clone();
            self.lyric_view.update(cx, |view, _| {
                view.set_text_visible(true);
                view.set_click_seek_enabled(true);
                view.set_background_visible(true);
                view.set_background_bounds(None);
                view.set_background_kind(if theme.is_dark() {
                    BackgroundRendererKind::MeshGradient
                } else {
                    BackgroundRendererKind::Css
                });
                view.set_colors(LyricRenderColors {
                    text: theme.text,
                    translation: theme.text_dim,
                    overlay: theme.overlay,
                    interlude_dot: theme.text,
                    fallback_background: theme.bg,
                    glow: theme.playing,
                });
                view.set_text_style(compact_lyric_style(&theme, self.lyric_sweep_fade_em));
            });
        }
    }

    // ---------- 底部「更多」抽屉 ----------

    const DRAWER_ANIM_MS: f32 = 240.0;

    fn hide_drawer_now(&mut self) {
        self.drawer_open = false;
        self.drawer_at = None;
        self.drawer_page = DrawerPage::Root;
    }

    fn open_drawer(&mut self, cx: &mut Context<Self>) {
        if self.eq_open {
            return;
        }
        self.drawer_open = true;
        self.drawer_page = DrawerPage::Root;
        self.drawer_at = Some(Instant::now());
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    fn close_drawer(&mut self, cx: &mut Context<Self>) {
        if !self.drawer_open {
            return;
        }
        self.drawer_open = false;
        self.drawer_at = Some(Instant::now());
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    fn drawer_animating(&self, now: Instant) -> bool {
        self.drawer_at.is_some_and(|at| {
            now.saturating_duration_since(at).as_secs_f32() * 1000.0 < Self::DRAWER_ANIM_MS
        })
    }

    fn drawer_visible(&self) -> bool {
        self.drawer_open || self.drawer_at.is_some()
    }

    fn drawer_progress(&self, now: Instant) -> f32 {
        let Some(at) = self.drawer_at else {
            return if self.drawer_open { 1.0 } else { 0.0 };
        };
        let t = (now.saturating_duration_since(at).as_secs_f32() * 1000.0
            / Self::DRAWER_ANIM_MS)
            .clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        if self.drawer_open { eased } else { 1.0 - eased }
    }

    pub(crate) fn show_portrait_toast(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let started = Instant::now();
        self.portrait_toast = Some((text.into(), started));
        // Toast 是静态文本，不需要动画帧；用一次性定时器清理即可，
        // 避免为 1.8 秒的显示期持续 300Hz 重绘。
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1800))
                .await;
            let _ = weak.update(cx, |app, cx| {
                if app
                    .portrait_toast
                    .as_ref()
                    .is_some_and(|(_, timestamp)| *timestamp == started)
                {
                    app.portrait_toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// 3.2 秒无输入后隐藏标题 / 进度 / Dock；移动鼠标或点击即恢复。
    fn ensure_portrait_idle_ticker(&mut self, cx: &mut Context<Self>) {
        if self.portrait_idle_ticker {
            return;
        }
        self.portrait_idle_ticker = true;
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(220))
                    .await;
                let alive = weak
                    .update(cx, |app, cx| {
                        if !app.immersive || app.portrait_chrome_hidden {
                            app.portrait_idle_ticker = false;
                            return false;
                        }
                        // 横向布局保持 AMLL 常驻控件，不做自动隐藏。
                        if !app.force_portrait && app.viewport_width >= app.viewport_height {
                            app.portrait_idle_ticker = false;
                            return false;
                        }
                        let idle = app.portrait_last_input.elapsed() >= Duration::from_millis(3200);
                        if idle && !app.drawer_open && app.portrait_transition.is_none() {
                            app.portrait_chrome_hidden = true;
                            app.portrait_chrome_at = Some(Instant::now());
                            app.portrait_idle_ticker = false;
                            app.ensure_ui_anim_ticker(cx);
                            cx.notify();
                            return false;
                        }
                        true
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn portrait_animating(&self) -> bool {
        if !self.immersive {
            return false;
        }
        let within = |t: Option<Instant>, ms: u64| {
            t.is_some_and(|t| t.elapsed() < Duration::from_millis(ms))
        };
        self.portrait_transition.is_some()
            || within(self.portrait_enter, 260)
            || within(self.portrait_chrome_at, 220)
    }

    fn cycle_play_mode(&mut self, cx: &mut Context<Self>) {
        self.play_mode = self.play_mode.next();
        if self.play_mode != PlayMode::Shuffle {
            self.last_repeat_mode = self.play_mode;
        }
        let text = self.play_mode.full_label();
        self.show_portrait_toast(text, cx);
        cx.notify();
    }

    /// AMLL 播放列的随机按钮：独立开关，关闭时回到上一次的循环档。
    fn toggle_amll_shuffle(&mut self) {
        if self.play_mode == PlayMode::Shuffle {
            self.play_mode = self.last_repeat_mode;
        } else {
            self.last_repeat_mode = self.play_mode;
            self.play_mode = PlayMode::Shuffle;
        }
    }

    /// AMLL 播放列的循环按钮：Off → All → One → Off。
    fn cycle_amll_repeat(&mut self) {
        self.play_mode = match self.play_mode {
            PlayMode::RepeatAll => PlayMode::RepeatOne,
            PlayMode::RepeatOne => PlayMode::Sequential,
            // Shuffle / Sequential 一律先进入列表循环，并把 shuffle 关掉。
            PlayMode::Shuffle | PlayMode::Sequential => PlayMode::RepeatAll,
        };
        self.last_repeat_mode = self.play_mode;
    }

    fn set_sleep_minutes(&mut self, minutes: Option<u32>, cx: &mut Context<Self>) {
        self.sleep_gen = self.sleep_gen.wrapping_add(1);
        let generation = self.sleep_gen;
        self.sleep_minutes = minutes;
        self.sleep_until = minutes.map(|m| Instant::now() + Duration::from_secs(m as u64 * 60));
        if let Some(minutes) = minutes {
            let weak = cx.entity().downgrade();
            cx.spawn(async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(minutes as u64 * 60))
                    .await;
                let _ = weak.update(cx, |app, cx| {
                    if app.sleep_gen != generation {
                        return;
                    }
                    app.sleep_until = None;
                    app.sleep_minutes = None;
                    app.player.pause();
                    app.player_status = app.player.status();
                    app.portrait_toast = Some(("睡眠定时结束，已暂停播放".into(), Instant::now()));
                    app.ensure_ui_anim_ticker(cx);
                    cx.notify();
                });
            })
            .detach();
            self.show_portrait_toast(format!("{minutes} 分钟后暂停播放"), cx);
        } else {
            self.show_portrait_toast("已取消睡眠定时", cx);
        }
        cx.notify();
    }

    // ---------- 参数均衡器 ----------

    fn open_eq(&mut self, cx: &mut Context<Self>) {
        self.eq_open = true;
        self.eq_open_at = Some(Instant::now());
        self.eq.settings = self.eq.settings.clone().sanitized();
        self.player.set_eq(self.eq.settings.clone());
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    pub(crate) fn close_eq(&mut self, cx: &mut Context<Self>) {
        self.eq_open = false;
        self.eq_save_at = None;
        self.persist_eq();
        cx.notify();
    }

    pub(crate) fn remove_saved_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.eq.saved.len() {
            self.eq.saved.remove(index);
            self.persist_eq();
            cx.notify();
        }
    }

    fn eq_animating(&self) -> bool {
        // 只有打开时的 240ms 入场淡入需要连续帧；400ms 落盘防抖是后台
        // 一次性任务，不应把整个界面拖入持续重绘。
        self.eq_open
            && self
                .eq_open_at
                .is_some_and(|t| t.elapsed() < Duration::from_millis(260))
    }

    /// 把当前配置推给播放线程，并在 400ms 后落盘。
    pub(crate) fn apply_eq(&mut self, cx: &mut Context<Self>) {
        self.eq.settings = self.eq.settings.clone().sanitized();
        self.eq.selected = self
            .eq
            .selected
            .filter(|index| *index < self.eq.settings.bands.len());
        self.player.set_eq(self.eq.settings.clone());
        self.eq_save_at = Some(Instant::now());
        self.eq_save_gen = self.eq_save_gen.wrapping_add(1);
        let generation = self.eq_save_gen;
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(420))
                .await;
            let _ = weak.update(cx, |app, _| {
                if app.eq_save_gen == generation && app.eq_save_at.is_some() {
                    app.eq_save_at = None;
                    app.persist_eq();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn set_eq_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        self.eq.settings.enabled = on;
        self.eq.touch();
        self.apply_eq(cx);
        self.show_portrait_toast(
            if on {
                "均衡器已开启"
            } else {
                "均衡器已关闭"
            },
            cx,
        );
        cx.notify();
    }

    pub(crate) fn add_eq_band(&mut self, cx: &mut Context<Self>) {
        if self.eq.add_band().is_some() {
            self.apply_eq(cx);
        } else {
            self.show_portrait_toast(format!("最多 {EQ_MAX_BANDS} 个频段"), cx);
        }
    }

    pub(crate) fn remove_eq_band(&mut self, cx: &mut Context<Self>) {
        self.eq.remove_selected();
        self.apply_eq(cx);
    }

    pub(crate) fn select_eq_band(&mut self, index: usize, cx: &mut Context<Self>) {
        self.eq.selected = Some(index);
        self.eq.type_menu_open = false;
        self.bump_portrait(cx);
        cx.notify();
    }

    pub(crate) fn set_eq_filter_kind(&mut self, kind: FilterKind, cx: &mut Context<Self>) {
        if let Some(band) = self.eq.selected_band_mut() {
            band.kind = kind;
            self.eq.type_menu_open = false;
            self.eq.touch();
            self.apply_eq(cx);
        }
    }

    pub(crate) fn set_eq_freq(&mut self, freq_hz: f32, cx: &mut Context<Self>) {
        if let Some(band) = self.eq.selected_band_mut() {
            band.freq_hz = freq_hz.clamp(20.0, 20_000.0);
            self.eq.touch();
            self.apply_eq(cx);
        }
    }

    pub(crate) fn set_eq_gain(&mut self, gain_db: f32, cx: &mut Context<Self>) {
        if let Some(band) = self.eq.selected_band_mut() {
            band.gain_db = gain_db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB);
            self.eq.touch();
            self.apply_eq(cx);
        }
    }

    pub(crate) fn set_eq_q(&mut self, q: f32, cx: &mut Context<Self>) {
        if let Some(band) = self.eq.selected_band_mut() {
            band.q = q.clamp(chordal_player::EQ_MIN_Q, chordal_player::EQ_MAX_Q);
            self.eq.touch();
            self.apply_eq(cx);
        }
    }

    pub(crate) fn load_eq_preset(&mut self, preset: PresetEntry, cx: &mut Context<Self>) {
        self.eq.load_preset(&preset);
        self.apply_eq(cx);
        self.show_portrait_toast(format!("已应用「{}」", preset.name), cx);
        cx.notify();
    }

    // ---------- AutoEQ 耳机档案 ----------

    /// 切换 EQ 分类；进入 AutoEQ 时按需加载仓库索引。
    pub(crate) fn select_eq_tab(&mut self, tab: EqTab, cx: &mut Context<Self>) {
        self.eq.tab = tab;
        self.eq.type_menu_open = false;
        if tab == EqTab::AutoEq {
            self.ensure_autoeq(cx);
        }
        cx.notify();
    }

    /// 首次进入 AutoEQ：先读本地缓存（同步、几毫秒），过期或缺失时后台拉取。
    pub(crate) fn ensure_autoeq(&mut self, cx: &mut Context<Self>) {
        if self.eq.autoeq.status != AutoEqStatus::Idle || self.eq.autoeq.index_loading {
            return;
        }
        match autoeq::load_cached_index() {
            Some((entries, modified)) => {
                self.eq.autoeq.replace_entries(entries);
                self.eq.autoeq.status = AutoEqStatus::Ready;
                self.eq.autoeq.from_cache = true;
                self.eq.autoeq.error = None;
                cx.notify();
                if autoeq::index_cache_is_stale(modified) {
                    self.sync_autoeq(false, cx);
                }
            }
            None => self.sync_autoeq(false, cx),
        }
    }

    /// 从 AutoEq 仓库同步索引；已有缓存时列表保持可见，只更新状态。
    pub(crate) fn sync_autoeq(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.eq.autoeq.index_loading {
            return;
        }
        self.eq.autoeq.index_loading = true;
        if self.eq.autoeq.entries.is_empty() {
            self.eq.autoeq.status = AutoEqStatus::Loading;
        }
        self.eq.autoeq.error = None;
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async { autoeq::fetch_index_blocking() })
                .await;
            let _ = weak.update(cx, |app, cx| {
                app.eq.autoeq.index_loading = false;
                let toast = match result {
                    Ok(entries) => {
                        let count = entries.len();
                        app.eq.autoeq.replace_entries(entries);
                        app.eq.autoeq.status = AutoEqStatus::Ready;
                        app.eq.autoeq.from_cache = false;
                        app.eq.autoeq.error = None;
                        tracing::info!(count, "autoeq index synced");
                        manual.then(|| format!("AutoEQ 已同步：{count} 款耳机"))
                    }
                    Err(error) => {
                        if app.eq.autoeq.entries.is_empty() {
                            app.eq.autoeq.status = AutoEqStatus::Failed;
                        }
                        tracing::warn!(error = %error, "autoeq index sync failed");
                        app.eq.autoeq.error = Some(error.clone());
                        manual.then(|| format!("AutoEQ 同步失败：{error}"))
                    }
                };
                if let Some(message) = toast {
                    app.show_portrait_toast(message, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 预览（`apply = false`）或直接应用（`apply = true`）某个型号；
    /// 曲线尚未拉取时先读缓存，再按需下载，拉取完成后自动应用。
    pub(crate) fn select_autoeq_entry(
        &mut self,
        file: String,
        apply: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((name, loaded)) = self
            .eq
            .autoeq
            .find(&file)
            .map(|entry| (entry.name.clone(), entry.bands.is_some()))
        else {
            return;
        };
        // 新选择覆盖旧的「拉取完自动应用」请求：预览别的型号不应再触发
        // 上一次的自动应用。
        if self.eq.autoeq.pending_apply.as_deref() != Some(file.as_str()) {
            self.eq.autoeq.pending_apply = None;
        }
        self.eq.autoeq_preview = Some(file.clone());
        if loaded {
            if apply {
                self.apply_autoeq_entry(&file, cx);
            } else {
                cx.notify();
            }
            return;
        }
        if let Some((bands, gain)) = autoeq::load_cached_param(&file) {
            self.store_autoeq_param(&file, bands, gain);
            if apply {
                self.apply_autoeq_entry(&file, cx);
            } else {
                cx.notify();
            }
            return;
        }
        if apply {
            self.eq.autoeq.pending_apply = Some(file.clone());
            self.show_portrait_toast(format!("正在拉取「{name}」…"), cx);
        }
        let already_loading = self
            .eq
            .autoeq
            .find(&file)
            .is_some_and(|entry| entry.loading);
        if !already_loading
            && let Some(entry) = self.eq.autoeq.find_mut(&file)
        {
            entry.loading = true;
            entry.error = None;
        }
        cx.notify();
        if already_loading {
            return;
        }
        cx.spawn(async move |weak, cx| {
            let key = file.clone();
            let result = cx
                .background_executor()
                .spawn(async move { autoeq::fetch_param_blocking(&key) })
                .await;
            let _ = weak.update(cx, |app, cx| {
                let pending = app.eq.autoeq.pending_apply.clone();
                let mut apply_now = false;
                let mut failure = None;
                if let Some(entry) = app.eq.autoeq.find_mut(&file) {
                    entry.loading = false;
                    match result {
                        Ok((bands, gain)) => {
                            entry.bands = Some(bands);
                            entry.input_gain_db = gain;
                            entry.error = None;
                            apply_now = pending.as_deref() == Some(file.as_str());
                        }
                        Err(error) => {
                            entry.error = Some(error.clone());
                            failure = Some(error);
                        }
                    }
                }
                if let Some(error) = &failure {
                    tracing::warn!(file = %file, error = %error, "autoeq param fetch failed");
                }
                if apply_now {
                    app.eq.autoeq.pending_apply = None;
                    app.apply_autoeq_entry(&file, cx);
                } else if failure.is_some() {
                    app.eq.autoeq.pending_apply = None;
                }
                if let Some(error) = failure {
                    app.show_portrait_toast(format!("AutoEQ 拉取失败：{error}"), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn store_autoeq_param(&mut self, file: &str, bands: Vec<EqBand>, input_gain_db: f32) {
        if let Some(entry) = self.eq.autoeq.find_mut(file) {
            entry.bands = Some(bands);
            entry.input_gain_db = input_gain_db;
            entry.error = None;
            entry.loading = false;
        }
    }

    pub(crate) fn apply_autoeq_entry(&mut self, file: &str, cx: &mut Context<Self>) {
        let Some((name, settings)) = self
            .eq
            .autoeq
            .find(file)
            .and_then(|entry| entry.to_settings().map(|settings| (entry.name.clone(), settings)))
        else {
            return;
        };
        self.eq.load_settings(name.clone(), settings);
        self.eq.autoeq_preview = Some(file.to_string());
        self.apply_eq(cx);
        self.show_portrait_toast(format!("已应用「{name}」"), cx);
        cx.notify();
    }

    pub(crate) fn save_eq_preset(&mut self, cx: &mut Context<Self>) {
        if self.eq.settings.bands.is_empty() {
            self.show_portrait_toast("空配置无需保存", cx);
            return;
        }
        let name = self.eq.save_as();
        self.eq.tab = EqTab::Personal;
        self.eq_save_at = None;
        self.persist_eq();
        self.show_portrait_toast(format!("已保存「{name}」"), cx);
        cx.notify();
    }

    pub(crate) fn persist_eq(&mut self) {
        let mut cfg = crate::config::load_or_default();
        cfg.equalizer = self.eq.settings.clone();
        cfg.eq_saved_presets = self.eq.saved.clone();
        crate::config::save(&cfg);
    }

    const UI_ANIM_PRESS_MS: f32 = 700.0;
    const UI_ANIM_ARROWS_MS: f32 = 600.0;
    const UI_ANIM_SPEAKER_MS: f32 = 750.0;
    /// Dock→全屏展开弹簧：轻微过冲后收敛。
    const EXPAND_DAMPING: f32 = 21.0;
    const EXPAND_STIFFNESS: f32 = 200.0;

    fn anim_progress(start: Option<Instant>, duration_ms: f32) -> f32 {
        match start {
            Some(t) => (t.elapsed().as_secs_f32() * 1000.0 / duration_ms).clamp(0.0, 1.0),
            None => 1.0,
        }
    }

    fn latest_anim(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
        match (a, b) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, None) => a,
            (None, b) => b,
        }
    }

    /// AMLL Cover：暂停时缩到 0.75，播放时回到 1。
    fn cover_scale_target(&self) -> f32 {
        if self.player_status.state == PlayerState::Playing {
            1.0
        } else {
            0.75
        }
    }

    fn cover_scale_now(&self, now: Instant) -> f32 {
        let Some((start, from, to, duration)) = self.cover_scale_anim else {
            return self.cover_scale_target();
        };
        let t = (now.saturating_duration_since(start).as_secs_f32() / duration.max(0.001))
            .clamp(0.0, 1.0);
        let eased = if to > from {
            // 播放：AMLL 的 cubic-bezier(0.3, 0.2, 0.2, 1.4)，轻微过冲。
            crate::icons::cubic_bezier(0.3, 0.2, 0.2, 1.4, t)
        } else {
            crate::icons::cubic_bezier(0.4, 0.2, 0.1, 1.0, t)
        };
        from + (to - from) * eased
    }

    /// 播放状态变化时启动封面缩放；`from` 必须在写入新状态之前采样。
    fn start_cover_scale_anim(&mut self, from: f32, playing: bool) {
        let target = if playing { 1.0 } else { 0.75 };
        if (from - target).abs() < 0.001 {
            self.cover_scale_anim = None;
            return;
        }
        let duration = if playing { 0.5 } else { 0.6 };
        self.cover_scale_anim = Some((Instant::now(), from, target, duration));
    }

    fn cover_scale_anim_active(&self, now: Instant) -> bool {
        self.cover_scale_anim.is_some_and(|(start, _, _, duration)| {
            now.saturating_duration_since(start).as_secs_f32() < duration
        })
    }

    fn ui_anim_active(&self) -> bool {
        if self.bench_anim {
            return true;
        }
        let running = |start: Option<Instant>, ms: f32| {
            start.is_some_and(|t| t.elapsed().as_secs_f32() * 1000.0 < ms)
        };
        running(self.ui_anim_enter, Self::UI_ANIM_PRESS_MS)
            || running(self.ui_anim_play, Self::UI_ANIM_PRESS_MS)
            || running(self.ui_anim_shuffle, Self::UI_ANIM_PRESS_MS)
            || running(self.ui_anim_repeat, Self::UI_ANIM_PRESS_MS)
            || self
                .ui_anim_volume
                .is_some_and(|(t, _)| t.elapsed().as_secs_f32() * 1000.0 < Self::UI_ANIM_SPEAKER_MS)
            || self
                .stage_motion
                .is_some_and(|motion| motion.active(Instant::now()))
            || self.cover_scale_anim_active(Instant::now())
            || self
                .info_marquee
                .is_some_and(|marquee| marquee.active(Instant::now()))
            || self.portrait_animating()
            || self.sheet_drag.is_some_and(|drag| drag.animating())
            || self.drawer_animating(Instant::now())
            || self.immersive_expanding(Instant::now())
            || self.eq_animating()
    }

    /// 安排一次重绘。帧节奏由 `Render` 中的 `request_animation_frame` 驱动：
    /// 只要有动画未结束，就跟随显示器刷新率（300Hz 屏即 300fps），动画
    /// 结束后自动停止。这里不再维护固定 16ms 定时泵。
    fn ensure_ui_anim_ticker(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    /// 在渲染开始前清理已结束的过渡状态，避免动画结束后仍请求下一帧。
    fn cleanup_finished_animations(&mut self, now: Instant, cx: &mut Context<Self>) {
        if self
            .portrait_transition
            .is_some_and(|transition| transition.done(now))
        {
            self.portrait_transition = None;
        }
        if self.stage_motion.is_some_and(|motion| motion.settled(now)) {
            self.stage_motion = None;
        }
        if let Some((start, _, _, duration)) = self.cover_scale_anim
            && now.saturating_duration_since(start).as_secs_f32() >= duration
        {
            self.cover_scale_anim = None;
        }
        if self
            .info_marquee
            .is_some_and(|marquee| !marquee.active(now))
        {
            self.info_marquee = None;
        }
        // 抽屉开/合动画结束：清掉起点；关闭状态由 `drawer_visible` 判定隐藏。
        if self.drawer_at.is_some_and(|at| {
            now.saturating_duration_since(at).as_secs_f32() * 1000.0 >= Self::DRAWER_ANIM_MS
        }) {
            self.drawer_at = None;
        }
        // Dock→全屏展开结束：撤销动画状态，页面由静态布局接管。
        if self
            .immersive_expand
            .is_some_and(|spring| spring.settled(now))
        {
            self.immersive_expand = None;
        }
        // 下拉关闭：回弹结束直接收手；关闭动画结束才真正退出沉浸层。
        if let Some(drag) = self.sheet_drag {
            match drag.settled(now) {
                Some(SheetDragOutcome::Returned) => self.sheet_drag = None,
                Some(SheetDragOutcome::Dismissed) => {
                    self.sheet_drag = None;
                    self.exit_immersive(cx);
                }
                None => {}
            }
        }
    }

    /// 音量滑杆输入：跨越 0 / 1 边界时触发 AMLL 的喇叭弹跳。
    fn note_volume_input(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(0.0, 1.0);
        let prev = self.last_volume_input;
        self.last_volume_input = value;
        let side = if value <= 0.0001 && prev > 0.0001 {
            Some(false)
        } else if value >= 0.9999 && prev < 0.9999 {
            Some(true)
        } else {
            None
        };
        if let Some(high) = side {
            self.ui_anim_volume = Some((Instant::now(), high));
            self.ensure_ui_anim_ticker(cx);
            cx.notify();
        }
    }

    // ---------- 沉浸页下拉关闭 ----------
    //
    // 顶部拖拽条是统一的 sheet 把手：按下开始记手，向下拖动时整页跟手
    // 下移并露出主面板，松手后按位移/速度回弹或退出沉浸层（参考视频的
    // 播放页下拉关闭手感）。

    fn begin_sheet_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        if !self.immersive || self.eq_open || self.immersive_expanding(Instant::now()) {
            return;
        }
        self.sheet_drag = Some(SheetDrag::begin(y, Instant::now()));
        cx.notify();
    }

    fn update_sheet_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        if let Some(drag) = &mut self.sheet_drag {
            if drag.update(y, Instant::now()) {
                cx.notify();
            }
        }
    }

    fn release_sheet_drag(&mut self, height: f32, cx: &mut Context<Self>) {
        let now = Instant::now();
        let dock_y = dock_rect(self.viewport_width, self.viewport_height).y;
        if let Some(drag) = &mut self.sheet_drag {
            drag.release(height, dock_y, now);
            if !drag.animating() {
                self.sheet_drag = None;
            }
        }
        self.ensure_ui_anim_ticker(cx);
        cx.notify();
    }

    /// Dock→全屏共享布局的当前进度：`1.0` = 页面仍完全贴在 Dock 上，
    /// `0.0` = 已铺满整个窗口。展开动画与下拉手势共用这一条通道。
    fn immersive_dock_progress(&self, now: Instant, height: f32) -> f32 {
        if let Some(spring) = self.immersive_expand {
            return spring.position(now).clamp(0.0, 1.0);
        }
        if let Some(drag) = self.sheet_drag {
            if drag.engaged() || drag.animating() {
                let dock_y = dock_rect(self.viewport_width, height).y.max(1.0);
                return (drag.offset(now) / dock_y).clamp(0.0, 1.0);
            }
        }
        0.0
    }

    fn immersive_expanding(&self, now: Instant) -> bool {
        self.immersive_expand
            .is_some_and(|spring| !spring.settled(now))
    }

    /// 当前沉浸页的共享元素目标模态（与页面实际布局选择一致）。
    fn immersive_stage_mode(&self, cx: &App, width: f32, height: f32) -> StageMode {
        if self.layout_one_active(cx, width, height) {
            StageMode::WideSplit
        } else {
            StageMode::CompactArt
        }
    }

    fn exit_immersive(&mut self, cx: &mut Context<Self>) {
        self.immersive = false;
        self.sheet_drag = None;
        self.immersive_expand = None;
        self.force_portrait = false;
        self.portrait_page = PortraitPage::Art;
        self.portrait_transition = None;
        self.hide_drawer_now();
        self.portrait_chrome_hidden = false;
        self.wide_lyrics_hidden = false;
        self.stage_motion = None;
        let theme = Theme::get(cx).clone();
        self.lyric_view.update(cx, |view, _| {
            view.set_text_visible(true);
            view.set_click_seek_enabled(true);
            view.set_background_visible(true);
            view.set_background_bounds(None);
            view.set_colors(LyricRenderColors {
                text: theme.text,
                translation: theme.text,
                overlay: theme.overlay,
                interlude_dot: theme.text,
                fallback_background: theme.bg,
                glow: theme.playing,
            });
            view.set_text_style(compact_lyric_style(&theme, self.lyric_sweep_fade_em));
        });
    }

    fn toggle_appearance(&mut self, cx: &mut Context<Self>) {
        let mode = match self.appearance {
            Appearance::Dark => Appearance::Light,
            Appearance::Light => Appearance::Dark,
        };
        self.appearance = mode;
        Theme::for_mode(mode).set_global(cx);
        // 歌词 view 的颜色/字体在构造时缓存，切换主题后需要重放。
        if self.phone_backdrop != PhoneBackdrop::Off {
            // 手机播放页的背景与文字是「封面驱动 + 白色」，不跟随明暗
            // 主题；重放主题样式会把它变回面板色，必须重新套用手机样式。
            self.phone_backdrop = PhoneBackdrop::Off;
            self.sync_phone_backdrop(cx);
        } else {
            let theme = Theme::get(cx).clone();
            // 沉浸层的字号/字重与面板不同；主题切换时必须按当前模式重放，
            // 否则沉浸层会被换成面板样式（字体高度/行高看起来会突然变化）。
            let style = if self.immersive {
                immersive_lyric_style(
                    self.viewport_width,
                    self.viewport_height,
                    self.lyric_sweep_fade_em,
                )
            } else {
                compact_lyric_style(&theme, self.lyric_sweep_fade_em)
            };
            let view = self.lyric_view.clone();
            view.update(cx, |view, _| {
                view.set_colors(LyricRenderColors {
                    text: theme.text,
                    translation: theme.text_dim,
                    overlay: theme.overlay,
                    interlude_dot: theme.text,
                    fallback_background: theme.bg,
                    glow: theme.playing,
                });
                view.set_text_style(style);
                view.set_background_kind(if theme.is_dark() {
                    BackgroundRendererKind::MeshGradient
                } else {
                    BackgroundRendererKind::Css
                });
                view.set_background_color(theme.bg);
            });
        }
        let mut cfg = crate::config::load_or_default();
        cfg.appearance = mode;
        crate::config::save(&cfg);
        cx.notify();
    }

    // ---------- 播放 ----------

    /// 解析播放句柄（可能跨源网络请求）后交给播放线程。
    fn resolve_and_play(&mut self, cx: &mut Context<Self>) {
        self.resolve_and_play_impl(true, cx);
    }

    fn resolve_and_play_impl(&mut self, load_extras: bool, cx: &mut Context<Self>) {
        let Some(pos) = self.queue_pos else { return };
        let Some(track) = self.queue.get(pos).cloned() else {
            return;
        };
        let registry = self.registry.clone();
        let source_id = track.id.source.clone();
        let track_id = track.id.clone();
        let title = track.title.clone();
        let preference = self.source_preference;

        // 显式下载优先：下载文件是“本地资产”，不占缓存配额，也不再请求网络。
        let downloaded = self.downloads.path_for(&track_id);
        // 在线曲目先找本地同曲候选；歌词/封面仍按原曲目加载。
        let local_match = if matches!(track.origin, TrackOrigin::Remote { .. }) {
            self.local_source
                .as_ref()
                .and_then(|source| source.find_equivalent(&track))
        } else {
            None
        };

        self.play_gen = self.play_gen.wrapping_add(1);
        let play_gen = self.play_gen;
        self.status_message = None;
        self.info_message = None;
        self.current_track = Some(track.clone());
        if load_extras {
            self.load_track_extras(track.clone(), cx);
        }

        if let Some(path) = downloaded {
            self.player.play(StreamHandle::File(path), title, 0);
            self.info_message = Some("播放已下载文件（不占缓存配额）".into());
            cx.notify();
            return;
        }

        cx.spawn(async move |weak, cx| {
            let (remote, remote_err) = match registry.get(&source_id) {
                Some(source) => match source.resolve_stream(&track_id).await {
                    Ok(resolved) => (Some(resolved), None),
                    Err(e) => (None, Some(e.to_string())),
                },
                None => (None, Some(format!("源 {source_id} 未加载"))),
            };
            let remote_quality = remote.as_ref().and_then(|r| r.quality.clone());
            let remote_label = remote_quality
                .as_ref()
                .map(|q| q.label())
                .unwrap_or_else(|| "未知音质".into());
            let remote_handle = remote.map(|r| r.handle);

            let local_choice = local_match.and_then(|t| match t.origin.clone() {
                TrackOrigin::Local { path } => Some((t, path)),
                _ => None,
            });
            let local_score = local_choice
                .as_ref()
                .map(|(t, _)| t.quality_score())
                .unwrap_or(0);
            let remote_score = remote_quality.as_ref().map(|q| q.score()).unwrap_or(0);
            let use_local = match preference {
                SourcePreference::PreferLocal => local_choice.is_some(),
                SourcePreference::PreferOnline => false,
                SourcePreference::Auto => local_choice.is_some() && local_score > remote_score,
            };

            // 在线解析失败时自动回退本地同曲；否则按偏好/音质分选择。
            let chosen_local = if use_local || remote_handle.is_none() {
                local_choice.clone()
            } else {
                None
            };
            let local_label = local_choice
                .as_ref()
                .and_then(|(t, _)| t.quality.clone())
                .unwrap_or_else(|| "未知音质".into());

            let _ = weak.update(cx, |app, cx| {
                if app.play_gen != play_gen {
                    return;
                }
                if let Some((local_track, path)) = chosen_local {
                    app.player
                        .play(StreamHandle::File(path), local_track.title.clone(), 0);
                    app.current_track = Some(local_track.clone());
                    if remote_err.is_some() {
                        app.status_message =
                            Some(format!("在线解析失败，已改播本地「{}」", local_track.title));
                    } else if preference == SourcePreference::PreferLocal {
                        app.info_message =
                            Some(format!("本地优先：{} · {local_label}", local_track.title));
                    } else {
                        app.info_message =
                            Some(format!("本地音质更高：{local_label} > 在线 {remote_label}"));
                    }
                    if load_extras {
                        app.load_track_extras(local_track, cx);
                    }
                    cx.notify();
                    return;
                }
                match remote_handle {
                    Some(handle) => {
                        app.player.play(handle, title.clone(), 0);
                        if let Some(current) = app.current_track.as_mut() {
                            current.quality = Some(remote_label.clone());
                            current.quality_info = remote_quality.clone();
                        }
                        // 只在“有本地同曲但最终选了在线”时提示，避免打扰。
                        if local_choice.is_some() && preference == SourcePreference::Auto {
                            app.info_message = Some(format!(
                                "在线 {remote_label} · 本地同曲 {local_label}（按音质选择）"
                            ));
                        }
                        cx.notify();
                    }
                    None => {
                        app.status_message = Some(format!(
                            "无法播放「{title}」：{}",
                            remote_err.unwrap_or_else(|| "无可用播放地址".into())
                        ));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// 点击列表行：把当前可见列表设为队列并播放。
    fn play_from_list(&mut self, list: QueueList, index: usize, cx: &mut Context<Self>) {
        let tracks: Arc<Vec<Track>> = match list {
            QueueList::Search => match &self.source_route {
                // 路由到单源时，"播放全部"与行点击都只在可见结果内流转。
                Some(route) => Arc::new(
                    self.search_tracks
                        .iter()
                        .filter(|t| &t.id.source == route)
                        .cloned()
                        .collect(),
                ),
                None => self.search_tracks.clone(),
            },
            QueueList::Library => self.library.clone(),
            QueueList::Queue => self.queue.clone(),
            QueueList::Collection => self
                .collection
                .as_ref()
                .map(|c| c.tracks.clone())
                .unwrap_or_else(|| Arc::new(Vec::new())),
        };
        if index >= tracks.len() {
            return;
        }
        self.queue = tracks;
        self.queue_pos = Some(index);
        self.status_message = None;
        self.resolve_and_play(cx);
        // 手机壳：选歌的最终目的是播放，直接回到播放页，避免用户还要
        // 再找一次返回入口。宽屏三栏同时可见，不需要这一步。
        self.show_phone_player();
        cx.notify();
    }

    /// 从播放队列移除一项；不打断当前播放，只修正 queue_pos。
    fn remove_queue_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.queue.len() {
            return;
        }
        let mut queue = self.queue.as_ref().clone();
        queue.remove(index);
        self.queue = Arc::new(queue);
        self.queue_pos = match self.queue_pos {
            Some(pos) if pos > index => Some(pos - 1),
            Some(pos) if pos == index => {
                if self.queue.is_empty() {
                    None
                } else {
                    Some(pos.min(self.queue.len() - 1))
                }
            }
            other => other,
        };
        cx.notify();
    }

    /// 自动播完：单曲循环原地重播；随机模式随机下一首；否则顺序下一首。
    fn on_track_ended(&mut self, cx: &mut Context<Self>) {
        match self.play_mode {
            PlayMode::RepeatOne => {
                if self.queue_pos.is_some() {
                    self.resolve_and_play_impl(false, cx);
                }
            }
            PlayMode::Shuffle => self.play_random(cx),
            PlayMode::RepeatAll | PlayMode::Sequential => self.play_next(cx),
        }
    }

    /// F5：没有当前曲目时从本地库第一首开始；否则等价于播放/暂停。
    pub fn play_first(&mut self, cx: &mut Context<Self>) {
        if self.queue_pos.is_none() && !self.library.is_empty() {
            self.play_from_list(QueueList::Library, 0, cx);
        } else {
            self.toggle_playback(cx);
        }
    }

    pub fn play_next(&mut self, cx: &mut Context<Self>) {
        let Some(pos) = self.queue_pos else { return };
        if pos + 1 < self.queue.len() {
            self.queue_pos = Some(pos + 1);
            self.resolve_and_play(cx);
        } else if self.play_mode == PlayMode::RepeatAll && !self.queue.is_empty() {
            self.queue_pos = Some(0);
            self.resolve_and_play(cx);
        } else {
            self.stop_playback(cx);
        }
        cx.notify();
    }

    fn play_prev(&mut self, cx: &mut Context<Self>) {
        let Some(pos) = self.queue_pos else { return };
        if self.play_mode == PlayMode::Shuffle {
            self.play_random(cx);
        } else if pos == 0 && self.play_mode == PlayMode::RepeatAll && !self.queue.is_empty() {
            self.queue_pos = Some(self.queue.len() - 1);
            self.resolve_and_play(cx);
        } else {
            self.queue_pos = Some(pos.saturating_sub(1));
            self.resolve_and_play(cx);
        }
        cx.notify();
    }

    fn play_random(&mut self, cx: &mut Context<Self>) {
        let len = self.queue.len();
        if len == 0 {
            return;
        }
        let current = self.queue_pos.unwrap_or(0);
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let mut next = (seed as usize) % len;
        if len > 1 && next == current {
            next = (next + 1) % len;
        }
        self.queue_pos = Some(next);
        self.resolve_and_play(cx);
        cx.notify();
    }

    fn stop_playback(&mut self, cx: &mut Context<Self>) {
        self.player.stop();
        self.queue_pos = None;
        self.play_gen = self.play_gen.wrapping_add(1);
        self.current_track = None;
        self.lyrics_status = None;
        if self.is_phone() {
            self.phone_page = PhonePage::Browse;
        }
        let old_cover = self.cover_image.take();
        self.retire_image(old_cover);
        self.lyric_view.update(cx, |view, _| {
            view.clear_lyrics();
            view.clear_album();
        });
        cx.notify();
    }

    fn toggle_source(&mut self, id: &SourceId, cx: &mut Context<Self>) {
        let registry = self.registry.clone();
        let id2 = id.clone();
        let is_netease = id.as_str() == "netease";
        if registry.get(id).is_some() {
            cx.spawn(async move |weak, cx| {
                let _ = registry.unload(&id2).await;
                let _ = weak.update(cx, |app, cx| {
                    if is_netease {
                        app.account_state = AccountState::Unknown;
                        let old_avatar = app.account_avatar.take();
                        app.retire_image(old_avatar);
                        app.user_playlists = Arc::new(Vec::new());
                        app.login_open = false;
                    }
                    if app.collection.as_ref().is_some_and(|c| c.id.source == id2) {
                        app.collection = None;
                        let old_cover = app.collection_cover.take();
                        app.retire_image(old_cover);
                        if app.view_tab == ViewTab::Collection {
                            app.view_tab = ViewTab::Library;
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        } else {
            // builder 捕获了预构建实例（本地源卸载后索引仍在内存，重载零成本）。
            let builder = cx.global::<Globals>().builders.get(id).cloned();
            if let Some(build) = builder {
                cx.spawn(async move |weak, cx| {
                    let result = registry.load(&id2, |info| build(info));
                    let _ = weak.update(cx, |app, cx| {
                        if let Err(e) = &result {
                            app.status_message = Some(format!("加载源失败：{e}"));
                        } else if is_netease {
                            app.spawn_netease_warmup(cx);
                            app.spawn_account_bootstrap(cx);
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    // ---------- 网易云账号 / 集合页 ----------

    fn netease_source(&self) -> Option<Arc<dyn MusicSource>> {
        self.registry.get(&SourceId::new("netease"))
    }

    fn save_cookie(&mut self, cookie: Option<String>) {
        let mut cfg = crate::config::load_or_default();
        cfg.netease.cookie = cookie;
        crate::config::save(&cfg);
    }

    fn spawn_account_bootstrap(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.netease_source() else {
            self.account_state = AccountState::Unknown;
            return;
        };
        let Some(account) = source.account() else {
            self.account_state = AccountState::Unsupported;
            return;
        };
        self.account_state = account.account_state();
        if account.cookie().is_none() {
            self.account_state = AccountState::LoggedOut;
            return;
        }
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            match account.refresh_account().await {
                Ok(Some(profile)) => {
                    let _ = weak.update(cx, |app, cx| {
                        if app.netease_source().is_none() {
                            return;
                        }
                        app.account_state = AccountState::LoggedIn(profile);
                        app.load_account_avatar(cx);
                        app.load_user_playlists(cx);
                        cx.notify();
                    });
                }
                Ok(None) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.account_state = AccountState::LoggedOut;
                        let old_avatar = app.account_avatar.take();
                        app.retire_image(old_avatar);
                        app.user_playlists = Arc::new(Vec::new());
                        app.save_cookie(None);
                        if app
                            .collection
                            .as_ref()
                            .is_some_and(|c| c.id.kind == CollectionKind::Playlist)
                        {
                            app.collection = None;
                            let old_cover = app.collection_cover.take();
                            app.retire_image(old_cover);
                            if app.view_tab == ViewTab::Collection {
                                app.view_tab = ViewTab::Library;
                            }
                        }
                        cx.notify();
                    });
                }
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.account_state = AccountState::Unknown;
                        app.info_message = Some(format!("账号校验失败：{e}"));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn spawn_netease_warmup(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.netease_source() else {
            return;
        };
        const PREPARING: &str = "正在准备网易云服务…首次运行会自动下载运行组件";
        self.info_message = Some(PREPARING.into());
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let result = source.warm_up().await;
            let _ = weak.update(cx, |app, cx| {
                match result {
                    Ok(()) => {
                        if app.info_message.as_deref() == Some(PREPARING) {
                            app.info_message = None;
                        }
                        // 旧版下载只写了音频流；服务就绪后原地补标签，
                        // 避免历史文件永远停留在“未知艺人/无封面/无歌词”。
                        app.spawn_download_metadata_repair(cx);
                    }
                    Err(e) => {
                        app.status_message = Some(format!("网易云服务准备失败：{e}"));
                        app.info_message = None;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 扫描下载索引，为缺少标题/艺人标签的历史下载补写元数据。
    ///
    /// 只处理“文件仍在、标签为空”的条目：重新取一次在线详情/歌词/封面，
    /// 复用下载落盘时的同一套写入逻辑，不重新下载音频。
    fn spawn_download_metadata_repair(&mut self, cx: &mut Context<Self>) {
        let downloads = self.downloads.clone();
        let registry = self.registry.clone();
        cx.spawn(async move |weak, cx| {
            let pending = cx
                .background_executor()
                .spawn({
                    let downloads = downloads.clone();
                    async move { downloads.entries_missing_metadata() }
                })
                .await;
            if pending.is_empty() {
                return;
            }

            let mut repaired: Vec<PathBuf> = Vec::new();
            for (id, path) in pending {
                let Some(source) = registry.get(&id.source) else {
                    continue;
                };
                let Ok(track) = source.track(&id).await else {
                    continue;
                };
                let lyrics = source
                    .lyrics(&id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|lyrics| lyrics_to_lrc(&lyrics));
                let cover_url = crate::lyrics::cover_url(&track);
                let downloads = downloads.clone();
                let path_for_task = path.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let cover = cover_url.and_then(crate::lyrics::fetch_cover_raw_blocking);
                        let extras = DownloadExtras { cover, lyrics };
                        downloads.enrich_file_blocking(&path_for_task, &track, &extras)
                    })
                    .await;
                if result.is_ok() {
                    repaired.push(path);
                }
            }
            if repaired.is_empty() {
                return;
            }

            let count = repaired.len();
            let _ = weak.update(cx, |app, cx| {
                app.info_message = Some(format!("已为 {count} 首已下载曲目补齐元数据"));
                if let Some(local) = app.local_source.clone()
                    && repaired
                        .iter()
                        .any(|path| local.roots().iter().any(|root| path.starts_with(root)))
                {
                    app.spawn_library_scan(local, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_user_playlists(&mut self, cx: &mut Context<Self>) {
        if self.playlists_loading {
            return;
        }
        let Some(source) = self.netease_source() else {
            return;
        };
        if source.account().is_none() {
            return;
        }
        self.playlists_loading = true;
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            let result = account.user_playlists().await;
            let _ = weak.update(cx, |app, cx| {
                app.playlists_loading = false;
                if !matches!(app.account_state, AccountState::LoggedIn(_)) {
                    return;
                }
                match result {
                    Ok(list) => {
                        app.user_playlists = Arc::new(list);
                        app.status_message = None;
                        // 登录后“正在加载用户歌单”的提示在数据到位后要收掉。
                        if app.info_message.as_deref() == Some("已登录网易云，正在加载用户歌单")
                        {
                            app.info_message = None;
                        }
                    }
                    Err(e) => {
                        app.user_playlists = Arc::new(Vec::new());
                        app.info_message = Some(format!("用户歌单加载失败：{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_login(&mut self, cx: &mut Context<Self>) {
        self.login_open = true;
        self.login_tab = LoginTab::Qr;
        self.start_login(cx);
    }

    fn close_login(&mut self, cx: &mut Context<Self>) {
        self.login_open = false;
        self.login_gen = self.login_gen.wrapping_add(1);
        self.login_challenge = None;
        let old_qr = self.login_qr.take();
        self.retire_image(old_qr);
        self.login_status = None;
        self.login_captcha_input
            .update(cx, |state, cx| state.clear(cx));
        cx.notify();
    }

    fn refresh_login(&mut self, cx: &mut Context<Self>) {
        if self.login_tab == LoginTab::Qr {
            self.start_login(cx);
        }
    }

    fn switch_login_tab(&mut self, tab: LoginTab, cx: &mut Context<Self>) {
        if self.login_tab == tab {
            return;
        }
        self.login_tab = tab;
        self.login_gen = self.login_gen.wrapping_add(1);
        self.login_challenge = None;
        let old_qr = self.login_qr.take();
        self.retire_image(old_qr);
        self.login_status = None;
        match tab {
            LoginTab::Qr => self.start_login(cx),
            LoginTab::Phone => {
                self.login_status = Some("输入手机号并获取验证码".into());
                cx.notify();
            }
        }
    }

    fn send_login_captcha(&mut self, cx: &mut Context<Self>) {
        if self
            .captcha_cooldown_until
            .is_some_and(|until| until > Instant::now())
        {
            return;
        }
        let phone = self.login_phone_input.read(cx).content().trim().to_string();
        if phone.chars().filter(|c| c.is_ascii_digit()).count() < 6 {
            self.login_status = Some("请输入正确的手机号".into());
            cx.notify();
            return;
        }
        let Some(source) = self.netease_source() else {
            self.login_status = Some("网易云源未加载".into());
            cx.notify();
            return;
        };
        self.login_status = Some(format!("正在向 {phone} 发送验证码…"));
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            match account.send_captcha(&phone).await {
                Ok(()) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.login_status = Some("验证码已发送".into());
                        app.captcha_cooldown_until = Some(Instant::now() + Duration::from_secs(60));
                        cx.notify();
                    });
                    cx.background_executor()
                        .timer(Duration::from_secs(61))
                        .await;
                    let _ = weak.update(cx, |app, cx| {
                        app.captcha_cooldown_until = None;
                        cx.notify();
                    });
                }
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.login_status = Some(format!("验证码发送失败：{e}"));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn submit_phone_login(&mut self, cx: &mut Context<Self>) {
        let phone = self.login_phone_input.read(cx).content().trim().to_string();
        let captcha = self
            .login_captcha_input
            .read(cx)
            .content()
            .trim()
            .to_string();
        if phone.is_empty() || captcha.is_empty() {
            self.login_status = Some("请输入手机号和验证码".into());
            cx.notify();
            return;
        }
        let Some(source) = self.netease_source() else {
            self.login_status = Some("网易云源未加载".into());
            cx.notify();
            return;
        };
        let generation = self.login_gen;
        self.login_status = Some("正在登录…".into());
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                return;
            };
            match account.login_cellphone(&phone, &captcha).await {
                Ok(profile) => {
                    let cookie = account.cookie();
                    let _ = weak.update(cx, |app, cx| {
                        if app.login_gen != generation || !app.login_open {
                            return;
                        }
                        app.account_state = AccountState::LoggedIn(profile);
                        app.save_cookie(cookie);
                        app.login_open = false;
                        app.login_gen = app.login_gen.wrapping_add(1);
                        app.login_status = None;
                        let old_qr = app.login_qr.take();
                        app.retire_image(old_qr);
                        app.login_challenge = None;
                        app.login_captcha_input
                            .update(cx, |state, cx| state.clear(cx));
                        app.info_message = Some("已登录网易云，正在加载用户歌单".into());
                        app.load_account_avatar(cx);
                        app.load_user_playlists(cx);
                        cx.notify();
                    });
                }
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.login_status = Some(format!("登录失败：{e}"));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn load_account_avatar(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.account_state.profile().cloned() else {
            let old = self.account_avatar.take();
            self.retire_image(old);
            return;
        };
        let Some(url) = profile.avatar.clone() else {
            let old = self.account_avatar.take();
            self.retire_image(old);
            return;
        };
        let cache = self.cache.clone();
        // 头像属于账号数据：单独 namespace，登出/卸载时随账号缓存一起回收。
        let namespace = SourceId::new(format!("netease-account-{}", profile.uid));
        cx.spawn(async move |weak, cx| {
            let decoded = cx
                .background_executor()
                .spawn(async move { fetch_cover_blocking(Some(cache), &namespace, url) })
                .await;
            if let Some((w, h, rgba)) = decoded {
                let _ = weak.update(cx, |app, cx| {
                    let old = app.account_avatar.replace(Self::rgba_to_image(w, h, &rgba));
                    app.retire_image(old);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn start_login(&mut self, cx: &mut Context<Self>) {
        self.login_gen = self.login_gen.wrapping_add(1);
        let generation = self.login_gen;
        self.login_challenge = None;
        let old_qr = self.login_qr.take();
        self.retire_image(old_qr);
        self.login_status = Some("正在获取二维码…".into());
        cx.notify();

        let Some(source) = self.netease_source() else {
            self.login_status = Some("网易云源未加载".into());
            cx.notify();
            return;
        };
        cx.spawn(async move |weak, cx| {
            let Some(account) = source.account() else {
                let _ = weak.update(cx, |app, cx| {
                    app.login_status = Some("该音源不支持登录".into());
                    cx.notify();
                });
                return;
            };
            let challenge = match account.login_begin(LoginMethod::Qr).await {
                Ok(challenge) => challenge,
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.login_status = Some(format!("获取二维码失败：{e}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let qr_url = challenge.qr_data_url.clone();
            let decoded = if let Some(url) = qr_url {
                cx.background_executor()
                    .spawn(async move { decode_data_url_image(&url) })
                    .await
            } else {
                None
            };
            let image = decoded.map(|(w, h, rgba)| Self::rgba_to_image(w, h, &rgba));
            let challenge_for_poll = challenge.clone();
            let ok = weak
                .update(cx, |app, cx| {
                    if app.login_gen != generation || !app.login_open {
                        return false;
                    }
                    app.login_challenge = Some(challenge);
                    let old_qr = std::mem::replace(&mut app.login_qr, image);
                    app.retire_image(old_qr);
                    app.login_status = Some(
                        if app.login_qr.is_some() {
                            "等待扫码"
                        } else {
                            "二维码图片解码失败，请刷新"
                        }
                        .into(),
                    );
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !ok {
                return;
            }

            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let alive = weak
                    .update(cx, |app, _| app.login_gen == generation && app.login_open)
                    .unwrap_or(false);
                if !alive {
                    return;
                }
                let progress = match account.login_poll(&challenge_for_poll).await {
                    Ok(progress) => progress,
                    Err(e) => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some(format!("登录轮询失败：{e}"));
                            cx.notify();
                        });
                        return;
                    }
                };
                match progress {
                    LoginProgress::Waiting => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some("等待扫码".into());
                            cx.notify();
                        });
                    }
                    LoginProgress::Scanned => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some("已扫描，请在手机确认".into());
                            cx.notify();
                        });
                    }
                    LoginProgress::Success(profile) => {
                        let cookie = account.cookie();
                        let _ = weak.update(cx, |app, cx| {
                            if app.login_gen != generation || !app.login_open {
                                return;
                            }
                            app.account_state = AccountState::LoggedIn(profile);
                            app.save_cookie(cookie);
                            app.login_open = false;
                            app.login_gen = app.login_gen.wrapping_add(1);
                            app.login_status = None;
                            let old_qr = app.login_qr.take();
                            app.retire_image(old_qr);
                            app.login_challenge = None;
                            app.info_message = Some("已登录网易云，正在加载用户歌单".into());
                            app.load_account_avatar(cx);
                            app.load_user_playlists(cx);
                            cx.notify();
                        });
                        return;
                    }
                    LoginProgress::Expired => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some("二维码已过期，点击刷新".into());
                            cx.notify();
                        });
                        return;
                    }
                    LoginProgress::Failed(e) => {
                        let _ = weak.update(cx, |app, cx| {
                            app.login_status = Some(format!("登录失败：{e}"));
                            cx.notify();
                        });
                        return;
                    }
                }
            }
        })
        .detach();
    }

    fn logout_account(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.netease_source() else {
            return;
        };
        self.info_message = Some("正在退出登录…".into());
        cx.notify();
        cx.spawn(async move |weak, cx| {
            if let Some(account) = source.account() {
                let _ = account.logout().await;
            }
            let _ = weak.update(cx, |app, cx| {
                app.account_state = AccountState::LoggedOut;
                let old_avatar = app.account_avatar.take();
                app.retire_image(old_avatar);
                app.user_playlists = Arc::new(Vec::new());
                app.save_cookie(None);
                if app
                    .collection
                    .as_ref()
                    .is_some_and(|c| c.id.kind == CollectionKind::Playlist)
                {
                    app.collection = None;
                    let old_cover = app.collection_cover.take();
                    app.retire_image(old_cover);
                    app.view_tab = ViewTab::Library;
                }
                app.info_message = Some("已退出网易云账号".into());
                cx.notify();
            });
        })
        .detach();
    }

    fn open_collection(&mut self, id: CollectionId, title: String, cx: &mut Context<Self>) {
        self.collection_gen = self.collection_gen.wrapping_add(1);
        let generation = self.collection_gen;
        self.view_tab = ViewTab::Collection;
        self.source_route = None;
        // 手机壳里专辑/歌手/歌单是曲库页的下一层，不能在播放页背后悄悄
        // 换内容，否则返回时看起来「没有反应」。
        self.show_phone_browse();
        let old_cover = self.collection_cover.take();
        self.retire_image(old_cover);
        self.collection = Some(CollectionView {
            id: id.clone(),
            title: title.clone(),
            subtitle: "正在加载…".into(),
            creator: None,
            description: None,
            tracks: Arc::new(Vec::new()),
            related_albums: Arc::new(Vec::new()),
            loading: true,
            error: None,
        });
        cx.notify();

        let Some(source) = self.registry.get(&id.source) else {
            self.collection.as_mut().unwrap().error = Some("音源未加载".into());
            return;
        };
        if source.catalog().is_none() {
            self.collection.as_mut().unwrap().error = Some("该音源不支持集合页".into());
            return;
        }
        let cache = self.cache.clone();
        let source_id = id.source.clone();
        cx.spawn(async move |weak, cx| {
            let Some(catalog) = source.catalog() else {
                return;
            };
            let result = catalog.collection(&id).await;
            let _ = weak.update(cx, |app, cx| {
                if app.collection_gen != generation {
                    return;
                }
                let Some(view) = app.collection.as_mut() else {
                    return;
                };
                match result {
                    Ok(page) => {
                        let summary = page.summary;
                        if let Some(summary) = &summary {
                            view.title = summary.title.clone();
                            view.creator = summary.creator.clone();
                            view.description = summary.description.clone();
                            view.subtitle = format!(
                                "{} · {} 首{}",
                                summary.kind.label(),
                                page.total,
                                summary
                                    .creator
                                    .as_ref()
                                    .map(|c| format!(" · {c}"))
                                    .unwrap_or_default()
                            );
                        } else {
                            view.subtitle = format!("{} 首", page.total);
                        }
                        view.tracks = Arc::new(page.tracks);
                        view.related_albums = Arc::new(page.related_albums);
                        view.loading = false;
                        view.error = None;
                        let cover = summary.and_then(|s| s.cover);
                        if let Some(cover) = cover {
                            let cache = cache.clone();
                            let ns = source_id.clone();
                            cx.spawn(async move |weak, cx| {
                                let decoded =
                                    cx.background_executor()
                                        .spawn(async move {
                                            fetch_cover_blocking(Some(cache), &ns, cover)
                                        })
                                        .await;
                                if let Some((w, h, rgba)) = decoded {
                                    let _ = weak.update(cx, |app, cx| {
                                        if app.collection_gen == generation {
                                            let image = Self::rgba_to_image(w, h, &rgba);
                                            let old = app.collection_cover.replace(image);
                                            app.retire_image(old);
                                            cx.notify();
                                        }
                                    });
                                }
                            })
                            .detach();
                        }
                    }
                    Err(e) => {
                        view.loading = false;
                        view.error = Some(e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_album_for_current(&mut self, cx: &mut Context<Self>) {
        let Some(track) = self.current_track.clone() else {
            return;
        };
        let Some(album) = track.album else {
            return;
        };
        let Some(id) = album.id else {
            return;
        };
        self.open_collection(
            CollectionId::new(track.id.source.clone(), CollectionKind::Album, id),
            album.name,
            cx,
        );
    }

    fn open_artist_for_current(&mut self, cx: &mut Context<Self>) {
        let Some(track) = self.current_track.clone() else {
            return;
        };
        let Some(artist) = track.artists.iter().find(|a| a.id.is_some()).cloned() else {
            return;
        };
        let Some(id) = artist.id else {
            return;
        };
        self.open_collection(
            CollectionId::new(track.id.source.clone(), CollectionKind::Artist, id),
            artist.name,
            cx,
        );
    }

    // ---------- 设置 / 下载 ----------

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = true;
        let stats = self.cache.stats();
        let runtime = SettingsRuntime {
            cache_root: stats.root,
            cache_used_bytes: stats.total_bytes,
            cache_limit_bytes: stats.limit_bytes,
            cache_entries: stats.entries,
            download_root: self.downloads.root().to_path_buf(),
            eq_name: self.eq.display_name(),
            eq_band_count: self.eq.settings.bands.len(),
            eq_enabled: self.eq.settings.enabled,
            backend: self.player.backend_name().to_string(),
        };
        self.settings_view
            .update(cx, |view, cx| view.sync(runtime, cx));
        cx.notify();
    }

    fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        cx.notify();
    }

    /// 设置中心的动作分发：能即时生效的在这里应用，只能下次启动生效的
    /// 只写配置，界面已用「重启生效」标注。
    fn handle_settings_action(
        &mut self,
        action: SettingsAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            SettingsAction::Close => self.close_settings(cx),
            SettingsAction::SetAppearance(mode) => {
                if self.appearance != mode {
                    self.toggle_appearance(cx);
                }
            }
            SettingsAction::SetSmoothScroll(on) => {
                self.smooth_scroll = on;
                let mut cfg = crate::config::load_or_default();
                cfg.smooth_scroll = on;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetSourcePreference(pref) => {
                self.set_source_preference(pref, cx);
            }
            SettingsAction::SetBeatBand(low, high) => self.commit_beat_band(low, high, cx),
            SettingsAction::SetBeatStrength(value) => self.commit_beat_strength(value, cx),
            SettingsAction::SetBackgroundRenderDivisor(divisor) => {
                self.commit_background_render_divisor(divisor, cx)
            }
            SettingsAction::SetLyricSweepFade(value) => self.commit_lyric_sweep_fade(value, cx),
            SettingsAction::SetEqEnabled(on) => self.set_eq_enabled(on, cx),
            SettingsAction::OpenEqualizer => {
                self.settings_open = false;
                self.open_eq(cx);
            }
            SettingsAction::SetCacheDir(path) => {
                self.apply_cache_dir(path.display().to_string(), cx);
            }
            SettingsAction::SetCacheLimit(bytes) => self.set_cache_limit_bytes(bytes, cx),
            SettingsAction::ClearCache => self.clear_cache_now(cx),
            SettingsAction::SetDownloadDir(path) => {
                self.apply_download_dir(path.display().to_string(), cx);
            }
            SettingsAction::SetSearchTimeout(ms) => {
                self.search_timeout_ms = ms.clamp(500, 60_000);
                let mut cfg = crate::config::load_or_default();
                cfg.search_timeout_ms = self.search_timeout_ms;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetNeteaseMode(mode) => {
                let mut cfg = crate::config::load_or_default();
                cfg.netease.mode = mode;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetNeteaseQuality(level) => {
                let mut cfg = crate::config::load_or_default();
                cfg.netease.level = Some(level);
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetNeteaseBaseUrl(url) => {
                let mut cfg = crate::config::load_or_default();
                cfg.netease.base_url = url;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::SetPlayerDevice(device) => {
                let mut cfg = crate::config::load_or_default();
                cfg.player_device = device;
                crate::config::save(&cfg);
                cx.notify();
            }
            SettingsAction::AddLocalRoot(path) => {
                let mut cfg = crate::config::load_or_default();
                if !cfg.local_roots.contains(&path) {
                    cfg.local_roots.push(path);
                    crate::config::save(&cfg);
                }
                let local = self.local_source.clone();
                if let Some(local) = local {
                    local.set_roots(cfg.local_roots.clone());
                    self.spawn_library_scan(local, cx);
                }
            }
            SettingsAction::RemoveLocalRoot(index) => {
                let mut cfg = crate::config::load_or_default();
                if cfg.local_roots.len() > 1 && index < cfg.local_roots.len() {
                    cfg.local_roots.remove(index);
                    crate::config::save(&cfg);
                    let local = self.local_source.clone();
                    if let Some(local) = local {
                        local.set_roots(cfg.local_roots.clone());
                        self.spawn_library_scan(local, cx);
                    }
                }
            }
            SettingsAction::RescanLibrary => {
                let local = self.local_source.clone();
                if let Some(local) = local {
                    self.spawn_library_scan(local, cx);
                }
            }
        }
    }

    fn apply_cache_dir(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let path = std::path::PathBuf::from(text);
        let mut cfg = crate::config::load_or_default();
        cfg.cache_dir = path.clone();
        crate::config::save(&cfg);
        let cache = self.cache.clone();
        let limit = cfg.cache_limit_bytes;
        let path_for_task = path.clone();
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .spawn(async move { cache.reconfigure(path_for_task, limit) })
                .await;
            let _ = weak.update(cx, |_app, cx| cx.notify());
        })
        .detach();
        self.info_message = Some(format!("缓存目录：{}", path.display()));
        cx.notify();
    }

    fn apply_download_dir(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let path = std::path::PathBuf::from(text);
        let mut cfg = crate::config::load_or_default();
        cfg.download_dir = path.clone();
        crate::config::save(&cfg);
        self.downloads = Arc::new(DownloadStore::open(path.clone()));
        self.info_message = Some(format!("下载目录：{}", path.display()));
        cx.notify();
    }

    fn set_cache_limit_bytes(&mut self, bytes: u64, cx: &mut Context<Self>) {
        let mut cfg = crate::config::load_or_default();
        cfg.cache_limit_bytes = bytes;
        crate::config::save(&cfg);
        let cache = self.cache.clone();
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .spawn(async move { cache.set_limit(bytes) })
                .await;
            let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            let _ = weak.update(cx, |app, cx| {
                app.info_message = Some(format!("缓存上限：{gb:.1} GB"));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn clear_cache_now(&mut self, cx: &mut Context<Self>) {
        let cache = self.cache.clone();
        let settings = self.settings_view.clone();
        self.info_message = Some("正在清空对象缓存…".into());
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let removed = cx
                .background_executor()
                .spawn(async move { cache.purge_all() })
                .await;
            let _ = weak.update(cx, |app, cx| {
                app.info_message = Some(format!("已清空对象缓存（{removed} 项）"));
                let stats = app.cache.stats();
                settings.update(cx, |view, cx| {
                    view.update_cache(
                        stats.total_bytes,
                        stats.limit_bytes,
                        stats.entries,
                        cx,
                    );
                    view.set_notice(format!("已清空对象缓存（{removed} 项）"), false, cx);
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn set_source_preference(&mut self, pref: SourcePreference, cx: &mut Context<Self>) {
        self.source_preference = pref;
        let mut cfg = crate::config::load_or_default();
        cfg.source_preference = pref;
        crate::config::save(&cfg);
        self.info_message = Some(format!("播放来源：{}", pref.label()));
        cx.notify();
    }

    fn cycle_source_preference(&mut self, cx: &mut Context<Self>) {
        self.set_source_preference(self.source_preference.next(), cx);
    }

    /// 设置面板提交：解析「下限 / 上限」两个输入框并应用频段。
    fn apply_beat_band(&mut self, cx: &mut Context<Self>) {
        let low_text = self.settings_beat_low_input.read(cx).content().to_string();
        let high_text = self.settings_beat_high_input.read(cx).content().to_string();
        let invalid =
            || "鼓点频段无效：需满足 20 ≤ 下限 < 上限 ≤ 22050 Hz（支持 4k / 10khz）".to_string();
        let (Some(low), Some(high)) = (parse_hz_input(&low_text), parse_hz_input(&high_text))
        else {
            self.status_message = Some(invalid());
            cx.notify();
            return;
        };
        let in_range = |v: f32| (20.0..=22_050.0).contains(&v);
        if !in_range(low) || !in_range(high) || high <= low {
            self.status_message = Some(invalid());
            cx.notify();
            return;
        }
        self.commit_beat_band(low, high, cx);
    }

    fn commit_beat_band(&mut self, low: f32, high: f32, cx: &mut Context<Self>) {
        let (low, high) = chordal_player::normalize_band(low, high);
        self.beat_band = (low, high);
        self.player.set_beat_band(low, high);
        let mut cfg = crate::config::load_or_default();
        cfg.beat_band_low_hz = low;
        cfg.beat_band_high_hz = high;
        crate::config::save(&cfg);
        self.settings_beat_low_input.update(cx, |state, cx| {
            state.set_content(format!("{low:.0}"), cx);
        });
        self.settings_beat_high_input.update(cx, |state, cx| {
            state.set_content(format!("{high:.0}"), cx);
        });
        self.info_message = Some(format!("背景鼓点频段：{low:.0}–{high:.0} Hz"));
        cx.notify();
    }

    fn set_beat_band_preset(&mut self, low: f32, high: f32, cx: &mut Context<Self>) {
        self.commit_beat_band(low, high, cx);
    }

    /// 应用背景鼓点的视觉倍率（0..=1.5），立即更新渲染器并持久化。
    fn commit_beat_strength(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = value.clamp(0.0, 1.5);
        self.lyric_view
            .update(cx, |view, _| view.set_background_beat_strength(value));
        let mut cfg = crate::config::load_or_default();
        cfg.beat_strength = value;
        crate::config::save(&cfg);
        self.info_message = Some(format!("背景鼓点强度：{:.0}%", value * 100.0));
        cx.notify();
    }

    /// 流体背景光栅缩放：输出分辨率 = 显示区域 ÷ 除数。
    fn background_render_scale(&self) -> f32 {
        1.0 / self.background_render_divisor.max(1) as f32
    }

    /// 应用流体背景分辨率除数（1..=20），立即更新渲染器并持久化。
    fn commit_background_render_divisor(&mut self, divisor: u32, cx: &mut Context<Self>) {
        let divisor = crate::config::clamp_background_render_divisor(divisor);
        self.background_render_divisor = divisor;
        let scale = self.background_render_scale();
        self.lyric_view
            .update(cx, |view, _| view.set_background_render_scale(scale));
        let mut cfg = crate::config::load_or_default();
        cfg.background_render_divisor = divisor;
        crate::config::save(&cfg);
        self.info_message = Some(format!("流体背景分辨率：1/{divisor}"));
        cx.notify();
    }

    /// 应用歌词整行扫光的渐变带宽度（em），立即重放当前模式的歌词样式并持久化。
    fn commit_lyric_sweep_fade(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = crate::config::clamp_lyric_sweep_fade_em(value);
        self.lyric_sweep_fade_em = value;
        let theme = Theme::get(cx).clone();
        // 手机播放页把歌词视图当背景，使用沉浸样式；其余按当前是否沉浸选择。
        let style = match self.phone_backdrop {
            PhoneBackdrop::Lyrics => phone_lyric_style(
                &theme,
                self.viewport_width,
                self.viewport_height,
                value,
            ),
            PhoneBackdrop::Player => {
                immersive_lyric_style(self.viewport_width, self.viewport_height, value)
            }
            PhoneBackdrop::Off if self.immersive => {
                immersive_lyric_style(self.viewport_width, self.viewport_height, value)
            }
            PhoneBackdrop::Off => compact_lyric_style(&theme, value),
        };
        self.lyric_view
            .update(cx, |view, _| view.set_text_style(style));
        let mut cfg = crate::config::load_or_default();
        cfg.lyric_sweep_fade_em = value;
        crate::config::save(&cfg);
        self.info_message = Some(format!("歌词扫光宽度：{value:.2}em"));
        cx.notify();
    }

    fn download_current(&mut self, cx: &mut Context<Self>) {
        // 队列里的条目是在线曲目；当前显示曲目可能已切换为本地同曲。
        let Some(track) = self
            .queue_pos
            .and_then(|pos| self.queue.get(pos).cloned())
            .or_else(|| self.current_track.clone())
        else {
            return;
        };
        if matches!(track.origin, TrackOrigin::Local { .. }) {
            self.info_message = Some("本地文件无需下载".into());
            cx.notify();
            return;
        }
        if self.downloads.path_for(&track.id).is_some() {
            self.info_message = Some("这首已经下载过了".into());
            cx.notify();
            return;
        }
        let Some(source) = self.registry.get(&track.id.source) else {
            self.status_message = Some("音源未加载，无法下载".into());
            cx.notify();
            return;
        };
        let downloads = self.downloads.clone();
        let title = track.title.clone();
        self.info_message = Some(format!("正在下载「{title}」…"));
        cx.notify();
        cx.spawn(async move |weak, cx| {
            // 搜索结果的专辑封面/序号可能缺失；下载前补一次详情，
            // 让写进文件的标签与在线页看到的一致。
            let detail = source.track(&track.id).await.ok();
            let track_for_download = detail.unwrap_or_else(|| track.clone());
            let lyrics_lrc = source
                .lyrics(&track.id)
                .await
                .ok()
                .flatten()
                .and_then(|lyrics| lyrics_to_lrc(&lyrics));
            let cover_url = crate::lyrics::cover_url(&track_for_download);

            let resolved = source.resolve_stream(&track.id).await;
            let (url, format_hint) = match resolved {
                Ok(resolved) => {
                    let hint = resolved.quality.as_ref().and_then(|q| q.codec.clone());
                    match resolved.handle {
                        StreamHandle::Url(url) => (url, hint),
                        StreamHandle::File(_) => {
                            let _ = weak.update(cx, |app, cx| {
                                app.info_message = Some("该曲目已经指向本地文件".into());
                                cx.notify();
                            });
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = weak.update(cx, |app, cx| {
                        app.status_message = Some(format!("下载失败：{e}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    // 封面字节只取一次：嵌入标签 + 写边车都由 DownloadExtras 消费。
                    let cover = cover_url.and_then(crate::lyrics::fetch_cover_raw_blocking);
                    let extras = DownloadExtras {
                        cover,
                        lyrics: lyrics_lrc,
                    };
                    downloads.download_blocking(
                        &track_for_download,
                        &url,
                        format_hint.as_deref(),
                        &extras,
                    )
                })
                .await;
            let _ = weak.update(cx, |app, cx| {
                match result {
                    Ok(path) => {
                        app.info_message = Some(format!("已下载：{}", path.display()));
                        app.status_message = None;
                        // 下载目录通常位于本地库根目录下：立即增量扫描，
                        // 让新文件带着标签出现在「本地音乐」里。
                        if let Some(local) = app.local_source.clone()
                            && local.roots().iter().any(|root| path.starts_with(root))
                        {
                            app.spawn_library_scan(local, cx);
                        }
                    }
                    Err(e) => app.status_message = Some(format!("下载失败：{e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---------- 歌词 / 封面 ----------

    /// 播放某曲目时异步补全元数据、歌词与封面。`lyrics_gen` 负责丢弃过期结果。
    fn load_track_extras(&mut self, track: Track, cx: &mut Context<Self>) {
        self.lyrics_gen = self.lyrics_gen.wrapping_add(1);
        let generation = self.lyrics_gen;
        let registry = self.registry.clone();
        let view = self.lyric_view.clone();
        let player = self.player.clone();
        let app_cache = self.cache.clone();
        self.lyrics_status = Some("正在加载歌词…".into());
        cx.notify();

        cx.spawn(async move |weak, cx| {
            let Some(source) = registry.get(&track.id.source) else {
                let _ = weak.update(cx, |app, cx| {
                    if app.lyrics_gen == generation {
                        app.lyrics_status = Some("音源未加载，无法获取歌词".into());
                        cx.notify();
                    }
                });
                return;
            };

            // 远端源补一次 song/detail：拿到专辑封面 / 时长等完整字段。
            let is_remote = !matches!(track.origin, TrackOrigin::Local { .. });
            let detail = if is_remote {
                source.track(&track.id).await.ok()
            } else {
                None
            };
            let effective = detail.clone().unwrap_or_else(|| track.clone());
            let duration = effective.duration_ms.or(track.duration_ms);

            // 封面下载与歌词请求并行：封面先查对象缓存，未命中再抓取；
            // 命中后后台做条件刷新（ETag/Last-Modified/内容摘要）。
            let cover_cache = app_cache.clone();
            let cover_ns = track.id.source.clone();
            let cover_task = cover_url(&effective).map(|url| {
                let cover_cache = cover_cache.clone();
                let cover_ns = cover_ns.clone();
                cx.background_executor()
                    .spawn(async move { fetch_cover_blocking(Some(cover_cache), &cover_ns, url) })
            });
            let lyrics = source.lyrics(&track.id).await;
            let cover = match cover_task {
                Some(task) => task.await,
                None => None,
            };

            let _ = weak.update(cx, |app, cx| {
                if app.lyrics_gen != generation {
                    return;
                }
                if let Some(detail) = detail {
                    app.current_track = Some(detail);
                }
                if let Some((w, h, rgba)) = cover {
                    let image = Self::rgba_to_image(w, h, &rgba);
                    let old = app.cover_image.replace(image);
                    app.retire_image(old);
                    view.update(cx, |view, _| view.set_album_rgba(w, h, rgba));
                } else {
                    let old = app.cover_image.take();
                    app.retire_image(old);
                    view.update(cx, |view, _| view.clear_album());
                }

                match lyrics {
                    Ok(Some(lyrics)) => {
                        let lines = to_amll_lyrics(&lyrics, duration);
                        let preview: Vec<PreviewLyric> = lines
                            .iter()
                            .filter_map(|line| {
                                let text = line
                                    .words
                                    .iter()
                                    .map(|w| w.word.as_str())
                                    .collect::<String>();
                                let text = SharedString::from(text.trim().to_string());
                                let translation =
                                    SharedString::from(line.translated_lyric.trim().to_string());
                                (!text.is_empty() || !translation.is_empty()).then_some(
                                    PreviewLyric {
                                        start_ms: line.start_time,
                                        text,
                                        translation,
                                    },
                                )
                            })
                            .collect();
                        app.lyric_preview = Arc::new(preview);
                        if lines.is_empty() {
                            view.update(cx, |view, _| view.clear_lyrics());
                            app.lyrics_status = Some("暂无歌词".into());
                        } else {
                            let count = lines.len();
                            let position = player.clock().0;
                            view.update(cx, |view, _| view.set_lyrics(lines, position));
                            app.lyrics_status = Some(format!("歌词 {count} 行"));
                        }
                    }
                    Ok(None) => {
                        view.update(cx, |view, _| view.clear_lyrics());
                        app.lyric_preview = Arc::new(Vec::new());
                        app.lyrics_status = Some("暂无歌词".into());
                    }
                    Err(err) => {
                        view.update(cx, |view, _| view.clear_lyrics());
                        app.lyric_preview = Arc::new(Vec::new());
                        app.lyrics_status = Some(format!("歌词加载失败：{err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 把不再绘制的图片排队，下一帧统一从 gpui sprite atlas 移除。
    ///
    /// `RenderImage` 的纹理一旦上传就不会随 `Arc` 释放自动回收；快速切歌时
    /// 每首封面都占一块图集，最终 `paint_image` 分配失败会表现为黑屏。
    fn retire_image(&mut self, image: Option<Arc<RenderImage>>) {
        if let Some(image) = image {
            self.retired_images.push(image);
        }
    }

    /// RGBA → gpui 需要的 BGRA `RenderImage`（不预乘）。
    fn rgba_to_image(w: u32, h: u32, rgba: &[u8]) -> Arc<RenderImage> {
        let mut bgra = rgba.to_vec();
        for px in bgra.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        let image = image::RgbaImage::from_raw(w, h, bgra).expect("cover dimensions");
        Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
    }

    // ---------- 渲染 ----------

    // ================= 布局断点 =================

    fn sidebar_width(&self) -> f32 {
        if self.viewport_width < 1180.0 {
            208.0
        } else {
            crate::theme::size::SIDEBAR
        }
    }

    fn list_width(&self) -> f32 {
        if self.viewport_width < 1180.0 {
            320.0
        } else {
            crate::theme::size::LIST
        }
    }

    fn dock_width(&self) -> f32 {
        (self.viewport_width - 32.0).min(840.0).max(520.0)
    }

    // ================= macOS 标题栏 =================

    fn render_titlebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let dark = theme.is_dark();
        // 中等宽度下状态消息最容易把标题栏挤爆；消息仍会通过列表/提示出现，
        // 标题栏只保留品牌与窗口操作。
        let show_messages = self.viewport_width >= 1180.0;

        let theme_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(
                    if dark {
                        crate::icons::Icon::Sun
                    } else {
                        crate::icons::Icon::Moon
                    },
                    14.0,
                    theme.text_dim,
                ),
                26.0,
                if dark { "切换浅色" } else { "切换深色" },
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_appearance(cx));
                },
            )
        };

        let settings_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(
                    crate::icons::Icon::Settings,
                    14.0,
                    theme.text_dim,
                ),
                26.0,
                "设置",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.open_settings(cx));
                },
            )
        };

        let center = Self::window_drag_area(
            div()
                .id("titlebar-drag")
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::OpenHand),
        );
        // Windows 的双击最大化由 DefWindowProc（HTCAPTION）原生处理，
        // 此处只给其余平台补上。
        #[cfg(not(target_os = "windows"))]
        let center = center.on_click(|event, window, _| {
            if event.click_count() == 2 {
                window.zoom_window();
            }
        });
        let center = center.child(
            div()
                .flex()
                .items_center()
                .gap(px(7.0))
                .child(crate::icons::icon(
                    crate::icons::Icon::Note,
                    12.0,
                    theme.accent,
                ))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_dim)
                        .child("Chordial"),
                ),
        );

        div()
            .h(px(crate::theme::size::TITLEBAR))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(14.0))
            .bg(theme.sidebar)
            .border_b_1()
            .border_color(theme.border)
            .child(Self::traffic_lights())
            .child(center)
            .when(show_messages, |d| {
                d.children(self.status_message.as_ref().map(|msg| {
                    div()
                        .max_w(px(360.0))
                        .text_size(px(10.5))
                        .text_color(theme.danger)
                        .text_ellipsis()
                        .child(SharedString::from(msg.clone()))
                        .into_any_element()
                }))
                .children(self.info_message.as_ref().map(|msg| {
                    div()
                        .max_w(px(360.0))
                        .text_size(px(10.5))
                        .text_color(theme.text_faint)
                        .text_ellipsis()
                        .child(SharedString::from(msg.clone()))
                        .into_any_element()
                }))
            })
            .child(settings_btn)
            .child(theme_btn)
            .into_any_element()
    }

    // ================= 左侧边栏 =================

    fn playlist_rows(&self) -> Arc<Vec<PlaylistRowText>> {
        let mut cache = self.playlist_rows.borrow_mut();
        match cache.as_ref() {
            Some((cached, rows)) if Arc::ptr_eq(cached, &self.user_playlists) => rows.clone(),
            _ => {
                let rows = Arc::new(
                    self.user_playlists
                        .iter()
                        .map(PlaylistRowText::new)
                        .collect::<Vec<_>>(),
                );
                *cache = Some((self.user_playlists.clone(), rows.clone()));
                rows
            }
        }
    }

    fn source_snapshot(&self) -> Arc<Vec<SourceSlotSnapshot>> {
        let version = self.registry.version();
        let mut cache = self.source_snapshot.borrow_mut();
        match cache.as_ref() {
            Some((cached, slots)) if *cached == version => slots.clone(),
            _ => {
                let slots = Arc::new(self.registry.snapshot());
                *cache = Some((version, slots.clone()));
                slots
            }
        }
    }

    /// 侧栏歌单保留缓存的输入版本。任何可能改变歌单行内容、位置或 hover
    /// 状态的输入都必须进入哈希，否则缓存的绘制范围会过期。
    fn sidebar_playlist_version(&self, mouse: gpui::Point<gpui::Pixels>) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (matches!(self.appearance, crate::theme::Appearance::Dark) as u8).hash(&mut hasher);
        self.user_playlists.as_ptr().hash(&mut hasher);
        self.playlists_loading.hash(&mut hasher);
        self.account_avatar
            .as_ref()
            .map(|image| Arc::as_ptr(image) as usize)
            .hash(&mut hasher);
        match &self.account_state {
            chordal_core::model::AccountState::Unsupported => 0u8.hash(&mut hasher),
            chordal_core::model::AccountState::LoggedOut => 1u8.hash(&mut hasher),
            chordal_core::model::AccountState::Unknown => 3u8.hash(&mut hasher),
            chordal_core::model::AccountState::LoggedIn(profile) => {
                2u8.hash(&mut hasher);
                profile.uid.hash(&mut hasher);
                profile.nickname.hash(&mut hasher);
                profile.avatar.hash(&mut hasher);
                profile.vip.hash(&mut hasher);
            }
        }
        if let Some(collection) = &self.collection {
            collection.id.hash(&mut hasher);
        }
        self.viewport_width.to_bits().hash(&mut hasher);
        self.viewport_height.to_bits().hash(&mut hasher);
        self.sidebar_width().to_bits().hash(&mut hasher);
        f32::from(self.sidebar_scroll.offset().y)
            .to_bits()
            .hash(&mut hasher);
        f32::from(mouse.x).to_bits().hash(&mut hasher);
        f32::from(mouse.y).to_bits().hash(&mut hasher);
        hasher.finish()
    }

    fn render_sidebar(&self, mouse: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let slots = self.source_snapshot();
        let loaded = slots
            .iter()
            .filter(|s| s.status == SourceStatus::Loaded)
            .count();
        let width = self.sidebar_width();
        let netease_loaded = slots
            .iter()
            .any(|s| s.info.id.as_str() == "netease" && s.status == SourceStatus::Loaded);
        let dock_left = ((self.viewport_width - self.dock_width()) / 2.0).max(8.0);
        let bottom_pad: f32 = if dock_left < width + 12.0 { 88.0 } else { 10.0 };

        let section = |label: &'static str| {
            div()
                .h(px(28.0))
                .px(px(10.0))
                .pb(px(6.0))
                .flex()
                .items_end()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_faint)
                .child(label)
                .into_any_element()
        };

        let nav_row = |id: &'static str,
                       label: &'static str,
                       count: String,
                       active: bool,
                       icon: crate::icons::Icon,
                       weak: gpui::WeakEntity<ChordialApp>,
                       target: LibraryFilter| {
            let fg = theme.text;
            div()
                .id(id)
                .h(px(32.0))
                .px(px(9.0))
                .rounded(px(7.0))
                .flex()
                .items_center()
                .gap(px(9.0))
                .bg(if active {
                    theme.active
                } else {
                    gpui::transparent_black()
                })
                .hover(|s| s.bg(theme.hover))
                .child(crate::icons::icon(icon, 14.0, fg))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.5))
                        .font_weight(if active {
                            gpui::FontWeight::MEDIUM
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(fg)
                        .text_ellipsis()
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(if active {
                            theme.text_dim
                        } else {
                            theme.text_faint
                        })
                        .child(count),
                )
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.library_filter = target;
                        app.source_route = None;
                        app.view_tab = ViewTab::Library;
                        cx.notify();
                    });
                })
        };

        let mut list = div().flex().flex_col().gap(px(2.0));
        list = list
            .child(section("音乐"))
            .child(nav_row(
                "nav-all",
                "所有音乐",
                self.library.len().to_string(),
                self.library_filter == LibraryFilter::All && self.view_tab == ViewTab::Library,
                crate::icons::Icon::Note,
                weak.clone(),
                LibraryFilter::All,
            ))
            .child(nav_row(
                "nav-queue",
                "播放队列",
                self.queue.len().to_string(),
                self.library_filter == LibraryFilter::Queue && self.view_tab == ViewTab::Library,
                crate::icons::Icon::Queue,
                weak.clone(),
                LibraryFilter::Queue,
            ))
            .child(div().h(px(6.0)))
            .child(section("音乐源"));

        // 全部音源路由。
        {
            let active = self.source_route.is_none() && self.view_tab == ViewTab::Search;
            let weak = weak.clone();
            list = list.child(
                div()
                    .id("route-all")
                    .h(px(32.0))
                    .px(px(9.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .bg(if active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|s| s.bg(theme.hover))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Search,
                        14.0,
                        if active { theme.text } else { theme.text_dim },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.5))
                            .font_weight(if active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child("全部音源"),
                    )
                    .on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.source_route = None;
                            app.view_tab = ViewTab::Search;
                            focus_input(&app.search_input, window, cx);
                            cx.notify();
                        });
                    }),
            );
        }

        for slot in slots.iter() {
            let source_id = slot.info.id.clone();
            let route_active =
                self.view_tab == ViewTab::Search && self.source_route.as_ref() == Some(&source_id);
            let loaded_now = slot.status == SourceStatus::Loaded;
            let local = source_id.0 == "local";
            let dot = match &slot.status {
                SourceStatus::Loaded => theme.ok,
                SourceStatus::Registered => theme.text_faint,
                SourceStatus::Failed(_) => theme.danger,
            };
            let count = if local {
                self.library.len()
            } else {
                self.search_tracks
                    .iter()
                    .filter(|t| t.id.source == source_id)
                    .count()
            };
            let weak_route = weak.clone();
            let weak_toggle = weak.clone();
            let route_id = source_id.clone();
            let toggle_id = source_id.clone();
            let icon = if local {
                crate::icons::Icon::Drive
            } else {
                crate::icons::Icon::Cloud
            };

            list = list.child(
                div()
                    .id(SharedString::from(format!("src-{}", source_id)))
                    .h(px(32.0))
                    .px(px(9.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .bg(if route_active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|s| s.bg(theme.hover))
                    .on_click(move |_, window, cx| {
                        let id = route_id.clone();
                        let _ = weak_route.update(cx, |app, cx| {
                            app.source_route = Some(id);
                            app.view_tab = ViewTab::Search;
                            focus_input(&app.search_input, window, cx);
                            cx.notify();
                        });
                    })
                    .child(crate::icons::icon(
                        icon,
                        14.0,
                        if route_active {
                            theme.text
                        } else {
                            theme.text_dim
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.5))
                            .font_weight(if route_active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(SharedString::from(slot.info.display_name.clone())),
                    )
                    .when(count > 0, |d| {
                        d.child(
                            div()
                                .text_size(px(9.5))
                                .text_color(theme.text_faint)
                                .child(SharedString::from(count.to_string())),
                        )
                    })
                    .child(div().size(px(5.0)).rounded(px(3.0)).bg(dot))
                    .child(
                        div()
                            .id(SharedString::from(format!("src-toggle-{}", toggle_id)))
                            .h(px(18.0))
                            .px(px(5.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .text_size(px(9.5))
                            .text_color(theme.text_faint)
                            .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                            .child(if loaded_now { "卸载" } else { "加载" })
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = weak_toggle
                                    .update(cx, |app, cx| app.toggle_source(&toggle_id, cx));
                            }),
                    ),
            );
        }

        // 账号 / 我的歌单（只在网易云源已加载时出现）。
        if netease_loaded {
            list = list.child(div().h(px(6.0))).child(section("网易云账号"));
            match self.account_state.clone() {
                AccountState::LoggedIn(profile) => {
                    let weak_logout = weak.clone();
                    let avatar_el: AnyElement = match &self.account_avatar {
                        Some(image) => {
                            let placeholder = div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(theme.surface_alt)
                                .child(crate::icons::icon(
                                    crate::icons::Icon::Cloud,
                                    12.0,
                                    theme.text_faint,
                                ))
                                .into_any_element();
                            crate::widgets::cover_thumb(
                                &theme,
                                Some(image.clone()),
                                28.0,
                                14.0,
                                placeholder,
                            )
                        }
                        None => crate::icons::icon(crate::icons::Icon::Cloud, 14.0, theme.ok),
                    };
                    list = list.child(
                        div()
                            .id("account-row")
                            .h(px(34.0))
                            .px(px(9.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(avatar_el)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .text_ellipsis()
                                            .child(SharedString::from(profile.nickname.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.5))
                                            .text_color(theme.text_faint)
                                            .child(SharedString::from(if profile.vip {
                                                "VIP · 已登录"
                                            } else {
                                                "已登录"
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("account-logout")
                                    .h(px(20.0))
                                    .px(px(6.0))
                                    .rounded(px(4.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .flex()
                                    .items_center()
                                    .text_size(px(9.5))
                                    .text_color(theme.text_faint)
                                    .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                                    .child("退出")
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        let _ = weak_logout
                                            .update(cx, |app, cx| app.logout_account(cx));
                                    }),
                            ),
                    );
                    list = list.child(div().h(px(4.0))).child(section("我的歌单"));
                    if self.playlists_loading {
                        list = list.child(
                            div()
                                .px(px(9.0))
                                .py(px(4.0))
                                .text_size(px(10.0))
                                .text_color(theme.text_faint)
                                .child("正在加载歌单…"),
                        );
                    } else if self.user_playlists.is_empty() {
                        list = list.child(
                            div()
                                .px(px(9.0))
                                .py(px(4.0))
                                .text_size(px(10.0))
                                .text_color(theme.text_faint)
                                .child("暂无歌单"),
                        );
                    } else {
                        let playlist_rows = self.playlist_rows();
                        let row_count = playlist_rows.len().min(30);
                        let mut style = gpui::Style::default();
                        style.size.width = gpui::relative(1.0).into();
                        style.size.height = px((row_count as f32) * 28.0).into();
                        style.flex_shrink = 0.0;

                        let version = self.sidebar_playlist_version(mouse);
                        let rows = playlist_rows;
                        let theme_rows = theme.clone();
                        let weak_rows = weak.clone();
                        let active_id = self.collection.as_ref().map(|collection| collection.id.clone());
                        list = list.child(gpui::Retained::new(
                            "sidebar-playlists",
                            version,
                            style,
                            move |_window, _cx| {
                                let mut column = div().w_full().flex().flex_col();
                                for row in rows.iter().take(row_count) {
                                    let weak = weak_rows.clone();
                                    let active = active_id.as_ref() == Some(&row.id);
                                    column = column.child(
                                        div()
                                            .id(row.element_id.clone())
                                            .h(px(28.0))
                                            .px(px(9.0))
                                            .rounded(px(6.0))
                                            .flex()
                                            .items_center()
                                            .gap(px(7.0))
                                            .bg(if active {
                                                theme_rows.active
                                            } else {
                                                gpui::transparent_black()
                                            })
                                            .hover(|s| s.bg(theme_rows.hover))
                                            .cursor_pointer()
                                            .child(crate::icons::icon(
                                                crate::icons::Icon::Layers,
                                                12.0,
                                                if active {
                                                    theme_rows.text
                                                } else {
                                                    theme_rows.text_faint
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_size(px(11.5))
                                                    .text_color(theme_rows.text)
                                                    .text_ellipsis()
                                                    .child(row.title.clone()),
                                            )
                                            .when(!row.count.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .text_size(px(9.5))
                                                        .text_color(theme_rows.text_faint)
                                                        .child(row.count.clone()),
                                                )
                                            })
                                            .on_click({
                                                let id = row.id.clone();
                                                let title = row.title.to_string();
                                                move |_, _, cx| {
                                                    let _ = weak.update(cx, |app, cx| {
                                                        app.open_collection(
                                                            id.clone(),
                                                            title.clone(),
                                                            cx,
                                                        );
                                                    });
                                                }
                                            }),
                                    );
                                }
                                column.into_any_element()
                            },
                        ));
                    }
                }
                AccountState::Unknown => {
                    let weak = weak.clone();
                    list = list.child(
                        div()
                            .id("account-retry")
                            .px(px(9.0))
                            .py(px(4.0))
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.text))
                            .child("正在校验账号 · 点击重试")
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.spawn_account_bootstrap(cx);
                                    cx.notify();
                                });
                            }),
                    );
                }
                _ => {
                    let weak = weak.clone();
                    list = list.child(
                        div()
                            .id("account-login")
                            .h(px(32.0))
                            .px(px(9.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.hover))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Cloud,
                                14.0,
                                theme.text_dim,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.5))
                                    .text_color(theme.text)
                                    .child("登录网易云"),
                            )
                            .child(crate::icons::icon(
                                crate::icons::Icon::ChevronDown,
                                10.0,
                                theme.text_faint,
                            ))
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| app.open_login(cx));
                            }),
                    );
                }
            }
        }

        // 播放模式。
        list = list.child(div().h(px(6.0))).child(section("播放模式"));
        for (mode, label, icon) in [
            (PlayMode::Sequential, "顺序播放", crate::icons::Icon::Menu),
            (PlayMode::RepeatAll, "列表循环", crate::icons::Icon::Repeat),
            (PlayMode::Shuffle, "随机播放", crate::icons::Icon::Shuffle),
            (PlayMode::RepeatOne, "单曲循环", crate::icons::Icon::Repeat),
        ] {
            let active = self.play_mode == mode;
            let weak = weak.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("mode-{label}")))
                    .h(px(32.0))
                    .px(px(9.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .hover(|s| s.bg(theme.hover))
                    .child(crate::icons::icon(
                        icon,
                        14.0,
                        if active {
                            theme.text_dim
                        } else {
                            theme.text_faint
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .font_weight(if active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(if active { theme.text } else { theme.text_dim })
                            .child(label),
                    )
                    .when(active, |d| {
                        d.child(crate::icons::icon(
                            crate::icons::Icon::Check,
                            11.0,
                            theme.text_dim,
                        ))
                    })
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.play_mode = mode;
                            cx.notify();
                        });
                    }),
            );
        }

        let footer_note = if self.library_scanning {
            "正在扫描本地库…".to_string()
        } else {
            format!("{loaded} 个音源已加载 · Ctrl K 搜索")
        };

        div()
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .track_scroll(&self.sidebar_scroll)
                    .overflow_y_scroll()
                    .when(self.smooth_scroll, |d| d.smooth_scroll())
                    .p(px(10.0))
                    .child(list),
            )
            .child(
                div()
                    .mx(px(10.0))
                    .mb(px(bottom_pad.max(10.0)))
                    .p(px(10.0))
                    .rounded(px(9.0))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface_alt)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.0))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Drive,
                                13.0,
                                theme.text_dim,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(11.5))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child("本地库"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.text_faint)
                                    .child(SharedString::from(format!(
                                        "{} 首",
                                        self.library.len()
                                    ))),
                            )
                            .child({
                                let weak = weak.clone();
                                crate::widgets::icon_button(
                                    &theme,
                                    crate::icons::icon(
                                        crate::icons::Icon::Settings,
                                        13.0,
                                        theme.text_dim,
                                    ),
                                    22.0,
                                    "设置",
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |app, cx| app.open_settings(cx));
                                    },
                                )
                            }),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(SharedString::from(footer_note)),
                    ),
            )
            .into_any_element()
    }

    // ================= 面板标题 =================

    /// 手机曲库页的页头：返回播放 + 标题 + 动作。
    /// 桌面页头保留 17px 标题；手机没有右栏，页头必须让出宽度给返回键。
    fn render_phone_pane_header(
        &self,
        theme: &Theme,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        right: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        // 没有当前曲目时播放页是空的；此时不摆一个按了没反应的返回键。
        let back: AnyElement = if self.current_track.is_some() {
            let weak = weak.clone();
            crate::widgets::icon_button(
                theme,
                crate::icons::icon(crate::icons::Icon::ChevronLeft, 15.0, theme.text_dim),
                30.0,
                "返回播放",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Player;
                        cx.notify();
                    });
                },
            )
        } else {
            div().w(px(30.0)).h(px(30.0)).into_any_element()
        };
        div()
            .h(px(54.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
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
                            .text_size(px(15.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(title.into()),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(subtitle.into()),
                    ),
            )
            .children(right)
            .into_any_element()
    }

    fn render_pane_header(
        &self,
        theme: &Theme,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        right: Option<AnyElement>,
    ) -> AnyElement {
        div()
            .h(px(crate::theme::size::PANE_HEAD))
            .px(px(16.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(title.into()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(subtitle.into()),
                    ),
            )
            .children(right)
            .into_any_element()
    }

    // ================= 曲目列表（窄栏） =================

    fn render_track_row(
        theme: &Theme,
        index: usize,
        row: &TrackRowText,
        list: QueueList,
        queue_pos: Option<usize>,
        queue_current_id: Option<&chordal_core::model::TrackId>,
        weak: gpui::WeakEntity<ChordialApp>,
    ) -> AnyElement {
        let is_current = queue_pos == Some(index) && queue_current_id == Some(&row.id);
        let number = if is_current {
            crate::widgets::equalizer(&theme, true)
        } else {
            div()
                .text_size(px(10.5))
                .text_color(theme.text_faint)
                .child(row.number.clone())
                .into_any_element()
        };
        let subtitle = match list {
            QueueList::Search => row.subtitle_search.clone(),
            _ => row.subtitle_default.clone(),
        };

        div()
            .id(gpui::ElementId::named_usize("track", index))
            .w_full()
            .h(px(crate::theme::size::ROW))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(7.0))
            .bg(if is_current {
                theme.selection
            } else {
                gpui::transparent_black()
            })
            .hover(|s| s.bg(theme.hover))
            .on_click(move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.play_from_list(list, index, cx));
            })
            .child(div().w(px(18.0)).flex().justify_center().child(number))
            .child(
                div()
                    .size(px(34.0))
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.surface_alt)
                    .child(crate::icons::icon(
                        crate::icons::Icon::Note,
                        14.0,
                        if is_current {
                            theme.text_dim
                        } else {
                            theme.text_faint
                        },
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(if is_current {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(row.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(subtitle),
                    ),
            )
            .child(
                div()
                    .w(px(40.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_faint)
                    .text_align(gpui::TextAlign::Right)
                    .child(row.duration.clone()),
            )
            .into_any_element()
    }

    fn render_track_header(theme: &Theme) -> AnyElement {
        div()
            .w_full()
            .h(px(26.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .border_b_1()
            .border_color(theme.border)
            .child(div().w(px(18.0)))
            .child(div().w(px(34.0)))
            .child(
                div()
                    .flex_1()
                    .text_size(px(10.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text_faint)
                    .child("标题"),
            )
            .child(
                div()
                    .w(px(40.0))
                    .text_size(px(10.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text_faint)
                    .text_align(gpui::TextAlign::Right)
                    .child("时长"),
            )
            .into_any_element()
    }

    fn render_track_table(
        &self,
        theme: &Theme,
        tracks: Arc<Vec<Track>>,
        list: QueueList,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let n = tracks.len();
        let queue_pos = self.queue_pos;
        let queue = self.queue.clone();
        let current_id = queue_pos.and_then(|i| queue.get(i).map(|t| t.id.clone()));
        let rows = {
            let mut cache = self.row_text_cache.borrow_mut();
            match cache.as_ref() {
                Some((cached_tracks, rows)) if Arc::ptr_eq(cached_tracks, &tracks) => rows.clone(),
                _ => {
                    let rows = Arc::new(
                        tracks
                            .iter()
                            .enumerate()
                            .map(|(index, track)| TrackRowText::new(index, track))
                            .collect::<Vec<_>>(),
                    );
                    *cache = Some((tracks.clone(), rows.clone()));
                    rows
                }
            }
        };
        let weak = cx.entity().downgrade();
        let row_theme = theme.clone();
        let mut body = uniform_list("track-list", n, move |range, _window, _cx| {
            range
                .filter_map(|i| {
                    rows.get(i).map(|row| {
                        Self::render_track_row(
                            &row_theme,
                            i,
                            row,
                            list,
                            queue_pos,
                            current_id.as_ref(),
                            weak.clone(),
                        )
                    })
                })
                .collect::<Vec<_>>()
        });
        if self.smooth_scroll {
            body = body.smooth_scroll();
        }
        let body: AnyElement = body.flex_1().min_h_0().into_any_element();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(Self::render_track_header(theme))
            .child(body)
            .into_any_element()
    }

    // ================= 中间：曲目板 =================

    /// 集合页头部：封面 + 创建者/简介 + 歌手页专辑快捷入口。
    fn render_collection_header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = &self.collection else {
            return div().into_any_element();
        };
        let weak = cx.entity().downgrade();
        let size = 64.0;
        let placeholder = div()
            .size_full()
            .bg(theme.surface_alt)
            .flex()
            .items_center()
            .justify_center()
            .child(crate::icons::icon(
                crate::icons::Icon::Layers,
                20.0,
                theme.text_faint,
            ))
            .into_any_element();
        let cover = crate::widgets::cover_thumb(
            theme,
            self.collection_cover.clone(),
            size,
            8.0,
            placeholder,
        );
        let mut info = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(3.0))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_dim)
                    .text_ellipsis()
                    .child(SharedString::from(
                        view.creator
                            .clone()
                            .filter(|c| !c.is_empty())
                            .unwrap_or_else(|| view.id.kind.label().to_string()),
                    )),
            )
            .when_some(
                view.description
                    .clone()
                    .filter(|d| !d.trim().is_empty())
                    .map(|d| d.lines().next().unwrap_or("").to_string()),
                |d, line| {
                    d.child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(SharedString::from(line)),
                    )
                },
            )
            .child(
                div().flex().items_center().gap(px(5.0)).mt(px(3.0)).child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme.text_faint)
                        .child(SharedString::from(format!(
                            "{} 首 · {}",
                            view.tracks.len(),
                            view.id.kind.label()
                        ))),
                ),
            );

        if !view.related_albums.is_empty() {
            let mut chips = div().flex().flex_wrap().gap(px(4.0)).mt(px(4.0));
            for album in view.related_albums.iter().take(8) {
                let weak = weak.clone();
                let id = album.id.clone();
                let title = album.title.clone();
                chips = chips.child(
                    div()
                        .id(SharedString::from(format!("coll-album-{}", album.id.id)))
                        .h(px(20.0))
                        .px(px(6.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface_alt)
                        .flex()
                        .items_center()
                        .text_size(px(9.5))
                        .text_color(theme.text_dim)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.hover).text_color(theme.text))
                        .child(SharedString::from(album.title.clone()))
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.open_collection(id.clone(), title.clone(), cx);
                            });
                        }),
                );
            }
            info = info.child(chips);
        }

        div()
            .px(px(14.0))
            .pb(px(10.0))
            .flex()
            .items_start()
            .gap(px(10.0))
            .child(cover)
            .child(info)
            .into_any_element()
    }

    fn render_search_tabs(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(12.0))
            .pb(px(6.0));
        for (kind, label) in [
            (SearchKind::Track, "歌曲"),
            (SearchKind::Album, "专辑"),
            (SearchKind::Artist, "歌手"),
            (SearchKind::Playlist, "歌单"),
        ] {
            let active = self.search_kind == kind;
            let weak = weak.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("search-kind-{label}")))
                    .h(px(24.0))
                    .px(px(9.0))
                    .rounded(px(5.0))
                    .bg(if active {
                        theme.active
                    } else {
                        gpui::transparent_black()
                    })
                    .flex()
                    .items_center()
                    .text_size(px(10.5))
                    .font_weight(if active {
                        gpui::FontWeight::MEDIUM
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(if active { theme.text } else { theme.text_faint })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover).text_color(theme.text))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.set_search_kind(kind, cx));
                    }),
            );
        }
        row.into_any_element()
    }

    fn render_collection_results(
        &self,
        theme: &Theme,
        collections: Arc<Vec<CollectionSummary>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme.clone();
        let weak = cx.entity().downgrade();
        let n = collections.len();
        let mut results = uniform_list("collection-results", n, move |range, _window, _cx| {
            range
                .filter_map(|i| {
                    let collection = collections.get(i)?;
                    let weak = weak.clone();
                    let source_label = match collection.id.source.as_str() {
                        "netease" => "网易云",
                        "local" => "本地",
                        other => other,
                    };
                    let meta = match (collection.creator.as_deref(), collection.track_count) {
                        (Some(creator), Some(count)) => {
                            format!("{} · {} · {count} 首", collection.kind.label(), creator)
                        }
                        (Some(creator), None) => {
                            format!("{} · {creator}", collection.kind.label())
                        }
                        (None, Some(count)) => {
                            format!("{} · {count} 首", collection.kind.label())
                        }
                        (None, None) => collection.kind.label().to_string(),
                    };
                    let id = collection.id.clone();
                    let title = collection.title.clone();
                    Some(
                        div()
                            .id(SharedString::from(format!(
                                "coll-{}-{:?}-{}",
                                collection.id.source, collection.id.kind, collection.id.id
                            )))
                            .w_full()
                            .h(px(56.0))
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .rounded(px(7.0))
                            .hover(|s| s.bg(theme.hover))
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.open_collection(id.clone(), title.clone(), cx);
                                });
                            })
                            .child(
                                div()
                                    .size(px(34.0))
                                    .rounded(px(5.0))
                                    .bg(theme.surface_alt)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(crate::icons::icon(
                                        crate::icons::Icon::Layers,
                                        14.0,
                                        theme.text_faint,
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.0))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .text_color(theme.text)
                                            .text_ellipsis()
                                            .child(SharedString::from(collection.title.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(theme.text_faint)
                                            .text_ellipsis()
                                            .child(SharedString::from(meta)),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(theme.text_faint)
                                    .child(SharedString::from(source_label.to_string())),
                            )
                            .into_any_element(),
                    )
                })
                .collect::<Vec<_>>()
        });
        if self.smooth_scroll {
            results = results.smooth_scroll();
        }
        results.flex_1().min_h_0().into_any_element()
    }

    fn render_collection_search_panel(
        &self,
        width: Option<f32>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let phone = self.is_phone();
        let visible: Arc<Vec<CollectionSummary>> = match &self.source_route {
            Some(route) => Arc::new(
                self.search_collections
                    .iter()
                    .filter(|c| &c.id.source == route)
                    .cloned()
                    .collect(),
            ),
            None => self.search_collections.clone(),
        };
        let route_label = self.source_route.as_ref().map(|id| {
            self.registry
                .snapshot()
                .into_iter()
                .find(|s| &s.info.id == id)
                .map(|s| s.info.display_name)
                .unwrap_or_else(|| id.0.clone())
        });
        let subtitle = if self.searching {
            "正在向已加载的音源请求…".to_string()
        } else if visible.is_empty() {
            if self.search_input.read(cx).content().trim().is_empty() {
                "选择类型后输入关键词回车搜索".to_string()
            } else {
                format!("没有找到{}", self.search_kind_label())
            }
        } else {
            let mut s = format!(
                "{} 个{} · 聚合 {}ms",
                visible.len(),
                self.search_kind_label(),
                self.search_elapsed_ms
            );
            if let Some(route) = route_label {
                s.push_str(&format!(" · {route}"));
            }
            s
        };

        let mut actions = div().flex().items_center().gap(px(6.0));
        if self.source_route.is_some() {
            let weak = weak.clone();
            actions = actions.child(
                div()
                    .id("clear-collection-route")
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_dim)
                    .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Close,
                        9.0,
                        theme.text_faint,
                    ))
                    .child("全部音源")
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.source_route = None;
                            cx.notify();
                        });
                    }),
            );
        }

        let body: AnyElement = if visible.is_empty() {
            let (icon, title, hint) = if self.searching {
                (
                    crate::icons::Icon::Refresh,
                    "正在搜索",
                    "正在向已加载的音源请求结果",
                )
            } else if self.search_input.read(cx).content().trim().is_empty() {
                (
                    crate::icons::Icon::Search,
                    "搜索收藏",
                    "按专辑、歌手或歌单查找，回车搜索",
                )
            } else {
                (
                    crate::icons::Icon::Alert,
                    "没有找到结果",
                    "换个关键词，或切换搜索类型",
                )
            };
            div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(7.0))
                .pb(px(40.0))
                .child(crate::icons::icon(icon, 22.0, theme.text_faint))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(title),
                )
                .child(
                    div()
                        .max_w(px(240.0))
                        .text_center()
                        .text_size(px(11.0))
                        .line_height(px(16.0))
                        .text_color(theme.text_faint)
                        .child(hint),
                )
                .into_any_element()
        } else {
            self.render_collection_results(theme, visible, cx)
        };

        let header = if phone {
            self.render_phone_pane_header(
                theme,
                "搜索",
                subtitle,
                Some(actions.into_any_element()),
                cx,
            )
        } else {
            self.render_pane_header(theme, "搜索", subtitle, Some(actions.into_any_element()))
        };

        div()
            .h_full()
            .when_some(width, |d, w| d.w(px(w)))
            .when(width.is_none(), |d| d.flex_1())
            .min_w(px(280.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.surface)
            .when(!phone, |d| d.border_r_1().border_color(theme.border))
            .child(header)
            .child(self.render_search_tabs(theme, cx))
            .child(
                div()
                    .px(px(12.0))
                    .pb(px(8.0))
                    .child(self.search_input_view.clone()),
            )
            .child(body)
            .child(
                div()
                    .h(px(if phone { 16.0 } else { 84.0 }))
                    .flex_none(),
            )
            .into_any_element()
    }

    fn search_kind_label(&self) -> &'static str {
        match self.search_kind {
            SearchKind::Track => "歌曲",
            SearchKind::Album => "专辑",
            SearchKind::Artist => "歌手",
            SearchKind::Playlist => "歌单",
        }
    }

    fn render_list_panel(&self, width: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let phone = self.is_phone();

        if self.view_tab == ViewTab::Search && self.search_kind != SearchKind::Track {
            return self.render_collection_search_panel(width, &theme, cx);
        }

        let (tracks, list, title, subtitle): (Arc<Vec<Track>>, QueueList, String, String) =
            match self.view_tab {
                ViewTab::Search => {
                    let visible: Arc<Vec<Track>> = match &self.source_route {
                        Some(route) => {
                            let mut cache = self.search_route_cache.borrow_mut();
                            match cache.as_ref() {
                                Some((cached_tracks, cached_route, visible))
                                    if Arc::ptr_eq(cached_tracks, &self.search_tracks)
                                        && cached_route == &self.source_route =>
                                {
                                    visible.clone()
                                }
                                _ => {
                                    let visible = Arc::new(
                                        self.search_tracks
                                            .iter()
                                            .filter(|track| &track.id.source == route)
                                            .cloned()
                                            .collect::<Vec<_>>(),
                                    );
                                    *cache = Some((
                                        self.search_tracks.clone(),
                                        self.source_route.clone(),
                                        visible.clone(),
                                    ));
                                    visible
                                }
                            }
                        }
                        None => self.search_tracks.clone(),
                    };
                    let route_label = self.source_route.as_ref().map(|id| {
                        self.registry
                            .snapshot()
                            .into_iter()
                            .find(|s| &s.info.id == id)
                            .map(|s| s.info.display_name)
                            .unwrap_or_else(|| id.0.clone())
                    });
                    let subtitle = if self.searching {
                        "正在向已加载的音源请求…".to_string()
                    } else if visible.is_empty() {
                        if self.search_input.read(cx).content().trim().is_empty() {
                            "输入关键词后回车搜索".to_string()
                        } else {
                            "没有找到结果".to_string()
                        }
                    } else {
                        let mut s =
                            format!("{} 项 · 聚合 {}ms", visible.len(), self.search_elapsed_ms);
                        if let Some(route) = route_label {
                            s.push_str(&format!(" · {route}"));
                        }
                        s
                    };
                    (visible, QueueList::Search, "搜索".into(), subtitle)
                }
                ViewTab::Library => {
                    let (tracks, list, title) = match self.library_filter {
                        LibraryFilter::All => (
                            self.library.clone(),
                            QueueList::Library,
                            "所有音乐".to_string(),
                        ),
                        LibraryFilter::Queue => {
                            (self.queue.clone(), QueueList::Queue, "播放队列".to_string())
                        }
                    };
                    let subtitle =
                        if self.library_scanning && self.library_filter == LibraryFilter::All {
                            "正在扫描本地库…".to_string()
                        } else {
                            format!("{} 首", tracks.len())
                        };
                    (tracks, list, title, subtitle)
                }
                ViewTab::Collection => {
                    let view = self.collection.as_ref();
                    let tracks = view
                        .map(|c| c.tracks.clone())
                        .unwrap_or_else(|| Arc::new(Vec::new()));
                    let title = view
                        .map(|c| c.title.clone())
                        .unwrap_or_else(|| "集合".into());
                    let subtitle = view
                        .map(|c| {
                            if let Some(error) = &c.error {
                                format!("加载失败：{error}")
                            } else {
                                c.subtitle.clone()
                            }
                        })
                        .unwrap_or_else(|| "未选择集合".into());
                    (tracks, QueueList::Collection, title, subtitle)
                }
            };

        let mut actions = div().flex().items_center().gap(px(6.0));
        if self.view_tab == ViewTab::Collection {
            let weak = weak.clone();
            actions = actions.child(
                div()
                    .id("collection-back")
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_dim)
                    .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                    .child(crate::icons::icon(
                        crate::icons::Icon::ChevronDown,
                        9.0,
                        theme.text_faint,
                    ))
                    .child("返回")
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.collection = None;
                            let old_cover = app.collection_cover.take();
                            app.retire_image(old_cover);
                            app.view_tab = ViewTab::Library;
                            cx.notify();
                        });
                    }),
            );
        }
        if self.source_route.is_some() {
            let weak = weak.clone();
            actions = actions.child(
                div()
                    .id("clear-route")
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_dim)
                    .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Close,
                        9.0,
                        theme.text_faint,
                    ))
                    .child("全部音源")
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.source_route = None;
                            cx.notify();
                        });
                    }),
            );
        }
        if !tracks.is_empty() {
            let weak = weak.clone();
            actions = actions.child(crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Play, 13.0, theme.text_dim),
                28.0,
                "播放全部",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_from_list(list, 0, cx));
                },
            ));
        }
        let has_album = self
            .current_track
            .as_ref()
            .and_then(|t| t.album.as_ref())
            .and_then(|a| a.id.as_ref())
            .is_some();
        let has_artist = self
            .current_track
            .as_ref()
            .is_some_and(|t| t.artists.iter().any(|a| a.id.is_some()));
        if has_album && !phone {
            let weak = weak.clone();
            actions = actions.child(crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Layers, 13.0, theme.text_dim),
                28.0,
                "当前专辑",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.open_album_for_current(cx));
                },
            ));
        }
        if has_artist && !phone {
            let weak = weak.clone();
            actions = actions.child(crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Note, 13.0, theme.text_dim),
                28.0,
                "当前歌手",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.open_artist_for_current(cx));
                },
            ));
        }

        let body: AnyElement = if tracks.is_empty() {
            let (icon, empty_title, hint) = match self.view_tab {
                ViewTab::Search => {
                    if self.searching {
                        (
                            crate::icons::Icon::Refresh,
                            "正在搜索",
                            "正在向已加载的音源请求结果",
                        )
                    } else if self.search_input.read(cx).content().trim().is_empty() {
                        (
                            crate::icons::Icon::Search,
                            "搜索曲目",
                            "输入歌名、歌手或专辑，回车搜索",
                        )
                    } else {
                        (
                            crate::icons::Icon::Alert,
                            "没有找到结果",
                            "换个关键词，或到全部音乐中查找",
                        )
                    }
                }
                ViewTab::Library => {
                    if self.library_scanning && self.library_filter == LibraryFilter::All {
                        (
                            crate::icons::Icon::Refresh,
                            "正在扫描本地库",
                            "读取标签与同名歌词，请稍候",
                        )
                    } else if self.library_filter == LibraryFilter::Queue {
                        (
                            crate::icons::Icon::Queue,
                            "播放队列为空",
                            "从搜索结果或全部音乐里点一首开始",
                        )
                    } else {
                        (
                            crate::icons::Icon::Note,
                            "本地库为空",
                            "在 config.json 的 local_roots 添加音乐文件夹",
                        )
                    }
                }
                ViewTab::Collection => {
                    let (title, hint) = match self.collection.as_ref() {
                        Some(view) if view.loading => {
                            ("正在加载集合", "正在请求歌单/专辑/歌手信息")
                        }
                        Some(view) if view.error.is_some() => {
                            ("集合加载失败", "检查网易云服务是否可用后重试")
                        }
                        Some(_) => ("集合为空", "这个歌单/专辑暂时没有曲目"),
                        None => ("未选择集合", "从侧栏歌单或曲目信息进入"),
                    };
                    (crate::icons::Icon::Layers, title, hint)
                }
            };
            div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(7.0))
                .pb(px(40.0))
                .child(crate::icons::icon(icon, 22.0, theme.text_faint))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(empty_title),
                )
                .child(
                    div()
                        .max_w(px(240.0))
                        .text_center()
                        .text_size(px(11.0))
                        .line_height(px(16.0))
                        .text_color(theme.text_faint)
                        .child(hint),
                )
                .into_any_element()
        } else {
            self.render_track_table(&theme, tracks, list, cx)
        };

        let header = if phone {
            self.render_phone_pane_header(
                &theme,
                title,
                subtitle,
                Some(actions.into_any_element()),
                cx,
            )
        } else {
            self.render_pane_header(
                &theme,
                title,
                subtitle,
                Some(actions.into_any_element()),
            )
        };

        div()
            .h_full()
            .when_some(width, |d, w| d.w(px(w)))
            .when(width.is_none(), |d| d.flex_1())
            .min_w(px(280.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.surface)
            .when(!phone, |d| d.border_r_1().border_color(theme.border))
            .child(header)
            .when(self.view_tab == ViewTab::Collection, |d| {
                d.child(self.render_collection_header(&theme, cx))
            })
            .when(self.view_tab == ViewTab::Search, |d| {
                d.child(self.render_search_tabs(&theme, cx))
            })
            .when(self.view_tab != ViewTab::Collection, |d| {
                d.child(
                    div()
                        .px(px(12.0))
                        .pb(px(8.0))
                        .child(self.search_input_view.clone()),
                )
            })
            .child(body)
            .child(
                div()
                    .h(px(if phone { 16.0 } else { 84.0 }))
                    .flex_none(),
            )
            .into_any_element()
    }

    // ================= 右侧：正在播放 =================

    fn now_playing_text(&self) -> Arc<NowPlayingText> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Some(track) = &self.current_track {
            track.title.hash(&mut hasher);
            for artist in &track.artists {
                artist.name.hash(&mut hasher);
            }
            if let Some(album) = &track.album {
                album.name.hash(&mut hasher);
            }
            track.quality.hash(&mut hasher);
            match &track.origin {
                TrackOrigin::Local { .. } => 0u8.hash(&mut hasher),
                TrackOrigin::Remote { source } => {
                    1u8.hash(&mut hasher);
                    source.0.hash(&mut hasher);
                }
            }
        }
        self.player_status.title.hash(&mut hasher);
        let key = hasher.finish();
        if let Some((cached, text)) = self.now_playing_text.borrow().as_ref()
            && *cached == key
        {
            return text.clone();
        }
        let text = Arc::new(NowPlayingText::new(
            self.current_track.as_ref(),
            self.player_status.title.as_deref(),
        ));
        *self.now_playing_text.borrow_mut() = Some((key, text.clone()));
        text
    }

    fn render_now_playing(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let playing = self.player_status.state == PlayerState::Playing;
        let has_track = self.current_track.is_some();
        let text = self.now_playing_text();
        let title = text.title.clone();
        let artists = text.artists.clone();
        let album = text.album.clone();
        let source_label = text.source_label.clone();
        let quality = text.quality.clone();

        let panel_w = (self.viewport_width - self.sidebar_width() - self.list_width()).max(360.0);
        let content_w = (panel_w - 56.0).clamp(240.0, 440.0);
        let compact_h = self.viewport_height < 780.0;
        let art = (self.viewport_height * 0.28).clamp(160.0, 260.0);
        let info_mt = if compact_h { 14.0 } else { 20.0 };
        let lyrics_mt = if compact_h { 14.0 } else { 24.0 };
        let lyrics_pt = if compact_h { 10.0 } else { 14.0 };
        let preview_count = if compact_h { 5 } else { 7 };

        let artwork: AnyElement = {
            let placeholder_bg = if theme.is_dark() {
                linear_gradient(
                    135.0,
                    linear_color_stop(crate::theme::hex(0x2C2C33), 0.0),
                    linear_color_stop(crate::theme::hex(0x131316), 1.0),
                )
            } else {
                linear_gradient(
                    135.0,
                    linear_color_stop(crate::theme::hex(0xF2F2F5), 0.0),
                    linear_color_stop(crate::theme::hex(0xD8D8DF), 1.0),
                )
            };
            let placeholder = div()
                .size_full()
                .bg(placeholder_bg)
                .flex()
                .items_center()
                .justify_center()
                .child(crate::icons::icon(
                    crate::icons::Icon::Note,
                    art * 0.26,
                    theme.text_faint,
                ))
                .into_any_element();
            let cover = crate::widgets::cover_plain(
                &theme,
                self.cover_image.clone(),
                art,
                12.0,
                placeholder,
            );
            let wrapper = div()
                .rounded(px(12.0))
                .shadow(vec![BoxShadow {
                    color: theme.shadow_strong,
                    offset: point(px(0.0), px(14.0)),
                    blur_radius: px(30.0),
                    spread_radius: px(0.0),
                }])
                .child(cover);
            if has_track {
                let weak = weak.clone();
                wrapper
                    .id("hero-cover")
                    .cursor_pointer()
                    .on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.open_portrait(cx);
                            window.focus(&app.focus_handle);
                            cx.notify();
                        });
                    })
                    .into_any_element()
            } else {
                wrapper.into_any_element()
            }
        };

        let artist_line = if has_track {
            if artists.is_empty() {
                SharedString::from("未知艺人")
            } else {
                artists
            }
        } else {
            SharedString::from("从左侧列表选择一首歌开始")
        };

        let pill = |text: SharedString| {
            div()
                .h(px(20.0))
                .px(px(7.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface_alt)
                .flex()
                .items_center()
                .text_size(px(9.5))
                .text_color(theme.text_faint)
                .child(text)
                .into_any_element()
        };

        let mut info = div()
            .w(px(content_w))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(3.0))
            .mt(px(info_mt))
            .child(
                div()
                    .w_full()
                    .text_size(px(21.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .text_ellipsis()
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .w_full()
                    .text_size(px(13.0))
                    .text_color(theme.text_dim)
                    .text_ellipsis()
                    .child(SharedString::from(artist_line)),
            );
        if !album.is_empty() {
            info = info.child(
                div()
                    .w_full()
                    .text_size(px(10.5))
                    .text_color(theme.text_faint)
                    .text_ellipsis()
                    .child(album.clone()),
            );
        }
        if has_track {
            let mut meta = div().flex().items_center().gap(px(6.0)).mt(px(9.0));
            if !source_label.is_empty() {
                meta = meta.child(pill(source_label.clone()));
            }
            if !quality.is_empty() {
                meta = meta.child(pill(quality.clone()));
            }
            info = info.child(meta);

            let album_link = self
                .current_track
                .as_ref()
                .and_then(|t| t.album.as_ref())
                .and_then(|a| a.id.as_ref())
                .is_some();
            let artist_link = self
                .current_track
                .as_ref()
                .is_some_and(|t| t.artists.iter().any(|a| a.id.is_some()));
            if album_link || artist_link {
                let mut links = div().flex().items_center().gap(px(6.0)).mt(px(3.0));
                if album_link {
                    let weak = weak.clone();
                    links = links.child(
                        div()
                            .id("goto-album")
                            .h(px(22.0))
                            .px(px(7.0))
                            .rounded(px(5.0))
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(px(10.0))
                            .text_color(theme.text_dim)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Layers,
                                11.0,
                                theme.text_faint,
                            ))
                            .child("专辑")
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.open_album_for_current(cx);
                                });
                            }),
                    );
                }
                if artist_link {
                    let weak = weak.clone();
                    links = links.child(
                        div()
                            .id("goto-artist")
                            .h(px(22.0))
                            .px(px(7.0))
                            .rounded(px(5.0))
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(px(10.0))
                            .text_color(theme.text_dim)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Note,
                                11.0,
                                theme.text_faint,
                            ))
                            .child("歌手")
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.open_artist_for_current(cx);
                                });
                            }),
                    );
                }
                info = info.child(links);
            }
        }

        // 歌词预览：跟随播放位置，当前行高亮。
        let position = self.player_status.position_ms;
        let preview = self.lyric_preview.as_ref();
        let current_idx = if preview.is_empty() {
            None
        } else {
            Some(
                preview
                    .partition_point(|l| l.start_ms <= position)
                    .saturating_sub(1),
            )
        };
        let start = current_idx.map(|i| i.saturating_sub(2)).unwrap_or(0);
        let end = (start + preview_count).min(preview.len());
        let mut lines = div().w(px(content_w)).flex().flex_col().gap(px(1.0));
        if preview.is_empty() {
            lines = lines.child(
                div()
                    .py(px(10.0))
                    .text_size(px(11.5))
                    .line_height(px(17.0))
                    .text_color(theme.text_faint)
                    .child(SharedString::from(
                        self.lyrics_status.clone().unwrap_or_else(|| {
                            if has_track {
                                "暂无歌词".to_string()
                            } else {
                                "播放曲目后在这里同步显示歌词".to_string()
                            }
                        }),
                    )),
            );
        } else {
            for i in start..end {
                let line = &preview[i];
                let active = current_idx == Some(i) && position > 0;
                let (main, sub) = if line.text.is_empty() {
                    (line.translation.clone(), SharedString::default())
                } else {
                    (line.text.clone(), line.translation.clone())
                };
                lines = lines.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .py(px(3.0))
                        .child(
                            div()
                                .text_size(px(if active { 13.5 } else { 12.0 }))
                                .font_weight(if active {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(if active { theme.text } else { theme.text_faint })
                                .text_ellipsis()
                                .child(main),
                        )
                        .when(!sub.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(theme.text_faint)
                                    .text_ellipsis()
                                    .child(sub),
                            )
                        }),
                );
            }
        }

        let immersive_btn = {
            let weak = weak.clone();
            div()
                .id("open-immersive")
                .h(px(24.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .text_size(px(10.5))
                .text_color(theme.text_dim)
                .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                .child(crate::icons::icon(
                    crate::icons::Icon::Expand,
                    11.0,
                    theme.text_faint,
                ))
                .child("沉浸")
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.force_portrait = false;
                        app.enter_immersive(cx);
                        window.focus(&app.focus_handle);
                        cx.notify();
                    });
                })
        };

        let lyrics_block = div()
            .w(px(content_w))
            .mt(px(lyrics_mt))
            .pt(px(lyrics_pt))
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(6.0))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text_faint)
                            .child("歌词"),
                    )
                    .child(immersive_btn),
            )
            .child(lines);

        div()
            .flex_1()
            .min_w(px(320.0))
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .border_l_1()
            .border_color(theme.border)
            .child(
                div()
                    .h(px(52.0))
                    .px(px(22.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("正在播放"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(crate::widgets::equalizer(&theme, playing))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.text_faint)
                                    .child(if playing { "正在播放" } else { "已暂停" }),
                            ),
                    ),
            )
            .child(
                div()
                    .id("now-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(self.smooth_scroll, |d| d.smooth_scroll())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .px(px(28.0))
                            .pt(px(18.0))
                            .pb(px(100.0))
                            .child(artwork)
                            .child(info)
                            .child(lyrics_block),
                    ),
            )
            .into_any_element()
    }

    // ================= 设置面板 =================

    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let _ = cx;
        self.settings_view.clone().into_any_element()
    }

    /// 旧版设置面板：已由 `settings.rs` 的独立实体取代，保留一版用于对照，
    /// 待确认新版无回归后删除。
    #[allow(dead_code)]
    fn render_settings_legacy(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let stats = self.cache.stats();
        let used = format_bytes(stats.total_bytes);
        let limit = format_bytes(stats.limit_bytes);
        let entries = stats.entries;
        let root = stats.root.display().to_string();
        let download_root = self.downloads.root().display().to_string();

        let section = |title: &'static str| {
            div()
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_faint)
                .child(title)
                .into_any_element()
        };
        let hint = |text: String| {
            div()
                .text_size(px(10.0))
                .text_color(theme.text_faint)
                .child(SharedString::from(text))
                .into_any_element()
        };

        let preset_btn = |label: &'static str, bytes: u64| {
            let weak = weak.clone();
            let active = stats.limit_bytes == bytes;
            div()
                .id(SharedString::from(format!("cache-limit-{label}")))
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
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_cache_limit_bytes(bytes, cx));
                })
        };
        let adjust_btn = |label: &'static str, delta: i64| {
            let weak = weak.clone();
            let current = stats.limit_bytes as i64;
            let next = (current + delta).clamp(256 * 1024 * 1024, 100 * 1024 * 1024 * 1024) as u64;
            div()
                .id(SharedString::from(format!("cache-adjust-{label}")))
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
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_cache_limit_bytes(next, cx));
                })
        };

        let pref_btn = |pref: SourcePreference| {
            let weak = weak.clone();
            let active = self.source_preference == pref;
            div()
                .id(SharedString::from(format!("pref-{}", pref.label())))
                .h(px(28.0))
                .px(px(10.0))
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
                .text_size(px(11.0))
                .text_color(if active { theme.text } else { theme.text_dim })
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .child(pref.label())
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_source_preference(pref, cx));
                })
        };

        let band_btn = |label: &'static str, low: f32, high: f32| {
            let weak = weak.clone();
            let active = self.beat_band == (low, high);
            div()
                .id(SharedString::from(format!("beat-preset-{label}")))
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
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.set_beat_band_preset(low, high, cx));
                })
        };

        let clear_btn = {
            let weak = weak.clone();
            div()
                .id("cache-clear")
                .h(px(26.0))
                .px(px(9.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.border)
                .flex()
                .items_center()
                .text_size(px(10.5))
                .text_color(theme.danger)
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .child("清空缓存")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.clear_cache_now(cx));
                })
        };
        let close_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Close, 12.0, theme.text_dim),
                26.0,
                "关闭设置",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_settings(cx));
                },
            )
        };

        let card = div()
            .id("settings-card")
            .w(px(620.0))
            .max_h(px(640.0))
            .overflow_y_scroll()
            .when(self.smooth_scroll, |d| d.smooth_scroll())
            .rounded(px(14.0))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.surface)
            .shadow(vec![BoxShadow {
                color: theme.shadow_strong,
                offset: point(px(0.0), px(12.0)),
                blur_radius: px(36.0),
                spread_radius: px(0.0),
            }])
            .p(px(20.0))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("设置"),
                    )
                    .child(close_btn),
            )
            .child(section("对象缓存"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(hint(format!(
                        "已用 {used} / {limit} · {entries} 项 · 目录（回车应用）"
                    )))
                    .child(self.settings_cache_view.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(preset_btn("1 GB", 1024 * 1024 * 1024))
                    .child(preset_btn("5 GB", 5 * 1024 * 1024 * 1024))
                    .child(preset_btn("10 GB", 10 * 1024 * 1024 * 1024))
                    .child(preset_btn("20 GB", 20 * 1024 * 1024 * 1024))
                    .child(adjust_btn("−", -(1024i64 * 1024 * 1024)))
                    .child(adjust_btn("+", 1024 * 1024 * 1024))
                    .child(div().flex_1())
                    .child(clear_btn),
            )
            .child(hint(format!("实际缓存根目录：{root}")))
            .child(section("显式下载"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(hint(
                        "下载文件直接写入该目录，不计入缓存配额；播放时优先使用。回车应用。".into(),
                    ))
                    .child(self.settings_download_view.clone()),
            )
            .child(hint(format!("实际下载目录：{download_root}")))
            .child(section("播放来源（本地同曲 vs 在线）"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(pref_btn(SourcePreference::Auto))
                    .child(pref_btn(SourcePreference::PreferLocal))
                    .child(pref_btn(SourcePreference::PreferOnline))
                    .child(div().flex_1())
                    .child(hint(
                        "音质优先：仅当本地更高时才播本地；在线优先：除已下载外始终在线".into(),
                    )),
            )
            .child(section("背景鼓点监听"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(hint(format!(
                        "当前 {:.0} Hz – {:.0} Hz · 沉浸歌词背景的网格随该频段能量跳动；回车应用",
                        self.beat_band.0, self.beat_band.1
                    )))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(hint("下限".into()))
                            .child(
                                div()
                                    .w(px(110.0))
                                    .child(self.settings_beat_low_view.clone()),
                            )
                            .child(hint("Hz".into()))
                            .child(div().w(px(12.0)))
                            .child(hint("上限".into()))
                            .child(
                                div()
                                    .w(px(110.0))
                                    .child(self.settings_beat_high_view.clone()),
                            )
                            .child(hint("Hz".into())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(band_btn("低音 80–2k", 80.0, 2_000.0))
                            .child(band_btn("鼓点 4–10k", 4_000.0, 10_000.0))
                            .child(band_btn("高频 8–16k", 8_000.0, 16_000.0))
                            .child(div().flex_1())
                            .child(hint("默认 4000–10000 Hz（鼓点/镲片）".into())),
                    ),
            )
            .child(section("均衡器"))
            .child({
                let blue: gpui::Hsla = gpui::rgb(0x3B82F6).into();
                let weak_eq = weak.clone();
                let weak_toggle = weak.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(hint(format!(
                        "{} · {} 个频段 · {}",
                        self.eq.display_name(),
                        self.eq.settings.bands.len(),
                        if self.eq.settings.enabled {
                            "已开启"
                        } else {
                            "已关闭"
                        }
                    )))
                    .child(div().flex_1())
                    .child(crate::widgets::switch(
                        self.eq.settings.enabled,
                        blue,
                        "settings-eq-enabled",
                        move |on, cx| {
                            let _ = weak_toggle.update(cx, |app, cx| {
                                app.set_eq_enabled(on, cx);
                            });
                        },
                    ))
                    .child(
                        div()
                            .id("settings-open-eq")
                            .h(px(28.0))
                            .px(px(10.0))
                            .rounded(px(7.0))
                            .bg(theme.text)
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_size(px(11.0))
                            .text_color(theme.on_accent)
                            .hover(|s| s.bg(theme.accent_hover))
                            .child("打开均衡器")
                            .on_click(move |_, _, cx| {
                                let _ = weak_eq.update(cx, |app, cx| {
                                    app.settings_open = false;
                                    app.open_eq(cx);
                                });
                            }),
                    )
            });

        let weak_overlay = weak.clone();
        div()
            .id("settings-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000066))
            .on_click(move |_, _, cx| {
                let _ = weak_overlay.update(cx, |app, cx| app.close_settings(cx));
            })
            .child(
                div()
                    .id("settings-card-wrap")
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(card),
            )
            .into_any_element()
    }

    // ================= 登录面板 =================

    fn render_login(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let tab = self.login_tab;
        let status = self.login_status.clone().unwrap_or_else(|| match tab {
            LoginTab::Qr => "等待扫码".into(),
            LoginTab::Phone => "输入手机号并获取验证码".into(),
        });
        let close_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Close, 12.0, theme.text_dim),
                26.0,
                "关闭登录",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_login(cx));
                },
            )
        };
        let tab_btn = |target: LoginTab, label: &'static str| {
            let weak = weak.clone();
            let active = tab == target;
            div()
                .id(SharedString::from(format!("login-tab-{label}")))
                .h(px(26.0))
                .px(px(10.0))
                .rounded(px(6.0))
                .bg(if active {
                    theme.active
                } else {
                    gpui::transparent_black()
                })
                .flex()
                .items_center()
                .text_size(px(11.0))
                .font_weight(if active {
                    gpui::FontWeight::MEDIUM
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(if active { theme.text } else { theme.text_faint })
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover).text_color(theme.text))
                .child(label)
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.switch_login_tab(target, cx));
                })
        };

        let body: AnyElement = match tab {
            LoginTab::Qr => {
                let placeholder = div()
                    .size_full()
                    .bg(theme.surface_alt)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::icons::icon(
                        crate::icons::Icon::Refresh,
                        28.0,
                        theme.text_faint,
                    ))
                    .into_any_element();
                let qr = crate::widgets::cover_thumb(
                    &theme,
                    self.login_qr.clone(),
                    216.0,
                    10.0,
                    placeholder,
                );
                let refresh_btn = {
                    let weak = weak.clone();
                    crate::widgets::text_button(
                        &theme,
                        "刷新二维码",
                        false,
                        move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| app.refresh_login(cx));
                        },
                    )
                };
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.0))
                    .child(qr)
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.text_dim)
                            .child(SharedString::from(status)),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .text_center()
                            .child("使用网易云音乐 App 扫码登录；Cookie 只保存在本机配置中"),
                    )
                    .child(refresh_btn)
                    .into_any_element()
            }
            LoginTab::Phone => {
                let cooldown = self
                    .captcha_cooldown_until
                    .is_some_and(|until| until > Instant::now());
                let send_btn = {
                    let weak = weak.clone();
                    let label = if cooldown {
                        "已发送"
                    } else {
                        "发送验证码"
                    };
                    crate::widgets::text_button(&theme, label, false, move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.send_login_captcha(cx));
                    })
                };
                let login_btn = {
                    let weak = weak.clone();
                    crate::widgets::text_button(&theme, "登录", true, move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.submit_phone_login(cx));
                    })
                };
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(self.login_phone_view.clone())
                    .child(self.login_captcha_view.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(send_btn)
                            .child(login_btn),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_dim)
                            .child(SharedString::from(status)),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.text_faint)
                            .child("验证码 5 分钟内有效；Cookie 只保存在本机配置中"),
                    )
                    .into_any_element()
            }
        };

        let card_w = (self.viewport_width - 24.0).clamp(300.0, 380.0);
        let card_pad = if self.viewport_width < 420.0 { 16.0 } else { 20.0 };
        let card = div()
            .id("login-card")
            .w(px(card_w))
            .rounded(px(14.0))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.surface)
            .shadow(vec![BoxShadow {
                color: theme.shadow_strong,
                offset: point(px(0.0), px(12.0)),
                blur_radius: px(36.0),
                spread_radius: px(0.0),
            }])
            .p(px(card_pad))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("登录网易云"),
                    )
                    .child(close_btn),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(tab_btn(LoginTab::Qr, "扫码登录"))
                    .child(tab_btn(LoginTab::Phone, "手机号登录")),
            )
            .child(body);

        let weak_overlay = weak.clone();
        div()
            .id("login-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000066))
            .on_click(move |_, _, cx| {
                let _ = weak_overlay.update(cx, |app, cx| app.close_login(cx));
            })
            .child(
                div()
                    .id("login-card-wrap")
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(card),
            )
            .into_any_element()
    }

    // ================= 底部悬浮 Dock =================

    fn render_portrait_queue(
        &self,
        content_w: f32,
        shared_hidden: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let queue = self.queue.clone();
        let current = self.queue_pos;
        let mode = self.play_mode;

        let hint = {
            let weak = weak.clone();
            div()
                .id("queue-back")
                .w_full()
                .h(px(58.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(white.opacity(0.58))
                .child("此处向下轻扫以返回播放界面")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.set_portrait_page(PortraitPage::Art, cx);
                    });
                })
        };

        let track_header = {
            let title = self.current_title_text();
            let artist = self.current_artist_text();
            let theme = Theme::dark();
            let cover = crate::widgets::cover_plain(
                &theme,
                self.cover_image.clone(),
                crate::layout_morph::QUEUE_COVER,
                crate::layout_morph::QUEUE_COVER_RADIUS,
                crate::icons::icon(crate::icons::Icon::Note, 20.0, white.opacity(0.35)),
            );
            div()
                .w(px(content_w))
                .flex()
                .items_center()
                .gap(px(crate::layout_morph::QUEUE_COVER_GAP))
                .pb(px(10.0))
                .child(
                    div()
                        .opacity(if shared_hidden { 0.0 } else { 1.0 })
                        .child(cover),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .opacity(if shared_hidden { 0.0 } else { 1.0 })
                        .child(
                            div()
                                .text_size(px(16.0))
                                .line_height(px(16.0 * INFO_LINE_RATIO))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(white.opacity(0.9))
                                .text_ellipsis()
                                .child(SharedString::from(title)),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .line_height(px(11.0 * INFO_LINE_RATIO))
                                .text_color(white.opacity(0.45))
                                .text_ellipsis()
                                .child(SharedString::from(artist)),
                        ),
                )
        };

        let clear = {
            let weak = weak.clone();
            div()
                .id("queue-clear")
                .h(px(28.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(white.opacity(0.55))
                .hover(|s| s.bg(white.opacity(0.08)).text_color(white.opacity(0.85)))
                .child("清除")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.stop_playback(cx);
                        app.queue = Arc::new(Vec::new());
                        app.queue_pos = None;
                        cx.notify();
                    });
                })
        };
        let count_row = div()
            .w(px(content_w))
            .h(px(34.0))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(white.opacity(0.42))
                    .child(SharedString::from(format!(
                        "{}/{}",
                        current.map(|p| p + 1).unwrap_or(0),
                        queue.len()
                    ))),
            )
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(white.opacity(0.92))
                    .child("播放队列"),
            )
            .child(clear);

        let list: AnyElement = if queue.is_empty() {
            div()
                .w(px(content_w))
                .flex_1()
                .min_h_0()
                .pt(px(40.0))
                .text_center()
                .text_size(px(12.0))
                .text_color(white.opacity(0.4))
                .child("队列为空")
                .into_any_element()
        } else {
            let rows_queue = queue.clone();
            let weak_rows = weak.clone();
            let mut list = gpui::uniform_list("queue-list", queue.len(), move |range, _window, _cx| {
                range
                    .filter_map(|index| {
                        let track = rows_queue.get(index)?;
                        let is_current = current == Some(index);
                        let title = track.title.clone();
                        let artist = track
                            .artists
                            .iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" / ");
                        let weak_row = weak_rows.clone();
                        let weak_remove = weak_rows.clone();
                        Some(
                            div()
                                .id(gpui::ElementId::named_usize("queue-row", index))
                                .w_full()
                                .h(px(60.0))
                                .pb(px(2.0))
                                .px(px(12.0))
                                .rounded(px(10.0))
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .when(is_current, |d| d.bg(white.opacity(0.08)))
                                .hover(|s| s.bg(white.opacity(0.10)))
                                .cursor_pointer()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .text_size(px(13.5))
                                                .font_weight(if is_current {
                                                    gpui::FontWeight::MEDIUM
                                                } else {
                                                    gpui::FontWeight::NORMAL
                                                })
                                                .text_color(white.opacity(if is_current {
                                                    0.96
                                                } else {
                                                    0.82
                                                }))
                                                .text_ellipsis()
                                                .child(SharedString::from(title)),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(10.5))
                                                .text_color(white.opacity(0.38))
                                                .text_ellipsis()
                                                .child(SharedString::from(artist)),
                                        ),
                                )
                                .child(
                                    div()
                                        .id(gpui::ElementId::named_usize("queue-remove", index))
                                        .size(px(28.0))
                                        .rounded(px(6.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .text_size(px(14.0))
                                        .text_color(white.opacity(0.38))
                                        .hover(|s| {
                                            s.bg(white.opacity(0.10)).text_color(white.opacity(0.8))
                                        })
                                        .child("—")
                                        .on_click(move |_, _, cx| {
                                            cx.stop_propagation();
                                            let _ = weak_remove.update(cx, |app, cx| {
                                                app.remove_queue_at(index, cx);
                                            });
                                        }),
                                )
                                .on_click(move |_, _, cx| {
                                    let _ = weak_row.update(cx, |app, cx| {
                                        app.play_from_list(QueueList::Queue, index, cx);
                                    });
                                })
                                .into_any_element(),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            if self.smooth_scroll {
                list = list.smooth_scroll();
            }
            list.track_scroll(self.queue_scroll.clone())
            .flex_1()
            .min_h_0()
            .w(px(content_w))
            .into_any_element()
        };

        let pill = {
            let weak = weak.clone();
            div()
                .id("queue-mode-pill")
                .absolute()
                .bottom(px(22.0))
                .left(px(((self.viewport_width - content_w) / 2.0).max(16.0)))
                .h(px(32.0))
                .px(px(14.0))
                .rounded(px(16.0))
                .bg(white.opacity(0.10))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(white.opacity(0.78))
                .hover(|s| s.bg(white.opacity(0.15)))
                .child(SharedString::from(mode.full_label()))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.cycle_play_mode(cx));
                })
        };

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(4.0))
            .child(hint)
            .child(div().w(px(content_w)).child(track_header))
            .child(count_row)
            .child(div().w(px(content_w)).h(px(1.0)).bg(white.opacity(0.10)))
            .child(list)
            .child(pill)
            .into_any_element()
    }

    fn render_dock(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let status = &self.player_status;
        let text = self.now_playing_text();
        let title = text.title.clone();
        let playing = status.state == PlayerState::Playing;
        let duration = status.duration_ms.unwrap_or(0);
        let position = status.position_ms.min(duration.max(1));
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };
        let artist = if self.current_track.is_some() {
            if !text.artists.is_empty() {
                text.artists.clone()
            } else if !text.album.is_empty() {
                text.album.clone()
            } else {
                SharedString::from("未知艺人")
            }
        } else {
            SharedString::from("选择一首歌开始")
        };

        let width = self.dock_width();
        let left_x = ((self.viewport_width - width) / 2.0).max(8.0);
        let show_volume = self.viewport_width >= 1120.0;
        let compact = width < 720.0;

        let cover = crate::widgets::cover_thumb(
            &theme,
            self.cover_image.clone(),
            42.0,
            7.0,
            crate::icons::icon(crate::icons::Icon::Note, 16.0, theme.text_faint),
        );
        let open_immersive = {
            let weak = weak.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |app, cx| {
                    app.open_portrait(cx);
                    window.focus(&app.focus_handle);
                    cx.notify();
                });
            }
        };
        let left = div()
            .id("dock-track")
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(10.0))
            .w(px(if compact { 180.0 } else { 232.0 }))
            .min_w_0()
            .child(cover)
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
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(if self.current_track.is_some() {
                                theme.text
                            } else {
                                theme.text_dim
                            })
                            .text_ellipsis()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(artist),
                    ),
            )
            .on_click(open_immersive);

        let prev = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Prev, 14.0, theme.text_dim),
                28.0,
                "上一首",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_prev(cx));
                },
            )
        };
        let next = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Next, 14.0, theme.text_dim),
                28.0,
                "下一首",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_next(cx));
                },
            )
        };
        let play_icon = if playing {
            crate::icons::icon(crate::icons::Icon::Pause, 15.0, theme.on_accent)
        } else {
            crate::icons::icon(crate::icons::Icon::Play, 15.0, theme.on_accent)
        };
        let play = {
            let weak = weak.clone();
            div()
                .id("dock-play")
                .size(px(36.0))
                .rounded(px(18.0))
                .bg(theme.accent)
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(theme.accent_hover))
                .child(play_icon)
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_playback(cx));
                })
        };
        let transport = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(prev)
            .child(play)
            .child(next);

        let seek = {
            let player = self.player.clone();
            move |fraction: f32| {
                if duration > 0 {
                    let target = (fraction.clamp(0.0, 1.0) * duration as f32) as u64;
                    player.seek_ms(target);
                }
            }
        };
        let progress_slider = crate::widgets::slider(
            progress,
            crate::widgets::SliderColors {
                track: theme.border_strong,
                fill: theme.accent,
                thumb: theme.accent,
            },
            "seek",
            seek,
        );
        let progress_group = div()
            .flex_1()
            .min_w(px(120.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(36.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_dim)
                    .text_align(gpui::TextAlign::Right)
                    .child(SharedString::from(fmt_ms(position))),
            )
            .child(div().flex_1().child(progress_slider))
            .child(
                div()
                    .w(px(36.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_faint)
                    .child(SharedString::from(if duration > 0 {
                        fmt_ms(duration)
                    } else {
                        "--:--".into()
                    })),
            );

        let volume_btn = {
            let weak = weak.clone();
            let muted = status.volume <= 0.001;
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(
                    if muted {
                        crate::icons::Icon::VolumeMute
                    } else {
                        crate::icons::Icon::Volume
                    },
                    15.0,
                    theme.text_dim,
                ),
                26.0,
                "静音",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, _| {
                        let v = if app.player_status.volume > 0.001 {
                            0.0
                        } else {
                            0.9
                        };
                        app.player.set_volume(v);
                        app.player_status.volume = v;
                    });
                },
            )
        };
        let volume_slider = {
            let player = self.player.clone();
            crate::widgets::slider(
                status.volume.clamp(0.0, 1.0),
                crate::widgets::SliderColors {
                    track: theme.border_strong,
                    fill: theme.text_dim,
                    thumb: theme.text,
                },
                "volume",
                move |fraction: f32| player.set_volume(fraction),
            )
        };
        let preference_btn = {
            let weak = weak.clone();
            let label = self.source_preference.label();
            div()
                .id("dock-preference")
                .h(px(26.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .cursor_pointer()
                .hover(|s| s.bg(theme.surface_alt))
                .child(crate::icons::icon(
                    crate::icons::Icon::Sliders,
                    13.0,
                    theme.text_faint,
                ))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme.text_dim)
                        .child(SharedString::from(label)),
                )
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.cycle_source_preference(cx));
                })
        };
        let download_btn = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Download, 15.0, theme.text_dim),
                26.0,
                "下载当前曲目",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.download_current(cx));
                },
            )
        };
        let lyrics_btn = {
            let weak = weak.clone();
            div()
                .id("dock-lyrics")
                .size(px(28.0))
                .rounded(px(7.0))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(theme.surface_alt))
                .child(crate::icons::icon(
                    crate::icons::Icon::Lyrics,
                    16.0,
                    theme.text_dim,
                ))
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.open_lyrics_immersive(cx);
                        window.focus(&app.focus_handle);
                        cx.notify();
                    });
                })
        };
        let right = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(volume_btn)
            .when(show_volume, |d| {
                d.child(div().w(px(72.0)).child(volume_slider))
            })
            .child(div().w(px(1.0)).h(px(18.0)).bg(theme.border))
            .child(preference_btn)
            .child(download_btn)
            .child(div().w(px(1.0)).h(px(18.0)).bg(theme.border))
            .child(lyrics_btn);

        div()
            .absolute()
            .left(px(left_x))
            .bottom(px(crate::theme::size::DOCK_BOTTOM))
            .w(px(width))
            .h(px(crate::theme::size::DOCK))
            .rounded(px(18.0))
            .bg(theme.glass)
            .border_1()
            .border_color(theme.glass_border)
            .shadow(vec![BoxShadow {
                color: theme.shadow_strong,
                offset: point(px(0.0), px(10.0)),
                blur_radius: px(28.0),
                spread_radius: px(0.0),
            }])
            .px(px(14.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(left)
            .child(transport)
            .child(progress_group)
            .child(right)
            .into_any_element()
    }

    /// 播放线程的位置快照约 33ms 更新一次；沉浸页在两次快照之间按真实
    /// 帧间隔外推，让进度条在 300Hz 下仍然连续（上限 80ms，避免卡顿后漂移）。
    fn sync_progress_clock(&mut self) {
        let (position, playing) = self.player.clock();
        let (last_position, last_playing, _) = self.progress_clock;
        if position != last_position || playing != last_playing {
            self.progress_clock = (position, playing, Instant::now());
        }
    }

    fn display_position_ms(&self) -> u64 {
        let (position, playing, since) = self.progress_clock;
        if playing {
            position.saturating_add(since.elapsed().as_millis().min(80) as u64)
        } else {
            position
        }
    }

    /// 音质标签点击后的「音频解码信息」弹窗（AMLL `AudioQualityDialog`）。
    fn render_quality_dialog(&self, cx: &mut Context<Self>) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let track = self.current_track.as_ref();
        let quality = track.and_then(|t| t.quality_info.as_ref());
        let codec = quality
            .and_then(|q| q.codec.clone())
            .unwrap_or_else(|| "未知".into());
        let bitrate = quality
            .and_then(|q| q.bitrate_kbps)
            .map(|v| format!("{v} kbps"))
            .unwrap_or_else(|| "—".into());
        let sample_rate = quality
            .and_then(|q| q.sample_rate_hz)
            .map(|v| format!("{v} Hz"))
            .unwrap_or_else(|| "—".into());
        let tier = quality
            .map(|q| match q.tier {
                chordal_core::model::QualityTier::Unknown => "未知",
                chordal_core::model::QualityTier::Standard => "标准",
                chordal_core::model::QualityTier::High => "高品质",
                chordal_core::model::QualityTier::Lossless => "无损",
                chordal_core::model::QualityTier::HiRes => "高解析度",
            })
            .unwrap_or("—");
        let lossless = quality
            .map(|q| if q.lossless { "是" } else { "否" })
            .unwrap_or("—");
        let row = |label: &'static str, value: String| {
            div()
                .w_full()
                .h(px(28.0))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(white.opacity(0.5))
                        .child(SharedString::from(label)),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(white.opacity(0.9))
                        .child(SharedString::from(value)),
                )
        };
        let close = {
            let weak = weak.clone();
            div()
                .id("quality-dialog-close")
                .h(px(30.0))
                .px(px(14.0))
                .rounded(px(7.0))
                .bg(white.opacity(0.12))
                .hover(|s| s.bg(white.opacity(0.18)))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(12.0))
                .text_color(white)
                .child("关闭")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.quality_dialog_open = false;
                        cx.notify();
                    });
                })
        };
        let backdrop = {
            let weak = weak.clone();
            div()
                .id("quality-dialog-backdrop")
                .absolute()
                .inset_0()
                .bg(gpui::black().opacity(0.45))
                .flex()
                .items_center()
                .justify_center()
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.quality_dialog_open = false;
                        cx.notify();
                    });
                })
        };
        let card = div()
            .id("quality-dialog")
            .w(px(340.0))
            .rounded(px(12.0))
            .bg(gpui::rgba(0x1C1C20F7))
            .border_1()
            .border_color(white.opacity(0.1))
            .shadow(vec![BoxShadow {
                color: gpui::black().opacity(0.35),
                offset: point(px(0.0), px(16.0)),
                blur_radius: px(40.0),
                spread_radius: px(0.0),
            }])
            .p(px(20.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .text_size(px(16.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(white.opacity(0.95))
                    .child("音频解码信息"),
            )
            .child(div().w_full().h(px(1.0)).my(px(10.0)).bg(white.opacity(0.1)))
            .child(row("音频解码器", codec))
            .child(row("码率", bitrate))
            .child(row("采样率", sample_rate))
            .child(row("品质", tier.to_string()))
            .child(row("无损", lossless.to_string()))
            .child(div().w_full().h(px(1.0)).my(px(10.0)).bg(white.opacity(0.1)))
            .child(div().w_full().flex().justify_end().child(close));
        backdrop.child(card).into_any_element()
    }

    fn render_immersive(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let bounds = window.bounds();
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let now = Instant::now();
        self.sync_progress_clock();

        // Dock→全屏共享布局：p = 1 时页面完全贴在 Dock 上，p = 0 时满窗。
        // 根容器保持满窗（输入命中区），真正缩放的是这张卡片。
        let p = self.immersive_dock_progress(now, height);
        let full = StageRect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        };
        let card = full.lerp(dock_rect(width, height), p);
        let card_w = card.w.max(1.0);
        let card_h = card.h.max(1.0);
        // 页面自身的控件在最后 1/3 的收缩距离内淡出（共享封面/曲名仍然可见）。
        let content_opacity = if p < 0.65 {
            1.0
        } else {
            ((1.0 - p) / 0.2).clamp(0.0, 1.0)
        };
        // 页面自带的 Dock 更早淡出：真实 Dock 就在卡片背后，晚淡出会
        // 看到两层图标叠加（黑边/重影）。
        let dock_opacity = (1.0 - p * 2.5).clamp(0.0, 1.0);
        // 靠近 Dock 时把卡片配色过渡回 Dock 的玻璃底与主题文字。这样
        // p = 1 的卡片与真实 Dock 视觉一致，退出/进入都不会闪一下明暗。
        let theme = Theme::get(cx).clone();
        let dock_blend = ((p - 0.72) / 0.28).clamp(0.0, 1.0);
        let clear_black = gpui::Hsla {
            a: 0.0,
            ..gpui::black()
        };
        let dock_glass = overlay_color(theme.bg, theme.glass);
        let dock_title = if self.current_track.is_some() {
            theme.text
        } else {
            theme.text_dim
        };
        // 卡片自身必须保持不透明，否则会透出下方真实 Dock 的文字/图标。
        let card_bg = overlay_color(theme.bg, mix_color(gpui::black(), dock_glass, dock_blend));
        let card_border = mix_color(clear_black, theme.glass_border, dock_blend);
        let title_color = mix_color(gpui::white().opacity(0.9), dock_title, dock_blend);
        let artist_color = mix_color(gpui::white().opacity(0.45), theme.text_faint, dock_blend);

        // 背景铺满卡片；光栅分辨率 = 卡片区域 ÷ 设置里的除数。
        let bg_scale = self.background_render_scale();
        self.lyric_view.update(cx, |view, _| {
            // 背景按窗口绝对坐标绘制，必须带上卡片原点；否则卡片下移时
            // 背景会被祖先裁掉一截，底部露出卡片的黑色底，形成“黑边”。
            view.set_background_bounds(Some(Bounds::new(
                point(px(card.x), px(card.y)),
                size(px(card_w), px(card_h)),
            )));
            view.set_background_render_scale(bg_scale);
        });

        let dock_shared = p > 0.001;
        // 布局 1 只在「横向窗口 + 歌词可见 + 播放页」时启用；其余情况
        // （竖屏、无歌词、主动收起歌词、队列子页）统一落到布局 2。
        let wide_layout = self.portrait_page == PortraitPage::Art
            && self.layout_one_active(cx, width, height);
        let page = if wide_layout {
            self.render_immersive_wide(card_w, card_h, dock_shared, dock_opacity, window, cx)
        } else {
            self.render_immersive_compact(card_w, card_h, dock_shared, dock_opacity, window, cx)
        };

        let mut card_el = div()
            .id("immersive-card")
            .absolute()
            .left(px(card.x))
            .top(px(card.y))
            .w(px(card_w))
            .h(px(card_h));
        // 满屏时不要圆角/边框/底色，避免在窗口边缘留下黑边或裁切；
        // 只有在 Dock↔全屏过渡期间卡片才需要这些装饰。
        if p > 0.0001 {
            card_el = card_el
                .overflow_hidden()
                .rounded(px(18.0))
                .border_1()
                .border_color(card_border)
                .bg(card_bg);
        }
        card_el = card_el.child(
            div()
                .absolute()
                .inset_0()
                .opacity(content_opacity)
                .child(page),
        );

        // 共享舞台（封面/曲名/艺人）独立于页面缩放，从 Dock 内的小元素
        // 连续飞到沉浸页对应位置；坐标从窗口空间平移进卡片空间。
        if dock_shared {
            let mode = self.immersive_stage_mode(cx, width, height);
            let page_geometry = immersive_geometry(mode, width, height);
            let dock_stage = dock_geometry(width, height);
            let shared = page_geometry
                .lerp(dock_stage, p)
                .translated(-card.x, -card.y);
            card_el = card_el.child(self.render_shared_stage_colored(
                shared,
                title_color,
                artist_color,
            ));
        }

        // 「更多」抽屉画在页面与共享舞台之上；布局 1 / 布局 2 共用。
        if self.drawer_visible() {
            card_el = card_el.child(self.render_drawer(card_w, card_h, now, cx));
        }

        let mut root = div()
            .id("immersive-root")
            .size_full()
            .relative()
            .child(card_el)
            .children(Self::resize_handles());

        // 拖拽期间卡片会缩小、把手不再跟随指针；用一张满窗捕获层接管
        // 后续移动与松手，保证手势连续。
        if self.sheet_drag.is_some() {
            root = root.child(
                div()
                    .id("immersive-drag-capture")
                    .absolute()
                    .inset_0()
                    .cursor(CursorStyle::ClosedHand)
                    .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(gpui::MouseButton::Left) {
                            app.update_sheet_drag(f32::from(event.position.y), cx);
                        } else if app.sheet_drag.is_some_and(|drag| !drag.animating()) {
                            app.release_sheet_drag(height, cx);
                        }
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(move |app, _: &MouseUpEvent, _, cx| {
                            if app.sheet_drag.is_some() {
                                app.release_sheet_drag(height, cx);
                            }
                        }),
                    ),
            );
        }
        root.into_any_element()
    }

    /// 窄窗沉浸：AMLL react-full `VerticalLayout` 的等价几何（宽 < 高）。
    ///
    /// 800×1000 实测：行 drag 30 / thumb 30 / main 1fr（gap 2）；
    /// 小封面 6em=96（宽 ≤ 480 时 4.5em），x = 3em，y = main_top + 1em；
    /// 曲目信息在封面右侧垂直居中（16px：name 500/op.9、artists op.45），
    /// 菜单钮 35×35 贴右缘 3em；歌词铺满其余高度并对齐 0.1/top，
    /// mask 只在顶部 10% 渐隐；歌词打开时不显示传输控件（与 AMLL 竖向一致）。
    // ---- AMLL 播放器控件（横向/竖向共用同一套写法） ----

    /// AMLL BouncingSlider 进度条 + 时间/音质标签行。
    ///
    /// 右侧时间默认显示剩余时长，点一下切到总时长（AMLL
    /// `showRemainingTimeAtom` 的交互）。音质标签是 20% 白色圆角胶囊，
    /// 点击打开解码信息弹窗。
    fn amll_progress_block(
        &self,
        width: f32,
        label_font: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let status = &self.player_status;
        let duration = status.duration_ms.unwrap_or(0);
        let position = self.display_position_ms().min(duration.max(1));
        let remaining = duration.saturating_sub(position);
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };
        let quality = self
            .current_track
            .as_ref()
            .and_then(|t| t.quality.clone())
            .unwrap_or_default();
        let seek = {
            let player = self.player.clone();
            move |fraction: f32, _cx: &mut App| {
                if duration > 0 {
                    player.seek_ms((fraction.clamp(0.0, 1.0) * duration as f32) as u64);
                }
            }
        };
        let progress_bar = crate::widgets::amll_slider(progress, "seek", seek);

        // AMLL `.qualityTag`：高 ≤1080 时 scale=1，否则 1.4。
        let tag_scale = if height > 1080.0 { 1.4 } else { 1.0 };
        let quality_pill = if quality.is_empty() {
            div().into_any_element()
        } else {
            let weak = weak.clone();
            div()
                .id("amll-quality-tag")
                .flex()
                .items_center()
                .rounded(px(5.0 * tag_scale))
                .bg(white.opacity(0.2))
                .pl(px(1.1 * tag_scale))
                .pr(px(5.72 * tag_scale))
                .py(px(2.0 * tag_scale))
                .text_size(px(11.0 * tag_scale))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(white)
                .cursor_pointer()
                // AMLL whileHover/whileTap：scale 0.95 / 0.9。
                .hover(|s| s.bg(white.opacity(0.26)))
                .child(SharedString::from(quality.to_uppercase()))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.quality_dialog_open = true;
                        cx.notify();
                    });
                })
                .into_any_element()
        };

        let right_label = {
            let weak = weak.clone();
            let text = if self.show_remaining_time {
                format!("-{}", fmt_ms(remaining))
            } else {
                fmt_ms(duration)
            };
            div()
                .id("amll-time-toggle")
                .flex_1()
                .flex()
                .justify_end()
                .cursor_pointer()
                .child(SharedString::from(text))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.show_remaining_time = !app.show_remaining_time;
                        cx.notify();
                    });
                })
        };

        div()
            .w(px(width))
            .flex()
            .flex_col()
            .child(div().h(px(24.0)).child(progress_bar))
            .child(
                div()
                    .w_full()
                    .mt(px(4.0))
                    .flex()
                    .items_center()
                    .text_size(px(label_font))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(white.opacity(0.5))
                    .child(div().flex_1().child(SharedString::from(fmt_ms(position))))
                    .child(div().flex_none().flex().justify_center().child(quality_pill))
                    .child(right_label),
            )
            .into_any_element()
    }

    /// AMLL MediaButton 传输行：shuffle / prev / play / next / repeat。
    fn amll_transport(
        &self,
        width: f32,
        btn: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let playing = self.player_status.state == PlayerState::Playing;
        let press_enter = Self::anim_progress(self.ui_anim_enter, Self::UI_ANIM_PRESS_MS);
        let press_play = Self::anim_progress(
            Self::latest_anim(self.ui_anim_enter, self.ui_anim_play),
            Self::UI_ANIM_PRESS_MS,
        );
        let press_shuffle = Self::anim_progress(
            Self::latest_anim(self.ui_anim_enter, self.ui_anim_shuffle),
            Self::UI_ANIM_PRESS_MS,
        );
        let press_repeat = Self::anim_progress(
            Self::latest_anim(self.ui_anim_enter, self.ui_anim_repeat),
            Self::UI_ANIM_PRESS_MS,
        );
        let arrows = Self::anim_progress(self.ui_anim_enter, Self::UI_ANIM_ARROWS_MS);
        // AMLL `.songMediaButton > svg` / `.songMediaPlayButton > svg` 的
        // 响应式 scale。svg intrinsic size 分别是 32 与 38；我们的 16 网格
        // 已按同一 viewBox 比例归一，所以画布 = intrinsic × scale。
        let media_scale = if height > 1080.0 {
            3.0
        } else if height > 768.0 {
            2.0
        } else if height > 512.0 {
            1.5
        } else {
            1.0
        };
        let play_scale = if height > 1080.0 {
            2.0
        } else if height > 768.0 {
            1.1
        } else if height > 512.0 {
            0.8
        } else {
            0.5
        };
        let icon_side = 20.8; // 1.3em
        let icon_skip = 32.0 * media_scale;
        let icon_play = 38.0 * play_scale;
        let shuffle_active = self.play_mode == PlayMode::Shuffle;
        let repeat_active = matches!(self.play_mode, PlayMode::RepeatAll | PlayMode::RepeatOne);
        // AMLL 的 shuffle/repeat 激活态是整块白色圆角图标；没有 knockout
        // 能力时用 22% 白色底 + 白色字形近似，保持同样的尺寸与位置。
        let icon_chip = |active: bool| {
            div()
                .size(px(icon_side))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .justify_center()
                .when(active, |d| d.bg(white.opacity(0.22)))
        };
        let sp = || div().flex_1();

        let shuffle = {
            let weak = weak.clone();
            div()
                .id("amll-shuffle")
                .size(px(btn))
                .rounded(px(btn / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.133)))
                .child(icon_chip(shuffle_active).child(crate::icons::icon_motion(
                    crate::icons::Icon::Shuffle,
                    icon_side,
                    white,
                    crate::icons::IconMotion {
                        press: press_shuffle,
                        ..Default::default()
                    },
                )))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.toggle_amll_shuffle();
                        app.ui_anim_shuffle = Some(Instant::now());
                        app.ensure_ui_anim_ticker(cx);
                        cx.notify();
                    });
                })
        };
        let prev = {
            let weak = weak.clone();
            div()
                .id("amll-prev")
                .size(px(btn))
                .rounded(px(btn / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.133)))
                .child(crate::icons::icon_motion(
                    crate::icons::Icon::Rewind,
                    icon_skip,
                    white,
                    crate::icons::IconMotion {
                        press: press_enter,
                        arrows,
                        ..Default::default()
                    },
                ))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_prev(cx));
                })
        };
        let play = {
            let weak = weak.clone();
            div()
                .id("amll-play")
                .size(px(btn))
                .rounded(px(btn / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.133)))
                .child(crate::icons::icon_motion(
                    if playing {
                        crate::icons::Icon::PauseMedia
                    } else {
                        crate::icons::Icon::PlayMedia
                    },
                    icon_play,
                    white,
                    crate::icons::IconMotion {
                        press: press_play,
                        ..Default::default()
                    },
                ))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_playback(cx));
                })
        };
        let next = {
            let weak = weak.clone();
            div()
                .id("amll-next")
                .size(px(btn))
                .rounded(px(btn / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.133)))
                .child(crate::icons::icon_motion(
                    crate::icons::Icon::Forward,
                    icon_skip,
                    white,
                    crate::icons::IconMotion {
                        press: press_enter,
                        arrows,
                        ..Default::default()
                    },
                ))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.play_next(cx));
                })
        };
        let repeat = {
            let weak = weak.clone();
            div()
                .id("amll-repeat")
                .size(px(btn))
                .rounded(px(btn / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.133)))
                .child(icon_chip(repeat_active).child(crate::icons::icon_motion(
                    if self.play_mode == PlayMode::RepeatOne {
                        crate::icons::Icon::RepeatOne
                    } else {
                        crate::icons::Icon::Repeat
                    },
                    icon_side,
                    white,
                    crate::icons::IconMotion {
                        press: press_repeat,
                        ..Default::default()
                    },
                )))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.cycle_amll_repeat();
                        app.ui_anim_repeat = Some(Instant::now());
                        app.ensure_ui_anim_ticker(cx);
                        cx.notify();
                    });
                })
        };
        div()
            .w(px(width))
            .flex()
            .items_center()
            .child(shuffle)
            .child(sp())
            .child(prev)
            .child(sp())
            .child(play)
            .child(sp())
            .child(next)
            .child(sp())
            .child(repeat)
            .into_any_element()
    }

    /// AMLL VolumeControl 等价音量行。
    fn amll_volume_row(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let speaker_progress = Self::anim_progress(
            self.ui_anim_volume.map(|(t, _)| t),
            Self::UI_ANIM_SPEAKER_MS,
        );
        let (speaker_low, speaker_high) = match self.ui_anim_volume {
            Some((_, true)) => (0.0, speaker_progress),
            Some((_, false)) => (speaker_progress, 0.0),
            None => (0.0, 0.0),
        };
        let volume_slider = {
            let player = self.player.clone();
            crate::widgets::amll_live_slider(
                self.player_status.volume.clamp(0.0, 1.0),
                "volume",
                move |fraction: f32, cx: &mut App| {
                    player.set_volume(fraction);
                    let _ = weak.update(cx, |app, cx| {
                        app.note_volume_input(fraction, cx);
                    });
                },
            )
        };
        // AMLL VolumeControl：行高 40（svg 布局高度），左右负边距 -12 / -8，
        // 喇叭的可见图形只占 svg 画布的一小部分。
        div()
            .w(px(width + 20.0))
            .ml(px(-12.0))
            .mr(px(-8.0))
            .h(px(40.0))
            .flex()
            .items_center()
            .child(
                div().w(px(32.0)).h(px(40.0)).flex().items_center().justify_center().child(
                    crate::icons::icon_motion(
                        crate::icons::Icon::VolumeLow,
                        26.7,
                        white.opacity(0.5),
                        crate::icons::IconMotion {
                            speaker: speaker_low,
                            ..Default::default()
                        },
                    ),
                ),
            )
            .child(div().flex_1().child(volume_slider))
            .child(
                div().w(px(43.0)).h(px(40.0)).flex().items_center().justify_center().child(
                    crate::icons::icon_motion(
                        crate::icons::Icon::VolumeHigh,
                        33.4,
                        white.opacity(0.5),
                        crate::icons::IconMotion {
                            speaker: speaker_high,
                            ..Default::default()
                        },
                    ),
                ),
            )
            .into_any_element()
    }

    /// 布局 1 / 布局 2 共用的底部控制行。
    ///
    /// 左：全屏；右：队列 ·（歌词）· 更多 · 关闭。两套布局只通过
    /// `show_lyrics` 决定是否露出直接歌词开关，其余按钮、尺寸、间距与
    /// 交互完全一致，不再维护两套 Dock。
    fn immersive_bottom_row(
        &self,
        width: f32,
        show_lyrics: bool,
        lyrics_on: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let btn = |icon: crate::icons::Icon,
                   id: &'static str,
                   active: bool,
                   on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>| {
            div()
                .id(id)
                .size(px(40.0))
                .rounded(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .opacity(if active { 1.0 } else { 0.45 })
                .hover(|s| s.opacity(1.0).bg(white.opacity(0.133)))
                .child(crate::icons::icon(icon, 18.0, white))
                .on_click(on_click)
                .into_any_element()
        };

        let fullscreen = btn(
            crate::icons::Icon::Expand,
            "immersive-fullscreen",
            false,
            Box::new(|_, window, _| window.toggle_fullscreen()),
        );
        let queue = {
            let weak = weak.clone();
            btn(
                crate::icons::Icon::Queue,
                "immersive-queue",
                self.portrait_page == PortraitPage::Queue,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.close_drawer(cx);
                        app.set_portrait_page(PortraitPage::Queue, cx);
                    });
                }),
            )
        };
        let lyrics = show_lyrics.then(|| {
            let weak = weak.clone();
            btn(
                crate::icons::Icon::Lyrics,
                "immersive-lyrics",
                lyrics_on,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.toggle_wide_lyrics(cx));
                }),
            )
        });
        let more = {
            let weak = weak.clone();
            btn(
                crate::icons::Icon::More,
                "immersive-more",
                self.drawer_open,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        if app.drawer_open {
                            app.close_drawer(cx);
                        } else {
                            app.open_drawer(cx);
                        }
                    });
                }),
            )
        };
        let close = {
            let weak = weak.clone();
            btn(
                crate::icons::Icon::Close,
                "immersive-close",
                false,
                Box::new(move |_, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.hide_drawer_now();
                        app.exit_immersive(cx);
                        window.focus(&app.focus_handle);
                        cx.notify();
                    });
                }),
            )
        };

        // 歌词列一直铺到卡片底边，整条控制行压在歌词之上。gpui 的普通
        // 命中区不会遮挡背后的命中区，不加 occlude 时点按钮会同时命中
        // 歌词视图的 on_mouse_down，误触「点歌词跳转时间轴」。这里把整条
        // 控制行声明为遮挡层：按钮照常可点，但背后的歌词不再接收点击。
        div()
            .w(px(width))
            .h(px(40.0))
            .flex()
            .items_center()
            .justify_between()
            .occlude()
            .child(fullscreen)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .child(queue)
                    .when_some(lyrics, |row, button| row.child(button))
                    .child(more)
                    .child(close),
            )
            .into_any_element()
    }

    /// 底部「更多」抽屉：布局 1 / 布局 2 共用。
    ///
    /// 一级页收纳定时、均衡器、歌词开关、队列、音频信息与艺术家；
    /// 定时选项是抽屉内的二级页，不再从 Dock 弹浮层。
    fn render_drawer(
        &self,
        width: f32,
        height: f32,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let phone_main = self.phone_shell_active();
        let progress = self.drawer_progress(now);
        let panel_w = (width - 24.0).clamp(280.0, 520.0).min(width.max(1.0));
        let panel_h = ((height * 0.68).clamp(280.0, 540.0)).min((height - 12.0).max(180.0));
        let left = ((width - panel_w) * 0.5).max(0.0);
        let top = height - panel_h * progress;

        let row = |icon: crate::icons::Icon,
                   id: &'static str,
                   title: &'static str,
                   detail: String,
                   active: bool,
                   on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>| {
            let mut labels = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .max_w_full()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(white.opacity(0.88))
                        .text_ellipsis()
                        .child(title),
                );
            if !detail.is_empty() {
                labels = labels.child(
                    div()
                        .max_w_full()
                        .text_size(px(11.0))
                        .text_color(white.opacity(0.42))
                        .text_ellipsis()
                        .child(SharedString::from(detail)),
                );
            }
            div()
                .id(id)
                .h(px(46.0))
                .w_full()
                .flex_none()
                .rounded(px(10.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .px(px(10.0))
                .cursor_pointer()
                .hover(|s| s.bg(white.opacity(0.06)))
                .child(crate::icons::icon(icon, 18.0, white.opacity(0.72)))
                .child(labels)
                .when(active, |d| {
                    d.child(crate::icons::icon(
                        crate::icons::Icon::Check,
                        14.0,
                        white.opacity(0.8),
                    ))
                })
                .on_click(on_click)
                .into_any_element()
        };

        let (panel_title, back_row) = match self.drawer_page {
            DrawerPage::Root => (if phone_main { "菜单" } else { "更多" }, None),
            DrawerPage::Sleep => {
                let weak = weak.clone();
                let back = row(
                    crate::icons::Icon::ChevronLeft,
                    "drawer-back",
                    "返回",
                    String::new(),
                    false,
                    Box::new(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.drawer_page = DrawerPage::Root;
                            cx.notify();
                        });
                    }),
                );
                ("睡眠定时", Some(back))
            }
        };

        let mut body = div().w_full().flex().flex_col().gap(px(2.0));
        if let Some(back) = back_row {
            body = body.child(back);
        }
        match self.drawer_page {
            DrawerPage::Root => {
                // 手机壳没有侧栏与标题栏按钮，抽屉就是唯一的导航面板：
                // 播放页 / 全部音乐 / 搜索 / 设置 / 主题都从这里进出。
                if phone_main {
                    let section = div()
                        .px(px(10.0))
                        .pt(px(4.0))
                        .pb(px(2.0))
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(white.opacity(0.38))
                        .child("导航");
                    let current = self
                        .current_track
                        .as_ref()
                        .map(|t| t.title.clone())
                        .unwrap_or_else(|| "未在播放".into());
                    let player = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Play,
                            "drawer-phone-player",
                            "正在播放",
                            current,
                            self.phone_page == PhonePage::Player,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Player;
                                    app.close_drawer(cx);
                                    cx.notify();
                                });
                            }),
                        )
                    };
                    let lyrics = self.current_track.is_some().then(|| {
                        let weak = weak.clone();
                        let has_lyrics = self.lyric_view.read(cx).has_lyrics();
                        row(
                            crate::icons::Icon::Lyrics,
                            "drawer-phone-lyrics",
                            "歌词",
                            if has_lyrics {
                                "全屏滚动歌词".into()
                            } else {
                                "暂无歌词".into()
                            },
                            self.phone_page == PhonePage::Lyrics,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Lyrics;
                                    app.close_drawer(cx);
                                    cx.notify();
                                });
                            }),
                        )
                    });
                    let library = {
                        let weak = weak.clone();
                        let active = self.phone_page == PhonePage::Browse
                            && self.view_tab == ViewTab::Library
                            && self.library_filter == LibraryFilter::All;
                        row(
                            crate::icons::Icon::Note,
                            "drawer-phone-library",
                            "所有音乐",
                            format!("{} 首", self.library.len()),
                            active,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Browse;
                                    app.view_tab = ViewTab::Library;
                                    app.library_filter = LibraryFilter::All;
                                    app.source_route = None;
                                    app.close_drawer(cx);
                                    cx.notify();
                                });
                            }),
                        )
                    };
                    let search = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Search,
                            "drawer-phone-search",
                            "搜索",
                            "歌曲 / 专辑 / 歌手 / 歌单".into(),
                            self.phone_page == PhonePage::Browse
                                && self.view_tab == ViewTab::Search,
                            Box::new(move |_, window, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.phone_page = PhonePage::Browse;
                                    app.view_tab = ViewTab::Search;
                                    app.close_drawer(cx);
                                    focus_input(&app.search_input, window, cx);
                                    cx.notify();
                                });
                            }),
                        )
                    };
                    let settings = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Settings,
                            "drawer-phone-settings",
                            "设置",
                            "外观、播放、音源与存储".into(),
                            self.settings_open,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.hide_drawer_now();
                                    app.open_settings(cx);
                                });
                            }),
                        )
                    };
                    let theme_row = {
                        let weak = weak.clone();
                        let dark = Theme::get(cx).is_dark();
                        row(
                            if dark {
                                crate::icons::Icon::Sun
                            } else {
                                crate::icons::Icon::Moon
                            },
                            "drawer-phone-theme",
                            if dark { "切换到浅色" } else { "切换到深色" },
                            String::new(),
                            false,
                            Box::new(move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.toggle_appearance(cx);
                                    cx.notify();
                                });
                            }),
                        )
                    };
                    body = body.child(section).child(player);
                    if let Some(lyrics) = lyrics {
                        body = body.child(lyrics);
                    }
                    body = body
                        .child(library)
                        .child(search)
                        .child(settings)
                        .child(theme_row)
                        .child(
                            div()
                                .h(px(1.0))
                                .my(px(4.0))
                                .bg(white.opacity(0.08)),
                        );
                }
                let sleep_detail = self
                    .sleep_minutes
                    .map(|m| format!("{m} 分钟后暂停"))
                    .unwrap_or_else(|| "未设置".into());
                let sleep = {
                    let weak = weak.clone();
                    row(
                        crate::icons::Icon::Clock,
                        "drawer-sleep",
                        "睡眠定时",
                        sleep_detail,
                        self.sleep_minutes.is_some(),
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.drawer_page = DrawerPage::Sleep;
                                cx.notify();
                            });
                        }),
                    )
                };
                let eq_detail = format!(
                    "{} · {}",
                    self.eq.preset_name,
                    if self.eq.settings.enabled {
                        "已开启"
                    } else {
                        "已关闭"
                    }
                );
                let eq = {
                    let weak = weak.clone();
                    row(
                        crate::icons::Icon::Equalizer,
                        "drawer-eq",
                        "均衡器",
                        eq_detail,
                        self.eq.settings.enabled,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.hide_drawer_now();
                                app.open_eq(cx);
                            });
                        }),
                    )
                };
                let queue = {
                    let weak = weak.clone();
                    let active = phone_main
                        && self.phone_page == PhonePage::Browse
                        && self.view_tab == ViewTab::Library
                        && self.library_filter == LibraryFilter::Queue;
                    row(
                        crate::icons::Icon::Queue,
                        "drawer-queue",
                        "播放队列",
                        format!("{} 首", self.queue.len()),
                        active,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.hide_drawer_now();
                                if app.phone_shell_active() {
                                    app.phone_page = PhonePage::Browse;
                                    app.view_tab = ViewTab::Library;
                                    app.library_filter = LibraryFilter::Queue;
                                } else {
                                    app.set_portrait_page(PortraitPage::Queue, cx);
                                }
                                cx.notify();
                            });
                        }),
                    )
                };
                body = body.child(sleep).child(eq);
                if self.layout_can_toggle_lyrics(cx) {
                    let weak = weak.clone();
                    let visible = !self.wide_lyrics_hidden;
                    body = body.child(row(
                        crate::icons::Icon::Lyrics,
                        "drawer-lyrics",
                        "歌词",
                        if visible {
                            "显示中 · 点击收起".into()
                        } else {
                            "已收起 · 点击显示".into()
                        },
                        visible,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.close_drawer(cx);
                                app.toggle_wide_lyrics(cx);
                            });
                        }),
                    ));
                }
                body = body.child(queue);

                let quality = self
                    .current_track
                    .as_ref()
                    .and_then(|t| t.quality.clone())
                    .unwrap_or_default();
                let source = self
                    .current_track
                    .as_ref()
                    .map(|t| match &t.origin {
                        TrackOrigin::Local { .. } => "本地音乐".to_string(),
                        TrackOrigin::Remote { source } => match source.0.as_str() {
                            "netease" => "网易云音乐".to_string(),
                            other => other.to_string(),
                        },
                    })
                    .unwrap_or_else(|| "—".into());
                body = body.child(
                    div()
                        .w_full()
                        .flex_none()
                        .rounded(px(10.0))
                        .bg(white.opacity(0.05))
                        .border_1()
                        .border_color(white.opacity(0.06))
                        .p(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(white.opacity(0.8))
                                .child("音频信息"),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(white.opacity(0.45))
                                .child(SharedString::from(if quality.is_empty() {
                                    "音质信息未知".to_string()
                                } else {
                                    quality
                                })),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(white.opacity(0.45))
                                .child(SharedString::from(format!("音源：{source}"))),
                        ),
                );

                let artists = self
                    .current_track
                    .as_ref()
                    .map(|t| t.artists.clone())
                    .unwrap_or_default();
                if !artists.is_empty() {
                    let mut card = div()
                        .w_full()
                        .flex_none()
                        .rounded(px(10.0))
                        .bg(white.opacity(0.05))
                        .border_1()
                        .border_color(white.opacity(0.06))
                        .p(px(10.0))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .px(px(2.0))
                                .pb(px(2.0))
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(white.opacity(0.8))
                                .child("参与创作的艺术家"),
                        );
                    for artist in artists {
                        let weak_artist = weak.clone();
                        let linked = artist.id.is_some();
                        card = card.child(
                            div()
                                .id(SharedString::from(format!(
                                    "drawer-artist-{}",
                                    artist.name
                                )))
                                .h(px(34.0))
                                .flex()
                                .items_center()
                                .gap(px(9.0))
                                .px(px(2.0))
                                .rounded(px(7.0))
                                .when(linked, |d| {
                                    d.cursor_pointer().hover(|s| s.bg(white.opacity(0.05)))
                                })
                                .child(
                                    div()
                                        .size(px(24.0))
                                        .rounded(px(12.0))
                                        .bg(white.opacity(0.08))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(crate::icons::icon(
                                            crate::icons::Icon::Person,
                                            12.0,
                                            white.opacity(0.55),
                                        )),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(12.0))
                                        .text_color(white.opacity(0.82))
                                        .text_ellipsis()
                                        .child(SharedString::from(artist.name.clone())),
                                )
                                .when(linked, |d| {
                                    d.on_click(move |_, window, cx| {
                                        let _ = weak_artist.update(cx, |app, cx| {
                                            app.hide_drawer_now();
                                            app.exit_immersive(cx);
                                            app.open_artist_for_current(cx);
                                            window.focus(&app.focus_handle);
                                            cx.notify();
                                        });
                                    })
                                }),
                        );
                    }
                    body = body.child(card);
                }

                // 「退出播放页」只属于沉浸模态；手机壳的曲库页由返回键
                // 和底部曲库按钮处理，不能再出现一个名字不符的入口。
                if self.immersive {
                    let exit = {
                        let weak = weak.clone();
                        row(
                            crate::icons::Icon::Close,
                            "drawer-exit",
                            "退出播放页",
                            "Esc".into(),
                            false,
                            Box::new(move |_, window, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.hide_drawer_now();
                                    app.exit_immersive(cx);
                                    window.focus(&app.focus_handle);
                                    cx.notify();
                                });
                            }),
                        )
                    };
                    body = body.child(exit);
                }
            }
            DrawerPage::Sleep => {
                for (label, minutes) in [
                    ("15 分钟", Some(15u32)),
                    ("30 分钟", Some(30u32)),
                    ("60 分钟", Some(60u32)),
                    ("关闭定时", None),
                ] {
                    let weak = weak.clone();
                    let active = self.sleep_minutes == minutes;
                    body = body.child(row(
                        crate::icons::Icon::Clock,
                        match minutes {
                            Some(15) => "drawer-sleep-15",
                            Some(30) => "drawer-sleep-30",
                            Some(60) => "drawer-sleep-60",
                            _ => "drawer-sleep-off",
                        },
                        label,
                        if active {
                            "当前已启用".into()
                        } else {
                            String::new()
                        },
                        active,
                        Box::new(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                app.set_sleep_minutes(minutes, cx);
                                app.close_drawer(cx);
                            });
                        }),
                    ));
                }
            }
        }

        let header_close = {
            let weak = weak.clone();
            div()
                .id("drawer-close")
                .size(px(28.0))
                .rounded(px(14.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.1)))
                .child(crate::icons::icon(
                    crate::icons::Icon::Close,
                    14.0,
                    white.opacity(0.7),
                ))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_drawer(cx));
                })
        };

        let panel = div()
            .id("immersive-drawer-panel")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(panel_w))
            .h(px(panel_h))
            .rounded(px(20.0))
            .border_1()
            .border_color(white.opacity(0.10))
            .bg(gpui::rgb(0x1C1C22))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w_full()
                    .pt(px(8.0))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w(px(38.0))
                            .h(px(4.0))
                            .rounded(px(2.0))
                            .bg(white.opacity(0.18)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .px(px(16.0))
                    .pt(px(4.0))
                    .pb(px(8.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(white.opacity(0.92))
                            .child(panel_title),
                    )
                    .child(header_close),
            )
            .child(
                div()
                    .id("drawer-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(self.smooth_scroll, |d| d.smooth_scroll())
                    .px(px(10.0))
                    .pb(px(12.0))
                    .child(body),
            );

        let scrim = {
            let weak = weak.clone();
            div()
                .id("immersive-drawer-scrim")
                .absolute()
                .inset_0()
                .bg(gpui::black().opacity(0.38 * progress))
                .occlude()
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.close_drawer(cx));
                })
        };

        scrim.child(panel).into_any_element()
    }

    /// 沉浸层统一的曲名文案。共享元素与页面占位共用同一份文本，
    /// 保证补间结束交还静态布局时不会发生文字替换。
    fn current_title_text(&self) -> String {
        self.current_track
            .as_ref()
            .map(|t| t.title.clone())
            .or_else(|| self.player_status.title.clone())
            .unwrap_or_else(|| "未在播放".into())
    }

    /// 沉浸层统一的艺人文案（AMLL 语义：`A, B & C`，无艺人时回退专辑）。
    fn current_artist_text(&self) -> String {
        match self.current_track.as_ref() {
            Some(track) => {
                let names: Vec<&str> = track
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .filter(|n| !n.is_empty())
                    .collect();
                match names.len() {
                    0 => track
                        .album
                        .as_ref()
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| "未知艺人".into()),
                    1 => names[0].to_string(),
                    n => format!("{} & {}", names[..n - 1].join(", "), names[n - 1]),
                }
            }
            None => "选择一首歌开始".into(),
        }
    }

    /// 只有曲目信息与右侧圆钮（封面态放在大封面下方）。
    fn amll_info_row(
        &self,
        width: f32,
        font: f32,
        height: f32,
        back: bool,
        interactive: bool,
        hide_text: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let title = self.current_title_text();
        let artist = self.current_artist_text();
        // AMLL MenuButton：width 3.5vh；窄屏改用 max(2em, min(5vw, 4vh))。
        let action_side = if self.viewport_width <= 1000.0 {
            ((self.viewport_width * 0.05).min(height * 0.04)).max(32.0)
        } else {
            height * 0.035
        };
        let action = {
            let base = div()
                .id("amll-info-action")
                .size(px(action_side))
                .rounded(px(action_side * 0.5))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .ml(px(16.0))
                .bg(white.opacity(0.082))
                .child(crate::icons::icon(
                    if back {
                        crate::icons::Icon::ChevronLeft
                    } else {
                        crate::icons::Icon::MoreDot
                    },
                    action_side * 0.72,
                    white,
                ));
            if interactive {
                let weak = weak.clone();
                base.on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        if back {
                            app.set_portrait_page(PortraitPage::Art, cx);
                        } else {
                            app.open_drawer(cx);
                        }
                    });
                })
                .into_any_element()
            } else {
                base.into_any_element()
            }
        };
        let available = (width - action_side - 16.0).max(1.0);
        let title_w = measure_text_width(window, &title, font, gpui::FontWeight::MEDIUM);
        let artist_w = measure_text_width(window, &artist, font, gpui::FontWeight::NORMAL);
        let now = Instant::now();
        let marquee = self.info_marquee;
        let make_line = |line: u8,
                         text: String,
                         measured: f32,
                         weight: gpui::FontWeight,
                         color: gpui::Hsla|
         -> AnyElement {
            // AMLL TextMarquee：只有当文字宽度超过容器 95% 时，悬停才滚动。
            let overflowing = measured > available * 0.95;
            let distance = (measured - available * 0.95).max(0.0);
            let duration_ms = (distance * 2.0 / 64.0 * 1000.0).max(1.0);
            let active = marquee.filter(|m| m.line == line);
            let animating = active.is_some_and(|m| m.active(now));
            let offset = active.map(|m| m.offset(now)).unwrap_or(0.0);
            let hover_weak = weak.clone();
            let mut outer = div()
                .id(if line == 0 {
                    "info-line-title"
                } else {
                    "info-line-artist"
                })
                .w_full()
                .overflow_hidden()
                .on_hover(move |hovered, _, cx| {
                    if !overflowing {
                        return;
                    }
                    let _ = hover_weak.update(cx, |app, cx| {
                        if *hovered {
                            app.info_marquee = Some(InfoMarquee {
                                line,
                                start: Instant::now(),
                                distance,
                                duration_ms,
                            });
                        } else if app.info_marquee.is_some_and(|m| m.line == line) {
                            app.info_marquee = None;
                        }
                        cx.notify();
                    });
                });
            if animating {
                outer = outer.child(
                    div()
                        .w(px(measured + 2.0))
                        .ml(px(-offset))
                        .text_size(px(font))
                        .line_height(px(font * INFO_LINE_RATIO))
                        .font_weight(weight)
                        .text_color(color)
                        .whitespace_nowrap()
                        .child(SharedString::from(text)),
                );
            } else {
                outer = outer.child(
                    div()
                        .w_full()
                        .text_size(px(font))
                        .line_height(px(font * INFO_LINE_RATIO))
                        .font_weight(weight)
                        .text_color(color)
                        .text_ellipsis()
                        .child(SharedString::from(text)),
                );
            }
            outer.into_any_element()
        };
        div()
            .w(px(width))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .opacity(if hide_text { 0.0 } else { 1.0 })
                    .child(make_line(
                        0,
                        title,
                        title_w,
                        gpui::FontWeight::MEDIUM,
                        white.opacity(0.9),
                    ))
                    .child(make_line(
                        1,
                        artist,
                        artist_w,
                        gpui::FontWeight::NORMAL,
                        white.opacity(0.45),
                    )),
            )
            .child(action)
            .into_any_element()
    }

    /// 桌面宽屏与竖屏共用的 AMLL 播放器列。
    ///
    /// 封面、信息、进度、传输、音量共用同一个列宽；歌词关闭/无歌词时由调用方
    /// 把这整列水平居中。这样两端只有定位不同，控件本身不再各写一套。
    fn amll_player_column(
        &self,
        column_w: f32,
        height: f32,
        controls_h: f32,
        chrome_o: f32,
        menu_interactive: bool,
        shared_hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let theme = Theme::dark();
        // AMLL Cover：半径 max(2%, 0.7vh)；整块带
        // `0 1em 1.2em rgba(0,0,0,.19)` 投影。暂停时整体缩到 0.75。
        let cover_scale = self
            .cover_scale_now(Instant::now())
            .clamp(0.5, 1.1);
        let cover_side = column_w * cover_scale;
        let cover_radius = (column_w * 0.02).max(height * 0.007) * cover_scale;
        let cover = div()
            .w(px(column_w))
            .h(px(column_w))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(cover_side))
                    .rounded(px(cover_radius))
                    .shadow(vec![BoxShadow {
                        color: gpui::black().opacity(0.19),
                        offset: point(px(0.0), px(16.0 * cover_scale)),
                        blur_radius: px(19.2 * cover_scale),
                        spread_radius: px(0.0),
                    }])
                    .child(crate::widgets::cover_plain(
                        &theme,
                        self.cover_image.clone(),
                        cover_side,
                        cover_radius,
                        crate::icons::icon(
                            crate::icons::Icon::Note,
                            (cover_side * 0.1).clamp(28.0, 40.0),
                            white.opacity(0.4),
                        ),
                    )),
            );
        let info_font = (height * 0.02).max(16.0);
        let info = div()
            .w(px(column_w))
            .opacity(chrome_o)
            .child(self.amll_info_row(
                column_w,
                info_font,
                height,
                false,
                menu_interactive,
                shared_hidden,
                window,
                cx,
            ));
        let label_font = (height * 0.012).max(12.8);
        let progress = div()
            .w(px(column_w))
            .h(px(45.2))
            .opacity(chrome_o)
            .child(self.amll_progress_block(column_w, label_font, height, cx));
        // 手机最小宽度时封面列只有 ~184px；40px 的按钮会让 5 个传输键
        // 互相挤压。低于 240 时收到 34px，图标仍保持可点击尺寸。
        let btn = (column_w * 0.18).clamp(if column_w < 240.0 { 34.0 } else { 40.0 }, 88.0);
        let transport = self.amll_transport(column_w, btn, height, cx);
        let volume = self.amll_volume_row(column_w, cx);
        let controls = div()
            .w(px(column_w))
            .h(px(controls_h.max(0.0)))
            .flex()
            .flex_col()
            .items_center()
            .justify_between()
            .child(info)
            .child(progress)
            .child(transport)
            .child(volume);
        div()
            .w(px(column_w))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .opacity(if shared_hidden { 0.0 } else { 1.0 })
                    .child(cover),
            )
            // AMLL：封面与 music-info 行之间是 8px grid gap，控件列再有
            // `margin-top: calc(-8px + 1.75em)`，合起来 28px（1em = 16px）。
            .child(div().h(px(28.0)))
            .child(controls)
            .into_any_element()
    }

    /// 共享元素舞台：一帧内只画一份封面 + 曲名/艺人，用插值几何定位。
    ///
    /// 调用方已在页面里把同名元素设为透明（保留占位），因此这里不做交叉
    /// 淡入——两端文案一致，字号、行高、位置连续变化即可。
    fn render_shared_stage(&self, geometry: StageGeometry) -> AnyElement {
        let white = gpui::white();
        self.render_shared_stage_colored(geometry, white.opacity(0.9), white.opacity(0.45))
    }

    /// 带文字的共享舞台；Dock 收尾时文字颜色会过渡回主面板主题色。
    fn render_shared_stage_colored(
        &self,
        geometry: StageGeometry,
        title_color: gpui::Hsla,
        artist_color: gpui::Hsla,
    ) -> AnyElement {
        let white = gpui::white();
        let theme = Theme::dark();
        let title = self.current_title_text();
        let artist = self.current_artist_text();
        let cover_size = geometry.cover.w.max(1.0);
        let cover = crate::widgets::cover_plain(
            &theme,
            self.cover_image.clone(),
            cover_size,
            geometry.cover_radius,
            crate::icons::icon(
                crate::icons::Icon::Note,
                (cover_size * 0.22).clamp(14.0, 40.0),
                white.opacity(0.35),
            ),
        );
        div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .left(px(geometry.cover.x))
                    .top(px(geometry.cover.y))
                    .child(cover),
            )
            .child(
                div()
                    .absolute()
                    .left(px(geometry.info.x))
                    .top(px(geometry.info.y))
                    .w(px(geometry.info.w.max(1.0)))
                    .flex()
                    .flex_col()
                    .gap(px(geometry.line_gap))
                    .child(
                        div()
                            .max_w_full()
                            .text_size(px(geometry.title_size))
                            .line_height(px(geometry.title_size * INFO_LINE_RATIO))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(title_color)
                            .text_ellipsis()
                            .child(SharedString::from(title)),
                    )
                    .child(
                        div()
                            .max_w_full()
                            .text_size(px(geometry.artist_size))
                            .line_height(px(geometry.artist_size * INFO_LINE_RATIO))
                            .text_color(artist_color)
                            .text_ellipsis()
                            .child(SharedString::from(artist)),
                    ),
            )
            .into_any_element()
    }

    /// AMLL 顶部拖拽 pill（50×8，白 17.6%）。
    ///
    /// 它只是把手的视觉标识；按下命中区由外层 dismiss-handle 承担，
    /// 拖动是「下拉关闭沉浸页」而不是移动窗口。
    fn amll_pill(&self, left: f32, top: f32, id: &'static str) -> AnyElement {
        div()
            .id(id)
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(50.0))
            .h(px(8.0))
            .rounded(px(4.0))
            .bg(gpui::white().opacity(0.176))
            .into_any_element()
    }

    /// 竖屏播放页：AMLL 竖向布局 + Salt Player 的子页面/自动隐藏逻辑。
    fn render_immersive_compact(
        &mut self,
        width: f32,
        height: f32,
        dock_shared: bool,
        dock_opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let now = Instant::now();
        // 同一列宽供封面、信息、进度、传输、音量与 Dock 使用。
        let content_w = compact_content_w(width, height);
        // AMLL 竖向布局的控件常驻：红框内的信息/进度与 Dock 不再淡出，
        // 避免停手 3.2 秒后中间留下一大片空白。
        let chrome_o = 1.0;
        let enter = self
            .portrait_enter
            .map(|t| (t.elapsed().as_secs_f32() / 0.26).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let enter = 1.0 - (1.0 - enter).powi(3);
        let page = self.portrait_page;
        let transition = self.portrait_transition.filter(|t| !t.done(now));
        // 共享元素补间：页面滑入/滑出时，封面与曲目信息走独立的 FLIP 路径，
        // 页面内的同名元素暂时透明，只保留占位。
        let stage = self
            .stage_motion
            .filter(|motion| motion.active(now) && motion.touches_compact())
            .map(|motion| motion.geometry(now))
            .filter(|geometry| geometry.is_valid());
        let shared_hidden = stage.is_some() || dock_shared;
        // 竖屏页不显示歌词：歌词视图退化为全窗 Mesh 背景。
        self.lyric_align = 0.1;
        self.lyric_view.update(cx, |view, _| {
            view.set_text_visible(false);
            view.set_click_seek_enabled(false);
            view.set_player_config(LyricPlayerConfig {
                align_anchor: AlignAnchor::Top,
                align_position: 0.1,
                enable_blur: !self.no_blur,
                ..LyricPlayerConfig::default()
            });
        });
        self.ensure_portrait_idle_ticker(cx);

        let lyric_view_el = div().absolute().inset_0().child(self.lyric_view.clone());

        // 顶部拖动带：播放页的整条页头都可拖动窗口；pill 附近保留
        // 下拉关闭手势。队列/更多页的页头有自己的可点元素，不铺拖动带。
        // Dock→全屏的位移由外层卡片承担，页面内部不再二次偏移。
        let sheet_offset = 0.0;
        let art_page = page == PortraitPage::Art && transition.is_none() && !self.eq_open;
        let drag_band = art_page.then(|| {
            Self::window_drag_area(
                div()
                    .id("immersive-drag-band-compact")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(COMPACT_TOP))
                    .cursor(CursorStyle::OpenHand),
            )
        });
        let handle_w = 120.0_f32.min(width);
        let handle_top = 0.0;
        let dismiss_handle = art_page.then(|| {
            div()
                .id("immersive-dismiss-handle-compact")
                .absolute()
                .top(px(handle_top))
                .left(px((width - handle_w) / 2.0))
                .w(px(handle_w))
                .h(px(56.0))
                .cursor(CursorStyle::OpenHand)
                .occlude()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|app, event: &MouseDownEvent, _, cx| {
                        app.begin_sheet_drag(f32::from(event.position.y), cx);
                    }),
                )
                .child(self.amll_pill(
                    handle_w / 2.0 - 25.0,
                    20.0 - handle_top,
                    "immersive-pill-compact",
                ))
        });

        // 页面本体：下拉时整块下移，露出背后的主面板。
        let mut sheet = div()
            .id("immersive-sheet-compact")
            .absolute()
            .left_0()
            .top(px(sheet_offset))
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(gpui::black())
            .opacity(enter)
            .child(lyric_view_el)
            .when_some(drag_band, |sheet, band| sheet.child(band))
            .when_some(dismiss_handle, |sheet, handle| sheet.child(handle));

        let overlay = div()
            .id("immersive-compact-overlay")
            .key_context("ChordialImmersive")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
                if app.quality_dialog_open {
                    app.quality_dialog_open = false;
                } else if app.drawer_open {
                    app.close_drawer(cx);
                } else if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.exit_immersive(cx);
                }
                window.focus(&app.focus_handle);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &TogglePortrait, _, cx| {
                app.exit_immersive(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &ToggleEqualizer, _, cx| {
                if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.open_eq(cx);
                }
            }))
            .on_action(cx.listener(|app, _: &PlayPause, _, cx| app.toggle_playback(cx)))
            .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
            .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
            .on_action(cx.listener(|app, _: &PlayFirst, _, cx| app.play_first(cx)))
            .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                app.bump_portrait(cx);
                let y = f32::from(event.position.y);
                match event.pressed_button {
                    Some(gpui::MouseButton::Left) => app.update_sheet_drag(y, cx),
                    None => {
                        if app.sheet_drag.is_some_and(|drag| !drag.animating()) {
                            app.release_sheet_drag(height, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |app, _: &MouseUpEvent, _, cx| {
                    if app.sheet_drag.is_some() {
                        app.release_sheet_drag(height, cx);
                    }
                }),
            )
            .on_click(cx.listener(|app, _, _, cx| app.bump_portrait(cx)))
            .size_full()
            .relative()
            .overflow_hidden();

        if let Some(tr) = transition {
            let (mut from_off, from_o, mut to_off, to_o) = tr.frames(now, width, height);
            // 从队列页返回播放页时方向相反：子页退出、播放页归位。
            if matches!(tr.from, PortraitPage::Queue) {
                from_off = -from_off;
                to_off = -to_off;
            }
            let (fx, fy) = match tr.kind {
                crate::player_page::TransitionKind::Vertical => (0.0, from_off),
                crate::player_page::TransitionKind::Fade => (0.0, 0.0),
            };
            let (tx, ty) = match tr.kind {
                crate::player_page::TransitionKind::Vertical => (0.0, to_off),
                crate::player_page::TransitionKind::Fade => (0.0, 0.0),
            };
            let from = gpui::batch_layer(self.render_compact_page(
                tr.from,
                content_w,
                height,
                width,
                chrome_o,
                dock_opacity,
                shared_hidden,
                window,
                cx,
            ));
            let to = gpui::batch_layer(self.render_compact_page(
                tr.to,
                content_w,
                height,
                width,
                chrome_o,
                dock_opacity,
                shared_hidden,
                window,
                cx,
            ));
            sheet = sheet
                .child(
                    div()
                        .absolute()
                        .left(px(fx))
                        .top(px(fy))
                        .size_full()
                        .opacity(from_o)
                        .child(from),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(tx))
                        .top(px(ty))
                        .size_full()
                        .opacity(to_o)
                        .child(to),
                );
            self.ensure_ui_anim_ticker(cx);
        } else {
            if self.portrait_transition.is_some() {
                self.portrait_transition = None;
            }
            sheet = sheet.child(gpui::batch_layer(self.render_compact_page(
                page,
                content_w,
                height,
                width,
                chrome_o,
                dock_opacity,
                shared_hidden,
                window,
                cx,
            )));
        }

        // 共享元素舞台画在页面之上：封面从大图连续缩到队列页头，
        // 曲名与艺人同步移动、缩放，而不是随页面一起硬切。
        if let Some(geometry) = stage {
            sheet = sheet.child(self.render_shared_stage(geometry));
        }

        if let Some((text, started)) = self.portrait_toast.clone() {
            if started.elapsed() < Duration::from_millis(1800) {
                sheet = sheet.child(
                    div()
                        .absolute()
                        .bottom(px(118.0))
                        .left(px((width - 300.0) / 2.0))
                        .w(px(300.0))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .rounded(px(16.0))
                                .bg(gpui::rgba(0x00000099))
                                .px(px(14.0))
                                .py(px(7.0))
                                .text_size(px(11.5))
                                .text_color(white.opacity(0.92))
                                .child(SharedString::from(text)),
                        ),
                );
            }
        }
        overlay.child(sheet).into_any_element()
    }

    fn render_compact_page(
        &self,
        page: PortraitPage,
        content_w: f32,
        height: f32,
        width: f32,
        chrome_o: f32,
        dock_opacity: f32,
        shared_hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match page {
            PortraitPage::Art => {
                self.render_compact_art(
                    width,
                    height,
                    chrome_o,
                    dock_opacity,
                    shared_hidden,
                    window,
                    cx,
                )
            }
            PortraitPage::Queue => self.render_portrait_queue(content_w, shared_hidden, cx),
        }
    }

    /// 竖屏播放页：与桌面宽屏同一根 `amll_player_column`，只是把整列水平居中。
    /// 页面不渲染歌词文本；歌词视图仅作为全窗 Mesh 背景。
    fn render_compact_art(
        &self,
        width: f32,
        height: f32,
        chrome_o: f32,
        dock_opacity: f32,
        shared_hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cover = compact_cover_size(width, height);
        let available = (height
            - COMPACT_TOP
            - crate::layout_morph::COMPACT_BOTTOM
            - crate::layout_morph::COMPACT_DOCK_H
            - 8.0)
            .max(0.0);
        let controls_h = (available - cover - COMPACT_COVER_GAP).max(0.0);
        // 从布局 1 / 队列页切换过来时，播放列跟随共享封面的 x 一起滑动，
        // 而不是整列硬切到居中位置。
        let now = Instant::now();
        let motion_x = self
            .stage_motion
            .filter(|motion| motion.active(now) && motion.touches_compact())
            .map(|motion| motion.geometry(now).cover.x);
        let left = motion_x.unwrap_or_else(|| ((width - cover) / 2.0).max(0.0));
        div()
            .size_full()
            .relative()
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(COMPACT_TOP))
                    .w(px(cover))
                    .child(self.amll_player_column(
                        cover,
                        height,
                        controls_h,
                        chrome_o,
                        true,
                        shared_hidden,
                        window,
                        cx,
                    )),
            )
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .bottom(px(crate::layout_morph::COMPACT_BOTTOM))
                    .w(px(cover))
                    .opacity(dock_opacity)
                    .child(self.immersive_bottom_row(cover, false, false, cx)),
            )
            .into_any_element()
    }

    /// 宽屏沉浸：AMLL react-full `HorizontalLayout` 的等价几何。
    ///
    /// 1600×1000 实测：列 info 0.45W / player 0.55W（gap 8px）；行
    /// drag = 0.12fr / thumb 40 / cover / music-info = 0.8fr / buttom 40 / tail = 0.08fr，
    /// fr = H − 120 − cover；封面 = min(45vh, 38vw)（H ≤ 1000，否则 50vh）；
    /// 歌词列右 padding 8%（H > 1000 时 15%）；歌词对齐封面中心（AMLL 用
    /// ResizeObserver 计算 cover 中线占比后作为 `alignPosition`）。
    fn render_immersive_wide(
        &mut self,
        width: f32,
        height: f32,
        dock_shared: bool,
        dock_opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = Instant::now();
        // 布局 1 只在歌词可见时启用；这里仍读一次状态，避免未来入口漂移。
        let lyrics_on = self.lyric_view.read(cx).has_lyrics() && !self.wide_lyrics_hidden;
        self.lyric_view.update(cx, |view, _| {
            view.set_text_visible(lyrics_on);
            view.set_click_seek_enabled(lyrics_on);
        });

        // ---- AMLL horizontal 几何 ----
        let cover = wide_cover_size(width, height);
        let gap = 8.0;
        // 剩余 fr 高度按 AMLL 的 6 行 grid 计算：thumb 2×2vh=36，
        // 5 道 8px gap。
        let fr = (height - cover - 76.0).max(0.0);
        let drag = fr * 0.12;
        let cover_top = wide_cover_top(width, height);
        // 底部控制行与布局 2 使用同一个底边距（16px）；音量行必须停在
        // 控制行上方，信息区用剩余高度做 space-between。
        let bottom_row_top = height - 16.0 - 40.0;
        let controls_top = cover_top + cover + COMPACT_COVER_GAP;
        let controls_h = (bottom_row_top - gap - controls_top).max(0.0);
        let info_w = width * 0.45;
        let lyric_left = info_w + gap;
        let lyric_col_w = (width - lyric_left).max(0.0);
        let lyric_pad_right = if height <= 1000.0 {
            lyric_col_w * 0.08
        } else {
            lyric_col_w * 0.15
        };

        // 布局 1 固定为「左播放列 + 右歌词」；歌词开关切换时会立即落到
        // 布局 2，因此这里不再维护居中分支。
        let split_x = wide_column_x(width, height);
        let wide_motion = self
            .stage_motion
            .filter(|motion| motion.active(now) && motion.touches_wide())
            .filter(|motion| motion.from.is_valid() && motion.to.is_valid());
        let stage = wide_motion.map(|motion| motion.geometry(now));
        let mut column_x = split_x;
        if let Some(geometry) = stage {
            column_x = geometry.cover.x;
        }
        let shared_hidden = stage.is_some() || dock_shared;

        let align = ((cover_top + cover * 0.5) / height).clamp(0.05, 0.95);
        if lyrics_on && (self.lyric_align - align).abs() > 0.0005 {
            self.lyric_align = align;
            self.lyric_view.update(cx, |view, _| {
                view.set_player_config(LyricPlayerConfig {
                    align_anchor: AlignAnchor::Center,
                    align_position: align,
                    enable_blur: !self.no_blur,
                    ..LyricPlayerConfig::default()
                });
            });
        }
        // —— 顶部窗口拖动带：整条空白头部都可拖动窗口（AMLL drag-area）。
        //    pill 只保留一个小的下拉关闭命中区，其余宽度全部用来移动窗口。
        // Dock→全屏的位移由外层卡片承担，页面内部不再二次偏移。
        let sheet_offset = 0.0;
        let pill_top = drag + gap + 16.0;
        let drag_band = (!self.eq_open).then(|| {
            Self::window_drag_area(
                div()
                    .id("immersive-drag-band")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(pill_top + 8.0))
                    .cursor(CursorStyle::OpenHand),
            )
        });

        // 下拉关闭把手：与 pill 等宽的窄条，occlude 掉下层的窗口拖动带。
        // EQ 覆盖层打开时不启用，避免顶部的 EQ 控件被手势抢走。
        let handle_w = 160.0_f32.min(width);
        let handle_top = (pill_top - 20.0).max(0.0);
        let dismiss_handle = (!self.eq_open).then(|| {
            div()
                .id("immersive-dismiss-handle")
                .absolute()
                .top(px(handle_top))
                .left(px(column_x + cover * 0.5 - handle_w / 2.0))
                .w(px(handle_w))
                .h(px(56.0))
                .cursor(CursorStyle::OpenHand)
                .occlude()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|app, event: &MouseDownEvent, _, cx| {
                        app.begin_sheet_drag(f32::from(event.position.y), cx);
                    }),
                )
                .child(self.amll_pill(
                    handle_w / 2.0 - 25.0,
                    pill_top - handle_top,
                    "immersive-pill",
                ))
        });

        // —— 共用 AMLL 播放器列：封面 / 信息 / 进度 / 传输 / 音量 ——
        let info_column = div()
            .absolute()
            .left(px(column_x))
            .top(px(cover_top))
            .w(px(cover))
            .child(self.amll_player_column(
                cover,
                height,
                controls_h,
                1.0,
                false,
                shared_hidden,
                window,
                cx,
            ));

        // —— 歌词开启时占右列；关闭/无歌词时整窗只保留 Mesh 背景 ——
        let lyric_area = if lyrics_on {
            div()
                .absolute()
                .left(px(lyric_left))
                .top_0()
                .bottom_0()
                .right_0()
                .pr(px(lyric_pad_right))
                .child(self.lyric_view.clone())
        } else {
            div().absolute().inset_0().child(self.lyric_view.clone())
        };

        // —— 统一的底部控制行：与布局 2 共用同一个构建函数 ——
        let bottom_row = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(bottom_row_top))
            .h(px(40.0))
            .px(px(32.0))
            .opacity(dock_opacity)
            .child(self.immersive_bottom_row(
                (width - 64.0).max(1.0),
                true,
                lyrics_on,
                cx,
            ));

        let stage_el = stage.map(|geometry| self.render_shared_stage(geometry));

        // 页面本体：下拉时整块下移，露出背后的主面板。拖动带在下、pill
        // 把手在上（occlude），因此只有 pill 附近是下拉手势，其余顶部拖窗口。
        let sheet = div()
            .id("immersive-sheet")
            .absolute()
            .left_0()
            .top(px(sheet_offset))
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(gpui::black())
            .child(lyric_area)
            .child(gpui::batch_layer(info_column))
            .when_some(stage_el, |sheet, stage| sheet.child(stage))
            .child(gpui::batch_layer(bottom_row))
            .when_some(drag_band, |sheet, band| sheet.child(band))
            .when_some(dismiss_handle, |sheet, handle| sheet.child(handle));

        div()
            .key_context("ChordialImmersive")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
                if app.quality_dialog_open {
                    app.quality_dialog_open = false;
                } else if app.drawer_open {
                    app.close_drawer(cx);
                } else if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.exit_immersive(cx);
                }
                window.focus(&app.focus_handle);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &TogglePortrait, _, cx| {
                app.exit_immersive(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &ToggleEqualizer, _, cx| {
                if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.open_eq(cx);
                }
            }))
            .on_action(cx.listener(|app, _: &PlayPause, _, cx| app.toggle_playback(cx)))
            .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
            .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
            .on_action(cx.listener(|app, _: &PlayFirst, _, cx| app.play_first(cx)))
            .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                let y = f32::from(event.position.y);
                match event.pressed_button {
                    Some(gpui::MouseButton::Left) => app.update_sheet_drag(y, cx),
                    None => {
                        if app.sheet_drag.is_some_and(|drag| !drag.animating()) {
                            app.release_sheet_drag(height, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |app, _: &MouseUpEvent, _, cx| {
                    if app.sheet_drag.is_some() {
                        app.release_sheet_drag(height, cx);
                    }
                }),
            )
            .size_full()
            .relative()
            .overflow_hidden()
            .child(sheet)
            .into_any_element()
    }
}

impl Render for ChordialApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        // 回收上一帧替换下来的封面/头像/二维码纹理，避免长会话/快速切歌
        // 把 sprite atlas（只增不减）撑爆后整块背景变黑。
        for image in std::mem::take(&mut self.retired_images) {
            cx.drop_image(image, Some(window));
        }
        self.cleanup_finished_animations(now, cx);
        // 跟随显示器刷新率驱动动画：300Hz 屏即 ~300fps，60Hz 屏即 60fps。
        // 动画全部结束后不再请求下一帧。
        if self.ui_anim_active() {
            window.request_animation_frame();
        }
        let viewport = window.viewport_size();
        self.viewport_width = f32::from(viewport.width);
        self.viewport_height = f32::from(viewport.height);
        // 断点迁移必须在任何页面渲染之前：它会收敛手机页/抽屉/歌词背景，
        // 避免布局切换的第一帧出现“桌面根节点 + 手机内容”的混合态。
        let class = ViewportClass::of(self.viewport_width);
        self.handle_viewport_class_change(class, cx);
        let base: AnyElement = if self.immersive {
            let width = f32::from(viewport.width);
            let height = f32::from(viewport.height);
            // 每帧重算沉浸样式；视图内部按完整样式去重，只有字号/字重/行高
            // 真正变化时才重排。不能只比较字号：窗口高度恰好让沉浸字号落在
            // 44px 附近时，面板样式（MEDIUM 44 / 行高 1.35）会被误认为已应用。
            let style = immersive_lyric_style(width, height, self.lyric_sweep_fade_em);
            self.lyric_view
                .update(cx, |view, _| view.set_text_style(style));
            let scope = PerfScope::enter("ui.render.pass");
            let immersive = self.render_immersive(window, cx);
            scope.finish(1);
            if self.eq_open {
                div()
                    .size_full()
                    .relative()
                    .child(immersive)
                    .child(gpui::batch_layer(self.render_eq(window, cx)))
                    .into_any_element()
            } else {
                // Dock→全屏展开或下拉收起时，卡片没有铺满窗口：把主面板画在
                // 背后，让露出的区域真的能看到列表与 Dock。手机宽度下背后
                // 必须是手机壳，否则会露出被压扁的三栏。
                let dock_p = self.immersive_dock_progress(now, height);
                if dock_p > 0.0001 {
                    let main = if self.is_phone() {
                        self.render_phone_main(window, cx, true)
                    } else {
                        self.render_main(window, cx, true)
                    };
                    // 沉浸层必须绝对定位覆盖在主面板之上；作为普通 flex 子元素
                    // 会被排到主面板后面/下方，拖拽时只剩主面板可见。
                    div()
                        .size_full()
                        .relative()
                        .child(main)
                        .child(div().absolute().inset_0().child(immersive))
                        .into_any_element()
                } else {
                    immersive
                }
            }
        } else {
            self.sync_phone_backdrop(cx);
            if self.is_phone() {
                self.render_phone_main(window, cx, false)
            } else {
                self.render_main(window, cx, false)
            }
        };

        // 音质解码信息弹窗盖在所有页面之上（宽屏/竖屏/手机播放页共用）。
        if self.quality_dialog_open {
            div()
                .size_full()
                .relative()
                .child(base)
                .child(self.render_quality_dialog(cx))
                .into_any_element()
        } else {
            base
        }
    }
}

impl ChordialApp {
    /// 主面板的根节点：三个布局（宽屏 / 中等 / 手机）共用同一套快捷键、
    /// 焦点上下文与边框。只负责“外壳”，页面内容由调用方追加。
    fn main_root(
        &mut self,
        background: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = Theme::get(cx).clone();
        let mut root = div();
        if !background {
            root = root
                .key_context("ChordialMain")
                .track_focus(&self.focus_handle);
        }
        root.on_action(cx.listener(|app, _: &PlayPause, _window, cx| {
            app.toggle_playback(cx);
        }))
        .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
        .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
        .on_action(cx.listener(|app, _: &PlayFirst, _, cx| app.play_first(cx)))
        .on_action(cx.listener(|app, _: &FocusSearch, window, cx| {
            if app.is_phone() && !app.immersive {
                // 手机壳的搜索框在曲库页。切页与聚焦必须分两拍：本帧
                // 输入框还没挂载，focus 会被下一帧的布局丢掉。
                app.phone_page = PhonePage::Browse;
                app.view_tab = ViewTab::Search;
                let input = app.search_input.clone();
                cx.defer_in(window, move |_, window, cx| {
                    focus_input(&input, window, cx);
                });
                cx.notify();
            } else {
                focus_input(&app.search_input, window, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &RescanLibrary, _, cx| {
            if let Some(local) = app.local_source.clone() {
                app.spawn_library_scan(local, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &ToggleSidebar, _, cx| {
            // 手机壳没有侧栏；Ctrl+B 改成开合手机抽屉（同样的“导航”语义）。
            match (app.is_phone() && !app.immersive, app.drawer_open) {
                (true, true) => app.close_drawer(cx),
                (true, false) => app.open_drawer(cx),
                (false, _) => {
                    app.sidebar_open = !app.sidebar_open;
                    cx.notify();
                }
            }
        }))
        .on_action(cx.listener(|app, _: &OpenSettings, _, cx| {
            app.open_settings(cx);
        }))
        .on_action(cx.listener(|app, _: &CloseSettings, _, cx| {
            // Esc 在手机壳里兼作「关抽屉 / 返回播放」，与系统返回键同义。
            if app.quality_dialog_open {
                app.quality_dialog_open = false;
            } else if app.settings_open {
                app.close_settings(cx);
            } else if app.drawer_open {
                app.close_drawer(cx);
            } else if app.is_phone()
                && matches!(app.phone_page, PhonePage::Browse | PhonePage::Lyrics)
                && app.current_track.is_some()
            {
                app.phone_page = PhonePage::Player;
                cx.notify();
            }
        }))
        .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
            if app.eq_open {
                app.close_eq(cx);
            } else if app.immersive {
                app.close_portrait(cx);
            }
            window.focus(&app.focus_handle);
            cx.notify();
        }))
        .on_action(cx.listener(|app, _: &TogglePortrait, window, cx| {
            if app.immersive {
                app.close_portrait(cx);
            } else {
                app.open_portrait(cx);
                window.focus(&app.focus_handle);
            }
            cx.notify();
        }))
        .on_action(cx.listener(|app, _: &ToggleEqualizer, window, cx| {
            if app.eq_open {
                app.close_eq(cx);
            } else {
                app.open_eq(cx);
                window.focus(&app.focus_handle);
            }
            cx.notify();
        }))
        .on_action(cx.listener(|app, _: &Quit, _, cx| {
            app.cache.flush();
            // 先停掉托管 sidecar（不清理缓存、不注销账号），再退出。
            let registry = app.registry.clone();
            cx.spawn(async move |weak, cx| {
                for info in registry.loaded_sources() {
                    if let Some(source) = registry.get(&info.id) {
                        source.stop_services().await;
                    }
                }
                let _ = weak.update(cx, |_, cx| cx.quit());
            })
            .detach();
        }))
        .size_full()
        .relative()
        .flex()
        .flex_col()
        .bg(theme.bg)
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text)
    }

    /// 主面板（非沉浸层）。沉浸页下拉关闭时也会画在 sheet 背后，
    /// 让页面下移时露出熟悉的列表与播放条。
    ///
    /// `background = true` 时只作为静态背景：不注册键位上下文与焦点节点，
    /// 避免与同时渲染的沉浸层抢同一个 `focus_handle`。
    fn render_main(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        background: bool,
    ) -> AnyElement {
        if !self.activated {
            self.activated = true;
            window.activate_window();
        }
        let scope = PerfScope::enter("ui.render.pass");
        let show_now = self.viewport_width >= crate::responsive::WIDE_MIN_W;
        let list_width = show_now.then(|| self.list_width());
        // 均衡器面板几乎铺满窗口：列表/正在播放完全落在面板背后，跳过
        // 它们的布局与绘制，EQ 列表滚动/预览时不再为不可见内容付费。
        // 侧栏左侧一窄条仍可见，保留。
        let covered_by_eq = self.eq_open
            && !self.settings_open
            && !self.login_open
            && !self.immersive
            && !background;

        let middle = div()
            .flex()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .when(self.sidebar_open, |d| {
                d.child(gpui::batch_layer(
                    self.render_sidebar(window.mouse_position(), cx),
                ))
            })
            .when(!covered_by_eq, |d| {
                d.child(gpui::batch_layer(self.render_list_panel(list_width, cx)))
                    .when(show_now, |d| {
                        d.child(gpui::batch_layer(self.render_now_playing(cx)))
                    })
            });

        let view = self
            .main_root(background, cx)
            .child(self.render_titlebar(cx))
            .child(middle)
            .child(gpui::batch_layer(self.render_dock(cx)))
            .when(self.settings_open, |d| d.child(self.render_settings(cx)))
            .when(self.login_open, |d| d.child(self.render_login(cx)))
            .when(self.eq_open, |d| {
                d.child(gpui::batch_layer(self.render_eq(window, cx)))
            })
            .children(Self::resize_handles());
        scope.finish(1);
        view.into_any_element()
    }

    // ================= 手机壳（单列） =================

    /// 手机壳：没有侧栏/右栏/Dock，播放页 / 歌词页 / 曲库页三选一。
    /// 抽屉是唯一的“全局面板”，承载曲库、队列、设置与均衡器入口。
    fn render_phone_main(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        background: bool,
    ) -> AnyElement {
        if !self.activated && !background {
            self.activated = true;
            window.activate_window();
        }
        let scope = PerfScope::enter("ui.render.pass");
        let now = Instant::now();
        self.sync_phone_backdrop(cx);
        let width = self.viewport_width;
        let height = self.viewport_height;
        let body: AnyElement = match self.phone_page {
            PhonePage::Player if self.current_track.is_some() => {
                self.render_phone_player(window, cx)
            }
            PhonePage::Lyrics if self.current_track.is_some() => {
                self.render_phone_lyrics(cx)
            }
            _ => self.render_phone_browse(cx),
        };

        let view = self
            .main_root(background, cx)
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .when(!background && self.drawer_visible(), |d| {
                d.child(self.render_drawer(width, height, now, cx))
            })
            .when(!background && self.settings_open, |d| {
                d.child(self.render_settings(cx))
            })
            .when(!background && self.login_open, |d| {
                d.child(self.render_login(cx))
            })
            .when(!background && self.eq_open, |d| {
                d.child(gpui::batch_layer(self.render_eq(window, cx)))
            })
            .children(Self::resize_handles());
        scope.finish(1);
        view.into_any_element()
    }

    /// 手机播放页：沿用竖屏沉浸层的 AMLL 几何（同一列宽、同一控件顺序），
    /// 只是把右上角的菜单入口换成手机壳抽屉，并保留窗口红绿灯。
    fn render_phone_player(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.sync_progress_clock();
        let width = self.viewport_width;
        let height = self.viewport_height;
        let cover = compact_cover_size(width, height);
        let left = ((width - cover) * 0.5).max(0.0);
        let available = (height
            - COMPACT_TOP
            - crate::layout_morph::COMPACT_BOTTOM
            - crate::layout_morph::COMPACT_DOCK_H
            - 8.0)
            .max(0.0);
        let controls_h = (available - cover - COMPACT_COVER_GAP).max(0.0);

        // 不在手机播放/歌词态时，绝不能再把同一个 lyric_view 塞进第二
        // 棵树里。用静态渐变兜底，等沉浸层交还后再切回 Mesh。
        let backdrop: AnyElement = if !self.no_bg && self.phone_backdrop != PhoneBackdrop::Off {
            div()
                .absolute()
                .inset_0()
                .bg(gpui::black())
                .child(self.lyric_view.clone())
                .into_any_element()
        } else {
            div()
                .absolute()
                .inset_0()
                .bg(linear_gradient(
                    165.0,
                    linear_color_stop(crate::theme::hex(0x232329), 0.0),
                    linear_color_stop(crate::theme::hex(0x08080A), 1.0),
                ))
                .into_any_element()
        };

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .child(backdrop)
            .child(self.render_phone_titlebar(true, false, cx))
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(COMPACT_TOP))
                    .w(px(cover))
                    .child(self.amll_player_column(
                        cover,
                        height,
                        controls_h,
                        1.0,
                        true,
                        false,
                        window,
                        cx,
                    )),
            )
            // 封面是手机播放页最自然的“打开歌词”入口。
            .child(
                div()
                    .id("phone-cover-lyrics")
                    .absolute()
                    .left(px(left))
                    .top(px(COMPACT_TOP))
                    .w(px(cover))
                    .h(px(cover))
                    .cursor_pointer()
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.phone_page = PhonePage::Lyrics;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .bottom(px(crate::layout_morph::COMPACT_BOTTOM))
                    .w(px(cover))
                    .child(self.phone_bottom_row(cover, cx)),
            )
            .into_any_element()
    }

    /// 手机歌词页：同一个 Mesh 背景 + 可滚动/可点击 seek 的歌词。
    /// 标题栏叠加在顶部，返回键回到播放页。
    fn render_phone_lyrics(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let width = self.viewport_width;
        let height = self.viewport_height;
        let sweep = self.lyric_sweep_fade_em;
        self.lyric_view.update(cx, |view, _| {
            view.set_text_style(phone_lyric_style(&theme, width, height, sweep));
        });
        let has_lyrics = self.lyric_view.read(cx).has_lyrics();
        let empty = (!has_lyrics).then(|| {
            let message = self
                .lyrics_status
                .clone()
                .unwrap_or_else(|| "暂无歌词".into());
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .px(px(40.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .line_height(px(20.0))
                        .text_center()
                        .text_color(gpui::white().opacity(0.60))
                        .child(SharedString::from(message)),
                )
        });

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(gpui::black())
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(self.lyric_view.clone()),
            )
            .child(self.render_phone_titlebar(true, true, cx))
            .children(empty)
            .into_any_element()
    }

    /// 手机曲库页：桌面曲目板直接铺满，外层只负责标题栏与返回播放。
    fn render_phone_browse(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.surface)
            .child(self.render_phone_titlebar(false, false, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_list_panel(None, cx)),
            )
            .into_any_element()
    }

    /// 手机标题栏：播放页/歌词页为透明覆盖层（顶部轻压暗），曲库页为
    /// 实心 sidebar 色。红绿灯与拖拽区保留，否则无边框窗口会失去
    /// 关闭/移动能力；歌词页额外提供返回播放。
    fn render_phone_titlebar(
        &self,
        overlay: bool,
        back: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let fg = if overlay {
            gpui::white().opacity(0.78)
        } else {
            theme.text_dim
        };
        let menu = {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Menu, 16.0, fg),
                30.0,
                "菜单",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        if app.drawer_open {
                            app.close_drawer(cx);
                        } else {
                            app.open_drawer(cx);
                        }
                    });
                },
            )
        };
        let back_btn = back.then(|| {
            let weak = weak.clone();
            crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::ChevronLeft, 16.0, fg),
                30.0,
                "返回播放",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Player;
                        cx.notify();
                    });
                },
            )
        });
        let center = {
            let pill = (overlay && !back).then(|| {
                div()
                    .w(px(50.0))
                    .h(px(8.0))
                    .rounded(px(4.0))
                    .bg(gpui::white().opacity(0.176))
                    .into_any_element()
            });
            let title = back.then(|| {
                div()
                    .max_w(px(210.0))
                    .text_size(px(12.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(gpui::white().opacity(0.88))
                    .text_ellipsis()
                    .child(SharedString::from(self.current_title_text()))
                    .into_any_element()
            });
            let brand = (!overlay).then(|| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Note,
                        12.0,
                        theme.accent,
                    ))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text_dim)
                            .child("Chordial"),
                    )
                    .into_any_element()
            });
            Self::window_drag_area(
                div()
                    .id(if overlay {
                        "phone-titlebar-drag-overlay"
                    } else {
                        "phone-titlebar-drag"
                    })
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor(CursorStyle::OpenHand),
            )
            .children(pill)
            .children(title)
            .children(brand)
        };

        let bar = div()
            .h(px(40.0))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(14.0))
            .when(overlay, |d| {
                d.absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(gpui::black().opacity(0.30), 0.0),
                        linear_color_stop(gpui::black().opacity(0.0), 1.0),
                    ))
            })
            .when(!overlay, |d| {
                d.bg(theme.sidebar)
                    .border_b_1()
                    .border_color(theme.border)
            })
            .child(Self::traffic_lights())
            .children(back_btn)
            .child(center)
            .child(menu);
        bar.into_any_element()
    }

    /// 手机播放页底部：全屏 / 曲库 / 队列 / 歌词。
    /// 抽屉仍由标题栏菜单与曲目信息「…」进入。
    fn phone_bottom_row(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let btn = |icon: crate::icons::Icon,
                   id: &'static str,
                   active: bool,
                   on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>| {
            div()
                .id(id)
                .size(px(40.0))
                .rounded(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .opacity(if active { 1.0 } else { 0.45 })
                .hover(|s| s.opacity(1.0).bg(white.opacity(0.133)))
                .child(crate::icons::icon(icon, 18.0, white))
                .on_click(on_click)
                .into_any_element()
        };

        let fullscreen = btn(
            crate::icons::Icon::Expand,
            "phone-fullscreen",
            false,
            Box::new(|_, window, _| window.toggle_fullscreen()),
        );
        let library = {
            let weak = weak.clone();
            let active = self.phone_page == PhonePage::Browse
                && self.view_tab == ViewTab::Library
                && self.library_filter == LibraryFilter::All;
            btn(
                crate::icons::Icon::Note,
                "phone-library",
                active,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Browse;
                        app.view_tab = ViewTab::Library;
                        app.library_filter = LibraryFilter::All;
                        cx.notify();
                    });
                }),
            )
        };
        let queue = {
            let weak = weak.clone();
            let active = self.phone_page == PhonePage::Browse
                && self.view_tab == ViewTab::Library
                && self.library_filter == LibraryFilter::Queue;
            btn(
                crate::icons::Icon::Queue,
                "phone-queue",
                active,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Browse;
                        app.view_tab = ViewTab::Library;
                        app.library_filter = LibraryFilter::Queue;
                        cx.notify();
                    });
                }),
            )
        };
        let lyrics = {
            let weak = weak.clone();
            btn(
                crate::icons::Icon::Lyrics,
                "phone-lyrics",
                false,
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.phone_page = PhonePage::Lyrics;
                        cx.notify();
                    });
                }),
            )
        };

        div()
            .w(px(width))
            .h(px(40.0))
            .flex()
            .items_center()
            .justify_between()
            .occlude()
            .child(fullscreen)
            .child(
                div()
                    .flex()
                    .items_center()
                    // 最小窗口时 cover 列只有 ~184px，4 个 40px 按钮需要
                    // 收紧间距，否则会顶出封面列。
                    .gap(px(if width < 240.0 { 8.0 } else { 14.0 }))
                    .child(library)
                    .child(queue)
                    .child(lyrics),
            )
            .into_any_element()
    }

    /// 无边框窗口的三颗红绿灯。桌面标题栏与手机标题栏共用。
    fn traffic_lights() -> AnyElement {
        let light = |id: &'static str, color: gpui::Hsla| {
            div()
                .id(id)
                .size(px(12.0))
                .rounded(px(6.0))
                .bg(color)
                .cursor_pointer()
        };
        let close = light("tl-close", crate::theme::hex(0xFF5F57))
            .on_click(|_, _, cx| cx.quit());
        let minimize = light("tl-min", crate::theme::hex(0xFEBC2E))
            .on_click(|_, window, _| window.minimize_window());
        let maximize = light("tl-max", crate::theme::hex(0x28C840))
            .on_click(|_, window, _| window.zoom_window());
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(close)
            .child(minimize)
            .child(maximize)
            .into_any_element()
    }
}

impl Drop for ChordialApp {
    fn drop(&mut self) {
        // 进程退出/窗口销毁时把 LRU 访问时间与新增对象索引落盘。
        self.cache.flush();
    }
}


fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.2} GB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MB", value / MIB)
    } else if value >= KIB {
        format!("{:.0} KB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// 解析频段输入：`4000`、`4k`、`10 kHz`、`10,000` 都接受。
fn parse_hz_input(raw: &str) -> Option<f32> {
    let text: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',' && *c != '_')
        .collect::<String>()
        .to_ascii_lowercase();
    if text.is_empty() {
        return None;
    }
    let (number, scale) = if let Some(v) = text.strip_suffix("khz") {
        (v, 1_000.0)
    } else if let Some(v) = text.strip_suffix('k') {
        (v, 1_000.0)
    } else if let Some(v) = text.strip_suffix("hz") {
        (v, 1.0)
    } else {
        (text.as_str(), 1.0)
    };
    number
        .parse::<f32>()
        .ok()
        .map(|v| v * scale)
        .filter(|v| v.is_finite() && *v > 0.0)
}

#[cfg(test)]
mod tests {
    use super::{LibraryFilter, PhonePage, ViewTab, parse_hz_input, phone_default_page};

    #[test]
    fn parses_common_hz_inputs() {
        assert_eq!(parse_hz_input("4000"), Some(4_000.0));
        assert_eq!(parse_hz_input(" 10 kHz "), Some(10_000.0));
        assert_eq!(parse_hz_input("4k"), Some(4_000.0));
        assert_eq!(parse_hz_input("22,050"), Some(22_050.0));
        assert_eq!(parse_hz_input("10000hz"), Some(10_000.0));
        assert_eq!(parse_hz_input(""), None);
        assert_eq!(parse_hz_input("abc"), None);
    }

    #[test]
    fn phone_entry_preserves_browsing_context() {
        // 普通曲库 + 正在播放：落到播放页。
        assert_eq!(
            phone_default_page(ViewTab::Library, LibraryFilter::All, true),
            PhonePage::Player
        );
        // 没有曲目：曲库页，避免空白封面。
        assert_eq!(
            phone_default_page(ViewTab::Library, LibraryFilter::All, false),
            PhonePage::Browse
        );
        // 搜索 / 集合 / 队列是主动浏览行为，不因收窄而被打断。
        assert_eq!(
            phone_default_page(ViewTab::Search, LibraryFilter::All, true),
            PhonePage::Browse
        );
        assert_eq!(
            phone_default_page(ViewTab::Collection, LibraryFilter::All, true),
            PhonePage::Browse
        );
        assert_eq!(
            phone_default_page(ViewTab::Library, LibraryFilter::Queue, true),
            PhonePage::Browse
        );
    }
}
