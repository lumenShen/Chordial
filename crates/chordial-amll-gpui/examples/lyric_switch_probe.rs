//! 歌词替换交叉淡化的静态探针：启动 3s 后把歌词换成第二首，600ms 生命周期
//! 自动走完。设置 `SWITCH_T=0.5` 可把进度钉在指定 t，便于逐帧截图核对
//! 淡出/淡入、模糊与内容缩放（背景不参与）。
//!
//! ```text
//! cargo run -p chordial-amll-gpui --example lyric_switch_probe
//! SWITCH_T=0.5 cargo run -p chordial-amll-gpui --example lyric_switch_probe
//! ```

use std::sync::Arc;
use std::time::Duration;

use chordial_amll_gpui::LyricWord;
use chordial_amll_gpui::model::LyricLine;
use chordial_amll_gpui::render::{LyricPlayerView, PlaybackClock};
use gpui::prelude::*;
use gpui::{
    App, Application, Bounds, Context, Render, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, px,
};

struct FixedClock;

impl PlaybackClock for FixedClock {
    fn position_ms(&self) -> u64 {
        1_200
    }

    fn is_playing(&self) -> bool {
        true
    }
}

fn lines(prefix: &str) -> Vec<LyricLine> {
    ["第一句歌词", "第二句歌词", "第三句歌词"]
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let start = i as u64 * 2_000;
            LyricLine {
                words: vec![LyricWord::new(
                    format!("{prefix} · {text}"),
                    start,
                    start + 1_600,
                )],
                start_time: start,
                end_time: start + 1_600,
                ..Default::default()
            }
        })
        .collect()
}

struct ProbeRoot {
    view: gpui::Entity<LyricPlayerView>,
    freeze_t: Option<f32>,
}

impl Render for ProbeRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(t) = self.freeze_t {
            self.view.read(cx).debug_set_switch_t(t);
        }
        div()
            .size_full()
            .bg(gpui::rgb(0x101014))
            .child(self.view.clone())
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let clock: Arc<dyn PlaybackClock> = Arc::new(FixedClock);
        let bounds = Bounds::centered(None, gpui::size(px(900.0), px(600.0)), cx);
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("lyric switch probe".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| {
                let view = cx.new(|cx| {
                    let mut view = LyricPlayerView::new(cx);
                    view.set_clock(Some(clock.clone()));
                    view.set_background_visible(false);
                    view.set_lyrics(lines("旧曲"), 1_200);
                    view
                });
                cx.new(|cx| {
                    let freeze_t = std::env::var("SWITCH_T")
                        .ok()
                        .and_then(|v| v.parse::<f32>().ok());
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(3_000))
                            .await;
                        let _ = this.update(cx, |root: &mut ProbeRoot, cx| {
                            root.view
                                .update(cx, |view, _| view.set_lyrics(lines("新曲"), 1_200));
                        });
                    })
                    .detach();
                    ProbeRoot {
                        view: view.clone(),
                        freeze_t,
                    }
                })
            },
        );
    });
}
