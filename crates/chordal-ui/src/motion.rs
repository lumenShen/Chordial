//! 统一的状态型动画弹簧。
//!
//! 这里是 UI 侧所有「有状态、会被打断」动画的唯一物理核心：解析解弹簧，
//! 任意时刻 O(1) 采样位置与速度，帧率无关；打断时用当前 `(position, velocity)`
//! 作为新弹簧的初值，因此快速连点、手势接弹簧都不会跳变或丢失惯性。
//!
//! 不负责 AMLL 歌词动画——那部分跟随音乐时间轴，保留原有的贝塞尔曲线。
//!
//! ## 生命周期
//!
//! - `enter`：从给定位置（通常是当前采样值）以给定速度出发；没有参考速度时传 0。
//! - `leave`：离开当前状态，仍然保留速度，避免原路返回时先顿一下。
//! - `interrupt`：动画途中换目标，逐通道保留位置与速度。
//!
//! 三个动作共用 [`MotionSpring::retarget`] / [`MotionSpring::retarget_with`]，
//! 区别只在调用方给的初值来源。

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::layout_morph::{StageGeometry, StageMode};

/// 弹簧预设。不同预设只决定基准角频率与阻尼比，倍率由 [`MotionTuning`] 统一施加。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MotionPreset {
    /// 紧凑：起步快、几乎不过冲。
    Compact,
    /// 均衡：轻微过冲，接近原先的 ease-out 观感。
    #[default]
    Balanced,
    /// 柔和：临界阻尼，不过冲。
    Soft,
    /// 弹性：明显但克制的回弹。
    Bouncy,
}

impl MotionPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "紧凑",
            Self::Balanced => "均衡",
            Self::Soft => "柔和",
            Self::Bouncy => "弹性",
        }
    }

    /// 基准角频率（rad/s）与阻尼比，以 240ms 动画为基准。
    fn base(self) -> (f32, f32) {
        match self {
            Self::Compact => (26.0, 0.90),
            Self::Balanced => (20.0, 0.72),
            Self::Soft => (15.0, 1.00),
            Self::Bouncy => (18.0, 0.45),
        }
    }
}

/// 用户可调的动画手感。已开始的动画会冻结创建时的参数，改动只影响新动画。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionTuning {
    pub preset: MotionPreset,
    /// 时长倍率：> 1 慢放，< 1 加速。
    pub duration: f32,
    /// 超伸倍率：> 1 回弹更明显，< 1 更稳。
    pub overshoot: f32,
}

impl Default for MotionTuning {
    fn default() -> Self {
        Self {
            preset: MotionPreset::Balanced,
            duration: 1.0,
            overshoot: 1.0,
        }
    }
}

impl MotionTuning {
    pub fn new(preset: MotionPreset, duration: f32, overshoot: f32) -> Self {
        Self {
            preset,
            duration: duration.max(0.01),
            overshoot: overshoot.max(0.01),
        }
    }

    /// 把一条「原本 `base_ms` 毫秒」的动画换算成弹簧参数。
    ///
    /// `ω' = ω × (240 / base_ms) / duration`，`ζ' = clamp(ζ / overshoot, 0.05, 1)`。
    /// 时间倍率只改频率、不改阻尼比，因此慢放不会顺带把回弹也放大。
    pub fn spec(self, base_ms: f32) -> MotionSpec {
        let (omega, zeta) = self.preset.base();
        let omega = (omega * (240.0 / base_ms.max(1.0)) / self.duration).max(0.1);
        let zeta = (zeta / self.overshoot).clamp(0.05, 1.0);
        MotionSpec { omega, zeta }
    }

    /// 直接构造一条弹簧；`value_tol` / `velocity_tol` 决定何时判定收敛。
    #[allow(clippy::too_many_arguments)] // 参数都是弹簧初值/容差，拆成结构体反而更难在调用点阅读
    pub fn spring(
        self,
        base_ms: f32,
        from: f32,
        to: f32,
        v0: f32,
        now: Instant,
        value_tol: f32,
        velocity_tol: f32,
    ) -> MotionSpring {
        MotionSpring::new(
            from,
            to,
            v0,
            now,
            self.spec(base_ms),
            value_tol,
            velocity_tol,
        )
    }
}

/// 解析解参数。`zeta >= 1` 走临界阻尼分支，否则走欠阻尼分支。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSpec {
    pub omega: f32,
    pub zeta: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSample {
    pub value: f32,
    pub velocity: f32,
}

/// 一路标量弹簧。
#[derive(Debug, Clone, Copy)]
pub struct MotionSpring {
    from: f32,
    to: f32,
    v0: f32,
    start: Instant,
    spec: MotionSpec,
    hard_timeout: f32,
    value_tol: f32,
    velocity_tol: f32,
}

impl MotionSpring {
    pub fn new(
        from: f32,
        to: f32,
        v0: f32,
        now: Instant,
        spec: MotionSpec,
        value_tol: f32,
        velocity_tol: f32,
    ) -> Self {
        // 6 个时间常数后残差已远小于容差；倍率越快，强制收敛也越快。
        let hard_timeout = (6.0 / (spec.zeta * spec.omega)).clamp(0.18, 3.0);
        Self {
            from,
            to,
            v0,
            start: now,
            spec,
            hard_timeout,
            value_tol,
            velocity_tol,
        }
    }

    /// 起点放到未来即实现延迟；延迟期采样保持 `from`、速度为 0。
    pub fn delayed(mut self, delay_secs: f32) -> Self {
        self.start += std::time::Duration::from_secs_f32(delay_secs.max(0.0));
        self
    }

    #[inline]
    pub fn target(&self) -> f32 {
        self.to
    }

    /// O(1) 解析采样：任意时刻都能求值，不依赖上一帧。
    pub fn sample(&self, now: Instant) -> MotionSample {
        if now < self.start {
            return MotionSample {
                value: self.from,
                velocity: 0.0,
            };
        }
        let t = now.saturating_duration_since(self.start).as_secs_f32();
        let x0 = self.from - self.to;
        let MotionSpec { omega, zeta } = self.spec;
        if zeta >= 1.0 {
            let decay = (-omega * t).exp();
            let b = self.v0 + omega * x0;
            let offset = x0 + b * t;
            MotionSample {
                value: self.to + offset * decay,
                velocity: (b - omega * offset) * decay,
            }
        } else {
            let wd = omega * (1.0 - zeta * zeta).sqrt();
            let decay = (-zeta * omega * t).exp();
            let phase = wd * t;
            let (sin, cos) = phase.sin_cos();
            let b = (self.v0 + zeta * omega * x0) / wd;
            let (s, c) = (x0 * cos + b * sin, -x0 * wd * sin + b * wd * cos);
            MotionSample {
                value: self.to + decay * s,
                velocity: decay * (c - zeta * omega * s),
            }
        }
    }

    #[inline]
    pub fn value(&self, now: Instant) -> f32 {
        self.sample(now).value
    }

    pub fn done(&self, now: Instant) -> bool {
        if now < self.start {
            return false;
        }
        if now.saturating_duration_since(self.start).as_secs_f32() >= self.hard_timeout {
            return true;
        }
        let sample = self.sample(now);
        (sample.value - self.to).abs() <= self.value_tol
            && sample.velocity.abs() <= self.velocity_tol
    }

    pub fn active(&self, now: Instant) -> bool {
        // 延迟期保持的是静态画面，不能因为它「尚未完成」就持续请求帧。
        now >= self.start && !self.done(now)
    }

    /// 打断：从当前位置与速度无缝换目标。
    pub fn retarget(&mut self, to: f32, now: Instant) {
        let sample = self.sample(now);
        self.retarget_with(to, sample.velocity, now);
    }

    /// 打断并指定初速度：手势交接时传手指/鼠标速度；没有参考速度传 0。
    pub fn retarget_with(&mut self, to: f32, velocity: f32, now: Instant) {
        let value = self.sample(now).value;
        *self = Self::new(
            value,
            to,
            velocity,
            now,
            self.spec,
            self.value_tol,
            self.velocity_tol,
        );
    }
}

/// 进度型通道（0..=1）的常用容差。
pub const PROGRESS_TOL: (f32, f32) = (0.002, 0.05);
/// 像素型通道的常用容差。
pub const PIXEL_TOL: (f32, f32) = (1.0, 24.0);

/// `StageGeometry` 的 14 个通道一起走同一条弹簧，几何不会散架。
#[derive(Debug, Clone)]
pub struct StageSpring {
    pub from: StageGeometry,
    pub to: StageGeometry,
    channels: [MotionSpring; 14],
    from_mode: StageMode,
    to_mode: StageMode,
}

impl StageSpring {
    pub fn new(
        from: StageGeometry,
        to: StageGeometry,
        from_mode: StageMode,
        to_mode: StageMode,
        now: Instant,
        spec: MotionSpec,
    ) -> Self {
        let a = geometry_channels(from);
        let b = geometry_channels(to);
        Self {
            from,
            to,
            channels: std::array::from_fn(|i| {
                MotionSpring::new(a[i], b[i], 0.0, now, spec, PIXEL_TOL.0, PIXEL_TOL.1)
            }),
            from_mode,
            to_mode,
        }
    }

    pub fn sample(&self, now: Instant) -> StageGeometry {
        let mut c = [0.0f32; 14];
        for (slot, spring) in c.iter_mut().zip(self.channels.iter()) {
            *slot = spring.value(now);
        }
        channels_geometry(c)
    }

    /// 回归测试用：断言打断前后 14 个通道的速度都连续。
    #[allow(dead_code)]
    pub fn velocity(&self, now: Instant) -> StageGeometry {
        let mut c = [0.0f32; 14];
        for (slot, spring) in c.iter_mut().zip(self.channels.iter()) {
            *slot = spring.sample(now).velocity;
        }
        channels_geometry(c)
    }

    /// 打断：逐通道保留位置与速度。
    pub fn retarget(&mut self, to: StageGeometry, to_mode: StageMode, now: Instant) {
        self.from = self.sample(now);
        self.to = to;
        self.from_mode = self.to_mode;
        self.to_mode = to_mode;
        let next = geometry_channels(to);
        for (spring, value) in self.channels.iter_mut().zip(next) {
            spring.retarget(value, now);
        }
    }

    pub fn done(&self, now: Instant) -> bool {
        self.channels.iter().all(|spring| spring.done(now))
    }

    pub fn active(&self, now: Instant) -> bool {
        !self.done(now)
    }

    pub fn touches_wide(&self) -> bool {
        self.to_mode == StageMode::WideSplit || self.from_mode == StageMode::WideSplit
    }

    pub fn touches_compact(&self) -> bool {
        matches!(
            self.to_mode,
            StageMode::CompactArt | StageMode::CompactQueue
        ) || matches!(
            self.from_mode,
            StageMode::CompactArt | StageMode::CompactQueue
        )
    }
}

fn geometry_channels(g: StageGeometry) -> [f32; 14] {
    [
        g.cover.x,
        g.cover.y,
        g.cover.w,
        g.cover.h,
        g.cover_radius,
        g.info.x,
        g.info.y,
        g.info.w,
        g.info.h,
        g.title_size,
        g.artist_size,
        g.line_gap,
        g.title_weight,
        g.artist_weight,
    ]
}

fn channels_geometry(c: [f32; 14]) -> StageGeometry {
    StageGeometry {
        cover: crate::layout_morph::StageRect {
            x: c[0],
            y: c[1],
            w: c[2],
            h: c[3],
        },
        cover_radius: c[4],
        info: crate::layout_morph::StageRect {
            x: c[5],
            y: c[6],
            w: c[7],
            h: c[8],
        },
        title_size: c[9],
        artist_size: c[10],
        line_gap: c[11],
        title_weight: c[12],
        artist_weight: c[13],
    }
}

/// 指针/鼠标速度追踪。手势松手与点击打断共用，避免各状态机各写一套平滑。
#[derive(Debug, Clone, Default)]
pub struct VelocityTracker {
    last: Option<(f32, Instant)>,
    filtered: f32,
}

impl VelocityTracker {
    pub fn push(&mut self, position: f32, now: Instant) {
        if let Some((previous, at)) = self.last {
            let dt = now.saturating_duration_since(at).as_secs_f32();
            if dt > 0.0 && dt < 0.25 {
                let instant = (position - previous) / dt;
                self.filtered = self.filtered * 0.55 + instant * 0.45;
            }
        }
        self.last = Some((position, now));
    }

    /// 指针已静止超过 100ms 时不再把旧速度带给弹簧。
    pub fn velocity_at(&self, now: Instant) -> f32 {
        let stale = self
            .last
            .is_some_and(|(_, at)| now.saturating_duration_since(at).as_secs_f32() > 0.1);
        if stale { 0.0 } else { self.filtered }
    }

    pub fn reset(&mut self) {
        self.last = None;
        self.filtered = 0.0;
    }
}

/// 把弹簧驱动的空间进度 `value` 从 `[from, to]` 映射为 0..=1 的显隐通道。
///
/// 这是空间映射而不是时间缓动：输入的 `value` 本身来自弹簧，不再另起一条
/// 时间曲线。`from > to` 表示反向区间。
pub fn ramp(value: f32, from: f32, to: f32) -> f32 {
    if (to - from).abs() < 1e-6 {
        return if value >= to { 1.0 } else { 0.0 };
    }
    let t = ((value - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 读取当前用户手感设置。已开始的动画不受设置改动影响，只有新动画会读取。
pub fn current_tuning(cx: &gpui::App) -> MotionTuning {
    cx.global::<crate::main_state::Globals>().motion_tuning
}

/// 一帧里产生的完成事件，供 app 驱动页面切换等副作用。
#[derive(Debug, Clone, Copy, Default)]
pub struct MotionEvents {
    pub immersive_exit_done: bool,
    pub phone_sheet_done: bool,
    /// 收起完成时的落点，app 据此切到 mini / browse 页。
    pub phone_sheet_target: Option<f32>,
}

/// 所有已迁移的状态型通道集中在这里；帧泵只问一次 `any_active`。
#[derive(Debug, Default)]
pub struct MotionSystem {
    pub stage: Option<StageSpring>,
    pub cover: Option<MotionSpring>,
    pub drawer: Option<MotionSpring>,
    pub portrait: Option<MotionSpring>,
    pub portrait_enter: Option<MotionSpring>,
    pub eq_enter: Option<MotionSpring>,
    pub dock: Option<MotionSpring>,
    /// 当前 dock 弹簧是在收起（退出沉浸层）还是展开。
    pub dock_collapsing: bool,
    pub chrome: Option<MotionSpring>,
    pub hint: Option<MotionSpring>,
    pub phone_sheet: Option<MotionSpring>,
}

impl MotionSystem {
    /// 清理已收敛的通道并返回本帧完成事件。
    pub fn advance(&mut self, now: Instant) -> MotionEvents {
        let mut events = MotionEvents::default();
        if self.stage.as_ref().is_some_and(|s| s.done(now)) {
            self.stage = None;
        }
        if self.cover.is_some_and(|s| s.done(now)) {
            self.cover = None;
        }
        if self.drawer.is_some_and(|s| s.done(now)) {
            self.drawer = None;
        }
        if self.portrait.is_some_and(|s| s.done(now)) {
            self.portrait = None;
        }
        if self.portrait_enter.is_some_and(|s| s.done(now)) {
            self.portrait_enter = None;
        }
        if self.eq_enter.is_some_and(|s| s.done(now)) {
            self.eq_enter = None;
        }
        if self.chrome.is_some_and(|s| s.done(now)) {
            self.chrome = None;
        }
        if self.hint.is_some_and(|s| s.done(now)) {
            self.hint = None;
        }
        if self.dock.is_some_and(|s| s.done(now)) {
            self.dock = None;
            if self.dock_collapsing {
                events.immersive_exit_done = true;
            }
            self.dock_collapsing = false;
        }
        if self.phone_sheet.as_ref().is_some_and(|s| s.done(now)) {
            events.phone_sheet_target = self.phone_sheet.as_ref().map(|s| s.target());
            self.phone_sheet = None;
            events.phone_sheet_done = true;
        }
        events
    }

    pub fn any_active(&self, now: Instant) -> bool {
        self.stage.as_ref().is_some_and(|s| s.active(now))
            || self.cover.is_some_and(|s| s.active(now))
            || self.drawer.is_some_and(|s| s.active(now))
            || self.portrait.is_some_and(|s| s.active(now))
            || self.portrait_enter.is_some_and(|s| s.active(now))
            || self.eq_enter.is_some_and(|s| s.active(now))
            || self.dock.is_some_and(|s| s.active(now))
            || self.chrome.is_some_and(|s| s.active(now))
            || self.hint.is_some_and(|s| s.active(now))
            || self.phone_sheet.is_some_and(|s| s.active(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn spec() -> MotionSpec {
        MotionTuning::default().spec(240.0)
    }

    #[test]
    fn spring_starts_at_from_and_converges_to_target() {
        let now = Instant::now();
        let spring = MotionSpring::new(0.0, 100.0, 0.0, now, spec(), 0.01, 0.1);
        assert_eq!(spring.value(now), 0.0);
        assert!(!spring.done(now));
        assert!((spring.value(now + Duration::from_secs(2)) - 100.0).abs() < 0.01);
        assert!(spring.done(now + Duration::from_secs(2)));
    }

    #[test]
    fn retarget_preserves_position_and_velocity() {
        let now = Instant::now();
        let mut spring = MotionSpring::new(0.0, 100.0, 0.0, now, spec(), 0.01, 0.1);
        let mid = now + Duration::from_millis(80);
        let before = spring.sample(mid);
        spring.retarget(200.0, mid);
        let after = spring.sample(mid);
        assert!((after.value - before.value).abs() < 1e-4);
        assert!((after.velocity - before.velocity).abs() < 1e-4);
    }

    #[test]
    fn explicit_velocity_is_used_when_given() {
        let now = Instant::now();
        let mut spring = MotionSpring::new(0.0, 100.0, 0.0, now, spec(), 0.01, 0.1);
        spring.retarget_with(100.0, 42.0, now);
        assert!((spring.sample(now).velocity - 42.0).abs() < 1e-4);
    }

    #[test]
    fn delayed_spring_stays_static_and_inactive_during_hold() {
        let now = Instant::now();
        let spring = MotionSpring::new(1.0, 0.0, 0.0, now, spec(), 0.01, 0.1).delayed(0.5);
        assert!(!spring.active(now));
        assert!(!spring.active(now + Duration::from_millis(100)));
        assert_eq!(spring.value(now + Duration::from_millis(100)), 1.0);
        assert!(spring.active(now + Duration::from_millis(600)));
    }

    #[test]
    fn sampling_is_frame_rate_independent() {
        let now = Instant::now();
        let spring = MotionSpring::new(0.0, 100.0, 0.0, now, spec(), 0.01, 0.1);
        // 解析采样只看绝对时间，60Hz 与 300Hz 网格的同一时刻必须逐位一致。
        let at_60 = now + Duration::from_micros(16_000 * 60);
        let at_300 = now + Duration::from_micros(3_200 * 300);
        assert!((spring.value(at_60) - spring.value(at_300)).abs() < 1e-5);
    }

    #[test]
    fn duration_multiplier_stretches_time_without_changing_overshoot() {
        let tuning = MotionTuning::default();
        let slow = MotionTuning::new(tuning.preset, 2.0, 1.0).spec(240.0);
        let normal = tuning.spec(240.0);
        assert!((slow.omega - normal.omega / 2.0).abs() < 1e-5);
        assert!((slow.zeta - normal.zeta).abs() < 1e-5);
    }

    #[test]
    fn overshoot_multiplier_changes_damping_ratio() {
        let tuning = MotionTuning::default();
        let bouncy = MotionTuning::new(tuning.preset, 1.0, 2.0).spec(240.0);
        assert!(bouncy.zeta < tuning.spec(240.0).zeta);
        let calm = MotionTuning::new(tuning.preset, 1.0, 0.5).spec(240.0);
        assert!(calm.zeta > tuning.spec(240.0).zeta);
    }
}
