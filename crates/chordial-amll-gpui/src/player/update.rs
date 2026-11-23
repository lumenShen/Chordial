//! LyricPlayer：update（由 player.rs 拆分，行为不变）。

use super::*;

impl LyricPlayer {
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
        //
        // 原实现 `mem::take(&mut self.animated)` 会把 HashSet 的桶数组一起
        // 清成零容量，每帧重新 insert 可见行都触发 rehash + 分配；300Hz 下
        // 是确定性抖动。现在只读遍历，收尾用 retain 原地淘汰已收敛行。
        self.anim_scratch.clear();
        self.anim_scratch.extend(self.animated.iter().copied());
        self.anim_scratch.sort_unstable();
        self.anim_keep.clear();
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
                self.anim_keep.push(i);
            }
        }
        // 原地淘汰，保留桶数组容量；anim_keep 已排序，用二分判定即可。
        let keep = &self.anim_keep;
        self.animated.retain(|i| keep.binary_search(i).is_ok());

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

    pub(super) fn collect_active(&mut self, time: u64) {
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

    pub(super) fn rebuild_prefix(&mut self) {
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
    pub(super) fn calc_layout(&mut self, force: bool) {
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
        // AMLL `.interludeDots { gap: 0.18em; padding: 0.4em var(--lyric-line-padding-x) }`。
        // 这里只建模点组内容，左右 padding 由下面的 x 锚点承担。
        let dots_gap = dot_size * 0.25 + 4.0;
        let dots_width = dot_size * 3.0 + dots_gap * 2.0;
        let dots_padding_x = self.metrics.padding_x.max(0.0);
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
                // AMLL 的 `.interludeDots` 用 `left: 0` + padding-left = 1em，
                // 所以普通行第一颗点与歌词文本左边缘对齐；对唱行整体右对齐，
                // 最后一颗点与文本右边缘对齐。
                let mut x = dots_padding_x;
                if interlude.is_some_and(|it| it.next_duet) {
                    x = (self.metrics.width - dots_padding_x - dots_width).max(dots_padding_x);
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
                    gap: dots_gap,
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
}
