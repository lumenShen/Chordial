//! 解析式弹簧动画（AMLL `utils/spring.ts` 的等价实现）。
//!
//! 与逐帧数值积分不同，这里保存解析解的参数，任意时刻都能以 O(1) 求出
//! 弹簧位置和速度；`update(dt)` 只推进时间并做一次求值。这样：
//!
//! - 与帧率解耦，掉帧不会改变动画轨迹；
//! - 每个弹簧固定内存、无分配；
//! - 可精确复现 AMLL 的 `mass / damping / stiffness / soft` 参数语义，
//!   并将非 soft 的真正过阻尼情况修正为双实根解析解。
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
    /// 强制使用临界阻尼分支，对应 AMLL 的 `soft` 参数。
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

    /// 参数是否可产生有限的物理解。质量与刚度必须为正，阻尼不可为负。
    pub fn is_valid(&self) -> bool {
        self.mass.is_finite()
            && self.mass > 0.0
            && self.damping.is_finite()
            && self.damping >= 0.0
            && self.stiffness.is_finite()
            && self.stiffness > 0.0
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.is_valid() {
            Ok(())
        } else if !self.mass.is_finite() || self.mass <= 0.0 {
            Err("spring mass must be finite and greater than zero")
        } else if !self.damping.is_finite() || self.damping < 0.0 {
            Err("spring damping must be finite and non-negative")
        } else {
            Err("spring stiffness must be finite and greater than zero")
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Solver {
    Static(Num),
    /// `soft` 或 ζ=1：AMLL 强制使用的临界阻尼分支。
    Critical {
        from: Num,
        to: Num,
        angular: Num,
        leftover: Num,
        delay: Num,
    },
    /// ζ>1：两个不同实特征根各自的指数项。
    Overdamped {
        from: Num,
        to: Num,
        root_slow: Num,
        root_fast: Num,
        coeff_slow: Num,
        coeff_fast: Num,
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
            Solver::Critical {
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
            Solver::Overdamped {
                from,
                to,
                root_slow,
                root_fast,
                coeff_slow,
                coeff_fast,
                delay,
            } => {
                let t = t - delay;
                if t < 0.0 {
                    return (from, 0.0);
                }
                let slow = E.powf(t * root_slow);
                let fast = E.powf(t * root_fast);
                (
                    to + coeff_slow * slow + coeff_fast * fast,
                    coeff_slow * root_slow * slow + coeff_fast * root_fast * fast,
                )
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
        let current_position = if current_position.is_finite() {
            current_position
        } else {
            0.0
        };
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
        if params.is_valid() {
            self.params = params;
            self.solver = Solver::Static(self.current_position);
        }
        self
    }

    fn reset_solver(&mut self) {
        let (pos, vel) = self.sample(self.current_time);
        self.current_position = pos;
        self.current_time = 0.0;
        self.solver = solve_spring(pos, vel, self.target_position, 0.0, self.params);
    }

    /// 是否已到达目标位置（位置与速度都足够小，且没有排队中的变更）。
    pub fn arrived(&mut self) -> bool {
        let (pos, vel) = self.sample(self.current_time);
        (self.target_position - pos).abs() < 0.01
            && vel.abs() < 0.01
            && self.queue_params.is_none()
            && self.queue_position.is_none()
    }

    /// 立即把弹簧放到某位置（不产生动画）。非有限位置会被忽略。
    pub fn set_position(&mut self, position: Num) {
        if !position.is_finite() {
            return;
        }
        self.current_position = position;
        self.target_position = position;
        self.current_time = 0.0;
        self.solver = Solver::Static(position);
    }

    /// 推进 `delta` 秒。无效时间步不会改变状态。
    pub fn update(&mut self, delta: Num) {
        if !delta.is_finite() || delta <= 0.0 {
            return;
        }
        let next_time = self.current_time + delta;
        if !next_time.is_finite() {
            return;
        }
        self.current_time = next_time;
        self.current_position = self.sample(self.current_time).0;

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
        if !params.is_valid() {
            return;
        }
        self.params = params;
        self.reset_solver();
    }

    pub fn update_params(&mut self, params: SpringParams, delay: Num) {
        if !params.is_valid() || !delay.is_finite() {
            return;
        }
        if delay > 0.0 {
            self.queue_params = Some((params, delay));
        } else {
            self.queue_params = None;
            self.apply_params(params);
        }
    }

    fn apply_target(&mut self, target: Num) {
        if !target.is_finite() {
            return;
        }
        self.target_position = target;
        self.reset_solver();
    }

    pub fn set_target_position(&mut self, target: Num, delay: Num) {
        if !target.is_finite() || !delay.is_finite() {
            return;
        }
        if delay > 0.0 {
            self.queue_position = Some((target, delay));
        } else {
            self.queue_position = None;
            self.apply_target(target);
        }
    }

    /// 不修改状态，求当前解析解在 `time` 秒处的位置与速度。
    ///
    /// `time` 相对于当前求解器的起点；传入负数可用于检查延迟段。非有限时间
    /// 返回当前已提交的位置与零速度。
    #[inline]
    pub fn sample(&self, time: Num) -> (Num, Num) {
        if time.is_finite() {
            self.solver.eval(time)
        } else {
            (self.current_position, 0.0)
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

    #[inline]
    pub const fn params(&self) -> SpringParams {
        self.params
    }
}

/// 解析解：从 `from`/`velocity` 到 `to`。语序与 AMLL 完全相同。
fn solve_spring(from: Num, velocity: Num, to: Num, delay: Num, params: SpringParams) -> Solver {
    debug_assert!(params.is_valid());
    let SpringParams {
        mass,
        damping,
        stiffness,
        soft,
    } = params;
    let delta = to - from;
    if soft || damping == 2.0 * (stiffness * mass).sqrt() {
        // AMLL 的 soft 分支强制复用临界阻尼公式，即使物理 ζ>1 也不改变语义。
        let angular = -(stiffness / mass).sqrt();
        let leftover = -angular * delta - velocity;
        return Solver::Critical {
            from,
            to,
            angular,
            leftover,
            delay,
        };
    }
    if damping * damping > 4.0 * mass * stiffness {
        // 真正过阻尼：x(t) = to + A*exp(r_slow*t) + B*exp(r_fast*t)。
        // 系数由 x(0)=from、x'(0)=velocity 独立求出。
        let discriminant = (damping * damping - 4.0 * mass * stiffness).sqrt();
        let root_slow = (-damping + discriminant) / (2.0 * mass);
        let root_fast = (-damping - discriminant) / (2.0 * mass);
        let coeff_slow = (velocity + root_fast * delta) / (root_slow - root_fast);
        let coeff_fast = -delta - coeff_slow;
        return Solver::Overdamped {
            from,
            to,
            root_slow,
            root_fast,
            coeff_slow,
            coeff_fast,
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
        let mut stepped = Spring::new(0.0);
        stepped.set_target_position(10.0, 0.0);
        let mut direct = Spring::new(0.0);
        direct.set_target_position(10.0, 0.0);

        // 先对照尚未到达的固定时刻，确认轨迹本身与帧步长无关。
        for _ in 0..30 {
            stepped.update(1.0 / 60.0);
        }
        let expected = direct.sample(0.5);
        direct.update(0.5);
        assert!(!stepped.arrived());
        assert!(!direct.arrived());
        assert!((stepped.get_current_position() - expected.0).abs() < 1e-6);
        assert!((stepped.get_current_position() - direct.get_current_position()).abs() < 1e-6);

        // 到达阈值后 update 会吸附；两条路径必须在同一时刻一起吸附。
        for _ in 0..90 {
            stepped.update(1.0 / 60.0);
        }
        direct.update(1.5);
        assert!(stepped.arrived());
        assert!(direct.arrived());
        assert_eq!(stepped.get_current_position(), stepped.target());
        assert_eq!(direct.get_current_position(), direct.target());
        assert!((stepped.get_current_position() - direct.get_current_position()).abs() < 1e-6);
    }

    #[test]
    fn frame_rate_independent_at_300hz() {
        // 300Hz 显示下每帧 3.33ms；解析弹簧轨迹必须与一次性求值一致。
        let mut stepped = Spring::new(0.0);
        stepped.set_target_position(10.0, 0.0);
        let mut direct = Spring::new(0.0);
        direct.set_target_position(10.0, 0.0);

        for _ in 0..75 {
            stepped.update(1.0 / 300.0);
        }
        let expected = direct.sample(0.25);
        direct.update(0.25);
        assert!(!stepped.arrived());
        assert!(!direct.arrived());
        assert!((stepped.get_current_position() - expected.0).abs() < 1e-6);

        for _ in 0..225 {
            stepped.update(1.0 / 300.0);
        }
        direct.update(0.75);
        assert_eq!(stepped.arrived(), direct.arrived());
        assert!(
            (stepped.get_current_position() - direct.get_current_position()).abs() < 1e-6,
            "300Hz={} 1s={}",
            stepped.get_current_position(),
            direct.get_current_position()
        );
    }

    #[test]
    fn public_sample_is_pure_and_matches_stepping() {
        let mut spring = Spring::new(0.0).with_params(SpringParams::new(1.0, 30.0, 100.0));
        spring.set_target_position(80.0, 0.0);
        let first = spring.sample(0.2);
        assert_eq!(spring.get_current_position(), 0.0);
        assert_eq!(spring.current_time, 0.0);
        spring.update(0.2);
        assert!((spring.get_current_position() - first.0).abs() < 1e-12);

        let second = spring.sample(0.8);
        spring.update(0.6);
        assert!((spring.get_current_position() - second.0).abs() < 1e-12);
        assert_eq!(
            spring.sample(Num::NAN),
            (spring.get_current_position(), 0.0)
        );
    }

    /// 独立参考公式：直接解 m*x'' + c*x' + k*(x-to) = 0，不复用生产求解器。
    fn reference_solution(
        params: SpringParams,
        from: Num,
        velocity: Num,
        to: Num,
        t: Num,
    ) -> (Num, Num) {
        if t < 0.0 {
            return (from, 0.0);
        }
        let SpringParams {
            mass,
            damping,
            stiffness,
            soft,
        } = params;
        let delta = to - from;
        let critical = 2.0 * (stiffness * mass).sqrt();
        if soft || damping == critical {
            let omega = (stiffness / mass).sqrt();
            let leftover = omega * delta - velocity;
            let e = (-omega * t).exp();
            let tail = delta + t * leftover;
            return (to - tail * e, -(leftover - omega * tail) * e);
        }
        if damping > critical {
            return true_overdamped_reference(params, from, velocity, to, t);
        }

        let wd = 0.5 * (4.0 * mass * stiffness - damping * damping).sqrt() / mass;
        let alpha = -0.5 * damping / mass;
        let leftover = (damping * delta - 2.0 * mass * velocity)
            / (4.0 * mass * stiffness - damping * damping).sqrt();
        let (sin, cos) = (wd * t).sin_cos();
        let e = (alpha * t).exp();
        let f = cos * delta + sin * leftover;
        let f_prime = wd * (-sin * delta + cos * leftover);
        (to - f * e, -(f_prime + alpha * f) * e)
    }

    fn true_overdamped_reference(
        params: SpringParams,
        from: Num,
        velocity: Num,
        to: Num,
        t: Num,
    ) -> (Num, Num) {
        let mass = params.mass;
        let damping = params.damping;
        let stiffness = params.stiffness;
        let delta = to - from;
        let discriminant = (damping * damping - 4.0 * mass * stiffness).sqrt();
        let root_slow = (-damping + discriminant) / (2.0 * mass);
        let root_fast = (-damping - discriminant) / (2.0 * mass);
        let a = (velocity - root_fast * (-delta)) / (root_slow - root_fast);
        let b = -delta - a;
        let slow = (root_slow * t).exp();
        let fast = (root_fast * t).exp();
        (
            to + a * slow + b * fast,
            a * root_slow * slow + b * root_fast * fast,
        )
    }

    fn assert_sample_close(actual: (Num, Num), expected: (Num, Num)) {
        assert!(
            (actual.0 - expected.0).abs() < 1e-9,
            "position: actual={}, expected={}",
            actual.0,
            expected.0
        );
        assert!(
            (actual.1 - expected.1).abs() < 1e-8,
            "velocity: actual={}, expected={}",
            actual.1,
            expected.1
        );
    }

    #[test]
    fn analytic_solution_matches_independent_reference_in_all_regimes() {
        let cases = [
            SpringParams::new(1.0, 10.0, 100.0),       // underdamped
            SpringParams::new(1.0, 20.0, 100.0),       // critically damped
            SpringParams::new(1.0, 30.0, 100.0),       // truly overdamped
            SpringParams::new(1.0, 5.0, 100.0).soft(), // soft forces critical
        ];
        for params in cases {
            let solver = solve_spring(3.0, -7.0, 50.0, 0.0, params);
            for t in [0.0, 0.01, 0.1, 0.5, 1.0, 2.0] {
                assert_sample_close(
                    solver.eval(t),
                    reference_solution(params, 3.0, -7.0, 50.0, t),
                );
            }
        }
    }

    #[test]
    fn true_overdamped_solution_uses_two_distinct_roots() {
        let params = SpringParams::new(1.0, 30.0, 100.0);
        let solver = solve_spring(3.0, -7.0, 50.0, 0.0, params);
        for t in [0.0, 0.01, 0.1, 0.5, 1.0, 2.0] {
            assert_sample_close(
                solver.eval(t),
                true_overdamped_reference(params, 3.0, -7.0, 50.0, t),
            );
        }
        let critical = reference_solution(params.soft(), 3.0, -7.0, 50.0, 0.25);
        assert!((solver.eval(0.25).0 - critical.0).abs() > 1e-3);
    }

    #[test]
    fn param_update_preserves_current_position_and_velocity() {
        let initial = SpringParams::new(1.0, 10.0, 100.0);
        let updated = SpringParams::new(0.9, 15.0, 90.0);
        let mut spring = Spring::new(0.0).with_params(initial);
        spring.set_target_position(100.0, 0.0);
        spring.update(0.12);
        let (position, velocity) = spring.sample(spring.current_time);
        assert!(velocity.abs() > 1.0);

        spring.update_params(updated, 0.0);
        assert_eq!(spring.get_current_position(), position);
        assert_sample_close(spring.sample(0.0), (position, velocity));
        assert_sample_close(
            spring.sample(0.001),
            reference_solution(updated, position, velocity, 100.0, 0.001),
        );
    }

    #[test]
    fn nonzero_velocity_survives_retargeting() {
        let params = SpringParams::new(0.9, 12.0, 140.0);
        let mut spring = Spring::new(0.0).with_params(params);
        spring.set_target_position(100.0, 0.0);
        spring.update(0.12);
        let from = spring.current_position;
        let velocity = spring.solver.eval(spring.current_time).1;
        assert!(velocity.abs() > 1.0);

        spring.set_target_position(-20.0, 0.0);
        let solver = solve_spring(from, velocity, -20.0, 0.0, params);
        for t in [0.0, 0.03, 0.2, 0.8] {
            assert_sample_close(
                spring.solver.eval(t),
                reference_solution(params, from, velocity, -20.0, t),
            );
            assert_sample_close(spring.solver.eval(t), solver.eval(t));
        }
    }

    #[test]
    fn analytic_delay_holds_position_and_zero_velocity() {
        let params = SpringParams::new(1.0, 30.0, 100.0);
        let solver = solve_spring(4.0, 9.0, 40.0, 0.1, params);
        assert_eq!(solver.eval(0.0), (4.0, 0.0));
        assert_eq!(solver.eval(0.099), (4.0, 0.0));
        assert_sample_close(
            solver.eval(0.1),
            true_overdamped_reference(params, 4.0, 9.0, 40.0, 0.0),
        );
        assert_sample_close(
            solver.eval(0.35),
            true_overdamped_reference(params, 4.0, 9.0, 40.0, 0.25),
        );
    }

    #[test]
    fn queued_params_and_targets_use_latest_value_and_can_be_cancelled() {
        let first = SpringParams::new(1.0, 10.0, 100.0);
        let latest = SpringParams::new(2.0, 20.0, 200.0);
        let mut spring = Spring::new(0.0).with_params(first);
        spring.update_params(first, 0.1);
        spring.update_params(latest, 0.2);
        spring.update(0.1);
        assert_eq!(spring.params, first);
        spring.update(0.11);
        assert_eq!(spring.params, latest);

        spring.set_target_position(10.0, 0.1);
        spring.set_target_position(20.0, 0.2);
        spring.update(0.1);
        assert_eq!(spring.target(), 0.0);
        spring.update(0.11);
        assert_eq!(spring.target(), 20.0);

        spring.update_params(first, 0.2);
        spring.update_params(latest, 0.0);
        assert!(spring.queue_params.is_none());
        assert_eq!(spring.params, latest);
        spring.set_target_position(30.0, 0.2);
        spring.set_target_position(40.0, 0.0);
        assert!(spring.queue_position.is_none());
        assert_eq!(spring.target(), 40.0);
    }

    #[test]
    fn invalid_params_targets_and_dt_are_rejected_without_poisoning_state() {
        let invalid = [
            SpringParams::new(0.0, 10.0, 100.0),
            SpringParams::new(-1.0, 10.0, 100.0),
            SpringParams::new(1.0, -1.0, 100.0),
            SpringParams::new(1.0, 10.0, 0.0),
            SpringParams::new(Num::NAN, 10.0, 100.0),
            SpringParams::new(1.0, Num::INFINITY, 100.0),
        ];
        for params in invalid {
            let mut spring = Spring::new(0.0).with_params(params);
            assert_eq!(spring.params, SpringParams::default());
            spring.update_params(params, 0.0);
            spring.update_params(params, 0.1);
            assert_eq!(spring.params, SpringParams::default());
            assert!(spring.queue_params.is_none());
            spring.set_target_position(10.0, 0.0);
            spring.update(0.1);
            assert!(spring.get_current_position().is_finite());
        }

        let mut spring = Spring::new(2.0);
        spring.update_params(SpringParams::default(), Num::NAN);
        assert!(spring.queue_params.is_none());
        assert_eq!(spring.params, SpringParams::default());
        assert_eq!(Spring::new(Num::NAN).get_current_position(), 0.0);
        let before = (
            spring.current_time,
            spring.current_position,
            spring.target(),
        );
        for dt in [0.0, -1.0, Num::NAN, Num::INFINITY] {
            spring.update(dt);
            assert_eq!(
                (
                    spring.current_time,
                    spring.current_position,
                    spring.target()
                ),
                before
            );
        }
        spring.set_target_position(Num::NAN, 0.0);
        spring.set_target_position(10.0, Num::NAN);
        spring.set_target_position(10.0, Num::INFINITY);
        assert_eq!(spring.target(), 2.0);
        assert!(spring.queue_position.is_none());
        spring.set_position(Num::NAN);
        assert_eq!(spring.get_current_position(), 2.0);
    }
}
