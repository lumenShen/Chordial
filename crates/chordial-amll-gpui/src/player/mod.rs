//! 歌词播放器的布局与滚动状态机 —— AMLL `LyricPlayerBase` 的 Rust 等价实现。
//!
//! 设计要点（与 AMLL 1:1 的部分）：
//! - **热行 / 缓冲行**语义完全一致：决定当前高亮、缩放、模糊与滚动目标；
//! - 每行位移与缩放使用解析弹簧，延迟逐行递增，形成"逐行跟随"的层次感；
//! - 间奏检测（≥4s 且下一行有前 250ms 留白）与呼吸点动画参数一致；
//! - 对齐锚点 / 对齐位置 / 背景行合并 / 隐藏已播放行等配置语义一致。
//!
//! 与 DOM 版不同的只是交付方式：这里不产生 DOM，而是把每帧结果写入
//! [`PlayerFrame`]，由渲染层（`render.rs`）按帧读取。状态机本身零分配
//! （帧缓冲、活跃行缓冲、前缀和全部复用），`update` 只遍历可见行，
//! 因此每帧成本 O(可见行)，与歌词总行数无关。

use chordal_core::perf::PerfScope;
use rustc_hash::FxHashSet;

use crate::document;
use crate::model::{LyricDocument, LyricLine};
use crate::spring::{Spring, SpringParams};

mod api;
mod update;
mod interlude;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignAnchor {
    Top,
    #[default]
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LyricPlayerConfig {
    pub align_anchor: AlignAnchor,
    /// 对齐位置占视口高度的比例（0..=1）。
    pub align_position: f32,
    pub enable_blur: bool,
    pub enable_scale: bool,
    pub hide_passed_lines: bool,
    /// 视口外预渲染距离（像素）。
    pub overscan_px: f32,
    pub base_font_size: f32,
    /// 非逐词（普通 LRC）非当前行的不透明度；AMLL 默认 0.2。
    pub non_dynamic_opacity: f32,
}

impl Default for LyricPlayerConfig {
    fn default() -> Self {
        Self {
            align_anchor: AlignAnchor::Center,
            align_position: 0.35,
            enable_blur: true,
            enable_scale: true,
            hide_passed_lines: false,
            overscan_px: 300.0,
            base_font_size: 24.0,
            non_dynamic_opacity: 0.2,
        }
    }
}

/// 渲染层提供的布局尺寸。`line_heights` 由文本测量得到（含上下 padding 与副行）。
#[derive(Debug, Clone, Default)]
pub struct LayoutMetrics {
    pub width: f32,
    pub height: f32,
    pub base_font_size: f32,
    /// 歌词文本的左右内边距（`--lyric-line-padding-x`），间奏点与歌词左/右对齐用。
    pub padding_x: f32,
    pub line_heights: Vec<f32>,
}

impl LayoutMetrics {
    #[inline]
    fn height_at(&self, i: usize) -> f32 {
        self.line_heights
            .get(i)
            .copied()
            .filter(|h| *h > 0.0)
            .unwrap_or(self.height / 5.0)
    }
}

/// 单行每帧输出（坐标相对歌词组件左上角）。
#[derive(Debug, Clone, Copy, Default)]
pub struct LineFrameState {
    pub top: f32,
    pub scale: f32,
    pub opacity: f32,
    pub blur: f32,
    pub visible: bool,
    /// 该行处于热行/缓冲行集合（AMLL `enable()` 状态）。
    ///
    /// 逐词浮动动画只在 enabled 时前进；离开时 AMLL 以 `playbackRate = -1`
    /// 让它回退，渲染层据此决定浮动相位是正放还是倒放。
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InterludeDotsFrame {
    /// 点组内容（不含 padding）左侧在播放器坐标中的 x。
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub dot_opacity: [f32; 3],
    /// 点组内容宽度（3 个点 + 2 个 gap，不含左右 padding）。
    pub width: f32,
    pub height: f32,
    pub dot_size: f32,
    pub gap: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InterludeFrame {
    pub start_ms: u64,
    pub end_ms: u64,
    /// 间奏前的最后一行下标；-1 表示歌曲开头。
    pub line_index: i64,
    pub next_duet: bool,
    pub dots: InterludeDotsFrame,
}

/// 一帧的完整快照（借用自播放器，不拷贝）。
#[derive(Debug, Default)]
pub struct PlayerFrame {
    pub lines: Vec<LineFrameState>,
    pub interlude: Option<InterludeFrame>,
    pub scroll_boundary: [f32; 2],
    pub scroll_offset: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct LineTarget {
    top: f32,
    scale: f32,
    opacity: f32,
    blur: f32,
    visible: bool,
}

struct LineSprings {
    pos_y: Spring,
    scale: Spring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InterludeInfo {
    start: u64,
    end: u64,
    index: i64,
    next_duet: bool,
}

#[derive(Debug, Clone, Copy)]
struct InterludeAnim {
    current_time: f32,
    scale: f32,
    dot_opacity: [f32; 3],
}

impl Default for InterludeAnim {
    fn default() -> Self {
        Self {
            current_time: 0.0,
            scale: 0.0,
            dot_opacity: [0.0; 3],
        }
    }
}

pub struct LyricPlayer {
    config: LyricPlayerConfig,
    doc: LyricDocument,
    metrics: LayoutMetrics,
    current_time: u64,
    last_current_time: u64,
    hot: FxHashSet<usize>,
    buffered: FxHashSet<usize>,
    scroll_to_index: usize,
    target_align_index: usize,
    scroll_offset: f32,
    scroll_boundary: [f32; 2],
    is_playing: bool,
    is_seeking: bool,
    initial_layout_finished: bool,
    prefix_all: Vec<f32>,
    prefix_no_bg: Vec<f32>,
    prefix_max_end: Vec<u64>,
    prefix_dirty: bool,
    springs: Vec<LineSprings>,
    targets: Vec<LineTarget>,
    frame: PlayerFrame,
    active_scratch: Vec<usize>,
    added_scratch: Vec<usize>,
    removed_scratch: Vec<usize>,
    /// 需要逐帧推进弹簧的行（可见行 + 尚未收敛的行）。
    animated: FxHashSet<usize>,
    anim_scratch: Vec<usize>,
    /// `update` 中判定仍需动画的行；保留容量，避免每帧重建 HashSet。
    anim_keep: Vec<usize>,
    interlude: Option<InterludeInfo>,
    interlude_anim: InterludeAnim,
    visible_start: usize,
    visible_end: usize,
}

impl Default for LyricPlayer {
    fn default() -> Self {
        Self::new(LyricPlayerConfig::default())
    }
}


fn smooth_over(
    opacity: f32,
    blur: f32,
    target_opacity: f32,
    target_blur: f32,
    dt: f32,
) -> (f32, f32) {
    // AMLL 用 CSS transition（opacity .25s / filter .2s）。这里用临界阻尼的一阶
    // 平滑近似，时间常数 80ms，视觉上同样平滑且无需额外状态。
    let k = 1.0 - (-dt / 0.08).exp();
    (
        opacity + (target_opacity - opacity) * k,
        blur + (target_blur - blur) * k,
    )
}

fn ease_out_expo(x: f32) -> f32 {
    if x >= 1.0 {
        1.0
    } else {
        1.0 - 2f32.powf(-10.0 * x)
    }
}

fn ease_in_out_back(x: f32) -> f32 {
    let c1 = 1.70158f32;
    let c2 = c1 * 1.525;
    if x < 0.5 {
        ((2.0 * x).powi(2) * ((c2 + 1.0) * 2.0 * x - c2)) / 2.0
    } else {
        ((2.0 * x - 2.0).powi(2) * ((c2 + 1.0) * (x * 2.0 - 2.0) + c2) + 2.0) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LyricWord;

    fn doc(n: usize, step: u64) -> Vec<LyricLine> {
        (0..n)
            .map(|i| {
                let t = i as u64 * step;
                LyricLine {
                    words: vec![LyricWord::new(format!("line {i}"), t, t + step - 100)],
                    start_time: t,
                    end_time: t + step - 100,
                    ..Default::default()
                }
            })
            .collect()
    }

    fn metrics(n: usize) -> LayoutMetrics {
        LayoutMetrics {
            width: 1000.0,
            height: 600.0,
            base_font_size: 24.0,
            padding_x: 24.0,
            line_heights: vec![80.0; n],
        }
    }

    #[test]
    fn initial_layout_centers_active_line() {
        let mut p = LyricPlayer::new(LyricPlayerConfig::default());
        p.set_lyrics(doc(10, 2_000), 0);
        p.set_metrics(metrics(10));
        let target = p.debug_target(0).unwrap();
        // 第一行中心应接近视口 35% 处。
        let center = target.0 + 40.0;
        assert!((center - 210.0).abs() < 1.0, "center = {center}");
    }

    #[test]
    fn advances_hot_line_on_time() {
        let mut p = LyricPlayer::new(LyricPlayerConfig::default());
        p.set_lyrics(doc(10, 2_000), 0);
        p.set_metrics(metrics(10));
        assert_eq!(p.hot.len(), 1);
        p.set_time(2_100, false);
        assert_eq!(p.hot.len(), 1);
        assert!(p.hot.contains(&1));
        assert!(p.buffered.contains(&1));
    }

    #[test]
    fn seek_jumps_without_animation() {
        let mut p = LyricPlayer::new(LyricPlayerConfig::default());
        p.set_lyrics(doc(100, 2_000), 0);
        p.set_metrics(metrics(100));
        p.set_time(100_000, true);
        assert_eq!(p.scroll_to_index, 50);
        let target = p.debug_target(50).unwrap();
        assert!((target.0 + 40.0 - 210.0).abs() < 1.0);
    }

    #[test]
    fn update_only_touches_visible_lines() {
        let mut p = LyricPlayer::new(LyricPlayerConfig::default());
        p.set_lyrics(doc(10_000, 2_000), 0);
        p.set_metrics(metrics(10_000));
        // 10k 行时准备耗时是 O(n)，但每帧 update 只处理可见范围。
        let start = std::time::Instant::now();
        for _ in 0..600 {
            p.update(16.0);
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 500,
            "600 frames took {elapsed:?} (visible-only update guard)"
        );
    }

    #[test]
    fn line_time_at_y_hits_visible_lines() {
        let mut p = LyricPlayer::new(LyricPlayerConfig::default());
        p.set_lyrics(doc(10, 2_000), 0);
        p.set_metrics(metrics(10));
        let (top, ..) = p.debug_target(3).unwrap();
        assert_eq!(p.line_time_at_y(top + 1.0), Some(5_900));
        assert_eq!(p.line_time_at_y(-10_000.0), None);
    }

    #[test]
    fn interlude_is_detected() {
        let mut lines = Vec::new();
        for i in 0..3u64 {
            let t = i * 10_000;
            lines.push(LyricLine {
                words: vec![LyricWord::new("x", t, t + 2_000)],
                start_time: t,
                end_time: t + 2_000,
                ..Default::default()
            });
        }
        let mut p = LyricPlayer::new(LyricPlayerConfig::default());
        p.set_lyrics(lines, 0);
        p.set_metrics(LayoutMetrics {
            width: 1000.0,
            height: 600.0,
            base_font_size: 24.0,
            padding_x: 24.0,
            line_heights: vec![80.0; 3],
        });
        p.set_time(5_000, false);
        assert!(p.frame().interlude.is_some());
    }

    #[test]
    fn interlude_dots_follow_lyric_padding_and_duet_alignment() {
        let build = |duet: bool| {
            let mut p = LyricPlayer::new(LyricPlayerConfig::default());
            let lines = vec![
                LyricLine {
                    words: vec![LyricWord::new("a", 0, 2_000)],
                    start_time: 0,
                    end_time: 2_000,
                    ..Default::default()
                },
                LyricLine {
                    words: vec![LyricWord::new("b", 10_000, 12_000)],
                    start_time: 10_000,
                    end_time: 12_000,
                    is_duet: duet,
                    ..Default::default()
                },
            ];
            p.set_lyrics(lines, 0);
            p.set_metrics(LayoutMetrics {
                width: 1_000.0,
                height: 600.0,
                base_font_size: 24.0,
                padding_x: 24.0,
                line_heights: vec![80.0; 2],
            });
            p.set_time(5_000, false);
            p
        };
        // 普通行：点组内容左边缘 = 歌词文本左边缘（padding_x）。
        let normal = build(false);
        let dots = normal.frame().interlude.expect("interlude").dots;
        assert!((dots.x - 24.0).abs() < 1e-4, "x = {}", dots.x);
        // 对唱行：点组内容右边缘 = 歌词文本右边缘（width - padding_x）。
        let duet = build(true);
        let dots = duet.frame().interlude.expect("interlude").dots;
        assert!(
            (dots.x + dots.width - (1_000.0 - 24.0)).abs() < 1e-4,
            "right = {}",
            dots.x + dots.width
        );
    }
}
