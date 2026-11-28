//! 竖屏播放页的页面模型与过渡曲线。
//!
//! 竖屏播放页本身不承载歌词（歌词只在宽屏沉浸层出现），因此页面只有
//! 播放与播放队列两种。队列是**纵向滑动**（播放页上移、队列从底部进入）。
//! 「更多」不再是整页，而是播放页上方的底部抽屉，不参与页面过渡。
//! 所有过渡都由显示刷新率驱动的帧请求驱动，结束即停。

use std::time::{Duration, Instant};

use crate::motion::{MotionSpring, MotionTuning, PROGRESS_TOL};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortraitPage {
    Art,
    Queue,
}

impl PortraitPage {
    pub fn _is_overlay(self) -> bool {
        matches!(self, Self::Queue)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Fade,
    Vertical,
}

impl TransitionKind {
    pub fn between(from: PortraitPage, to: PortraitPage) -> Self {
        if from == to {
            return Self::Fade;
        }
        match (from, to) {
            (PortraitPage::Queue, _) | (_, PortraitPage::Queue) => Self::Vertical,
            _ => Self::Fade,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Transition {
    pub from: PortraitPage,
    pub to: PortraitPage,
    pub kind: TransitionKind,
    spring: MotionSpring,
    held: Option<(f32, f32)>,
    target: f32,
}

impl Transition {
    pub fn new(from: PortraitPage, to: PortraitPage, tuning: MotionTuning) -> Self {
        let kind = TransitionKind::between(from, to);
        let duration_ms = match kind {
            TransitionKind::Fade => 160.0,
            TransitionKind::Vertical => 320.0,
        };
        Self {
            from,
            to,
            kind,
            spring: tuning.spring(
                Duration::from_secs_f32(duration_ms / 1000.0),
                0.0,
                1.0,
                0.0,
                Instant::now(),
                PROGRESS_TOL.0,
                PROGRESS_TOL.1,
            ),
            held: Some((0.0, 0.0)),
            target: 1.0,
        }
    }

    pub fn held(&self) -> bool {
        self.held.is_some()
    }

    pub fn spec(&self) -> crate::motion::MotionSpec {
        self.spring.spec()
    }

    pub fn begin(&mut self, now: Instant) {
        if let Some((value, velocity)) = self.held.take() {
            self.spring
                .retarget_from_with(value, self.target, velocity, now);
        }
    }

    pub fn retarget(&mut self, to: PortraitPage, now: Instant) {
        let sample = self.spring.sample(now);
        let (mut value, mut velocity) = self.held.unwrap_or((sample.value, sample.velocity));
        let previous_from = self.from;
        self.from = self.to;
        self.to = to;
        self.kind = TransitionKind::between(self.from, to);
        self.target = if to == previous_from { 0.0 } else { 1.0 };
        if self.target == 0.0 {
            value = 1.0 - value;
            velocity = -velocity;
        }
        self.held = Some((value, velocity));
    }

    /// 0..1 的弹簧进度。
    pub fn progress(&self, now: Instant) -> f32 {
        self.held
            .map(|(value, _)| value)
            .unwrap_or_else(|| self.spring.value(now))
            .clamp(0.0, 1.0)
    }

    pub fn done(&self, now: Instant) -> bool {
        self.held.is_none() && self.spring.done(now)
    }

    /// 返回 `(from_offset, from_opacity, to_offset, to_opacity)`。
    /// 偏移单位为像素：纵向是 y，横向是 x。
    pub fn frames(&self, now: Instant, _width: f32, height: f32) -> (f32, f32, f32, f32) {
        let t = self.progress(now);
        match self.kind {
            TransitionKind::Fade => (0.0, 1.0 - t, 0.0, t),
            TransitionKind::Vertical => {
                let (mut from_offset, mut to_offset) = (-height * t, height * (1.0 - t));
                if self.from == PortraitPage::Queue {
                    from_offset = -from_offset;
                    to_offset = -to_offset;
                }
                (from_offset, 1.0, to_offset, 1.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_kinds_follow_page_geometry() {
        assert_eq!(
            TransitionKind::between(PortraitPage::Art, PortraitPage::Art),
            TransitionKind::Fade
        );
        assert_eq!(
            TransitionKind::between(PortraitPage::Art, PortraitPage::Queue),
            TransitionKind::Vertical
        );
    }

    #[test]
    fn interrupted_transition_holds_visible_progress_until_measurement_finishes() {
        let mut tr = Transition::new(
            PortraitPage::Art,
            PortraitPage::Queue,
            MotionTuning::default(),
        );
        let start = Instant::now();
        tr.begin(start);
        let mid = start + std::time::Duration::from_millis(120);
        let visible = tr.progress(mid);
        let before = tr.frames(mid, 800.0, 1000.0);
        assert!(visible > 0.0 && visible < 1.0);
        tr.retarget(PortraitPage::Art, mid);
        assert!(tr.held());
        let after = tr.frames(mid, 800.0, 1000.0);
        let continuity = [
            (before.0, after.2),
            (before.1, after.3),
            (before.2, after.0),
            (before.3, after.1),
        ];
        for (before, after) in continuity {
            assert!((before - after).abs() < 0.001, "{before} != {after}");
        }
        assert_eq!(
            tr.progress(mid + std::time::Duration::from_millis(32)),
            1.0 - visible
        );
        tr.begin(mid + std::time::Duration::from_millis(32));
        assert!(!tr.held());
        assert!(
            (tr.progress(mid + std::time::Duration::from_millis(32)) - (1.0 - visible)).abs()
                < 0.001
        );
        assert!(tr.progress(mid + std::time::Duration::from_secs(2)) < visible);
    }

    #[test]
    fn vertical_frames_start_and_end_cleanly() {
        let mut tr = Transition::new(
            PortraitPage::Art,
            PortraitPage::Queue,
            MotionTuning::default(),
        );
        let start = Instant::now();
        tr.begin(start);
        let (from_y, _, to_y, _) = tr.frames(start, 800.0, 1000.0);
        assert!(from_y.abs() < 1.0);
        assert!((to_y - 1000.0).abs() < 1.0);

        let end = start + std::time::Duration::from_secs(2);
        assert!(tr.done(end));
        let (from_y, _, to_y, _) = tr.frames(end, 800.0, 1000.0);
        assert!(from_y < -999.0);
        assert!(to_y.abs() < 1.0);
    }
}
