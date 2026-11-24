//! `ChordialApp` 的 render_quality 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    /// 音质标签点击后的「音频解码信息」弹窗（AMLL `AudioQualityDialog`）。
    pub(super) fn render_quality_dialog(&self, cx: &mut Context<Self>) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let track = self.current_track.as_ref();
        let quality = track.and_then(|t| t.quality_info.as_ref());
        let codec = quality
            .and_then(|q| q.codec.clone())
            .unwrap_or_else(|| "未知".into());
        let bitrate = quality
            .and_then(|q| q.bitrate_kbps)
            .map(|v| format!("{v} kbps"))
            .unwrap_or_else(|| "—".into());
        let sample_rate = quality
            .and_then(|q| q.sample_rate_hz)
            .map(|v| format!("{v} Hz"))
            .unwrap_or_else(|| "—".into());
        let tier = quality
            .map(|q| match q.tier {
                chordal_core::model::QualityTier::Unknown => "未知",
                chordal_core::model::QualityTier::Standard => "标准",
                chordal_core::model::QualityTier::High => "高品质",
                chordal_core::model::QualityTier::Lossless => "无损",
                chordal_core::model::QualityTier::HiRes => "高解析度",
            })
            .unwrap_or("—");
        let lossless = quality
            .map(|q| if q.lossless { "是" } else { "否" })
            .unwrap_or("—");
        let row = |label: &'static str, value: String| {
            div()
                .w_full()
                .h(px(28.0))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(white.opacity(0.5))
                        .child(SharedString::from(label)),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(white.opacity(0.9))
                        .child(SharedString::from(value)),
                )
        };
        let close = {
            let weak = weak.clone();
            div()
                .id("quality-dialog-close")
                .h(px(30.0))
                .px(px(14.0))
                .rounded(px(7.0))
                .bg(white.opacity(0.12))
                .hover(|s| s.bg(white.opacity(0.18)))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(12.0))
                .text_color(white)
                .child("关闭")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.quality_dialog_open = false;
                        cx.notify();
                    });
                })
        };
        let backdrop = {
            let weak = weak.clone();
            div()
                .id("quality-dialog-backdrop")
                .absolute()
                .inset_0()
                .bg(gpui::black().opacity(0.45))
                .flex()
                .items_center()
                .justify_center()
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.quality_dialog_open = false;
                        cx.notify();
                    });
                })
        };
        let card = div()
            .id("quality-dialog")
            .w(px(340.0))
            .rounded(px(12.0))
            .bg(gpui::rgba(0x1C1C20F7))
            .border_1()
            .border_color(white.opacity(0.1))
            .shadow(vec![BoxShadow {
                color: gpui::black().opacity(0.35),
                offset: point(px(0.0), px(16.0)),
                blur_radius: px(40.0),
                spread_radius: px(0.0),
            }])
            .p(px(20.0))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .text_size(px(16.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(white.opacity(0.95))
                    .child("音频解码信息"),
            )
            .child(
                div()
                    .w_full()
                    .h(px(1.0))
                    .my(px(10.0))
                    .bg(white.opacity(0.1)),
            )
            .child(row("音频解码器", codec))
            .child(row("码率", bitrate))
            .child(row("采样率", sample_rate))
            .child(row("品质", tier.to_string()))
            .child(row("无损", lossless.to_string()))
            .child(
                div()
                    .w_full()
                    .h(px(1.0))
                    .my(px(10.0))
                    .bg(white.opacity(0.1)),
            )
            .child(div().w_full().flex().justify_end().child(close));
        backdrop.child(card).into_any_element()
    }

    /// 「音频详情」视图：与右侧歌词列共用同一块容器，不弹模态框。
    pub(super) fn render_audio_details_panel(
        &self,
        width: f32,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let track = self.current_track.as_ref();
        let quality = track.and_then(|t| t.quality_info.as_ref());
        let field = |label: &'static str, value: String| {
            div()
                .w_full()
                .min_h(px(34.0))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .border_b_1()
                .border_color(white.opacity(0.07))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(white.opacity(0.5))
                        .child(SharedString::from(label)),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(white.opacity(0.92))
                        .text_right()
                        .child(SharedString::from(value)),
                )
        };
        let tier = quality
            .map(|q| match q.tier {
                chordal_core::model::QualityTier::Unknown => "未知",
                chordal_core::model::QualityTier::Standard => "标准",
                chordal_core::model::QualityTier::High => "高品质",
                chordal_core::model::QualityTier::Lossless => "无损",
                chordal_core::model::QualityTier::HiRes => "高解析度",
            })
            .unwrap_or("—")
            .to_string();
        let source = track
            .map(|t| match &t.origin {
                TrackOrigin::Local { .. } => "本地音乐".to_string(),
                TrackOrigin::Remote { source } => match source.0.as_str() {
                    "netease" => "网易云音乐".to_string(),
                    other => other.to_string(),
                },
            })
            .unwrap_or_else(|| "—".into());
        let badge = track.and_then(|t| t.quality.clone()).unwrap_or_default();

        div()
            .w(px(width))
            .h_full()
            .flex()
            .flex_col()
            .pt(px(28.0))
            .child(
                div()
                    .text_size(px(19.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(white.opacity(0.95))
                    .child("音频详情"),
            )
            .child(
                div()
                    .mt(px(6.0))
                    .mb(px(14.0))
                    .h(px(1.0))
                    .bg(white.opacity(0.12)),
            )
            .child(field(
                "音质",
                if badge.is_empty() {
                    tier
                } else {
                    format!("{} · {}", badge, tier)
                },
            ))
            .child(field(
                "解码器",
                quality
                    .and_then(|q| q.codec.clone())
                    .unwrap_or_else(|| "未知".into()),
            ))
            .child(field(
                "码率",
                quality
                    .and_then(|q| q.bitrate_kbps)
                    .map(|v| format!("{v} kbps"))
                    .unwrap_or_else(|| "—".into()),
            ))
            .child(field(
                "采样率",
                quality
                    .and_then(|q| q.sample_rate_hz)
                    .map(|v| format!("{v} Hz"))
                    .unwrap_or_else(|| "—".into()),
            ))
            .child(field(
                "无损",
                quality
                    .map(|q| if q.lossless { "是" } else { "否" })
                    .unwrap_or("—")
                    .to_string(),
            ))
            .child(field("音源", source))
            .into_any_element()
    }
}
