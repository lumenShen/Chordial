//! 逐词动画的静态探针：在固定播放时间下渲染一屏动态歌词，便于截图核对
//! AMLL 的扫光边界、逐词浮动与强调词（缩放/位移/辉光）。
//!
//! `MOTION_MS=2000 cargo run -p chordial-amll-gpui --example lyric_motion_probe`

use std::sync::Arc;

use chordial_amll_gpui::render::{LyricPlayerView, LyricRenderColors, PlaybackClock};
use chordial_amll_gpui::text::LyricTextStyle;
use chordial_amll_gpui::{LyricPlayerConfig, parse_eslrc};
use gpui::prelude::*;
use gpui::{
    App, Application, Bounds, Context, Render, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, px, size,
};

const LYRICS: &str = "\
[00:00.00] Go [00:00.40]on, [00:00.80]give [00:01.20]yourself [00:02.40] a [00:02.70]chance [00:04.50]
[00:05.00] Rush [00:05.50]away [00:06.00]like [00:06.50]an [00:07.00]avalanche [00:08.80]
[00:09.50] Look [00:10.10]down [00:10.70]on [00:11.20]a [00:11.60]lonely [00:12.40]flame [00:14.00]
";

struct FixedClock {
    ms: u64,
}

impl PlaybackClock for FixedClock {
    fn position_ms(&self) -> u64 {
        self.ms
    }

    fn is_playing(&self) -> bool {
        true
    }
}

struct ProbeRoot {
    view: gpui::Entity<LyricPlayerView>,
}

impl Render for ProbeRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(gpui::rgb(0x101014))
            .child(self.view.clone())
    }
}

fn main() {
    let ms = std::env::var("MOTION_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    Application::new().run(move |cx: &mut App| {
        let clock: Arc<dyn PlaybackClock> = Arc::new(FixedClock { ms });
        let view = cx.new(|cx| {
            let mut view = LyricPlayerView::new(cx);
            view.set_clock(Some(clock));
            view.set_background_visible(false);
            view.set_colors(LyricRenderColors {
                text: gpui::white(),
                translation: gpui::white(),
                overlay: gpui::rgba(0x00000000).into(),
                interlude_dot: gpui::white(),
                fallback_background: gpui::rgb(0x101014).into(),
                glow: gpui::white(),
            });
            view.set_text_style(LyricTextStyle {
                font_family: "Microsoft YaHei UI".into(),
                font_weight: gpui::FontWeight::SEMIBOLD,
                base_font_size: 56.0,
                line_height_ratio: 1.2,
                sub_font_size_ratio: 0.5,
                sub_line_height_ratio: 1.5,
                padding_x_em: 1.0,
                padding_y_ratio: 0.02,
                ..Default::default()
            });
            view.set_player_config(LyricPlayerConfig {
                align_anchor: chordial_amll_gpui::AlignAnchor::Center,
                align_position: 0.5,
                base_font_size: 56.0,
                ..Default::default()
            });
            view.set_lyrics(parse_eslrc(LYRICS), ms);
            view
        });

        let bounds = Bounds::new(gpui::point(px(0.), px(0.)), size(px(1200.), px(800.)));
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Lyric motion probe".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_window, cx| cx.new(|_| ProbeRoot { view }),
        )
        .expect("open probe window");
    });
}
