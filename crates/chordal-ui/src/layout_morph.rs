//! 共享元素布局动画（anime.js `createLayout` / FLIP 的 GPUI 等价实现）。
//!
//! GPUI 没有可查询的 DOM 布局树，因此这里不读取「变换前后」的真实 rect，而是
//! 把每个沉浸模态的共享元素几何写成纯函数：
//!
//! ```text
//! immersive_geometry(mode, viewport) -> StageGeometry { cover, info, ... }
//! ```
//!
//! 切换模态时记录 First（切换瞬间的插值几何）与 Last（目标模态几何），
//! 渲染阶段用 `ease_out_quint` 插值一个覆盖层舞台。页面本身在动画期间把
//! 自带的封面 / 曲名 / 艺人设为透明（只保留占位），结束后再由页面按同一份
//! 几何函数绘制，保证「动画终点」与「静态布局」严格一致、不会跳动。
//!
//! 这对应 anime.js Layout 的 `layout.record()` + `layout.animate()`：
//! 记录前态 → 改布局 → 在两者之间补间。区别只是这里共享的是封面与曲目文案，
//! 而不是整个容器的 flex/grid 属性。

use std::time::{Duration, Instant};

/// 沉浸层里共享元素几何不同的布局模态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageMode {
    /// 竖屏：封面态（大封面 + 标题在下方）。
    CompactArt,
    /// 竖屏：播放队列（54px 小封面 + 标题在页头）。
    CompactQueue,
    /// 宽屏：歌词打开，播放列位于左侧 45% 区域。这是布局 1。
    WideSplit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StageRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl StageRect {
    pub fn is_zero(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            x: lerp(self.x, other.x, t),
            y: lerp(self.y, other.y, t),
            w: lerp(self.w, other.w, t),
            h: lerp(self.h, other.h, t),
        }
    }

    /// 平移到另一个坐标空间（例如把窗口坐标的共享舞台移进缩小的卡片）。
    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

/// 一个模态下共享元素的完整几何。
///
/// `info` 是曲名/艺人文案块的左上角与可用宽度；高度由字号与行高推导，
/// 因此动画期间文字会随封面一起连续缩放，而不是在两份文本之间硬切。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageGeometry {
    pub cover: StageRect,
    pub cover_radius: f32,
    /// 文案块的左上角；`w` 为可用宽度，`h` 为按行高推导出的高度。
    pub info: StageRect,
    pub title_size: f32,
    pub artist_size: f32,
    /// 标题行与艺人行之间的固定间距（竖屏队列页为 2px，其余为 0）。
    pub line_gap: f32,
}

impl StageGeometry {
    pub fn is_valid(self) -> bool {
        !self.cover.is_zero() && self.info.w > 0.5 && self.title_size > 0.5
    }

    /// 文案块总高度：两行各自 `字号 × 1.25`，中间加 `line_gap`。
    pub fn info_height(self) -> f32 {
        self.title_size * INFO_LINE_RATIO + self.artist_size * INFO_LINE_RATIO + self.line_gap
    }

    pub fn lerp(self, target: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            cover: self.cover.lerp(target.cover, t),
            cover_radius: lerp(self.cover_radius, target.cover_radius, t),
            info: self.info.lerp(target.info, t),
            title_size: lerp(self.title_size, target.title_size, t),
            artist_size: lerp(self.artist_size, target.artist_size, t),
            line_gap: lerp(self.line_gap, target.line_gap, t),
        }
    }

    /// 把整块几何平移到另一个坐标空间（共享舞台画在缩小的卡片里时使用）。
    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self {
            cover: self.cover.translated(dx, dy),
            info: self.info.translated(dx, dy),
            ..self
        }
    }
}

/// anime.js 默认的 `out(5)` 近似——起步快、收尾稳，适合位置/尺寸补间。
pub fn ease_out_quint(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(5)
}

/// 一次布局补间：First / Last 两帧几何 + 起始时刻。
#[derive(Clone, Copy, Debug)]
pub struct LayoutMotion {
    pub from: StageGeometry,
    pub to: StageGeometry,
    /// First / Last 对应的模态；用于判断这次补间接管的是宽屏列还是竖屏页面。
    pub from_mode: StageMode,
    pub to_mode: StageMode,
    pub start: Instant,
    pub duration: Duration,
}

impl LayoutMotion {
    #[allow(dead_code)] // 测试与后续入场/退场复用
    pub fn new(
        from_mode: StageMode,
        from: StageGeometry,
        to_mode: StageMode,
        to: StageGeometry,
        duration_ms: f32,
    ) -> Self {
        Self::starting_at(from_mode, from, to_mode, to, Instant::now(), duration_ms)
    }

    /// 显式指定起始时刻，便于同一帧内先采样 First 再构造补间。
    pub fn starting_at(
        from_mode: StageMode,
        from: StageGeometry,
        to_mode: StageMode,
        to: StageGeometry,
        start: Instant,
        duration_ms: f32,
    ) -> Self {
        Self {
            from,
            to,
            from_mode,
            to_mode,
            start,
            duration: Duration::from_secs_f32((duration_ms / 1000.0).max(0.001)),
        }
    }

    /// 线性进度 0..=1（用于淡入淡出等不需要缓动的通道）。
    pub fn progress(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32())
            .clamp(0.0, 1.0)
    }

    /// 缓动后的进度 0..=1（用于位置/尺寸通道）。
    pub fn eased(&self, now: Instant) -> f32 {
        ease_out_quint(self.progress(now))
    }

    pub fn geometry(&self, now: Instant) -> StageGeometry {
        self.from.lerp(self.to, self.eased(now))
    }

    #[allow(dead_code)] // 测试使用；调用方通常用 settled() 判断是否交还静态布局
    pub fn done(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }

    /// 结束后再保留一小段「终点帧」，让静态页面接管前最后一帧确实画在目标位置。
    pub fn settled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= self.duration + SETTLE_GRACE
    }

    pub fn active(&self, now: Instant) -> bool {
        !self.settled(now)
    }

    /// 目标模态是否是「宽屏」几何；用于判断一次补间是否接管宽屏播放列。
    pub fn touches_wide(&self) -> bool {
        matches!(self.to_mode, StageMode::WideSplit)
            || matches!(self.from_mode, StageMode::WideSplit)
    }

    /// 目标模态是否是「竖屏」几何。
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

// ---------------------------------------------------------------------------
// 几何常量与纯函数：渲染层与动画层共用，避免两套公式漂移。
// ---------------------------------------------------------------------------

/// 曲名 / 艺人行高比例（与 AMLL 播放器列一致）。
pub const INFO_LINE_RATIO: f32 = 1.25;
/// 动画结束后多留 ~3 帧再交还给静态布局，避免最后一帧被跳过。
const SETTLE_GRACE: Duration = Duration::from_millis(48);
/// 信息行右侧的 35px 菜单/返回圆钮。
pub const INFO_ACTION_W: f32 = 35.0;

pub const COMPACT_TOP: f32 = 56.0;
pub const COMPACT_BOTTOM: f32 = 16.0;
pub const COMPACT_DOCK_H: f32 = 40.0;
/// 封面与信息区之间留 28px：AMLL horizontal 的 8px grid gap 加上控件列的
/// `margin-top: calc(-8px + 1.75em)`（1em = 16px）。
pub const COMPACT_COVER_GAP: f32 = 28.0;
pub const COMPACT_MIN_CONTROLS: f32 = 196.0;

pub const QUEUE_TOP_PAD: f32 = 4.0;
pub const QUEUE_HINT_H: f32 = 58.0;
pub const QUEUE_COVER: f32 = 54.0;
pub const QUEUE_COVER_RADIUS: f32 = 7.0;
pub const QUEUE_COVER_GAP: f32 = 12.0;

/// 竖屏内容列宽（封面、信息、进度、传输、音量、底栏共用）。
pub fn compact_content_w(width: f32, height: f32) -> f32 {
    ((width * 0.82).min(height * 0.55).min(560.0)).max(260.0)
}

/// 竖屏封面态的大封面边长。
pub fn compact_cover_size(width: f32, height: f32) -> f32 {
    let content_w = compact_content_w(width, height);
    let available = (height - COMPACT_TOP - COMPACT_BOTTOM - COMPACT_DOCK_H - 8.0).max(0.0);
    content_w.min((available - COMPACT_MIN_CONTROLS - COMPACT_COVER_GAP).max(160.0))
}

/// 宽屏封面边长（AMLL HorizontalLayout）。
pub fn wide_cover_size(width: f32, height: f32) -> f32 {
    if height <= 1000.0 {
        (height * 0.45).min(width * 0.38)
    } else {
        (height * 0.5).min(width * 0.38)
    }
}

/// 宽屏封面顶部 y。AMLL grid：drag(.45fr) / thumb(auto=2×2vh=36) /
/// cover / music-info(3fr) / bottom(0fr) / spacer(.3fr)，gap 8。
/// 剩余 fr 高度 = H - cover - 36 - 5×8。
pub fn wide_cover_top(width: f32, height: f32) -> f32 {
    let cover = wide_cover_size(width, height);
    let fr = (height - cover - 76.0).max(0.0);
    let drag = fr * 0.12;
    drag + 8.0 + 36.0 + 8.0
}

/// 宽屏播放列的 x：定位在左侧 45% 歌词分栏的中心。
pub fn wide_column_x(width: f32, height: f32) -> f32 {
    let cover = wide_cover_size(width, height);
    ((width * 0.45 - cover) * 0.5).max(0.0)
}

/// 底部悬浮 Dock 的条状几何（与 `render_dock` 的绝对定位一致）。
///
/// 沉浸页的展开/收起动画以这条 Dock 为第一帧：页面从它的矩形放大到
/// 整个窗口，共享封面/曲名则从 Dock 内的小封面/文案位置飞到沉浸页位置。
pub fn dock_rect(width: f32, height: f32) -> StageRect {
    let dock_w = ((width - 32.0).min(840.0)).max(520.0);
    let x = ((width - dock_w) * 0.5).max(8.0);
    StageRect {
        x,
        y: height - crate::theme::size::DOCK_BOTTOM - crate::theme::size::DOCK,
        w: dock_w,
        h: crate::theme::size::DOCK,
    }
}

/// Dock 内共享元素（42px 封面 + 曲名/艺人）的几何。
pub fn dock_geometry(width: f32, height: f32) -> StageGeometry {
    let bar = dock_rect(width, height);
    let cover = 42.0;
    let pad = 14.0;
    let title_size = 12.5;
    let artist_size = 10.5;
    let line_gap = 1.0;
    let info_h = title_size * INFO_LINE_RATIO + artist_size * INFO_LINE_RATIO + line_gap;
    // 与 render_dock 的左侧曲目区同宽：compact 180 / 宽屏 232。
    let info_w = if bar.w < 720.0 { 180.0 } else { 232.0 } - cover - 10.0;
    StageGeometry {
        cover: StageRect {
            x: bar.x + pad,
            y: bar.y + (bar.h - cover) * 0.5,
            w: cover,
            h: cover,
        },
        cover_radius: 7.0,
        info: StageRect {
            x: bar.x + pad + cover + 10.0,
            y: bar.y + (bar.h - info_h) * 0.5,
            w: info_w.max(0.0),
            h: info_h,
        },
        title_size,
        artist_size,
        line_gap,
    }
}

/// 计算某个沉浸模态的共享元素几何。
pub fn immersive_geometry(mode: StageMode, width: f32, height: f32) -> StageGeometry {
    match mode {
        StageMode::CompactArt => {
            let cover_size = compact_cover_size(width, height);
            let cover_x = ((width - cover_size) * 0.5).max(0.0);
            let title_size = (height * 0.02).max(16.0);
            let info_top = COMPACT_TOP + cover_size + COMPACT_COVER_GAP;
            StageGeometry {
                cover: StageRect {
                    x: cover_x,
                    y: COMPACT_TOP,
                    w: cover_size,
                    h: cover_size,
                },
                cover_radius: 0.0,
                info: StageRect {
                    x: cover_x,
                    y: info_top,
                    // 右侧 35px 被菜单圆钮占用。
                    w: (cover_size - INFO_ACTION_W).max(0.0),
                    h: 0.0,
                },
                title_size,
                artist_size: title_size,
                line_gap: 0.0,
            }
            .with_info_height()
        }
        StageMode::CompactQueue => {
            let content_w = compact_content_w(width, height);
            let stage_x = ((width - content_w) * 0.5).max(0.0);
            let title_size = 16.0;
            let artist_size = 11.0;
            let line_gap = 2.0;
            let info_h = title_size * INFO_LINE_RATIO + artist_size * INFO_LINE_RATIO + line_gap;
            let header_y = QUEUE_TOP_PAD + QUEUE_HINT_H;
            StageGeometry {
                cover: StageRect {
                    x: stage_x,
                    y: header_y,
                    w: QUEUE_COVER,
                    h: QUEUE_COVER,
                },
                cover_radius: QUEUE_COVER_RADIUS,
                info: StageRect {
                    x: stage_x + QUEUE_COVER + QUEUE_COVER_GAP,
                    y: header_y + (QUEUE_COVER - info_h) * 0.5,
                    w: (content_w - QUEUE_COVER - QUEUE_COVER_GAP).max(0.0),
                    h: info_h,
                },
                title_size,
                artist_size,
                line_gap,
            }
        }
        StageMode::WideSplit => {
            let cover_size = wide_cover_size(width, height);
            let cover_top = wide_cover_top(width, height);
            let column_x = wide_column_x(width, height);
            let title_size = (height * 0.02).max(16.0);
            StageGeometry {
                cover: StageRect {
                    x: column_x,
                    y: cover_top,
                    w: cover_size,
                    h: cover_size,
                },
                cover_radius: 0.0,
                info: StageRect {
                    x: column_x,
                    y: cover_top + cover_size + COMPACT_COVER_GAP,
                    w: (cover_size - INFO_ACTION_W).max(0.0),
                    h: 0.0,
                },
                title_size,
                artist_size: title_size,
                line_gap: 0.0,
            }
            .with_info_height()
        }
    }
}

impl StageGeometry {
    /// 把 info.h 填成由字号推导出的真实高度。
    fn with_info_height(mut self) -> Self {
        self.info.h = self.info_height();
        self
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_endpoints_are_exact() {
        assert_eq!(ease_out_quint(0.0), 0.0);
        assert_eq!(ease_out_quint(1.0), 1.0);
        // 单调且起步快：中点已经走过大半。
        assert!(ease_out_quint(0.5) > 0.9);
    }

    #[test]
    fn geometry_endpoints_match_modes() {
        let (w, h) = (800.0, 1000.0);
        let art = immersive_geometry(StageMode::CompactArt, w, h);
        let queue = immersive_geometry(StageMode::CompactQueue, w, h);
        let split = immersive_geometry(StageMode::WideSplit, 1600.0, 1000.0);

        assert!(art.is_valid() && queue.is_valid() && split.is_valid());

        // 大封面水平居中；队列页头的封面明显更小。
        assert!((art.cover.x + art.cover.w * 0.5 - w * 0.5).abs() < 0.01);
        assert!(queue.cover.w < art.cover.w);

        // 布局 1 的播放列位于左侧歌词分栏内。
        assert!(split.cover.x < 1600.0 * 0.225);
    }

    #[test]
    fn motion_interpolates_and_finishes() {
        let from = immersive_geometry(StageMode::CompactArt, 800.0, 1000.0);
        let to = immersive_geometry(StageMode::CompactQueue, 800.0, 1000.0);
        let motion = LayoutMotion::new(
            StageMode::CompactArt,
            from,
            StageMode::CompactQueue,
            to,
            320.0,
        );

        assert_eq!(motion.progress(motion.start), 0.0);
        assert_eq!(motion.geometry(motion.start).cover.x, from.cover.x);
        assert!(!motion.done(motion.start));

        let end = motion.start + Duration::from_millis(320);
        assert!(motion.done(end));
        assert!((motion.geometry(end).cover.x - to.cover.x).abs() < 0.01);
        assert!((motion.geometry(end).title_size - to.title_size).abs() < 0.01);
    }

    #[test]
    fn retarget_mid_flight_preserves_current_rect() {
        let art = immersive_geometry(StageMode::CompactArt, 800.0, 1000.0);
        let queue = immersive_geometry(StageMode::CompactQueue, 800.0, 1000.0);
        let motion = LayoutMotion::new(
            StageMode::CompactArt,
            art,
            StageMode::CompactQueue,
            queue,
            320.0,
        );
        let mid = motion.start + Duration::from_millis(160);
        let current = motion.geometry(mid);
        let next = LayoutMotion::new(
            StageMode::CompactQueue,
            current,
            StageMode::CompactArt,
            art,
            320.0,
        );
        assert_eq!(next.geometry(next.start), current);
    }

    #[test]
    fn info_height_uses_two_lines_and_gap() {
        let art = immersive_geometry(StageMode::CompactArt, 800.0, 1000.0);
        assert!((art.info.h - art.title_size * INFO_LINE_RATIO * 2.0).abs() < 0.01);

        let queue = immersive_geometry(StageMode::CompactQueue, 800.0, 1000.0);
        assert!(
            (queue.info.h
                - (queue.title_size * INFO_LINE_RATIO
                    + queue.artist_size * INFO_LINE_RATIO
                    + queue.line_gap))
                .abs()
                < 0.01
        );
    }
}
