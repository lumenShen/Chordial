//! `ChordialApp` 的 render_portrait_queue 行为。
//!
//! 竖屏/沉浸页的播放队列现在与主内容区共用同一套列表实现（截图版本）：
//! 浅色主题、播放队列页头、标题/时长表头、带封面与均衡器的曲目行。
//! `panel` 只决定是否占满内容宽度，不再维护另一套深色行渲染。

use super::*;

impl ChordialApp {
    pub(super) fn render_portrait_queue(
        &self,
        content_w: f32,
        _shared_hidden: bool,
        panel: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let queue = self.queue.clone();
        let mode = self.play_mode;

        // 与桌面播放队列完全一致的页头动作：播放全部 / 顺序 / 清除 / 专辑 / 歌手。
        let mut actions = div().flex().items_center().gap(px(6.0));
        if !queue.is_empty() {
            let weak = weak.clone();
            actions = actions.child(crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Play, 13.0, theme.text_dim),
                28.0,
                "播放全部",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.queue_play_all(&QueueList::Queue, cx)
                    });
                },
            ));
        }
        {
            let weak_mode = weak.clone();
            actions = actions.child(
                div()
                    .id("portrait-queue-mode")
                    .h(px(26.0))
                    .px(px(9.0))
                    .rounded(px(13.0))
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .text_color(theme.text_dim)
                    .hover(|s| s.bg(theme.surface_alt).text_color(theme.text))
                    .child(crate::icons::icon(
                        match mode {
                            PlayMode::Shuffle => crate::icons::Icon::Shuffle,
                            PlayMode::RepeatOne => crate::icons::Icon::RepeatOne,
                            PlayMode::RepeatAll => crate::icons::Icon::Repeat,
                            PlayMode::Sequential => crate::icons::Icon::Forward,
                        },
                        10.0,
                        theme.text_faint,
                    ))
                    .child(mode.short_label())
                    .on_click(move |_, _, cx| {
                        let _ = weak_mode.update(cx, |app, cx| app.cycle_play_mode(cx));
                    }),
            );
        }
        if !queue.is_empty() {
            let weak = weak.clone();
            actions = actions.child(
                div()
                    .id("portrait-queue-clear")
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .text_color(theme.text_faint)
                    .hover(|s| s.bg(theme.surface_alt).text_color(theme.danger))
                    .child("清除")
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.clear_queue(cx));
                    }),
            );
        }
        let has_album = self
            .current_track
            .as_ref()
            .and_then(|t| t.album.as_ref())
            .and_then(|a| a.id.as_ref())
            .is_some();
        let has_artist = self
            .current_track
            .as_ref()
            .is_some_and(|t| t.artists.iter().any(|a| a.id.is_some()));
        if has_album {
            let weak = weak.clone();
            actions = actions.child(crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Layers, 13.0, theme.text_dim),
                28.0,
                "当前专辑",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.open_album_for_current(cx));
                },
            ));
        }
        if has_artist {
            let weak = weak.clone();
            actions = actions.child(crate::widgets::icon_button(
                &theme,
                crate::icons::icon(crate::icons::Icon::Note, 13.0, theme.text_dim),
                28.0,
                "当前歌手",
                move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.open_artist_for_current(cx));
                },
            ));
        }

        let header = self.render_pane_header(
            &theme,
            "播放队列",
            format!("{} 首", queue.len()),
            Some(actions.into_any_element()),
        );

        let body = if queue.is_empty() {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_size(px(12.0))
                .text_color(theme.text_faint)
                .child("播放队列为空")
                .into_any_element()
        } else {
            self.render_track_table(&theme, queue, QueueList::Queue, cx)
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.surface)
            .w(px(content_w))
            .when(panel, |d| d.border_l_1().border_color(theme.border))
            .child(header)
            .child(body)
            .into_any_element()
    }
}
