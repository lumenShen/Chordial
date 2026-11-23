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
use chordal_core::mem::{self, Gauge};
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
    StageRect, compact_content_w, compact_cover_size, dock_geometry, dock_rect, immersive_geometry,
    wide_column_x, wide_cover_size, wide_cover_top,
};
use crate::lyrics::{cover_url, decode_data_url_image, fetch_cover_blocking, to_amll_lyrics};
use crate::main_state::Globals;
use crate::phone_gesture::{EdgeSwipe, SheetPage, SheetSwipe, SwipeOutcome};
use crate::player_page::{PortraitPage, Transition};
use crate::responsive::{MIN_WIDE_SPLIT_W, ViewportClass};
use crate::settings::{SettingsAction, SettingsRuntime, SettingsView};
use crate::sheet_drag::{SheetDrag, SheetDragOutcome, SpringSettle};
use crate::theme::{Appearance, Theme, fmt_ms};
use types::*;

// ---- 按职责拆分出的子模块（纯代码搬移） ----
mod lifecycle;
mod types;
mod navigation;
mod overlays;
mod eq_actions;
mod animations;
mod playback;
mod account;
mod settings_ops;
mod media;
mod render_sidebar;
mod render_track_list;
mod render_collection;
mod render_list_panel;
mod render_now_playing;
mod render_settings_login;
mod render_portrait_queue;
mod render_dock;
mod render_quality;
mod render_immersive;
mod render_drawer;
mod render_stage;
mod render_phone;
mod render_menus;

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
    /// 画面性能诊断浮窗开关（与配置同步）。
    perf_overlay_on: bool,
    /// 性能浮窗句柄；关闭后置空，避免重复开窗。
    perf_window: Option<gpui::AnyWindowHandle>,
    settings_open: bool,
    /// 设置页打开代数：每次打开递增，内存诊断刷新循环据此退出。
    settings_gen: u64,
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
    /// 手机壳 mini player ⇄ 播放页的纵向跟手手势；None = 静止。
    phone_sheet: Option<SheetSwipe>,
    /// 点击（非拖拽）展开/收起播放页时的弹簧；None = 静止。
    phone_sheet_spring: Option<SpringSettle>,
    /// mini player 上按住但还没越过死区时的起点 y；渲染层据此铺一层
    /// 全窗捕获层，保证指针移出小条后手势仍然连续。
    phone_sheet_anchor: Option<f32>,
    /// 封面 ⇄ 歌词的横滑；None = 静止。
    phone_edge_swipe: Option<EdgeSwipe>,
    /// 横滑刚提交后短暂吞掉 click，避免松手又触发一次「打开歌词」。
    phone_swipe_suppress_click: Option<Instant>,
    /// 曲目行右键 / 「⋮」菜单。
    track_menu: Option<TrackMenuState>,
    /// 多选模式；None = 普通浏览。
    selection: Option<SelectionState>,
    /// 长按起手（列表, 行号, 代号）。480ms 内没有抬起就进入多选。
    long_press: Option<(QueueList, usize, Instant)>,
    long_press_pos: Option<(f32, f32)>,
    long_press_gen: u64,
    /// 长按刚触发多选时吞掉随后的 click，避免又把勾选取消。
    row_click_suppress_until: Option<Instant>,
    /// 队列拖拽排序。
    queue_drag: Option<QueueDragState>,
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
    /// 按音源聚合的搜索结果计数：原实现每个音源都全量扫一遍 search_tracks。
    search_source_counts: RefCell<Option<(usize, HashMap<SourceId, usize>)>>,
    /// 集合搜索的按源过滤缓存（与 `search_route_cache` 同理）。
    collection_route_cache: RefCell<
        Option<(
            Arc<Vec<CollectionSummary>>,
            Option<SourceId>,
            Arc<Vec<CollectionSummary>>,
        )>,
    >,
    /// 曲目行显示文本缓存：列表快照不变时 render 期间零字符串分配。
    row_text_cache: RefCell<Option<(Arc<Vec<Track>>, Arc<Vec<TrackRowText>>)>>,
    /// 列表行的小尺寸封面：由可见行触发懒加载，后台下载/解码后回写。
    /// 用 `Mutex` 是为了让虚拟列表 prepaint 回调和后台任务都能安全读写。
    track_covers: Arc<std::sync::Mutex<TrackCovers>>,
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
fn phone_lyric_style(theme: &Theme, width: f32, height: f32, sweep_fade_em: f32) -> LyricTextStyle {
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


    // ---------- 参数均衡器 ----------


    const UI_ANIM_PRESS_MS: f32 = 700.0;
    const UI_ANIM_ARROWS_MS: f32 = 600.0;
    const UI_ANIM_SPEAKER_MS: f32 = 750.0;
    /// Dock→全屏展开弹簧：轻微过冲后收敛。
    const EXPAND_DAMPING: f32 = 21.0;
    const EXPAND_STIFFNESS: f32 = 200.0;



    // ---------- 播放 ----------



    // ---------- 设置 / 下载 ----------



    // ---------- 渲染 ----------

    // ================= 布局断点 =================


    // ================= 面板标题 =================


    // ================= 中间：曲目板 =================



    // ================= 右侧：正在播放 =================


    // ================= 设置面板 =================


    // ================= 底部悬浮 Dock =================






}

impl Render for ChordialApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 性能浮窗的帧率采样点：未开启时只读一个原子布尔。
        crate::perf_overlay::note_frame();
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
    use super::{
        ChordialApp, LibraryFilter, PhonePage, TrackRowText, ViewTab, parse_hz_input,
        phone_default_page, track_cover_key,
    };
    use chordal_core::model::{AlbumRef, SourceId, Track, TrackId, TrackOrigin};

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

    fn track_with_cover(cover: Option<&str>, source: &str) -> Track {
        Track::new(
            TrackId::new(SourceId::new(source), "42"),
            "Song",
            Vec::new(),
            Some(AlbumRef {
                id: None,
                name: "Album".into(),
                cover: cover.map(str::to_string),
            }),
            None,
            TrackOrigin::Remote {
                source: SourceId::new(source),
            },
        )
    }

    #[test]
    fn track_rows_keep_a_cover_key_for_lazy_loading() {
        let track = track_with_cover(Some("https://p1.music.126.net/a.jpg"), "netease");
        let key = "netease|https://p1.music.126.net/a.jpg";
        assert_eq!(track_cover_key(&track).as_deref(), Some(key));
        assert_eq!(TrackRowText::new(0, &track).cover_key.as_deref(), Some(key));

        // 同一 URL 在不同音源下不能共用缓存键（对象缓存按 namespace 隔离）。
        let local = track_with_cover(Some("https://p1.music.126.net/a.jpg"), "local");
        assert_ne!(track_cover_key(&local), track_cover_key(&track));

        let no_cover = track_with_cover(None, "netease");
        assert_eq!(TrackRowText::new(0, &no_cover).cover_key, None);
    }

    #[test]
    fn list_thumbnails_are_downscaled_before_upload() {
        let rgba = vec![255u8; 512 * 256 * 4];
        let image = ChordialApp::rgba_to_thumb_image(512, 256, &rgba);
        let size = image.size(0);
        assert_eq!(size.width.0, 72);
        assert_eq!(size.height.0, 36);

        let small = ChordialApp::rgba_to_thumb_image(32, 24, &vec![0u8; 32 * 24 * 4]);
        let size = small.size(0);
        assert_eq!((size.width.0, size.height.0), (32, 24));
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
