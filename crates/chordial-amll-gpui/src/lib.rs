//! `chordial-amll-gpui`：在 gpui 上等价实现 AMLL 的歌词播放器 / 歌词渲染 /
//! 歌词背景渲染器。
//!
//! 模块划分：
//! - [`model`] / [`parser`] / [`document`]：AMLL 歌词模型、LRC/ESLRC 解析、
//!   与 `setLyricLines` 等价的时间整理；
//! - [`spring`] / [`player`]：解析弹簧与热行/缓冲行滚动状态机（纯逻辑，无 gpui）；
//! - [`text`]：gpui 文本整形、逐词几何与 LRU 缓存；
//! - [`render`]：`LyricPlayerView`（gpui Render）+ 逐字形绘制、渐变、强调、模糊；
//! - [`background`]：`LyricBackgroundRenderer`（Mesh Gradient / CSS / None）。
//!
//! 行为参考 AMLL (GPL-3.0) 的公开行为与文档，本 crate 为独立实现，未复制
//! 其源码；性能与内存约束见各模块文档。

pub mod background;
pub mod document;
pub mod model;
pub mod parser;
pub mod player;
pub mod render;
pub mod spring;
pub mod text;

pub use background::{
    BackgroundRendererKind, ControlPoint, ControlPointPreset, CssBackgroundRenderer,
    LyricBackgroundRenderer, MeshGradientRenderer,
};
pub use document::process as process_lyrics;
pub use model::{LyricDocument, LyricLine, LyricWord};
pub use parser::{LyricFormat, merge_translations, parse_auto, parse_eslrc, parse_lrc};
pub use player::{
    AlignAnchor, InterludeDotsFrame, InterludeFrame, LayoutMetrics, LineFrameState, LyricPlayer,
    LyricPlayerConfig, PlayerFrame,
};
pub use render::LyricPlayerView;
pub use spring::{Spring, SpringParams};
pub use text::{LyricTextStyle, TextLayoutCache};
