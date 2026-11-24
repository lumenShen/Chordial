#![allow(dead_code)]

//! Chordial 设计令牌（Studio Neutral）。
//!
//! 参照 EffiBuddy_GPUI 的克制语言：中性炭黑/纸面、hairline 分隔、
//! 唯一墨色强调色；彩色只承担状态语义。
//!
//! 注意 gpui 0.2.2 的 `rgba(u32)` 是 **0xRRGGBBAA**，不是 CSS 的 AARRGGBB。
//! 之前 `hex()` 用 `0xff << 24 | rgb` 会把颜色解释成暗红色半透明，已修正。

use gpui::{App, Global, Hsla, SharedString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub mode: Appearance,
    pub bg: Hsla,
    pub sidebar: Hsla,
    pub surface: Hsla,
    pub surface_alt: Hsla,
    pub elevated: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_dim: Hsla,
    pub text_faint: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_soft: Hsla,
    pub on_accent: Hsla,
    pub playing: Hsla,
    pub selection: Hsla,
    pub ok: Hsla,
    pub warn: Hsla,
    pub danger: Hsla,
    pub overlay: Hsla,
    pub scrollbar: Hsla,
    pub scrollbar_hover: Hsla,
    pub focus_ring: Hsla,
    pub disabled_bg: Hsla,
    pub disabled_fg: Hsla,
    pub surface_hover: Hsla,
    pub accent_dim: Hsla,
    /// 底栏玻璃填充（半透明；没有 backdrop blur 时也保留材质层次）。
    pub glass: Hsla,
    /// 玻璃上沿高光 / 描边。
    pub glass_border: Hsla,
    /// 面板与封面的轻阴影。
    pub shadow: Hsla,
    /// 浮层（底栏、播放器）的深阴影。
    pub shadow_strong: Hsla,
    pub radius: f32,
    pub radius_sm: f32,
    pub radius_lg: f32,
    pub font_family: SharedString,
    pub font_mono: SharedString,
    pub font_size: f32,
}

impl Global for Theme {}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            mode: Appearance::Dark,
            bg: hex(0x0E0E11),
            sidebar: hex(0x161619),
            surface: hex(0x1A1A1E),
            surface_alt: hex(0x212127),
            elevated: hex(0x28282E),
            hover: alpha(0xFFFFFF0F),
            active: hex(0x2C2C32),
            border: alpha(0xFFFFFF14),
            border_strong: alpha(0xFFFFFF2E),
            text: hex(0xEDEDF2),
            text_dim: hex(0xA2A2AD),
            text_faint: hex(0x767680),
            accent: hex(0xEDEDF2),
            accent_hover: hex(0xFFFFFF),
            accent_soft: alpha(0xFFFFFF14),
            on_accent: hex(0x141418),
            playing: hex(0x0A84FF),
            selection: hex(0x26262C),
            ok: hex(0x30D158),
            warn: hex(0xD2AC72),
            danger: hex(0xFF6961),
            overlay: alpha(0x000000C2),
            scrollbar: alpha(0xFFFFFF29),
            scrollbar_hover: alpha(0xFFFFFF4D),
            focus_ring: alpha(0x0A84FF66),
            disabled_bg: hex(0x24242A),
            disabled_fg: hex(0x6B6B74),
            surface_hover: alpha(0xFFFFFF1A),
            accent_dim: hex(0x143A63),
            glass: alpha(0x1D1D22F2),
            glass_border: alpha(0xFFFFFF24),
            shadow: alpha(0x00000066),
            shadow_strong: alpha(0x000000A6),
            radius: 8.0,
            radius_sm: 6.0,
            radius_lg: 12.0,
            font_family: SharedString::from("Noto Sans CJK SC"),
            font_mono: SharedString::from("Noto Sans Mono"),
            font_size: 13.0,
        }
    }

    pub fn light() -> Self {
        Self {
            mode: Appearance::Light,
            bg: hex(0xE7E7EA),
            sidebar: hex(0xEFEFF2),
            surface: hex(0xFFFFFF),
            surface_alt: hex(0xF4F4F6),
            elevated: hex(0xFFFFFF),
            hover: alpha(0x0A0A100A),
            active: hex(0xDCDCE1),
            border: alpha(0x0A0A1014),
            border_strong: alpha(0x0A0A1024),
            text: hex(0x1D1D1F),
            text_dim: hex(0x5E5E66),
            text_faint: hex(0x8A8A90),
            accent: hex(0x1D1D1F),
            accent_hover: hex(0x000000),
            accent_soft: alpha(0x0A0A1012),
            on_accent: hex(0xFFFFFF),
            playing: hex(0x0A84FF),
            selection: hex(0xE5E5EA),
            ok: hex(0x248A3D),
            warn: hex(0x8A6320),
            danger: hex(0xC0443A),
            overlay: alpha(0x00000066),
            scrollbar: alpha(0x0A0A1024),
            scrollbar_hover: alpha(0x0A0A1040),
            focus_ring: alpha(0x0A84FF66),
            disabled_bg: hex(0xEDEEF0),
            disabled_fg: hex(0xB4B4BC),
            surface_hover: alpha(0x0A0A100D),
            accent_dim: hex(0xDCEBFF),
            glass: alpha(0xFFFFFFF2),
            glass_border: alpha(0x0A0A1014),
            shadow: alpha(0x0B0B1224),
            shadow_strong: alpha(0x0B0B1233),
            radius: 8.0,
            radius_sm: 6.0,
            radius_lg: 12.0,
            font_family: SharedString::from("Noto Sans CJK SC"),
            font_mono: SharedString::from("Noto Sans Mono"),
            font_size: 13.0,
        }
    }

    pub fn for_mode(mode: Appearance) -> Self {
        match mode {
            Appearance::Dark => Self::dark(),
            Appearance::Light => Self::light(),
        }
    }

    pub fn get(cx: &App) -> &Theme {
        static FALLBACK: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();
        cx.try_global::<Theme>()
            .unwrap_or_else(|| FALLBACK.get_or_init(Theme::light))
    }

    pub fn set_global(self, cx: &mut App) {
        cx.set_global(self);
    }

    pub fn set_mode(&mut self, mode: Appearance) {
        *self = Self::for_mode(mode);
    }

    pub fn is_dark(&self) -> bool {
        self.mode == Appearance::Dark
    }
}

/// 0xRRGGBB → 不透明 Hsla。
pub fn hex(rgb: u32) -> Hsla {
    gpui::rgb(rgb).into()
}

/// 0xRRGGBBAA → Hsla。
pub fn alpha(rgba: u32) -> Hsla {
    gpui::rgba(rgba).into()
}

pub fn fmt_ms(ms: u64) -> String {
    let total = ms / 1000;
    format!("{}:{:02}", total / 60, total % 60)
}

pub mod text_size {
    pub const MICRO: f32 = 10.0;
    pub const META: f32 = 11.0;
    pub const SMALL: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const BODY_LG: f32 = 14.0;
    pub const TITLE: f32 = 18.0;
    pub const DISPLAY: f32 = 20.0;
}

pub mod size {
    pub const TITLEBAR: f32 = 40.0;
    pub const SIDEBAR: f32 = 232.0;
    /// 中间曲目板：macOS Music 的固定窄列表。
    pub const LIST: f32 = 352.0;
    /// 悬浮底栏（Dock）的高度与离底距离。时间轴独占下层，因此比旧版高 16px。
    pub const DOCK: f32 = 88.0;
    pub const DOCK_BOTTOM: f32 = 18.0;
    pub const ROW: f32 = 52.0;
    pub const CONTROL: f32 = 30.0;
    pub const PAGE_PAD: f32 = 18.0;
    pub const PANE_HEAD: f32 = 52.0;
    pub const DETAIL: f32 = 320.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_opaque_and_correct() {
        let c = hex(0x121212);
        assert!((c.a - 1.0).abs() < f32::EPSILON);
        assert!(c.l < 0.1);
        let light = hex(0xFAFAFA);
        assert!(light.l > 0.9);
    }

    #[test]
    fn alpha_uses_rrggbbaa() {
        let c = alpha(0xFFFFFF0F);
        assert!((c.a - 15.0 / 255.0).abs() < 0.01);
    }

    #[test]
    fn light_and_dark_differ() {
        assert_ne!(Theme::dark().bg, Theme::light().bg);
    }
}
