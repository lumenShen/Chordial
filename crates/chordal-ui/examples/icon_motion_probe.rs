//! 图标动画关键帧探针：把 AMLL 的箭头合并 / 按压回弹 / 喇叭弹跳按固定
//! 进度并排画出来，便于截图核对。
//!
//! `cargo run -p chordal-ui --example icon_motion_probe`

#[path = "../src/icons.rs"]
mod icons;

use gpui::prelude::*;
use gpui::{
    App, Application, Bounds, Context, Render, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, px, size,
};

use icons::{Icon, IconMotion, icon_motion};

struct ProbeRoot;

impl Render for ProbeRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let col = |title: &'static str, children: Vec<gpui::AnyElement>| {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .text_color(gpui::rgb(0x888888))
                        .text_size(px(12.0))
                        .child(title),
                )
                .children(children)
        };
        let arrow_steps = [0.0, 0.1, 0.2, 0.33, 0.5, 0.7, 1.0];
        let press_steps = [0.0, 0.1, 0.2, 0.35, 0.5, 0.7, 1.0];
        let speak_steps = [0.0, 0.1, 0.2, 0.3, 0.45, 0.6, 0.75];

        div()
            .size_full()
            .bg(gpui::rgb(0x101014))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(36.0))
            .child(col(
                "Forward arrows 0..1",
                arrow_steps
                    .iter()
                    .map(|&p| {
                        icon_motion(
                            Icon::Forward,
                            56.0,
                            gpui::white(),
                            IconMotion {
                                arrows: p,
                                ..Default::default()
                            },
                        )
                    })
                    .collect(),
            ))
            .child(col(
                "Rewind arrows 0..1",
                arrow_steps
                    .iter()
                    .map(|&p| {
                        icon_motion(
                            Icon::Rewind,
                            56.0,
                            gpui::white(),
                            IconMotion {
                                arrows: p,
                                ..Default::default()
                            },
                        )
                    })
                    .collect(),
            ))
            .child(col(
                "Play press 0..1",
                press_steps
                    .iter()
                    .map(|&p| {
                        icon_motion(
                            Icon::Play,
                            56.0,
                            gpui::white(),
                            IconMotion {
                                press: p,
                                ..Default::default()
                            },
                        )
                    })
                    .collect(),
            ))
            .child(col(
                "Speaker bounce 0..0.75",
                speak_steps
                    .iter()
                    .map(|&p| {
                        icon_motion(
                            Icon::VolumeHigh,
                            56.0,
                            gpui::white(),
                            IconMotion {
                                speaker: p,
                                ..Default::default()
                            },
                        )
                    })
                    .collect(),
            ))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::new(gpui::point(px(0.), px(0.)), size(px(720.), px(520.)));
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Icon motion probe".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| cx.new(|_| ProbeRoot),
        )
        .expect("open probe window");
    });
}
