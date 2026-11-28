//! `ChordialApp` 的 render_portrait_queue 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

impl ChordialApp {
    /// `panel = true` 时作为右侧内容区（与宽屏歌词列同位置）渲染：
    /// 去掉「向下轻扫返回」提示，播放模式胶囊按面板左缘定位。
    pub(super) fn render_portrait_queue(
        &self,
        content_w: f32,
        shared_hidden: bool,
        panel: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let queue = self.queue.clone();
        let current = self.queue_pos;
        let mode = self.play_mode;

        let hint: AnyElement = if panel {
            div().into_any_element()
        } else {
            let weak = weak.clone();
            div()
                .id("queue-back")
                .w_full()
                .h(px(58.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(white.opacity(0.58))
                .child("此处向下轻扫以返回播放界面")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.set_portrait_page(PortraitPage::Art, cx);
                    });
                })
                .into_any_element()
        };

        let track_header = {
            let title = self.current_title_text();
            let artist = self.current_artist_text();
            let theme = Theme::dark();
            let cover = crate::widgets::cover_plain(
                &theme,
                self.cover_image.clone(),
                crate::layout_morph::QUEUE_COVER,
                crate::layout_morph::QUEUE_COVER_RADIUS,
                crate::icons::icon(crate::icons::Icon::Note, 20.0, white.opacity(0.35)),
            );
            self.stage_measurements.observe(
                StageMode::CompactQueue,
                div()
                    .w(px(content_w))
                    .flex()
                    .items_center()
                    .gap(px(crate::layout_morph::QUEUE_COVER_GAP))
                    .pb(px(10.0))
                    .child(
                        div()
                            .opacity(if shared_hidden { 0.0 } else { 1.0 })
                            .child(cover),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .opacity(if shared_hidden { 0.0 } else { 1.0 })
                            .child(
                                div()
                                    .text_size(px(16.0))
                                    .line_height(px(16.0 * INFO_LINE_RATIO))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(white.opacity(0.9))
                                    .text_ellipsis()
                                    .child(SharedString::from(title)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .line_height(px(11.0 * INFO_LINE_RATIO))
                                    .text_color(white.opacity(0.45))
                                    .text_ellipsis()
                                    .child(SharedString::from(artist)),
                            ),
                    ),
                &[(0, MeasurePart::Cover), (1, MeasurePart::Info)],
            )
        };

        let clear = {
            let weak = weak.clone();
            div()
                .id("queue-clear")
                .h(px(28.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(white.opacity(0.55))
                .hover(|s| s.bg(white.opacity(0.08)).text_color(white.opacity(0.85)))
                .child("清除")
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.stop_playback(cx);
                        app.set_queue(Arc::new(Vec::new()), None);
                        app.queue_pos = None;
                        cx.notify();
                    });
                })
        };
        let count_row = div()
            .w(px(content_w))
            .h(px(34.0))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(white.opacity(0.42))
                    .child(SharedString::from(format!(
                        "{}/{}",
                        current.map(|p| p + 1).unwrap_or(0),
                        queue.len()
                    ))),
            )
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(white.opacity(0.92))
                    .child("播放队列"),
            )
            .child(clear);

        let list: AnyElement = if queue.is_empty() {
            div()
                .w(px(content_w))
                .flex_1()
                .min_h_0()
                .pt(px(40.0))
                .text_center()
                .text_size(px(12.0))
                .text_color(white.opacity(0.4))
                .child("队列为空")
                .into_any_element()
        } else {
            let rows_queue = queue.clone();
            let weak_rows = weak.clone();
            let mut list =
                gpui::uniform_list("queue-list", queue.len(), move |range, _window, _cx| {
                    range
                        .filter_map(|index| {
                            let track = rows_queue.get(index)?;
                            let is_current = current == Some(index);
                            let title = track.title.clone();
                            let artist = track
                                .artists
                                .iter()
                                .map(|a| a.name.as_str())
                                .collect::<Vec<_>>()
                                .join(" / ");
                            let weak_row = weak_rows.clone();
                            let weak_remove = weak_rows.clone();
                            Some(
                                div()
                                    .id(gpui::ElementId::named_usize("queue-row", index))
                                    .w_full()
                                    .h(px(60.0))
                                    .pb(px(2.0))
                                    .px(px(12.0))
                                    .rounded(px(10.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .when(is_current, |d| d.bg(white.opacity(0.08)))
                                    .hover(|s| s.bg(white.opacity(0.10)))
                                    .cursor_pointer()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.0))
                                            .child(
                                                div()
                                                    .text_size(px(13.5))
                                                    .font_weight(if is_current {
                                                        gpui::FontWeight::MEDIUM
                                                    } else {
                                                        gpui::FontWeight::NORMAL
                                                    })
                                                    .text_color(white.opacity(if is_current {
                                                        0.96
                                                    } else {
                                                        0.82
                                                    }))
                                                    .text_ellipsis()
                                                    .child(SharedString::from(title)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(10.5))
                                                    .text_color(white.opacity(0.38))
                                                    .text_ellipsis()
                                                    .child(SharedString::from(artist)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id(gpui::ElementId::named_usize("queue-remove", index))
                                            .size(px(28.0))
                                            .rounded(px(6.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .text_size(px(14.0))
                                            .text_color(white.opacity(0.38))
                                            .hover(|s| {
                                                s.bg(white.opacity(0.10))
                                                    .text_color(white.opacity(0.8))
                                            })
                                            .child("—")
                                            .on_click(move |_, _, cx| {
                                                cx.stop_propagation();
                                                let _ = weak_remove.update(cx, |app, cx| {
                                                    app.remove_queue_at(index, cx);
                                                });
                                            }),
                                    )
                                    .on_click(move |_, _, cx| {
                                        let _ = weak_row.update(cx, |app, cx| {
                                            app.play_from_list(QueueList::Queue, index, cx);
                                        });
                                    })
                                    .into_any_element(),
                            )
                        })
                        .collect::<Vec<_>>()
                });
            if self.smooth_scroll {
                list = list.smooth_scroll();
            }
            list.track_scroll(self.queue_scroll.clone())
                .flex_1()
                .min_h_0()
                .w(px(content_w))
                .into_any_element()
        };

        let pill = {
            let weak = weak.clone();
            div()
                .id("queue-mode-pill")
                .absolute()
                .bottom(px(22.0))
                .left(px(if panel {
                    16.0
                } else {
                    ((self.viewport_width - content_w) / 2.0).max(16.0)
                }))
                .h(px(32.0))
                .px(px(14.0))
                .rounded(px(16.0))
                .bg(white.opacity(0.10))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(white.opacity(0.78))
                .hover(|s| s.bg(white.opacity(0.15)))
                .child(SharedString::from(mode.full_label()))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.cycle_play_mode(cx));
                })
        };

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(4.0))
            .child(hint)
            .child(div().w(px(content_w)).child(track_header))
            .child(count_row)
            .child(div().w(px(content_w)).h(px(1.0)).bg(white.opacity(0.10)))
            .child(list)
            .child(pill)
            .into_any_element()
    }
}
