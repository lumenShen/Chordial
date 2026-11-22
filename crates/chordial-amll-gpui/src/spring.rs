//! 解析式弹簧动画（AMLL `utils/spring.ts` 的等价实现）。
//!
//! 与逐帧数值积分不同，这里保存解析解的参数，任意时刻都能以 O(1) 求出
//! 弹簧位置和速度；`update(dt)` 只推进时间并做一次求值。这样：
//!
//! - 与帧率解耦，掉帧不会改变动画轨迹；
//! - 每个弹簧固定内存、无分配；
//! - 可精确复现 AMLL 的 `mass / damping / stiffness / soft` 语义。
//!
//! 所有时间单位为**秒**，位置/缩放单位为逻辑像素或百分比（由调用方决定）。

use std::f64::consts::E;

pub type Num = f64;

/// 弹簧参数。缺省值与 AMLL 默认值一致。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringParams {
    pub mass: Num,
    pub damping: Num,
    pub stiffness: Num,
    /// 强制使用过阻尼（临界阻尼）分支，对应 AMLL 的 `soft` 参数。
    pub soft: bool,
}

impl Default for SpringParams {
    fn default() -> Self {
        Self {
            mass: 1.0,
            damping: 10.0,
            stiffness: 100.0,
            soft: false,
        }
    }
}

impl SpringParams {
    pub const fn new(mass: Num, damping: Num, stiffness: Num) -> Self {
        Self {
            mass,
            damping,
            stiffness,
            soft: false,
        }
    }

    pub const fn soft(mut self) -> Self {
        self.soft = true;
        self
    }
}

#[derive(Debug, Clone, Copy)]
enum Solver {
    Static(Num),
    /// 过阻尼 / soft：`to - (delta + t * leftover) * exp(t * angular)`
    Overdamped {
        from: Num,
        to: Num,
        angular: Num,
        leftover: Num,
        delay: Num,
    },
    /// 欠阻尼：`to - (cos(t*dfm)*delta + sin(t*dfm)*leftover) * exp(t*dm)`
    Underdamped {
        from: Num,
        to: Num,
        dfm: Num,
        dm: Num,
        delta: Num,
        leftover: Num,
        delay: Num,
    },
}

impl Solver {
    #[inline]
    fn eval(self, t: Num) -> (Num, Num) {
        match self {
            Solver::Static(p) => (p, 0.0),
            Solver::Overdamped {
                from,
                to,
                angular,
                leftover,
                delay,
            } => {
                let t = t - delay;
                if t < 0.0 {
                    return (from, 0.0);
                }
                let e = E.powf(t * angular);
                let pos = to - (to - from + t * leftover) * e;
                let vel = -(leftover + angular * ((to - from) + t * leftover)) * e;
                (pos, vel)
            }
            Solver::Underdamped {
                from,
                to,
                dfm,
                dm,
                delta,
                leftover,
                delay,
            } => {
                let t = t - delay;
                if t < 0.0 {
                    return (from, 0.0);
                }
                let (sin, cos) = (t * dfm).sin_cos();
                let e = E.powf(t * dm);
                let f = cos * delta + sin * leftover;
                let f_prime = (-dfm * sin * delta + dfm * cos * leftover) * e + f * dm * e;
                (to - f * e, -f_prime)
            }
        }
    }
}

/// 一维弹簧。
#[derive(Debug, Clone)]
pub struct Spring {
    current_position: Num,
    target_position: Num,
    current_time: Num,
    params: SpringParams,
    solver: Solver,
    queue_params: Option<(SpringParams, Num)>,
    queue_position: Option<(Num, Num)>,
}

impl Spring {
    pub fn new(current_position: Num) -> Self {
        Self {
            current_position,
            target_position: current_position,
            current_time: 0.0,
            params: SpringParams::default(),
            solver: Solver::Static(current_position),
            queue_params: None,
            queue_position: None,
        }
    }

    pub fn with_params(mut self, params: SpringParams) -> Self {
        self.params = params;
        self.solver = Solver::Static(self.current_position);
        self
    }

    fn reset_solver(&mut self) {
        let (pos, vel) = self.solver.eval(self.current_time);
        self.current_position = pos;
        self.current_time = 0.0;
        self.solver = solve_spring(pos, vel, self.target_position, 0.0, self.params);
    }

    /// 是否已到达目标位置（位置与速度都足够小，且没有排队中的变更）。
    pub fn arrived(&mut self) -> bool {
        let (pos, vel) = self.solver.eval(self.current_time);
        (self.target_position - pos).abs() < 0.01
            && vel.abs() < 0.01
            && self.queue_params.is_none()
            && self.queue_position.is_none()
    }

    /// 立即把弹簧放到某位置（不产生动画）。
    pub fn set_position(&mut self, position: Num) {
        self.current_position = position;
        self.target_position = position;
        self.current_time = 0.0;
        self.solver = Solver::Static(position);
    }

    /// 推进 `delta` 秒。
    pub fn update(&mut self, delta: Num) {
        if !delta.is_finite() || delta <= 0.0 {
            return;
        }
        self.current_time += delta;
        self.current_position = self.solver.eval(self.current_time).0;

        if let Some((params, time)) = self.queue_params.as_mut() {
            *time -= delta;
            if *time <= 0.0 {
                let params = *params;
                self.queue_params = None;
                self.apply_params(params);
            }
        }
        if let Some((position, time)) = self.queue_position.as_mut() {
            *time -= delta;
            if *time <= 0.0 {
                let position = *position;
                self.queue_position = None;
                self.apply_target(position);
            }
        }
        if self.arrived() {
            self.set_position(self.target_position);
        }
    }

    fn apply_params(&mut self, params: SpringParams) {
        self.params = params;
        self.reset_solver();
    }

    pub fn update_params(&mut self, params: SpringParams, delay: Num) {
        if delay > 0.0 {
            self.queue_params = Some((params, delay));
        } else {
            self.queue_params = None;
            self.apply_params(params);
        }
    }

    fn apply_target(&mut self, target: Num) {
        self.target_position = target;
        self.reset_solver();
    }

    pub fn set_target_position(&mut self, target: Num, delay: Num) {
        if delay > 0.0 {
            self.queue_position = Some((target, delay));
        } else {
            self.queue_position = None;
            self.apply_target(target);
        }
    }

    #[inline]
    pub fn get_current_position(&self) -> Num {
        self.current_position
    }

    #[inline]
    pub fn target(&self) -> Num {
        self.target_position
    }
}

/// 解析解：从 `from`/`velocity` 到 `to`。语序与 AMLL 完全相同。
fn solve_spring(from: Num, velocity: Num, to: Num, delay: Num, params: SpringParams) -> Solver {
    let SpringParams {
        mass,
        damping,
        stiffness,
        soft,
    } = params;
    let delta = to - from;
    if soft || 1.0 <= damping / (2.0 * (stiffness * mass).sqrt()) {
        let angular = -(stiffness / mass).sqrt();
        let leftover = -angular * delta - velocity;
        return Solver::Overdamped {
            from,
            to,
            angular,
            leftover,
            delay,
        };
    }
    let damping_frequency = (4.0 * mass * stiffness - damping * damping).sqrt();
    let leftover = (damping * delta - 2.0 * mass * velocity) / damping_frequency;
    let dfm = 0.5 * damping_frequency / mass;
    let dm = -0.5 * damping / mass;
    Solver::Underdamped {
        from,
        to,
        dfm,
        dm,
        delta,
        leftover,
        delay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges_to_target() {
        let mut s = Spring::new(0.0).with_params(SpringParams::new(1.0, 10.0, 100.0));
        s.set_target_position(100.0, 0.0);
        let mut max_overshoot: Num = 0.0;
        for _ in 0..240 {
            s.update(1.0 / 60.0);
            max_overshoot = max_overshoot.max(s.get_current_position());
        }
        // 欠阻尼弹簧应有过冲，但最终收敛。
        assert!(
            max_overshoot > 100.0,
            "expected overshoot, got {max_overshoot}"
        );
        assert!(
            (s.get_current_position() - 100.0).abs() < 0.5,
            "position = {}",
            s.get_current_position()
        );
        assert!(s.arrived());
    }

    #[test]
    fn overdamped_never_overshoots() {
        let mut s = Spring::new(0.0).with_params(SpringParams::new(1.0, 30.0, 100.0));
        s.set_target_position(100.0, 0.0);
        for _ in 0..300 {
            s.update(1.0 / 60.0);
            assert!(s.get_current_position() <= 100.0001);
        }
    }

    #[test]
    fn delayed_target_waits() {
        let mut s = Spring::new(0.0);
        s.set_target_position(50.0, 0.1);
        for _ in 0..5 {
            s.update(1.0 / 60.0);
            assert_eq!(s.get_current_position(), 0.0);
        }
        for _ in 0..30 {
            s.update(1.0 / 60.0);
        }
        assert!(s.get_current_position() > 1.0);
    }

    #[test]
    fn frame_rate_independent() {
        let mut a = Spring::new(0.0);
        a.set_target_position(10.0, 0.0);
        let mut b = Spring::new(0.0);
        b.set_target_position(10.0, 0.0);
        for _ in 0..120 {
            a.update(1.0 / 60.0);
        }
        b.update(2.0);
        assert!((a.get_current_position() - b.get_current_position()).abs() < 1e-6);
    }
}
