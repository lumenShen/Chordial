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
    album: Option<(u32, u32, Vec<u8>)>,
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
    /// 是否响应「点击歌词行跳转」。背景层必须关闭，避免误 seek。
    click_seek_enabled: bool,
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
            album: None,
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
            click_seek_enabled: true,
            background_bounds: None,
            last_bounds: Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(0.0), px(0.0))),
            seek_handler: None,
            beat_source: None,
            retired_images: Vec::new(),
            last_clock_pos: 0,
            display_time_ms: 0.0,
            initial_shaped: false,
            last_frame_request: Instant::now() - Duration::from_secs(1),
        }
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

    fn sync_metrics(&mut self, width: f32, height: f32) {
        if !self.metrics_dirty
            && (width - self.last_width).abs() < 0.01
            && (height - self.last_height).abs() < 0.01
        {
            return;
        }
        self.last_width = width;
        self.last_height = height;
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

        if (width - self.last_width).abs() > 0.01 || (height - self.last_height).abs() > 0.01 {
            self.text.set_viewport(width, height);
            self.last_heights.clear();
            self.metrics_dirty = true;
        }

        // 竖屏播放页把歌词视图当纯背景使用：不整形、不测量，避免每帧
        // 对不可见文本做无用工作。
        if self.text_visible {
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

        // 先释放上一帧替换下来的背景纹理，避免图集（只增不减）被 30fps
        // 的背景帧撑爆，最终分配失败导致 `paint_image` 什么都不画（黑屏）。
        for image in std::mem::take(&mut self.retired_images) {
            let _ = window.drop_image(image);
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
                    let _ = window.drop_image(old);
                }
                self.bg_image = next;
            }
        } else if let Some(old) = self.bg_image.take() {
            let _ = window.drop_image(old);
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
        let lyric_animating = self.animating || (self.playing && has_lyrics);
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

    pub fn set_lyrics(&mut self, lines: Vec<LyricLine>, initial_time_ms: u64) {
        let mut inner = self.inner.borrow_mut();
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

    pub fn clear_lyrics(&mut self) {
        self.set_lyrics(Vec::new(), 0);
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

    /// 是否把点击解释为「跳到该行」。作为纯背景层时应传 `false`。
    pub fn set_click_seek_enabled(&mut self, enabled: bool) {
        let mut inner = self.inner.borrow_mut();
        if inner.click_seek_enabled != enabled {
            inner.click_seek_enabled = enabled;
        }
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
        inner.text.set_style(style, w, h);
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
        if let Some((w, h, data)) = inner.album.clone() {
            inner.background.set_album_rgba(w, h, data);
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
    pub fn set_album_rgba(&mut self, width: u32, height: u32, rgba: Vec<u8>) {
        let mut inner = self.inner.borrow_mut();
        inner.album = Some((width, height, rgba.clone()));
        inner.background.set_album_rgba(width, height, rgba);
        // 旧帧保留到下一帧 `prepare`：新帧渲染出来后在那里统一从图集释放。
    }

    pub fn clear_album(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.album = None;
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

        let doc = inner.player.document();
        if doc.is_empty() || !inner.text_visible {
            return;
        }
        let frame = inner.player.frame();
        let range = inner.player.visible_range();
        let time = inner.time_ms;
        let style = inner.text.style().clone();
        let mask = ContentMask { bounds };

        window.with_content_mask(Some(mask), |window| {
            let scope = PerfScope::enter("amll.render.pass");
            let mut glyphs = 0u64;
            for i in range {
                let Some(shape) = inner.text.get(i) else {
                    continue;
                };
                let Some(line_frame) = frame.lines.get(i).copied() else {
                    continue;
                };
                if !line_frame.visible || line_frame.opacity < 0.0001 {
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
                    &style,
                    inner.colors,
                    time,
                    doc.is_dynamic,
                );
            }

            if let Some(interlude) = frame.interlude {
                paint_interlude_dots(window, bounds, interlude.dots, inner.colors.interlude_dot);
            }
            scope.finish(glyphs);
        });
    }
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
}

impl LineXform {
    fn x(&self, x: Pixels) -> Pixels {
        self.pivot_x + (x - self.pivot_x) * self.scale
    }

    fn y(&self, y: Pixels) -> Pixels {
        self.pivot_y + (y - self.pivot_y) * self.scale
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_lyric_line(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    line: &LyricLine,
    shape: &LineShape,
    line_frame: LineFrameState,
    style: &LyricTextStyle,
    colors: LyricRenderColors,
    time: u64,
    is_dynamic: bool,
) -> u64 {
    let scale = (line_frame.scale / 100.0).max(0.01);
    let line_top = bounds.top() + px(line_frame.top);
    let text_left = bounds.left() + shape.padding_x;
    let text_right = bounds.right() - shape.padding_x;
    // `.lyricLine` 的 `padding: 0.2em; margin: -0.2em` 把缩放盒向外扩 0.2em，
    // `transform-origin: left/right` 因此落在文本内容边缘外 0.2em。em 使用
    // 该行自己的字号（背景行是 0.7em）。
    let origin_inset = px(f32::from(shape.main_font_size) * 0.2);
    let pivot_x = if line.is_duet {
        text_right + origin_inset
    } else {
        text_left - origin_inset
    };
    let pivot_y = line_top + shape.height * 0.5;
    let xform = LineXform {
        scale,
        pivot_x,
        pivot_y,
    };
    let mut y = line_top + shape.padding_top;
    let mut painted = 0u64;

    // 与 AMLL 的 --bright/dark-mask-alpha 一致：由缩放弹簧驱动。
    let k = ((line_frame.scale / 100.0 - 0.97) / 0.03).clamp(0.0, 1.0);
    let bright = k * 0.8 + 0.2;
    let dark = k * 0.2 + 0.2;
    // 整行扫光的渐变带宽度。亮暗不再由每个词自己的窄边决定，而是一条
    // 覆盖整行的 mask 前沿；软边取 1.5em 左右，才能同时压住相邻的字/词，
    // 让渐变看起来是在整行上扫过，而不是缩在单个字形里。
    let fade_em = style.sweep_fade_em.max(0.0001);
    let line_enabled = line_frame.enabled;
    // AMLL `.lyricBgLine`：背景行 wrapper 透明度 0.4。
    let wrapper_alpha = if line.is_bg { 0.4 } else { 1.0 };
    let opacity = line_frame.opacity * wrapper_alpha;
    let blur = line_frame.blur;

    // AMLL 歌词容器的 CSS mask：上下各按容器高度比例渐隐（竖向布局底部为 0）。
    let container_h = f32::from(bounds.size.height);
    let fade_top = container_h * style.edge_fade_top.max(0.0);
    let fade_bottom = container_h * style.edge_fade_bottom.max(0.0);
    let edge_for = |row_top: Pixels, row_h: f32| -> f32 {
        // 渐隐按字形在屏幕上的实际中心计算（行缩放围绕行框中心）。
        let center = f32::from(xform.y(row_top + px(row_h * 0.5)) - bounds.top());
        let mut a = 1.0f32;
        if fade_top > 0.5 {
            a *= (center / fade_top).clamp(0.0, 1.0);
        }
        if fade_bottom > 0.5 {
            a *= ((container_h - center) / fade_bottom).clamp(0.0, 1.0);
        }
        a
    };

    for row in &shape.main {
        // AMLL 默认 align-items: flex-start；对唱行切到 flex-end。
        let row_origin_x = if line.is_duet {
            text_right - row.width
        } else {
            text_left
        };
        let edge = edge_for(y, f32::from(row.height));
        // 渐隐区/极低透明度行整行跳过：省掉全部字形绘制。
        if opacity * edge >= 0.004 {
            painted += paint_row(
                window,
                row,
                row_origin_x,
                y,
                xform,
                row.line.font_size,
                line,
                time,
                is_dynamic,
                bright,
                dark,
                f32::from(row.line.font_size) * fade_em,
                opacity,
                blur,
                edge,
                colors.text,
                colors.glow,
                &shape.emphasis_chunks,
                &shape.word_chunks,
                line_enabled,
            );
        }
        y += row.height;
    }

    for row in &shape.translation {
        // AMLL 默认 align-items: flex-start；对唱行切到 flex-end。
        let row_origin_x = if line.is_duet {
            text_right - row.width
        } else {
            text_left
        };
        let edge = edge_for(y, f32::from(row.height));
        if opacity * edge >= 0.004 {
            painted += paint_row_plain(
                window,
                row,
                row_origin_x,
                y,
                xform,
                row.line.font_size,
                colors.translation.opacity(0.3),
                opacity,
                blur,
                edge,
            );
        }
        y += row.height;
    }
    for row in &shape.roman {
        // AMLL 默认 align-items: flex-start；对唱行切到 flex-end。
        let row_origin_x = if line.is_duet {
            text_right - row.width
        } else {
            text_left
        };
        let edge = edge_for(y, f32::from(row.height));
        if opacity * edge >= 0.004 {
            painted += paint_row_plain(
                window,
                row,
                row_origin_x,
                y,
                xform,
                row.line.font_size,
                colors.translation.opacity(0.3),
                opacity,
                blur,
                edge,
            );
        }
        y += row.height;
    }

    painted
}

#[allow(clippy::too_many_arguments)]
fn paint_row(
    window: &mut Window,
    row: &ShapedRow,
    origin_x: Pixels,
    row_top: Pixels,
    xform: LineXform,
    font_size: Pixels,
    line: &LyricLine,
    time: u64,
    is_dynamic: bool,
    bright: f32,
    dark: f32,
    fade: f32,
    opacity: f32,
    blur: f32,
    edge: f32,
    base_color: Hsla,
    glow_color: Hsla,
    chunks: &[EmphasisChunk],
    word_chunks: &[Option<usize>],
    line_enabled: bool,
) -> u64 {
    let line_height = f32::from(row.height);
    let ascent = f32::from(row.line.ascent);
    let descent = f32::from(row.line.descent);
    // 行内基线（未缩放坐标）；行级缩放统一由 `xform` 施加。
    let baseline = row_top + px((line_height - ascent - descent) / 2.0 + ascent);
    // AMLL 的强调动画作用在字符 `<span>` 上，未设置 transform-origin，
    // 因此锚点是字符盒（宽 = advance，高 = line-height）的中心。
    let center_y = row_top + row.height * 0.5;
    let mut painted = 0u64;
    let mut word_cursor = 0usize;
    let text = row.line.text.clone();
    let font_px = f32::from(font_size);

    // AMLL `initFloatAnimation`：每个词在自己的时长内上浮 0.05em（背景行 0.1em），
    // 行离场时以 playbackRate = -1 回退。先按词缓存，避免逐字形重复计算。
    // 非逐词歌词（isNonDynamic）在 AMLL 里直接渲染整行文本，不创建 word span，
    // 因此也不会有浮动动画。
    let float_offsets: Vec<f32> = if is_dynamic {
        row.words
            .iter()
            .map(|span| {
                line.words
                    .get(span.word_index)
                    .map(|word| word_float_offset(word, line, time, line_enabled, font_px))
                    .unwrap_or(0.0)
            })
            .collect()
    } else {
        vec![0.0; row.words.len()]
    };
    let mut last_span: Option<(usize, usize)> = None;
    let mut next_char_ordinal = 0usize;

    // 整行一条扫光：所有字形共享同一个前沿，渐变带才能跨字、跨词连续推进。
    // 背景行按 AMLL 走均匀 0.4 alpha，不参与扫光。
    let sweep = if is_dynamic && !line.is_bg {
        row_sweep_bounds(&row.words, line, time, fade)
    } else {
        None
    };

    for (run_index, run) in row.line.runs.iter().enumerate() {
        for (glyph_index, glyph) in run.glyphs.iter().enumerate() {
            while word_cursor + 1 < row.words.len()
                && glyph.index >= row.words[word_cursor].byte_range.end
            {
                word_cursor += 1;
            }
            let span = row
                .words
                .get(word_cursor)
                .filter(|w| w.byte_range.contains(&glyph.index));

            // 强调动画的 stagger 序号按“词内非空白字符”连续计数；同一个词跨
            // 视觉行时由 `WordSpan::char_offset` 接续。
            let mut char_ordinal = None;
            if let Some(span) = span {
                let key = (span.word_index, span.byte_range.start);
                if last_span != Some(key) {
                    last_span = Some(key);
                    next_char_ordinal = span.char_offset;
                }
                char_ordinal = Some(next_char_ordinal);
                if text
                    .get(glyph.index..)
                    .and_then(|s| s.chars().next())
                    .is_some_and(|c| !c.is_whitespace())
                {
                    next_char_ordinal += 1;
                }
            }

            // 强调词使用 char span 自己的 float 动画（含 stagger 和 sin 曲线），
            // 不再叠加 word 级别的 float；非强调词才走 word float。
            let chunk = span.and_then(|span| {
                word_chunks
                    .get(span.word_index)
                    .copied()
                    .flatten()
                    .and_then(|index| chunks.get(index))
            });
            let emphasis_word = chunk.is_some_and(|chunk| chunk.emphasized);
            let float_y = if span.is_some() && !emphasis_word {
                float_offsets.get(word_cursor).copied().unwrap_or(0.0)
            } else {
                0.0
            };

            let glyph_x = origin_x + glyph.position.x;
            let next_x = run
                .glyphs
                .get(glyph_index + 1)
                .map(|g| g.position.x)
                .or_else(|| {
                    row.line
                        .runs
                        .get(run_index + 1)?
                        .glyphs
                        .first()
                        .map(|g| g.position.x)
                });
            let advance = next_x.map_or(row.width - glyph.position.x, |x| x - glyph.position.x);
            let advance = if advance > px(0.01) {
                advance
            } else {
                font_size * 0.5
            };
            let center_x = glyph_x + advance * 0.5;

            let mut glyph_scale = xform.scale;
            let mut off_x = px(0.0);
            let mut off_y = px(float_y);
            let mut ha = 1.0f32;
            // AMLL 的 `mask-image` 扫光：一条带软边界的水平渐变作为**整行**
            // 的 mask 层。`row_sweep_bounds` 每行只算一次，所有字形共用同一
            // 组边界，因此边界可以落在两个字/词之间；alpha 仍由 shader 按
            // fragment x 插值，同一个字形内部也是连续渐变。
            let mut mask = SpriteMask::NONE;
            if let Some((light_x, dark_x)) = sweep {
                mask = SpriteMask::new(
                    f32::from(xform.x(origin_x + px(light_x))),
                    f32::from(xform.x(origin_x + px(dark_x))),
                    bright,
                    dark,
                );
            } else if is_dynamic {
                // 动态行但本视觉行没有可用的词时间轴（例如纯空白行）：
                // 保持 AMLL 的整块暗层。
                ha = dark;
            }
            let mut glow = 0.0f32;
            let mut glow_sigma_em = 0.0f32;

            // 强调词：按强调分组的合并时序逐字 scale/translate/glow。
            if let (Some(ordinal), Some(chunk)) = (char_ordinal, chunk)
                && let Some(frame) =
                    emphasize_glyph(chunk, ordinal, line, time, line_enabled, font_px)
            {
                glyph_scale = xform.scale * frame.scale;
                off_x = px(frame.offset_x);
                off_y = px(frame.offset_y + frame.float_y);
                glow = frame.glow;
                glow_sigma_em = frame.glow_sigma_em;
            }

            // 背景人声行：AMLL SOLID 模式无逐词遮罩，统一 0.4 alpha
            //（乘上 wrapper 的 0.4 之前；wrapper 已在 opacity 里）。
            if line.is_bg {
                ha = 0.4;
                mask = SpriteMask::NONE;
            }

            let color = Hsla {
                a: base_color.a * (ha * opacity * edge).clamp(0.0, 1.0),
                ..base_color
            };
            // 先围绕字符盒中心做强调缩放，再叠加行级仿射变换：
            //   final(q) = L(C) + (line_scale * emp_scale) * (q + offset - C)
            // 与 AMLL 的「字符 span scale(默认中心 origin) → 行元素 scale(left/right center)」
            // 组合结果一致。字形始终用基准字号光栅化，缩放由 sprite 变换承担。
            let origin = transformed_glyph_origin(
                xform,
                glyph_x,
                baseline,
                center_x,
                center_y,
                glyph_scale,
                point(off_x, off_y),
            );
            let gx = origin.x;
            let gy = origin.y;

            if glow > 0.01 {
                // AMLL 强调词用 CSS `text-shadow: 0 0 R rgba(...)`；R 是模糊
                // 半径，浏览器按高斯 σ = R/2 实现。这里直接绘制一份真模糊的
                // 字形（vendor/gpui 在光栅化阶段卷积 alpha 掩码），与行级滤镜
                // 模糊在方差域叠加；比“稀疏复制 9 次”更接近真实辉光。
                let shadow_sigma = (font_px * glow_sigma_em * 0.5).max(0.0);
                let sigma = (blur * blur + shadow_sigma * shadow_sigma).sqrt();
                // mask 作用于整个词元素（含 text-shadow），所以辉光也要乘上
                // 当前字形的亮/暗层 alpha。
                let shadow_color = Hsla {
                    a: glow_color.a * (glow * ha * opacity * edge).clamp(0.0, 1.0),
                    ..glow_color
                };
                painted += paint_glyph_blurred(
                    window,
                    point(gx, gy),
                    run.font_id,
                    glyph.id,
                    glyph.is_emoji,
                    font_size,
                    glyph_scale,
                    shadow_color,
                    sigma,
                    mask,
                );
            }

            painted += paint_glyph_blurred(
                window,
                point(gx, gy),
                run.font_id,
                glyph.id,
                glyph.is_emoji,
                font_size,
                glyph_scale,
                color,
                blur,
                mask,
            );
        }
    }
    painted
}

/// AMLL `initFloatAnimation` 的等价相位。
///
/// - 正放：`ease-out` 在 `max(1s, 词时长)` 内把词上移 0.05em（背景行 0.1em）；
/// - 行离开热行集合时，AMLL 把动画 `playbackRate` 设为 -1 回退。这里用
///   “离场时间 / 词时长”还原同一轨迹，并在 0 处截断。
fn word_float_offset(
    word: &LyricWord,
    line: &LyricLine,
    time: u64,
    enabled: bool,
    font_size: f32,
) -> f32 {
    let duration = word.duration_ms().max(1000) as f32;
    let phase = if time < line.start_time {
        0.0
    } else if enabled {
        ((time.saturating_sub(word.start_time)) as f32 / duration).clamp(0.0, 1.0)
    } else {
        // 刚离场时从“行结束时刻的相位”开始倒放。
        let at_end =
            ((line.end_time.saturating_sub(word.start_time)) as f32 / duration).clamp(0.0, 1.0);
        (at_end - (time.saturating_sub(line.end_time)) as f32 / duration).clamp(0.0, 1.0)
    };
    let up = if line.is_bg { 0.1 } else { 0.05 };
    -up * font_size * ease_out(phase)
}

/// CSS `ease-out` = `cubic-bezier(0, 0, 0.58, 1)`。
fn ease_out(x: f32) -> f32 {
    cubic_bezier(0.0, 0.0, 0.58, 1.0, x)
}

#[allow(clippy::too_many_arguments)]
fn paint_row_plain(
    window: &mut Window,
    row: &ShapedRow,
    origin_x: Pixels,
    row_top: Pixels,
    xform: LineXform,
    font_size: Pixels,
    color: Hsla,
    opacity: f32,
    blur: f32,
    edge: f32,
) -> u64 {
    let line_height = f32::from(row.height);
    let base = row_top
        + px(
            (line_height - f32::from(row.line.ascent) - f32::from(row.line.descent)) / 2.0
                + f32::from(row.line.ascent),
        );
    let color = Hsla {
        a: color.a * opacity * edge,
        ..color
    };
    let mut painted = 0;
    for run in &row.line.runs {
        for glyph in &run.glyphs {
            painted += paint_glyph_blurred(
                window,
                point(xform.x(origin_x + glyph.position.x), xform.y(base)),
                run.font_id,
                glyph.id,
                glyph.is_emoji,
                font_size,
                xform.scale,
                color,
                blur,
                SpriteMask::NONE,
            );
        }
    }
    painted
}

/// 字形基线原点的最终屏幕坐标。
///
/// `scale` = 行弹簧缩放 × 强调缩放；`center` 是字符盒中心（未缩放行内坐标），
/// 因为 AMLL 强调动画作用在字符 span 上且未设置 `transform-origin`，默认锚点
/// 是 `50% 50%`。行级缩放围绕 `xform` 的 left/right-center 锚点。
fn transformed_glyph_origin(
    xform: LineXform,
    glyph_x: Pixels,
    baseline: Pixels,
    center_x: Pixels,
    center_y: Pixels,
    scale: f32,
    offset: gpui::Point<Pixels>,
) -> gpui::Point<Pixels> {
    point(
        xform.x(center_x) + (glyph_x - center_x + offset.x) * scale,
        xform.y(center_y) + (baseline - center_y + offset.y) * scale,
    )
}

/// 绘制一个字形。
///
/// `font_size` 是视觉行整形时使用的**基准字号**；`glyph_scale`（行弹簧缩放
/// × 强调缩放）通过 sprite 变换应用，而不是改变光栅化字号 —— 弹簧动画每帧
/// 都会产生新的分数字号，如果拿它当字形大小学会触发每帧重新光栅化，使
/// DirectWrite 的 hinting/抗锯齿在帧间变化（视觉上就是"抖"和"糊"）。
///
/// 模糊是字形光栅化阶段对 alpha 掩码做的真卷积（见 vendor/gpui）：每个
/// `(字形, 基准字号, σ 档位)` 只产生一次 sprite，位置的小数部分由变换矩阵
/// 平移承担；emoji 不走单色掩码路径。
#[allow(clippy::too_many_arguments)]
fn paint_glyph_blurred(
    window: &mut Window,
    origin: gpui::Point<Pixels>,
    font_id: gpui::FontId,
    glyph_id: gpui::GlyphId,
    is_emoji: bool,
    font_size: Pixels,
    glyph_scale: f32,
    color: Hsla,
    blur: f32,
    mask: SpriteMask,
) -> u64 {
    if is_emoji {
        // 彩色 emoji 无单色掩码，也没有 sprite 变换路径；按缩放后的字号
        // 重新光栅化（歌词里 emoji 很少，成本可忽略）。遮罩无法作用到彩色
        // 图元上，保持原样。
        let _ = window.paint_emoji(
            origin,
            font_id,
            glyph_id,
            px(f32::from(font_size) * glyph_scale),
        );
        return 1;
    }
    let blur = if blur > 0.05 { px(blur) } else { px(0.0) };
    let _ = window.paint_glyph_scaled_masked(
        origin,
        font_id,
        glyph_id,
        font_size,
        color,
        blur,
        glyph_scale,
        mask,
    );
    1
}

/// 整行扫光的渐变边界（行内坐标，px）。
///
/// 返回 `(light_from, dark_from)`：`light_from` 左侧完全是 `bright`，
/// `dark_from` 右侧完全是 `dark`，中间按 fragment x 线性过渡。
/// **一行只有一个前沿**，所有字形共用这组边界；前沿随时间从行首推进到
/// 行尾，渐变带因此会压在两个词/两个字之间，而不是每个词各自重新点亮。
///
/// 时间轴：每个词的播放时间内，前沿从上一词尾（行首词从 `首词起点 - fade`）
/// 推进到本词词尾；词与词之间的停顿前沿保持不动，空白处不会出现跳变。
/// 所有词结束后整行处于亮侧。
fn row_sweep_bounds(
    words: &[WordSpan],
    line: &LyricLine,
    time: u64,
    fade: f32,
) -> Option<(f32, f32)> {
    let mut prev_end_time = 0u64;
    let mut prev_x1: Option<f32> = None;
    for span in words {
        let Some(word) = line.words.get(span.word_index) else {
            continue;
        };
        let x0 = f32::from(span.x);
        let x1 = x0 + f32::from(span.width);
        // 重叠/乱序的时间戳按上一词尾夹紧，保证前沿单调向前。
        let start = word.start_time.max(prev_end_time);
        let end = word.end_time.max(start);
        let from = prev_x1.unwrap_or(x0 - fade);
        if time < start {
            // 停顿区间：停在上一词尾（行首则停在整行左侧之外）。
            return Some((from, from + fade));
        }
        if time <= end {
            let progress = if end > start {
                (time - start) as f32 / (end - start) as f32
            } else {
                1.0
            };
            let front = from + (x1 - from) * progress;
            return Some((front, front + fade));
        }
        prev_end_time = end;
        prev_x1 = Some(x1);
    }
    // 没有可用词段时返回 None（由调用方回退到整块暗层）；否则所有词都
    // 已播完，整行在亮侧。
    prev_x1.map(|front| (front, front + fade))
}

/// 强调词逐字动画的一帧结果（全部为逻辑像素 / 无量纲）。
struct EmphasisFrame {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
    /// AMLL `emphasize-word-float` 的附加上浮（composite: add）。
    float_y: f32,
    /// `text-shadow` 的 alpha = `empEasing(x) * blur`。
    glow: f32,
    /// `text-shadow` 模糊半径（em）：`min(0.3, blur * 0.3)`。
    glow_sigma_em: f32,
}

/// AMLL `initEmphasizeAnimation` 的等价实现（按强调分组而非单字计算）。
fn emphasize_glyph(
    chunk: &EmphasisChunk,
    ordinal: usize,
    line: &LyricLine,
    time: u64,
    enabled: bool,
    font_size: f32,
) -> Option<EmphasisFrame> {
    if !chunk.emphasized || time < line.start_time {
        return None;
    }
    let char_count = chunk.char_count.max(1);
    let ordinal = ordinal.min(char_count - 1);

    let mut du = chunk.end_time.saturating_sub(chunk.start_time).max(1000) as f32;
    let mut amount = du / 2000.0;
    amount = if amount > 1.0 {
        amount.sqrt()
    } else {
        amount * amount * amount
    };
    let mut blur = du / 3000.0;
    blur = if blur > 1.0 {
        blur.sqrt()
    } else {
        blur * blur * blur
    };
    amount *= 0.6;
    blur *= 0.5;
    // 行尾强调分组额外增强（AMLL 的 last word 规则）。
    if chunk.word_end == line.words.len() {
        amount *= 1.6;
        blur *= 1.5;
        du *= 1.2;
    }
    let amount = amount.min(1.2);
    let blur = blur.min(0.8);

    let delay = chunk.start_time.saturating_sub(line.start_time) as f32;
    let per_char_delay = du / 2.5 / char_count as f32 * ordinal as f32;
    let local = (time - line.start_time) as f32;
    let x = ((local - delay - per_char_delay) / du).clamp(0.0, 1.0);
    if x <= 0.0 {
        return None;
    }
    let trans = emp_easing(x);

    // AMLL 的第二次动画（createCharFloatAnimation）：时长 `du * 1.4`，延迟与
    // 辉光动画相同；Y 在整段内按 `sin(πx)·up + x·up` 上浮（BG 两倍 up），
    // 结束后停在 -up。行离开热行集合时 AMLL 把该动画 playbackRate 设为 -1，
    // 这里用“行结束时刻的相位 - 离场后经过的时间”还原同一倒放轨迹。
    let float_duration = (du * 1.4).max(1.0);
    let float_delay = delay + per_char_delay;
    let float_phase_at = |local: f32| ((local - float_delay) / float_duration).clamp(0.0, 1.0);
    let float_x = if enabled {
        float_phase_at(local)
    } else {
        let at_end = float_phase_at(line.end_time.saturating_sub(line.start_time) as f32);
        (at_end - (time.saturating_sub(line.end_time)) as f32 / float_duration).clamp(0.0, 1.0)
    };
    let float_up = if line.is_bg { 0.1 } else { 0.05 };
    let float_y = -((float_x * std::f32::consts::PI).sin() + float_x) * float_up * font_size;

    Some(EmphasisFrame {
        scale: 1.0 + trans * 0.1 * amount,
        offset_x: -trans * 0.03 * amount * (char_count as f32 / 2.0 - ordinal as f32) * font_size,
        offset_y: -trans * 0.025 * amount * font_size,
        float_y,
        glow: trans * blur,
        glow_sigma_em: (blur * 0.3).min(0.3),
    })
}

fn emp_easing(x: f32) -> f32 {
    let mid = 0.5;
    if x < mid {
        cubic_bezier(0.2, 0.4, 0.58, 1.0, x / mid)
    } else {
        1.0 - cubic_bezier(0.3, 0.0, 0.58, 1.0, (x - mid) / mid)
    }
}

/// 三次贝塞尔缓动（CSS `cubic-bezier(x1,y1,x2,y2)`）。
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;

    let sample_x = |t: f32| ((ax * t + bx) * t + cx) * t;
    let sample_y = |t: f32| ((ay * t + by) * t + cy) * t;

    let (mut low, mut high) = (0.0f32, 1.0f32);
    let mut t = x;
    for _ in 0..8 {
        let sx = sample_x(t);
        if (sx - x).abs() < 1e-4 {
            break;
        }
        if sx < x {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) * 0.5;
    }
    sample_y(t)
}

#[cfg(test)]
fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x2E80..=0x2EFF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3190..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FA1F
    )
}

fn paint_interlude_dots(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    dots: InterludeDotsFrame,
    color: Hsla,
) {
    if dots.scale <= 0.001 {
        return;
    }
    let scale = dots.scale;
    let diameter = px(dots.dot_size.max(1.0) * scale);
    // AMLL `.interludeDots` 的 `transform-origin: center`：缩放围绕点组中心，
    // 间距也随 scale 一起缩放（flex 布局先完成，再整体缩放）。
    let group_w = px(dots.width.max(0.0));
    let center_x = bounds.left() + px(dots.x) + group_w * 0.5;
    let step = px((dots.dot_size + dots.gap) * scale);
    let mut x = center_x - group_w * scale * 0.5;
    let center_y = bounds.top() + px(dots.y) + px(dots.height * 0.5);
    let y = center_y - diameter * 0.5;
    for opacity in dots.dot_opacity {
        let alpha = (opacity * color.a).clamp(0.0, 1.0);
        if alpha > 0.001 {
            let mut quad = fill(
                Bounds::new(point(x, y), gpui::size(diameter, diameter)),
                Hsla { a: alpha, ..color },
            );
            quad.corner_radii = Corners::all(diameter * 0.5);
            window.paint_quad(quad);
        }
        x += step;
    }
}

/// 供测试/调试：当前视图使用的歌词行数。
impl LyricPlayerView {
    #[doc(hidden)]
    pub fn debug_line_count(&self) -> usize {
        self.inner.borrow().player.document().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_detection() {
        assert!(is_cjk_char('海'));
        assert!(!is_cjk_char('a'));
        assert!(is_cjk_char('あ'));
    }

    fn span(word_index: usize, x: f32, width: f32) -> WordSpan {
        WordSpan {
            word_index,
            byte_range: 0..1,
            char_offset: 0,
            x: px(x),
            width: px(width),
        }
    }

    fn line_with(words: Vec<LyricWord>) -> LyricLine {
        LyricLine {
            start_time: words.first().map_or(0, |w| w.start_time),
            end_time: words.last().map_or(0, |w| w.end_time),
            words,
            ..Default::default()
        }
    }

    #[test]
    fn row_sweep_is_one_continuous_front_across_words() {
        // 两个字紧挨着（CJK），词 0 在 [100, 150]，词 1 在 [150, 200]。
        let line = line_with(vec![
            LyricWord::new("字", 1_000, 2_000),
            LyricWord::new("词", 2_000, 3_000),
        ]);
        let words = [span(0, 100.0, 50.0), span(1, 150.0, 50.0)];
        let fade = 30.0;

        // 首词开始：前沿从词左侧之外进入，整个词在暗侧。
        let (light, dark) = row_sweep_bounds(&words, &line, 1_000, fade).unwrap();
        assert_eq!((light, dark), (70.0, 100.0));

        // 首词中段：渐变带横跨词 0 的右半和词 1 的左半。
        let (light, _) = row_sweep_bounds(&words, &line, 1_500, fade).unwrap();
        assert!((light - 110.0).abs() < 1e-4, "light = {light}");

        // 词 0 结束、词 1 开始：前沿停在 [150, 180]，没有按词重置。
        let at_end = row_sweep_bounds(&words, &line, 2_000, fade).unwrap();
        let just_started = row_sweep_bounds(&words, &line, 2_000 + 1, fade).unwrap();
        assert_eq!(at_end, (150.0, 180.0));
        assert!((just_started.0 - 150.05).abs() < 0.01, "{just_started:?}");

        // 整行播完：整行处于亮侧。
        assert_eq!(
            row_sweep_bounds(&words, &line, 3_000, fade),
            Some((200.0, 230.0))
        );
    }

    #[test]
    fn row_sweep_holds_during_word_gaps() {
        let line = line_with(vec![
            LyricWord::new("a", 0, 1_000),
            LyricWord::new("b", 2_000, 3_000),
        ]);
        let words = [span(0, 0.0, 100.0), span(1, 200.0, 100.0)];
        let fade = 40.0;

        // 词 0 结束到词 1 开始之间的停顿：停在词 0 词尾。
        assert_eq!(
            row_sweep_bounds(&words, &line, 1_500, fade),
            Some((100.0, 140.0))
        );
        assert_eq!(
            row_sweep_bounds(&words, &line, 2_000, fade),
            Some((100.0, 140.0))
        );
        // 词 1 播放时继续向前，覆盖空白 + 本词宽度。
        let (light, _) = row_sweep_bounds(&words, &line, 2_500, fade).unwrap();
        assert!((light - 200.0).abs() < 1e-4, "light = {light}");
    }

    #[test]
    fn row_sweep_without_words_falls_back() {
        let line = line_with(vec![LyricWord::new("x", 0, 1_000)]);
        assert_eq!(row_sweep_bounds(&[], &line, 500, 30.0), None);
    }

    #[test]
    fn word_float_advances_and_reverses_after_line_end() {
        let word = LyricWord::new("x", 1_000, 2_000);
        let line = LyricLine {
            words: vec![word.clone()],
            start_time: 0,
            end_time: 3_000,
            ..Default::default()
        };
        assert_eq!(word_float_offset(&word, &line, 500, true, 100.0), 0.0);
        let mid = word_float_offset(&word, &line, 1_500, true, 100.0);
        assert!((-5.0..0.0).contains(&mid), "mid = {mid}");
        let end = word_float_offset(&word, &line, 2_000, true, 100.0);
        assert!((end + 5.0).abs() < 0.01, "end = {end}");
        // 行结束后的倒放：应回到 0 的过程中。
        let back = word_float_offset(&word, &line, 3_500, false, 100.0);
        assert!((-5.0..0.0).contains(&back), "back = {back}");
        assert_eq!(word_float_offset(&word, &line, 5_000, false, 100.0), 0.0);
    }

    #[test]
    fn emphasized_char_float_ends_lifted_and_reverses() {
        let word = LyricWord::new("字", 1_000, 2_000);
        let line = LyricLine {
            words: vec![word],
            start_time: 0,
            end_time: 3_000,
            ..Default::default()
        };
        let chunk = EmphasisChunk {
            word_start: 0,
            word_end: 1,
            start_time: 1_000,
            end_time: 2_000,
            emphasized: true,
            char_count: 1,
            word_char_offsets: vec![0],
        };
        // 动画结束后（含 sin + 线性项）停在 -0.05em = -5px。
        let settled = emphasize_glyph(&chunk, 0, &line, 10_000, true, 100.0).unwrap();
        assert!(
            (settled.float_y + 5.0).abs() < 0.01,
            "float_y = {}",
            settled.float_y
        );
        assert!((settled.scale - 1.0).abs() < 1e-4);
        // 离场后倒放：sin 峰值处会比最终位置更高，随后回到 0。
        let back = emphasize_glyph(&chunk, 0, &line, 3_500, false, 100.0).unwrap();
        assert!(back.float_y < -5.0, "back = {}", back.float_y);
        assert_eq!(
            emphasize_glyph(&chunk, 0, &line, 10_000, false, 100.0)
                .unwrap()
                .float_y,
            0.0
        );
    }

    #[test]
    fn bezier_endpoints() {
        assert_eq!(cubic_bezier(0.2, 0.4, 0.58, 1.0, 0.0), 0.0);
        assert_eq!(cubic_bezier(0.2, 0.4, 0.58, 1.0, 1.0), 1.0);
        let mid = cubic_bezier(0.2, 0.4, 0.58, 1.0, 0.5);
        assert!(mid > 0.4 && mid < 0.9, "mid = {mid}");
    }

    #[test]
    fn line_xform_identity_at_scale_one() {
        let xform = LineXform {
            scale: 1.0,
            pivot_x: px(100.0),
            pivot_y: px(50.0),
        };
        assert_eq!(xform.x(px(37.0)), px(37.0));
        assert_eq!(xform.y(px(210.0)), px(210.0));
    }

    #[test]
    fn line_xform_scales_around_left_center_pivot() {
        // AMLL `.lyricLine { transform-origin: left }`：左边缘 + 垂直中心。
        let xform = LineXform {
            scale: 0.97,
            pivot_x: px(10.0),
            pivot_y: px(100.0),
        };
        // 锚点自身不动。
        assert!((f32::from(xform.x(px(10.0))) - 10.0).abs() < 1e-4);
        assert!((f32::from(xform.y(px(100.0))) - 100.0).abs() < 1e-4);
        // 左侧文本向右收缩、下方文本向上收缩。
        assert!((f32::from(xform.x(px(210.0))) - 204.0).abs() < 1e-3);
        assert!((f32::from(xform.y(px(200.0))) - 197.0).abs() < 1e-3);
    }

    #[test]
    fn emphasize_scales_around_char_box_center() {
        // 行不缩放，只有强调缩放：字符盒中心必须保持不动（AMLL 默认
        // transform-origin: 50% 50%）。
        let xform = LineXform {
            scale: 1.0,
            pivot_x: px(0.0),
            pivot_y: px(0.0),
        };
        let glyph_x = px(100.0);
        let baseline = px(80.0);
        let center_x = glyph_x + px(10.0);
        let center_y = px(60.0);
        let scale = 1.1;
        let origin = transformed_glyph_origin(
            xform,
            glyph_x,
            baseline,
            center_x,
            center_y,
            scale,
            point(px(0.0), px(0.0)),
        );
        // 把字形原点映射回字符中心，应与未缩放时的中心重合。
        let mapped_center_x = origin.x + (center_x - glyph_x) * scale;
        let mapped_center_y = origin.y + (center_y - baseline) * scale;
        assert!((f32::from(mapped_center_x) - f32::from(center_x)).abs() < 1e-4);
        assert!((f32::from(mapped_center_y) - f32::from(center_y)).abs() < 1e-4);
    }

    #[test]
    fn emphasis_without_scale_is_identity() {
        let xform = LineXform {
            scale: 1.0,
            pivot_x: px(0.0),
            pivot_y: px(0.0),
        };
        let origin = transformed_glyph_origin(
            xform,
            px(12.5),
            px(44.25),
            px(18.0),
            px(30.0),
            1.0,
            point(px(0.0), px(0.0)),
        );
        assert_eq!(origin, point(px(12.5), px(44.25)));
    }

    #[test]
    fn emphasis_offset_is_scaled_with_line_scale() {
        // AMLL 的 `matrix(scale) translate(offset)` 中 offset 会被缩放，
        // 且行级缩放先生效：offset * line_scale * emp_scale。
        let xform = LineXform {
            scale: 0.97,
            pivot_x: px(10.0),
            pivot_y: px(100.0),
        };
        let no_offset = transformed_glyph_origin(
            xform,
            px(100.0),
            px(80.0),
            px(110.0),
            px(60.0),
            0.97 * 1.1,
            point(px(0.0), px(0.0)),
        );
        let with_offset = transformed_glyph_origin(
            xform,
            px(100.0),
            px(80.0),
            px(110.0),
            px(60.0),
            0.97 * 1.1,
            point(px(2.0), px(-1.0)),
        );
        let sx = (f32::from(with_offset.x) - f32::from(no_offset.x)) / (0.97 * 1.1);
        let sy = (f32::from(with_offset.y) - f32::from(no_offset.y)) / (0.97 * 1.1);
        assert!((sx - 2.0).abs() < 1e-3, "sx = {sx}");
        assert!((sy + 1.0).abs() < 1e-3, "sy = {sy}");
    }
}
