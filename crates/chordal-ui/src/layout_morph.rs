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
//! 渲染阶段用 [`crate::motion::StageSpring`] 插值一个覆盖层舞台。页面本身在动画期间把
//! 自带的封面 / 曲名 / 艺人设为透明（只保留占位），结束后再由页面按同一份
//! 几何函数绘制，保证「动画终点」与「静态布局」严格一致、不会跳动。
//!
//! 这对应 anime.js Layout 的 `layout.record()` + `layout.animate()`：
//! 记录前态 → 改布局 → 在两者之间补间。区别只是这里共享的是封面与曲目文案，
//! 而不是整个容器的 flex/grid 属性。

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
    /// 标题字重（GPUI `FontWeight` 的数值）。Dock 是 600，沉浸页是 500；
    /// 连续插值后单层文字即可完成字重过渡，不需要两层叠加交叉淡化。
    pub title_weight: f32,
    /// 艺人行字重。两端都是 400，留出通道是为了让 FLIP 的所有样式都同源。
    pub artist_weight: f32,
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
            title_weight: lerp(self.title_weight, target.title_weight, t),
            artist_weight: lerp(self.artist_weight, target.artist_weight, t),
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

// ---------------------------------------------------------------------------
// 几何常量与纯函数：渲染层与动画层共用，避免两套公式漂移。
// ---------------------------------------------------------------------------

/// 曲名 / 艺人行高比例（与 AMLL 播放器列一致）。
pub const INFO_LINE_RATIO: f32 = 1.25;
/// 圆钮与文案之间的固定间距（`amll_info_row` 里的 `ml(16)`）。
pub const INFO_ACTION_GAP: f32 = 16.0;

/// 沉浸页曲名行右侧圆钮的边长。
///
/// 静态页面（`amll_info_row`）与共享舞台必须使用同一个公式，否则动画收口、
/// 交还静态布局的那一帧文案可用宽度会突变，看起来像「文字往左跳了一下」。
pub fn info_action_side(viewport_width: f32, height: f32) -> f32 {
    if viewport_width <= 1000.0 {
        ((viewport_width * 0.05).min(height * 0.04)).max(32.0)
    } else {
        height * 0.035
    }
}

/// 曲名/艺人文案在给定列宽里的可用宽度（扣掉右侧圆钮与间距）。
pub fn info_text_width(column_w: f32, viewport_width: f32, height: f32) -> f32 {
    (column_w - info_action_side(viewport_width, height) - INFO_ACTION_GAP).max(0.0)
}

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
pub const QUEUE_COVER_RADIUS: f32 = 1.0;
pub const QUEUE_COVER_GAP: f32 = 12.0;

/// 竖屏内容列宽（封面、信息、进度、传输、音量、底栏共用）。
pub fn compact_content_w(width: f32, height: f32) -> f32 {
    (width * 0.82).min(height * 0.55).clamp(260.0, 560.0)
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

/// 歌词列右侧留白（按 100vh 视口算，下拉手势时不随之改变）。
pub fn stable_lyric_pad(width: f32, height: f32) -> f32 {
    let lyric_w = (width - width * 0.45 - 8.0).max(1.0);
    if height <= 1000.0 {
        lyric_w * 0.08
    } else {
        lyric_w * 0.15
    }
}

/// 沉浸页底部按钮行的矩形。
///
/// 宽屏时左右边界分别对齐播放列左缘与歌词列右缘（绿色主沉浸区的边界），
/// 并整体落在内容区内；紧凑布局沿用居中内容列。下拉/展开动画中卡片会
/// 缩放，这里只提供静态目标位置，避免按钮随着手势抖动。
pub fn stage_bottom_rect(width: f32, height: f32, wide: bool, lyric_pad: f32) -> StageRect {
    if wide {
        let left = wide_column_x(width, height);
        let right = (width - lyric_pad).max(left + 160.0);
        StageRect {
            x: left,
            y: height - 32.0 - COMPACT_DOCK_H,
            w: (right - left).max(160.0),
            h: COMPACT_DOCK_H,
        }
    } else {
        let content_w = compact_content_w(width, height);
        StageRect {
            x: ((width - content_w) * 0.5).max(0.0),
            y: height - COMPACT_BOTTOM - COMPACT_DOCK_H,
            w: content_w,
            h: COMPACT_DOCK_H,
        }
    }
}

/// 底部悬浮 Dock 的宽度。渲染层和动画层必须调用同一个函数，避免任一端
/// 改了尺寸后展开/收起动画停在错位的位置。
pub fn dock_width(viewport_width: f32) -> f32 {
    (viewport_width - 32.0).clamp(520.0, 880.0)
}

/// 底部悬浮 Dock 的条状几何（与 `render_dock` 的绝对定位一致）。
///
/// 沉浸页的展开/收起动画以这条 Dock 为第一帧：页面从它的矩形放大到
/// 整个窗口，共享封面/曲名则从 Dock 内的小封面/文案位置飞到沉浸页位置。
pub fn dock_rect(width: f32, height: f32) -> StageRect {
    let dock_w = dock_width(width);
    let x = ((width - dock_w) * 0.5).max(8.0);
    StageRect {
        x,
        y: height - crate::theme::size::DOCK_BOTTOM - crate::theme::size::DOCK,
        w: dock_w,
        h: crate::theme::size::DOCK,
    }
}

// Dock 内容几何。render_dock 与本模块共用这些常量，任何一端单独改尺寸都会
// 让展开/收起动画的终点与真实 Dock 错位。
pub const DOCK_PAD_X: f32 = 18.0;
pub const DOCK_PAD_TOP: f32 = 9.0;
pub const DOCK_PAD_BOTTOM: f32 = 7.0;
pub const DOCK_GAP: f32 = 2.0;
pub const DOCK_TIMELINE_H: f32 = 18.0;
pub const DOCK_TRACK_PAD_X: f32 = 3.0;
pub const DOCK_COVER: f32 = 40.0;
pub const DOCK_COVER_RADIUS: f32 = 1.0;
pub const DOCK_COVER_GAP: f32 = 10.0;
pub const DOCK_TRANSPORT_BTN: f32 = 30.0;
pub const DOCK_PLAY_BTN: f32 = 42.0;
pub const DOCK_TRANSPORT_GAP: f32 = 4.0;
pub const DOCK_TRANSPORT_PAD_X: f32 = 5.0;
/// 上一首 30 + 播放 42 + 下一首 30 + 2×4 gap + 2×5 水平内边距。
pub const DOCK_TRANSPORT_W: f32 = DOCK_TRANSPORT_BTN * 2.0
    + DOCK_PLAY_BTN
    + DOCK_TRANSPORT_GAP * 2.0
    + DOCK_TRANSPORT_PAD_X * 2.0;
pub const DOCK_UPPER_GAP: f32 = 10.0;
pub const DOCK_TITLE_SIZE: f32 = 12.5;
pub const DOCK_ARTIST_SIZE: f32 = 10.5;
pub const DOCK_LINE_GAP: f32 = 1.0;

/// Dock 上层（曲目 / 传输 / 工具）的高度：整条高度减去上下内边距、间距与时间轴。
pub fn dock_upper_h() -> f32 {
    crate::theme::size::DOCK - DOCK_PAD_TOP - DOCK_PAD_BOTTOM - DOCK_GAP - DOCK_TIMELINE_H
}

/// Dock 内共享元素（40px 封面 + 曲名/艺人）的几何。
pub fn dock_geometry(width: f32, height: f32) -> StageGeometry {
    let bar = dock_rect(width, height);
    let upper_h = dock_upper_h();
    let info_h =
        DOCK_TITLE_SIZE * INFO_LINE_RATIO + DOCK_ARTIST_SIZE * INFO_LINE_RATIO + DOCK_LINE_GAP;
    let track_w = (bar.w - DOCK_PAD_X * 2.0 - DOCK_TRANSPORT_W - DOCK_UPPER_GAP * 2.0) * 0.5;
    let info_w = track_w - DOCK_TRACK_PAD_X * 2.0 - DOCK_COVER - DOCK_COVER_GAP;
    StageGeometry {
        cover: StageRect {
            x: bar.x + DOCK_PAD_X + DOCK_TRACK_PAD_X,
            y: bar.y + DOCK_PAD_TOP + (upper_h - DOCK_COVER) * 0.5,
            w: DOCK_COVER,
            h: DOCK_COVER,
        },
        cover_radius: DOCK_COVER_RADIUS,
        info: StageRect {
            x: bar.x + DOCK_PAD_X + DOCK_TRACK_PAD_X + DOCK_COVER + DOCK_COVER_GAP,
            y: bar.y + DOCK_PAD_TOP + (upper_h - info_h) * 0.5,
            w: info_w.max(0.0),
            h: info_h,
        },
        title_size: DOCK_TITLE_SIZE,
        artist_size: DOCK_ARTIST_SIZE,
        line_gap: DOCK_LINE_GAP,
        title_weight: 600.0,
        artist_weight: 400.0,
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
                cover_radius: 1.0,
                info: StageRect {
                    x: cover_x,
                    y: info_top,
                    // 与静态页面共用同一公式：圆钮边长 + 16px 间距。
                    w: info_text_width(cover_size, width, height),
                    h: 0.0,
                },
                title_size,
                artist_size: title_size,
                line_gap: 0.0,
                title_weight: 500.0,
                artist_weight: 400.0,
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
                title_weight: 500.0,
                artist_weight: 400.0,
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
                cover_radius: 1.0,
                info: StageRect {
                    x: column_x,
                    y: cover_top + cover_size + COMPACT_COVER_GAP,
                    w: info_text_width(cover_size, width, height),
                    h: 0.0,
                },
                title_size,
                artist_size: title_size,
                line_gap: 0.0,
                title_weight: 500.0,
                artist_weight: 400.0,
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
    use crate::motion::{MotionTuning, StageSpring, ramp};
    use std::time::{Duration, Instant};

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
    fn spring_starts_at_first_and_converges_to_last() {
        let from = immersive_geometry(StageMode::CompactArt, 800.0, 1000.0);
        let to = immersive_geometry(StageMode::CompactQueue, 800.0, 1000.0);
        let start = Instant::now();
        let motion = StageSpring::new(
            from,
            to,
            StageMode::CompactArt,
            StageMode::CompactQueue,
            start,
            MotionTuning::default().spec(320.0),
        );

        assert_eq!(motion.sample(start), from);
        assert!(!motion.done(start));

        let end = start + Duration::from_secs(2);
        let settled = motion.sample(end);
        assert!((settled.cover.x - to.cover.x).abs() < 0.01);
        assert!((settled.title_size - to.title_size).abs() < 0.01);
        assert!(motion.done(end));
    }

    #[test]
    fn retarget_mid_flight_preserves_rect_and_velocity() {
        let art = immersive_geometry(StageMode::CompactArt, 800.0, 1000.0);
        let queue = immersive_geometry(StageMode::CompactQueue, 800.0, 1000.0);
        let start = Instant::now();
        let mut motion = StageSpring::new(
            art,
            queue,
            StageMode::CompactArt,
            StageMode::CompactQueue,
            start,
            MotionTuning::default().spec(320.0),
        );
        let mid = start + Duration::from_millis(160);
        let rect = motion.sample(mid);
        let velocity = motion.velocity(mid);

        motion.retarget(art, StageMode::CompactArt, mid);
        // 位置与速度都必须连续；解析解重算会有浮点尾差，用像素/秒级容差。
        assert!((motion.sample(mid).cover.x - rect.cover.x).abs() < 0.01);
        assert!((motion.sample(mid).cover.w - rect.cover.w).abs() < 0.01);
        assert!((motion.sample(mid).info.y - rect.info.y).abs() < 0.01);
        assert!((motion.velocity(mid).cover.w - velocity.cover.w).abs() < 1.0);
    }

    #[test]
    fn dock_rect_uses_same_width_as_rendered_dock() {
        // 回归：dock_width 的上限从 840 调到 880 时动画仍用旧值，
        // 展开/收起的终点会比真实 Dock 窄 40px、左移 20px。
        for w in [520.0, 800.0, 1000.0, 1440.0, 1920.0, 2560.0] {
            assert_eq!(dock_rect(w, 900.0).w, dock_width(w), "width={w}");
            let expected_x = ((w - dock_width(w)) * 0.5).max(8.0);
            assert_eq!(dock_rect(w, 900.0).x, expected_x, "width={w}");
        }
        assert_eq!(dock_width(1440.0), 880.0);
    }

    #[test]
    fn dock_geometry_matches_rendered_dock_layout() {
        for (w, h) in [(960.0, 900.0), (1600.0, 1000.0)] {
            let bar = dock_rect(w, h);
            let g = dock_geometry(w, h);
            let upper_h = dock_upper_h();

            assert_eq!(
                bar.y,
                h - crate::theme::size::DOCK_BOTTOM - crate::theme::size::DOCK
            );
            assert_eq!(upper_h, 52.0);
            assert_eq!(g.cover.x, bar.x + DOCK_PAD_X + DOCK_TRACK_PAD_X);
            assert_eq!(
                g.cover.y,
                bar.y + DOCK_PAD_TOP + (upper_h - DOCK_COVER) * 0.5
            );
            assert_eq!(g.cover.w, DOCK_COVER);
            assert_eq!(g.info.x, g.cover.x + DOCK_COVER + DOCK_COVER_GAP);
            assert_eq!(g.info.y, bar.y + DOCK_PAD_TOP + (upper_h - g.info.h) * 0.5);
            assert!(g.info.w > 0.0, "info width must stay positive: {g:?}");
        }
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

    #[test]
    fn dock_to_immersive_interpolates_font_weight() {
        for (w, h) in [(960.0, 900.0), (1600.0, 1000.0)] {
            let dock = dock_geometry(w, h);
            let page = immersive_geometry(StageMode::CompactArt, w, h);
            assert_eq!(dock.title_weight, 600.0);
            assert_eq!(page.title_weight, 500.0);

            let half = dock.lerp(page, 0.5);
            assert!((half.title_weight - 550.0).abs() < 0.01);
            assert!((half.artist_weight - 400.0).abs() < 0.01);

            // 逐帧插值必须单调，否则字重会在过渡中来回抖动。
            let mut last = dock.title_weight;
            for step in 1..=10 {
                let t = step as f32 / 10.0;
                let weight = dock.lerp(page, t).title_weight;
                assert!(weight <= last + 0.001, "non-monotonic at t={t}");
                last = weight;
            }
        }
    }

    #[test]
    fn ramp_has_zero_slope_at_both_ends() {
        // 淡入淡出两端斜率必须趋近 0，否则控件在起止帧会「啪」地出现。
        assert_eq!(ramp(0.0, 1.0, 0.5), 1.0);
        assert_eq!(ramp(1.0, 1.0, 0.5), 0.0);
        assert!(ramp(0.01, 1.0, 0.5) - 1.0 > -0.01);
        assert!(ramp(0.99, 1.0, 0.5) < 0.01);

        // 反向区间：从 0 到 0.38 逐渐归零，用于背后真实 Dock 的淡出。
        assert_eq!(ramp(0.0, 0.0, 0.38), 0.0);
        assert_eq!(ramp(0.38, 0.0, 0.38), 1.0);
    }

    #[test]
    fn info_width_matches_static_page_formula() {
        for (w, h) in [(800.0, 1000.0), (1600.0, 1000.0), (1440.0, 900.0)] {
            let cover = compact_cover_size(w, h);
            let expected = (cover - info_action_side(w, h) - INFO_ACTION_GAP).max(0.0);
            assert!((info_text_width(cover, w, h) - expected).abs() < 0.001);

            let art = immersive_geometry(StageMode::CompactArt, w, h);
            assert!(
                (art.info.w - expected).abs() < 0.001,
                "shared info width must match static layout: {w}x{h}"
            );
        }
    }

    #[test]
    fn bottom_row_aligns_with_immersive_content_edges() {
        let (w, h) = (1296.0, 816.0);
        let pad = stable_lyric_pad(w, h);
        let row = stage_bottom_rect(w, h, true, pad);
        // 左缘 = 播放列左缘，右缘 = 歌词列右缘，整行落在内容区内部。
        assert!((row.x - wide_column_x(w, h)).abs() < 0.001);
        assert!((row.x + row.w - (w - pad)).abs() < 0.001);
        assert!(row.y > 0.0 && row.y + row.h < h);

        let compact = stage_bottom_rect(w, h, false, pad);
        let content = compact_content_w(w, h);
        assert!((compact.w - content).abs() < 0.001);
        assert!((compact.x - (w - content) * 0.5).abs() < 0.001);
    }
}
