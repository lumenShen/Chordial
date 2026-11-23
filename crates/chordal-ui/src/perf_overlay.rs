//! 画面性能诊断：置顶透明浮窗 + 帧率采样。
//!
//! 数据面刻意保持零成本：
//! - 主窗口每帧 `render` 入口调用 [`note_frame`]；浮窗关闭时只读一次
//!   原子布尔，不开锁、不分配。
//! - 浮窗自身 500ms 刷新一次，读取最近帧间隔计算 FPS / 1% Low，并从
//!   [`chordal_core::mem`] 取进程内存与累计 CPU 时间。
//!
//! 浮窗是独立 gpui 窗口：无系统标题栏、工具窗口（不进任务栏）、
//! 半透明背景、默认置顶。标题区域可拖动，右上角关闭。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chordal_core::mem::{self, MemSnapshot};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, Hsla, IntoElement, MouseButton, Render, SharedString, Window,
    canvas, div, fill, point, px, rgba, size,
};

use crate::app::ChordialApp;
use crate::icons::{self, Icon};

/// 浮窗刷新间隔。500ms 足以看清 FPS 抖动，又不会让观测本身成为负载。
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);
/// 帧间隔环形缓冲容量：300Hz 约 3.4s，60Hz 约 17s。
const FRAME_CAPACITY: usize = 1024;
/// 超过该间隔视为“空闲后重新开始”，清空样本，避免把挂起时间算成卡顿帧。
const IDLE_RESET: Duration = Duration::from_secs(1);
/// 小于该间隔的重复 render（同一平台帧内）忽略。
const MIN_FRAME: Duration = Duration::from_micros(200);
/// “当前 FPS”取最近约 1 秒的样本。
const FPS_WINDOW_US: u64 = 1_000_000;
/// 迷你帧时间图的柱子数量。
const SPARK_BARS: usize = 64;
/// 迷你图纵轴下限（60Hz 预算），峰值更慢时自动放大纵轴。
const SPARK_FLOOR_MS: f32 = 16.67;

const WINDOW_W: f32 = 300.0;
const WINDOW_H: f32 = 250.0;
const SCREEN_MARGIN: f32 = 24.0;

// ===================== 帧采样 =====================

struct FrameProbe {
    last: Option<Instant>,
    /// 相邻两帧 render 入口的间隔（微秒），按时间先后排列。
    intervals: VecDeque<u32>,
}

impl FrameProbe {
    fn new() -> Self {
        Self {
            last: None,
            intervals: VecDeque::with_capacity(FRAME_CAPACITY),
        }
    }

    fn note(&mut self, now: Instant) {
        let Some(previous) = self.last.replace(now) else {
            return;
        };
        let delta = now.saturating_duration_since(previous);
        if delta > IDLE_RESET {
            // 中间没有任何重绘（静止/最小化），这是空闲而不是慢帧。
            self.intervals.clear();
        } else if delta >= MIN_FRAME {
            if self.intervals.len() == FRAME_CAPACITY {
                self.intervals.pop_front();
            }
            self.intervals
                .push_back(delta.as_micros().min(u32::MAX as u128) as u32);
        }
    }

    fn metrics(&self, now: Instant) -> FrameMetrics {
        let mut metrics = FrameMetrics {
            samples: self.intervals.len(),
            ..Default::default()
        };
        let Some(last) = self.last else {
            return metrics;
        };
        // 最近 1 秒没有重绘 = 当前没有帧在产出。
        if now.saturating_duration_since(last) > IDLE_RESET {
            return metrics;
        }

        // 当前 FPS：从最近样本往回累加约 1 秒。
        let mut total_us = 0u64;
        let mut frames = 0u64;
        for us in self.intervals.iter().rev() {
            if total_us >= FPS_WINDOW_US {
                break;
            }
            total_us += *us as u64;
            frames += 1;
        }
        if total_us > 0 && frames > 0 {
            metrics.fps = Some(frames as f64 * 1_000_000.0 / total_us as f64);
        }

        if !self.intervals.is_empty() {
            let mut sorted: Vec<u32> = self.intervals.iter().copied().collect();
            sorted.sort_unstable();
            let count = sorted.len();
            let at = |p: f64| sorted[((count - 1) as f64 * p).round() as usize];
            metrics.p50_ms = Some(at(0.50) as f64 / 1000.0);
            metrics.p99_ms = Some(at(0.99) as f64 / 1000.0);

            // 1% Low：取最慢的 1% 帧（至少 1 帧），用其平均帧时间换算 FPS。
            // n < 10 时样本太少，不给数字，避免一帧抖动就把读数砸到个位数。
            if count >= 10 {
                let worst = (count / 100).max(1);
                let sum: u64 = sorted.iter().rev().take(worst).map(|us| *us as u64).sum();
                let mean_us = sum as f64 / worst as f64;
                if mean_us > 0.0 {
                    metrics.low_1pct_fps = Some(1_000_000.0 / mean_us);
                }
            }

            // 迷你图按时间顺序取最近样本（sorted 只用于分位数）。
            metrics.sparkline_ms = self
                .intervals
                .iter()
                .skip(self.intervals.len().saturating_sub(SPARK_BARS))
                .map(|us| *us as f32 / 1000.0)
                .collect();
        }
        metrics
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static PROBE: OnceLock<Mutex<FrameProbe>> = OnceLock::new();

fn probe() -> &'static Mutex<FrameProbe> {
    PROBE.get_or_init(|| Mutex::new(FrameProbe::new()))
}

/// 浮窗开关。关闭时清空样本，避免下次打开读到上一次会话的陈旧数据。
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if let Some(probe) = PROBE.get()
        && let Ok(mut probe) = probe.lock()
    {
        *probe = FrameProbe::new();
    }
}

/// 主窗口每帧调用。未开启浮窗时只有一次原子读。
pub fn note_frame() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    note_frame_at(Instant::now());
}

fn note_frame_at(now: Instant) {
    if let Ok(mut probe) = probe().lock() {
        probe.note(now);
    }
}

#[derive(Debug, Clone, Default)]
struct FrameMetrics {
    fps: Option<f64>,
    /// 最慢 1% 帧的平均帧率（标准 1% Low）。
    low_1pct_fps: Option<f64>,
    p50_ms: Option<f64>,
    p99_ms: Option<f64>,
    samples: usize,
    /// 最近若干帧的帧时间（ms），供迷你图使用。
    sparkline_ms: Vec<f32>,
}

fn frame_metrics(now: Instant) -> FrameMetrics {
    probe()
        .lock()
        .map(|probe| probe.metrics(now))
        .unwrap_or_default()
}

// ===================== 进程采样 =====================

struct CpuSampler {
    last: Option<(Instant, Option<Duration>)>,
    percent: Option<f64>,
}

impl CpuSampler {
    fn new() -> Self {
        Self {
            last: Some((Instant::now(), mem::process_cpu_time())),
            percent: None,
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let cpu = mem::process_cpu_time();
        if let (Some((previous_at, Some(previous_cpu))), Some(cpu)) = (self.last, cpu) {
            let wall = now.saturating_duration_since(previous_at).as_secs_f64();
            if wall > 0.05 {
                let used = cpu.saturating_sub(previous_cpu).as_secs_f64();
                // 100% = 一个核心满载；多核进程可能超过 100%。
                self.percent = Some((used / wall * 100.0).clamp(0.0, 999.9));
            }
        }
        self.last = Some((now, cpu));
    }
}

// ===================== 浮窗视图 =====================

pub struct PerfOverlayView {
    app: gpui::WeakEntity<ChordialApp>,
    cpu: CpuSampler,
    frames: FrameMetrics,
    memory: MemSnapshot,
    cores: usize,
}

impl PerfOverlayView {
    pub fn new(
        app: gpui::WeakEntity<ChordialApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        set_enabled(true);
        pin_window_topmost(window);

        // Alt+F4 / 系统关闭：只更新宿主状态，不在这里 remove_window，
        // 让平台按正常关闭流程走。
        let app_for_close = app.clone();
        window.on_window_should_close(cx, move |_window, cx| {
            let _ = app_for_close.update(cx, |app, cx| app.perf_overlay_closed(cx));
            true
        });

        let mut this = Self {
            app,
            cpu: CpuSampler::new(),
            frames: FrameMetrics::default(),
            memory: mem::snapshot(),
            cores: std::thread::available_parallelism()
                .map(|count| count.get())
                .unwrap_or(1),
        };
        this.tick();

        // 浮窗自己的刷新节拍。实体随窗口销毁后 update 失败，循环退出。
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor().timer(REFRESH_INTERVAL).await;
                if weak
                    .update(cx, |view, cx| {
                        view.tick();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        this
    }

    fn tick(&mut self) {
        self.cpu.tick();
        self.frames = frame_metrics(Instant::now());
        self.memory = mem::snapshot();
    }
}

impl Drop for PerfOverlayView {
    fn drop(&mut self) {
        set_enabled(false);
    }
}

fn paint_sparkline(bounds: Bounds<gpui::Pixels>, samples: &[f32], window: &mut Window) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    if width < 1.0 || height < 1.0 || samples.is_empty() {
        return;
    }

    // 纵轴自适应：至少覆盖 60Hz 预算，慢帧场景按峰值放大，避免全部顶格。
    let peak = samples
        .iter()
        .copied()
        .fold(SPARK_FLOOR_MS, f32::max)
        .min(200.0);
    let max_ms = (peak * 1.12).max(SPARK_FLOOR_MS);

    // 60Hz 预算线：作为读图基准，不参与颜色判定。
    let budget_y = f32::from(bounds.bottom()) - (height - 1.0) * (SPARK_FLOOR_MS / max_ms);
    window.paint_quad(
        fill(
            Bounds::new(
                point(px(f32::from(bounds.left())), px(budget_y)),
                size(px(width), px(1.0)),
            ),
            hxa(0xFFFFFF24),
        )
        .corner_radii(px(0.5)),
    );

    let count = samples.len();
    let gap = 1.0;
    let bar_w = ((width - gap * (count.saturating_sub(1)) as f32) / count as f32).max(1.0);
    for (index, ms) in samples.iter().enumerate() {
        let fraction = (ms / max_ms).clamp(0.04, 1.0);
        let bar_h = (height - 1.0) * fraction;
        let x = f32::from(bounds.left()) + index as f32 * (bar_w + gap);
        let y = f32::from(bounds.bottom()) - bar_h;
        let color = if *ms <= 17.0 {
            hx(0x3FB950)
        } else if *ms <= 34.0 {
            hx(0xD29922)
        } else {
            hx(0xF85149)
        };
        window.paint_quad(
            fill(
                Bounds::new(point(px(x), px(y)), size(px(bar_w), px(bar_h))),
                color,
            )
            .corner_radii(px(0.5)),
        );
    }
}

fn hx(rgb: u32) -> Hsla {
    gpui::rgb(rgb).into()
}

fn hxa(value: u32) -> Hsla {
    rgba(value).into()
}

fn fps_color(fps: Option<f64>) -> Hsla {
    match fps {
        Some(fps) if fps >= 55.0 => hx(0x3FB950),
        Some(fps) if fps >= 30.0 => hx(0xD29922),
        Some(_) => hx(0xF85149),
        None => hx(0x6E7681),
    }
}

fn fmt_fps(fps: Option<f64>) -> SharedString {
    match fps {
        Some(fps) if fps.is_finite() => format!("{fps:.0}").into(),
        _ => "--".into(),
    }
}

fn stat_row(label: &'static str, value: SharedString, color: Hsla) -> AnyElement {
    div()
        .h(px(16.0))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(10.0))
                .text_color(hx(0x8B949E))
                .child(label),
        )
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(color)
                .child(value),
        )
        .into_any_element()
}

impl Render for PerfOverlayView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let text = hx(0xE6EDF3);
        let dim = hx(0x8B949E);
        let faint = hx(0x6E7681);
        let fps = fmt_fps(self.frames.fps);
        let low = fmt_fps(self.frames.low_1pct_fps);
        let fps_tone = fps_color(self.frames.fps);
        let low_tone = fps_color(self.frames.low_1pct_fps);

        let frame_time = match (self.frames.p50_ms, self.frames.p99_ms) {
            (Some(p50), Some(p99)) => format!("{p50:.1} · {p99:.1} ms"),
            _ => "--".to_string(),
        };
        let cpu = self
            .cpu
            .percent
            .map(|percent| format!("{percent:.1}%"))
            .unwrap_or_else(|| "--".to_string());
        let resident = mem::format_bytes(self.memory.resident());
        let heap = mem::format_bytes(self.memory.live_bytes as u64);
        let footer = if self.frames.fps.is_some() {
            format!("{} 线程 · 最近 {} 帧", self.cores, self.frames.samples)
        } else {
            format!("{} 线程 · 无重绘", self.cores)
        };

        let close_app = self.app.clone();
        let close = div()
            .id("perf-overlay-close")
            .size(px(20.0))
            .rounded(px(5.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(hxa(0xFFFFFF14)))
            .child(icons::icon(Icon::Close, 10.0, faint))
            .on_click(move |_, window, cx| {
                window.remove_window();
                let _ = close_app.update(cx, |app, cx| app.perf_overlay_closed(cx));
            });

        let header = div()
            .h(px(20.0))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .cursor(gpui::CursorStyle::OpenHand)
                    .on_mouse_down(MouseButton::Left, |_, window, _| {
                        window.start_window_move();
                    })
                    .child(div().size(px(6.0)).rounded_full().bg(hx(0x3FB950)))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(text)
                            .child("画面性能"),
                    ),
            )
            .child(close);

        let hero = div()
            .flex()
            .items_end()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(34.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(fps_tone)
                            .child(fps),
                    )
                    .child(div().text_size(px(9.0)).text_color(faint).child("FPS")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(1.0))
                    .child(div().text_size(px(8.0)).text_color(faint).child("1% LOW"))
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(low_tone)
                            .child(low),
                    ),
            );

        let sparkline_ms = self.frames.sparkline_ms.clone();
        let sparkline = canvas(
            move |bounds, _, _| bounds,
            move |_prepaint, bounds, window, _| {
                paint_sparkline(bounds, &sparkline_ms, window);
            },
        )
        .h(px(28.0))
        .w_full();

        div().size_full().p(px(1.0)).child(
            div()
                .size_full()
                .rounded(px(13.0))
                .bg(hxa(0x0B0F14E0))
                .border_1()
                .border_color(hxa(0xFFFFFF1A))
                .flex()
                .flex_col()
                .px(px(14.0))
                .py(px(12.0))
                .child(header)
                .child(div().h(px(6.0)))
                .child(hero)
                .child(div().h(px(7.0)))
                .child(sparkline)
                .child(div().h(px(7.0)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(stat_row("帧耗时 P50 · P99", frame_time.into(), text))
                        .child(stat_row("CPU（单核=100%）", cpu.into(), text))
                        .child(stat_row("内存", resident.into(), text))
                        .child(stat_row("堆内存", heap.into(), dim)),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(8.0))
                        .text_color(faint)
                        .child(footer)
                        .child("置顶 · 可拖动"),
                ),
        )
    }
}

// ===================== 窗口 =====================

/// 浮窗窗口参数：主屏右上角、无边框、半透明、工具窗口。
pub fn window_options(cx: &App) -> gpui::WindowOptions {
    let origin = cx
        .primary_display()
        .map(|display| {
            let bounds = display.bounds();
            point(
                bounds.origin.x + bounds.size.width - px(WINDOW_W) - px(SCREEN_MARGIN),
                bounds.origin.y + px(SCREEN_MARGIN),
            )
        })
        .unwrap_or_else(|| point(px(SCREEN_MARGIN), px(SCREEN_MARGIN)));

    gpui::WindowOptions {
        window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::new(
            origin,
            size(px(WINDOW_W), px(WINDOW_H)),
        ))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Chordial 画面性能".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        focus: false,
        show: true,
        kind: gpui::WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        window_background: gpui::WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

/// Windows 上把工具窗口钉到最上层；其它平台由 `WindowKind::PopUp` 的
/// 窗口层级策略处理（macOS 浮动面板、Linux 由 WM 决定）。
fn pin_window_topmost(window: &Window) {
    #[cfg(windows)]
    {
        use core::ffi::c_void;
        use raw_window_handle::RawWindowHandle;

        const HWND_TOPMOST: isize = -1;
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOMOVE: u32 = 0x0002;
        const SWP_NOACTIVATE: u32 = 0x0010;

        #[link(name = "user32")]
        unsafe extern "system" {
            fn SetWindowPos(
                hwnd: *mut c_void,
                insert_after: *mut c_void,
                x: i32,
                y: i32,
                cx: i32,
                cy: i32,
                flags: u32,
            ) -> i32;
        }

        if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window)
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            let hwnd = win32.hwnd.get() as *mut c_void;
            // SAFETY: hwnd 来自当前窗口的原始句柄；只修改 Z 序，不触碰尺寸/位置。
            unsafe {
                SetWindowPos(
                    hwnd,
                    HWND_TOPMOST as *mut c_void,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(probe: &mut FrameProbe, start: Instant, interval: Duration, frames: usize) -> Instant {
        let mut now = start;
        for _ in 0..frames {
            now += interval;
            probe.note(now);
        }
        now
    }

    #[test]
    fn steady_60hz_reports_matching_fps_and_low() {
        let start = Instant::now();
        let mut probe = FrameProbe::new();
        // note 的第一次只记录时间戳；120 个间隔 = 121 次 note。
        let now = feed(&mut probe, start, Duration::from_millis(16), 121);
        let metrics = probe.metrics(now);
        assert_eq!(metrics.samples, 120);
        assert!((metrics.fps.unwrap() - 62.5).abs() < 1.0);
        assert!((metrics.low_1pct_fps.unwrap() - 62.5).abs() < 1.0);
        assert!((metrics.p50_ms.unwrap() - 16.0).abs() < 0.05);
        assert!((metrics.p99_ms.unwrap() - 16.0).abs() < 0.05);
        assert_eq!(metrics.sparkline_ms.len(), 64);
    }

    #[test]
    fn single_long_frame_drags_one_percent_low_down() {
        let start = Instant::now();
        let mut probe = FrameProbe::new();
        let now = feed(&mut probe, start, Duration::from_millis(16), 101);
        // 一帧卡 100ms：1% Low 应掉到 10fps，而平均 FPS 不会被完全拖垮。
        probe.note(now + Duration::from_millis(100));
        let metrics = probe.metrics(now + Duration::from_millis(100));
        assert!(metrics.low_1pct_fps.unwrap() < 12.0);
        assert!(metrics.fps.unwrap() > 25.0);
    }

    #[test]
    fn idle_gap_resets_samples_instead_of_counting_a_slow_frame() {
        let start = Instant::now();
        let mut probe = FrameProbe::new();
        feed(&mut probe, start, Duration::from_millis(16), 10);
        assert_eq!(probe.intervals.len(), 9);
        probe.note(start + Duration::from_secs(3));
        assert!(probe.intervals.is_empty());
        let metrics = probe.metrics(start + Duration::from_secs(5));
        assert!(metrics.fps.is_none());
        assert!(metrics.low_1pct_fps.is_none());
    }

    #[test]
    fn duplicate_render_in_same_platform_frame_is_ignored() {
        let start = Instant::now();
        let mut probe = FrameProbe::new();
        probe.note(start);
        probe.note(start + Duration::from_micros(50));
        assert!(probe.intervals.is_empty());
        probe.note(start + Duration::from_millis(16));
        assert_eq!(probe.intervals.len(), 1);
    }
}
