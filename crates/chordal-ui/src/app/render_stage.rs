//! `ChordialApp` 的 render_stage 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;
use crate::stage_chrome::{StageChromeAnchor, StageChromeGroup, StageChromeVisual};

/// 静态控件几何：进度、传输、音量保持完整布局里的设计位置。
/// 闲置收起只改变显隐滤镜与整体平移，不重新分配行间距。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct StageControlsLayout {
    progress: StageRect,
    transport: StageRect,
    volume: StageRect,
    bottom_row: StageRect,
    group: StageRect,
}

impl StageControlsLayout {
    #[cfg(test)]
    fn contains(&self, rect: StageRect) -> bool {
        let group = self.group;
        rect.x >= group.x
            && rect.y >= group.y
            && rect.x + rect.w <= group.x + group.w
            && rect.y + rect.h <= group.y + group.h
    }

    /// 闲置居中：整组只做垂直刚体平移，行与行之间的间距保持不变。
    fn translated_y(self, dy: f32) -> Self {
        Self {
            progress: self.progress.translated(0.0, dy),
            transport: self.transport.translated(0.0, dy),
            volume: self.volume.translated(0.0, dy),
            bottom_row: self.bottom_row.translated(0.0, dy),
            group: self.group.translated(0.0, dy),
        }
    }
}

/// 由固定行高与剩余空间计算控件位置；滤镜与闲置居中只作用于渲染偏移。
///
/// `volume_h` 为 0 表示矮窗里音量行被收起：此时 `gap_count` 也少一道，
/// 空出的高度按剩余行重新均分，而不是在传输行下方留一条空洞。
#[allow(clippy::too_many_arguments)]
pub(super) fn stage_controls_layout(
    controls_top: f32,
    controls_h: f32,
    leading_h: f32,
    gap_count: u32,
    column_x: f32,
    column_w: f32,
    button: f32,
    volume_h: f32,
    bottom_rect: StageRect,
) -> StageControlsLayout {
    let gap = if gap_count == 0 {
        0.0
    } else {
        ((controls_h - leading_h - STAGE_PROGRESS_H - button - volume_h) / gap_count as f32)
            .max(0.0)
    };
    let progress = StageRect {
        x: column_x,
        y: controls_top + leading_h + gap,
        w: column_w,
        h: STAGE_PROGRESS_H,
    };
    let transport = StageRect {
        x: column_x,
        y: progress.y + progress.h + gap,
        w: column_w,
        h: button,
    };
    let volume = StageRect {
        x: column_x,
        y: transport.y + transport.h + gap,
        w: column_w,
        h: volume_h,
    };
    let group_x = column_x.min(bottom_rect.x);
    let group_right = (column_x + column_w).max(bottom_rect.x + bottom_rect.w);
    StageControlsLayout {
        progress,
        transport,
        volume,
        bottom_row: bottom_rect,
        group: StageRect {
            x: group_x,
            y: progress.y,
            w: group_right - group_x,
            h: bottom_rect.y + bottom_rect.h - progress.y,
        },
    }
}

/// 竖屏封面态（无歌词 / 无侧栏）的底部控件几何。
///
/// `render_compact_art` 据此绘制控件，`render_immersive_compact` 据此计算
/// 顶部把手与整组闲置居中的位移；两处必须共用同一份公式，否则把手会与
/// 进度条脱节。
fn compact_art_controls_layout(
    width: f32,
    height: f32,
    viewport_width: f32,
    left: f32,
    cover: f32,
) -> StageControlsLayout {
    // 窄布局的底栏不使用 lyric_pad，传 0 与渲染路径等价。
    let bottom_rect = stage_bottom_rect(width, height, false, 0.0);
    let art_bottom = StageRect {
        x: left,
        y: bottom_rect.y,
        w: cover,
        h: bottom_rect.h,
    };
    let controls_top = COMPACT_TOP + cover + COMPACT_COVER_GAP;
    let controls_h = (art_bottom.y - controls_top).max(0.0);
    let volume_h = if stage_volume_visible(height) {
        STAGE_VOLUME_H
    } else {
        0.0
    };
    let gap_count = if volume_h > 0.0 { 4 } else { 3 };
    stage_controls_layout(
        controls_top,
        controls_h,
        amll_info_height(viewport_width, height),
        gap_count,
        left,
        cover,
        stage_button_size(cover),
        volume_h,
        art_bottom,
    )
}

/// 竖屏顶部小白条收起态的顶边。闲置居中时把手与播放列一起下移。
const COMPACT_PILL_TOP: f32 = 20.0;

fn stage_chrome_bottom_group(
    rect: StageRect,
    visual: StageChromeVisual,
    child: AnyElement,
) -> AnyElement {
    div()
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.w))
        .h(px(rect.h))
        .child(
            StageChromeGroup::new(
                div().size_full().child(child),
                visual,
                StageChromeAnchor::BottomCenter,
            )
            .into_any_element(),
        )
        .into_any_element()
}

/// 竖屏「显示歌词」的 AMLL `VerticalLayout` 等价几何。
///
/// 顶栏、歌词正文与底部控件共用同一条内容列：歌词正文两侧各有 1em
/// padding（字号即 padding），顶栏与控件也按同一字号内缩，因此三者的
/// 左右边界严格重合，不会再出现「封面贴左、控件居中」的双对齐线。
/// 底部依次是信息、进度、传输、音量、底栏；四道间距均分剩余空间，
/// 并保留最小 lyric 带，不让控件贴成一团。所有尺寸只依赖窗口大小，
/// 闲置时传输/音量/底栏只走滤镜淡出；歌词带夹在顶栏与控件之间，
/// 不参与封面态的整组居中平移，避免顶栏压进歌词。
struct VerticalLyricsGeometry {
    content_w: f32,
    left: f32,
    cover_side: f32,
    header_left: f32,
    header_top: f32,
    header_w: f32,
    info_w: f32,
    info_font: f32,
    label_font: f32,
    btn: f32,
    volume_h: f32,
    gap_count: u32,
    controls_h: f32,
    controls_top: f32,
    lyric_top: f32,
    lyric_bottom: f32,
}

fn vertical_lyrics_geometry(width: f32, height: f32) -> VerticalLyricsGeometry {
    let em = 16.0_f32;
    // 歌词正文字号 × 1em 决定其左右 padding；顶栏与控件都从这里起算。
    let lyric_pad = immersive_lyric_padding(width, height).min(width * 0.5);
    let left = lyric_pad.max(0.0);
    let content_w = (width - left * 2.0).max(1.0);
    // AMLL 竖向布局：小封面 6em，宽 ≤ 480 收到 4.5em；x 与歌词正文对齐。
    let cover_side =
        (if width <= 480.0 { em * 4.5 } else { em * 6.0 }).min((content_w * 0.5).max(24.0));
    // drag 30 / thumb 30 / gap 2；封面再从主区顶部下移 1em。
    let header_top = 30.0 + 30.0 + 2.0 + em;
    let header_left = left;
    let header_w = content_w;
    let info_w = (header_w - cover_side - em).max(1.0);
    let info_font = (height * 0.02).clamp(14.0, 20.0);
    let label_font = (height * 0.012).max(12.8);
    // 竖屏控件列在强制竖屏的横向窗口里可以很宽；传输键不必跟着整列
    // 放大到 88px，否则短窗里传输行会把歌词带和底栏间距一起吃掉。
    let btn = (content_w * 0.18).clamp(34.0, 64.0);
    let header_bottom = header_top + cover_side;
    let bottom_y =
        height - crate::layout_morph::COMPACT_BOTTOM - crate::layout_morph::COMPACT_DOCK_H;
    // 矮窗收起音量行，少一行 + 一道间距；剩余行之间仍等宽均分。
    let volume_h = if stage_volume_visible(height) {
        STAGE_VOLUME_H
    } else {
        0.0
    };
    let gap_count = if volume_h > 0.0 { 4 } else { 3 };
    let gaps = gap_count as f32;
    // 信息 / 进度 / 传输 / 音量 / 底栏之间等宽 gap。剩余空间按 30%
    // 分给间距、70% 留给歌词；同时给 lyric 带一个下限，短窗里 gap 会
    // 自动收缩，而不是把控件压到 0 间距。
    let rows = STAGE_PROGRESS_H + btn + volume_h;
    let free = (bottom_y - header_bottom - rows - em).max(0.0);
    let min_band = (height * 0.16).max(64.0);
    let max_gap = ((bottom_y - header_bottom - rows - em - min_band) / gaps).max(2.0);
    let gap = ((free * 0.30) / gaps)
        .clamp(6.0, 24.0)
        .min(max_gap)
        .min((free / gaps).max(2.0));
    let controls_top = (bottom_y - rows - gap * gaps).max(header_bottom + 8.0);
    let controls_h = (bottom_y - controls_top).max(0.0);
    let lyric_top = header_bottom + 8.0;
    let lyric_bottom = (controls_top - 8.0).max(lyric_top + 1.0);
    VerticalLyricsGeometry {
        content_w,
        left,
        cover_side,
        header_left,
        header_top,
        header_w,
        info_w,
        info_font,
        label_font,
        btn,
        volume_h,
        gap_count,
        controls_h,
        controls_top,
        lyric_top,
        lyric_bottom,
    }
}

/// 竖屏歌词底部 `mask-image` 的渐隐高度（相对歌词容器高度）。
/// 进度条在闲置态也保留，因此遮罩不再随控件显隐倍率归零。
pub(super) fn compact_lyric_bottom_mask(width: f32, height: f32) -> f32 {
    let g = vertical_lyrics_geometry(width, height);
    ((height - g.controls_top + 8.0) / height.max(1.0)).clamp(0.0, 1.0)
}

/// 竖屏歌词的底部控件几何。
///
/// 渲染路径（`amll_vertical_lyrics_column`）与闲置态的「歌词唤醒捕获层」
/// 必须共用同一份进度条矩形：进度条在闲置态保持可拖动，捕获层要在它上面
/// 留出空洞，不能把第一次点击吃掉。
fn vertical_lyrics_controls_layout(
    width: f32,
    height: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> (VerticalLyricsGeometry, StageControlsLayout) {
    let g = vertical_lyrics_geometry(width, height);
    let base_bottom = stage_bottom_rect(
        width,
        height,
        false,
        stable_lyric_pad(viewport_width, viewport_height),
    );
    let bottom_rect = StageRect {
        x: g.left,
        y: base_bottom.y,
        w: g.content_w,
        h: base_bottom.h,
    };
    let layout = stage_controls_layout(
        g.controls_top,
        g.controls_h,
        0.0,
        g.gap_count,
        g.left,
        g.content_w,
        g.btn,
        g.volume_h,
        bottom_rect,
    );
    (g, layout)
}

impl ChordialApp {
    /// 沉浸层统一的曲名文案。共享元素与页面占位共用同一份文本，
    /// 保证补间结束交还静态布局时不会发生文字替换。
    pub(super) fn current_title_text(&self) -> String {
        self.current_track
            .as_ref()
            .map(|t| t.title.clone())
            .or_else(|| self.player_status.title.clone())
            .unwrap_or_else(|| "未在播放".into())
    }

    /// 沉浸层统一的艺人文案（AMLL 语义：`A, B & C`，无艺人时回退专辑）。
    pub(super) fn current_artist_text(&self) -> String {
        if matches!(self.player_status.state, PlayerState::Failed(_)) {
            return "播放失败 · 点击播放键重试".into();
        }
        if self.player_status.state == PlayerState::Loading {
            return "正在加载…".into();
        }
        match self.current_track.as_ref() {
            Some(track) => {
                let names: Vec<&str> = track
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .filter(|n| !n.is_empty())
                    .collect();
                match names.len() {
                    0 => track
                        .album
                        .as_ref()
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| "未知艺人".into()),
                    1 => names[0].to_string(),
                    n => format!("{} & {}", names[..n - 1].join(", "), names[n - 1]),
                }
            }
            None => "选择一首歌开始".into(),
        }
    }

    /// 只有曲目信息与右侧圆钮（封面态放在大封面下方）。
    pub(super) fn amll_info_row(
        &self,
        width: f32,
        font: f32,
        height: f32,
        back: bool,
        interactive: bool,
        hide_text: bool,
        visual: StageChromeVisual,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let title = self.current_title_text();
        let artist = self.current_artist_text();
        let failed = matches!(self.player_status.state, PlayerState::Failed(_));
        // AMLL MenuButton：width 3.5vh；窄屏改用 max(2em, min(5vw, 4vh))。
        // 与共享舞台 info 宽度共用同一公式，交还静态布局那一帧不会跳字。
        let action_side = info_action_side(self.viewport_width, height);
        let action = {
            let trigger = div()
                .id("amll-info-action")
                .size(px(action_side))
                .rounded(px(action_side * 0.5))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .bg(white.opacity(0.082))
                .child(crate::icons::icon(
                    if back {
                        crate::icons::Icon::ChevronLeft
                    } else {
                        crate::icons::Icon::MoreDot
                    },
                    action_side * 0.72,
                    white,
                ))
                .into_any_element();
            if interactive && !self.stage_chrome_hidden {
                let weak = weak.clone();
                crate::bubble::bubble_button(
                    crate::bubble::BubbleStyle::on_dark(Theme::get(cx).font_family.clone()),
                    "amll-info-action",
                    if back {
                        "返回播放页"
                    } else {
                        "更多播放选项"
                    },
                    trigger,
                    move |_, _, cx| {
                        let _ = weak.update(cx, |app, cx| {
                            if back {
                                app.set_portrait_page(PortraitPage::Art, cx);
                            } else {
                                app.open_drawer(cx);
                            }
                        });
                    },
                )
            } else {
                trigger
            }
        };
        // 动作按钮拥有固定槽位与固定间距；隐藏只过滤按钮本身，文案不重排。
        let action = div()
            .ml(px(INFO_ACTION_GAP))
            .w(px(action_side))
            .h(px(action_side))
            .flex_none()
            .child(StageChromeGroup::new(
                action,
                visual,
                StageChromeAnchor::TopRight,
            ));
        let available = info_text_width(width, self.viewport_width, height);
        let title_w = measure_text_width(window, &title, font, gpui::FontWeight::MEDIUM);
        let artist_w = measure_text_width(window, &artist, font, gpui::FontWeight::NORMAL);
        let now = Instant::now();
        let marquee = self.info_marquee;
        let make_line = |line: u8,
                         text: String,
                         measured: f32,
                         weight: gpui::FontWeight,
                         color: gpui::Hsla|
         -> AnyElement {
            // AMLL TextMarquee：只有当文字宽度超过容器 95% 时，悬停才滚动。
            let overflowing = measured > available * 0.95;
            let distance = (measured - available * 0.95).max(0.0);
            let duration_ms = (distance * 2.0 / 64.0 * 1000.0).max(1.0);
            let active = marquee.filter(|m| m.line == line);
            let animating = active.is_some_and(|m| m.active(now));
            let offset = active.map(|m| m.offset(now)).unwrap_or(0.0);
            let hover_weak = weak.clone();
            let mut outer = div()
                .id(if line == 0 {
                    "info-line-title"
                } else {
                    "info-line-artist"
                })
                .w_full()
                .overflow_hidden()
                .on_hover(move |hovered, _, cx| {
                    if !overflowing {
                        return;
                    }
                    let _ = hover_weak.update(cx, |app, cx| {
                        if *hovered {
                            app.info_marquee = Some(InfoMarquee {
                                line,
                                start: Instant::now(),
                                distance,
                                duration_ms,
                            });
                        } else if app.info_marquee.is_some_and(|m| m.line == line) {
                            app.info_marquee = None;
                        }
                        cx.notify();
                    });
                });
            if animating {
                outer = outer.child(
                    div()
                        .w(px(measured + 2.0))
                        .ml(px(-offset))
                        .text_size(px(font))
                        .line_height(px(font * INFO_LINE_RATIO))
                        .font_weight(weight)
                        .text_color(color)
                        .whitespace_nowrap()
                        .child(SharedString::from(text)),
                );
            } else {
                outer = outer.child(
                    div()
                        .w_full()
                        .text_size(px(font))
                        .line_height(px(font * INFO_LINE_RATIO))
                        .font_weight(weight)
                        .text_color(color)
                        .text_ellipsis()
                        .child(SharedString::from(text)),
                );
            }
            outer.into_any_element()
        };
        let mode = self.stage_measurement_mode(cx);
        self.stage_measurements
            .observe(
                mode,
                div()
                    .w(px(width))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .opacity(if hide_text { 0.0 } else { 1.0 })
                            .child(make_line(
                                0,
                                title,
                                title_w,
                                gpui::FontWeight::MEDIUM,
                                white.opacity(0.9),
                            ))
                            .child(make_line(
                                1,
                                artist,
                                artist_w,
                                gpui::FontWeight::NORMAL,
                                if failed {
                                    Theme::get(cx).danger
                                } else {
                                    white.opacity(0.45)
                                },
                            )),
                    )
                    .child(action),
                &[(0, MeasurePart::Info)],
            )
            .into_any_element()
    }

    /// 静态封面 / 信息列；进度、传输、音量与底栏由独立的底部滤镜组绘制。
    ///
    /// 封面、信息与顶部动作共用同一个列宽；歌词关闭/无歌词时由调用方
    /// 把这整列水平居中。控件本身不再随显隐改变高度或位置。
    ///
    /// `cover_drag` 为 true 时，整块封面和顶部小白条一样响应下拉关闭手势
    /// （手机壳播放页有自己的横滑/点击手势，传 false）。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn amll_player_column(
        &self,
        column_w: f32,
        height: f32,
        menu_interactive: bool,
        visual: StageChromeVisual,
        shared_hidden: bool,
        cover_drag: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let theme = Theme::dark();
        // 专辑封面统一 1px 圆角，保留 AMLL 的大投影与暂停缩放。
        let cover_scale = self.cover_scale_now(Instant::now()).clamp(0.5, 1.1);
        let cover_side = column_w * cover_scale;
        let cover_radius = 1.0;
        let mode = self.stage_measurement_mode(cx);
        let cover = self.stage_measurements.observe(
            mode,
            div()
                .w(px(column_w))
                .h(px(column_w))
                .flex()
                .items_center()
                .justify_center()
                .when(cover_drag, |cover| {
                    cover.cursor(CursorStyle::OpenHand).occlude().on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|app, event: &MouseDownEvent, _, cx| {
                            app.begin_sheet_drag(f32::from(event.position.y), cx);
                        }),
                    )
                })
                .child(
                    div()
                        .size(px(cover_side))
                        .rounded(px(cover_radius))
                        .shadow(vec![BoxShadow {
                            color: gpui::black().opacity(0.19),
                            offset: point(px(0.0), px(16.0 * cover_scale)),
                            blur_radius: px(19.2 * cover_scale),
                            spread_radius: px(0.0),
                        }])
                        .child(crate::widgets::cover_plain(
                            &theme,
                            self.cover_image.clone(),
                            cover_side,
                            cover_radius,
                            crate::icons::icon(
                                crate::icons::Icon::Note,
                                (cover_side * 0.1).clamp(28.0, 40.0),
                                white.opacity(0.4),
                            ),
                        )),
                ),
            &[(0, MeasurePart::Cover)],
        );
        let info_font = (height * 0.02).max(16.0);
        let info = div().w(px(column_w)).child(self.amll_info_row(
            column_w,
            info_font,
            height,
            false,
            menu_interactive,
            shared_hidden,
            visual,
            window,
            cx,
        ));
        div()
            .w(px(column_w))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .opacity(if shared_hidden { 0.0 } else { 1.0 })
                    .child(cover),
            )
            // AMLL：封面与 music-info 行之间是 8px grid gap，控件列再有
            // `margin-top: calc(-8px + 1.75em)`，合起来 28px（1em = 16px）。
            .child(div().h(px(28.0)))
            .child(info)
            .into_any_element()
    }

    /// 底部进度、传输、音量与底栏。
    ///
    /// 闲置收起时进度条保持可见、可拖，因此它不进入显隐滤镜组；传输键、
    /// 音量条与底栏共用一块滤镜组。整组仍由 `layout` 统一定位，闲置居中
    /// 时一起平移，行间距保持设计值。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn stage_controls_group(
        &self,
        layout: StageControlsLayout,
        height: f32,
        label_font: f32,
        button: f32,
        visual: StageChromeVisual,
        bottom_row: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = layout.group;
        let relative = |rect: StageRect| {
            (
                px(rect.x - group.x),
                px(rect.y - group.y),
                px(rect.w),
                px(rect.h),
            )
        };
        let (progress_x, progress_y, progress_w, progress_h) = relative(layout.progress);
        // 传输 / 音量 / 底栏的滤镜组矩形：从传输行顶边包到底栏底边，
        // 缩放与模糊锚点仍是底边中心，和收起前的观感一致。
        let hide_rect = StageRect {
            x: group.x,
            y: layout.transport.y,
            w: group.w,
            h: (layout.bottom_row.y + layout.bottom_row.h - layout.transport.y).max(0.0),
        };
        let hide_relative = |rect: StageRect| {
            (
                px(rect.x - hide_rect.x),
                px(rect.y - hide_rect.y),
                px(rect.w),
                px(rect.h),
            )
        };
        let (transport_x, transport_y, transport_w, transport_h) = hide_relative(layout.transport);
        let (volume_x, volume_y, volume_w, volume_h) = hide_relative(layout.volume);
        let (bottom_x, bottom_y, bottom_w, bottom_h) = hide_relative(layout.bottom_row);
        let hideable = div()
            .size_full()
            .relative()
            .child(
                div()
                    .absolute()
                    .left(transport_x)
                    .top(transport_y)
                    .w(transport_w)
                    .h(transport_h)
                    .child(self.amll_transport(layout.transport.w, button, height, cx)),
            )
            .when(layout.volume.h > 0.0, |content| {
                content.child(
                    div()
                        .absolute()
                        .left(volume_x)
                        .top(volume_y)
                        .w(volume_w)
                        .h(volume_h)
                        .child(self.amll_volume_row(layout.volume.w, cx)),
                )
            })
            .child(
                div()
                    .absolute()
                    .left(bottom_x)
                    .top(bottom_y)
                    .w(bottom_w)
                    .h(bottom_h)
                    .child(bottom_row),
            );
        div()
            .absolute()
            .left(px(group.x))
            .top(px(group.y))
            .w(px(group.w))
            .h(px(group.h))
            .child(
                div()
                    .absolute()
                    .left(progress_x)
                    .top(progress_y)
                    .w(progress_w)
                    .h(progress_h)
                    .child(self.amll_progress_block(layout.progress.w, label_font, height, cx)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(hide_rect.x - group.x))
                    .top(px(hide_rect.y - group.y))
                    .w(px(hide_rect.w))
                    .h(px(hide_rect.h))
                    .child(
                        StageChromeGroup::new(hideable, visual, StageChromeAnchor::BottomCenter)
                            .into_any_element(),
                    ),
            )
            .into_any_element()
    }

    /// 共享元素舞台：一帧内只画一份封面 + 曲名/艺人，用插值几何定位。
    ///
    /// 调用方已在页面里把同名元素设为透明（保留占位），因此这里不做交叉
    /// 淡入——两端文案一致，字号、行高、位置连续变化即可。
    pub(super) fn render_shared_stage(&self, geometry: StageGeometry) -> AnyElement {
        let white = gpui::white();
        self.render_shared_stage_colored(geometry, white.opacity(0.9), white.opacity(0.45))
    }

    fn shared_stage_cover(&self, geometry: StageGeometry) -> AnyElement {
        let white = gpui::white();
        let theme = Theme::dark();
        let cover_size = geometry.cover.w.max(1.0);
        crate::widgets::cover_plain(
            &theme,
            self.cover_image.clone(),
            cover_size,
            geometry.cover_radius,
            crate::icons::icon(
                crate::icons::Icon::Note,
                (cover_size * 0.22).clamp(14.0, 40.0),
                white.opacity(0.35),
            ),
        )
    }

    /// 共享舞台的文案块：字号、行高、间距与字重都由插值几何决定。
    ///
    /// GPUI 的 `FontWeight` 内部就是 f32，可以直接连续插值。两端文案若是
    /// 两层交叉淡化，也必须使用同一个插值字重，否则会看到两种笔画粗细
    /// 叠在一起「抖动」，而不是一次平滑加粗。
    fn shared_stage_text(
        geometry: StageGeometry,
        title: SharedString,
        artist: SharedString,
        title_color: gpui::Hsla,
        artist_color: gpui::Hsla,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(geometry.line_gap))
            .child(
                div()
                    .max_w_full()
                    .text_size(px(geometry.title_size))
                    .line_height(px(geometry.title_size * INFO_LINE_RATIO))
                    .font_weight(gpui::FontWeight(geometry.title_weight))
                    .text_color(title_color)
                    .text_ellipsis()
                    .child(title),
            )
            .child(
                div()
                    .max_w_full()
                    .text_size(px(geometry.artist_size))
                    .line_height(px(geometry.artist_size * INFO_LINE_RATIO))
                    .font_weight(gpui::FontWeight(geometry.artist_weight))
                    .text_color(artist_color)
                    .text_ellipsis()
                    .child(artist),
            )
            .into_any_element()
    }

    /// 带文字的共享舞台；Dock 收尾时文字颜色会过渡回主面板主题色。
    pub(super) fn render_shared_stage_colored(
        &self,
        geometry: StageGeometry,
        title_color: gpui::Hsla,
        artist_color: gpui::Hsla,
    ) -> AnyElement {
        let cover = self.shared_stage_cover(geometry);
        let text = Self::shared_stage_text(
            geometry,
            SharedString::from(self.current_title_text()),
            SharedString::from(self.current_artist_text()),
            title_color,
            artist_color,
        );
        div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .left(px(geometry.cover.x))
                    .top(px(geometry.cover.y))
                    .child(cover),
            )
            .child(
                div()
                    .absolute()
                    .left(px(geometry.info.x))
                    .top(px(geometry.info.y))
                    .w(px(geometry.info.w.max(1.0)))
                    .child(text),
            )
            .into_any_element()
    }

    /// Dock↔全屏专用共享舞台：`p = 1` 时文案、字重与 Dock 完全一致，
    /// `p = 0` 时与静态沉浸页一致。
    ///
    /// 只有「文案内容」不同的两行（副标题多了 `· 来源`）才交叉淡化；
    /// 字重、字号、行高与颜色全部走插值后的同一份几何，避免出现两层
    /// 不同粗细的文字互相透出。
    pub(super) fn render_dock_shared_stage(
        &self,
        geometry: StageGeometry,
        title_color: gpui::Hsla,
        artist_color: gpui::Hsla,
        dock_p: f32,
    ) -> AnyElement {
        let cover = self.shared_stage_cover(geometry);
        let (dock_title, dock_artist) = self.dock_track_text();
        let page_text = Self::shared_stage_text(
            geometry,
            SharedString::from(self.current_title_text()),
            SharedString::from(self.current_artist_text()),
            title_color,
            artist_color,
        );
        let dock_text =
            Self::shared_stage_text(geometry, dock_title, dock_artist, title_color, artist_color);
        div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .left(px(geometry.cover.x))
                    .top(px(geometry.cover.y))
                    .child(cover),
            )
            .child(
                div()
                    .absolute()
                    .left(px(geometry.info.x))
                    .top(px(geometry.info.y))
                    .w(px(geometry.info.w.max(1.0)))
                    .h(px(geometry.info.h.max(1.0)))
                    .relative()
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .opacity(1.0 - dock_p)
                            .child(page_text),
                    )
                    .child(div().absolute().inset_0().opacity(dock_p).child(dock_text)),
            )
            .into_any_element()
    }

    /// AMLL 顶部拖拽 pill（50×8，白 17.6%）。
    ///
    /// 进入沉浸页后的前 5 秒，pill 会展开成一条提示文案（拖动/ESC 退出），
    /// 随后用 320ms 收回原来的小白条。按下命中区由外层 dismiss-handle
    /// 承担，拖动是「下拉关闭沉浸页」而不是移动窗口。
    ///
    /// `center` 是把手中心（相对页面），`max_text_w` 限定提示胶囊宽度，
    /// 窄窗里文案会省略而不是撑出封面。
    pub(super) fn amll_pill(
        &self,
        center: f32,
        top: f32,
        id: &'static str,
        max_text_w: f32,
    ) -> AnyElement {
        let white = gpui::white();
        let expand = self.immersive_hint_expand(Instant::now());
        let hint_w = 300.0_f32.min(max_text_w.max(120.0));
        // 高度 8→24、宽度 50→hint_w：展开时像「白条长成胶囊」，
        // 收回时再连续变回原来的小白条。
        let w = 50.0 + (hint_w - 50.0) * expand;
        let h = 8.0 + 16.0 * expand;
        // 胶囊还没长到能容纳文字时先不显示字，避免窄条里挤出残字。
        let text_alpha = ((expand - 0.15) / 0.85).clamp(0.0, 1.0);
        let mut pill = div()
            .id(id)
            .absolute()
            .left(px(center - w * 0.5))
            .top(px(top + 4.0 - h * 0.5))
            .w(px(w))
            .h(px(h))
            .rounded(px(h * 0.5))
            .bg(white.opacity(0.176))
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden();
        if expand > 0.02 {
            // 文字宽度固定为展开态宽度：收起过程中只被父级裁切，
            // 不会逐帧重新换行/测量。
            pill = pill.child(
                div()
                    .flex_none()
                    .w(px((hint_w - 20.0).max(1.0)))
                    .opacity(text_alpha)
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(white.opacity(0.92))
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(
                        "拖动此处退出沉浸模式或者ESC键退出沉浸模式",
                    )),
            );
        }
        pill.into_any_element()
    }

    /// 竖屏播放页：AMLL 竖向布局 + Salt Player 的子页面/自动隐藏逻辑。
    pub(super) fn render_immersive_compact(
        &mut self,
        width: f32,
        height: f32,
        dock_shared: bool,
        dock_opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let now = Instant::now();
        // 同一列宽供封面、信息、进度、传输、音量与 Dock 使用。
        let content_w = compact_content_w(width, height);
        let enter = self
            .motion
            .portrait_enter
            .as_ref()
            .map(|spring| spring.value(now).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let page = self.portrait_page;
        let transition = self
            .portrait_transition
            .filter(|t| !t.done(now) && !t.held());
        let preparing = self.stage_request.is_some()
            || self
                .portrait_transition
                .as_ref()
                .is_some_and(|transition| transition.held());
        let stage = if let Some(request) = self.stage_request {
            (!request.from_mode.is_source() && !request.to_mode.is_source()).then_some(request.from)
        } else {
            self.motion
                .stage
                .as_ref()
                .filter(|motion| motion.active(now) && motion.touches_compact())
                .map(|motion| motion.sample(now))
                .or_else(|| {
                    transition.as_ref().and_then(|_| {
                        self.stage_measurements
                            .cached(self.immersive_stage_mode(cx, width, height))
                    })
                })
        }
        .filter(|geometry| geometry.is_valid());
        let shared_hidden = stage.is_some() || dock_shared || preparing || transition.is_some();
        // 布局 2 的歌词文字：用户在右下角/抽屉里打开后，歌词铺满整窗，
        // 播放列让位（与 AMLL 竖向布局「歌词打开时隐藏传输控件」一致）。
        // 页面切换期间先隐藏文字，避免歌词压在滑动中的队列页上。
        let lyrics_on = self.compact_lyrics_visible(cx) && transition.is_none() && !self.eq_open;
        // 竖屏歌词把当前行放进小封面顶栏与底部控件之间，而不是整窗 42%。
        // 这个位置只由窗口尺寸决定；闲置只淡出传输/音量/底栏（进度条保留），
        // 不改容器/行高/字号，也不改这里的值，歌词不会因此重新整形或跳动。
        let target_align = if lyrics_on {
            let g = vertical_lyrics_geometry(width, height);
            ((g.lyric_top + (g.lyric_bottom - g.lyric_top) * 0.42) / height.max(1.0))
                .clamp(0.02, 0.98)
        } else {
            0.42
        };
        if (target_align - self.lyric_align).abs() > 0.0005 {
            self.lyric_align = target_align;
        }
        let lyric_align = self.lyric_align;
        self.lyric_view.update(cx, |view, _| {
            view.set_text_visible(lyrics_on);
            view.set_click_seek_enabled(lyrics_on);
            // 隐藏时仍保持测量：点开歌词直接复用当前行高与滚动位置。
            view.set_layout_when_hidden(true);
            view.set_player_config(LyricPlayerConfig {
                align_anchor: AlignAnchor::Center,
                align_position: lyric_align,
                enable_blur: !self.no_blur,
                ..LyricPlayerConfig::default()
            });
        });
        // 歌词容器固定为窗口尺寸（100vh / 100vw）：下拉收起时卡片变矮，
        // 歌词只被裁剪，不改变排版视口，因此不会逐帧重排/重绘。正文左右
        // 各有 1em padding，竖屏顶栏/控件按同一字号内缩，三者共用对齐线。
        let lyric_view_el = div()
            .absolute()
            .left_0()
            .top_0()
            .w(px(self.viewport_width.max(1.0)))
            .h(px(self.viewport_height.max(1.0)))
            .child(self.lyric_view.clone());

        // 顶部拖动带：播放页的整条页头都可拖动窗口；pill 附近保留
        // 下拉关闭手势。队列/更多页的页头有自己的可点元素，不铺拖动带。
        // Dock→全屏的位移由外层卡片承担，页面内部不再二次偏移。
        let sheet_offset = 0.0;
        let art_page =
            page == PortraitPage::Art && transition.is_none() && !preparing && !self.eq_open;
        let drag_band = art_page.then(|| {
            Self::window_drag_area(
                div()
                    .id("immersive-drag-band-compact")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(COMPACT_TOP))
                    .cursor(CursorStyle::OpenHand),
            )
        });
        // 顶部把手命中区与封面同宽：蓝色封面区域整块都能下拉关闭，
        // 而不是只有中间 120px 的小白条。
        let handle_w = compact_cover_size(width, height)
            .min(width)
            .max(120.0_f32.min(width));
        let handle_left = ((width - handle_w) / 2.0).max(0.0);
        // 竖屏封面态闲置后整组（把手 → 封面/信息 → 进度条）居中，把手跟
        // 播放列、进度条一起下移；竖屏歌词页的歌词带夹在顶栏与控件之间，
        // 平移会把顶栏压进歌词，因此只保留进度条、不参与整体平移。
        let side_panel = matches!(self.stage_view, StageView::Queue | StageView::Audio);
        let idle_shift = if art_page && !lyrics_on && !side_panel {
            let cover = compact_cover_size(width, height);
            let left = ((width - cover) / 2.0).max(0.0);
            let layout =
                compact_art_controls_layout(width, height, self.viewport_width, left, cover);
            self.stage_idle_shift(
                now,
                height,
                COMPACT_PILL_TOP,
                layout.progress.y + layout.progress.h,
            )
        } else {
            0.0
        };
        let handle_top = idle_shift;
        let dismiss_handle = art_page.then(|| {
            div()
                .id("immersive-dismiss-handle-compact")
                .absolute()
                .top(px(handle_top))
                .left(px(handle_left))
                .w(px(handle_w))
                .h(px(56.0))
                .cursor(CursorStyle::OpenHand)
                .occlude()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|app, event: &MouseDownEvent, _, cx| {
                        app.begin_sheet_drag(f32::from(event.position.y), cx);
                    }),
                )
                .child(self.amll_pill(handle_w / 2.0, 20.0, "immersive-pill-compact", width - 32.0))
        });

        // 页面本体：下拉时整块下移，露出背后的主面板。
        let mut sheet = div()
            .id("immersive-sheet-compact")
            .absolute()
            .left_0()
            .top(px(sheet_offset))
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(gpui::black())
            .opacity(if preparing { 0.0 } else { enter })
            .child(lyric_view_el)
            .when_some(drag_band, |sheet, band| sheet.child(band))
            .when_some(dismiss_handle, |sheet, handle| sheet.child(handle));

        // 控件收起时歌词仍可点，第一次点击/触摸应该只唤出控件，不能顺手
        // 命中歌词行跳时间轴。进度条在闲置态保留且可拖动，因此捕获层分成
        // 进度条上下两块，给它留出完整的可操作条带；控件恢复后的下一次
        // 点击才交给歌词。
        if lyrics_on && self.stage_chrome_hidden {
            let (_, controls) = vertical_lyrics_controls_layout(
                width,
                height,
                self.viewport_width,
                self.viewport_height,
            );
            let progress_top = controls.progress.y.clamp(0.0, height);
            let progress_bottom =
                (controls.progress.y + controls.progress.h).clamp(progress_top, height);
            let capture = |id: &'static str, top: f32, bottom: f32, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .absolute()
                    .left_0()
                    .top(px(top))
                    .w(px(width))
                    .h(px((bottom - top).max(0.0)))
                    .occlude()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|app, _, _, cx| {
                            app.note_stage_interaction(cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_move(cx.listener(|app, event: &MouseMoveEvent, _, cx| {
                        app.note_stage_pointer(
                            f32::from(event.position.x),
                            f32::from(event.position.y),
                            cx,
                        );
                    }))
            };
            sheet = sheet
                .child(capture("lyrics-wake-capture-top", 0.0, progress_top, cx))
                .child(capture(
                    "lyrics-wake-capture-bottom",
                    progress_bottom,
                    height,
                    cx,
                ));
        }

        let overlay = div()
            .id("immersive-compact-overlay")
            .key_context("ChordialImmersive")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
                if app.quality_dialog_open {
                    app.quality_dialog_open = false;
                } else if app.drawer_open {
                    app.close_drawer(cx);
                } else if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.exit_immersive(cx);
                }
                window.focus(&app.focus_handle);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &TogglePortrait, _, cx| {
                app.exit_immersive(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &ToggleEqualizer, _, cx| {
                if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.open_eq(cx);
                }
            }))
            .on_action(cx.listener(|app, _: &PlayPause, _, cx| app.toggle_playback(cx)))
            .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
            .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
            .on_action(cx.listener(|app, _: &PlayFirst, _, cx| app.play_first(cx)))
            .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                let y = f32::from(event.position.y);
                match event.pressed_button {
                    Some(gpui::MouseButton::Left) => app.update_sheet_drag(y, cx),
                    None => {
                        if app
                            .sheet_drag
                            .as_ref()
                            .is_some_and(|drag| !drag.animating(Instant::now()))
                        {
                            app.release_sheet_drag(y, height, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |app, event: &MouseUpEvent, _, cx| {
                    if app.sheet_drag.is_some() {
                        app.release_sheet_drag(f32::from(event.position.y), height, cx);
                    }
                }),
            )
            .size_full()
            .relative()
            .overflow_hidden();

        if let Some(tr) = transition {
            let (from_off, from_o, to_off, to_o) = tr.frames(now, width, height);
            let (fx, fy) = match tr.kind {
                crate::player_page::TransitionKind::Vertical => (0.0, from_off),
                crate::player_page::TransitionKind::Fade => (0.0, 0.0),
            };
            let (tx, ty) = match tr.kind {
                crate::player_page::TransitionKind::Vertical => (0.0, to_off),
                crate::player_page::TransitionKind::Fade => (0.0, 0.0),
            };
            let from = gpui::batch_layer(self.render_compact_page(
                tr.from,
                content_w,
                height,
                width,
                dock_opacity,
                shared_hidden,
                lyrics_on,
                window,
                cx,
            ));
            let to = gpui::batch_layer(self.render_compact_page(
                tr.to,
                content_w,
                height,
                width,
                dock_opacity,
                shared_hidden,
                lyrics_on,
                window,
                cx,
            ));
            sheet = sheet
                .child(
                    div()
                        .absolute()
                        .left(px(fx))
                        .top(px(fy))
                        .size_full()
                        .opacity(from_o)
                        .child(from),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(tx))
                        .top(px(ty))
                        .size_full()
                        .opacity(to_o)
                        .child(to),
                );
            self.ensure_ui_anim_ticker(cx);
        } else {
            sheet = sheet.child(gpui::batch_layer(self.render_compact_page(
                page,
                content_w,
                height,
                width,
                dock_opacity,
                shared_hidden,
                lyrics_on,
                window,
                cx,
            )));
        }

        if let Some((text, started)) = self.portrait_toast.clone() {
            if started.elapsed() < Duration::from_millis(1800) {
                sheet = sheet.child(
                    div()
                        .absolute()
                        .bottom(px(118.0))
                        .left(px((width - 300.0) / 2.0))
                        .w(px(300.0))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .rounded(px(16.0))
                                .bg(gpui::rgba(0x00000099))
                                .px(px(14.0))
                                .py(px(7.0))
                                .text_size(px(11.5))
                                .text_color(white.opacity(0.92))
                                .child(SharedString::from(text)),
                        ),
                );
            }
        }
        let mut overlay = overlay.child(sheet);
        if let Some(geometry) = stage {
            overlay = overlay.child(self.render_shared_stage(geometry));
        }
        overlay.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_compact_page(
        &self,
        page: PortraitPage,
        content_w: f32,
        height: f32,
        width: f32,
        dock_opacity: f32,
        shared_hidden: bool,
        lyrics_on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match page {
            PortraitPage::Art => self.render_compact_art(
                width,
                height,
                dock_opacity,
                shared_hidden,
                lyrics_on,
                window,
                cx,
            ),
            PortraitPage::Queue => self.render_portrait_queue(content_w, shared_hidden, false, cx),
        }
    }

    /// 视图选择器下拉菜单的锚定矩形。
    ///
    /// 必须与实际底栏使用同一套几何：竖屏歌词态的底栏是 1em 内缩的
    /// 全宽列，不能沿用「居中播放列」的宽度，否则菜单会从按钮左侧弹出。
    pub(super) fn immersive_view_menu_rect(
        &self,
        width: f32,
        height: f32,
        split: bool,
        cx: &App,
    ) -> StageRect {
        let pad = stable_lyric_pad(self.viewport_width, self.viewport_height);
        if split {
            return stage_bottom_rect(width, height, true, pad);
        }
        let transition_active = self
            .portrait_transition
            .is_some_and(|transition| !transition.done(Instant::now()));
        let side_panel = matches!(self.stage_view, StageView::Queue | StageView::Audio);
        if !side_panel && !self.eq_open && !transition_active && self.compact_lyrics_visible(cx) {
            let g = vertical_lyrics_geometry(width, height);
            return StageRect {
                x: g.left,
                y: height
                    - crate::layout_morph::COMPACT_BOTTOM
                    - crate::layout_morph::COMPACT_DOCK_H,
                w: g.content_w,
                h: crate::layout_morph::COMPACT_DOCK_H,
            };
        }
        if !side_panel {
            // 封面态底栏与播放列同宽（cover 可能小于 content_w）。
            let cover = compact_cover_size(width, height);
            return StageRect {
                x: ((width - cover) * 0.5).max(0.0),
                y: height
                    - crate::layout_morph::COMPACT_BOTTOM
                    - crate::layout_morph::COMPACT_DOCK_H,
                w: cover,
                h: crate::layout_morph::COMPACT_DOCK_H,
            };
        }
        stage_bottom_rect(width, height, false, pad)
    }

    /// 竖屏「显示歌词」：小封面 + 标题/更多顶栏，歌词铺满中部，控件沉底。
    ///
    /// 顶栏、歌词与底部控件共用 `vertical_lyrics_geometry` 的内容列；
    /// 滤镜组只改变透明度、模糊与缩放，歌词视图仍是固定视口，淡出不改行高
    /// 和字号。底栏按内容列宽重建，不能沿用居中播放列的底栏。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn amll_vertical_lyrics_column(
        &self,
        width: f32,
        height: f32,
        visual: StageChromeVisual,
        shared_hidden: bool,
        dock_opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (g, mut layout) = vertical_lyrics_controls_layout(
            width,
            height,
            self.viewport_width,
            self.viewport_height,
        );
        let white = gpui::white();
        let theme = Theme::dark();
        let cover_scale = self.cover_scale_now(Instant::now()).clamp(0.5, 1.1);
        let cover_side = g.cover_side * cover_scale;
        // 底栏与顶栏/控件同列，而不是 stage_bottom_rect 里更宽的居中列。
        let bottom_rect = layout.bottom_row;
        let bottom_row = div()
            .w(px(bottom_rect.w))
            .h(px(bottom_rect.h))
            .opacity(1.0 - dock_opacity)
            .child(self.immersive_bottom_row(bottom_rect.w, cx))
            .into_any_element();
        // 传输图标组不跟着全宽列拉散：居中并限制在标准内容列宽度内，
        // 进度 / 音量 / 底栏仍然占满整列，左右对齐线不受影响。
        let transport_w = layout.transport.w.min(560.0);
        layout.transport.x += (layout.transport.w - transport_w) * 0.5;
        layout.transport.w = transport_w;
        let controls =
            self.stage_controls_group(layout, height, g.label_font, g.btn, visual, bottom_row, cx);
        div()
            .size_full()
            .relative()
            .child(
                div()
                    .id("vertical-lyrics-header")
                    .absolute()
                    .left(px(g.header_left))
                    .top(px(g.header_top))
                    .w(px(g.header_w))
                    .h(px(g.cover_side))
                    .flex()
                    .items_center()
                    .occlude()
                    .child(
                        self.stage_measurements.observe(
                            StageMode::CompactLyrics,
                            div()
                                .size(px(g.cover_side))
                                .flex_none()
                                .opacity(if shared_hidden { 0.0 } else { 1.0 })
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(crate::widgets::cover_plain(
                                    &theme,
                                    self.cover_image.clone(),
                                    cover_side,
                                    1.0,
                                    crate::icons::icon(
                                        crate::icons::Icon::Note,
                                        (cover_side * 0.1).clamp(28.0, 40.0),
                                        white.opacity(0.4),
                                    ),
                                )),
                            &[(0, MeasurePart::Cover)],
                        ),
                    )
                    .child(div().ml(px(16.0)).w(px(g.info_w)).child(self.amll_info_row(
                        g.info_w,
                        g.info_font,
                        height,
                        false,
                        true,
                        shared_hidden,
                        visual,
                        window,
                        cx,
                    ))),
            )
            .child(controls)
            .into_any_element()
    }

    /// 竖屏播放页：与桌面宽屏同一根 `amll_player_column`，只是把整列水平居中。
    /// 歌词打开时整列让位，只保留底栏控件与全窗歌词文字。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_compact_art(
        &self,
        width: f32,
        height: f32,
        dock_opacity: f32,
        shared_hidden: bool,
        lyrics_on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cover = compact_cover_size(width, height);
        // 从布局 1 / 队列页切换过来时，播放列跟随共享封面的 x 一起滑动，
        // 而不是整列硬切到居中位置。
        let now = Instant::now();
        let visual = self.stage_chrome_visual(now);
        let motion_x = self
            .motion
            .stage
            .as_ref()
            .filter(|motion| motion.active(now) && motion.touches_compact())
            .map(|motion| motion.sample(now).cover.x);
        let left = motion_x.unwrap_or_else(|| ((width - cover) / 2.0).max(0.0));
        // 底栏与内容列同宽，左右边界与主沉浸区对齐；不再把按钮挂在窗口角上。
        let bottom_rect = stage_bottom_rect(
            width,
            height,
            false,
            stable_lyric_pad(self.viewport_width, self.viewport_height),
        );
        let side_panel = matches!(self.stage_view, StageView::Queue | StageView::Audio);
        let label_font = (height * 0.012).max(12.8);
        let btn = stage_button_size(cover);
        let bottom_row = div()
            .w(px(bottom_rect.w))
            .h(px(bottom_rect.h))
            .opacity(1.0 - dock_opacity)
            .child(self.immersive_bottom_row(bottom_rect.w, cx))
            .into_any_element();
        let mut root = div().size_full().relative();
        if lyrics_on && !side_panel {
            // 竖屏「显示歌词」：封面缩到顶栏左侧，标题/更多贴其右，
            // 中部整块留给歌词；传输/音量/底栏沉底并随闲置收起，进度条保留。
            root = root.child(self.amll_vertical_lyrics_column(
                width,
                height,
                visual,
                shared_hidden,
                dock_opacity,
                window,
                cx,
            ));
        } else if !lyrics_on && !side_panel {
            // 控件区一直延伸到固定底栏的顶边，剩余高度在 3/4 道间距
            // （矮窗收起音量行）之间均分，避免最短的窗口把最后一道压成 0。
            // 闲置收起时整组（把手 → 封面/信息 → 进度条）做刚体平移，
            // 内部间距保持完整布局的设计值，只把组中点对齐到窗口中心。
            let layout =
                compact_art_controls_layout(width, height, self.viewport_width, left, cover);
            let idle_shift = self.stage_idle_shift(
                now,
                height,
                COMPACT_PILL_TOP,
                layout.progress.y + layout.progress.h,
            );
            let layout = layout.translated_y(idle_shift);
            // 底栏与播放列同宽：cover 被高度压缩时，底栏不能还按更宽的
            // content_w 画，否则全屏钮会凸到封面/进度条左侧。
            let art_bottom = StageRect {
                x: left,
                y: layout.bottom_row.y,
                w: cover,
                h: layout.bottom_row.h,
            };
            let art_bottom_row = div()
                .w(px(art_bottom.w))
                .h(px(art_bottom.h))
                .opacity(1.0 - dock_opacity)
                .child(self.immersive_bottom_row(art_bottom.w, cx))
                .into_any_element();
            root = root
                .child(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(COMPACT_TOP + idle_shift))
                        .w(px(cover))
                        .child(self.amll_player_column(
                            cover,
                            height,
                            true,
                            visual,
                            shared_hidden,
                            !shared_hidden,
                            window,
                            cx,
                        )),
                )
                .child(self.stage_controls_group(
                    layout,
                    height,
                    label_font,
                    btn,
                    visual,
                    art_bottom_row,
                    cx,
                ));
        } else {
            // 窄窗没有宽屏右列：把同一份内容放在右侧区域，播放列让位。
            let inner_w = (bottom_rect.w - 24.0).max(1.0);
            let panel_h = (bottom_rect.y - COMPACT_TOP - 12.0).max(1.0);
            let panel = div()
                .absolute()
                .left(px(bottom_rect.x))
                .top(px(COMPACT_TOP))
                .w(px(bottom_rect.w))
                .h(px(panel_h))
                .p(px(12.0))
                .rounded(px(12.0))
                .bg(gpui::black().opacity(0.62))
                .border_1()
                .border_color(gpui::white().opacity(0.08))
                .overflow_hidden()
                .child(match self.stage_view {
                    StageView::Queue => {
                        self.render_portrait_queue(inner_w, shared_hidden, true, cx)
                    }
                    StageView::Audio => self.render_audio_details_panel(inner_w, cx),
                    _ => div().into_any_element(),
                });
            root =
                root.child(panel)
                    .child(stage_chrome_bottom_group(bottom_rect, visual, bottom_row));
        }
        root.into_any_element()
    }

    /// 宽屏沉浸：AMLL react-full `HorizontalLayout` 的等价几何。
    ///
    /// 1600×1000 实测：列 info 0.45W / player 0.55W（gap 8px）；行
    /// drag = 0.12fr / thumb 40 / cover / music-info = 0.8fr / buttom 40 / tail = 0.08fr，
    /// fr = H − 120 − cover；封面 = min(45vh, 38vw)（H ≤ 1000，否则 50vh）；
    /// 歌词列右 padding 8%（H > 1000 时 15%）；歌词对齐封面中心（AMLL 用
    /// ResizeObserver 计算 cover 中线占比后作为 `alignPosition`）。
    pub(super) fn render_immersive_wide(
        &mut self,
        width: f32,
        height: f32,
        dock_shared: bool,
        dock_opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = Instant::now();
        let visual = self.stage_chrome_visual(now);
        // 右侧内容列只在「显示歌词」时承载歌词文字；队列/音频详情模式下
        // 同一块区域改画对应内容，歌词退回纯背景。
        let lyrics_in_view = self.stage_view == StageView::Lyrics;
        self.lyric_view.update(cx, |view, _| {
            view.set_text_visible(lyrics_in_view);
            view.set_click_seek_enabled(lyrics_in_view);
        });

        // ---- AMLL horizontal 几何 ----
        let cover = wide_cover_size(width, height);
        let gap = 8.0;
        // 剩余 fr 高度按 AMLL 的 6 行 grid 计算：thumb 2×2vh=36，
        // 5 道 8px gap。
        let fr = (height - cover - 76.0).max(0.0);
        let drag = fr * 0.12;
        let cover_top = wide_cover_top(width, height);
        // 歌词容器固定按窗口几何（100vh）计算，而不是随下拉手势收缩的
        // 卡片：卡片只负责裁剪，内容视口在拖动期间保持不变。
        let stable_w = self.viewport_width.max(1.0);
        let stable_h = self.viewport_height.max(1.0);
        let stable_info_w = stable_w * 0.45;
        let stable_lyric_col_w = (stable_w - stable_info_w - gap).max(1.0);
        let stable_lyric_pad = stable_lyric_pad(stable_w, stable_h);
        // 底栏左右边界与主沉浸区对齐；音量行停在控制行上方，
        // 信息区用剩余高度做 space-between。
        let bottom_rect = stage_bottom_rect(width, height, true, stable_lyric_pad);
        let bottom_row_top = bottom_rect.y;
        let controls_top = cover_top + cover + COMPACT_COVER_GAP;
        // 控件区包含到固定底栏顶边：4 道间距均分（信息→进度、进度→传输、
        // 传输→音量、音量→底栏），底栏不再只靠 8px 硬留白贴在窗口底部。
        let controls_h = (bottom_row_top - controls_top).max(0.0);
        let info_w = width * 0.45;
        let lyric_left = info_w + gap;

        // 布局 1 固定为「左播放列 + 右歌词」；歌词开关切换时会立即落到
        // 布局 2，因此这里不再维护居中分支。
        let preparing = self.stage_request.is_some()
            || self
                .portrait_transition
                .as_ref()
                .is_some_and(|transition| transition.held());
        let split_x = wide_column_x(width, height);
        let stage = if let Some(request) = self.stage_request {
            (!request.from_mode.is_source() && !request.to_mode.is_source()).then_some(request.from)
        } else {
            self.motion
                .stage
                .as_ref()
                .filter(|motion| motion.active(now) && motion.touches_wide())
                .map(|motion| motion.sample(now))
        }
        .filter(|geometry| geometry.is_valid());
        let mut column_x = split_x;
        if let Some(geometry) = stage {
            column_x = geometry.cover.x;
        }
        let shared_hidden = stage.is_some() || dock_shared || preparing;

        // 对齐位置同样按 100vh 的封面中线计算：下拉手势只改变卡片可见
        // 区域，不应每帧改写 alignPosition（那会触发整段歌词重新布局）。
        let stable_cover = wide_cover_size(stable_w, stable_h);
        let stable_cover_top = wide_cover_top(stable_w, stable_h);
        let align = ((stable_cover_top + stable_cover * 0.5) / stable_h).clamp(0.05, 0.95);
        if (self.lyric_align - align).abs() > 0.0005 {
            self.lyric_align = align;
            self.lyric_view.update(cx, |view, _| {
                view.set_player_config(LyricPlayerConfig {
                    align_anchor: AlignAnchor::Center,
                    align_position: align,
                    enable_blur: !self.no_blur,
                    ..LyricPlayerConfig::default()
                });
            });
        }
        // —— 底部控件的完整布局先算出来：闲置居中需要它的进度条底边。 ——
        let label_font = (height * 0.012).max(12.8);
        let btn = stage_button_size(cover);
        let volume_h = if stage_volume_visible(height) {
            STAGE_VOLUME_H
        } else {
            0.0
        };
        let gap_count = if volume_h > 0.0 { 4 } else { 3 };
        let controls_layout = stage_controls_layout(
            controls_top,
            controls_h,
            amll_info_height(self.viewport_width, height),
            gap_count,
            column_x,
            cover,
            btn,
            volume_h,
            bottom_rect,
        );
        // 闲置收起后保留可见组（顶部把手 → 封面/信息 → 进度条）整体居中：
        // 只做刚体平移，组内间距仍是完整布局的设计值。传输/音量/底栏跟着
        // 同一下移量走，避免进度条滑过它们时出现重叠；它们的显隐仍由滤镜
        // 负责。
        let pill_top = drag + gap + 16.0;
        let idle_shift = if self.shared_stage_active(now) {
            0.0
        } else {
            self.stage_idle_shift(
                now,
                height,
                pill_top,
                controls_layout.progress.y + controls_layout.progress.h,
            )
        };
        let controls_layout = controls_layout.translated_y(idle_shift);

        // —— 顶部窗口拖动带：整条空白头部都可拖动窗口（AMLL drag-area）。
        //    中间与封面同宽的区域是下拉关闭把手，其余宽度用来移动窗口。
        // Dock→全屏的位移由外层卡片承担，页面内部不再二次偏移。
        let sheet_offset = 0.0;
        let drag_band = (!self.eq_open).then(|| {
            Self::window_drag_area(
                div()
                    .id("immersive-drag-band")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(pill_top + 8.0))
                    .cursor(CursorStyle::OpenHand),
            )
        });

        // 下拉关闭把手：与封面同宽的整块区域（含 pill 所在的顶部），
        // occlude 掉下层的窗口拖动带。封面区域不再只响应小白条，
        // 而是像拖动小白条一样整块下拉关闭。EQ 覆盖层打开时不启用，
        // 避免顶部的 EQ 控件被手势抢走。闲置居中时把手跟着 pill 下移。
        let handle_w = cover.max(120.0).min(width);
        let handle_top = (pill_top - 20.0).max(0.0) + idle_shift;
        let handle_left = (column_x + cover * 0.5 - handle_w / 2.0).max(0.0);
        let dismiss_handle = (!self.eq_open).then(|| {
            div()
                .id("immersive-dismiss-handle")
                .absolute()
                .top(px(handle_top))
                .left(px(handle_left))
                .w(px(handle_w))
                .h(px(56.0))
                .cursor(CursorStyle::OpenHand)
                .occlude()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|app, event: &MouseDownEvent, _, cx| {
                        app.begin_sheet_drag(f32::from(event.position.y), cx);
                    }),
                )
                .child(self.amll_pill(handle_w / 2.0, 20.0, "immersive-pill", cover))
        });

        // —— 静态封面 / 信息列；底部控件单独成组 ——
        let info_column = div()
            .absolute()
            .left(px(column_x))
            .top(px(cover_top + idle_shift))
            .w(px(cover))
            .child(self.amll_player_column(
                cover,
                height,
                !dock_shared,
                visual,
                shared_hidden,
                !shared_hidden,
                window,
                cx,
            ));
        let bottom_row = div()
            .w(px(bottom_rect.w))
            .h(px(bottom_rect.h))
            .opacity(1.0 - dock_opacity)
            .child(self.immersive_bottom_row(bottom_rect.w, cx))
            .into_any_element();
        let controls_group = self.stage_controls_group(
            controls_layout,
            height,
            label_font,
            btn,
            visual,
            bottom_row,
            cx,
        );

        // —— 与宽屏「显示歌词」完全相同的右侧内容区：歌词 / 队列 /
        // 音频详情共用这一块矩形，只有「仅显示主内容」走紧凑布局收起它。
        // 左端跟随卡片移动，宽高固定为窗口几何，拖动时不重新排版内容。
        let panel_w = (stable_lyric_col_w - stable_lyric_pad).max(1.0);
        let side_content = match self.stage_view {
            StageView::Queue => self.render_portrait_queue(panel_w, shared_hidden, true, cx),
            StageView::Audio => self.render_audio_details_panel(panel_w, cx),
            _ => self.lyric_view.clone().into_any_element(),
        };
        let right_area = div()
            .absolute()
            .left(px(lyric_left))
            .top_0()
            .w(px(stable_lyric_col_w))
            .h(px(stable_h))
            .pr(px(stable_lyric_pad))
            .child(side_content);

        let sheet = div()
            .id("immersive-sheet")
            .absolute()
            .left_0()
            .top(px(sheet_offset))
            .w(px(width))
            .h(px(height))
            .opacity(if preparing { 0.0 } else { 1.0 })
            .overflow_hidden()
            .bg(gpui::black())
            .child(right_area)
            .child(gpui::batch_layer(info_column))
            .child(gpui::batch_layer(controls_group))
            .when_some(drag_band, |sheet, band| sheet.child(band))
            .when_some(dismiss_handle, |sheet, handle| sheet.child(handle));

        let root = div()
            .key_context("ChordialImmersive")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &ExitImmersive, window, cx| {
                if app.quality_dialog_open {
                    app.quality_dialog_open = false;
                } else if app.drawer_open {
                    app.close_drawer(cx);
                } else if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.exit_immersive(cx);
                }
                window.focus(&app.focus_handle);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &TogglePortrait, _, cx| {
                app.exit_immersive(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &ToggleEqualizer, _, cx| {
                if app.eq_open {
                    app.close_eq(cx);
                } else {
                    app.open_eq(cx);
                }
            }))
            .on_action(cx.listener(|app, _: &PlayPause, _, cx| app.toggle_playback(cx)))
            .on_action(cx.listener(|app, _: &NextTrack, _, cx| app.play_next(cx)))
            .on_action(cx.listener(|app, _: &PrevTrack, _, cx| app.play_prev(cx)))
            .on_action(cx.listener(|app, _: &PlayFirst, _, cx| app.play_first(cx)))
            .on_mouse_move(cx.listener(move |app, event: &MouseMoveEvent, _, cx| {
                let y = f32::from(event.position.y);
                match event.pressed_button {
                    Some(gpui::MouseButton::Left) => app.update_sheet_drag(y, cx),
                    None => {
                        if app
                            .sheet_drag
                            .as_ref()
                            .is_some_and(|drag| !drag.animating(Instant::now()))
                        {
                            app.release_sheet_drag(y, height, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |app, event: &MouseUpEvent, _, cx| {
                    if app.sheet_drag.is_some() {
                        app.release_sheet_drag(f32::from(event.position.y), height, cx);
                    }
                }),
            )
            .size_full()
            .relative()
            .overflow_hidden();
        let mut root = root.child(sheet);
        if let Some(geometry) = stage {
            root = root.child(self.render_shared_stage(geometry));
        }
        root.into_any_element()
    }
}

#[cfg(test)]
mod stage_controls_tests {
    use super::*;

    fn bottom_rect(x: f32, y: f32, w: f32) -> StageRect {
        StageRect { x, y, w, h: 40.0 }
    }

    #[test]
    fn controls_keep_fixed_rows_and_one_shared_group() {
        let layout = stage_controls_layout(
            300.0,
            300.0,
            100.0,
            3,
            10.0,
            200.0,
            40.0,
            40.0,
            bottom_rect(0.0, 700.0, 300.0),
        );

        assert_eq!(layout.progress.y, 300.0 + 100.0 + (300.0 - 225.2) / 3.0);
        assert_eq!(layout.progress.h, 45.2);
        assert_eq!(layout.transport.h, 40.0);
        assert_eq!(layout.volume.h, 40.0);
        assert_eq!(layout.group.y, layout.progress.y);
        assert_eq!(layout.group.y + layout.group.h, 740.0);
        assert!(layout.contains(layout.progress));
        assert!(layout.contains(layout.transport));
        assert!(layout.contains(layout.volume));
        assert!(layout.contains(layout.bottom_row));
    }

    #[test]
    fn group_expands_to_cover_a_wider_bottom_row() {
        let layout = stage_controls_layout(
            300.0,
            200.0,
            80.0,
            3,
            200.0,
            180.0,
            40.0,
            40.0,
            bottom_rect(20.0, 700.0, 500.0),
        );

        assert_eq!(layout.group.x, 20.0);
        assert_eq!(layout.group.w, 500.0);
    }

    #[test]
    fn idle_shift_is_rigid_and_centers_the_surviving_group() {
        // 截图尺寸 1586×1056：封面态闲置后，保留下来的「把手 → 进度条」
        // 组中点落到窗口中心，且进度/传输/音量/底栏之间的设计间距不变。
        let (w, h) = (1586.0, 1056.0);
        let cover = wide_cover_size(w, h);
        let cover_top = wide_cover_top(w, h);
        let bottom = stage_bottom_rect(w, h, true, stable_lyric_pad(w, h));
        let controls_top = cover_top + cover + COMPACT_COVER_GAP;
        let controls_h = (bottom.y - controls_top).max(0.0);
        let volume_h = if stage_volume_visible(h) {
            STAGE_VOLUME_H
        } else {
            0.0
        };
        let gap_count = if volume_h > 0.0 { 4 } else { 3 };
        let layout = stage_controls_layout(
            controls_top,
            controls_h,
            amll_info_height(w, h),
            gap_count,
            wide_column_x(w, h),
            cover,
            stage_button_size(cover),
            volume_h,
            bottom,
        );
        // 与 render_immersive_wide 的 pill_top 公式一致。
        let pill_top = (h - cover - 76.0).max(0.0) * 0.12 + 8.0 + 16.0;
        let shift = crate::layout_morph::idle_chrome_shift(
            h,
            pill_top,
            layout.progress.y + layout.progress.h,
        );
        assert!(shift > 0.0);
        let moved = layout.translated_y(shift);

        let old_gap = layout.transport.y - (layout.progress.y + layout.progress.h);
        let new_gap = moved.transport.y - (moved.progress.y + moved.progress.h);
        assert!((old_gap - new_gap).abs() < 1e-4);
        let old_row_gap = layout.transport.y - layout.progress.y;
        let new_row_gap = moved.transport.y - moved.progress.y;
        assert!((old_row_gap - new_row_gap).abs() < 1e-4);
        let center = ((pill_top + shift) + moved.progress.y + moved.progress.h) * 0.5;
        assert!((center - h * 0.5).abs() < 1e-3);
    }

    #[test]
    fn vertical_lyrics_header_lyrics_and_controls_share_one_column() {
        // 强制竖屏的横向窗口是错位最明显的场景：顶栏曾贴窗口左缘
        // （1.75em），控件却是 0.55H 的居中列。三者必须共用同一条列。
        for (w, h) in [(400.0, 800.0), (809.0, 523.0), (1200.0, 800.0)] {
            let g = vertical_lyrics_geometry(w, h);
            let pad = immersive_lyric_padding(w, h);
            assert!((g.header_left - g.left).abs() < 0.01, "{w}x{h}");
            assert!((g.header_w - g.content_w).abs() < 0.01, "{w}x{h}");
            // 歌词正文左缘 = 视口左缘 + 1em padding；右缘同样减去 padding。
            assert!((g.left - pad).abs() < 0.01, "{w}x{h}");
            assert!((g.left + g.content_w - (w - pad)).abs() < 0.01, "{w}x{h}");
            assert!(g.info_w > 0.0);

            let bottom = StageRect {
                x: g.left,
                y: h - crate::layout_morph::COMPACT_BOTTOM - crate::layout_morph::COMPACT_DOCK_H,
                w: g.content_w,
                h: 40.0,
            };
            let layout = stage_controls_layout(
                g.controls_top,
                g.controls_h,
                0.0,
                g.gap_count,
                g.left,
                g.content_w,
                g.btn,
                g.volume_h,
                bottom,
            );
            let transport_end = layout.transport.y + layout.transport.h;
            let mut gaps = vec![
                layout.progress.y - g.controls_top,
                layout.transport.y - (layout.progress.y + layout.progress.h),
            ];
            if g.volume_h > 0.0 {
                gaps.push(layout.volume.y - transport_end);
                gaps.push(bottom.y - (layout.volume.y + layout.volume.h));
            } else {
                // 音量行高度为 0 时它正好落在底栏顶边，传输行到
                // 底栏之间只保留一道 gap。
                gaps.push(bottom.y - transport_end);
            }
            assert_eq!(gaps.len(), g.gap_count as usize, "{w}x{h}");
            for gap in &gaps {
                assert!(
                    (gap - gaps[0]).abs() < 0.01,
                    "{w}x{h}: gaps not even: {gaps:?}"
                );
                assert!(*gap >= 5.9, "{w}x{h}: cramped gap {gap}");
            }
            assert_eq!(
                g.volume_h > 0.0,
                h >= crate::layout_morph::STAGE_VOLUME_MIN_H
            );
            assert!(g.lyric_bottom > g.lyric_top);
            assert!((g.lyric_bottom - layout.progress.y).abs() > 0.0);
        }
    }
}
