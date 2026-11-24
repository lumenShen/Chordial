//! 歌词模糊的视觉烟雾测试。
//!
//! `cargo run -p chordial-amll-gpui --example blur_demo`
//!
//! 固定 10 行动态歌词，初始时间停在中间行，因此焦点行清晰、上下行分别处于
//! blur 2~5 档，可直接肉眼对比重影是否消失。

use std::sync::Arc;
use std::time::Instant;

use chordial_amll_gpui::render::{LyricPlayerView, LyricRenderColors, PlaybackClock};
use chordial_amll_gpui::text::LyricTextStyle;
use chordial_amll_gpui::{BackgroundRendererKind, LyricPlayerConfig, parse_auto};
use gpui::prelude::*;
use gpui::{
    App, Application, Bounds, Context, Render, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, px, size,
};

const LYRICS: &str = "\
[00:00.000]夜色[00:01.200]温柔[00:02.400] 而我[00:03.600]心碎[00:04.800]
[00:05.000]路灯[00:06.000]把影子[00:07.200]拉长[00:08.500]
[00:10.000]风从[00:11.000]旧站台[00:12.200]吹过来[00:13.800]
[00:15.000]带着[00:16.000]那年夏天[00:17.400]的潮湿[00:19.200]
[00:20.000]我数着[00:21.200]车窗上[00:22.400]倒退的[00:23.800]灯[00:24.600]
[00:25.000]像数着[00:26.200]没说完[00:27.400]的话[00:28.800]
[00:30.000]如果[00:31.000]时间[00:32.000]可以[00:33.200]倒放[00:34.600]
[00:35.000]我想[00:36.000]再看[00:37.200]一眼[00:38.400]那天的[00:39.600]月亮[00:40.800]
[00:42.000]可惜[00:43.000]列车[00:44.200]只会[00:45.400]向前[00:46.800]
[00:48.000]而我[00:49.000]还在[00:50.200]原地[00:51.400]等[00:52.600]
";

struct DemoClock {
    start: Instant,
}

impl PlaybackClock for DemoClock {
    fn position_ms(&self) -> u64 {
        20_000 + self.start.elapsed().as_millis() as u64
    }

    fn is_playing(&self) -> bool {
        true
    }
}

struct DemoRoot {
    view: gpui::Entity<LyricPlayerView>,
}

impl Render for DemoRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(gpui::rgb(0x101014))
            .child(self.view.clone())
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let clock: Arc<dyn PlaybackClock> = Arc::new(DemoClock {
            start: Instant::now(),
        });
        let view = cx.new(|cx| {
            let mut view = LyricPlayerView::new(cx);
            view.set_clock(Some(clock));
            view.set_colors(LyricRenderColors {
                text: gpui::white(),
                translation: gpui::white(),
                overlay: gpui::rgba(0x00000033).into(),
                interlude_dot: gpui::white(),
                fallback_background: gpui::rgb(0x101014).into(),
                glow: gpui::rgb(0x88ccff).into(),
            });
            view.set_text_style(LyricTextStyle {
                font_family: "Microsoft YaHei UI".into(),
                font_weight: gpui::FontWeight::MEDIUM,
                base_font_size: 44.0,
                line_height_ratio: 1.35,
                sub_font_size_ratio: 0.54,
                sub_line_height_ratio: 1.45,
                padding_x_em: 1.1,
                padding_y_em: 0.6,
                ..Default::default()
            });
            view.set_player_config(LyricPlayerConfig {
                align_position: 0.4,
                base_font_size: 44.0,
                non_dynamic_opacity: 0.6,
                ..Default::default()
            });
            view.set_background_kind(BackgroundRendererKind::Css);
            view.set_background_visible(true);
            view.set_background_color(gpui::rgb(0x101014).into());
            view.set_lyrics(parse_auto(LYRICS), 20_000);
            view
        });

        let bounds = Bounds::centered(None, size(px(900.0), px(700.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Chordial blur demo".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.open_window(options, |_window, cx| cx.new(|_cx| DemoRoot { view }))
            .expect("open blur demo window");
    });
}
