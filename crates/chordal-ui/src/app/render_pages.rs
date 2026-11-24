//! 整页主内容：音源页 / 管理音源 / 二级编辑 / 本地与订阅歌单。
//!
//! 这些页面覆盖在“曲库三栏”之上，侧栏和 Dock 保持不变。页面只负责展示与
//! 发出动作；持久化、实例注册与歌单写入都在 `source_ops` / `playlist_ops`。

use super::*;
use chordal_core::playlist::{PlaylistKind, PlaylistMutation};

const PAGE_MAX_W: f32 = 1180.0;

fn page_scroll(
    theme: &Theme,
    content: AnyElement,
    max_w: f32,
    scroll: &gpui::ScrollHandle,
) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .bg(theme.surface)
        .overflow_hidden()
        .child(
            div()
                .id("main-page-scroll")
                .size_full()
                .track_scroll(scroll)
                .overflow_y_scroll()
                .child(
                    div()
                        .w_full()
                        .max_w(px(max_w))
                        .mx_auto()
                        .px(px(30.0))
                        .pt(px(22.0))
                        .pb(px(120.0))
                        .child(content),
                ),
        )
        .into_any_element()
}

fn back_row(
    theme: &Theme,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("page-back-{label}")))
        .h(px(28.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .cursor_pointer()
        .text_size(px(10.5))
        .text_color(theme.text_dim)
        .hover(|s| s.bg(theme.hover).text_color(theme.text))
        .child(crate::icons::icon(
            crate::icons::Icon::ChevronLeft,
            11.0,
            theme.text_faint,
        ))
        .child(label)
        .on_click(on_click)
        .into_any_element()
}

fn page_header(
    theme: &Theme,
    eyebrow: String,
    title: String,
    subtitle: String,
    actions: AnyElement,
) -> AnyElement {
    div()
        .flex()
        .items_end()
        .justify_between()
        .gap(px(24.0))
        .child(
            div()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(9.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_faint)
                        .child(SharedString::from(eyebrow)),
                )
                .child(
                    div()
                        .text_size(px(26.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .text_ellipsis()
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(theme.text_faint)
                        .text_ellipsis()
                        .child(SharedString::from(subtitle)),
                ),
        )
        .child(div().flex().items_center().gap(px(6.0)).child(actions))
        .into_any_element()
}

fn section_title(theme: &Theme, title: &'static str, meta: Option<String>) -> AnyElement {
    div()
        .h(px(28.0))
        .mt(px(18.0))
        .flex()
        .items_end()
        .justify_between()
        .child(
            div()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_faint)
                .child(title),
        )
        .children(meta.map(|meta| {
            div()
                .text_size(px(9.5))
                .text_color(theme.text_faint)
                .child(SharedString::from(meta))
        }))
        .into_any_element()
}

/// 带柔和色调的图标徽章：用于源 Hero、统计卡、状态卡。
fn icon_badge(theme: &Theme, icon: crate::icons::Icon, size: f32, tint: gpui::Hsla) -> AnyElement {
    let dark = theme.is_dark();
    div()
        .size(px(size))
        .flex_none()
        .rounded(px((size * 0.26).max(7.0)))
        .border_1()
        .border_color(tint.opacity(if dark { 0.26 } else { 0.16 }))
        .bg(tint.opacity(if dark { 0.20 } else { 0.10 }))
        .flex()
        .items_center()
        .justify_center()
        .child(crate::icons::icon(icon, size * 0.5, tint))
        .into_any_element()
}

/// 状态胶囊：小圆点 + 文案，颜色承担语义（绿=就绪 / 灰=关闭 / 蓝=等待）。
fn status_pill(theme: &Theme, text: impl Into<SharedString>, color: gpui::Hsla) -> AnyElement {
    let dark = theme.is_dark();
    div()
        .h(px(22.0))
        .px(px(9.0))
        .flex_none()
        .rounded(px(11.0))
        .border_1()
        .border_color(color.opacity(if dark { 0.34 } else { 0.22 }))
        .bg(color.opacity(if dark { 0.20 } else { 0.10 }))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(div().size(px(5.0)).rounded(px(3.0)).bg(color))
        .child(
            div()
                .text_size(px(9.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(color)
                .child(text.into()),
        )
        .into_any_element()
}

/// 源页面顶部统一 Hero：图标徽章 + 标题 + 描述 + 状态 + 操作。
fn source_hero(
    theme: &Theme,
    icon: crate::icons::Icon,
    eyebrow: &'static str,
    title: String,
    description: String,
    status: AnyElement,
    actions: AnyElement,
) -> AnyElement {
    div()
        .mt(px(12.0))
        .p(px(18.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .flex()
        .items_center()
        .gap(px(14.0))
        .child(icon_badge(theme, icon, 48.0, theme.text))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_faint)
                        .child(eyebrow),
                )
                .child(
                    div()
                        .text_size(px(22.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .text_ellipsis()
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(theme.text_dim)
                        .text_ellipsis()
                        .child(SharedString::from(description)),
                ),
        )
        .child(status)
        .child(actions)
        .into_any_element()
}

/// 推荐曲目/歌单的封面占位：按文本生成稳定的柔和色块 + 首字，
/// 不触发网络请求，也让列表有封面节奏。
fn cover_thumb(theme: &Theme, seed: &str, size: f32) -> AnyElement {
    let palette = [
        theme.playing,
        theme.ok,
        theme.warn,
        theme.accent,
        theme.text_dim,
    ];
    let hash: usize = seed
        .bytes()
        .enumerate()
        .map(|(index, byte)| byte as usize * (index + 1))
        .sum();
    let tint = palette[hash % palette.len()];
    let glyph = seed
        .chars()
        .next()
        .map(|char| char.to_string())
        .unwrap_or_else(|| "♪".into());
    let dark = theme.is_dark();
    div()
        .size(px(size))
        .flex_none()
        .rounded(px((size * 0.22).max(4.0)))
        .border_1()
        .border_color(tint.opacity(if dark { 0.28 } else { 0.18 }))
        .bg(tint.opacity(if dark { 0.22 } else { 0.12 }))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(px((size * 0.38).max(9.0)))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tint)
                .child(SharedString::from(glyph)),
        )
        .into_any_element()
}

/// 内容面板：推荐区块/目录列表的统一卡片容器。
fn content_panel(theme: &Theme, content: impl gpui::IntoElement) -> AnyElement {
    div()
        .mt(px(6.0))
        .p(px(10.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .child(content)
        .into_any_element()
}

/// 带竖线标记的章节标题，比纯文字更适合面板化布局。
fn section_title_v2(theme: &Theme, title: &'static str, meta: Option<String>) -> AnyElement {
    div()
        .h(px(30.0))
        .mt(px(20.0))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(3.0))
                        .h(px(13.0))
                        .rounded(px(2.0))
                        .bg(theme.text_dim),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(title),
                ),
        )
        .children(meta.map(|meta| {
            div()
                .text_size(px(9.5))
                .text_color(theme.text_faint)
                .child(SharedString::from(meta))
        }))
        .into_any_element()
}

/// 面板内部的紧凑标题（无外侧上边距）。
fn section_title_flat(theme: &Theme, title: &'static str, meta: Option<String>) -> AnyElement {
    div()
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(10.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_faint)
                .child(title),
        )
        .children(meta.map(|meta| {
            div()
                .text_size(px(9.5))
                .text_color(theme.text_faint)
                .child(SharedString::from(meta))
        }))
        .into_any_element()
}

/// 统计卡：图标 + 标签 + 大数值 + 说明 + 底部色条。
fn stat_tile(
    theme: &Theme,
    label: &'static str,
    value: String,
    hint: &'static str,
    icon: crate::icons::Icon,
    tint: gpui::Hsla,
) -> AnyElement {
    let dark = theme.is_dark();
    div()
        .flex_1()
        .min_w(px(0.0))
        .p(px(13.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(7.0))
                .child(
                    div()
                        .size(px(22.0))
                        .rounded(px(6.0))
                        .bg(tint.opacity(if dark { 0.20 } else { 0.11 }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(crate::icons::icon(icon, 12.0, tint)),
                )
                .child(
                    div()
                        .text_size(px(9.5))
                        .text_color(theme.text_faint)
                        .child(label),
                ),
        )
        .child(
            div()
                .text_size(px(23.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(SharedString::from(value)),
        )
        .child(
            div()
                .text_size(px(9.0))
                .text_color(theme.text_faint)
                .child(hint),
        )
        .child(
            div()
                .mt(px(2.0))
                .h(px(2.0))
                .w(px(28.0))
                .rounded(px(1.0))
                .bg(tint.opacity(if dark { 0.55 } else { 0.40 })),
        )
        .into_any_element()
}

fn source_toggle(
    theme: &Theme,
    on: bool,
    id: SourceId,
    weak: gpui::WeakEntity<ChordialApp>,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("page-source-switch-{}", id.0)))
        .w(px(34.0))
        .h(px(19.0))
        .flex_none()
        .rounded(px(10.0))
        .border_1()
        .border_color(if on {
            theme.accent
        } else {
            theme.border_strong
        })
        .bg(if on { theme.accent } else { theme.active })
        .relative()
        .cursor_pointer()
        .child(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(if on { 15.0 } else { 1.0 }))
                .size(px(15.0))
                .rounded(px(8.0))
                .bg(if on { theme.on_accent } else { theme.surface }),
        )
        .on_click(move |_, _, cx| {
            let id = id.clone();
            let _ = weak.update(cx, |app, cx| {
                let enabled = app.source_is_enabled(&id);
                app.set_source_enabled(&id, !enabled, cx);
            });
        })
        .into_any_element()
}

fn status_line(
    self_: &ChordialApp,
    config: &SourceInstanceConfig,
    id: &SourceId,
    cx: &App,
) -> (String, gpui::Hsla) {
    let status = self_.source_status(id);
    if !config.enabled {
        return ("已关闭 · 配置保留".into(), self_.theme_faint(cx));
    }
    match status {
        SourceStatus::Loaded => {
            if config.is_local() {
                let tracks = cx
                    .global::<Globals>()
                    .local_sources
                    .get(id)
                    .map(|local| local.stats().tracks)
                    .unwrap_or(0);
                (format!("已就绪 · {tracks} 首"), self_.theme_ok(cx))
            } else {
                let account = cx
                    .global::<Globals>()
                    .netease_sources
                    .get(id)
                    .and_then(|source| source.account().map(|a| a.account_state()));
                match account {
                    Some(AccountState::LoggedIn(profile)) => {
                        (format!("已连接 · {}", profile.nickname), self_.theme_ok(cx))
                    }
                    Some(AccountState::LoggedOut) => {
                        ("已连接 · 未登录".into(), self_.theme_warn(cx))
                    }
                    Some(AccountState::Unknown) => ("已连接 · 校验中".into(), self_.theme_warn(cx)),
                    _ => ("已连接".into(), self_.theme_ok(cx)),
                }
            }
        }
        SourceStatus::Failed(error) => (
            format!("加载失败 · {}", truncate(&error, 42)),
            self_.theme_danger(cx),
        ),
        SourceStatus::Registered => ("未加载 · 可到音源页启用".into(), self_.theme_faint(cx)),
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

fn relative_time(ms: u64) -> String {
    if ms == 0 {
        return "尚未同步".into();
    }
    let now = crate::playlists::now_ms();
    let delta = now.saturating_sub(ms);
    if delta < 60_000 {
        "刚刚".into()
    } else if delta < 3_600_000 {
        format!("{} 分钟前", delta / 60_000)
    } else if delta < 86_400_000 {
        format!("{} 小时前", delta / 3_600_000)
    } else {
        format!("{} 天前", delta / 86_400_000)
    }
}

/// 推荐流区块的提示/空态行。
fn feed_note(theme: &Theme, text: impl Into<SharedString>, danger: bool) -> AnyElement {
    div()
        .py(px(10.0))
        .px(px(6.0))
        .text_size(px(10.5))
        .text_color(if danger {
            theme.danger
        } else {
            theme.text_faint
        })
        .child(text.into())
        .into_any_element()
}

/// 推荐曲目区块：最多展示 8 首，点击把整段推荐作为队列播放。
fn feed_track_section(
    theme: &Theme,
    title: &'static str,
    hint: String,
    tracks: &[Track],
    error: Option<&SharedString>,
    loading: bool,
    empty_hint: &'static str,
    limit: usize,
    source: &SourceId,
    kind: SourceFeedKind,
    weak: &gpui::WeakEntity<ChordialApp>,
    track_covers: &std::sync::Mutex<TrackCovers>,
    queue_pos: Option<usize>,
    current_id: Option<&chordal_core::model::TrackId>,
    current_queue: Arc<Vec<Track>>,
) -> AnyElement {
    let mut block = div().flex().flex_col();
    block = block.child(section_title_flat(theme, title, Some(hint)));
    if loading && tracks.is_empty() {
        return block
            .child(feed_note(theme, "正在加载…", false))
            .into_any_element();
    }
    if let Some(error) = error {
        return block
            .child(feed_note(theme, error.clone(), false))
            .into_any_element();
    }
    if tracks.is_empty() {
        return block
            .child(feed_note(theme, empty_hint, false))
            .into_any_element();
    }
    // 推荐流行也复用完整队列的行渲染：封面懒加载、均衡器、时长与 ⋮ 菜单一致。
    let feed_list = QueueList::Feed {
        source: source.clone(),
        kind,
    };
    for (index, track) in tracks.iter().take(limit).enumerate() {
        let row_text = TrackRowText::new(index, track);
        block = block.child(ChordialApp::render_track_row(
            theme,
            index,
            &row_text,
            track_covers,
            feed_list.clone(),
            queue_pos,
            current_id,
            weak.clone(),
            current_queue.clone(),
            false,
            false,
            false,
            None,
            None,
            &TrackRowStyle::full(),
            None,
        ));
    }
    block.into_any_element()
}

/// 推荐歌单区块：点击打开集合页，可在集合页内订阅到「我的歌单」。
fn feed_collection_section(
    theme: &Theme,
    title: &'static str,
    hint: String,
    collections: &[CollectionSummary],
    error: Option<&SharedString>,
    loading: bool,
    empty_hint: &'static str,
    limit: usize,
    weak: &gpui::WeakEntity<ChordialApp>,
) -> AnyElement {
    let mut block = div().flex().flex_col();
    block = block.child(section_title_flat(theme, title, Some(hint)));
    if loading && collections.is_empty() {
        return block
            .child(feed_note(theme, "正在加载…", false))
            .into_any_element();
    }
    if let Some(error) = error {
        return block
            .child(feed_note(theme, error.clone(), false))
            .into_any_element();
    }
    if collections.is_empty() {
        return block
            .child(feed_note(theme, empty_hint, false))
            .into_any_element();
    }
    for (index, summary) in collections.iter().take(limit).enumerate() {
        let weak = weak.clone();
        let click_summary = summary.clone();
        let element_id = SharedString::from(format!("feed-collection-{title}-{}", summary.id.id));
        let title_text = SharedString::from(summary.title.clone());
        let creator_text = SharedString::from(
            summary
                .creator
                .clone()
                .filter(|creator| !creator.trim().is_empty())
                .unwrap_or_else(|| format!("#{}", index + 1)),
        );
        let count_text = SharedString::from(
            summary
                .track_count
                .map(|count| count.to_string())
                .unwrap_or_default(),
        );
        block = block.child(
            div()
                .id(element_id)
                .h(px(48.0))
                .px(px(6.0))
                .rounded(px(7.0))
                .flex()
                .items_center()
                .gap(px(11.0))
                .hover(|s| s.bg(theme.hover))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let summary = click_summary.clone();
                    let _ = weak.update(cx, |app, cx| {
                        app.page = None;
                        app.open_collection(summary.id.clone(), summary.title.clone(), cx);
                    });
                })
                .child(cover_thumb(theme, summary.title.as_str(), 36.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(theme.text)
                                .text_ellipsis()
                                .child(title_text),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(theme.text_faint)
                                .text_ellipsis()
                                .child(creator_text),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(9.5))
                        .text_color(theme.text_faint)
                        .child(count_text),
                ),
        );
    }
    block.into_any_element()
}

/// 推荐入口卡片：专辑图 + 名称 + 描述 + 曲数，点开进入详情列表。
/// 4 个入口横向排列，避免把完整曲目直接铺在源页面里。
fn feed_entry_card(
    theme: &Theme,
    cover_seed: &str,
    title: &'static str,
    desc: String,
    count: String,
    accent: gpui::Hsla,
    weak: &gpui::WeakEntity<ChordialApp>,
    source: SourceId,
    kind: SourceFeedKind,
) -> AnyElement {
    let weak_click = weak.clone();
    div()
        .id(SharedString::from(format!("feed-entry-{kind:?}")))
        .w(px(268.0))
        .h(px(92.0))
        .flex_none()
        .p(px(10.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface_alt)
        .flex()
        .items_center()
        .gap(px(11.0))
        .cursor_pointer()
        .hover(|s| s.bg(theme.hover).border_color(theme.border_strong))
        .on_click(move |_, _, cx| {
            let _ = weak_click.update(cx, |app, cx| {
                app.open_feed_detail(source.clone(), kind, cx);
            });
        })
        .child(
            div()
                .relative()
                .child(cover_thumb(theme, cover_seed, 64.0))
                .child(
                    div()
                        .absolute()
                        .right(px(-3.0))
                        .bottom(px(-3.0))
                        .size(px(20.0))
                        .rounded(px(10.0))
                        .border_2()
                        .border_color(theme.surface_alt)
                        .bg(accent)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(crate::icons::icon(
                            crate::icons::Icon::ChevronRight,
                            10.0,
                            theme.on_accent,
                        )),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .text_ellipsis()
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(theme.text_faint)
                        .text_ellipsis()
                        .child(SharedString::from(desc)),
                )
                .child(div().mt(px(2.0)).child(status_pill(theme, count, accent))),
        )
        .into_any_element()
}

impl ChordialApp {
    fn theme_ok(&self, cx: &App) -> gpui::Hsla {
        Theme::get(cx).ok
    }
    fn theme_warn(&self, cx: &App) -> gpui::Hsla {
        Theme::get(cx).warn
    }
    fn theme_danger(&self, cx: &App) -> gpui::Hsla {
        Theme::get(cx).danger
    }
    fn theme_faint(&self, cx: &App) -> gpui::Hsla {
        Theme::get(cx).text_faint
    }

    pub(super) fn render_main_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        match self.page.clone() {
            Some(MainPage::Source(id)) => self.render_source_page(&id, &theme, cx),
            Some(MainPage::ManageSources) => self.render_source_manager(&theme, cx),
            Some(MainPage::NewSource) => self.render_new_source(&theme, cx),
            Some(MainPage::EditSource(id)) => self.render_source_edit(&id, &theme, cx),
            Some(MainPage::Playlist(id)) => self.render_playlist_page(&id, &theme, cx),
            None => div().into_any_element(),
        }
    }

    // ================= 音源页 =================

    fn render_source_page(
        &self,
        id: &SourceId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(config) = self.source_config_by_id(&id.0) else {
            return page_scroll(
                theme,
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child("音源实例不存在")
                    .into_any_element(),
                PAGE_MAX_W,
                &self.main_page_scroll,
            );
        };
        let weak = cx.entity().downgrade();
        let (status_text, status_color) = status_line(self, config, id, cx);
        let enabled = config.enabled;
        let is_local = config.is_local();

        let back = {
            let weak = weak.clone();
            back_row(theme, "返回曲库", move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.close_main_page(cx));
            })
        };
        let actions = {
            let weak_edit = weak.clone();
            let id_edit = id.clone();
            let weak_toggle = weak.clone();
            let id_toggle = id.clone();
            let mut row = div().flex().items_center().gap(px(6.0));
            row = row.child(crate::widgets::text_button(
                theme,
                if enabled {
                    "关闭音源"
                } else {
                    "启用音源"
                },
                enabled,
                move |_, _, cx| {
                    let _ = weak_toggle.update(cx, |app, cx| {
                        app.set_source_enabled(&id_toggle, !enabled, cx);
                    });
                },
            ));
            row = row.child(crate::widgets::text_button(
                theme,
                "编辑配置",
                false,
                move |_, _, cx| {
                    let _ = weak_edit.update(cx, |app, cx| {
                        app.open_source_edit(id_edit.clone(), cx);
                    });
                },
            ));
            if is_local {
                let weak = weak.clone();
                let id = id.clone();
                row = row.child(crate::widgets::icon_button(
                    theme,
                    crate::icons::icon(crate::icons::Icon::Refresh, 13.0, theme.text_dim),
                    30.0,
                    "重新扫描",
                    move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            let local = cx.global::<Globals>().local_sources.get(&id).cloned();
                            if let Some(local) = local {
                                app.spawn_library_scan_source(local, cx);
                            }
                        });
                    },
                ));
            }
            row.into_any_element()
        };

        let hero = source_hero(
            theme,
            if is_local {
                crate::icons::Icon::Drive
            } else {
                crate::icons::Icon::Cloud
            },
            "音源实例",
            config.name.clone(),
            config.summary(),
            status_pill(theme, status_text, status_color),
            actions,
        );

        let body: AnyElement = if is_local {
            self.render_local_source_body(config, id, theme, cx)
        } else {
            self.render_netease_source_body(config, id, theme, cx)
        };

        let content = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(back)
            .child(hero)
            .child(body)
            .into_any_element();
        page_scroll(theme, content, PAGE_MAX_W, &self.main_page_scroll)
    }

    fn render_local_source_body(
        &self,
        config: &SourceInstanceConfig,
        id: &SourceId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let local = cx.global::<Globals>().local_sources.get(id).cloned();
        let stats = local
            .as_ref()
            .map(|local| local.stats())
            .unwrap_or_default();
        let roots = local
            .as_ref()
            .map(|local| local.roots())
            .unwrap_or_else(|| {
                config
                    .local
                    .as_ref()
                    .map(|local| local.roots.clone())
                    .unwrap_or_default()
            });
        let tracks = Arc::new(
            local
                .as_ref()
                .map(|local| local.all_tracks())
                .unwrap_or_default(),
        );
        let weak = cx.entity().downgrade();

        let stats_row = div()
            .flex()
            .gap(px(9.0))
            .mt(px(18.0))
            .child(stat_tile(
                theme,
                "曲目",
                stats.tracks.to_string(),
                "已索引入库",
                crate::icons::Icon::Note,
                theme.playing,
            ))
            .child(stat_tile(
                theme,
                "音乐人",
                stats.artists.to_string(),
                "扫描期派生",
                crate::icons::Icon::Person,
                theme.text,
            ))
            .child(stat_tile(
                theme,
                "专辑",
                stats.albums.to_string(),
                "同目录 / 标签聚合",
                crate::icons::Icon::Layers,
                theme.warn,
            ))
            .child(stat_tile(
                theme,
                "目录",
                stats.roots.to_string(),
                "音乐根目录",
                crate::icons::Icon::Drive,
                theme.ok,
            ))
            .into_any_element();

        let add_root = {
            let weak = weak.clone();
            let id_add = id.clone();
            crate::widgets::text_button(theme, "管理目录", false, move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| {
                    app.open_source_edit(id_add.clone(), cx);
                });
            })
        };

        let mut roots_rows = div().flex().flex_col().gap(px(2.0));
        if roots.is_empty() {
            roots_rows = roots_rows.child(
                div()
                    .py(px(16.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Drive,
                        14.0,
                        theme.text_faint,
                    ))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .child("还没有音乐目录，添加后即可扫描"),
                    ),
            );
        }
        for (index, root) in roots.iter().enumerate() {
            roots_rows = roots_rows.child(
                div()
                    .id(SharedString::from(format!("source-root-{index}")))
                    .h(px(38.0))
                    .px(px(6.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .hover(|s| s.bg(theme.hover))
                    .child(icon_badge(
                        theme,
                        crate::icons::Icon::Drive,
                        26.0,
                        theme.text_dim,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text)
                                    .text_ellipsis()
                                    .child(SharedString::from(root.display().to_string())),
                            )
                            .child(
                                div()
                                    .text_size(px(9.0))
                                    .text_color(theme.text_faint)
                                    .child("递归扫描 · 自动读取标签与封面"),
                            ),
                    )
                    .child(status_pill(theme, "已纳入", theme.ok)),
            );
        }

        let mut tracks_rows = div().flex().flex_col().gap(px(1.0));
        if !config.enabled {
            tracks_rows = tracks_rows.child(
                div()
                    .py(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Alert,
                        14.0,
                        theme.text_faint,
                    ))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .child("音源已关闭；启用后可查看曲库并播放。"),
                    ),
            );
        } else if tracks.is_empty() {
            tracks_rows = tracks_rows.child(
                div()
                    .py(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .child(crate::icons::icon(
                        crate::icons::Icon::Note,
                        14.0,
                        theme.text_faint,
                    ))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .child("还没有扫描到曲目，点右上角刷新重新扫描。"),
                    ),
            );
        } else {
            // 音源页曲目预览与完整队列共用同一行；行内 ⋮ 也能直接入队。
            let source_list = QueueList::Snapshot(tracks.clone());
            for (index, track) in tracks.iter().take(40).enumerate() {
                let row_text = TrackRowText::new(index, track);
                tracks_rows = tracks_rows.child(ChordialApp::render_track_row(
                    theme,
                    index,
                    &row_text,
                    &self.track_covers,
                    source_list.clone(),
                    self.queue_pos,
                    self.queue_pos
                        .and_then(|p| self.queue.get(p))
                        .map(|t| &t.id),
                    weak.clone(),
                    tracks.clone(),
                    false,
                    false,
                    false,
                    None,
                    None,
                    &TrackRowStyle::full(),
                    None,
                ));
            }
        }

        let play_all = {
            let weak = weak.clone();
            let snapshot = tracks.clone();
            crate::widgets::text_button(theme, "播放全部", true, move |_, _, cx| {
                let tracks = snapshot.clone();
                let _ = weak.update(cx, |app, cx| app.play_tracks(tracks, 0, cx));
            })
        };

        div()
            .flex()
            .flex_col()
            .child(stats_row)
            .child(
                div()
                    .mt(px(22.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .w(px(3.0))
                                    .h(px(13.0))
                                    .rounded(px(2.0))
                                    .bg(theme.text_dim),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.text)
                                    .child("音乐目录"),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(theme.text_faint)
                                    .child(format!("{} 个", roots.len())),
                            ),
                    )
                    .child(add_root),
            )
            .child(content_panel(theme, roots_rows))
            .child(
                div()
                    .mt(px(22.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .w(px(3.0))
                                    .h(px(13.0))
                                    .rounded(px(2.0))
                                    .bg(theme.text_dim),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.text)
                                    .child("曲库内容"),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(theme.text_faint)
                                    .child(format!("共 {} 首 · 预览前 40 首", stats.tracks)),
                            ),
                    )
                    .child(play_all),
            )
            .child(content_panel(theme, tracks_rows))
            .into_any_element()
    }

    /// 推荐入口点开后的完整详情：展示该数据源的全部条目，而不是铺在源页面里。
    fn render_feed_detail(
        &self,
        detail: &SourceFeedDetail,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let source = detail.source.clone();
        let kind = detail.kind;
        let back = {
            let weak = weak.clone();
            back_row(theme, "返回音源", move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.close_feed_detail(cx));
            })
        };
        let refresh = {
            let weak = weak.clone();
            let id_refresh = source.clone();
            crate::widgets::text_button(theme, "刷新", false, move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| {
                    app.refresh_source_feeds(&id_refresh, cx);
                });
            })
        };

        let feeds = self.source_feeds.get(&source);
        let (title, desc, cover_seed, accent): (&'static str, String, &'static str, gpui::Hsla) =
            match kind {
                SourceFeedKind::Daily => (
                    "今日推荐",
                    "根据你的收听习惯每日生成".to_string(),
                    "荐",
                    theme.playing,
                ),
                SourceFeedKind::Fm => (
                    "私人 FM",
                    "无限播放的个性化电台".to_string(),
                    "FM",
                    theme.ok,
                ),
                SourceFeedKind::Recommended => (
                    "每日推荐歌单",
                    "打开歌单后可订阅到我的歌单".to_string(),
                    "单",
                    theme.warn,
                ),
                SourceFeedKind::Radar => (
                    "私人雷达",
                    "按账号每日生成的发现歌单".to_string(),
                    "雷",
                    theme.text,
                ),
            };

        let detail_hero = div()
            .mt(px(12.0))
            .p(px(16.0))
            .rounded(px(12.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface_alt)
            .flex()
            .items_center()
            .gap(px(14.0))
            .child(cover_thumb(theme, cover_seed, 52.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(theme.text_faint)
                            .child(SharedString::from(desc)),
                    ),
            )
            .child(refresh);

        let list: AnyElement = match kind {
            SourceFeedKind::Daily | SourceFeedKind::Fm => {
                let (tracks, error, loading) = match feeds {
                    Some(feed) => match kind {
                        SourceFeedKind::Daily => (
                            feed.daily.as_slice(),
                            feed.daily_error.as_ref(),
                            feed.loading_daily,
                        ),
                        _ => (feed.fm.as_slice(), feed.fm_error.as_ref(), feed.loading_fm),
                    },
                    None => (&[][..], None, true),
                };
                let play_all = {
                    let weak = weak.clone();
                    let feed_list = QueueList::Feed {
                        source: source.clone(),
                        kind,
                    };
                    crate::widgets::text_button(theme, "播放全部", true, move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.queue_play_all(&feed_list, cx)
                        });
                    })
                };
                div()
                    .flex()
                    .flex_col()
                    .child(div().mt(px(10.0)).flex().justify_end().child(play_all))
                    .child(feed_track_section(
                        theme,
                        title,
                        format!("{} 首", tracks.len()),
                        tracks,
                        error,
                        loading,
                        "暂无内容",
                        usize::MAX,
                        &source,
                        kind,
                        &weak,
                        &self.track_covers,
                        self.queue_pos,
                        self.queue_pos
                            .and_then(|p| self.queue.get(p))
                            .map(|t| &t.id),
                        self.queue.clone(),
                    ))
                    .into_any_element()
            }
            SourceFeedKind::Recommended | SourceFeedKind::Radar => {
                let (collections, error, loading) = match feeds {
                    Some(feed) => match kind {
                        SourceFeedKind::Recommended => (
                            feed.recommended.as_slice(),
                            feed.recommended_error.as_ref(),
                            feed.loading_recommended,
                        ),
                        _ => (
                            feed.radar.as_slice(),
                            feed.radar_error.as_ref(),
                            feed.loading_radar,
                        ),
                    },
                    None => (&[][..], None, true),
                };
                content_panel(
                    theme,
                    feed_collection_section(
                        theme,
                        title,
                        format!("{} 个", collections.len()),
                        collections,
                        error,
                        loading,
                        "暂无内容",
                        usize::MAX,
                        &weak,
                    ),
                )
            }
        };

        page_scroll(
            theme,
            div()
                .flex()
                .flex_col()
                .child(back)
                .child(detail_hero)
                .child(list)
                .into_any_element(),
            PAGE_MAX_W,
            &self.main_page_scroll,
        )
    }

    fn render_netease_source_body(
        &self,
        config: &SourceInstanceConfig,
        id: &SourceId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 推荐入口详情：点开后在主内容区展示完整列表。
        if let Some(detail) = self.source_feed_detail.as_ref()
            && &detail.source == id
        {
            return self.render_feed_detail(detail, theme, cx);
        }
        let weak = cx.entity().downgrade();
        let netease = cx.global::<Globals>().netease_sources.get(id).cloned();
        let account = netease
            .as_ref()
            .and_then(|source| source.account().map(|account| account.account_state()))
            .unwrap_or(AccountState::Unknown);
        let enabled = config.enabled;

        let account_card: AnyElement = if !enabled {
            div()
                .mt(px(16.0))
                .p(px(14.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface_alt)
                .flex()
                .items_center()
                .gap(px(14.0))
                .child(icon_badge(
                    theme,
                    crate::icons::Icon::Cloud,
                    38.0,
                    theme.text_faint,
                ))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child("音源已关闭"),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(theme.text_faint)
                                .child("启用后账号、歌单与推荐继续使用本地保存的配置"),
                        ),
                )
                .into_any_element()
        } else {
            match account {
                AccountState::LoggedIn(profile) => {
                    let weak_logout = weak.clone();
                    let id_logout = id.clone();
                    let avatar = self.account_avatar.clone();
                    let playlist_count = self
                        .source_user_playlists
                        .get(id)
                        .map(|list| list.len())
                        .unwrap_or(0);
                    div()
                        .mt(px(18.0))
                        .p(px(16.0))
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface_alt)
                        .flex()
                        .items_center()
                        .gap(px(14.0))
                        .child(crate::widgets::cover_plain(
                            theme,
                            avatar,
                            46.0,
                            12.0,
                            icon_badge(theme, crate::icons::Icon::Person, 46.0, theme.text_dim),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .text_size(px(14.0))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(theme.text)
                                                .text_ellipsis()
                                                .child(SharedString::from(
                                                    profile.nickname.clone(),
                                                )),
                                        )
                                        .when(profile.vip, |row| {
                                            row.child(status_pill(theme, "VIP", theme.warn))
                                        }),
                                )
                                .child(
                                    div().text_size(px(9.5)).text_color(theme.text_faint).child(
                                        SharedString::from(format!(
                                            "UID {} · 已连接 api-enhanced",
                                            profile.uid
                                        )),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_end()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .text_size(px(17.0))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(theme.text)
                                        .child(playlist_count.to_string()),
                                )
                                .child(
                                    div()
                                        .text_size(px(9.0))
                                        .text_color(theme.text_faint)
                                        .child("账号歌单"),
                                ),
                        )
                        .child(crate::widgets::text_button(
                            theme,
                            "退出登录",
                            false,
                            move |_, _, cx| {
                                let _ = weak_logout.update(cx, |app, cx| {
                                    app.logout_netease_source(&id_logout, cx);
                                });
                            },
                        ))
                        .into_any_element()
                }
                AccountState::LoggedOut => {
                    let weak_login = weak.clone();
                    let id_login = id.clone();
                    div()
                        .mt(px(16.0))
                        .p(px(14.0))
                        .rounded(px(10.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface_alt)
                        .flex()
                        .items_center()
                        .gap(px(14.0))
                        .child(icon_badge(
                            theme,
                            crate::icons::Icon::Cloud,
                            38.0,
                            theme.text_dim,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(3.0))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .child("未登录网易云"),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(theme.text_faint)
                                        .child(
                                            "登录后可加载账号歌单、每日推荐、私人 FM 与私人雷达",
                                        ),
                                ),
                        )
                        .child(crate::widgets::text_button(
                            theme,
                            "登录网易云",
                            true,
                            move |_, _, cx| {
                                let _ = weak_login.update(cx, |app, cx| {
                                    app.open_login_for(id_login.clone(), cx);
                                });
                            },
                        ))
                        .into_any_element()
                }
                _ => {
                    let weak_refresh = weak.clone();
                    let id_refresh = id.clone();
                    div()
                        .mt(px(16.0))
                        .p(px(14.0))
                        .rounded(px(10.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface_alt)
                        .flex()
                        .items_center()
                        .gap(px(14.0))
                        .child(icon_badge(
                            theme,
                            crate::icons::Icon::Refresh,
                            38.0,
                            theme.text_dim,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(11.0))
                                .text_color(theme.text_dim)
                                .child("正在校验账号状态"),
                        )
                        .child(crate::widgets::text_button(
                            theme,
                            "重新校验",
                            false,
                            move |_, _, cx| {
                                let _ = weak_refresh.update(cx, |app, cx| {
                                    app.refresh_source_account(&id_refresh, cx);
                                });
                            },
                        ))
                        .into_any_element()
                }
            }
        };

        let playlists: AnyElement = {
            let cached = self.source_user_playlists.get(id).cloned();
            let loading = self.source_user_playlists_loading.contains(id);
            let mut block = div().flex().flex_col();
            if loading && cached.is_none() {
                block = block.child(
                    div()
                        .py(px(10.0))
                        .text_size(px(10.5))
                        .text_color(theme.text_faint)
                        .child("正在加载账号歌单…"),
                );
            } else if let Some(list) = cached {
                if list.is_empty() {
                    block = block.child(
                        div()
                            .py(px(10.0))
                            .text_size(px(10.5))
                            .text_color(theme.text_faint)
                            .child("账号还没有歌单"),
                    );
                }
                for playlist in list.iter().take(30) {
                    let weak = weak.clone();
                    let summary = playlist.clone();
                    block = block.child(
                        div()
                            .id(SharedString::from(format!(
                                "source-user-playlist-{}",
                                summary.id.id
                            )))
                            .h(px(48.0))
                            .px(px(6.0))
                            .rounded(px(7.0))
                            .flex()
                            .items_center()
                            .gap(px(11.0))
                            .hover(|s| s.bg(theme.hover))
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                let summary = summary.clone();
                                let _ = weak.update(cx, |app, cx| {
                                    app.page = None;
                                    app.open_collection(
                                        summary.id.clone(),
                                        summary.title.clone(),
                                        cx,
                                    );
                                });
                            })
                            .child(cover_thumb(theme, playlist.title.as_str(), 34.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.0))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.text)
                                            .text_ellipsis()
                                            .child(SharedString::from(playlist.title.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.0))
                                            .text_color(theme.text_faint)
                                            .text_ellipsis()
                                            .child(SharedString::from(
                                                playlist
                                                    .creator
                                                    .clone()
                                                    .filter(|creator| !creator.trim().is_empty())
                                                    .unwrap_or_else(|| "我的歌单".into()),
                                            )),
                                    ),
                            )
                            .child(
                                div().text_size(px(9.5)).text_color(theme.text_faint).child(
                                    SharedString::from(
                                        playlist
                                            .track_count
                                            .map(|count| count.to_string())
                                            .unwrap_or_default(),
                                    ),
                                ),
                            )
                            .when(playlist.subscribed, |row| {
                                row.child(status_pill(theme, "已订阅", theme.text_dim))
                            }),
                    );
                }
            } else {
                let weak = weak.clone();
                let id_load = id.clone();
                block = block.child(
                    div()
                        .py(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(10.5))
                                .text_color(theme.text_faint)
                                .child(if enabled {
                                    "未加载账号歌单"
                                } else {
                                    "音源已关闭；启用后可加载账号歌单"
                                }),
                        )
                        .child(crate::widgets::text_button(
                            theme,
                            "加载",
                            false,
                            move |_, _, cx| {
                                let _ = weak.update(cx, |app, cx| {
                                    app.load_source_user_playlists(&id_load, cx);
                                });
                            },
                        )),
                );
            }
            block.into_any_element()
        };

        let feeds = self.source_feeds.get(id);
        let mut content = div().flex().flex_col().child(account_card);
        if enabled {
            let loading = feeds.is_some_and(SourceFeedState::loading);
            let refresh = {
                let weak = weak.clone();
                let id_refresh = id.clone();
                crate::widgets::text_button(
                    theme,
                    if loading {
                        "刷新中…"
                    } else {
                        "刷新推荐"
                    },
                    false,
                    move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.refresh_source_feeds(&id_refresh, cx);
                        });
                    },
                )
            };
            content = content.child(
                div()
                    .h(px(30.0))
                    .mt(px(22.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .w(px(3.0))
                                    .h(px(13.0))
                                    .rounded(px(2.0))
                                    .bg(theme.text_dim),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.text)
                                    .child("为你推荐"),
                            ),
                    )
                    .child(refresh),
            );
            // 横向排列的推荐入口；完整列表只在点开卡片后的详情页显示。
            let (daily_count, fm_count, recommended_count, radar_count) = match feeds {
                Some(feed) => (
                    feed.daily.len(),
                    feed.fm.len(),
                    feed.recommended.len(),
                    feed.radar.len(),
                ),
                None => (0, 0, 0, 0),
            };
            let count_or_loading = |count: usize| {
                if feeds.is_none() && loading {
                    "加载中…".to_string()
                } else {
                    format!("{count} 首 · 查看详情")
                }
            };
            let collection_count = |count: usize| {
                if feeds.is_none() && loading {
                    "加载中…".to_string()
                } else {
                    format!("{count} 个 · 查看详情")
                }
            };
            let entries = div()
                .mt(px(10.0))
                .flex()
                .gap(px(10.0))
                .child(feed_entry_card(
                    theme,
                    "荐",
                    "今日推荐",
                    "每日专属曲目 · 点击查看完整列表".to_string(),
                    count_or_loading(daily_count),
                    theme.playing,
                    &weak,
                    id.clone(),
                    SourceFeedKind::Daily,
                ))
                .child(feed_entry_card(
                    theme,
                    "FM",
                    "私人 FM",
                    "无限播放的个性化电台".to_string(),
                    count_or_loading(fm_count),
                    theme.ok,
                    &weak,
                    id.clone(),
                    SourceFeedKind::Fm,
                ))
                .child(feed_entry_card(
                    theme,
                    "单",
                    "每日推荐歌单",
                    "按口味推荐 · 可订阅到我的歌单".to_string(),
                    collection_count(recommended_count),
                    theme.warn,
                    &weak,
                    id.clone(),
                    SourceFeedKind::Recommended,
                ))
                .child(feed_entry_card(
                    theme,
                    "雷",
                    "私人雷达",
                    "按账号每日生成的发现歌单".to_string(),
                    collection_count(radar_count),
                    theme.text,
                    &weak,
                    id.clone(),
                    SourceFeedKind::Radar,
                ));
            content = content.child(entries);
        }
        if enabled {
            content = content
                .child(section_title_v2(
                    theme,
                    "账号歌单",
                    Some("打开后可订阅到我的歌单".into()),
                ))
                .child(content_panel(theme, playlists));
        }
        content.into_any_element()
    }

    // ================= 管理音源 =================

    fn render_source_manager(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let back = {
            let weak = weak.clone();
            back_row(theme, "返回曲库", move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.close_main_page(cx));
            })
        };
        let add = {
            let weak = weak.clone();
            crate::widgets::text_button(theme, "添加音源", true, move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.open_new_source(cx));
            })
        };
        let header = page_header(
            theme,
            "音源".into(),
            "管理音源".into(),
            format!(
                "端点状态 · 配置 · 新增 · 共 {} 个实例 / {} 个开启",
                self.source_configs.len(),
                self.source_configs.iter().filter(|s| s.enabled).count()
            ),
            add,
        );

        let mut rows = div().flex().flex_col();
        for config in &self.source_configs {
            let id = SourceId::new(config.id.clone());
            let (status_text, status_color) = status_line(self, config, &id, cx);
            let weak_edit = weak.clone();
            let weak_switch = weak.clone();
            let id_edit = id.clone();
            let id_switch = id.clone();
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("manage-source-{}", config.id)))
                    .h(px(64.0))
                    .px(px(10.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(px(11.0))
                    .hover(|s| s.bg(theme.hover))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let _ = weak_edit.update(cx, |app, cx| {
                            app.open_source_edit(id_edit.clone(), cx);
                        });
                    })
                    .child(
                        div()
                            .size(px(32.0))
                            .rounded(px(8.0))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.surface_alt)
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(if config.enabled { 1.0 } else { 0.55 })
                            .child(crate::icons::icon(
                                if config.is_local() {
                                    crate::icons::Icon::Drive
                                } else {
                                    crate::icons::Icon::Cloud
                                },
                                15.0,
                                theme.text_dim,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(if config.enabled {
                                                theme.text
                                            } else {
                                                theme.text_faint
                                            })
                                            .child(SharedString::from(config.name.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.0))
                                            .px(px(4.0))
                                            .rounded(px(3.0))
                                            .bg(theme.surface_alt)
                                            .text_color(theme.text_faint)
                                            .child(config.type_label()),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(div().size(px(5.0)).rounded(px(3.0)).bg(status_color))
                                    .child(
                                        div()
                                            .text_size(px(9.5))
                                            .text_color(theme.text_faint)
                                            .text_ellipsis()
                                            .child(SharedString::from(status_text)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.5))
                                            .text_color(theme.text_faint)
                                            .text_ellipsis()
                                            .child(SharedString::from(config.summary())),
                                    ),
                            ),
                    )
                    .child(source_toggle(theme, config.enabled, id_switch, weak_switch))
                    .child(crate::icons::icon(
                        crate::icons::Icon::ChevronRight,
                        12.0,
                        theme.text_faint,
                    )),
            );
        }

        let content = div()
            .flex()
            .flex_col()
            .child(back)
            .child(div().h(px(6.0)))
            .child(header)
            .child(div().mt(px(14.0)).child(rows))
            .child(
                div()
                    .mt(px(12.0))
                    .text_size(px(10.0))
                    .line_height(px(16.0))
                    .text_color(theme.text_faint)
                    .child(
                        "每个实例都有独立的追踪 ID。删除或关闭实例不会删除我的歌单；\
                         歌单里引用该源的曲目会灰显，若本地库有同身份副本则自动改播本地。",
                    ),
            )
            .into_any_element();
        page_scroll(theme, content, PAGE_MAX_W, &self.main_page_scroll)
    }

    // ================= 新增音源 =================

    fn render_new_source(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let back = {
            let weak = weak.clone();
            back_row(theme, "返回音源管理", move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.open_source_manager(cx));
            })
        };
        let header = page_header(
            theme,
            "音源".into(),
            "添加音源".into(),
            "选择本程序已支持的数据源类型；新增实例会生成独立追踪 ID".into(),
            div().into_any_element(),
        );
        let option = |icon: crate::icons::Icon,
                      title: &'static str,
                      description: &'static str,
                      source_type: &'static str,
                      weak: gpui::WeakEntity<ChordialApp>| {
            div()
                .id(SharedString::from(format!("add-source-{source_type}")))
                .flex_1()
                .min_w(px(0.0))
                .p(px(18.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface_alt)
                .flex()
                .flex_col()
                .gap(px(8.0))
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover).border_color(theme.border_strong))
                .on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| {
                        app.create_source_of_type(source_type, cx);
                    });
                })
                .child(crate::icons::icon(icon, 22.0, theme.text_dim))
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .line_height(px(16.0))
                        .text_color(theme.text_faint)
                        .child(description),
                )
                .child(
                    div()
                        .mt(px(4.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .text_size(px(10.0))
                        .text_color(theme.text_dim)
                        .child("添加")
                        .child(crate::icons::icon(
                            crate::icons::Icon::ChevronRight,
                            10.0,
                            theme.text_faint,
                        )),
                )
                .into_any_element()
        };

        let content = div()
            .flex()
            .flex_col()
            .child(back)
            .child(div().h(px(6.0)))
            .child(header)
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .gap(px(12.0))
                    .child(option(
                        crate::icons::Icon::Drive,
                        "本地音乐",
                        "扫描一个或多个音乐目录；支持增量扫描、封面/歌词边车文件。",
                        crate::config::SOURCE_TYPE_LOCAL,
                        weak.clone(),
                    ))
                    .child(option(
                        crate::icons::Icon::Cloud,
                        "网易云音乐",
                        "连接自部署 api-enhanced；可登录账号、加载歌单与推荐。",
                        crate::config::SOURCE_TYPE_NETEASE,
                        weak,
                    )),
            )
            .child(
                div()
                    .mt(px(14.0))
                    .text_size(px(10.0))
                    .line_height(px(16.0))
                    .text_color(theme.text_faint)
                    .child(
                        "同一类型可以添加多个实例（例如两个不同音乐目录的本地库）。\
                         实例 ID 与曲目/歌单引用绑定，重命名不会改变引用。",
                    ),
            )
            .into_any_element();
        page_scroll(theme, content, PAGE_MAX_W, &self.main_page_scroll)
    }

    // ================= 二级编辑 =================

    fn render_source_edit(
        &self,
        id: &SourceId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(config) = self.source_config_by_id(&id.0) else {
            return div().into_any_element();
        };
        let weak = cx.entity().downgrade();
        let back = {
            let weak = weak.clone();
            back_row(theme, "返回音源管理", move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.open_source_manager(cx));
            })
        };
        let enabled = config.enabled;
        let header = page_header(
            theme,
            format!("{} · 编辑", config.type_label()),
            config.name.clone(),
            "实例 ID 固定不变；名称、目录或地址变更不会影响歌单引用".into(),
            div().into_any_element(),
        );
        let enabled_row = {
            let weak = weak.clone();
            let id = id.clone();
            div()
                .h(px(48.0))
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(theme.text)
                                .child("启用该音源"),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(theme.text_faint)
                                .child("关闭后保留配置与本地缓存，不停止已下载内容"),
                        ),
                )
                .child(source_toggle(theme, enabled, id, weak))
                .into_any_element()
        };
        let name_section = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(10.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.text_faint)
                    .child("名称"),
            )
            .child(self.source_edit_name_view.clone())
            .child(
                div()
                    .text_size(px(9.5))
                    .text_color(theme.text_faint)
                    .child("回车应用；只改展示名，不改追踪 ID"),
            )
            .into_any_element();

        let type_section: AnyElement = if config.is_local() {
            let roots = config
                .local
                .as_ref()
                .map(|local| local.roots.clone())
                .unwrap_or_default();
            let mut root_rows = div().flex().flex_col();
            for (index, root) in roots.iter().enumerate() {
                let weak = weak.clone();
                let id = id.clone();
                let can_remove = roots.len() > 1;
                root_rows = root_rows.child(
                    div()
                        .h(px(36.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .border_b_1()
                        .border_color(theme.border)
                        .child(crate::icons::icon(
                            crate::icons::Icon::Drive,
                            13.0,
                            theme.text_faint,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .text_size(px(10.5))
                                .text_color(theme.text_dim)
                                .text_ellipsis()
                                .child(SharedString::from(root.display().to_string())),
                        )
                        .when(can_remove, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("source-root-remove-{index}")))
                                    .h(px(22.0))
                                    .px(px(7.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .text_size(px(9.5))
                                    .text_color(theme.text_faint)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.hover).text_color(theme.danger))
                                    .child("移除")
                                    .on_click(move |_, _, cx| {
                                        let _ = weak.update(cx, |app, cx| {
                                            app.remove_source_root(&id, index, cx);
                                        });
                                    }),
                            )
                        }),
                );
            }
            div()
                .flex()
                .flex_col()
                .gap(px(7.0))
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_faint)
                        .child("音乐目录"),
                )
                .child(root_rows)
                .child(self.source_edit_root_view.clone())
                .child(
                    div()
                        .text_size(px(9.5))
                        .text_color(theme.text_faint)
                        .child("回车添加目录；保存会重建实例并重新扫描"),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(7.0))
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_faint)
                        .child("服务地址"),
                )
                .child(self.source_edit_url_view.clone())
                .child(
                    div()
                        .text_size(px(9.5))
                        .text_color(theme.text_faint)
                        .child("回车应用并重建实例；Cookie 由登录维护，不会显示在这里"),
                )
                .into_any_element()
        };

        let delete = {
            let weak = weak.clone();
            let id = id.clone();
            let confirming = self.source_delete_confirm.as_ref() == Some(&id);
            div()
                .mt(px(22.0))
                .pt(px(14.0))
                .border_t_1()
                .border_color(theme.border)
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.danger)
                                .child("删除音源实例"),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(theme.text_faint)
                                .child("歌单里的引用会保留并灰显；本地缓存不会被删除"),
                        ),
                )
                .child(
                    div()
                        .id("source-delete")
                        .h(px(28.0))
                        .px(px(10.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(if confirming {
                            theme.danger
                        } else {
                            theme.border
                        })
                        .flex()
                        .items_center()
                        .text_size(px(10.0))
                        .text_color(if confirming {
                            theme.danger
                        } else {
                            theme.text_dim
                        })
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.hover))
                        .child(if confirming {
                            "再次点击确认删除"
                        } else {
                            "删除音源"
                        })
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                if app.source_delete_confirm.as_ref() == Some(&id) {
                                    app.source_delete_confirm = None;
                                    app.delete_source_instance(&id, cx);
                                } else {
                                    app.source_delete_confirm = Some(id.clone());
                                    cx.notify();
                                }
                            });
                        }),
                )
                .into_any_element()
        };

        let content = div()
            .flex()
            .flex_col()
            .child(back)
            .child(div().h(px(6.0)))
            .child(header)
            .child(div().mt(px(10.0)).child(enabled_row))
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .flex_col()
                    .gap(px(18.0))
                    .child(name_section)
                    .child(type_section),
            )
            .child(delete)
            .into_any_element();
        page_scroll(theme, content, PAGE_MAX_W, &self.main_page_scroll)
    }

    // ================= 添加到歌单浮层 =================

    pub(super) fn render_playlist_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.playlist_picker.as_ref() else {
            return div().into_any_element();
        };
        let theme = Theme::get(cx).clone();
        let weak = cx.entity().downgrade();
        let width = self.viewport_width;
        let height = self.viewport_height;
        // 订阅歌单也列入：远端 AddTracks 由 PlaylistEditor 执行，
        // 跨源曲目会被音源自身过滤并给出提示。
        let local: Vec<_> = self.playlists.summaries();
        let panel_w = 248.0;
        let rows_h = (local.len().min(8) as f32) * 32.0;
        let panel_h = (74.0 + rows_h + 68.0).min(380.0);
        let x = picker.x.min((width - panel_w - 8.0).max(8.0)).max(8.0);
        let y = picker.y.min((height - panel_h - 12.0).max(44.0)).max(44.0);

        let item_label =
            |icon: crate::icons::Icon,
             label: SharedString,
             meta: String,
             on_click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>| {
                div()
                    .id(SharedString::from(format!("picker-{label}")))
                    .h(px(32.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover))
                    .child(crate::icons::icon(icon, 13.0, theme.text_faint))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(11.5))
                            .text_color(theme.text)
                            .text_ellipsis()
                            .child(label),
                    )
                    .children((!meta.is_empty()).then(|| {
                        div()
                            .text_size(px(9.5))
                            .text_color(theme.text_faint)
                            .child(SharedString::from(meta))
                    }))
                    .on_click(on_click)
                    .into_any_element()
            };

        let mut rows: Vec<AnyElement> = Vec::new();
        {
            let weak = weak.clone();
            rows.push(item_label(
                crate::icons::Icon::Plus,
                "新建歌单并添加".into(),
                String::new(),
                Box::new(move |_, _, cx| {
                    let _ = weak.update(cx, |app, cx| app.create_playlist_from_picker(cx));
                }),
            ));
        }
        for summary in &local {
            let weak = weak.clone();
            let id = summary.id.clone();
            rows.push(item_label(
                if summary.is_subscribed() {
                    crate::icons::Icon::Cloud
                } else {
                    crate::icons::Icon::Layers
                },
                summary.title.clone().into(),
                summary.count.to_string(),
                Box::new(move |_, _, cx| {
                    let id = id.clone();
                    let _ = weak.update(cx, |app, cx| {
                        app.add_picker_track_to_playlist(&id, cx);
                    });
                }),
            ));
        }

        div()
            .id("playlist-picker-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, _, _, cx| app.close_playlist_picker(cx)),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|app, _, _, cx| app.close_playlist_picker(cx)),
            )
            .child(
                div()
                    .id("playlist-picker-panel")
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(panel_w))
                    .rounded(px(12.0))
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(vec![BoxShadow {
                        color: gpui::black().opacity(0.42),
                        blur_radius: px(28.0),
                        spread_radius: px(0.0),
                        offset: point(px(0.0), px(10.0)),
                    }])
                    .p(px(6.0))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px(px(8.0))
                            .py(px(6.0))
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child("添加到歌单"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(theme.text_faint)
                                    .text_ellipsis()
                                    .child(SharedString::from(picker.track.title.clone())),
                            ),
                    )
                    .child(div().h(px(1.0)).mx(px(4.0)).my(px(4.0)).bg(theme.border))
                    .child(
                        div()
                            .id("playlist-picker-rows")
                            .max_h(px(rows_h.max(64.0)))
                            .overflow_y_scroll()
                            .children(rows),
                    )
                    .child(
                        div()
                            .id("picker-cancel")
                            .h(px(30.0))
                            .px(px(8.0))
                            .rounded(px(6.0))
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.hover))
                            .child(crate::icons::icon(
                                crate::icons::Icon::Close,
                                12.0,
                                theme.text_faint,
                            ))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_faint)
                                    .child("取消"),
                            )
                            .on_click({
                                let weak = weak.clone();
                                move |_, _, cx| {
                                    let _ =
                                        weak.update(cx, |app, cx| app.close_playlist_picker(cx));
                                }
                            }),
                    ),
            )
            .into_any_element()
    }

    // ================= 歌单页 =================

    fn render_playlist_page(
        &self,
        id: &chordal_core::playlist::PlaylistId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(playlist) = self.playlists.get(id) else {
            return page_scroll(
                theme,
                div().child("歌单不存在").into_any_element(),
                PAGE_MAX_W,
                &self.main_page_scroll,
            );
        };
        let weak = cx.entity().downgrade();
        // 订阅歌单只要源实现了 PlaylistEditor 就同样可编辑（网易云支持）。
        let caps = self.playlist_capabilities(playlist);
        let editable = caps.remove_tracks && caps.reorder;
        let subscribed = playlist.kind.is_subscribed();
        let kind_label = if subscribed {
            "订阅歌单"
        } else {
            "本地歌单"
        };
        let rows = self.playlist_rows(playlist, cx);
        let count = playlist.entries.len();
        let source_label = playlist
            .kind
            .subscribed_collection()
            .and_then(|collection| self.source_config_by_id(&collection.source.0))
            .map(|source| source.name.clone())
            .unwrap_or_else(|| "本地".into());
        let subtitle = if subscribed {
            format!(
                "{count} 首 · 来自 {source_label} · {}",
                relative_time(match &playlist.kind {
                    PlaylistKind::Subscribed { last_synced_ms, .. } => *last_synced_ms,
                    PlaylistKind::Local => 0,
                })
            )
        } else {
            format!("{count} 首 · 保存在本地")
        };

        let back = {
            let weak = weak.clone();
            back_row(theme, "返回曲库", move |_, _, cx| {
                let _ = weak.update(cx, |app, cx| app.close_main_page(cx));
            })
        };
        let actions = {
            let mut row = div().flex().items_center().gap(px(6.0));
            {
                let weak = weak.clone();
                let id = id.clone();
                row = row.child(crate::widgets::text_button(
                    theme,
                    "播放全部",
                    true,
                    move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| app.play_playlist(&id, cx));
                    },
                ));
            }
            if caps.add_tracks {
                let weak = weak.clone();
                let id = id.clone();
                row = row.child(crate::widgets::text_button(
                    theme,
                    "从当前队列添加",
                    false,
                    move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.add_queue_to_playlist(&id, cx);
                        });
                    },
                ));
            }
            {
                let weak = weak.clone();
                let id = id.clone();
                let confirming = self.playlist_delete_confirm.as_ref() == Some(&id);
                row = row.child(
                    div()
                        .id("playlist-delete")
                        .h(px(29.0))
                        .px(px(9.0))
                        .rounded(px(7.0))
                        .flex()
                        .items_center()
                        .text_size(px(10.5))
                        .text_color(if confirming {
                            theme.danger
                        } else {
                            theme.text_faint
                        })
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.hover).text_color(theme.danger))
                        .child(if confirming {
                            "再次点击删除"
                        } else {
                            "删除歌单"
                        })
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |app, cx| {
                                if app.playlist_delete_confirm.as_ref() == Some(&id) {
                                    app.playlist_delete_confirm = None;
                                    app.delete_playlist(&id, cx);
                                } else {
                                    app.playlist_delete_confirm = Some(id.clone());
                                    cx.notify();
                                }
                            });
                        }),
                );
            }
            row.into_any_element()
        };

        let header = page_header(
            theme,
            kind_label.into(),
            playlist.title.clone(),
            subtitle,
            actions,
        );

        let rename_row = div()
            .w(px(360.0))
            .max_w_full()
            .child(self.source_playlist_rename_view.clone())
            .into_any_element();

        let subscription_notice: AnyElement = if subscribed {
            let weak = weak.clone();
            let id = id.clone();
            div()
                .mt(px(10.0))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .p(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface_alt)
                .child(
                    div()
                        .text_size(px(10.0))
                        .line_height(px(15.0))
                        .text_color(theme.text_faint)
                        .child(
                            "打开时先显示本地缓存，并在后台向来源校验更新；来源关闭时只读本地。",
                        ),
                )
                .child(crate::widgets::text_button(
                    theme,
                    "立即同步",
                    false,
                    move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            app.refresh_playlist_subscription(&id, cx);
                            app.info_message = Some("正在后台同步订阅歌单…".into());
                        });
                    },
                ))
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let list_theme = theme.clone();
        let list_weak = weak.clone();
        let list_rows = rows.clone();
        let playlist_id = id.clone();
        let list_ref = QueueList::Playlist(id.clone());
        let tracks_snapshot: Arc<Vec<Track>> =
            Arc::new(rows.iter().map(|row| row.track.clone()).collect());
        let covers = self.track_covers.clone();
        let queue_pos = self.queue_pos;
        let current_id = queue_pos
            .and_then(|pos| self.queue.get(pos))
            .map(|t| t.id.clone());
        let mut list = uniform_list("playlist-tracks", count, move |range, _window, _cx| {
            range
                .filter_map(|index| {
                    let row = list_rows.get(index)?;
                    let row_text = TrackRowText::new(index, &row.track);
                    let style = TrackRowStyle {
                        source_label: Some(row.source_label.clone()),
                        unavailable: row.unavailable,
                        local_substitute: row.local_substitute,
                        ..TrackRowStyle::full()
                    };
                    // 歌单编辑按钮作为行尾随操作交给统一行渲染器。
                    let trailing = editable.then(|| {
                        let weak_remove = list_weak.clone();
                        let weak_up = list_weak.clone();
                        let weak_down = list_weak.clone();
                        let playlist_id_remove = playlist_id.clone();
                        let playlist_id_up = playlist_id.clone();
                        let playlist_id_down = playlist_id.clone();
                        let track_remove = row.track.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .id(gpui::ElementId::named_usize("playlist-up", index))
                                    .size(px(22.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .opacity(0.55)
                                    .hover(|s| s.bg(list_theme.surface_alt).opacity(1.0))
                                    .child(crate::icons::icon(
                                        crate::icons::Icon::ChevronUp,
                                        12.0,
                                        list_theme.text_faint,
                                    ))
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        if index == 0 {
                                            return;
                                        }
                                        let _ = weak_up.update(cx, |app, cx| {
                                            app.reorder_playlist(&playlist_id_up, index, index - 1, cx);
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .id(gpui::ElementId::named_usize("playlist-down", index))
                                    .size(px(22.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .opacity(0.55)
                                    .hover(|s| s.bg(list_theme.surface_alt).opacity(1.0))
                                    .child(crate::icons::icon(
                                        crate::icons::Icon::ChevronDown,
                                        12.0,
                                        list_theme.text_faint,
                                    ))
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        let _ = weak_down.update(cx, |app, cx| {
                                            app.reorder_playlist(&playlist_id_down, index, index + 1, cx);
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .id(gpui::ElementId::named_usize("playlist-remove", index))
                                    .size(px(22.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .opacity(0.55)
                                    .hover(|s| s.bg(list_theme.surface_alt).opacity(1.0))
                                    .child(crate::icons::icon(
                                        crate::icons::Icon::Close,
                                        11.0,
                                        list_theme.text_faint,
                                    ))
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        let _ = weak_remove.update(cx, |app, cx| {
                                            app.apply_playlist_mutation(
                                                &playlist_id_remove,
                                                PlaylistMutation::RemoveTracks {
                                                    tracks: vec![track_remove.clone()],
                                                },
                                                cx,
                                            );
                                        });
                                    }),
                            )
                            .into_any_element()
                    });
                    Some(
                        div()
                            .w_full()
                            .px(px(30.0))
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(PAGE_MAX_W))
                                    .mx_auto()
                                    .child(ChordialApp::render_track_row(
                                        &list_theme,
                                        index,
                                        &row_text,
                                        &covers,
                                        list_ref.clone(),
                                        queue_pos,
                                        current_id.as_ref(),
                                        list_weak.clone(),
                                        tracks_snapshot.clone(),
                                        false,
                                        false,
                                        false,
                                        None,
                                        None,
                                        &style,
                                        trailing,
                                    )),
                            )
                            .into_any_element(),
                    )
                })
                .collect::<Vec<_>>()
        });
        // 滚动偏移存在 App 句柄里：沉浸层卸载歌单页后，回来仍停在原位置。
        list = list.track_scroll(self.playlist_scroll.clone());
        if self.smooth_scroll {
            list = list.smooth_scroll();
        }

        div()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .bg(theme.surface)
            .flex()
            .flex_col()
            .child(
                div().flex_none().child(
                    div()
                        .w_full()
                        .max_w(px(PAGE_MAX_W))
                        .mx_auto()
                        .px(px(30.0))
                        .pt(px(22.0))
                        .pb(px(10.0))
                        .child(back)
                        .child(div().h(px(6.0)))
                        .child(header)
                        .child(div().mt(px(10.0)).child(rename_row))
                        .child(subscription_notice),
                ),
            )
            .child(list.flex_1().min_h_0())
            .child(div().h(px(92.0)).flex_none())
            .into_any_element()
    }
}
