//! 沉浸页下滑关闭（sheet dismissal）的手势状态机。
//!
//! 移动端播放页的顶部拖拽条是统一的「下拉关闭」把手：按住向下拖时整页
//! 跟手位移，露出背后的主面板；松手后按位移与甩动速度决定回弹还是继续
//! 下滑退出。手势本身不依赖定时器，帧节奏由 `Render` 中的
//! `request_animation_frame` 驱动（与 `player_page::Transition` 一致）。

use std::time::Instant;

/// 位移超过窗口高度的该比例时判定为关闭。
const DISMISS_HEIGHT_RATIO: f32 = 0.18;
/// 快速甩动：超过该速度且已有足够位移时直接关闭（px/s）。
const DISMISS_FLING_VELOCITY: f32 = 720.0;
const DISMISS_FLING_OFFSET: f32 = 48.0;
/// 起手死区：小于该位移视为点击而不是拖拽。
const DRAG_SLOP: f32 = 3.0;
/// 松手回弹：欠阻尼弹簧，轻微过冲。
const RETURN_DAMPING: f32 = 14.0;
const RETURN_STIFFNESS: f32 = 170.0;
/// 关闭：接近临界阻尼，直接收进 Dock，不回弹。
const DISMISS_DAMPING: f32 = 24.0;
const DISMISS_STIFFNESS: f32 = 170.0;
/// 弹簧最长收敛时间（秒），超过后强制结束。
const SPRING_MAX_SECS: f32 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetDragOutcome {
    Returned,
    Dismissed,
}

/// 一维解析弹簧（单位与调用方一致）。`from` → `to`，带初速度 `v0`。
///
/// 用解析解而不是逐帧积分：任意时刻 O(1) 求值，掉帧不改变轨迹；
/// 与项目里的 AMLL `spring.rs` 使用同一套物理语义。
#[derive(Debug, Clone, Copy)]
pub struct SpringSettle {
    from: f32,
    to: f32,
    v0: f32,
    start: Instant,
    damping: f32,
    stiffness: f32,
}

impl SpringSettle {
    pub fn new(from: f32, to: f32, v0: f32, start: Instant, damping: f32, stiffness: f32) -> Self {
        Self {
            from,
            to,
            v0,
            start,
            damping,
            stiffness,
        }
    }

    fn coeffs(&self) -> (f32, f32, f32) {
        // mass = 1
        let w0 = self.stiffness.max(0.001).sqrt();
        let zeta = self.damping / (2.0 * w0.max(0.001));
        (w0, zeta, self.from - self.to)
    }

    pub fn position(&self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.start).as_secs_f32();
        let (w0, zeta, x0) = self.coeffs();
        if zeta < 1.0 {
            let wd = w0 * (1.0 - zeta * zeta).max(0.0).sqrt();
            let e = (-zeta * w0 * t).exp();
            let (sin, cos) = (wd * t).sin_cos();
            let b = (self.v0 + zeta * w0 * x0) / wd.max(0.001);
            self.to + e * (x0 * cos + b * sin)
        } else {
            let e = (-w0 * t).exp();
            self.to + (x0 + (self.v0 + w0 * x0) * t) * e
        }
    }

    pub fn velocity(&self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.start).as_secs_f32();
        let (w0, zeta, x0) = self.coeffs();
        if zeta < 1.0 {
            let wd = w0 * (1.0 - zeta * zeta).max(0.0).sqrt();
            let e = (-zeta * w0 * t).exp();
            let (sin, cos) = (wd * t).sin_cos();
            let a = x0;
            let b = (self.v0 + zeta * w0 * x0) / wd.max(0.001);
            e * ((-zeta * w0) * (a * cos + b * sin) + (-a * wd * sin + b * wd * cos))
        } else {
            let e = (-w0 * t).exp();
            let b = self.v0 + w0 * x0;
            e * (b - w0 * (x0 + b * t))
        }
    }

    /// 弹簧已收敛（位移 < 0.5 且速度 < 12）或超过最大时长。
    pub fn settled(&self, now: Instant) -> bool {
        if now.saturating_duration_since(self.start).as_secs_f32() >= SPRING_MAX_SECS {
            return true;
        }
        (self.position(now) - self.to).abs() < 0.5 && self.velocity(now).abs() < 12.0
    }
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Tracking,
    Settling { spring: SpringSettle, dismiss: bool },
}

/// 一次下拉关闭手势。`None` 表示没有手势；进入 `Settling` 后由 render
/// 每帧读取 `offset`，直到 `settled` 返回结果。
#[derive(Debug, Clone, Copy)]
pub struct SheetDrag {
    start_y: f32,
    offset: f32,
    last_y: f32,
    last_at: Instant,
    velocity: f32,
    engaged: bool,
    phase: Phase,
}

impl SheetDrag {
    pub fn begin(y: f32, now: Instant) -> Self {
        Self {
            start_y: y,
            offset: 0.0,
            last_y: y,
            last_at: now,
            velocity: 0.0,
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
        let dy = y - self.start_y;
        let dt = now
            .saturating_duration_since(self.last_at)
            .as_secs_f32()
            .max(0.001);
        let instant_velocity = (y - self.last_y) / dt;
        // 平滑速度：松手前的最后一次抖动不应该决定去留。
        self.velocity = self.velocity * 0.55 + instant_velocity * 0.45;
        self.last_y = y;
        self.last_at = now;
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
    /// `height` 只用于判定距离阈值；`dismiss_target` 是关闭动画的终点
    /// （沉浸页收进 Dock 时传入 Dock 的顶边 y）。
    pub fn release(&mut self, height: f32, dismiss_target: f32, now: Instant) {
        if !matches!(self.phase, Phase::Tracking) || !self.engaged {
            return;
        }
        // 松手前手指已停住时，速度不再代表甩动。
        if now.saturating_duration_since(self.last_at).as_secs_f32() > 0.08 {
            self.velocity = 0.0;
        }
        let dismiss = self.offset > height * DISMISS_HEIGHT_RATIO
            || (self.velocity > DISMISS_FLING_VELOCITY && self.offset > DISMISS_FLING_OFFSET);
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
        let (damping, stiffness) = if dismiss {
            (DISMISS_DAMPING, DISMISS_STIFFNESS)
        } else {
            (RETURN_DAMPING, RETURN_STIFFNESS)
        };
        self.phase = Phase::Settling {
            spring: SpringSettle::new(self.offset, to, self.velocity, now, damping, stiffness),
            dismiss,
        };
    }

    /// 当前页面应下移的像素。
    pub fn offset(&self, now: Instant) -> f32 {
        match self.phase {
            Phase::Tracking => self.offset,
            Phase::Settling { spring, .. } => spring.position(now),
        }
    }

    pub fn engaged(&self) -> bool {
        self.engaged
    }

    /// 是否处于松手后的回弹/关闭动画。
    pub fn animating(&self) -> bool {
        matches!(self.phase, Phase::Settling { .. })
    }

    /// 动画结束后的结果；仍在进行中返回 `None`。
    pub fn settled(&self, now: Instant) -> Option<SheetDragOutcome> {
        let Phase::Settling { spring, dismiss } = self.phase else {
            return None;
        };
        if !spring.settled(now) {
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

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn click_without_movement_is_not_a_drag() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(200.0, now);
        assert!(!drag.update(201.0, at(now, 16)));
        assert!(!drag.engaged());
        drag.release(1000.0, 1000.0, at(now, 32));
        assert!(!drag.animating());
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
        drag.release(1000.0, 1000.0, at(now, 32));
        assert!(drag.animating());
        assert_eq!(drag.settled(at(now, 40)), None);
        let end_ms = 32 + (SPRING_MAX_SECS * 1000.0) as u64;
        assert_eq!(
            drag.settled(at(now, end_ms)),
            Some(SheetDragOutcome::Returned)
        );
        let end = drag.offset(at(now, end_ms));
        assert!(end.abs() < 0.5, "回弹结束后应回到 0，实际 {end}");
    }

    #[test]
    fn long_drag_dismisses() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(0.0, now);
        drag.update(300.0, at(now, 16));
        drag.release(1000.0, 1000.0, at(now, 32));
        assert!(drag.animating());
        let end_ms = 32 + (SPRING_MAX_SECS * 1000.0) as u64;
        assert_eq!(
            drag.settled(at(now, end_ms)),
            Some(SheetDragOutcome::Dismissed)
        );
        let end = drag.offset(at(now, end_ms));
        assert!(end > 990.0, "关闭结束后页面应到达目标位置，实际 {end}");
    }

    #[test]
    fn fast_fling_dismisses_even_below_height_threshold() {
        let now = Instant::now();
        let mut drag = SheetDrag::begin(0.0, now);
        // 16ms 内下移 120px ≈ 7500px/s。
        drag.update(120.0, at(now, 2));
        drag.release(1000.0, 1000.0, at(now, 3));
        assert_eq!(
            drag.settled(at(now, 3 + (SPRING_MAX_SECS * 1000.0) as u64)),
            Some(SheetDragOutcome::Dismissed)
        );
    }
}
