//! gpui 歌词渲染视图。
//!
//! 这里不依赖任何 DOM/CSS：文本由 gpui 整形后，**逐字形**绘制。这样能精确
//! 复现 AMLL 的几个关键视觉效果：
//! - 逐词渐变扫光（按字形位置计算 mask alpha，不再依赖 CSS mask）；
//! - 强调词逐字缩放 + 辉光（AMLL `initEmphasizeAnimation` 的等价缓动与参数）；
//! - 行级弹簧位移/缩放、透明度/模糊过渡；
//! - 对唱行右对齐、背景歌词行、间奏呼吸点。
//!
//! 性能：每帧只为 `visible_range()`（含 overscan）内的行整形并绘制字形；
//! 文本整形结果按行 LRU 缓存；`paint_glyph` 调用数只与可见字符数相关。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use chordal_core::perf::PerfScope;
use gpui::prelude::*;
use gpui::{
    App, Bounds, ContentMask, Context, Corners, Element, ElementId, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, Pixels, Render,
    RenderImage, Style, Window, div, fill, point, px, relative,
};

use crate::background::{BackgroundRendererKind, LyricBackgroundRenderer};
use crate::model::{LyricLine, LyricWord};
use crate::player::{
    InterludeDotsFrame, LayoutMetrics, LineFrameState, LyricPlayer, LyricPlayerConfig,
};
use crate::text::{EmphasisChunk, LineShape, LyricTextStyle, ShapedRow, TextLayoutCache};

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
    /// 由外部绘制全窗口背景时关闭内部背景，只保留歌词。
    background_visible: bool,
    /// 背景光栅/绘制的覆盖区域。沉浸层里歌词视图只占一列，但背景要铺满
    /// 整个窗口：此时把该值设为窗口 bounds，光栅按全窗尺寸计算（低分辨率），
    /// 由 GPU 双线性上采样到屏幕。
    background_bounds: Option<Bounds<Pixels>>,
    last_bounds: Bounds<Pixels>,
    seek_handler: Option<Arc<dyn Fn(u64) + Send + Sync>>,
    /// 鼓点包络来源（0..1）。每帧读取，避免外层再维护定时器。
    beat_source: Option<Arc<dyn Fn() -> f32 + Send + Sync>>,
    /// 状态快照 33ms 才更新一次，这里用帧间隔外推，让逐词扫光保持 60fps 平滑。
    last_clock_pos: u64,
    display_time_ms: f64,
    initial_shaped: bool,
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
            background_visible: true,
            background_bounds: None,
            last_bounds: Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(0.0), px(0.0))),
            seek_handler: None,
            beat_source: None,
            last_clock_pos: 0,
            display_time_ms: 0.0,
            initial_shaped: false,
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

        // 典型歌曲（≤1000 行）一次性整形，让滚动前缀和从第一帧就是真实高度；
        // 超长文档仍按可见范围懒整形，避免首帧卡顿。
        if !self.initial_shaped {
            let n = self.player.document().len();
            if (1..=1000).contains(&n) {
                let doc = self.player.document();
                self.text.ensure_range(doc, 0..n, window);
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
            self.bg_image = self.background.render(self.dt_ms);
        } else {
            self.bg_image = None;
        }
    }
}

/// AMLL 歌词播放器的 gpui 视图。
pub struct LyricPlayerView {
    inner: Rc<RefCell<Inner>>,
}

impl LyricPlayerView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let view = Self {
            inner: Rc::new(RefCell::new(Inner::new())),
        };
        // 60fps 渲染泵：不跟随显示器刷新率。在 120Hz 屏上 `request_animation_frame`
        // 会让整个 gpui 场景每 8ms 提交一次（CPU 翻倍），而 AMLL 的参考节奏是 60fps。
        // 泵只在仍有动画（播放中 / 弹簧未收敛 / 背景流动）时唤醒重绘，暂停后彻底空闲。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        let (needs, has_lyrics) = {
                            let inner = this.inner.borrow();
                            (
                                inner.playing
                                    || inner.animating
                                    || (inner.background_visible
                                        && inner.background.is_animating()),
                                !inner.player.document().is_empty(),
                            )
                        };
                        if needs || (this.inner.borrow().playing && has_lyrics) {
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
        view
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

    /// 鼓点包络来源（0..1）。视图每帧读取一次，驱动 Mesh 背景的跳动；
    /// 传 `None` 则背景保持静态流动。
    pub fn set_beat_source(&mut self, source: Option<Arc<dyn Fn() -> f32 + Send + Sync>>) {
        self.inner.borrow_mut().beat_source = source;
    }

    fn handle_click(&self, position: gpui::Point<Pixels>) {
        // 只在本作用域内持有借用；回调可能更新本视图（BorrowMutError 风险）。
        let (start, handler) = {
            let inner = self.inner.borrow();
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
        inner.background = LyricBackgroundRenderer::new(kind);
        inner.background.set_fps(fps);
        inner.background.set_render_scale(scale);
        inner.background.set_flow_speed(flow);
        inner.background.set_static_mode(static_mode);
        if let Some((w, h, data)) = inner.album.clone() {
            inner.background.set_album_rgba(w, h, data);
        }
        inner.bg_image = None;
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

    /// 设置专辑封面（RGBA8，非预乘）。解码由调用方完成，避免在 UI 线程阻塞。
    pub fn set_album_rgba(&mut self, width: u32, height: u32, rgba: Vec<u8>) {
        let mut inner = self.inner.borrow_mut();
        inner.album = Some((width, height, rgba.clone()));
        inner.background.set_album_rgba(width, height, rgba);
        // 暂停时也立刻丢掉旧封面；background 会为 needs_clear 渲染一帧。
        inner.bg_image = None;
    }

    pub fn clear_album(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.album = None;
        inner.background.clear_album();
        // 停止/无封面时立即回落到底色；曲目切换走 set_album_rgba 的交叉淡入。
        inner.bg_image = None;
    }

    pub fn background_kind(&self) -> BackgroundRendererKind {
        self.inner.borrow().background.kind()
    }

    pub fn has_lyrics(&self) -> bool {
        !self.inner.borrow().player.document().is_empty()
    }

    pub fn is_animating(&self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let dt = inner.last_instant.elapsed().as_secs_f32() * 1000.0;
        let animating = inner.player.update(dt);
        inner.last_instant = Instant::now();
        animating || inner.background.is_animating()
    }
}

impl Render for LyricPlayerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (has_lyrics, fallback_text_color) = {
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
            // `animating` 只用于 60fps 泵的唤醒判断（`is_animating` 读取），
            // 这里不再请求 `request_animation_frame`。
            let _ = inner.animating || bg_anim || (playing && has_lyrics);
            (has_lyrics, inner.colors.text)
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
            .when(!has_lyrics, |this| {
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
    type PrepaintState = ();

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
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
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
                let _ = window.paint_image(bg_bounds, Corners::default(), image.clone(), 0, false);
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
        if doc.is_empty() {
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
    // AMLL 的扫光半宽 = `word.height * wordFadeWidth`（word.height 是行盒高度，
    // 即 1.2em），不是字号 × 0.5。用行高才能与 AMLL 的渐变宽度逐像素一致。
    let fade_ratio = style.word_fade_width.max(0.0001);
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
                f32::from(row.height) * fade_ratio,
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

            let float_y = if span.is_some() {
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
            let mut glow = 0.0f32;
            let mut glow_sigma_em = 0.0f32;

            if is_dynamic {
                if let Some(span) = span {
                    if let Some(word) = line.words.get(span.word_index) {
                        let progress = word_progress(word, time);
                        let t = sweep_brightness(
                            f32::from(glyph.position.x),
                            f32::from(span.x),
                            f32::from(span.width),
                            progress,
                            fade,
                        );
                        ha = dark + (bright - dark) * t;

                        // 强调词：按强调分组的合并时序逐字 scale/translate/glow。
                        if let (Some(ordinal), Some(&Some(chunk_index))) =
                            (char_ordinal, word_chunks.get(span.word_index))
                            && let Some(chunk) = chunks.get(chunk_index)
                            && let Some(frame) =
                                emphasize_glyph(chunk, ordinal, line, time, font_px)
                        {
                            glyph_scale = xform.scale * frame.scale;
                            off_x = px(frame.offset_x);
                            off_y += px(frame.offset_y + frame.float_y);
                            glow = frame.glow;
                            glow_sigma_em = frame.glow_sigma_em;
                        }
                    } else {
                        ha = dark;
                    }
                } else {
                    ha = dark;
                }
            }

            // 背景人声行：AMLL SOLID 模式无逐词遮罩，统一 0.4 alpha
            //（乘上 wrapper 的 0.4 之前；wrapper 已在 opacity 里）。
            if line.is_bg {
                ha = 0.4;
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
) -> u64 {
    if is_emoji {
        // 彩色 emoji 无单色掩码，也没有 sprite 变换路径；按缩放后的字号
        // 重新光栅化（歌词里 emoji 很少，成本可忽略）。
        let _ = window.paint_emoji(
            origin,
            font_id,
            glyph_id,
            px(f32::from(font_size) * glyph_scale),
        );
        return 1;
    }
    let blur = if blur > 0.05 { px(blur) } else { px(0.0) };
    let _ = window.paint_glyph_scaled(
        origin,
        font_id,
        glyph_id,
        font_size,
        color,
        blur,
        glyph_scale,
    );
    1
}

fn word_progress(word: &LyricWord, time: u64) -> f32 {
    if time <= word.start_time {
        0.0
    } else if time >= word.end_time || word.end_time <= word.start_time {
        1.0
    } else {
        (time - word.start_time) as f32 / (word.end_time - word.start_time) as f32
    }
}

/// 扫光在某个字形处的亮度（1 = bright，0 = dark）。
///
/// AMLL 的 mask 在单词开始前停在 `词起点 - fadeWidth`，结束时边界越过词尾；
/// 因此边界从 `x - fade` 走到 `x + width`。渐变从边界开始，向右 `fade` 宽
/// 由 bright 过渡到 dark。
fn sweep_brightness(glyph_x: f32, span_x: f32, span_w: f32, progress: f32, fade: f32) -> f32 {
    let sweep = span_x - fade + (span_w + fade) * progress;
    if fade > 0.01 {
        (((sweep + fade) - glyph_x) / fade).clamp(0.0, 1.0)
    } else if glyph_x <= sweep {
        1.0
    } else {
        0.0
    }
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

    // AMLL 的第二次动画：Y 在整段内按 sin 曲线再浮起 0.05em（BG ×2），
    // 延迟为 `wordDe - 400ms`，时长 `du * 1.4`。
    let float_delay = delay + per_char_delay - 400.0;
    let float_x = ((local - float_delay) / (du * 1.4)).clamp(0.0, 1.0);
    let float_up = if line.is_bg { 0.1 } else { 0.05 };
    // -sin(π·x) · up · font_size（AMLL 的 `translateY(-y * 0.05em)`）。
    let float_y = -(float_x * std::f32::consts::PI).sin() * float_up * font_size;

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
    let diameter = px(dots.dot_size.max(1.0) * dots.scale);
    let gap = px(dots.dot_size.max(1.0) * 0.25 + 4.0);
    let mut x = bounds.left() + px(dots.x);
    let y = bounds.top() + px(dots.y + (dots.height - dots.dot_size) * 0.5);
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
        x += diameter + gap;
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
    fn word_progress_clamps() {
        let w = LyricWord::new("x", 1000, 2000);
        assert_eq!(word_progress(&w, 500), 0.0);
        assert_eq!(word_progress(&w, 1500), 0.5);
        assert_eq!(word_progress(&w, 2500), 1.0);
    }

    #[test]
    fn cjk_detection() {
        assert!(is_cjk_char('海'));
        assert!(!is_cjk_char('a'));
        assert!(is_cjk_char('あ'));
    }

    #[test]
    fn sweep_word_is_dark_before_start_and_bright_at_end() {
        // 词起点 100、宽 200、fade 30（≈0.6em @50px）。
        let (x, w, fade) = (100.0, 200.0, 30.0);
        // 进度 0：整个词（含第一个字形）必须仍处于暗层。
        assert_eq!(sweep_brightness(x, x, w, 0.0, fade), 0.0);
        assert_eq!(sweep_brightness(x + 50.0, x, w, 0.0, fade), 0.0);
        // 进度 1：整个词变亮（最后一个字形也在 bright 一侧）。
        assert_eq!(sweep_brightness(x, x, w, 1.0, fade), 1.0);
        assert_eq!(sweep_brightness(x + w, x, w, 1.0, fade), 1.0);
        // 中间：边界在词内，且左亮右暗。
        let mid = sweep_brightness(x + w * 0.5, x, w, 0.5, fade);
        assert!((0.0..1.0).contains(&mid), "mid = {mid}");
        assert!(
            sweep_brightness(x + 10.0, x, w, 0.5, fade)
                > sweep_brightness(x + w - 10.0, x, w, 0.5, fade)
        );
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
