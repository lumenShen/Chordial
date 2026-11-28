//! 沉浸页下滑关闭（sheet dismissal）的手势状态机。
//!
//! 移动端播放页的顶部拖拽条是统一的「下拉关闭」把手：按住向下拖时整页
//! 跟手位移，露出背后的主面板；松手后按位移与甩动速度决定回弹还是继续
//! 下滑退出。手势本身不依赖定时器，帧节奏由 `Render` 中的
//! `request_animation_frame` 驱动（与 `player_page::Transition` 一致）。
//!
//! 松手后的弹簧统一走 [`MotionSpring`]（见 `motion` 模块）：解析解采样，
//! 掉帧不改变轨迹；本模块只保留「回弹还是关闭」的判定阈值。

use std::time::{Duration, Instant};

use crate::motion::{MotionSpring, MotionTuning, PIXEL_TOL, VelocityTracker};

/// 位移超过窗口高度的该比例时判定为关闭。
const DISMISS_HEIGHT_RATIO: f32 = 0.18;
/// 快速甩动：超过该速度且已有足够位移时直接关闭（px/s）。
const DISMISS_FLING_VELOCITY: f32 = 720.0;
const DISMISS_FLING_OFFSET: f32 = 48.0;
/// 起手死区：小于该位移视为点击而不是拖拽。
const DRAG_SLOP: f32 = 3.0;
/// 回弹基准时长：对应原先 (damping 14, stiffness 170) 的 ζ≈0.54。
const RETURN_BASE_MS: f32 = 260.0;
/// 回弹超伸倍率：把均衡预设压到欠阻尼，保留轻微过冲。
const RETURN_OVERSHOOT: f32 = 1.35;
/// 关闭基准时长：对应原先 (damping 24, stiffness 170) 的近临界阻尼。
const DISMISS_BASE_MS: f32 = 200.0;
/// 关闭不需要过冲，直接收进 Dock。
const DISMISS_OVERSHOOT: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetDragOutcome {
    Returned,
    Dismissed,
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Tracking,
    Settling { spring: MotionSpring, dismiss: bool },
}

/// 一次下拉关闭手势。`None` 表示没有手势；进入 `Settling` 后由 render
/// 每帧读取 `offset`，直到 `settled` 返回结果。
#[derive(Debug, Clone)]
pub struct SheetDrag {
    start_y: f32,
    offset: f32,
    tracker: VelocityTracker,
    engaged: bool,
    phase: Phase,
}

impl SheetDrag {
    pub fn begin(y: f32, now: Instant) -> Self {
        let mut tracker = VelocityTracker::default();
        // 用按下点给追踪器打底，第一帧的位移即可算出速度（与旧实现的 last_y 一致）。
        tracker.push(y, now);
        Self {
            start_y: y,
            offset: 0.0,
            tracker,
            engaged: false,
            phase: Phase::Tracking,
        }
    }

    /// 更新手指位置。返回 `true` 表示页面位移发生变化，需要重绘。
    ///
    /// 只跟手向下的位移；向上拖时不移动页面（返回 `false`）。超过死区
    /// 才真正进入拖拽，避免顶部空白区的一次普通点击造成抖动。
    pub fn update(&mut self, y: f32, now: Instant) -> bool {
        if !matches!(self.phase, Phase::Tracking) {
            return false;
        }
        // 速度由统一的追踪器平滑；松手时再按最后一点时间决定是否算甩动。
        self.tracker.push(y, now);
        let dy = y - self.start_y;
        if dy.abs() > DRAG_SLOP {
            self.engaged = true;
        }
        if !self.engaged {
            return false;
        }
        let next = dy.max(0.0);
        if (next - self.offset).abs() <= 0.01 {
            return false;
        }
        self.offset = next;
        true
    }

    /// 松手：决定回弹还是关闭。未越过死区或已经拖回起点的手势直接结束
    /// （不进入动画）。
    ///
    /// `y` 是最终松手坐标，会先于阈值判定写入位移与速度；`height` 只用于
    /// 判定距离阈值；`dismiss_target` 是关闭动画的终点（沉浸页收进 Dock 时
    /// 传入 Dock 的顶边 y）。`tuning` 决定新弹簧的手感，已开始的弹簧不受
    /// 后续设置改动影响。
    pub fn release(
        &mut self,
        y: f32,
        height: f32,
        dismiss_target: f32,
        now: Instant,
        tuning: MotionTuning,
    ) {
        if !matches!(self.phase, Phase::Tracking) {
            return;
        }
        // 高刷下最后一次 move 可能被合并，松手点必须参与位移与速度判定，
        // 否则实际已拖过阈值仍会按旧坐标回弹到全屏。
        self.update(y, now);
        if !self.engaged {
            return;
        }
        // 追踪器会把静止超过 100ms 的旧速度清零，避免停住后松手仍被当成甩动。
        let velocity = self.tracker.velocity_at(now);
        let dismiss = self.offset > height * DISMISS_HEIGHT_RATIO
            || (velocity > DISMISS_FLING_VELOCITY && self.offset > DISMISS_FLING_OFFSET);
        if !dismiss && self.offset <= 0.5 {
            return;
        }
        let to = if dismiss {
            dismiss_target
                .max(self.offset + 60.0)
                .min(height.max(self.offset + 60.0))
        } else {
            0.0
        };
        let (base_ms, overshoot_factor) = if dismiss {
            (DISMISS_BASE_MS, DISMISS_OVERSHOOT)
        } else {
            (RETURN_BASE_MS, RETURN_OVERSHOOT)
        };
        // 手感的超伸倍率只放大这一条弹簧的回弹，不改动全局设置。
        let tuned = MotionTuning::new(
            tuning.preset,
            tuning.duration,
            tuning.overshoot * overshoot_factor,
        );
        self.phase = Phase::Settling {
            spring: tuned.spring(
                Duration::from_secs_f32(base_ms / 1000.0),
                self.offset,
                to,
                velocity,
                now,
                PIXEL_TOL.0,
                PIXEL_TOL.1,
            ),
            dismiss,
        };
    }

    /// 当前页面应下移的像素。
    pub fn offset(&self, now: Instant) -> f32 {
        match self.phase {
            Phase::Tracking => self.offset,
            Phase::Settling { spring, .. } => spring.value(now),
        }
    }

    pub fn engaged(&self) -> bool {
        self.engaged
    }

    /// 是否处于松手后的回弹/关闭动画。
    pub fn animating(&self, now: Instant) -> bool {
        match self.phase {
            Phase::Settling { spring, .. } => spring.active(now),
            Phase::Tracking => false,
        }
    }

    /// 动画结束后的结果；仍在进行中返回 `None`。
    pub fn settled(&self, now: Instant) -> Option<SheetDragOutcome> {
        let Phase::Settling { spring, dismiss } = self.phase else {
            return None;
        };
        if !spring.done(now) {
            return None;
        }
        Some(if dismiss {
            SheetDragOutcome::Dismissed
        } else {
            SheetDragOutcome::Returned
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 新弹簧的硬超时由 6 个时间常数推导（最长约 0.6s），留足余量再看落点。
    const WAIT_MS: u64 = 1200;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn click_without_movement_is_not_a_drag() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(200.0, now);
        assert!(!drag.update(201.0, at(now, 16)));
        assert!(!drag.engaged());
        drag.release(201.0, 1000.0, 1000.0, at(now, 32), MotionTuning::default());
        assert!(!drag.animating(at(now, 32)));
    }

    #[test]
    fn downward_drag_follows_pointer() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(200.0, now);
        assert!(drag.update(260.0, at(now, 16)));
        assert_eq!(drag.offset(at(now, 16)), 60.0);
        // 向上拖回起点以内时页面跟着回到 0，不会继续上移。
        assert!(drag.update(150.0, at(now, 32)));
        assert_eq!(drag.offset(at(now, 32)), 0.0);
    }

    #[test]
    fn short_drag_snaps_back() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(200.0, now);
        drag.update(240.0, at(now, 16));
        drag.release(240.0, 1000.0, 1000.0, at(now, 32), MotionTuning::default());
        assert!(drag.animating(at(now, 32)));
        assert_eq!(drag.settled(at(now, 40)), None);
        assert_eq!(
            drag.settled(at(now, 32 + WAIT_MS)),
            Some(SheetDragOutcome::Returned)
        );
        let end = drag.offset(at(now, 32 + WAIT_MS));
        assert!(end.abs() < 0.5, "回弹结束后应回到 0，实际 {end}");
    }

    #[test]
    fn long_drag_dismisses() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(0.0, now);
        drag.update(300.0, at(now, 16));
        drag.release(300.0, 1000.0, 1000.0, at(now, 32), MotionTuning::default());
        assert!(drag.animating(at(now, 32)));
        assert_eq!(
            drag.settled(at(now, 32 + WAIT_MS)),
            Some(SheetDragOutcome::Dismissed)
        );
        let end = drag.offset(at(now, 32 + WAIT_MS));
        assert!(end > 990.0, "关闭结束后页面应到达目标位置，实际 {end}");
    }

    #[test]
    fn fast_fling_dismisses_even_below_height_threshold() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(0.0, now);
        // 16ms 内下移 120px ≈ 7500px/s。
        drag.update(120.0, at(now, 2));
        drag.release(120.0, 1000.0, 1000.0, at(now, 3), MotionTuning::default());
        assert_eq!(
            drag.settled(at(now, 3 + WAIT_MS)),
            Some(SheetDragOutcome::Dismissed)
        );
    }

    #[test]
    fn final_release_position_can_cross_dismiss_threshold() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(0.0, now);
        // 最后一次 move 只有 170px，低于 1000px 窗口的 18% 关闭阈值。
        drag.update(170.0, at(now, 16));
        // 松手点继续下移到 220px；即使中间没有 move，也必须按最终坐标关闭。
        drag.release(220.0, 1000.0, 1000.0, at(now, 32), MotionTuning::default());
        assert_eq!(drag.offset(at(now, 32)), 220.0);
        assert_eq!(
            drag.settled(at(now, 32 + WAIT_MS)),
            Some(SheetDragOutcome::Dismissed)
        );
    }

    #[test]
    fn release_without_movement_is_not_a_drag() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(200.0, now);
        drag.release(201.0, 1000.0, 1000.0, at(now, 16), MotionTuning::default());
        assert!(!drag.engaged());
        assert!(!drag.animating(at(now, 16)));
        assert_eq!(drag.settled(at(now, 16)), None);
    }

    #[test]
    fn repeated_release_preserves_dismiss_outcome() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(0.0, now);
        drag.update(300.0, at(now, 16));
        drag.release(300.0, 1000.0, 1000.0, at(now, 32), MotionTuning::default());
        let done_at = at(now, 32 + WAIT_MS);
        assert_eq!(drag.settled(done_at), Some(SheetDragOutcome::Dismissed));

        drag.release(300.0, 1000.0, 1000.0, done_at, MotionTuning::default());
        assert_eq!(drag.settled(done_at), Some(SheetDragOutcome::Dismissed));
    }
}
