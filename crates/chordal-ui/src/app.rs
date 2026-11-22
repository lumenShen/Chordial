//! 根视图：源面板 / 搜索 / 本地库 / 播放条。
//!
//! 渲染性能纪律：
//! - 列表一律 [`uniform_list`] 虚拟化（只渲染可视行）；
//! - 结果集以 `Arc<Vec<Track>>` 快照持有，render 期间零深拷贝；
//! - 行回调通过 `WeakEntity` 捕获，行元素按 index 建 id；
//! - 播放状态 200ms 轮询 + 变化才 `cx.notify()`；
//! - 渲染 pass 带 `ui.render.pass` 检查点（预算 16ms，超预算告警）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chordal_cache::ObjectCache;
use chordal_core::model::{
    AccountState, CollectionId, CollectionKind, CollectionSummary, LoginChallenge, LoginMethod,
    LoginProgress, SearchKind, SearchQuery, SourceId, StreamHandle, Track, TrackOrigin,
};
use chordal_core::perf::PerfScope;
use chordal_core::registry::{SourceRegistry, SourceStatus};
use chordal_core::source::MusicSource;
use chordal_player::{Event as PlayerEvent, Player, PlayerState};
use chordial_amll_gpui::background::BackgroundRendererKind;
use chordial_amll_gpui::render::{LyricPlayerView, LyricRenderColors, PlaybackClock};
use chordial_amll_gpui::{AlignAnchor, LyricPlayerConfig, LyricTextStyle};
use gpui::StatefulInteractiveElement;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, BoxShadow, Context, CursorStyle, Entity, FocusHandle, RenderImage,
    ResizeEdge, SharedString, Window, div, linear_color_stop, linear_gradient, point, px, size,
    uniform_list,
};

use crate::actions::{
    ExitImmersive, FocusSearch, NextTrack, PlayPause, PrevTrack, Quit, RescanLibrary, ToggleSidebar,
};
use crate::config::SourcePreference;
use crate::downloads::DownloadStore;
use crate::input::{InputState, InputView, focus_input};
use crate::lyrics::{cover_url, decode_data_url_image, fetch_cover_blocking, to_amll_lyrics};
use crate::main_state::Globals;
use crate::theme::{Appearance, Theme, fmt_ms};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayMode {
    Sequential,
    Shuffle,
    RepeatOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginTab {
    Qr,
    Phone,
}

/// 右栏歌词预览的一行（已压平成纯文本 + 译文）。
#[derive(Clone)]
struct PreviewLyric {
    start_ms: u64,
    text: String,
    translation: String,
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
    settings_open: bool,
    settings_cache_input: Entity<InputState>,
    settings_cache_view: Entity<InputView>,
    settings_download_input: Entity<InputState>,
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

    queue: Arc<Vec<Track>>,
    queue_pos: Option<usize>,
    player_status: chordal_player::Status,
    status_message: Option<String>,
    /// 非错误提示（下载完成、音质选择说明等）。
    info_message: Option<String>,
    focus_handle: FocusHandle,

    // ---- 歌词页 ----
    lyric_view: Entity<LyricPlayerView>,
    current_track: Option<Track>,
    lyrics_gen: u64,
    lyrics_status: Option<String>,
    /// 播放请求代号：网络流可能乱序返回，旧请求不得覆盖新曲目。
    play_gen: u64,
    appearance: Appearance,
    cover_image: Option<Arc<RenderImage>>,
    /// 当前曲目歌词压平后的预览行（右栏用，异步加载完成后整体替换）。
    lyric_preview: Arc<Vec<PreviewLyric>>,
    activated: bool,
    /// 当前客户区尺寸；用于列表/播放条/封面断点。
    viewport_width: f32,
    viewport_height: f32,
    /// 沉浸歌词字号（随窗口高度缩放）。
    lyric_font_px: f32,
    /// 沉浸层是否处于竖向布局（宽 < 高）；决定歌词 mask 的底部渐隐。
    lyric_vertical: bool,
    /// 宽屏布局下缓存的歌词对齐比例（封面中心占比）。
    lyric_align: f32,
    /// 诊断开关：禁用歌词模糊（CHORDIAL_NO_BLUR=1，性能隔离用）。
    no_blur: bool,
    /// 诊断开关：禁用沉浸背景（CHORDIAL_NO_BG=1，性能隔离用）。
    no_bg: bool,
    immersive: bool,

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
    /// 60fps 控件动画泵是否在运行（只在有动画时唤醒，结束后自动退出）。
    ui_anim_ticker: bool,
    /// 上一次音量输入值（用于检测跨越 0/1 边界）。
    last_volume_input: f32,
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
fn compact_lyric_style(theme: &Theme) -> LyricTextStyle {
    LyricTextStyle {
        font_family: theme.font_family.clone(),
        font_weight: gpui::FontWeight::MEDIUM,
        base_font_size: 44.0,
        line_height_ratio: 1.35,
        sub_font_size_ratio: 0.54,
        sub_line_height_ratio: 1.45,
        padding_x_em: 1.1,
        padding_y_ratio: 0.03,
        ..Default::default()
    }
}

/// AMLL 等价字号：Medium 档 `max(max(5vh, 2.5vw), 14px)`；宽 ≤768 用
/// `max(8vw, 12px)`。是否竖向由宽高比决定（影响底部 mask 渐隐）。
fn immersive_lyric_style(width: f32, height: f32) -> (LyricTextStyle, f32, bool) {
    let font = if width <= 768.0 {
        (width * 0.08).max(12.0)
    } else {
        (height * 0.05).max(width * 0.025).max(14.0)
    };
    let vertical = width < height;
    (
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
            ..Default::default()
        },
        font,
        vertical,
    )
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
            view.set_text_style(compact_lyric_style(&theme));
            view.set_player_config(LyricPlayerConfig {
                align_position: 0.38,
                base_font_size: 44.0,
                non_dynamic_opacity: 0.60,
                ..Default::default()
            });
            view.set_background_fps(24);
            view.set_background_render_scale(0.35);
            view.set_background_flow_speed(2.0);
            // 浅色主题用纸面纯色，避免深色 Mesh 与深色文字对比不足。
            if !theme.is_dark() {
                view.set_background_kind(BackgroundRendererKind::Css);
            }
            view.set_background_color(theme.bg);
            view
        });

        let mut app = Self {
            registry,
            player,
            local_source: local_source.clone(),
            search_timeout_ms,
            cache,
            downloads: Arc::new(DownloadStore::open(cfg.download_dir.clone())),
            source_preference: cfg.source_preference,
            settings_open: false,
            settings_cache_input,
            settings_cache_view,
            settings_download_input,
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
            lyric_preview: Arc::new(Vec::new()),
            activated: false,
            viewport_width: 1280.0,
            viewport_height: 800.0,
            lyric_font_px: 44.0,
            lyric_vertical: false,
            lyric_align: 0.35,
            no_blur: std::env::var_os("CHORDIAL_NO_BLUR").is_some(),
            no_bg: std::env::var_os("CHORDIAL_NO_BG").is_some(),
            immersive: false,
            ui_anim_enter: None,
            ui_anim_play: None,
            ui_anim_shuffle: None,
            ui_anim_repeat: None,
            ui_anim_volume: None,
            ui_anim_ticker: false,
            last_volume_input: 0.9,
        };

        if let Some(local) = local_source {
            app.spawn_library_scan(local, cx);
        }
        app.spawn_account_bootstrap(cx);
        app.spawn_status_poll(cx);
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
                            app.player_status = status;
                            // AMLL 的播放/暂停 SVG 在状态切换时重新挂载，因此
                            // pressed 回弹与图标交换必须同帧发生。
                            if app.immersive && was_playing != is_playing {
                                app.ui_anim_play = Some(Instant::now());
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

    // ---------- 搜索 ----------

    pub fn start_search(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.search_gen += 1;
        self.searching = true;
        self.view_tab = ViewTab::Search;
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
            cx.notify();
        }
    }

    /// 无边框窗口的拖动热区。
    ///
    /// 两端的实现机制不同，不能共用 `start_window_move`：
    /// - Linux（Wayland/X11）由 gpui 向 compositor 发 move 请求；
    /// - Windows 上 gpui 的 `start_window_move` 是空实现。必须把热区
    ///   标记为系统标题栏区域，让 WM_NCHITTEST 返回 HTCAPTION，由
    ///   DefWindowProc 接管拖动（顺带获得贴边分屏、双击最大化等原生行为）。
    #[cfg(target_os = "windows")]
    fn window_drag_area<E: gpui::InteractiveElement>(element: E) -> E {
        element.window_control_area(gpui::WindowControlArea::Drag)
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
            el.on_mouse_down(gpui::MouseButton::Left, move |_, window, _| {
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
        // AMLL：页面打开时所有 MediaButton 的 SVG 内容重新挂载 → pressed 回弹；
        // 上一首/下一首的双箭头同时播放一次合并入场动画。
        self.ui_anim_enter = Some(Instant::now());
        self.ui_anim_play = None;
        self.ui_anim_shuffle = None;
        self.ui_anim_repeat = None;
        self.ui_anim_volume = None;
        self.last_volume_input = self.player_status.volume;
        self.ensure_ui_anim_ticker(cx);
        let white = gpui::Hsla {
            h: 0.0,
            s: 0.0,
            l: 1.0,
            a: 1.0,
        };
        self.lyric_view.update(cx, |view, _| {
            // 沉浸层背景 = AMLL 的 Mesh Gradient（动画），铺满整个窗口：
            // 光栅按低分辨率计算，由 GPU 双线性放大到屏幕。
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

    // ---------- 沉浸层控件动画 ----------

    const UI_ANIM_PRESS_MS: f32 = 700.0;
    const UI_ANIM_ARROWS_MS: f32 = 600.0;
    const UI_ANIM_SPEAKER_MS: f32 = 750.0;

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

    fn ui_anim_active(&self) -> bool {
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
    }

    /// 有控件动画时以 60fps 唤醒重绘；动画全部结束后泵自行退出。
    fn ensure_ui_anim_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ui_anim_ticker {
            return;
        }
        self.ui_anim_ticker = true;
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let alive = weak
                    .update(cx, |app, cx| {
                        if !app.immersive || !app.ui_anim_active() {
                            app.ui_anim_ticker = false;
                            return false;
                        }
                        cx.notify();
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

    fn exit_immersive(&mut self, cx: &mut Context<Self>) {
        self.immersive = false;
        let theme = Theme::get(cx).clone();
        self.lyric_view.update(cx, |view, _| {
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
            view.set_text_style(compact_lyric_style(&theme));
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
        {
            let theme = Theme::get(cx).clone();
            // 沉浸层的字号/字重与面板不同；主题切换时必须按当前模式重放，
            // 否则沉浸层会被换成面板样式（字体高度/行高看起来会突然变化）。
            let style = if self.immersive {
                let (style, font, vertical) =
                    immersive_lyric_style(self.viewport_width, self.viewport_height);
                self.lyric_font_px = font;
                self.lyric_vertical = vertical;
                style
            } else {
                compact_lyric_style(&theme)
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
            PlayMode::Sequential => self.play_next(cx),
        }
    }

    pub fn play_next(&mut self, cx: &mut Context<Self>) {
        let Some(pos) = self.queue_pos else { return };
        if pos + 1 < self.queue.len() {
            self.queue_pos = Some(pos + 1);
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
        self.cover_image = None;
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
                        app.account_avatar = None;
                        app.user_playlists = Arc::new(Vec::new());
                        app.login_open = false;
                    }
                    if app.collection.as_ref().is_some_and(|c| c.id.source == id2) {
                        app.collection = None;
                        app.collection_cover = None;
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
                        app.account_avatar = None;
                        app.user_playlists = Arc::new(Vec::new());
                        app.save_cookie(None);
                        if app
                            .collection
                            .as_ref()
                            .is_some_and(|c| c.id.kind == CollectionKind::Playlist)
                        {
                            app.collection = None;
                            app.collection_cover = None;
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
        self.login_qr = None;
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
        self.login_qr = None;
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
                        app.login_qr = None;
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
            self.account_avatar = None;
            return;
        };
        let Some(url) = profile.avatar.clone() else {
            self.account_avatar = None;
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
                    app.account_avatar = Some(Self::rgba_to_image(w, h, &rgba));
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
        self.login_qr = None;
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
                    app.login_qr = image;
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
                            app.login_qr = None;
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
                app.account_avatar = None;
                app.user_playlists = Arc::new(Vec::new());
                app.save_cookie(None);
                if app
                    .collection
                    .as_ref()
                    .is_some_and(|c| c.id.kind == CollectionKind::Playlist)
                {
                    app.collection = None;
                    app.collection_cover = None;
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
        self.collection_cover = None;
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
                                            app.collection_cover =
                                                Some(Self::rgba_to_image(w, h, &rgba));
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
        let cfg = crate::config::load_or_default();
        self.settings_cache_input.update(cx, |state, cx| {
            state.set_content(stats.root.display().to_string(), cx);
        });
        self.settings_download_input.update(cx, |state, cx| {
            state.set_content(cfg.download_dir.display().to_string(), cx);
        });
        self.settings_beat_low_input.update(cx, |state, cx| {
            state.set_content(format!("{:.0}", self.beat_band.0), cx);
        });
        self.settings_beat_high_input.update(cx, |state, cx| {
            state.set_content(format!("{:.0}", self.beat_band.1), cx);
        });
        cx.notify();
    }

    fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        cx.notify();
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
        self.info_message = Some("正在清空对象缓存…".into());
        cx.notify();
        cx.spawn(async move |weak, cx| {
            let removed = cx
                .background_executor()
                .spawn(async move { cache.purge_all() })
                .await;
            let _ = weak.update(cx, |app, cx| {
                app.info_message = Some(format!("已清空对象缓存（{removed} 项）"));
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
            let track_for_download = track.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    downloads.download_blocking(&track_for_download, &url, format_hint.as_deref())
                })
                .await;
            let _ = weak.update(cx, |app, cx| {
                match result {
                    Ok(path) => {
                        app.info_message = Some(format!("已下载：{}", path.display()));
                        app.status_message = None;
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
                    app.cover_image = Some(Self::rgba_to_image(w, h, &rgba));
                    view.update(cx, |view, _| view.set_album_rgba(w, h, rgba));
                } else {
                    app.cover_image = None;
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
                                let text = text.trim().to_string();
                                let translation = line.translated_lyric.trim().to_string();
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

        let light = |id: &'static str, color: gpui::Hsla| {
            div()
                .id(id)
                .size(px(12.0))
                .rounded(px(6.0))
                .bg(color)
                .cursor_pointer()
        };
        let close = light("tl-close", crate::theme::hex(0xFF5F57)).on_click(|_, _, cx| cx.quit());
        let minimize = light("tl-min", crate::theme::hex(0xFEBC2E))
            .on_click(|_, window, _| window.minimize_window());
        let maximize = light("tl-max", crate::theme::hex(0x28C840))
            .on_click(|_, window, _| window.zoom_window());

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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(close)
                    .child(minimize)
                    .child(maximize),
            )
            .child(center)
            .children(self.status_message.as_ref().map(|msg| {
                div()
                    .max_w(px(360.0))
                    .text_size(px(10.5))
                    .text_color(theme.danger)
                    .text_ellipsis()
                    .child(SharedString::from(msg.clone()))
            }))
            .children(self.info_message.as_ref().map(|msg| {
                div()
                    .max_w(px(360.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_faint)
                    .text_ellipsis()
                    .child(SharedString::from(msg.clone()))
            }))
            .child(theme_btn)
            .into_any_element()
    }

    // ================= 左侧边栏 =================

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let slots = self.registry.snapshot();
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

        for slot in slots {
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
                        for playlist in self.user_playlists.iter().take(30) {
                            let weak = weak.clone();
                            let id = playlist.id.clone();
                            let title = playlist.title.clone();
                            let count = playlist
                                .track_count
                                .map(|n| n.to_string())
                                .unwrap_or_default();
                            let active = self
                                .collection
                                .as_ref()
                                .is_some_and(|c| c.id == playlist.id);
                            list = list.child(
                                div()
                                    .id(SharedString::from(format!(
                                        "user-playlist-{}",
                                        playlist.id.id
                                    )))
                                    .h(px(28.0))
                                    .px(px(9.0))
                                    .rounded(px(6.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(7.0))
                                    .bg(if active {
                                        theme.active
                                    } else {
                                        gpui::transparent_black()
                                    })
                                    .hover(|s| s.bg(theme.hover))
                                    .cursor_pointer()
                                    .child(crate::icons::icon(
                                        crate::icons::Icon::Layers,
                                        12.0,
                                        if active { theme.text } else { theme.text_faint },
                                    ))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(11.5))
                                            .text_color(theme.text)
                                            .text_ellipsis()
                                            .child(SharedString::from(title.clone())),
                                    )
                                    .when(!count.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .text_size(px(9.5))
                                                .text_color(theme.text_faint)
                                                .child(SharedString::from(count)),
                                        )
                                    })
                                    .on_click(move |_, _, cx| {
                                        let _ = weak.update(cx, |app, cx| {
                                            app.open_collection(id.clone(), title.clone(), cx);
                                        });
                                    }),
                            );
                        }
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
                    .overflow_y_scroll()
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
        index: usize,
        track: &Track,
        list: QueueList,
        queue_pos: Option<usize>,
        queue_current_id: Option<&chordal_core::model::TrackId>,
        weak: gpui::WeakEntity<ChordialApp>,
        cx: &mut App,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let is_current =
            queue_pos == Some(index) && queue_current_id.map(|id| id == &track.id).unwrap_or(false);
        let artists = track
            .artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        let album = track.album.as_ref().map(|a| a.name.as_str()).unwrap_or("");
        let duration = track
            .duration_ms
            .map(fmt_ms)
            .unwrap_or_else(|| "--:--".into());
        let source_label = match &track.origin {
            TrackOrigin::Local { .. } => "本地".to_string(),
            TrackOrigin::Remote { source } => match source.0.as_str() {
                "netease" => "网易云".to_string(),
                other => other.to_string(),
            },
        };
        let subtitle = match list {
            QueueList::Search => {
                if artists.is_empty() {
                    source_label
                } else {
                    format!("{artists}  ·  {source_label}")
                }
            }
            _ => {
                if !artists.is_empty() && !album.is_empty() {
                    format!("{artists}  ·  {album}")
                } else if !artists.is_empty() {
                    artists
                } else {
                    album.to_string()
                }
            }
        };
        let number = if is_current {
            crate::widgets::equalizer(&theme, true)
        } else {
            div()
                .text_size(px(10.5))
                .text_color(theme.text_faint)
                .child(SharedString::from((index + 1).to_string()))
                .into_any_element()
        };

        div()
            .id(SharedString::from(format!("track-{index}")))
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
                            .child(SharedString::from(track.title.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(SharedString::from(subtitle)),
                    ),
            )
            .child(
                div()
                    .w(px(40.0))
                    .text_size(px(10.5))
                    .text_color(theme.text_faint)
                    .text_align(gpui::TextAlign::Right)
                    .child(SharedString::from(duration)),
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
        let weak = cx.entity().downgrade();
        let body: AnyElement = uniform_list("track-list", n, move |range, _window, cx| {
            let current_id = queue_pos.and_then(|i| queue.get(i).map(|t| t.id.clone()));
            range
                .filter_map(|i| {
                    tracks.get(i).map(|t| {
                        Self::render_track_row(
                            i,
                            t,
                            list,
                            queue_pos,
                            current_id.as_ref(),
                            weak.clone(),
                            cx,
                        )
                    })
                })
                .collect::<Vec<_>>()
        })
        .flex_1()
        .min_h_0()
        .into_any_element();
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
        uniform_list("collection-results", n, move |range, _window, _cx| {
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
        })
        .flex_1()
        .min_h_0()
        .into_any_element()
    }

    fn render_collection_search_panel(
        &self,
        width: Option<f32>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
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

        div()
            .h_full()
            .when_some(width, |d, w| d.w(px(w)))
            .when(width.is_none(), |d| d.flex_1())
            .min_w(px(280.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.surface)
            .border_r_1()
            .border_color(theme.border)
            .child(self.render_pane_header(
                theme,
                "搜索",
                subtitle,
                Some(actions.into_any_element()),
            ))
            .child(self.render_search_tabs(theme, cx))
            .child(
                div()
                    .px(px(12.0))
                    .pb(px(8.0))
                    .child(self.search_input_view.clone()),
            )
            .child(body)
            .child(div().h(px(84.0)).flex_none())
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

        if self.view_tab == ViewTab::Search && self.search_kind != SearchKind::Track {
            return self.render_collection_search_panel(width, &theme, cx);
        }

        let (tracks, list, title, subtitle): (Arc<Vec<Track>>, QueueList, String, String) =
            match self.view_tab {
                ViewTab::Search => {
                    let visible: Arc<Vec<Track>> = match &self.source_route {
                        Some(route) => Arc::new(
                            self.search_tracks
                                .iter()
                                .filter(|t| &t.id.source == route)
                                .cloned()
                                .collect(),
                        ),
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
                            app.collection_cover = None;
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
        if has_album {
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
        if has_artist {
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

        div()
            .h_full()
            .when_some(width, |d, w| d.w(px(w)))
            .when(width.is_none(), |d| d.flex_1())
            .min_w(px(280.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.surface)
            .border_r_1()
            .border_color(theme.border)
            .child(self.render_pane_header(
                &theme,
                title,
                subtitle,
                Some(actions.into_any_element()),
            ))
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
            .child(div().h(px(84.0)).flex_none())
            .into_any_element()
    }

    // ================= 右侧：正在播放 =================

    fn render_now_playing(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let playing = self.player_status.state == PlayerState::Playing;
        let has_track = self.current_track.is_some();
        let title = self
            .current_track
            .as_ref()
            .map(|t| t.title.clone())
            .or_else(|| self.player_status.title.clone())
            .unwrap_or_else(|| "未在播放".into());
        let artists = self
            .current_track
            .as_ref()
            .map(|t| {
                t.artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        let album = self
            .current_track
            .as_ref()
            .and_then(|t| t.album.as_ref())
            .map(|a| a.name.clone())
            .unwrap_or_default();
        let source_label = self
            .current_track
            .as_ref()
            .map(|t| match &t.origin {
                TrackOrigin::Local { .. } => "本地".to_string(),
                TrackOrigin::Remote { source } => match source.0.as_str() {
                    "netease" => "网易云".to_string(),
                    other => other.to_string(),
                },
            })
            .unwrap_or_default();
        let quality = self
            .current_track
            .as_ref()
            .and_then(|t| t.quality.clone())
            .unwrap_or_default();

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
                            app.enter_immersive(cx);
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
                "未知艺人".to_string()
            } else {
                artists
            }
        } else {
            "从左侧列表选择一首歌开始".to_string()
        };

        let pill = |text: String| {
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
                .child(SharedString::from(text))
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
                    .child(SharedString::from(album)),
            );
        }
        if has_track {
            let mut meta = div().flex().items_center().gap(px(6.0)).mt(px(9.0));
            if !source_label.is_empty() {
                meta = meta.child(pill(source_label));
            }
            if !quality.is_empty() {
                meta = meta.child(pill(quality));
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
                    (line.translation.as_str(), "")
                } else {
                    (line.text.as_str(), line.translation.as_str())
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
                                .child(SharedString::from(main.to_string())),
                        )
                        .when(!sub.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(theme.text_faint)
                                    .text_ellipsis()
                                    .child(SharedString::from(sub.to_string())),
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
                        if app.current_track.is_some() {
                            app.enter_immersive(cx);
                            window.focus(&app.focus_handle);
                            cx.notify();
                        }
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
            );

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

        let card = div()
            .id("login-card")
            .w(px(380.0))
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

    fn render_dock(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let status = &self.player_status;
        let title = status.title.clone().unwrap_or_else(|| "未在播放".into());
        let playing = status.state == PlayerState::Playing;
        let duration = status.duration_ms.unwrap_or(0);
        let position = status.position_ms.min(duration.max(1));
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };
        let artist = match self.current_track.as_ref() {
            Some(track) => {
                let joined = track
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ");
                if !joined.is_empty() {
                    joined
                } else {
                    track
                        .album
                        .as_ref()
                        .map(|a| a.name.clone())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "未知艺人".into())
                }
            }
            None => "选择一首歌开始".into(),
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
                    if app.current_track.is_some() {
                        app.enter_immersive(cx);
                        window.focus(&app.focus_handle);
                        cx.notify();
                    }
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
                            .child(SharedString::from(title)),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .text_ellipsis()
                            .child(SharedString::from(artist)),
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
                    let _ = weak.update(cx, |app, _| app.player.toggle());
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
                        app.enter_immersive(cx);
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

    fn render_immersive(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let bounds = window.bounds();
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        // 背景铺满整个窗口：光栅长边 ≤ 200px（30fps ≈ 25k px/帧），
        // 最终像素由 GPU 双线性采样到全窗。
        let bg_scale = (200.0 / width.max(height)).clamp(0.04, 0.5);
        self.lyric_view.update(cx, |view, _| {
            view.set_background_bounds(Some(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(width), px(height)),
            )));
            view.set_background_render_scale(bg_scale);
        });
        if width >= height {
            self.render_immersive_wide(width, height, cx)
        } else {
            self.render_immersive_compact(width, height, cx)
        }
    }

    /// 窄窗沉浸：AMLL react-full `VerticalLayout` 的等价几何（宽 < 高）。
    ///
    /// 800×1000 实测：行 drag 30 / thumb 30 / main 1fr（gap 2）；
    /// 小封面 6em=96（宽 ≤ 480 时 4.5em），x = 3em，y = main_top + 1em；
    /// 曲目信息在封面右侧垂直居中（16px：name 500/op.9、artists op.45），
    /// 菜单钮 35×35 贴右缘 3em；歌词铺满其余高度并对齐 0.1/top，
    /// mask 只在顶部 10% 渐隐；歌词打开时不显示传输控件（与 AMLL 竖向一致）。
    fn render_immersive_compact(
        &mut self,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = height;
        let theme = Theme::dark();
        let white = gpui::white();
        let em = 16.0;
        let gap = 2.0;
        let row_h = 30.0;
        let main_top = row_h + gap + row_h + gap;
        let small_cover = if width <= 480.0 { em * 4.5 } else { em * 6.0 };
        let side_pad = if width <= 480.0 { 20.0 } else { em * 3.0 };
        let cover_x = side_pad;
        let cover_y = main_top + em;

        let status = &self.player_status;
        let title = status.title.clone().unwrap_or_else(|| "未在播放".into());
        // AMLL MusicInfo：艺人以 ", " 连接，最后一位前用 " & "。
        let artist = match self.current_track.as_ref() {
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
        };

        // AMLL 竖向：歌词对齐 alignPosition 0.1 / top（歌曲信息下方）。
        if (self.lyric_align - 0.1).abs() > 0.0005 {
            self.lyric_align = 0.1;
            self.lyric_view.update(cx, |view, _| {
                view.set_player_config(LyricPlayerConfig {
                    align_anchor: AlignAnchor::Top,
                    align_position: 0.1,
                    enable_blur: !self.no_blur,
                    ..LyricPlayerConfig::default()
                });
            });
        }

        // —— 顶部拖拽 pill（ControlThumb 50×8 r4 白 17.6%，thumb 行内居中）——
        let pill = Self::window_drag_area(
            div()
                .id("immersive-pill-compact")
                .absolute()
                .left(px(width / 2.0 - 25.0))
                .top(px(row_h + gap + 11.0))
                .w(px(50.0))
                .h(px(8.0))
                .rounded(px(4.0))
                .bg(white.opacity(0.176))
                .cursor(CursorStyle::OpenHand),
        );

        // —— 小封面 + 曲目信息 + 菜单钮 ——
        let small_cover_el = crate::widgets::cover_plain(
            &theme,
            self.cover_image.clone(),
            small_cover,
            8.0,
            crate::icons::icon(crate::icons::Icon::Note, 24.0, white.opacity(0.4)),
        );
        let info_font = em;
        let controls_row = div()
            .absolute()
            .left(px(cover_x))
            .right(px(side_pad))
            .top(px(cover_y))
            .h(px(small_cover))
            .flex()
            .items_center()
            .child(small_cover_el)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .pl(px(em))
                    .line_height(px(info_font * 1.25))
                    .child(
                        div()
                            .max_w_full()
                            .text_size(px(info_font))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(white.opacity(0.9))
                            .text_ellipsis()
                            .child(SharedString::from(title)),
                    )
                    .child(
                        div()
                            .max_w_full()
                            .text_size(px(info_font))
                            .text_color(white.opacity(0.45))
                            .text_ellipsis()
                            .child(SharedString::from(artist)),
                    ),
            )
            .child(
                div()
                    .id("immersive-menu-compact")
                    .size(px(35.0))
                    .rounded(px(17.5))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .bg(white.opacity(0.08))
                    .hover(|s| s.bg(white.opacity(0.133)))
                    .child(crate::icons::icon(crate::icons::Icon::More, 16.0, white)),
            );

        // —— 歌词：从控制行下缘铺到窗口底部，mask 顶部 10% 渐隐（文本层实现）——
        let lyric_top = cover_y + small_cover;
        let lyric_area = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(lyric_top))
            .bottom_0()
            .child(self.lyric_view.clone());

        let drag_strip = Self::window_drag_area(
            div()
                .id("immersive-drag-strip-compact")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(main_top))
                .cursor(CursorStyle::OpenHand),
        );

        div()
            .key_context("ChordialImmersive")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
                app.exit_immersive(cx);
                window.focus(&app.focus_handle);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &PlayPause, _, _cx| app.player.toggle()))
            .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
            .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(gpui::black())
            .child(lyric_area)
            .child(controls_row)
            .child(drag_strip)
            .child(pill)
            .children(Self::resize_handles())
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
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::dark();
        let weak = cx.entity().downgrade();
        // AMLL 控件动画进度：进入页面时所有按钮回弹；播放/暂停、shuffle、
        // repeat 在各自状态切换时重新播放回弹；上/下一首只播放入场双箭头合并。
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
        let speaker_progress = Self::anim_progress(
            self.ui_anim_volume.map(|(t, _)| t),
            Self::UI_ANIM_SPEAKER_MS,
        );
        let (speaker_low, speaker_high) = match self.ui_anim_volume {
            Some((_, true)) => (0.0, speaker_progress),
            Some((_, false)) => (speaker_progress, 0.0),
            None => (0.0, 0.0),
        };
        let status = &self.player_status;
        let title = status.title.clone().unwrap_or_else(|| "未在播放".into());
        let playing = status.state == PlayerState::Playing;
        let duration = status.duration_ms.unwrap_or(0);
        let position = status.position_ms.min(duration.max(1));
        let progress = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };
        let remaining = duration.saturating_sub(position);
        // AMLL MusicInfo：艺人以 ", " 连接，最后一位前用 " & "。
        let artist = match self.current_track.as_ref() {
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
        };
        let quality = self
            .current_track
            .as_ref()
            .and_then(|t| t.quality.clone())
            .unwrap_or_default();

        // ---- AMLL horizontal 几何 ----
        let cover = if height <= 1000.0 {
            (height * 0.45).min(width * 0.38)
        } else {
            (height * 0.5).min(width * 0.38)
        };
        let gap = 8.0;
        let fr = (height - 120.0 - cover).max(0.0);
        let drag = fr * 0.12;
        let tail = fr * 0.08;
        let cover_top = drag + gap + 40.0 + gap;
        // 底部控制行占 [bottom_row_top, +40]，音量行必须停在它上方；信息区
        // 用剩余高度做 space-between，避免矮窗口时被挤出窗口底边。
        let bottom_row_top = height - tail - gap - 40.0;
        let controls_top = cover_top + cover + 20.0;
        let controls_h = (bottom_row_top - gap - controls_top).max(0.0);
        let info_w = width * 0.45;
        let info_center = info_w * 0.5;
        let lyric_left = info_w + gap;
        let lyric_col_w = (width - lyric_left).max(0.0);
        let lyric_pad_right = if height <= 1000.0 {
            lyric_col_w * 0.08
        } else {
            lyric_col_w * 0.15
        };
        let align = ((cover_top + cover * 0.5) / height).clamp(0.05, 0.95);
        if (self.lyric_align - align).abs() > 0.0005 {
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

        let white = gpui::white();
        // AMLL MediaButton：hover 时白 13% 圆底；图标尺寸按 81px 按钮实测比例。
        let btn = cover * 0.18;
        let icon_side = (btn * 0.26).max(14.0);
        let icon_skip = (btn * 0.32).max(16.0);
        let icon_play = (btn * 0.37).max(18.0);

        // —— 顶部拖拽 pill（AMLL ControlThumb：50×8、圆角 4、白 17.6%）——
        let pill = Self::window_drag_area(
            div()
                .id("immersive-pill")
                .absolute()
                .left(px(info_center - 25.0))
                .top(px(drag + gap + 16.0))
                .w(px(50.0))
                .h(px(8.0))
                .rounded(px(4.0))
                .bg(white.opacity(0.176))
                .cursor(CursorStyle::OpenHand),
        );

        // —— 封面（AMLL Cover：无圆角，播放时 drop-shadow）——
        let cover_el = crate::widgets::cover_plain(
            &theme,
            self.cover_image.clone(),
            cover,
            0.0,
            crate::icons::icon(crate::icons::Icon::Note, 40.0, white.opacity(0.4)),
        );

        // —— 曲目信息：字号 max(2vh, 1em)；name 500/op.9、artists 400/op.45 ——
        let info_font = (height * 0.02).max(16.0);
        let info_line_h = px(info_font * 1.25);
        let info_row = div()
            .w(px(cover))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .line_height(info_line_h)
                    .child(
                        div()
                            .max_w_full()
                            .text_size(px(info_font))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(white.opacity(0.9))
                            .text_ellipsis()
                            .child(SharedString::from(title.clone())),
                    )
                    .child(
                        div()
                            .max_w_full()
                            .text_size(px(info_font))
                            .text_color(white.opacity(0.45))
                            .text_ellipsis()
                            .child(SharedString::from(artist)),
                    ),
            )
            .child(
                div()
                    .id("immersive-menu")
                    .size(px(35.0))
                    .rounded(px(17.5))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .bg(white.opacity(0.08))
                    .hover(|s| s.bg(white.opacity(0.133)))
                    .child(crate::icons::icon(crate::icons::Icon::More, 16.0, white)),
            );

        // —— 进度条（BouncingSlider 等价）+ 标签行（12.8px w500 op.5）——
        let label_font = (height * 0.012).max(12.8);
        let seek = {
            let player = self.player.clone();
            move |fraction: f32, _cx: &mut App| {
                if duration > 0 {
                    player.seek_ms((fraction.clamp(0.0, 1.0) * duration as f32) as u64);
                }
            }
        };
        let progress_bar = crate::widgets::amll_slider(progress, "seek", seek);
        let quality_pill = if quality.is_empty() {
            div().into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .rounded(px(3.0))
                .border_1()
                .border_color(white.opacity(0.2))
                .px(px(6.66))
                .py(px(2.0))
                .text_size(px(label_font))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(white)
                .child(SharedString::from(quality.to_uppercase()))
                .into_any_element()
        };
        let labels = div()
            .w(px(cover))
            .mt(px(4.0))
            .flex()
            .items_center()
            .text_size(px(label_font))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(white.opacity(0.5))
            .child(SharedString::from(fmt_ms(position)))
            .child(div().flex_1().flex().justify_center().child(quality_pill))
            .child(SharedString::from(format!("-{}", fmt_ms(remaining))));

        let progress_block = div()
            .w(px(cover))
            .flex()
            .flex_col()
            .child(div().h(px(24.0)).child(progress_bar))
            .child(labels);

        // —— 传输按钮：5 × 18%cover，space-evenly（等宽弹性间隔实现）——
        let shuffle_active = self.play_mode == PlayMode::Shuffle;
        let repeat_active = self.play_mode == PlayMode::RepeatOne;
        let sp = || div().flex_1();
        let shuffle_btn = {
            let weak = weak.clone();
            div()
                .id("immersive-shuffle")
                .size(px(btn))
                .rounded(px(btn / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.133)))
                .child(crate::icons::icon_motion(
                    crate::icons::Icon::Shuffle,
                    icon_side,
                    if shuffle_active {
                        white
                    } else {
                        white.opacity(0.55)
                    },
                    crate::icons::IconMotion {
                        press: press_shuffle,
                        ..Default::default()
                    },
                ))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.play_mode = if app.play_mode == PlayMode::Shuffle {
                            PlayMode::Sequential
                        } else {
                            PlayMode::Shuffle
                        };
                        // AMLL：shuffle/repeat 的 on/off 是两套 SVG，切换时重新挂载
                        // → 图标播放 pressed 回弹。
                        app.ui_anim_shuffle = Some(Instant::now());
                        app.ensure_ui_anim_ticker(cx);
                        cx.notify();
                    });
                })
        };
        let prev_btn = {
            let weak = weak.clone();
            div()
                .id("immersive-prev")
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
        let play_btn = {
            let weak = weak.clone();
            div()
                .id("immersive-play")
                .size(px(btn))
                .rounded(px(btn / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.133)))
                .child(crate::icons::icon_motion(
                    if playing {
                        crate::icons::Icon::Pause
                    } else {
                        crate::icons::Icon::Play
                    },
                    icon_play,
                    white,
                    crate::icons::IconMotion {
                        press: press_play,
                        ..Default::default()
                    },
                ))
                .on_click(move |_, _, cx| {
                    // 状态切换由 200ms 轮询观察；图标交换与回弹在那里同帧触发。
                    let _ = weak.update(cx, |app, _| app.player.toggle());
                })
        };
        let next_btn = {
            let weak = weak.clone();
            div()
                .id("immersive-next")
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
        let repeat_btn = {
            let weak = weak.clone();
            div()
                .id("immersive-repeat")
                .size(px(btn))
                .rounded(px(btn / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(white.opacity(0.133)))
                .child(crate::icons::icon_motion(
                    crate::icons::Icon::Repeat,
                    icon_side,
                    if repeat_active {
                        white
                    } else {
                        white.opacity(0.55)
                    },
                    crate::icons::IconMotion {
                        press: press_repeat,
                        ..Default::default()
                    },
                ))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.play_mode = if app.play_mode == PlayMode::RepeatOne {
                            PlayMode::Sequential
                        } else {
                            PlayMode::RepeatOne
                        };
                        app.ui_anim_repeat = Some(Instant::now());
                        app.ensure_ui_anim_ticker(cx);
                        cx.notify();
                    });
                })
        };
        let transport = div()
            .w(px(cover))
            .flex()
            .items_center()
            .child(shuffle_btn)
            .child(sp())
            .child(prev_btn)
            .child(sp())
            .child(play_btn)
            .child(sp())
            .child(next_btn)
            .child(sp())
            .child(repeat_btn);

        // —— 音量行：小喇叭 + 滑杆 + 大喇叭（opacity 0.5，margin -12/-8 的等价内距）——
        let volume_value = status.volume.clamp(0.0, 1.0);
        let volume_slider = {
            let player = self.player.clone();
            let weak = weak.clone();
            crate::widgets::amll_slider(
                volume_value,
                "volume",
                move |fraction: f32, cx: &mut App| {
                    player.set_volume(fraction);
                    // AMLL VolumeControl：到达 0 / 1 时对应喇叭的部件依次弹跳。
                    let _ = weak.update(cx, |app, cx| {
                        app.note_volume_input(fraction, cx);
                    });
                },
            )
        };
        let volume_row = div()
            .w(px(cover))
            .flex()
            .items_center()
            .child(crate::icons::icon_motion(
                crate::icons::Icon::VolumeLow,
                16.0,
                white.opacity(0.5),
                crate::icons::IconMotion {
                    speaker: speaker_low,
                    ..Default::default()
                },
            ))
            .child(div().flex_1().child(volume_slider))
            .child(crate::icons::icon_motion(
                crate::icons::Icon::VolumeHigh,
                16.0,
                white.opacity(0.5),
                crate::icons::IconMotion {
                    speaker: speaker_high,
                    ..Default::default()
                },
            ));

        // —— 左列：封面固定，信息/进度/传输/音量在剩余高度内 space-between；
        //    间距随窗口高度伸缩，音量行始终落在底部控制行上方。 ——
        let controls = div()
            .w(px(cover))
            .h(px(controls_h))
            .mt(px(20.0))
            .flex()
            .flex_col()
            .items_center()
            .justify_between()
            .child(info_row)
            .child(progress_block)
            .child(transport)
            .child(volume_row);
        let info_column = div()
            .absolute()
            .left_0()
            .top(px(cover_top))
            .w(px(info_w))
            .flex()
            .flex_col()
            .items_center()
            .child(cover_el)
            .child(controls);

        // —— 歌词列：player-side 全高，右 padding 8% ——
        let lyric_area = div()
            .absolute()
            .left(px(lyric_left))
            .top_0()
            .bottom_0()
            .right_0()
            .pr(px(lyric_pad_right))
            .child(self.lyric_view.clone());

        // —— 底部控制行：左全屏、右队列/退出，40×40 圆钮 opacity 0.45，gap 32 ——
        let fullscreen_btn = div()
            .id("immersive-fullscreen")
            .size(px(40.0))
            .rounded(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .opacity(0.45)
            .hover(|s| s.opacity(1.0).bg(white.opacity(0.133)))
            .child(crate::icons::icon(crate::icons::Icon::Expand, 18.0, white))
            .on_click(|_, window, _| window.toggle_fullscreen());
        let queue_btn = {
            let weak = weak.clone();
            div()
                .id("immersive-queue")
                .size(px(40.0))
                .rounded(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .opacity(0.45)
                .hover(|s| s.opacity(1.0).bg(white.opacity(0.133)))
                .child(crate::icons::icon(crate::icons::Icon::Queue, 18.0, white))
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.exit_immersive(cx);
                        app.view_tab = ViewTab::Library;
                        app.library_filter = LibraryFilter::Queue;
                        window.focus(&app.focus_handle);
                        cx.notify();
                    });
                })
        };
        let close_btn = {
            let weak = weak.clone();
            div()
                .id("immersive-close")
                .size(px(40.0))
                .rounded(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .opacity(0.45)
                .hover(|s| s.opacity(1.0).bg(white.opacity(0.133)))
                .child(crate::icons::icon(crate::icons::Icon::Close, 18.0, white))
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.exit_immersive(cx);
                        window.focus(&app.focus_handle);
                        cx.notify();
                    });
                })
        };
        let bottom_row = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(bottom_row_top))
            .h(px(40.0))
            .px(px(32.0))
            .flex()
            .items_center()
            .justify_between()
            .child(fullscreen_btn)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(32.0))
                    .child(queue_btn)
                    .child(close_btn),
            );

        // 顶部拖拽区（AMLL drag-area 行：整宽、可拖动窗口）
        let drag_strip = Self::window_drag_area(
            div()
                .id("immersive-drag-strip")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(drag))
                .cursor(CursorStyle::OpenHand),
        );

        div()
            .key_context("ChordialImmersive")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
                app.exit_immersive(cx);
                window.focus(&app.focus_handle);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &PlayPause, _, _cx| app.player.toggle()))
            .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
            .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(gpui::black())
            .child(lyric_area)
            .child(info_column)
            .child(drag_strip)
            .child(pill)
            .child(bottom_row)
            .children(Self::resize_handles())
            .into_any_element()
    }
}

impl Render for ChordialApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        self.viewport_width = f32::from(viewport.width);
        self.viewport_height = f32::from(viewport.height);
        if self.immersive {
            let width = f32::from(viewport.width);
            let height = f32::from(viewport.height);
            let (style, font, vertical) = immersive_lyric_style(width, height);
            if (font - self.lyric_font_px).abs() > 1.0 || vertical != self.lyric_vertical {
                self.lyric_font_px = font;
                self.lyric_vertical = vertical;
                self.lyric_view
                    .update(cx, |view, _| view.set_text_style(style));
            }
            return self.render_immersive(window, cx).into_any_element();
        }
        if !self.activated {
            self.activated = true;
            window.activate_window();
        }
        let scope = PerfScope::enter("ui.render.pass");
        let theme = Theme::get(cx).clone();
        let show_now = self.viewport_width >= 1020.0;
        let list_width = show_now.then(|| self.list_width());

        let view = div()
            .key_context("ChordialMain")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &PlayPause, _window, _cx| {
                app.player.toggle();
                app.player_status = app.player.status();
            }))
            .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
            .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
            .on_action(cx.listener(|app, _: &FocusSearch, window, cx| {
                focus_input(&app.search_input, window, cx)
            }))
            .on_action(cx.listener(|app, _: &RescanLibrary, _, cx| {
                if let Some(local) = app.local_source.clone() {
                    app.spawn_library_scan(local, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &ToggleSidebar, _, cx| {
                app.sidebar_open = !app.sidebar_open;
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
            .child(self.render_titlebar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(self.sidebar_open, |d| d.child(self.render_sidebar(cx)))
                    .child(self.render_list_panel(list_width, cx))
                    .when(show_now, |d| d.child(self.render_now_playing(cx))),
            )
            .child(self.render_dock(cx))
            .when(self.settings_open, |d| d.child(self.render_settings(cx)))
            .when(self.login_open, |d| d.child(self.render_login(cx)))
            .children(Self::resize_handles());
        scope.finish(1);
        view.into_any_element()
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
    use super::parse_hz_input;

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
}
