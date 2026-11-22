//! 窗口断点与手机视图的最小尺寸约束。
//!
//! 主面板、沉浸层与设置中心各自有布局，但「什么时候算窄」必须只有一处
//! 定义：否则侧栏宽度、Dock 收缩、设置卡片与手机壳会在不同像素上互相打架。
//!
//! - `Phone`：单列手机壳，只保留播放页 / 曲库页，Dock 与侧栏不参与布局；
//! - `Medium`：侧栏 + 曲目板，右栏「正在播放」隐藏；
//! - `Wide`：完整三栏 + 悬浮 Dock。
//!
//! 沉浸层的横向/竖向选择与这里不同：它沿用 AMLL 的「宽 ≥ 高」判据，
//! 但额外要求宽度足够（`MIN_WIDE_SPLIT_W`），避免 700px 宽时硬拆两列。

/// 小于该宽度进入手机壳（单列）。
pub const PHONE_MAX_W: f32 = 720.0;
/// 大于等于该宽度显示右侧「正在播放」栏。
pub const WIDE_MIN_W: f32 = 1020.0;
/// 沉浸层允许「左播放列 + 右歌词列」的最小宽度。
pub const MIN_WIDE_SPLIT_W: f32 = 820.0;
/// 窗口允许的最小尺寸：接近 9:16 手机竖屏，同时保证 260px 内容列可用。
pub const MIN_WINDOW_W: f32 = 360.0;
pub const MIN_WINDOW_H: f32 = 520.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportClass {
    Phone,
    Medium,
    Wide,
}

impl ViewportClass {
    pub const fn of(width: f32) -> Self {
        if width < PHONE_MAX_W {
            Self::Phone
        } else if width < WIDE_MIN_W {
            Self::Medium
        } else {
            Self::Wide
        }
    }

    pub const fn is_phone(self) -> bool {
        matches!(self, Self::Phone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_are_inclusive_at_the_wide_edge() {
        assert_eq!(ViewportClass::of(359.0), ViewportClass::Phone);
        assert_eq!(ViewportClass::of(PHONE_MAX_W - 0.1), ViewportClass::Phone);
        assert_eq!(ViewportClass::of(PHONE_MAX_W), ViewportClass::Medium);
        assert_eq!(ViewportClass::of(WIDE_MIN_W - 0.1), ViewportClass::Medium);
        assert_eq!(ViewportClass::of(WIDE_MIN_W), ViewportClass::Wide);
    }

    #[test]
    fn minimum_window_is_a_valid_phone() {
        assert!(ViewportClass::of(MIN_WINDOW_W).is_phone());
        assert!(MIN_WINDOW_W < PHONE_MAX_W);
        assert!(MIN_WINDOW_H <= 640.0);
    }
}
