//! LyricPlayer：api（由 player.rs 拆分，行为不变）。

use super::*;

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
            pos_y_params: SpringParams::new(0.9, 15.0, 90.0),
            targets: Vec::new(),
            frame: PlayerFrame::default(),
            active_scratch: Vec::new(),
            added_scratch: Vec::new(),
            removed_scratch: Vec::new(),
            animated: FxHashSet::default(),
            anim_scratch: Vec::new(),
            anim_keep: Vec::new(),
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

    /// 运行时修改配置。仅在配置真正变化时重新布局（O(n) 一次）。
    /// 每帧重复设置同一份配置必须是 O(1) —— 渲染路径依赖这一点。
    pub fn set_config(&mut self, config: LyricPlayerConfig) {
        if self.config == config {
            return;
        }
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
        self.pos_y_params = SpringParams::new(0.9, 15.0, 90.0);
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
        for line in &self.doc.lines {
            let scale_params = if line.is_bg {
                SpringParams::new(1.0, 20.0, 50.0)
            } else {
                SpringParams::new(2.0, 25.0, 100.0)
            };
            self.springs.push(LineSprings {
                pos_y: Spring::new(0.0).with_params(self.pos_y_params),
                scale: Spring::new(100.0).with_params(scale_params),
            });
        }

        self.metrics.line_heights.clear();
        self.metrics.line_heights.resize(n, 0.0);
        self.prefix_dirty = true;
        self.rebuild_prefix();

        // 与 AMLL 相同：先按初始时间找出活跃行，再提交一次布局目标。
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
        // 首次测量仍从屏外进入；force 不再等同于 initial_layout_finished。
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

    pub(super) fn reset_scroll(&mut self) {
        self.scroll_offset = 0.0;
    }

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
