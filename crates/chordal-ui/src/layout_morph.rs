//! 共享元素布局动画（anime.js `createLayout` / FLIP 的 GPUI 等价实现）。
//!
//! GPUI 的元素树没有可查询的 DOM，但 `prepaint` 阶段能取得 Taffy 计算后的真实
//! `Bounds`。共享元素因此在状态切换时测量静态组件的封面、文案块与 Dock 卡片，
//! 缓存按视口、封面缩放和闲置位移失效；动画每帧只采样弹簧，不重新遍历布局。
//!
//! 切换时先冻结当前可见几何，隐藏目标页面完成一次真实布局测量，再用同一份
//! 覆盖层舞台在 First / Last 之间过渡。`immersive_geometry` 等纯函数只用于首次
//! 测量前的兜底与几何单元测试，不再作为运行时动画终点的真值源。

/// 沉浸层里共享元素几何不同的布局模态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageMode {
    /// 桌面主面板底部 Dock。
    Dock,
    /// 手机曲库页底部 mini player。
    MiniPlayer,
    /// 手机全屏播放页。
    PhonePlayer,
    /// 竖屏：封面态（大封面 + 标题在下方）。
    CompactArt,
    /// 竖屏：全屏歌词（小封面 + 标题在页头）。
    CompactLyrics,
    /// 竖屏：播放队列（54px 小封面 + 标题在页头）。
    CompactQueue,
    /// 宽屏：歌词打开，播放列位于左侧 45% 区域。这是布局 1。
    WideSplit,
}

impl StageMode {
    pub fn is_source(self) -> bool {
        matches!(self, Self::Dock | Self::MiniPlayer | Self::PhonePlayer)
    }
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
/// `info` 是曲名/艺人文案块的实测左上角与可用宽度；字号、行重和间距来自
/// 静态组件样式，动画期间文字随封面连续过渡，不在两层文本之间硬切。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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

    /// 公式兜底的文案块总高度；运行时优先使用静态布局实测高度。
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
}

// ---------------------------------------------------------------------------
// 几何常量与纯函数：提供首次测量前的兜底，运行时以实测 Bounds 为准。
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

/// 进度条行高 / 音量行高。渲染层、竖屏歌词几何与宽屏封面反推共用同一份值，
/// 避免任一布局单独改行高后其它布局的间距算错。
pub const STAGE_PROGRESS_H: f32 = 45.2;
pub const STAGE_VOLUME_H: f32 = 40.0;
/// 信息 / 进度 / 传输 / 音量 / 底栏之间至少保留的呼吸间距。
pub const STAGE_MIN_GAP: f32 = 10.0;
/// 宽屏底栏距离窗口底部的空间（32）与按钮行高度（40）。
pub const STAGE_BOTTOM_RESERVE: f32 = 72.0;
/// 窗口高度低于此值时收起音量行。矮窗里音量不是播放必需操作，
/// 把 40px + 一道间距让给封面/歌词，比把封面压小或把控件贴成一团更好。
pub const STAGE_VOLUME_MIN_H: f32 = 700.0;
/// 宽屏封面的下限；再矮的窗口交给 0 间距兜底，避免封面缩到不可见。
const WIDE_COVER_MIN: f32 = 120.0;

pub fn stage_volume_visible(height: f32) -> bool {
    height >= STAGE_VOLUME_MIN_H
}

/// 沉浸层传输按钮的边长：5 个按钮共用列宽的 18%。
pub fn stage_button_size(column_w: f32) -> f32 {
    (column_w * 0.18).clamp(if column_w < 240.0 { 34.0 } else { 40.0 }, 88.0)
}

/// 曲名/艺人两行所需的垂直空间（也是控件区的 leading）。
pub fn amll_info_height(viewport_width: f32, height: f32) -> f32 {
    let font = (height * 0.02).max(16.0);
    (font * INFO_LINE_RATIO * 2.0).max(info_action_side(viewport_width, height))
}

/// 沉浸歌词字号（AMLL Medium 档）。歌词视口的左右 padding 等于字号 ×
/// `IMMERSIVE_LYRIC_PADDING_EM`，竖屏顶栏/控件必须按同一数值内缩，
/// 才能与歌词正文共用一条对齐线。
pub fn immersive_lyric_font_size(width: f32, height: f32) -> f32 {
    if width <= 768.0 {
        (width * 0.08).max(12.0)
    } else {
        (height * 0.05).max(width * 0.025).max(14.0)
    }
}

/// 沉浸歌词正文的左右 padding（em）。样式与竖屏几何唯一真值源。
pub const IMMERSIVE_LYRIC_PADDING_EM: f32 = 1.0;

pub fn immersive_lyric_padding(width: f32, height: f32) -> f32 {
    immersive_lyric_font_size(width, height) * IMMERSIVE_LYRIC_PADDING_EM
}

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
    // 音量行被收起时，控件区的最小高度同步减少，封面可以把这部分空间拿回去。
    let min_controls = if stage_volume_visible(height) {
        COMPACT_MIN_CONTROLS
    } else {
        COMPACT_MIN_CONTROLS - STAGE_VOLUME_H
    };
    content_w.min((available - min_controls - COMPACT_COVER_GAP).max(160.0))
}

/// 宽屏封面边长（AMLL HorizontalLayout）。
///
/// 45vh / 50vh 只描述「高窗口」的理想比例。窗口较矮时封面会按比例吃掉
/// 控件区，进度 / 传输 / 音量 / 底栏只能以 0 间距互相贴住（1310×850 以下的
/// 实测就是标题压进度、音量贴底栏）。这里用控件区的最小高度反推封面上限：
///
/// ```text
/// available = bottom_row_top − (cover_top(cover) + cover + 28)
/// required  = info + progress + button(cover) + volume + 4 × min_gap
/// ```
///
/// 封面每缩小 1px，available 增加 0.88px；按钮在中段再增加 0.18px。
/// 迭代收敛后取与理想值的较小者，高窗口行为完全不变。
pub fn wide_cover_size(width: f32, height: f32) -> f32 {
    let preferred = if height <= 1000.0 {
        (height * 0.45).min(width * 0.38)
    } else {
        (height * 0.5).min(width * 0.38)
    };
    if !preferred.is_finite() || !width.is_finite() || !height.is_finite() {
        return 0.0;
    }
    if preferred <= 0.0 {
        return 0.0;
    }
    let info_h = amll_info_height(width, height);
    let bottom_top = height - STAGE_BOTTOM_RESERVE;
    // 矮窗隐藏音量行后，少一行高度和一道间距，封面可以保持更大的尺寸。
    let volume_h = if stage_volume_visible(height) {
        STAGE_VOLUME_H
    } else {
        0.0
    };
    let gap_count = if volume_h > 0.0 { 4.0 } else { 3.0 };
    let required_rows = info_h + STAGE_PROGRESS_H + volume_h + gap_count * STAGE_MIN_GAP;
    let mut cover = preferred;
    for _ in 0..12 {
        let required = required_rows + stage_button_size(cover);
        let available =
            bottom_top - (wide_cover_top_for(cover, height) + cover + COMPACT_COVER_GAP);
        if available + 0.01 >= required {
            break;
        }
        cover = (cover - (required - available) / 1.06).max(WIDE_COVER_MIN);
    }
    cover.clamp(0.0, preferred)
}

fn wide_cover_top_for(cover: f32, height: f32) -> f32 {
    let fr = (height - cover - 76.0).max(0.0);
    fr * 0.12 + 8.0 + 36.0 + 8.0
}

/// 宽屏封面顶部 y。AMLL grid：drag(.45fr) / thumb(auto=2×2vh=36) /
/// cover / music-info(3fr) / bottom(0fr) / spacer(.3fr)，gap 8。
/// 剩余 fr 高度 = H - cover - 36 - 5×8。
pub fn wide_cover_top(width: f32, height: f32) -> f32 {
    wide_cover_top_for(wide_cover_size(width, height), height)
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

/// 闲置收起后，保留可见组（顶部把手 → 进度条）离开窗口边缘的最小距离。
pub const IDLE_CHROME_MARGIN: f32 = 8.0;

/// 闲置收起后把可见组整体平移到窗口垂直中心所需的位移。
///
/// 组内保持完整布局里的原始间距（把手→封面、封面→信息、信息→进度都是
/// 设计值），这里只返回一个刚体平移量，不做 `space-between` 式的重新分配。
/// 组比窗口还高时退回贴顶，至少不把顶部把手推出窗口。
pub fn idle_chrome_shift(height: f32, group_top: f32, group_bottom: f32) -> f32 {
    if !height.is_finite() || !group_top.is_finite() || !group_bottom.is_finite() {
        return 0.0;
    }
    let (group_top, group_bottom) = if group_top <= group_bottom {
        (group_top, group_bottom)
    } else {
        (group_bottom, group_top)
    };
    let center = (group_top + group_bottom) * 0.5;
    let shift = height * 0.5 - center;
    // 允许的位移区间：组顶不低于上边距，组底不越过下边距。
    let min_shift = IDLE_CHROME_MARGIN - group_top;
    let max_shift = (height - IDLE_CHROME_MARGIN) - group_bottom;
    if min_shift > max_shift {
        // 组本身比可用高度还高：优先保证顶部把手可见。
        min_shift
    } else {
        shift.clamp(min_shift, max_shift)
    }
}

pub const PHONE_NAV_H: f32 = 54.0;
pub const PHONE_MINI_H: f32 = 62.0;

pub fn mini_player_rect(width: f32, height: f32) -> StageRect {
    StageRect {
        x: 12.0,
        y: height - PHONE_NAV_H - 8.0 - PHONE_MINI_H,
        w: (width - 24.0).max(1.0),
        h: PHONE_MINI_H,
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

/// 计算某个模态的公式兜底几何；真实转场目标由 prepaint 测量提供。
pub fn immersive_geometry(mode: StageMode, width: f32, height: f32) -> StageGeometry {
    match mode {
        StageMode::Dock => dock_geometry(width, height),
        StageMode::MiniPlayer => StageGeometry {
            cover: StageRect {
                x: 20.0,
                y: mini_player_rect(width, height).y + 10.0,
                w: 42.0,
                h: 42.0,
            },
            cover_radius: 1.0,
            info: StageRect {
                x: 72.0,
                y: mini_player_rect(width, height).y + 15.0,
                w: (width - 24.0 - 42.0 - 10.0 - 132.0 - 16.0).max(1.0),
                h: 31.75,
            },
            title_size: 12.5,
            artist_size: 10.0,
            line_gap: 2.0,
            title_weight: 500.0,
            artist_weight: 400.0,
        },
        StageMode::PhonePlayer | StageMode::CompactArt | StageMode::CompactLyrics => {
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
            MotionTuning::default().spec(Duration::from_millis(320)),
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
            MotionTuning::default().spec(Duration::from_millis(320)),
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
    fn mini_player_fallback_matches_phone_shell_rect() {
        let rect = mini_player_rect(390.0, 844.0);
        assert_eq!(rect.x, 12.0);
        assert_eq!(rect.w, 366.0);
        assert_eq!(rect.y, 720.0);
        assert_eq!(rect.h, 62.0);
        assert!(immersive_geometry(StageMode::MiniPlayer, 390.0, 844.0).is_valid());
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
    fn short_windows_keep_control_gaps_and_drop_the_volume_row() {
        // 1310×850 以下的实测窗口：45vh 封面会吃掉控件区，进度/传输/音量
        // 只能以 0 间距互贴。反推出的封面上限必须给间距留够空间；
        // 高度不足时优先收起音量行，少一行高度和一道间距。
        for (w, h) in [
            (903.0, 586.0),
            (900.0, 600.0),
            (1296.0, 700.0),
            (900.0, 720.0),
        ] {
            let cover = wide_cover_size(w, h);
            let info = amll_info_height(w, h);
            let volume_h = if stage_volume_visible(h) {
                STAGE_VOLUME_H
            } else {
                0.0
            };
            let gap_count = if volume_h > 0.0 { 4.0 } else { 3.0 };
            let controls_top = wide_cover_top(w, h) + cover + COMPACT_COVER_GAP;
            let available = (h - STAGE_BOTTOM_RESERVE) - controls_top;
            let required = info
                + STAGE_PROGRESS_H
                + stage_button_size(cover)
                + volume_h
                + gap_count * STAGE_MIN_GAP;
            assert!(
                available + 0.1 >= required,
                "{w}x{h}: cover={cover} available={available} required={required}"
            );
        }

        // 高窗口保持 AMLL 的 45vh，不因短窗兜底而缩小。
        assert!((wide_cover_size(1600.0, 1000.0) - 450.0).abs() < 0.001);
        // 586 高属于「太矮」：收起音量行后封面仍应比 45vh 略小，
        // 但不应再被压到原来的 196px。
        assert!(wide_cover_size(903.0, 586.0) < 586.0 * 0.45 - 1.0);
        assert!(wide_cover_size(903.0, 586.0) > 240.0);
        assert!(stage_volume_visible(900.0));
        assert!(!stage_volume_visible(STAGE_VOLUME_MIN_H - 1.0));
    }

    #[test]
    fn idle_shift_centers_group_without_redistributing_gaps() {
        // 1440×900：把手 84 / 进度条底 700，组中点偏上，整体下移。
        let (top, bottom) = (84.0, 700.0);
        let shift = idle_chrome_shift(900.0, top, bottom);
        assert!(shift > 0.0);
        let shifted_center = ((top + shift) + (bottom + shift)) * 0.5;
        assert!((shifted_center - 450.0).abs() < 0.001);
        // 刚体平移：组内每一段的间距都不变。
        let gap = 120.0;
        assert!(((top + shift + gap) - (top + shift)) - gap < 1e-6);

        // 组本来就在中心时不做位移。
        assert_eq!(idle_chrome_shift(800.0, 300.0, 500.0), 0.0);
    }

    #[test]
    fn idle_shift_keeps_group_inside_short_windows() {
        // 组高于可用高度：贴顶而不是把把手推出窗口。
        let shift = idle_chrome_shift(300.0, 10.0, 400.0);
        assert!((10.0 + shift - IDLE_CHROME_MARGIN).abs() < 0.001);

        // 正常窗口：顶部与底部都留在安全边距内。
        for (h, top, bottom) in [
            (1000.0, 20.0, 750.0),
            (586.0, 18.0, 410.0),
            (900.0, 300.0, 620.0),
        ] {
            let shift = idle_chrome_shift(h, top, bottom);
            assert!(top + shift >= IDLE_CHROME_MARGIN - 0.001, "{h}");
            assert!(bottom + shift <= h - IDLE_CHROME_MARGIN + 0.001, "{h}");
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
