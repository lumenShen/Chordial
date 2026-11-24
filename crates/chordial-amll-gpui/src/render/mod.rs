//! gpui 歌词渲染视图。
//!
//! 这里不依赖任何 DOM/CSS：文本由 gpui 整形后，**逐字形**绘制。这样能精确
//! 复现 AMLL 的几个关键视觉效果：
//! - 整行渐变扫光（一条 mask 前沿跨字/词推进，不再依赖 CSS mask）；
//! - 强调词逐字缩放 + 辉光（AMLL `initEmphasizeAnimation` 的等价缓动与参数）；
//! - 行级弹簧位移/缩放、透明度/模糊过渡；
//! - 对唱行右对齐、背景歌词行、间奏呼吸点。
//!
//! 性能：每帧只为 `visible_range()`（含 overscan）内的行整形并绘制字形；
//! 文本整形结果按行 LRU 缓存；`paint_glyph` 调用数只与可见字符数相关。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chordal_core::perf::PerfScope;
use gpui::prelude::*;
use gpui::{
    App, Bounds, ContentMask, Context, Corners, Element, ElementId, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, Pixels, Render,
    RenderImage, SpriteMask, Style, Window, div, fill, point, px, relative,
};

use crate::background::{BackgroundRendererKind, LyricBackgroundRenderer};
use crate::model::{LyricLine, LyricWord};
use crate::player::{
    InterludeDotsFrame, LayoutMetrics, LineFrameState, LyricPlayer, LyricPlayerConfig,
};
use crate::text::{EmphasisChunk, LineShape, LyricTextStyle, ShapedRow, TextLayoutCache, WordSpan};

mod emphasis;
mod paint;

use emphasis::*;
use paint::*;

/// 播放时钟：视图每帧读取，避免外层再维护一套 60Hz 定时器。
pub trait PlaybackClock: Send + Sync {
    fn position_ms(&self) -> u64;
    fn is_playing(&self) -> bool;

    /// 一次读取位置与播放状态（默认分两次调用；实现方可合并锁）。
    fn snapshot(&self) -> (u64, bool) {
        (self.position_ms(), self.is_playing())
    }
}

/// 视图配色。
#[derive(Debug, Clone, Copy)]
pub struct LyricRenderColors {
    pub text: Hsla,
    pub translation: Hsla,
    /// 背景图上的压暗层（也是无封面时的底色）。
    pub overlay: Hsla,
    pub interlude_dot: Hsla,
    /// 无封面/CSS 背景时的纯色兜底。
    pub fallback_background: Hsla,
    /// 强调词的辉光颜色。
    pub glow: Hsla,
}

impl Default for LyricRenderColors {
    fn default() -> Self {
        Self {
            text: gpui::white(),
            translation: gpui::white(),
            overlay: gpui::rgba(0x00000055).into(),
            interlude_dot: gpui::white(),
            fallback_background: gpui::rgb(0x111111).into(),
            glow: gpui::white(),
        }
    }
}

/// 单曲歌词替换的时长。t=0 开始交叉淡化，t=600ms 旧层移除、新层进入稳定态。
const LYRIC_SWITCH_MS: f32 = 600.0;
/// 切换时的模糊半径（单位 em，按每行字号换算成 px；vendor/gpui 会再按 5px 截断）。
const LYRIC_SWITCH_BLUR_EM: f32 = 0.12;
/// 切换时内容的缩放终点（只作用于字形及其坐标，容器/背景不动）。
const LYRIC_SWITCH_SCALE: f32 = 0.95;

/// 一层歌词在切换动画中的视觉参数。
///
/// 注意 `content_scale` 与行弹簧的 `LineFrameState::scale` 是两回事：后者由
/// AMLL 的高亮状态驱动，并同时决定扫光的亮/暗；这里只做一次整体内容缩放，
/// 不改行状态，也不碰容器。
#[derive(Debug, Clone, Copy, PartialEq)]
struct LyricFade {
    opacity: f32,
    blur_em: f32,
    content_scale: f32,
}

impl LyricFade {
    const NORMAL: Self = Self {
        opacity: 1.0,
        blur_em: 0.0,
        content_scale: 1.0,
    };
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// 歌词替换的纯逻辑时间线（不持有任何 GPU 资源，便于单测）。
#[derive(Debug, Clone, Copy)]
struct LyricSwitch {
    elapsed_ms: f32,
}

impl LyricSwitch {
    fn new() -> Self {
        Self { elapsed_ms: 0.0 }
    }

    /// 推进时间线；返回 false 表示 600ms 已到，调用方应移除淡出层。
    fn advance(&mut self, dt_ms: f32) -> bool {
        self.elapsed_ms = (self.elapsed_ms + dt_ms.max(0.0)).min(LYRIC_SWITCH_MS);
        self.elapsed_ms < LYRIC_SWITCH_MS
    }

    #[inline]
    fn t(&self) -> f32 {
        (self.elapsed_ms / LYRIC_SWITCH_MS).clamp(0.0, 1.0)
    }

    /// 旧歌词：模糊 0→0.12em、淡出 1→0、内容 scale 1→0.95。
    fn outgoing_fade(&self) -> LyricFade {
        let p = smoothstep(self.t());
        LyricFade {
            opacity: 1.0 - p,
            blur_em: LYRIC_SWITCH_BLUR_EM * p,
            content_scale: 1.0 - (1.0 - LYRIC_SWITCH_SCALE) * p,
        }
    }

    /// 新歌词：模糊 0.12em→0、淡入 0→1、内容 scale 0.95→1。
    fn incoming_fade(&self) -> LyricFade {
        let p = smoothstep(self.t());
        LyricFade {
            opacity: p,
            blur_em: LYRIC_SWITCH_BLUR_EM * (1.0 - p),
            content_scale: LYRIC_SWITCH_SCALE + (1.0 - LYRIC_SWITCH_SCALE) * p,
        }
    }
}

/// 正在淡出的上一首歌词：播放器状态与排版缓存整体冻结，跟随切换时间线绘制。
/// 背景仍是当前层唯一的，不参与淡出（避免切换时背景闪一下）。
struct OutgoingLyrics {
    player: LyricPlayer,
    text: TextLayoutCache,
}

struct Inner {
    player: LyricPlayer,
    text: TextLayoutCache,
    background: LyricBackgroundRenderer,
    time_ms: u64,
    playing: bool,
    last_instant: Instant,
    dt_ms: f32,
    colors: LyricRenderColors,
    bg_image: Option<Arc<RenderImage>>,
    /// 已烘焙的 32×32 BGRA 调色纹理（4 KiB）。切换背景渲染器时用它
    /// 重建，避免长期保留整幅封面 RGBA（512² ≈ 1 MiB）。
    album_palette: Option<Vec<u8>>,
    last_heights: Vec<f32>,
    last_width: f32,
    last_height: f32,
    metrics_dirty: bool,
    animating: bool,
    clock: Option<Arc<dyn PlaybackClock>>,
    bg_fps: u32,
    bg_scale: f32,
    bg_flow_speed: f32,
    bg_static: bool,
    /// 鼓点视觉倍率（0..=1.5），切换渲染器/重建时保留。
    bg_beat_strength: f32,
    /// 由外部绘制全窗口背景时关闭内部背景，只保留歌词。
    background_visible: bool,
    /// 只画背景、不画歌词（竖屏播放页把同一视图当作动态背景层）。
    text_visible: bool,
    /// 隐藏文字时仍保持整形/测量。沉浸页的歌词开关依赖它：点开歌词时
    /// 直接复用已经算好的行高与滚动位置，而不是从零做一次初始布局。
    layout_when_hidden: bool,
    /// 是否响应「点击歌词行跳转」。背景层必须关闭，避免误 seek。
    click_seek_enabled: bool,
    /// 容器底部 mask-image 渐隐比例（相对容器高度；`-1` = 用样式默认值）。
    /// 只在绘制期乘到字形 alpha 上，不进入整形管线，闲置显隐不会重排歌词。
    bottom_mask: f32,
    /// 背景光栅/绘制的覆盖区域。沉浸层里歌词视图只占一列，但背景要铺满
    /// 整个窗口：此时把该值设为窗口 bounds，光栅按全窗尺寸计算（低分辨率），
    /// 由 GPU 双线性上采样到屏幕。
    background_bounds: Option<Bounds<Pixels>>,
    last_bounds: Bounds<Pixels>,
    seek_handler: Option<Arc<dyn Fn(u64) + Send + Sync>>,
    /// 鼓点包络来源（0..1）。每帧读取，避免外层再维护定时器。
    beat_source: Option<Arc<dyn Fn() -> f32 + Send + Sync>>,
    /// 已经不再绘制的背景帧。下一帧 `prepare` 里从 gpui sprite atlas 移除，
    /// 否则 30fps 的背景每帧都会永久占用一个纹理图块（图集只增不减）。
    retired_images: Vec<Arc<RenderImage>>,
    /// 状态快照 33ms 才更新一次，这里用帧间隔外推，让扫光前沿保持帧率级平滑。
    last_clock_pos: u64,
    display_time_ms: f64,
    initial_shaped: bool,
    /// 上一次请求 `request_animation_frame` 的时刻；背景单独流动时按
    /// `bg_fps` 限速，歌词动画期间则每个显示刷新周期都请求。
    last_frame_request: Instant,
    /// 歌词替换时的淡出层与进度。任意时刻最多一层：快速连续切歌时先把当前
    /// 层按稳定态接管为新的淡出层，旧淡出层直接丢弃，避免叠字/闪黑/累积。
    outgoing: Option<OutgoingLyrics>,
    switch: Option<LyricSwitch>,
}

impl Inner {
    fn new() -> Self {
        let text = TextLayoutCache::new(LyricTextStyle::default());
        let base_font = text.style().base_font_size;
        let config = LyricPlayerConfig {
            base_font_size: base_font,
            ..Default::default()
        };
        Self {
            player: LyricPlayer::new(config),
            text,
            background: LyricBackgroundRenderer::new(BackgroundRendererKind::MeshGradient),
            time_ms: 0,
            playing: false,
            last_instant: Instant::now(),
            dt_ms: 16.0,
            colors: LyricRenderColors::default(),
            bg_image: None,
            album_palette: None,
            last_heights: Vec::new(),
            last_width: 0.0,
            last_height: 0.0,
            metrics_dirty: true,
            animating: false,
            clock: None,
            bg_fps: 30,
            bg_scale: 0.5,
            bg_flow_speed: 4.0,
            bg_static: false,
            bg_beat_strength: 1.0,
            background_visible: true,
            text_visible: true,
            layout_when_hidden: false,
            click_seek_enabled: true,
            bottom_mask: -1.0,
            background_bounds: None,
            last_bounds: Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(0.0), px(0.0))),
            seek_handler: None,
            beat_source: None,
            retired_images: Vec::new(),
            last_clock_pos: 0,
            display_time_ms: 0.0,
            initial_shaped: false,
            last_frame_request: Instant::now() - Duration::from_secs(1),
            outgoing: None,
            switch: None,
        }
    }

    /// 把当前歌词层冻结为淡出层，并清零切换进度。
    ///
    /// 连续切歌（上一次 600ms 还没走完）时，更早的淡出层直接释放；当前层
    /// 此时可能正处于淡入中途，这里按「完整可见」接管，保证每次切换都从
    /// 一层的稳定画面出发，不会出现两层都半透明的黑屏。
    fn begin_switch(&mut self) {
        if self.player.document().is_empty() {
            self.outgoing = None;
            self.switch = None;
            return;
        }
        let config = *self.player.config();
        let style = self.text.style().clone();
        let old_player = std::mem::replace(&mut self.player, LyricPlayer::new(config));
        let old_text = std::mem::replace(&mut self.text, TextLayoutCache::new(style));
        self.outgoing = Some(OutgoingLyrics {
            player: old_player,
            text: old_text,
        });
        self.switch = Some(LyricSwitch::new());
    }

    /// 立即结束切换：丢弃淡出层，当前层恢复稳定态。清歌词/停止时调用。
    fn finish_switch(&mut self) {
        self.outgoing = None;
        self.switch = None;
    }

    /// 比较可见行的高度缓存，有变化才重建布局（O(可见行)，不是 O(n)）。
    fn check_heights(&mut self, range: std::ops::Range<usize>) -> bool {
        let heights = self.text.heights();
        if self.last_heights.len() != heights.len() {
            self.last_heights.clear();
            self.last_heights.extend_from_slice(heights);
            self.metrics_dirty = true;
            return true;
        }
        let mut changed = false;
        for i in range {
            if let (Some(&new), Some(old)) = (heights.get(i), self.last_heights.get(i))
                && (new - old).abs() > 0.01
            {
                self.last_heights[i] = new;
                changed = true;
            }
        }
        self.metrics_dirty |= changed;
        changed
    }

    /// 同步排版视口。必须与文本是否可见无关：歌词隐藏时（例如沉浸层切到
    /// 队列/音频详情）画布仍可能换成整窗尺寸并改写缓存的 viewport；若这里
    /// 不记录新尺寸，切回原列宽时 `last_width` 会误判为未变化，跳过
    /// `set_viewport`，缓存继续按整窗宽度换行，歌词就会溢出且不换行。
    fn sync_viewport(&mut self, width: f32, height: f32) {
        if !width.is_finite() || !height.is_finite() || width <= 1.0 || height <= 1.0 {
            return;
        }
        if (width - self.last_width).abs() < 0.01 && (height - self.last_height).abs() < 0.01 {
            return;
        }
        self.last_width = width;
        self.last_height = height;
        self.text.set_viewport(width, height);
        self.last_heights.clear();
        self.metrics_dirty = true;
    }

    fn sync_metrics(&mut self, width: f32, height: f32) {
        if !self.metrics_dirty {
            return;
        }
        self.metrics_dirty = false;
        let heights = self.text.heights();
        self.last_heights.clear();
        self.last_heights.extend_from_slice(heights);
        self.player.set_metrics(LayoutMetrics {
            width,
            height,
            base_font_size: self.text.style().base_font_size,
            padding_x: self.text.style().base_font_size * self.text.style().padding_x_em.max(0.0),
            line_heights: heights.to_vec(),
        });
    }

    fn prepare(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        if width <= 1.0 || height <= 1.0 {
            return;
        }

        self.sync_viewport(width, height);

        // 竖屏播放页把歌词视图当纯背景使用：不整形、不测量，避免每帧
        // 对不可见文本做无用工作。沉浸页则把 `layout_when_hidden` 打开，
        // 让歌词在隐藏时就完成测量并跟上播放进度，点开时无需重置。
        if self.text_visible || self.layout_when_hidden {
            let shaped_before = self.text.shaped_count();
            // 只整形当前滚动位置附近的一小段；其余行保留估算高度，
            // 随视口推进按需整形。完整整形 1000 行会造成单帧 50-500ms 卡顿。
            if !self.initial_shaped {
                let n = self.player.document().len();
                if n > 0 {
                    let center = self.player.scroll_to_index().min(n);
                    let start = center.saturating_sub(20);
                    let end = (center + 80).min(n);
                    let doc = self.player.document();
                    self.text.ensure_range(doc, start..end, window);
                }
                self.initial_shaped = true;
                self.metrics_dirty = true;
            }

            let (start, end) = {
                let n = self.player.document().len();
                let range = self.player.visible_range();
                if range.is_empty() && n > 0 {
                    let center = self.player.scroll_to_index();
                    (center.saturating_sub(4), (center + 20).min(n))
                } else {
                    (range.start, range.end)
                }
            };
            if start < end {
                let doc = self.player.document();
                self.text.ensure_range(doc, start..end, window);
            }
            self.sync_metrics(width, height);

            // 首帧/尺寸变化后可见范围会扩大，再整形一轮（最多两轮，避免抖动）。
            // 只有本帧真的新增了整形行（估算高度 → 精确高度）才需要收敛；
            // 稳态每帧重复 ensure_range + check_heights 是纯粹的 O(可见行) 浪费。
            if self.text.shaped_count() != shaped_before {
                for _ in 0..2 {
                    let range = self.player.visible_range();
                    if range.is_empty() {
                        break;
                    }
                    let doc = self.player.document();
                    self.text.ensure_range(doc, range.clone(), window);
                    if !self.check_heights(range) {
                        break;
                    }
                    self.sync_metrics(width, height);
                }
            }

            // 淡出层沿用被冻结前的排版缓存：正常情况下全部命中，只有切换
            // 期间恰好改了视口/样式才需要重新整形那几行。
            if let Some(out) = &mut self.outgoing {
                out.text.set_viewport(width, height);
                let range = out.player.visible_range();
                let doc = out.player.document();
                if range.is_empty() {
                    let n = doc.len();
                    if n > 0 {
                        let center = out.player.scroll_to_index().min(n);
                        out.text.ensure_range(
                            doc,
                            center.saturating_sub(4)..(center + 20).min(n),
                            window,
                        );
                    }
                } else {
                    out.text.ensure_range(doc, range, window);
                }
            }
        }

        // 先释放上一帧替换下来的背景纹理，避免图集（只增不减）被 30fps
        // 的背景帧撑爆，最终分配失败导致 `paint_image` 什么都不画（黑屏）。
        for image in std::mem::take(&mut self.retired_images) {
            self.retire_bg_frame(window, image);
        }
        if self.background_visible {
            // 背景网格按「绘制区域」生成（沉浸层为整个窗口），光栅分辨率由
            // render_scale 决定；屏幕上由 GPU 双线性放大。
            let bg = self.background_bounds.unwrap_or(bounds);
            self.background.set_viewport(
                f32::from(bg.size.width) as u32,
                f32::from(bg.size.height) as u32,
            );
            if let Some(source) = &self.beat_source {
                let level = source();
                self.background.set_beat_level(level);
            }
            // `None` 表示没有图层/无封面，必须同步清掉旧图，避免残留上一首封面。
            let next = self.background.render(self.dt_ms);
            let same = match (&self.bg_image, &next) {
                (Some(old), Some(new)) => old.id == new.id,
                _ => false,
            };
            if !same {
                if let Some(old) = self.bg_image.take() {
                    self.retire_bg_frame(window, old);
                }
                self.bg_image = next;
            }
        } else if let Some(old) = self.bg_image.take() {
            self.retire_bg_frame(window, old);
        }
    }

    /// 从图集移除一帧背景纹理，并尝试回收它的像素缓冲。
    ///
    /// `RenderImage` 的纹理必须由 `drop_image` 释放；释放后若引用计数归零，
    /// 就把 BGRA 像素交回光栅器复用。背景 30fps × 1–2 MiB 的每帧分配因此
    /// 变成零分配（只剩每帧 `fill(0)`）。
    fn retire_bg_frame(&mut self, window: &mut Window, image: Arc<RenderImage>) {
        let _ = window.drop_image(image.clone());
        if let Ok(image) = Arc::try_unwrap(image)
            && let Some(frame) = image.into_frames().into_iter().next()
        {
            self.background
                .recycle_output(frame.into_buffer().into_raw());
        }
    }

    /// 当前是否需要继续请求动画帧，以及允许的请求间隔。
    ///
    /// - 歌词/弹簧/扫光在动：`0`，即每个显示刷新周期都请求；
    /// - 只有背景网格在流动：按背景光栅帧率请求，避免为 30fps 的纹理做
    ///   300Hz 的无效重绘；
    /// - 全部静止：`None`，泵自动停止。
    fn animation_interval_ms(&self) -> Option<f32> {
        let has_lyrics = !self.player.document().is_empty();
        let lyric_animating =
            self.animating || self.switch.is_some() || (self.playing && has_lyrics);
        if lyric_animating {
            return Some(0.0);
        }
        let background_animating = self.background_visible && self.background.is_animating();
        if background_animating {
            return Some(1000.0 / self.bg_fps.max(1) as f32);
        }
        None
    }

    /// 按当前动画源限速判断本帧是否要请求下一帧。
    fn wants_animation_frame(&mut self, now: Instant) -> bool {
        let Some(interval_ms) = self.animation_interval_ms() else {
            return false;
        };
        let elapsed_ms = now
            .saturating_duration_since(self.last_frame_request)
            .as_secs_f32()
            * 1000.0;
        if interval_ms <= 0.5 || elapsed_ms >= interval_ms * 0.9 {
            self.last_frame_request = now;
            true
        } else {
            false
        }
    }
}

/// AMLL 歌词播放器的 gpui 视图。
pub struct LyricPlayerView {
    inner: Rc<RefCell<Inner>>,
}

impl LyricPlayerView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        // 帧调度由 `LyricCanvas` 在每帧绘制时调用 `request_animation_frame`
        // 完成：有歌词/弹簧动画时跟随显示器刷新率，仅背景流动时按 `bg_fps`
        // 限速，全部静止后自动停止。不再维护固定 16ms 定时泵。
        Self {
            inner: Rc::new(RefCell::new(Inner::new())),
        }
    }

    /// 安装一份新歌词。已有歌词时启动 600ms 交叉淡化：旧层淡出，新层淡入。
    pub fn set_lyrics(&mut self, lines: Vec<LyricLine>, initial_time_ms: u64) {
        let mut inner = self.inner.borrow_mut();
        if !lines.is_empty() {
            // 启动新切换：若上一次还没走完，更早的淡出层会在 begin_switch 里
            // 被释放，当前层接管为新的淡出层。连续切曲不会堆积多层。
            inner.begin_switch();
        } else {
            inner.finish_switch();
        }
        Self::install_lyrics(&mut inner, lines, initial_time_ms);
    }

    /// 清空歌词（无歌词/加载失败/停止）。不走交叉淡化：空内容没有可淡入的
    /// 目标，直接移除旧层，避免导航层误判 `has_lyrics()`。
    pub fn clear_lyrics(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.finish_switch();
        Self::install_lyrics(&mut inner, Vec::new(), 0);
    }

    fn install_lyrics(inner: &mut Inner, lines: Vec<LyricLine>, initial_time_ms: u64) {
        inner.time_ms = initial_time_ms;
        inner.player.set_lyrics(lines, initial_time_ms);
        let doc = inner.player.document().clone();
        inner.text.set_lyrics(&doc);
        inner.last_heights.clear();
        inner.metrics_dirty = true;
        inner.animating = true;
        inner.initial_shaped = false;
        inner.last_instant = Instant::now();
    }

    pub fn set_clock(&mut self, clock: Option<Arc<dyn PlaybackClock>>) {
        self.inner.borrow_mut().clock = clock;
    }

    /// 点击歌词行时的跳转回调（通常直接调用播放器的 seek）。
    pub fn set_seek_handler(&mut self, handler: Option<Arc<dyn Fn(u64) + Send + Sync>>) {
        self.inner.borrow_mut().seek_handler = handler;
    }

    /// 只绘制/隐藏歌词文本；背景与动画不受影响。
    pub fn set_text_visible(&mut self, visible: bool) {
        let mut inner = self.inner.borrow_mut();
        if inner.text_visible != visible {
            inner.text_visible = visible;
        }
    }

    /// 隐藏歌词文字时是否仍保持整形与布局。
    ///
    /// 沉浸页的歌词开关会反复切换可见性：保持测量后，`set_text_visible(true)`
    /// 只是开始绘制，不会触发一次初始布局或滚动位置重置。纯背景层
    /// （如手机播放页）保持关闭，避免对不可见文本做整形。
    pub fn set_layout_when_hidden(&mut self, keep: bool) {
        let mut inner = self.inner.borrow_mut();
        if inner.layout_when_hidden != keep {
            inner.layout_when_hidden = keep;
        }
    }

    /// 是否把点击解释为「跳到该行」。作为纯背景层时应传 `false`。
    pub fn set_click_seek_enabled(&mut self, enabled: bool) {
        let mut inner = self.inner.borrow_mut();
        if inner.click_seek_enabled != enabled {
            inner.click_seek_enabled = enabled;
        }
    }

    /// 覆盖歌词容器底部的 `mask-image` 渐隐比例（相对容器高度）。
    ///
    /// `-1.0` 表示沿用 `LyricTextStyle::edge_fade_bottom`。这是绘制期的
    /// alpha 遮罩，不进入 `TextLayoutCache`：显隐动画期间逐帧改变它也不会
    /// 重新整形、重排或改字号，只让字形按容器底部渐变淡出。
    pub fn set_bottom_mask(&mut self, fraction: f32) {
        let value = if fraction < 0.0 {
            -1.0
        } else {
            fraction.clamp(0.0, 1.0)
        };
        let mut inner = self.inner.borrow_mut();
        inner.bottom_mask = value;
    }

    /// 鼓点包络来源（0..1）。视图每帧读取一次，驱动 Mesh 背景的跳动；
    /// 传 `None` 则背景保持静态流动。
    pub fn set_beat_source(&mut self, source: Option<Arc<dyn Fn() -> f32 + Send + Sync>>) {
        self.inner.borrow_mut().beat_source = source;
    }

    fn handle_click(&self, position: gpui::Point<Pixels>) {
        // 只在本作用域内持有借用；回调可能更新本视图（BorrowMutError 风险）。
        let (start, handler) = {
            let inner = self.inner.borrow();
            if !inner.click_seek_enabled {
                return;
            }
            if !inner.last_bounds.contains(&position) {
                return;
            }
            let local_y = f32::from(position.y - inner.last_bounds.top());
            match inner.player.line_time_at_y(local_y) {
                Some(start) => (start, inner.seek_handler.clone()),
                None => return,
            }
        };
        if let Some(handler) = handler {
            handler(start);
        }
    }

    pub fn set_time_ms(&mut self, time_ms: u64) {
        let mut inner = self.inner.borrow_mut();
        inner.time_ms = time_ms;
        inner.last_clock_pos = time_ms;
        inner.display_time_ms = time_ms as f64;
    }

    pub fn set_playing(&mut self, playing: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.playing = playing;
        inner.player.set_playing(playing);
    }

    pub fn set_player_config(&mut self, config: LyricPlayerConfig) {
        self.inner.borrow_mut().player.set_config(config);
    }

    pub fn set_text_style(&mut self, style: LyricTextStyle) {
        let mut inner = self.inner.borrow_mut();
        // 沉浸层每帧都会按窗口尺寸重算样式，这里必须按完整样式去重：
        // 只比较字号会漏掉字重/行高变化（例如面板 MEDIUM 44 与沉浸层
        // SEMIBOLD ≈44 的首帧），导致歌词沿用旧的整形缓存。
        if inner.text.style() == &style {
            return;
        }
        let w = inner.last_width;
        let h = inner.last_height;
        inner.text.set_style(style.clone(), w, h);
        // 淡出层也要同步样式：沉浸页每帧重算样式，切换途中若从沉浸层退到
        // 手机壳，旧层不能继续用上一套字号/换行。
        if let Some(out) = &mut inner.outgoing {
            out.text.set_style(style, w, h);
        }
        let base = inner.text.style().base_font_size;
        let mut config = *inner.player.config();
        config.base_font_size = base;
        inner.player.set_config(config);
        inner.last_heights.clear();
        inner.metrics_dirty = true;
    }

    pub fn set_colors(&mut self, colors: LyricRenderColors) {
        self.inner.borrow_mut().colors = colors;
    }

    pub fn set_background_kind(&mut self, kind: BackgroundRendererKind) {
        let mut inner = self.inner.borrow_mut();
        let (fps, scale, flow, static_mode) = (
            inner.bg_fps,
            inner.bg_scale,
            inner.bg_flow_speed,
            inner.bg_static,
        );
        let beat_strength = inner.bg_beat_strength;
        let old_last = inner.background.take_last_image();
        inner.background = LyricBackgroundRenderer::new(kind);
        inner.background.set_fps(fps);
        inner.background.set_render_scale(scale);
        inner.background.set_flow_speed(flow);
        inner.background.set_static_mode(static_mode);
        inner.background.set_beat_strength(beat_strength);
        if let Some(palette) = inner.album_palette.clone() {
            inner.background.set_album_palette(palette);
        }
        // 旧渲染器的缓存帧也要回收，否则每切换一次背景种类就泄漏一张纹理。
        if let Some(old) = old_last {
            inner.retired_images.push(old);
        }
        if let Some(old) = inner.bg_image.take() {
            inner.retired_images.push(old);
        }
    }

    pub fn set_background_visible(&mut self, visible: bool) {
        self.inner.borrow_mut().background_visible = visible;
    }

    /// 设置背景的绘制/光栅区域（`None` = 用视图自身 bounds）。
    ///
    /// 沉浸层调用方把该值设为整个窗口 bounds：背景网格按全窗宽高比生成，
    /// 光栅分辨率仍由 [`Self::set_background_render_scale`] 控制（低分辨率），
    /// 最终由 GPU 双线性放大到全窗 —— 每个屏幕像素的采样都在 GPU 上完成。
    pub fn set_background_bounds(&mut self, bounds: Option<Bounds<Pixels>>) {
        self.inner.borrow_mut().background_bounds = bounds;
    }

    pub fn set_background_color(&mut self, color: Hsla) {
        let mut inner = self.inner.borrow_mut();
        inner.background.set_solid_color(color);
        inner.colors.fallback_background = color;
    }

    pub fn set_background_fps(&mut self, fps: u32) {
        let mut inner = self.inner.borrow_mut();
        inner.bg_fps = fps;
        inner.background.set_fps(fps);
    }

    pub fn set_background_render_scale(&mut self, scale: f32) {
        let mut inner = self.inner.borrow_mut();
        inner.bg_scale = scale;
        inner.background.set_render_scale(scale);
    }

    pub fn set_background_flow_speed(&mut self, speed: f32) {
        let mut inner = self.inner.borrow_mut();
        inner.bg_flow_speed = speed;
        inner.background.set_flow_speed(speed);
    }

    pub fn set_background_static_mode(&mut self, enable: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.bg_static = enable;
        inner.background.set_static_mode(enable);
    }

    pub fn set_background_low_freq_volume(&mut self, volume: f32) {
        self.inner
            .borrow_mut()
            .background
            .set_low_freq_volume(volume);
    }

    /// 鼓点视觉效果倍率（0..=1.5）：0 关闭呼吸/旋转/提亮，1 为原始幅度。
    pub fn set_background_beat_strength(&mut self, strength: f32) {
        let mut inner = self.inner.borrow_mut();
        let strength = strength.clamp(0.0, 1.5);
        inner.bg_beat_strength = strength;
        inner.background.set_beat_strength(strength);
    }

    /// 设置专辑封面（RGBA8，非预乘）。解码由调用方完成，避免在 UI 线程阻塞。
    ///
    /// 视图只保存 32×32 调色纹理（4 KiB），整幅 RGBA 在烘焙后立即释放。
    pub fn set_album_rgba(&mut self, width: u32, height: u32, rgba: Vec<u8>) {
        let Some(palette) = crate::background::album_palette_bytes(width, height, &rgba) else {
            return;
        };
        let mut inner = self.inner.borrow_mut();
        inner.album_palette = Some(palette.clone());
        inner.background.set_album_palette(palette);
        // 旧帧保留到下一帧 `prepare`：新帧渲染出来后在那里统一从图集释放。
    }

    pub fn clear_album(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.album_palette = None;
        inner.background.clear_album();
        // 停止/无封面时背景淡出到透明，由 `prepare` 回收旧帧。
    }

    pub fn background_kind(&self) -> BackgroundRendererKind {
        self.inner.borrow().background.kind()
    }

    pub fn has_lyrics(&self) -> bool {
        !self.inner.borrow().player.document().is_empty()
    }

    /// 只读判断：外部轮询不能推进动画状态（推进只在每帧 `Render` 中发生）。
    pub fn is_animating(&self) -> bool {
        let inner = self.inner.borrow();
        inner.animating
            || inner.switch.is_some()
            || (inner.playing && !inner.player.document().is_empty())
            || (inner.background_visible && inner.background.is_animating())
    }
}

impl Render for LyricPlayerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (has_lyrics, text_visible, fallback_text_color) = {
            let mut inner = self.inner.borrow_mut();
            let now = Instant::now();
            let mut dt = (now - inner.last_instant).as_secs_f32() * 1000.0;
            inner.last_instant = now;
            if !dt.is_finite() {
                dt = 16.0;
            }
            inner.dt_ms = dt.clamp(0.0, 100.0);

            if let Some(clock) = inner.clock.clone() {
                let (pos, playing) = clock.snapshot();
                // 状态快照 33ms 更新一次；在两次快照之间外推，最多 80ms，
                // 让扫光保持帧率级平滑。seek/回退时立即重置。
                if pos + 500 < inner.display_time_ms as u64 || pos < inner.last_clock_pos {
                    inner.display_time_ms = pos as f64;
                } else if pos != inner.last_clock_pos {
                    inner.display_time_ms = inner.display_time_ms.max(pos as f64);
                } else if playing {
                    inner.display_time_ms =
                        (inner.display_time_ms + inner.dt_ms as f64).min(pos as f64 + 80.0);
                }
                if !playing {
                    inner.display_time_ms = pos as f64;
                }
                inner.last_clock_pos = pos;
                let time = inner.display_time_ms.round() as u64;
                if playing != inner.playing {
                    inner.playing = playing;
                    inner.player.set_playing(playing);
                    if playing {
                        inner.background.resume();
                    } else {
                        inner.background.pause();
                    }
                }
                if time != inner.player.current_time() {
                    let seek = time.abs_diff(inner.player.current_time()) > 1_500;
                    inner.player.set_time(time, seek);
                }
                inner.time_ms = time;
            }
            let time = inner.time_ms;
            if time != inner.player.current_time() {
                let seek = time.abs_diff(inner.player.current_time()) > 1_500;
                inner.player.set_time(time, seek);
            }
            let playing = inner.playing;
            let dt = inner.dt_ms;
            // 切换动画只依赖帧间隔推进；暂停/后台返回时 dt 已被钳到 100ms，
            // 600ms  lifecycle 不会被一帧跳过。到点立即释放淡出层与其整形缓存。
            let switching = inner
                .switch
                .as_mut()
                .is_some_and(|switch| switch.advance(dt));
            if !switching {
                inner.finish_switch();
            }
            inner.animating = inner.player.update(dt);
            // 暂停时也让弹簧收敛（AMLL 暂停后仍会完成当前动画）。
            let bg_anim = inner.background_visible && inner.background.is_animating();
            let has_lyrics = !inner.player.document().is_empty();
            // `animating` 只用于帧请求判断（`is_animating` 读取），
            // 这里不再请求 `request_animation_frame`。
            let _ = inner.animating || bg_anim || (playing && has_lyrics);
            (has_lyrics, inner.text_visible, inner.colors.text)
        };

        div()
            .relative()
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| this.handle_click(event.position)),
            )
            .child(LyricCanvas {
                inner: self.inner.clone(),
            })
            .when(!has_lyrics && text_visible, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(fallback_text_color.opacity(0.55))
                        .text_size(px(18.0))
                        .child("暂无歌词"),
                )
            })
    }
}

struct LyricCanvas {
    inner: Rc<RefCell<Inner>>,
}

impl IntoElement for LyricCanvas {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for LyricCanvas {
    type RequestLayoutState = ();
    /// 本帧绘制结束后是否继续请求下一帧。
    type PrepaintState = bool;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        let mut inner = self.inner.borrow_mut();
        inner.last_bounds = bounds;
        inner.prepare(bounds, window);
        inner.wants_animation_frame(Instant::now())
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        self.paint_content(bounds, window);
        if *prepaint {
            window.request_animation_frame();
        }
    }
}

impl LyricCanvas {
    fn paint_content(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let inner = self.inner.borrow();
        if inner.background_visible {
            // 背景区域：默认取视图 bounds；沉浸层由调用方设为整个窗口。
            let bg_bounds = inner.background_bounds.unwrap_or(bounds);
            // 底色：无封面 / CSS 背景时至少有一层纯色。
            let base = inner
                .background
                .solid_color()
                .unwrap_or(inner.colors.fallback_background);
            window.paint_quad(fill(bg_bounds, base));

            if let Some(image) = &inner.bg_image {
                // 低分辨率光栅 → GPU 双线性放大到全窗。
                //
                // 图集里的图块没有 padding，采样器又是 WRAP：双线性在贴图边缘的
                // 半个纹素会采到相邻图块/未初始化纹素（透明黑），再叠上 sprite
                // 自身的边缘 AA，就会在整窗四周留下约一个源纹素宽的黑边；网格
                // 动画时边缘底色变化，看起来就是黑边闪烁。把绘制区域向外扩一个
                // 源纹素，让窗口内的采样点始终落在图块内部（外扩部分会被祖先的
                // overflow_hidden 裁掉）。
                let src = image.size(0);
                let pad_x = bg_bounds.size.width / (src.width.0.max(1) as f32);
                let pad_y = bg_bounds.size.height / (src.height.0.max(1) as f32);
                let image_bounds = Bounds::new(
                    point(bg_bounds.left() - pad_x, bg_bounds.top() - pad_y),
                    gpui::size(
                        bg_bounds.size.width + pad_x * 2.0,
                        bg_bounds.size.height + pad_y * 2.0,
                    ),
                );
                // 用内容遮罩把外扩部分裁回背景区域，避免依赖祖先的裁剪。
                let mask = ContentMask { bounds: bg_bounds };
                window.with_content_mask(Some(mask), |window| {
                    let _ = window.paint_image(
                        image_bounds,
                        Corners::default(),
                        image.clone(),
                        0,
                        false,
                    );
                });
                // AMLL `.background::after`：全高 `transparent 60% → rgba(0,0,0,0.102)`。
                // gpui 只有双停靠点，用底部 40% 的条带承载同一段线性渐变。
                let strip_h = bg_bounds.size.height * 0.4;
                let strip = Bounds::new(
                    point(bg_bounds.left(), bg_bounds.bottom() - strip_h),
                    gpui::size(bg_bounds.size.width, strip_h),
                );
                window.paint_quad(fill(
                    strip,
                    gpui::linear_gradient(
                        180.0,
                        gpui::linear_color_stop(gpui::black().opacity(0.0), 0.0),
                        gpui::linear_color_stop(gpui::black().opacity(0.102), 1.0),
                    ),
                ));
                window.paint_quad(fill(bg_bounds, inner.colors.overlay));
            }
        }

        if inner.player.document().is_empty() || !inner.text_visible {
            return;
        }
        let style = inner.text.style();
        let colors = inner.colors;
        // 交叉淡化时先画旧层再画新层，新层压在上面；两层共用同一块 content
        // mask（竖向边缘渐隐），背景只由当前层绘制一次，不参与淡化。
        let (incoming, outgoing) = match inner.switch {
            Some(switch) => (
                switch.incoming_fade(),
                inner
                    .outgoing
                    .as_ref()
                    .map(|out| (out, switch.outgoing_fade())),
            ),
            None => (LyricFade::NORMAL, None),
        };
        let mask = ContentMask { bounds };

        window.with_content_mask(Some(mask), |window| {
            let scope = PerfScope::enter("amll.render.pass");
            let mut glyphs = 0u64;
            if let Some((out, fade)) = outgoing {
                glyphs += paint_lyric_layer(
                    window,
                    bounds,
                    &out.player,
                    &out.text,
                    out.player.current_time(),
                    fade,
                    colors,
                    style,
                    inner.bottom_mask,
                );
            }
            glyphs += paint_lyric_layer(
                window,
                bounds,
                &inner.player,
                &inner.text,
                inner.time_ms,
                incoming,
                colors,
                style,
                inner.bottom_mask,
            );
            scope.finish(glyphs);
        });
    }
}

/// 绘制一层歌词（当前层或淡出层）。切换参数 `fade` 只影响字形，不影响行状态。
fn paint_lyric_layer(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    player: &LyricPlayer,
    text: &TextLayoutCache,
    time: u64,
    fade: LyricFade,
    colors: LyricRenderColors,
    style: &LyricTextStyle,
    bottom_mask: f32,
) -> u64 {
    let doc = player.document();
    let frame = player.frame();
    let mut glyphs = 0u64;
    for i in player.visible_range() {
        let Some(shape) = text.get(i) else {
            continue;
        };
        let Some(line_frame) = frame.lines.get(i).copied() else {
            continue;
        };
        if !line_frame.visible || line_frame.opacity * fade.opacity < 0.0001 {
            continue;
        }
        let Some(line) = doc.lines.get(i) else {
            continue;
        };
        glyphs += paint_lyric_line(
            window,
            bounds,
            line,
            &shape,
            line_frame,
            style,
            colors,
            time,
            doc.is_dynamic,
            fade,
            bottom_mask,
        );
    }
    if let Some(interlude) = frame.interlude {
        paint_interlude_dots(window, bounds, interlude.dots, colors.interlude_dot, fade);
    }
    glyphs
}

/// 行级缩放的仿射变换：`p' = pivot + (p - pivot) * scale`。
///
/// 对应 AMLL 的 `.lyricLine { transform-origin: left }`（对唱行 `right`）：
/// 水平锚点在线框左/右边缘（文本内容边缘再向外 0.2em，来自 `.lyricLine` 的
/// `padding: 0.2em; margin: -0.2em`），垂直锚点在线框中心；`translateY` 由外层
/// `.lyricLineWrapper` 承担，不参与缩放。
#[derive(Clone, Copy)]
struct LineXform {
    scale: f32,
    pivot_x: Pixels,
    pivot_y: Pixels,
    /// 歌词替换动画的额外内容缩放，围绕歌词容器中心施加（在行缩放之后）。
    /// 与 `scale` 分开保存，避免把切换动画混进行弹簧的亮暗判定。
    content_scale: f32,
    content_cx: Pixels,
    content_cy: Pixels,
}

impl LineXform {
    /// 不带切换动画的普通构造（单测使用）。
    #[cfg(test)]
    fn plain(scale: f32, pivot_x: Pixels, pivot_y: Pixels) -> Self {
        Self {
            scale,
            pivot_x,
            pivot_y,
            content_scale: 1.0,
            content_cx: px(0.0),
            content_cy: px(0.0),
        }
    }

    fn x(&self, x: Pixels) -> Pixels {
        let line_scaled = self.pivot_x + (x - self.pivot_x) * self.scale;
        self.content_cx + (line_scaled - self.content_cx) * self.content_scale
    }

    fn y(&self, y: Pixels) -> Pixels {
        let line_scaled = self.pivot_y + (y - self.pivot_y) * self.scale;
        self.content_cy + (line_scaled - self.content_cy) * self.content_scale
    }
}

/// 供测试/调试：当前视图使用的歌词行数。
impl LyricPlayerView {
    #[doc(hidden)]
    pub fn debug_line_count(&self) -> usize {
        self.inner.borrow().player.document().len()
    }

    /// 供探针/调试：把切换进度钉在指定 t（截图核对用，每帧调用即可冻结画面）。
    #[doc(hidden)]
    pub fn debug_set_switch_t(&self, t: f32) {
        let mut inner = self.inner.borrow_mut();
        if let Some(switch) = inner.switch.as_mut() {
            switch.elapsed_ms = t.clamp(0.0, 1.0) * LYRIC_SWITCH_MS;
        }
    }

}

#[cfg(test)]
mod tests;
