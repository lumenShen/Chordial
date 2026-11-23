//! 手机壳的跟手手势：mini player ⇄ 播放页的纵向 sheet，以及封面 ⇄ 歌词的横滑。
//!
//! 状态机只负责「位移 → 进度」和「松手后的弹簧」，不碰任何页面状态。
//! 渲染层用 `progress` / `offset` 直接摆放卡片，动画结束后再切换页面。
//! 与 `sheet_drag` 共用同一套解析弹簧语义，掉帧不会改变轨迹。

use std::time::Instant;

use crate::sheet_drag::SpringSettle;

/// 起手死区：小于该位移视为点击。
const DRAG_SLOP: f32 = 4.0;
/// 松手时越过该比例的行程即提交（展开/收起、切页）。
const COMMIT_RATIO: f32 = 0.24;
/// 甩动提交速度（px/s）。
const FLING_VELOCITY: f32 = 560.0;
/// 回弹弹簧：轻微过冲。
const RETURN_DAMPING: f32 = 16.0;
const RETURN_STIFFNESS: f32 = 180.0;
/// 提交弹簧：接近临界阻尼，直接落到目标。
const COMMIT_DAMPING: f32 = 25.0;
const COMMIT_STIFFNESS: f32 = 190.0;
/// 弹簧最长收敛时间（秒），超过后强制结束。
const SPRING_MAX_SECS: f32 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetPage {
    Mini,
    Full,
}

/// mini player（0）到播放页（1）的纵向拖拽。
///
/// 两个方向都能起手：曲库页从 mini player 往上拖展开，播放页从顶部
/// 把手往下拖收起。`start_progress` 记录起手时停在哪一端。
#[derive(Clone, Copy, Debug)]
pub struct SheetSwipe {
    start_progress: f32,
    start_y: f32,
    last_y: f32,
    last_at: Instant,
    velocity: f32,
    progress: f32,
    engaged: bool,
    settle: Option<SheetSettle>,
}

#[derive(Clone, Copy, Debug)]
struct SheetSettle {
    spring: SpringSettle,
    target: f32,
}

impl SheetSwipe {
    pub fn begin(start_progress: f32, y: f32, now: Instant) -> Self {
        let start_progress = start_progress.clamp(0.0, 1.0);
        Self {
            start_progress,
            start_y: y,
            last_y: y,
            last_at: now,
            velocity: 0.0,
            progress: start_progress,
            engaged: false,
            settle: None,
        }
    }

    /// 更新指针。返回 `true` 表示进度发生变化，需要重绘。
    pub fn update(&mut self, y: f32, height: f32, now: Instant) -> bool {
        if self.settle.is_some() {
            return false;
        }
        let dy = y - self.start_y;
        let dt = now
            .saturating_duration_since(self.last_at)
            .as_secs_f32()
            .max(0.001);
        let instant = (y - self.last_y) / dt;
        self.velocity = self.velocity * 0.6 + instant * 0.4;
        self.last_y = y;
        self.last_at = now;

        if !self.engaged {
            if dy.abs() <= DRAG_SLOP {
                return false;
            }
            // 已在端点时继续往端点外拖：不接管，让列表自己滚动/点击。
            let can_expand = self.start_progress < 0.5 && dy < 0.0;
            let can_collapse = self.start_progress >= 0.5 && dy > 0.0;
            if !can_expand && !can_collapse {
                return false;
            }
            self.engaged = true;
        }

        let next = (self.start_progress - dy / height.max(1.0)).clamp(0.0, 1.0);
        if (next - self.progress).abs() <= 0.0005 {
            return false;
        }
        self.progress = next;
        true
    }

    /// 松手：决定提交还是回弹。返回是否进入动画。
    pub fn release(&mut self, height: f32, now: Instant) -> bool {
        if self.settle.is_some() || !self.engaged {
            return false;
        }
        // 松手前已经停住，速度不再代表甩动。
        if now.saturating_duration_since(self.last_at).as_secs_f32() > 0.09 {
            self.velocity = 0.0;
        }
        let expanding = self.start_progress < 0.5;
        let commit = if expanding {
            self.progress > COMMIT_RATIO || self.velocity < -FLING_VELOCITY
        } else {
            self.progress < 1.0 - COMMIT_RATIO || self.velocity > FLING_VELOCITY
        };
        let target = if commit {
            if expanding { 1.0 } else { 0.0 }
        } else if expanding {
            0.0
        } else {
            1.0
        };
        if (self.progress - target).abs() <= 0.001 {
            // 已经贴住目标，直接结束，避免无意义的弹簧帧。
            self.progress = target;
            return false;
        }
        let (damping, stiffness) = if commit {
            (COMMIT_DAMPING, COMMIT_STIFFNESS)
        } else {
            (RETURN_DAMPING, RETURN_STIFFNESS)
        };
        self.settle = Some(SheetSettle {
            spring: SpringSettle::new(
                self.progress,
                target,
                self.velocity / height.max(1.0),
                now,
                damping,
                stiffness,
            ),
            target,
        });
        true
    }

    pub fn progress(&self, now: Instant) -> f32 {
        match self.settle {
            Some(s) => s.spring.position(now).clamp(0.0, 1.0),
            None => self.progress,
        }
    }

    pub fn engaged(&self) -> bool {
        self.engaged
    }

    /// 是否处于松手后的动画中。
    pub fn animating(&self, now: Instant) -> bool {
        match self.settle {
            Some(s) => !s.spring.settled(now),
            None => false,
        }
    }

    /// 动画结束后的落点；仍未结束或仍在拖拽时返回 `None`。
    pub fn settled(&self, now: Instant) -> Option<SheetPage> {
        let settle = self.settle?;
        if !settle.spring.settled(now)
            && now.saturating_duration_since(self.last_at).as_secs_f32() < SPRING_MAX_SECS
        {
            return None;
        }
        Some(if settle.target >= 0.5 {
            SheetPage::Full
        } else {
            SheetPage::Mini
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDirection {
    /// 左滑：封面 → 歌词。
    Forward,
    /// 右滑：歌词 → 封面。
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeOutcome {
    Returned,
    Committed(SwipeDirection),
}

/// 封面/歌词的横向跟手位移。
///
/// 点击仍然由上层 `on_click` 处理；只有横向位移超过死区后
/// `engaged()` 才为真，上层据此吞掉本次点击。
#[derive(Clone, Copy, Debug)]
pub struct EdgeSwipe {
    start_x: f32,
    start_y: f32,
    last_x: f32,
    last_at: Instant,
    velocity: f32,
    offset: f32,
    engaged: bool,
    allow_forward: bool,
    allow_back: bool,
    settle: Option<EdgeSettle>,
}

#[derive(Clone, Copy, Debug)]
struct EdgeSettle {
    spring: SpringSettle,
    outcome: SwipeOutcome,
}

impl EdgeSwipe {
    pub fn begin(x: f32, y: f32, now: Instant, allow_forward: bool, allow_back: bool) -> Self {
        Self {
            start_x: x,
            start_y: y,
            last_x: x,
            last_at: now,
            velocity: 0.0,
            offset: 0.0,
            engaged: false,
            allow_forward,
            allow_back,
            settle: None,
        }
    }

    pub fn update(&mut self, x: f32, y: f32, now: Instant) -> bool {
        if self.settle.is_some() {
            return false;
        }
        let dx = x - self.start_x;
        let dy = y - self.start_y;
        let dt = now
            .saturating_duration_since(self.last_at)
            .as_secs_f32()
            .max(0.001);
        let instant = (x - self.last_x) / dt;
        self.velocity = self.velocity * 0.6 + instant * 0.4;
        self.last_x = x;
        self.last_at = now;

        if !self.engaged {
            if dx.abs() <= DRAG_SLOP || dx.abs() <= dy.abs() + DRAG_SLOP {
                return false;
            }
            if (dx < 0.0 && !self.allow_forward) || (dx > 0.0 && !self.allow_back) {
                return false;
            }
            self.engaged = true;
        }
        let next = if dx < 0.0 {
            dx.max(-1000.0)
        } else {
            dx.min(1000.0)
        };
        if (next - self.offset).abs() <= 0.01 {
            return false;
        }
        self.offset = next;
        true
    }

    pub fn release(&mut self, width: f32, now: Instant) -> bool {
        if self.settle.is_some() || !self.engaged {
            return false;
        }
        if now.saturating_duration_since(self.last_at).as_secs_f32() > 0.09 {
            self.velocity = 0.0;
        }
        let commit = if self.offset < 0.0 {
            self.offset.abs() > width * COMMIT_RATIO || self.velocity < -FLING_VELOCITY
        } else {
            self.offset.abs() > width * COMMIT_RATIO || self.velocity > FLING_VELOCITY
        };
        let (target, outcome) = if commit {
            if self.offset < 0.0 {
                (-width, SwipeOutcome::Committed(SwipeDirection::Forward))
            } else {
                (width, SwipeOutcome::Committed(SwipeDirection::Back))
            }
        } else {
            (0.0, SwipeOutcome::Returned)
        };
        if (self.offset - target).abs() <= 0.001 {
            return false;
        }
        let (damping, stiffness) = if commit {
            (COMMIT_DAMPING, COMMIT_STIFFNESS)
        } else {
            (RETURN_DAMPING, RETURN_STIFFNESS)
        };
        self.settle = Some(EdgeSettle {
            spring: SpringSettle::new(self.offset, target, self.velocity, now, damping, stiffness),
            outcome,
        });
        true
    }

    pub fn offset(&self, now: Instant) -> f32 {
        match self.settle {
            Some(s) => s.spring.position(now),
            None => self.offset,
        }
    }

    pub fn engaged(&self) -> bool {
        self.engaged
    }

    pub fn animating(&self, now: Instant) -> bool {
        match self.settle {
            Some(s) => !s.spring.settled(now),
            None => false,
        }
    }

    pub fn settled(&self, now: Instant) -> Option<SwipeOutcome> {
        let settle = self.settle?;
        if !settle.spring.settled(now)
            && now.saturating_duration_since(self.last_at).as_secs_f32() < SPRING_MAX_SECS
        {
            return None;
        }
        Some(settle.outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn mini_player_drag_up_expands() {
        let now = Instant::now();
        let mut swipe = SheetSwipe::begin(0.0, 800.0, now);
        assert!(swipe.update(600.0, 800.0, at(now, 16)));
        assert!(swipe.progress(at(now, 16)) > 0.2);
        swipe.release(800.0, at(now, 32));
        assert!(swipe.animating(at(now, 32)));
        assert_eq!(swipe.settled(at(now, 32 + 900)), Some(SheetPage::Full));
    }

    #[test]
    fn short_drag_returns_to_mini() {
        let now = Instant::now();
        let mut swipe = SheetSwipe::begin(0.0, 800.0, now);
        swipe.update(770.0, 800.0, at(now, 16));
        // 松手前停一下，让速度归零：这是「拖了一点又放回去」而不是甩动。
        swipe.release(800.0, at(now, 220));
        assert_eq!(swipe.settled(at(now, 220 + 900)), Some(SheetPage::Mini));
    }

    #[test]
    fn player_drag_down_dismisses() {
        let now = Instant::now();
        let mut swipe = SheetSwipe::begin(1.0, 100.0, now);
        assert!(swipe.update(500.0, 800.0, at(now, 16)));
        swipe.release(800.0, at(now, 32));
        assert_eq!(swipe.settled(at(now, 32 + 900)), Some(SheetPage::Mini));
    }

    #[test]
    fn upward_drag_on_full_page_is_not_taken_over() {
        let now = Instant::now();
        let mut swipe = SheetSwipe::begin(1.0, 300.0, now);
        assert!(!swipe.update(100.0, 800.0, at(now, 16)));
        assert!(!swipe.engaged());
    }

    #[test]
    fn cover_swipe_left_commits_to_lyrics() {
        let now = Instant::now();
        let mut swipe = EdgeSwipe::begin(500.0, 500.0, now, true, false);
        assert!(swipe.update(400.0, 505.0, at(now, 16)));
        swipe.release(700.0, at(now, 32));
        assert_eq!(
            swipe.settled(at(now, 32 + 900)),
            Some(SwipeOutcome::Committed(SwipeDirection::Forward))
        );
    }

    #[test]
    fn vertical_scroll_is_not_a_horizontal_swipe() {
        let now = Instant::now();
        let mut swipe = EdgeSwipe::begin(500.0, 500.0, now, true, true);
        assert!(!swipe.update(480.0, 600.0, at(now, 16)));
        assert!(!swipe.engaged());
    }
}
