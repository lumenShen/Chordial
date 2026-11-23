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

mod paint;
mod emphasis;

use paint::*;
use emphasis::*;

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


/// 供测试/调试：当前视图使用的歌词行数。
impl LyricPlayerView {
    #[doc(hidden)]
    pub fn debug_line_count(&self) -> usize {
        self.inner.borrow().player.document().len()
    }
}

#[cfg(test)]
mod tests;
