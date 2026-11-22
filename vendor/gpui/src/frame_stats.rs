//! 可选帧统计（环境变量开启，默认零开销分支）。
//!
//! 目标：为 300Hz 优化提供 draw/present/总耗时与 miss 的可观测数据。
//! 仅在 `GPUI_FRAME_STATS=1` 时记录；热路径为一次 `OnceLock` 读取 + 单把
//! Mutex 的定容环形写入（O(1)、无逐帧分配）。
//!
//! - `GPUI_TARGET_HZ`：预算目标，默认 300。
//! - `GPUI_FRAME_STATS`：`1`/`true` 开启。
//!
//! 每 240 个实际绘制的帧输出一次 p50/p95/p99/max（微秒），以及超过预算的
//! miss 数。输出走 `log`，与仓库的 tracing-subscriber 默认桥接兼容。

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const CAPACITY: usize = 4096;
const REPORT_FRAMES: u64 = 240;
const TARGET_HZ_DEFAULT: f64 = 300.0;

#[derive(Clone, Copy)]
struct FrameRecord {
    draw_us: u64,
    prepaint_us: u64,
    paint_us: u64,
    present_us: u64,
    total_us: u64,
    interval_us: u64,
    dirty_views: u32,
}

struct Stats {
    frames: VecDeque<FrameRecord>,
    drawn_frames: u64,
    missed: u64,
    last_frame_start: Option<Instant>,
}

impl Stats {
    fn new() -> Self {
        Self {
            frames: VecDeque::with_capacity(CAPACITY),
            drawn_frames: 0,
            missed: 0,
            last_frame_start: None,
        }
    }
}

static STATS: OnceLock<Mutex<Stats>> = OnceLock::new();
static ENABLED: OnceLock<bool> = OnceLock::new();
static TARGET_US: OnceLock<u64> = OnceLock::new();

fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("GPUI_FRAME_STATS").as_deref(),
            Ok("1") | Ok("true") | Ok("TRUE")
        )
    })
}

fn target_us() -> u64 {
    *TARGET_US.get_or_init(|| {
        let hz = std::env::var("GPUI_TARGET_HZ")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|hz| hz.is_finite() && *hz >= 30.0)
            .unwrap_or(TARGET_HZ_DEFAULT);
        (1_000_000.0 / hz).round() as u64
    })
}

/// 记录一次实际绘制的帧。`start` 是 request_frame 回调进入时刻。
pub(crate) fn record(
    start: Instant,
    draw: Duration,
    prepaint: Duration,
    paint: Duration,
    present: Duration,
    dirty_views: usize,
) {
    if !enabled() {
        return;
    }

    let total = start.elapsed();
    let stats = STATS.get_or_init(|| Mutex::new(Stats::new()));
    let Ok(mut stats) = stats.lock() else {
        return;
    };

    let interval_us = stats
        .last_frame_start
        .replace(start)
        .map(|previous| start.saturating_duration_since(previous).as_micros() as u64)
        .unwrap_or(0);

    let record = FrameRecord {
        draw_us: draw.as_micros() as u64,
        prepaint_us: prepaint.as_micros() as u64,
        paint_us: paint.as_micros() as u64,
        present_us: present.as_micros() as u64,
        total_us: total.as_micros() as u64,
        interval_us,
        dirty_views: dirty_views.min(u32::MAX as usize) as u32,
    };

    stats.drawn_frames += 1;
    if record.total_us > target_us() {
        stats.missed += 1;
    }
    if stats.frames.len() == CAPACITY {
        stats.frames.pop_front();
    }
    stats.frames.push_back(record);

    if stats.drawn_frames % REPORT_FRAMES == 0 {
        report(&stats);
    }
}

fn report(stats: &Stats) {
    let target = target_us();
    // 只统计最近一个报告窗口：长累计窗口会把启动、页面切换等尖峰永久
    // 留在分位数里，无法反映“当前场景”的稳态帧成本。
    let window: Vec<FrameRecord> = stats
        .frames
        .iter()
        .rev()
        .take(REPORT_FRAMES as usize)
        .copied()
        .collect();
    let mut draw: Vec<u64> = Vec::with_capacity(window.len());
    let mut prepaint: Vec<u64> = Vec::with_capacity(window.len());
    let mut paint: Vec<u64> = Vec::with_capacity(window.len());
    let mut present: Vec<u64> = Vec::with_capacity(window.len());
    let mut total: Vec<u64> = Vec::with_capacity(window.len());
    let mut interval: Vec<u64> = Vec::with_capacity(window.len());
    let mut dirty_max = 0u32;
    let mut window_miss = 0u64;
    for frame in &window {
        draw.push(frame.draw_us);
        prepaint.push(frame.prepaint_us);
        paint.push(frame.paint_us);
        present.push(frame.present_us);
        total.push(frame.total_us);
        if frame.interval_us > 0 {
            interval.push(frame.interval_us);
        }
        if frame.total_us > target {
            window_miss += 1;
        }
        dirty_max = dirty_max.max(frame.dirty_views);
    }

    let (draw_p50, draw_p95, draw_p99, draw_max) = percentiles(&mut draw);
    let (pre_p50, pre_p95, pre_p99, pre_max) = percentiles(&mut prepaint);
    let (paint_p50, paint_p95, paint_p99, paint_max) = percentiles(&mut paint);
    let (present_p50, present_p95, present_p99, present_max) = percentiles(&mut present);
    let (total_p50, total_p95, total_p99, total_max) = percentiles(&mut total);
    let (interval_p50, interval_p95, _, _) = percentiles(&mut interval);

    let effective_hz = if interval_p50 > 0 {
        1_000_000.0 / interval_p50 as f64
    } else {
        0.0
    };

    log::info!(
        target: "gpui::frame",
        "frames={} window={} target={:.1}Hz effective={:.1}Hz miss={}/{} \
         draw_us(p50/p95/p99/max)={}/{}/{}/{} \
         prepaint_us(p50/p95/p99/max)={}/{}/{}/{} \
         paint_us(p50/p95/p99/max)={}/{}/{}/{} \
         present_us(p50/p95/p99/max)={}/{}/{}/{} \
         total_us(p50/p95/p99/max)={}/{}/{}/{} \
         interval_us(p50/p95)={}/{} dirty_views_max={}",
        stats.drawn_frames,
        window.len(),
        1_000_000.0 / target as f64,
        effective_hz,
        window_miss,
        stats.missed,
        draw_p50,
        draw_p95,
        draw_p99,
        draw_max,
        pre_p50,
        pre_p95,
        pre_p99,
        pre_max,
        paint_p50,
        paint_p95,
        paint_p99,
        paint_max,
        present_p50,
        present_p95,
        present_p99,
        present_max,
        total_p50,
        total_p95,
        total_p99,
        total_max,
        interval_p50,
        interval_p95,
        dirty_max,
    );
}

fn percentiles(values: &mut [u64]) -> (u64, u64, u64, u64) {
    if values.is_empty() {
        return (0, 0, 0, 0);
    }
    values.sort_unstable();
    let at = |p: f64| {
        let index = ((values.len() as f64 - 1.0) * p).round() as usize;
        values[index.min(values.len() - 1)]
    };
    (at(0.50), at(0.95), at(0.99), values[values.len() - 1])
}
