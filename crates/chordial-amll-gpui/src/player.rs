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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignAnchor {
    Top,
    #[default]
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy)]
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
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub dot_opacity: [f32; 3],
    pub width: f32,
    pub height: f32,
    pub dot_size: f32,
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

impl LyricPlayer {
    pub fn new(config: LyricPlayerConfig) -> Self {
        Self {
            config,
            doc: LyricDocument::default(),
            metrics: LayoutMetrics::default(),
            current_time: 0,
            last_current_time: 0,
            hot: FxHashSet::default(),
            buffered: FxHashSet::default(),
            scroll_to_index: 0,
            target_align_index: 0,
            scroll_offset: 0.0,
            scroll_boundary: [0.0, 0.0],
            is_playing: true,
            is_seeking: false,
            initial_layout_finished: false,
            prefix_all: Vec::new(),
            prefix_no_bg: Vec::new(),
            prefix_max_end: Vec::new(),
            prefix_dirty: true,
            springs: Vec::new(),
            targets: Vec::new(),
            frame: PlayerFrame::default(),
            active_scratch: Vec::new(),
            added_scratch: Vec::new(),
            removed_scratch: Vec::new(),
            animated: FxHashSet::default(),
            anim_scratch: Vec::new(),
            interlude: None,
            interlude_anim: InterludeAnim::default(),
            visible_start: 0,
            visible_end: 0,
        }
    }

    #[inline]
    pub fn config(&self) -> &LyricPlayerConfig {
        &self.config
    }

    /// 运行时修改配置。会重新布局，O(n) 一次。
    pub fn set_config(&mut self, config: LyricPlayerConfig) {
        let playing_changed = self.is_playing;
        self.config = config;
        if playing_changed {
            self.prefix_dirty = true;
        }
        self.calc_layout(false);
    }

    #[inline]
    pub fn document(&self) -> &LyricDocument {
        &self.doc
    }

    #[inline]
    pub fn frame(&self) -> &PlayerFrame {
        &self.frame
    }

    #[inline]
    pub fn current_time(&self) -> u64 {
        self.current_time
    }

    #[inline]
    pub fn scroll_to_index(&self) -> usize {
        self.scroll_to_index
    }

    #[inline]
    pub fn is_playing(&self) -> bool {
        self.is_playing
    }

    /// 当前应参与绘制的行范围（含 overscan，连续且有序）。
    #[inline]
    pub fn visible_range(&self) -> std::ops::Range<usize> {
        self.visible_start..self.visible_end
    }

    /// 点击歌词区 y 坐标（相对组件顶部）时，返回该行的开始时间。
    /// 与 AMLL dom-slim 的点击行跳转一致（后续 +50ms 由调用方决定）。
    pub fn line_time_at_y(&self, y: f32) -> Option<u64> {
        let range = self.visible_range();
        for i in range {
            // 用当前绘制位置/缩放做命中，滚动动画中不会跳错相邻行。
            let frame = self.frame.lines.get(i)?;
            let top = frame.top;
            let height = (self.metrics.height_at(i) * frame.scale / 100.0).max(1.0);
            if y >= top && y < top + height {
                return self.doc.lines.get(i).map(|line| line.start_time);
            }
        }
        None
    }

    /// 设置歌词。`initial_time` 用于挂载时对齐到当前播放进度。
    ///
    /// 该操作会重置所有行状态；随后需调用 [`Self::set_metrics`] 提供测量结果。
    pub fn set_lyrics(&mut self, lines: Vec<LyricLine>, initial_time: u64) {
        self.doc = document::process(lines);
        self.current_time = initial_time;
        self.last_current_time = initial_time;
        self.hot.clear();
        self.buffered.clear();
        self.scroll_to_index = 0;
        self.target_align_index = 0;
        self.scroll_offset = 0.0;
        self.initial_layout_finished = false;
        self.interlude = None;
        self.interlude_anim = InterludeAnim::default();
        self.animated.clear();

        let n = self.doc.lines.len();
        self.springs.clear();
        self.springs.reserve(n);
        self.targets.clear();
        self.targets.resize(
            n,
            LineTarget {
                scale: 100.0,
                ..Default::default()
            },
        );
        self.frame.lines.clear();
        self.frame.lines.resize(
            n,
            LineFrameState {
                scale: 100.0,
                opacity: 1.0,
                ..Default::default()
            },
        );
        self.frame.interlude = None;
        self.visible_start = 0;
        self.visible_end = 0;
        for _ in 0..n {
            self.springs.push(LineSprings {
                pos_y: Spring::new(0.0).with_params(SpringParams::new(0.9, 15.0, 90.0)),
                scale: Spring::new(100.0).with_params(SpringParams::new(2.0, 25.0, 100.0)),
            });
        }

        self.metrics.line_heights.clear();
        self.metrics.line_heights.resize(n, 0.0);
        self.prefix_dirty = true;
        self.rebuild_prefix();

        // 与 AMLL 相同：先按初始时间找出活跃行，再强制布局一次。
        self.set_time(initial_time, true);
    }

    /// 更新尺寸 / 行高（文本测量结果）。行数必须与歌词行数对齐。
    pub fn set_metrics(&mut self, metrics: LayoutMetrics) {
        let force = !self.initial_layout_finished;
        if metrics.line_heights.len() != self.doc.lines.len() {
            let mut heights = metrics.line_heights;
            heights.resize(self.doc.lines.len(), 0.0);
            self.metrics = LayoutMetrics {
                line_heights: heights,
                ..metrics
            };
        } else {
            self.metrics = metrics;
        }
        self.prefix_dirty = true;
        self.calc_layout(force);
        self.initial_layout_finished = true;
    }

    pub fn set_playing(&mut self, playing: bool) {
        if self.is_playing == playing {
            return;
        }
        self.is_playing = playing;
        self.prefix_dirty = true;
        // AMLL 暂停时把背景行高度计入布局。
        self.calc_layout(false);
    }

    /// 更新播放进度。`seek` 为真时立即重置滚动目标（不做追赶动画）。
    pub fn set_time(&mut self, time_ms: u64, seek: bool) {
        let _scope = PerfScope::enter("amll.set_time");
        self.current_time = time_ms;
        if !self.initial_layout_finished && !seek {
            return;
        }
        self.is_seeking = seek;

        self.removed_scratch.clear();
        self.added_scratch.clear();

        // 1. 移除已过期的热行（与 AMLL 的 BG 成对判断一致）。
        for &id in &self.hot {
            let Some(line) = self.doc.lines.get(id) else {
                self.removed_scratch.push(id);
                continue;
            };
            if line.is_bg {
                continue;
            }
            if let Some(next) = self.doc.lines.get(id + 1)
                && next.is_bg
            {
                let next_main_start = self
                    .doc
                    .lines
                    .get(id + 2)
                    .map(|l| l.start_time)
                    .unwrap_or(u64::MAX);
                let start = line.start_time.min(next.start_time);
                // 与 AMLL 完全一致：
                // min(max(line.end, nextMain.start ?? MAX), max(line.end, next.end))
                let end = line
                    .end_time
                    .max(next_main_start)
                    .min(line.end_time.max(next.end_time));
                if start > time_ms || end <= time_ms {
                    self.removed_scratch.push(id);
                    self.removed_scratch.push(id + 1);
                }
                continue;
            }
            if line.start_time > time_ms || line.end_time <= time_ms {
                self.removed_scratch.push(id);
            }
        }
        for &id in &self.removed_scratch {
            self.hot.remove(&id);
        }
        self.removed_scratch.clear();

        // 2. 找出当前活跃的非背景行。二分 + 前缀最大结束时间，
        //    只扫描真正可能活跃的行，避免每帧 O(n)。
        self.collect_active(time_ms);
        for k in 0..self.active_scratch.len() {
            let id = self.active_scratch[k];
            if !self.hot.contains(&id) {
                self.hot.insert(id);
                self.added_scratch.push(id);
            }
            if let Some(next) = self.doc.lines.get(id + 1)
                && next.is_bg
                && self.hot.insert(id + 1)
            {
                self.added_scratch.push(id + 1);
            }
        }

        // 3. 缓冲行中不再热的行进入待移除集合。
        for &id in &self.buffered {
            if !self.hot.contains(&id) {
                self.removed_scratch.push(id);
            }
        }

        if seek {
            self.buffered.clear();
            self.buffered.extend(self.hot.iter().copied());
            self.scroll_to_index = if self.buffered.is_empty() {
                let idx = self.doc.lines.partition_point(|l| l.start_time < time_ms);
                idx.min(self.doc.lines.len())
            } else {
                self.buffered.iter().copied().min().unwrap_or(0)
            };
            self.reset_scroll();
            let force = !self.initial_layout_finished;
            self.calc_layout(force);
        } else if !self.removed_scratch.is_empty() || !self.added_scratch.is_empty() {
            if self.removed_scratch.is_empty() {
                // 只有新增：全部进入缓冲，滚动目标取最小缓冲行。
                for &id in &self.added_scratch {
                    self.buffered.insert(id);
                }
                if let Some(min) = self.buffered.iter().copied().min() {
                    self.scroll_to_index = min;
                }
                self.calc_layout(false);
            } else if self.added_scratch.is_empty() {
                // 只有移除：仅当移除集合覆盖全部缓冲行时才真正解除并重排。
                let covers = self
                    .removed_scratch
                    .iter()
                    .all(|id| self.buffered.contains(id))
                    && self.removed_scratch.len() == self.buffered.len();
                if covers {
                    for &id in &self.removed_scratch {
                        if !self.hot.contains(&id) {
                            self.buffered.remove(&id);
                        }
                    }
                    self.calc_layout(false);
                }
            } else {
                for &id in &self.added_scratch {
                    self.buffered.insert(id);
                }
                for &id in &self.removed_scratch {
                    self.buffered.remove(&id);
                }
                if let Some(min) = self.buffered.iter().copied().min() {
                    self.scroll_to_index = min;
                }
                self.calc_layout(false);
            }
        }

        self.added_scratch.clear();
        self.removed_scratch.clear();
        self.last_current_time = time_ms;
        self.is_seeking = false;
        _scope.finish(self.hot.len() as u64);
    }

    /// 推进动画。返回是否仍在动（渲染层据此决定是否请求下一帧）。
    pub fn update(&mut self, delta_ms: f32) -> bool {
        if self.doc.lines.is_empty() {
            return false;
        }
        let _scope = PerfScope::enter("amll.update");
        let dt = (delta_ms / 1000.0).clamp(0.0, 0.1);
        let mut animating = false;
        let mut updated = 0u64;

        // 只遍历"需要动画"的行集合（可见 + 未收敛），每帧 O(可见行)。
        // 集合由 calc_layout 重建；不可见的行在布局时直接吸附到目标位置，
        // 因此不会出现进入视口时的长距离追赶。
        let current = std::mem::take(&mut self.animated);
        self.anim_scratch.clear();
        self.anim_scratch.extend(current.iter().copied());
        self.anim_scratch.sort_unstable();
        for &i in &self.anim_scratch {
            let Some(frame_line) = self.frame.lines.get_mut(i) else {
                continue;
            };
            let Some(target) = self.targets.get(i).copied() else {
                continue;
            };
            let Some(springs) = self.springs.get_mut(i) else {
                continue;
            };
            springs.pos_y.update(dt as f64);
            springs.scale.update(dt as f64);
            frame_line.top = springs.pos_y.get_current_position() as f32;
            frame_line.scale = springs.scale.get_current_position() as f32;
            updated += 1;
            let spring_anim = !springs.pos_y.arrived() || !springs.scale.arrived();
            animating |= spring_anim;
            if frame_line.visible || (frame_line.opacity - target.opacity).abs() > 0.001 {
                let (opacity, blur) = smooth_over(
                    frame_line.opacity,
                    frame_line.blur,
                    target.opacity,
                    target.blur,
                    dt,
                );
                frame_line.opacity = opacity;
                frame_line.blur = blur;
                animating |=
                    (opacity - target.opacity).abs() > 0.001 || (blur - target.blur).abs() > 0.01;
            }
            if frame_line.visible || spring_anim {
                self.animated.insert(i);
            }
        }

        if let Some(interlude) = self.interlude {
            self.update_interlude_anim(dt * 1000.0, interlude);
            animating |= self.is_playing;
            if let Some(frame) = self.frame.interlude.as_mut() {
                frame.dots.scale = self.interlude_anim.scale;
                frame.dots.dot_opacity = self.interlude_anim.dot_opacity;
            }
        }

        self.frame.scroll_boundary = self.scroll_boundary;
        self.frame.scroll_offset = self.scroll_offset;
        _scope.finish(updated);
        animating
    }

    fn collect_active(&mut self, time: u64) {
        self.active_scratch.clear();
        let lines = &self.doc.lines;
        let first = lines.partition_point(|l| l.start_time <= time);
        let mut i = first;
        while i > 0 {
            i -= 1;
            if self.prefix_max_end.get(i).copied().unwrap_or(0) <= time {
                break;
            }
            let line = &lines[i];
            if !line.is_bg && line.start_time <= time && line.end_time > time {
                self.active_scratch.push(i);
            }
        }
        self.active_scratch.reverse();
    }

    fn rebuild_prefix(&mut self) {
        if !self.prefix_dirty {
            return;
        }
        let n = self.doc.lines.len();
        self.prefix_all.clear();
        self.prefix_all.reserve(n + 1);
        self.prefix_no_bg.clear();
        self.prefix_no_bg.reserve(n + 1);
        self.prefix_max_end.clear();
        self.prefix_max_end.reserve(n);

        self.prefix_all.push(0.0);
        self.prefix_no_bg.push(0.0);
        let mut sum_all = 0.0f32;
        let mut sum_no_bg = 0.0f32;
        let mut max_end = 0u64;
        for (i, line) in self.doc.lines.iter().enumerate() {
            let h = self.metrics.height_at(i);
            sum_all += h;
            if !line.is_bg {
                sum_no_bg += h;
            }
            max_end = max_end.max(line.end_time);
            self.prefix_all.push(sum_all);
            self.prefix_no_bg.push(sum_no_bg);
            self.prefix_max_end.push(max_end);
        }
        self.prefix_dirty = false;
    }

    /// 计算所有行的目标位置/缩放/透明度/模糊。O(n)，只在换行/跳转/尺寸变化时调用。
    fn calc_layout(&mut self, force: bool) {
        if self.doc.lines.is_empty() || self.metrics.height <= 1.0 || self.metrics.width <= 1.0 {
            return;
        }
        let _scope = PerfScope::enter("amll.layout");
        self.rebuild_prefix();

        let interlude = self.get_current_interlude();
        self.set_interlude(interlude);

        let font_size = if self.metrics.base_font_size > 0.0 {
            self.metrics.base_font_size
        } else {
            self.config.base_font_size
        };
        let dot_margin = font_size * 0.4;
        let dot_size = (font_size * 0.5)
            .clamp(0.0, 1e9)
            .min((self.metrics.height * 0.01).max(font_size * 0.5))
            .min(font_size * 3.0)
            .max(font_size * 0.5);
        let dots_padding_y = self.metrics.width * 0.025;
        let dots_height = dot_size + dots_padding_y * 2.0;
        let dots_width =
            dot_size * 3.0 + font_size * 0.25 * 2.0 + 3.0 * 4.0 + font_size * 0.75 * 2.0;
        let total_interlude_height = dots_height + dot_margin * 2.0;

        let target_align_index = self.scroll_to_index.min(self.doc.lines.len());
        self.target_align_index = target_align_index;
        let mut cur_pos = -self.scroll_offset;

        if let Some(inter) = interlude
            && inter.index != -1
        {
            cur_pos -= total_interlude_height;
        }

        let scroll_offset = if self.is_playing {
            self.prefix_no_bg
                .get(target_align_index)
                .copied()
                .unwrap_or(0.0)
        } else {
            self.prefix_all
                .get(target_align_index)
                .copied()
                .unwrap_or(0.0)
        };
        self.scroll_boundary[0] = -scroll_offset;
        cur_pos -= scroll_offset;
        cur_pos += self.metrics.height * self.config.align_position;

        if target_align_index < self.doc.lines.len() {
            let line_height = self.metrics.height_at(target_align_index);
            match self.config.align_anchor {
                AlignAnchor::Bottom => cur_pos -= line_height,
                AlignAnchor::Center => cur_pos -= line_height / 2.0,
                AlignAnchor::Top => {}
            }
        }

        let latest_index = self
            .buffered
            .iter()
            .copied()
            .max()
            .unwrap_or(self.scroll_to_index);
        let mut delay = 0.0f32;
        let mut base_delay = if force { 0.0 } else { 0.05 };
        let mut dots: Option<InterludeDotsFrame> = None;
        let mut visible_start = self.doc.lines.len();
        let mut visible_end = 0usize;

        for i in 0..self.doc.lines.len() {
            let line = &self.doc.lines[i];
            let has_buffered = self.buffered.contains(&i);
            let is_active = has_buffered || (i >= self.scroll_to_index && i < latest_index);
            let should_show_dots = interlude.is_some_and(|it| i as i64 == it.index + 1);

            if dots.is_none() && should_show_dots {
                let mut x = 0.0;
                if interlude.is_some_and(|it| it.next_duet) {
                    x = (self.metrics.width - dots_width).max(0.0);
                }
                cur_pos += dot_margin;
                let y = cur_pos;
                cur_pos += dots_height;
                cur_pos += dot_margin;
                dots = Some(InterludeDotsFrame {
                    x,
                    y,
                    width: dots_width,
                    height: dots_height,
                    dot_size,
                    ..Default::default()
                });
            }

            // AMLL：高亮组（正在演唱的行）wrapper 透明度 0.85，其余行 1.0
            //（非逐词歌词的非当前行回退到 non_dynamic_opacity = 0.2）。
            let target_opacity = if has_buffered {
                0.85
            } else if self.config.hide_passed_lines {
                let past = i < if let Some(it) = interlude {
                    (it.index + 1).max(0) as usize
                } else {
                    self.scroll_to_index
                };
                if past && self.is_playing {
                    // AMLL 用一个极小但不为零的值避免浏览器优化掉合成层。
                    0.00001
                } else if is_active || self.doc.is_dynamic {
                    1.0
                } else {
                    self.config.non_dynamic_opacity
                }
            } else if is_active || self.doc.is_dynamic {
                1.0
            } else {
                self.config.non_dynamic_opacity
            };

            // AMLL resolveBlurLevel：blur = 1 + distance（焦点上方 +1）；
            // 窄视口（容器宽 ≤ 1024）整体 ×0.8。
            let mut blur_level = 0.0f32;
            if self.config.enable_blur && !is_active {
                blur_level = 1.0;
                if i < self.scroll_to_index {
                    blur_level += (self.scroll_to_index - i) as f32 + 1.0;
                } else {
                    blur_level += (i as f32 - self.scroll_to_index.max(latest_index) as f32).abs();
                }
                if self.metrics.width <= 1024.0 {
                    blur_level *= 0.8;
                }
                // AMLL `lyric-group.ts` 在写 CSS filter 时取 min(5, blur)；
                // 超过 5px 的差异肉眼不可辨，却会让字形图集多出大量条目。
                blur_level = blur_level.min(5.0);
            }

            let scale_aspect = if self.config.enable_scale {
                97.0
            } else {
                100.0
            };
            let mut target_scale = 100.0f32;
            if !is_active && self.is_playing {
                target_scale = if line.is_bg { 75.0 } else { scale_aspect };
            }

            let visible = {
                let h = self.metrics.height_at(i);
                let top = cur_pos;
                top + h > -self.config.overscan_px
                    && top < self.metrics.height + self.config.overscan_px
            };

            self.targets[i] = LineTarget {
                top: cur_pos,
                scale: target_scale,
                opacity: target_opacity,
                blur: blur_level,
                visible,
            };
            if visible {
                visible_start = visible_start.min(i);
                visible_end = i + 1;
            }
            let frame_line = &mut self.frame.lines[i];
            frame_line.visible = visible;
            frame_line.enabled = has_buffered;
            if !visible {
                // 视口外（含 overscan 之外）不参与逐帧动画：直接吸附目标，
                // 进入视口前已经处于正确位置，视觉等价且避免 O(全部行)/帧。
                self.springs[i].pos_y.set_position(cur_pos as f64);
                self.springs[i].scale.set_position(target_scale as f64);
                frame_line.top = cur_pos;
                frame_line.scale = target_scale;
                frame_line.opacity = target_opacity;
                frame_line.blur = blur_level;
                self.animated.remove(&i);
            } else if force || !self.initial_layout_finished {
                self.springs[i].pos_y.set_position(cur_pos as f64);
                self.springs[i].scale.set_position(target_scale as f64);
                frame_line.top = cur_pos;
                frame_line.scale = target_scale;
                frame_line.opacity = target_opacity;
                frame_line.blur = blur_level;
                self.animated.insert(i);
            } else {
                self.springs[i]
                    .pos_y
                    .set_target_position(cur_pos as f64, delay as f64);
                self.springs[i]
                    .scale
                    .set_target_position(target_scale as f64, 0.0);
                self.animated.insert(i);
            }

            if line.is_bg {
                if is_active || !self.is_playing {
                    cur_pos += self.metrics.height_at(i);
                }
            } else {
                cur_pos += self.metrics.height_at(i);
            }

            if cur_pos >= 0.0 && !self.is_seeking {
                if !line.is_bg {
                    delay += base_delay;
                }
                if i >= self.scroll_to_index {
                    base_delay /= 1.05;
                }
            }
        }

        if visible_end <= visible_start {
            visible_start = 0;
            visible_end = 0;
        }
        self.visible_start = visible_start;
        self.visible_end = visible_end;

        self.scroll_boundary[1] = cur_pos + self.scroll_offset - self.metrics.height / 2.0;
        self.frame.scroll_boundary = self.scroll_boundary;
        self.frame.scroll_offset = self.scroll_offset;

        if let Some(inter) = interlude {
            self.frame.interlude = Some(InterludeFrame {
                start_ms: inter.start,
                end_ms: inter.end,
                line_index: inter.index,
                next_duet: inter.next_duet,
                dots: dots.unwrap_or_default(),
            });
            if let Some(frame) = self.frame.interlude.as_mut() {
                frame.dots.scale = self.interlude_anim.scale;
                frame.dots.dot_opacity = self.interlude_anim.dot_opacity;
            }
        } else {
            self.frame.interlude = None;
        }
        _scope.finish(self.doc.lines.len() as u64);
    }

    fn set_interlude(&mut self, interlude: Option<InterludeInfo>) {
        if self.interlude != interlude {
            self.interlude = interlude;
            if let Some(it) = interlude {
                self.interlude_anim = InterludeAnim {
                    current_time: it.start as f32,
                    scale: 0.0,
                    dot_opacity: [0.0; 3],
                };
            } else {
                self.interlude_anim = InterludeAnim::default();
            }
        }
    }

    fn update_interlude_anim(&mut self, delta_ms: f32, interlude: InterludeInfo) {
        if !self.is_playing {
            return;
        }
        self.interlude_anim.current_time += delta_ms;
        let current = self.interlude_anim.current_time;
        let interlude_start = interlude.start as f32;
        let interlude_end = interlude.end as f32;
        let duration = (interlude_end - interlude_start).max(1.0);
        let current_duration = current - interlude_start;
        if current_duration > duration {
            self.interlude_anim.scale = 0.0;
            self.interlude_anim.dot_opacity = [0.0; 3];
            return;
        }

        let target_breathe = 1500.0f32;
        let breathe_count = (duration / target_breathe).ceil().max(1.0);
        let breathe_duration = duration / breathe_count;
        let mut scale = 1.0f32;
        let mut global_opacity = 1.0f32;

        // 与 AMLL interlude-dots.ts 完全一致：相位每周期只前进 2.0（不是 2π）。
        scale *= (1.5 * std::f32::consts::PI - (current_duration / breathe_duration) * 2.0).sin()
            / 20.0
            + 1.0;

        if current_duration < 2000.0 {
            scale *= ease_out_expo(current_duration / 2000.0);
        }
        if current_duration < 500.0 {
            global_opacity = 0.0;
        } else if current_duration < 1000.0 {
            global_opacity *= (current_duration - 500.0) / 500.0;
        }
        if duration - current_duration < 750.0 {
            scale *= 1.0 - ease_in_out_back((750.0 - (duration - current_duration)) / 750.0 / 2.0);
        }
        if duration - current_duration < 375.0 {
            global_opacity *= ((duration - current_duration) / 375.0).clamp(0.0, 1.0);
        }

        let dots_duration = (duration - 750.0).max(0.0).max(1.0);
        let dot0 = (((current_duration * 3.0) / dots_duration) * 0.75).clamp(0.25, 1.0);
        let dot1 = ((((current_duration - dots_duration / 3.0) * 3.0) / dots_duration) * 0.75)
            .clamp(0.25, 1.0);
        let dot2 = ((((current_duration - (dots_duration / 3.0) * 2.0) * 3.0) / dots_duration)
            * 0.75)
            .clamp(0.25, 1.0);

        self.interlude_anim.scale = scale.max(0.0) * 0.7;
        self.interlude_anim.dot_opacity = [
            (global_opacity * dot0).clamp(0.0, 1.0),
            (global_opacity * dot1).clamp(0.0, 1.0),
            (global_opacity * dot2).clamp(0.0, 1.0),
        ];
    }

    /// AMLL `getCurrentInterlude` 的等价实现。
    fn get_current_interlude(&self) -> Option<InterludeInfo> {
        let time = self.current_time.saturating_add(20);
        let current_index = self.scroll_to_index as i64;
        let lines = &self.doc.lines;

        let check_gap = |k: i64| -> Option<InterludeInfo> {
            if k < -1 || k >= lines.len() as i64 - 1 {
                return None;
            }
            let prev = if k == -1 { None } else { lines.get(k as usize) };
            let next = lines.get((k + 1) as usize)?;
            let gap_start = prev.map(|l| l.end_time).unwrap_or(0);
            let gap_end = gap_start.max(next.start_time.saturating_sub(250));
            if gap_end.saturating_sub(gap_start) < 4000 {
                return None;
            }
            if gap_end > time && gap_start < time {
                return Some(InterludeInfo {
                    start: gap_start.max(time),
                    end: gap_end,
                    index: k,
                    next_duet: next.is_duet,
                });
            }
            None
        };

        check_gap(current_index - 1)
            .or_else(|| check_gap(current_index))
            .or_else(|| check_gap(current_index + 1))
    }

    fn reset_scroll(&mut self) {
        self.scroll_offset = 0.0;
    }

    /// 测试/调试：当前热行与缓冲行数量。
    #[doc(hidden)]
    pub fn debug_line_state(&self) -> (usize, usize, usize) {
        (self.hot.len(), self.buffered.len(), self.scroll_to_index)
    }

    #[doc(hidden)]
    pub fn debug_target(&self, i: usize) -> Option<(f32, f32, f32, f32, bool)> {
        self.targets
            .get(i)
            .map(|t| (t.top, t.scale, t.opacity, t.blur, t.visible))
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
            line_heights: vec![80.0; 3],
        });
        p.set_time(5_000, false);
        assert!(p.frame().interlude.is_some());
    }
}
