//! AMLL 等价实现的复杂度护栏。
//!
//! 用 `chordal_core::perf` 的检查点做 per-item / per-call 归一化：
//! O(n) 算法的每项耗时应随 4× 数据保持恒定；O(n²) 会变成 4×。
//! 这些测试在 debug 构建下运行，容差比 release 宽，但仍能拦住二次退化。

use std::sync::{Mutex, MutexGuard};

use chordal_core::perf;
use chordial_amll_gpui::model::{LyricLine, LyricWord};
use chordial_amll_gpui::parser::parse_lrc;
use chordial_amll_gpui::player::{LayoutMetrics, LyricPlayer, LyricPlayerConfig};

/// perf 账本是进程级全局状态，本文件内的多档测量必须串行，否则
/// `perf::clear()` 会清掉其它测试正在积累的事件。
static PERF_LOCK: Mutex<()> = Mutex::new(());

fn perf_lock() -> MutexGuard<'static, ()> {
    PERF_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lrc(n: usize) -> String {
    let mut out = String::with_capacity(n * 24);
    for i in 0..n {
        let ms = (i as u64) * 1_000;
        let (m, s, f) = (ms / 60_000, (ms / 1_000) % 60, ms % 1_000);
        out.push_str(&format!("[{m:02}:{s:02}.{f:03}]line {i}\n"));
    }
    out
}

fn lines(n: usize) -> Vec<LyricLine> {
    (0..n)
        .map(|i| {
            let start = i as u64 * 1_000;
            LyricLine {
                words: vec![LyricWord::new(format!("line {i}"), start, start + 900)],
                start_time: start,
                end_time: start + 900,
                ..Default::default()
            }
        })
        .collect()
}

fn metrics(n: usize) -> LayoutMetrics {
    LayoutMetrics {
        width: 1_200.0,
        height: 800.0,
        base_font_size: 30.0,
        padding_x: 30.0,
        line_heights: vec![90.0; n],
    }
}

fn parser_per_item(n: usize) -> f64 {
    let raw = lrc(n);
    // 预热：让分配器/页缓存稳定，避免第一档数据被低估。
    let _ = parse_lrc(&raw);
    perf::clear();
    let parsed = parse_lrc(&raw);
    assert_eq!(parsed.len(), n);
    let (events, total) = perf::total_for("amll.parse");
    assert!(events >= 1);
    total as f64 / n as f64
}

#[test]
fn parse_is_linear() {
    let _guard = perf_lock();
    let p1 = parser_per_item(2_000);
    let p2 = parser_per_item(8_000);
    let p3 = parser_per_item(32_000);
    // 4× 数据，每项耗时不超过 2.5×。
    assert!(p2 < p1 * 2.5, "parse per-item 2k→8k: {p1:.3}us → {p2:.3}us");
    assert!(
        p3 < p2 * 2.5,
        "parse per-item 8k→32k: {p2:.3}us → {p3:.3}us"
    );
}

fn document_per_item(n: usize) -> f64 {
    let raw = lrc(n);
    let _ = chordial_amll_gpui::document::process(parse_lrc(&raw));
    let parsed = parse_lrc(&raw);
    perf::clear();
    let doc = chordial_amll_gpui::document::process(parsed);
    assert_eq!(doc.lines.len(), n);
    let (events, total) = perf::total_for("amll.document");
    assert!(events >= 1);
    total as f64 / n as f64
}

#[test]
fn document_processing_is_linear() {
    let _guard = perf_lock();
    let p1 = document_per_item(2_000);
    let p2 = document_per_item(8_000);
    let p3 = document_per_item(32_000);
    assert!(
        p2 < p1 * 3.2,
        "document per-item 2k→8k: {p1:.3}us → {p2:.3}us"
    );
    assert!(
        p3 < p2 * 3.2,
        "document per-item 8k→32k: {p2:.3}us → {p3:.3}us"
    );
}

fn set_time_per_call(n: usize) -> f64 {
    let mut player = LyricPlayer::new(LyricPlayerConfig::default());
    player.set_lyrics(lines(n), 0);
    player.set_metrics(metrics(n));
    // 停在中段同一行内，确保不触发 calc_layout。
    let t = (n / 2) as u64 * 1_000 + 500;
    player.set_time(t, true);
    perf::clear();
    let calls = 2_000u64;
    let start = std::time::Instant::now();
    for k in 0..calls {
        player.set_time(t + (k & 1), false);
    }
    let per_call = start.elapsed().as_secs_f64() * 1e6 / calls as f64;
    let (events, _) = perf::total_for("amll.set_time");
    assert_eq!(events, calls, "每次 set_time 都必须记录检查点");
    per_call
}

#[test]
fn set_time_is_sublinear_in_line_count() {
    let _guard = perf_lock();
    let p1 = set_time_per_call(1_000);
    let p16 = set_time_per_call(16_000);
    // 16× 歌词行：二分 + 前缀最大结束时间，不应该是 16×。
    // 单次调用可能低于 perf 的微秒分辨率，这里用墙钟并留出噪声下限。
    assert!(
        p16 <= (p1 * 4.0).max(0.25),
        "set_time per-call 1k→16k: {p1:.3}us → {p16:.3}us"
    );
}

fn update_per_frame(n: usize) -> f64 {
    let mut player = LyricPlayer::new(LyricPlayerConfig::default());
    player.set_lyrics(lines(n), 0);
    player.set_metrics(metrics(n));
    let t = (n / 2) as u64 * 1_000 + 100;
    player.set_time(t, true);
    perf::clear();
    let frames = 120u64;
    let start = std::time::Instant::now();
    for _ in 0..frames {
        player.update(16.0);
    }
    let per_frame = start.elapsed().as_secs_f64() * 1e6 / frames as f64;
    let (events, _) = perf::total_for("amll.update");
    assert!(events >= 1);
    per_frame
}

#[test]
fn per_frame_update_depends_on_visible_lines_not_total() {
    let _guard = perf_lock();
    let small = update_per_frame(1_000);
    let large = update_per_frame(16_000);
    assert!(
        large <= (small * 3.0).max(0.5),
        "update per-frame 1k→16k: {small:.3}us → {large:.3}us (must be O(visible))"
    );
}

#[test]
fn layout_has_no_runaway_superlinear_cost() {
    let _guard = perf_lock();
    for n in [2_000usize, 8_000, 32_000] {
        let mut player = LyricPlayer::new(LyricPlayerConfig::default());
        player.set_lyrics(lines(n), 0);
        perf::clear();
        player.set_metrics(metrics(n));
        let (events, total) = perf::total_for("amll.layout");
        assert!(events >= 1, "layout checkpoint missing for n={n}");
        let per_item = total as f64 / n as f64;
        assert!(
            per_item < 6.0,
            "layout per-item too high at n={n}: {per_item:.3}us"
        );
    }
}
