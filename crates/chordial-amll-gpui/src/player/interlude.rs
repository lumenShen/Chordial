//! LyricPlayer：interlude（由 player.rs 拆分，行为不变）。

use super::*;

impl LyricPlayer {
    pub(super) fn set_interlude(&mut self, interlude: Option<InterludeInfo>) {
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

    pub(super) fn update_interlude_anim(&mut self, delta_ms: f32, interlude: InterludeInfo) {
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
    pub(super) fn get_current_interlude(&self) -> Option<InterludeInfo> {
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
}
