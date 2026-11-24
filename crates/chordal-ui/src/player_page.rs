//! 竖屏播放页的页面模型与过渡曲线。
//!
//! 竖屏播放页本身不承载歌词（歌词只在宽屏沉浸层出现），因此页面只有
//! 播放与播放队列两种。队列是**纵向滑动**（播放页上移、队列从底部进入）。
//! 「更多」不再是整页，而是播放页上方的底部抽屉，不参与页面过渡。
//! 所有过渡都由显示刷新率驱动的帧请求驱动，结束即停。

use std::time::Instant;

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
    pub start: Instant,
    pub duration_ms: f32,
}

impl Transition {
    pub fn new(from: PortraitPage, to: PortraitPage) -> Self {
        let kind = TransitionKind::between(from, to);
        let duration_ms = match kind {
            TransitionKind::Fade => 160.0,
            TransitionKind::Vertical => 320.0,
        };
        Self {
            from,
            to,
            kind,
            start: Instant::now(),
            duration_ms,
        }
    }

    /// 0..1 的缓动进度（ease-out cubic）。
    pub fn progress(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.start).as_secs_f32() * 1000.0
            / self.duration_ms)
            .clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }

    pub fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 >= self.duration_ms
    }

    /// 返回 `(from_offset, from_opacity, to_offset, to_opacity)`。
    /// 偏移单位为像素：纵向是 y，横向是 x。
    pub fn frames(&self, now: Instant, _width: f32, height: f32) -> (f32, f32, f32, f32) {
        let t = self.progress(now);
        match self.kind {
            TransitionKind::Fade => (0.0, 1.0 - t, 0.0, t),
            TransitionKind::Vertical => (-height * t, 1.0, height * (1.0 - t), 1.0),
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
    fn vertical_frames_start_and_end_cleanly() {
        let mut tr = Transition::new(PortraitPage::Art, PortraitPage::Queue);
        tr.duration_ms = 320.0;
        let now = tr.start + std::time::Duration::from_millis(320);
        assert!(tr.done(now));
        let (from_y, _, to_y, _) = tr.frames(now, 800.0, 1000.0);
        assert!(from_y < -999.0);
        assert!(to_y.abs() < 1.0);
    }
}
