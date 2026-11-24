//! `ChordialApp` 的 render_stage 行为。
//!
//! 从 app.rs 拆分而来；仅做代码搬移，行为不变。

use super::*;

/// 竖屏「显示歌词」的 AMLL `VerticalLayout` 等价几何。
///
/// 顶栏 = 3em 左留白 + 6em（窄屏 4.5em）小封面 + 1em 间距 + 曲名/艺人/更多；
/// 中部歌词沿用整窗容器，只把对齐位置压进顶栏与控件之间；
/// 底部依次是进度、传输、音量、底栏。所有尺寸只依赖窗口大小，闲置隐藏
/// 只改 alpha / 命中高度，绝不参与这套几何重算（歌词不重新整形）。
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
    progress_h: f32,
    volume_h: f32,
    controls_h: f32,
    controls_top: f32,
    lyric_top: f32,
    lyric_bottom: f32,
}

fn vertical_lyrics_geometry(width: f32, height: f32) -> VerticalLyricsGeometry {
    let em = 16.0_f32;
    let content_w = compact_content_w(width, height);
    // AMLL 竖向布局：小封面 6em，宽 ≤ 480 收到 4.5em；x = 3em。
    let cover_side = if width <= 480.0 { em * 4.5 } else { em * 6.0 };
    // drag 30 / thumb 30 / gap 2；封面再从主区顶部下移 1em。
    let header_top = 30.0 + 30.0 + 2.0 + em;
    let left = ((width - content_w) / 2.0).max(0.0);
    // 封面贴左边距 1.75em，与用户的对齐标线一致（不再跟内容列一起居中）。
    let header_left = (em * 1.75).min((width - cover_side).max(0.0));
    let right_pad = em * 3.0;
    let header_w = (width - header_left - right_pad).max(1.0);
    let info_w = (header_w - cover_side - em).max(1.0);
    let info_font = (height * 0.02).clamp(14.0, 20.0);
    let label_font = (height * 0.012).max(12.8);
    let btn = (content_w * 0.18).clamp(if content_w < 240.0 { 34.0 } else { 40.0 }, 88.0);
    let progress_h = 45.2;
    let volume_h = 40.0;
    let controls_h = progress_h + btn + volume_h + 8.0;
    let bottom_y =
        height - crate::layout_morph::COMPACT_BOTTOM - crate::layout_morph::COMPACT_DOCK_H;
    let controls_top = (bottom_y - 8.0 - controls_h).max(header_top + cover_side + 8.0);
    let lyric_top = header_top + cover_side + 8.0;
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
        progress_h,
        volume_h,
        controls_h,
        controls_top,
        lyric_top,
        lyric_bottom,
    }
}

/// 竖屏歌词底部 `mask-image` 的渐隐高度（相对歌词容器高度）。
/// 控件可见时按此比例把字形淡出；控件收起后乘 0，遮罩随之取消。
pub(super) fn compact_lyric_bottom_mask(width: f32, height: f32) -> f32 {
    let g = vertical_lyrics_geometry(width, height);
    ((height - g.controls_top + 8.0) / height.max(1.0)).clamp(0.0, 1.0)
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
        chrome: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let white = gpui::white();
        let weak = cx.entity().downgrade();
        let title = self.current_title_text();
        let artist = self.current_artist_text();
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
            if interactive {
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
        // 闲置隐藏时圆钮连同留白一起收起，曲名/艺人获得整行宽度。
        let action = div()
            .ml(px(INFO_ACTION_GAP * chrome))
            .w(px(action_side * chrome))
            .h(px(action_side))
            .overflow_hidden()
            .opacity(chrome)
            .child(action);
        let available = (info_text_width(width, self.viewport_width, height)
            + (action_side + INFO_ACTION_GAP) * (1.0 - chrome))
            .max(1.0);
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
                        white.opacity(0.45),
                    )),
            )
            .child(action)
            .into_any_element()
    }

    /// 桌面宽屏与竖屏共用的 AMLL 播放器列。
    ///
    /// 封面、信息、进度、传输、音量共用同一个列宽；歌词关闭/无歌词时由调用方
    /// 把这整列水平居中。这样两端只有定位不同，控件本身不再各写一套。
    ///
    /// `cover_drag` 为 true 时，整块封面和顶部小白条一样响应下拉关闭手势
    /// （手机壳播放页有自己的横滑/点击手势，传 false）。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn amll_player_column(
        &self,
        column_w: f32,
        height: f32,
        controls_h: f32,
        menu_interactive: bool,
        chrome: f32,
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
        let cover = div()
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
            );
        let info_font = (height * 0.02).max(16.0);
        let info = div().w(px(column_w)).child(self.amll_info_row(
            column_w,
            info_font,
            height,
            false,
            menu_interactive,
            shared_hidden,
            chrome,
            window,
            cx,
        ));
        let label_font = (height * 0.012).max(12.8);
        let progress = div()
            .w(px(column_w))
            .h(px(45.2))
            .child(self.amll_progress_block(column_w, label_font, height, cx));
        // 手机最小宽度时封面列只有 ~184px；40px 的按钮会让 5 个传输键
        // 互相挤压。低于 240 时收到 34px，图标仍保持可点击尺寸。
        let btn = (column_w * 0.18).clamp(if column_w < 240.0 { 34.0 } else { 40.0 }, 88.0);
        // 闲置时传输键与音量行淡出并把高度让给 space-between 的剩余分布，
        // 信息行和进度条不隐藏（对齐用户要求的关键元素）。
        let transport = div()
            .w(px(column_w))
            .h(px(btn * chrome))
            .overflow_hidden()
            .opacity(chrome)
            .child(self.amll_transport(column_w, btn, height, cx));
        let volume = div()
            .w(px(column_w))
            .h(px(40.0 * chrome))
            .overflow_hidden()
            .opacity(chrome)
            .child(self.amll_volume_row(column_w, cx));
        let controls = div()
            .w(px(column_w))
            .h(px(controls_h.max(0.0)))
            .flex()
            .flex_col()
            .items_center()
            .justify_between()
            .child(info)
            .child(progress)
            .child(transport)
            .child(volume);
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
            .child(controls)
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
            .portrait_enter
            .map(|t| (t.elapsed().as_secs_f32() / 0.26).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let enter = 1.0 - (1.0 - enter).powi(3);
        let page = self.portrait_page;
        let transition = self.portrait_transition.filter(|t| !t.done(now));
        // 共享元素补间：页面滑入/滑出时，封面与曲目信息走独立的 FLIP 路径，
        // 页面内的同名元素暂时透明，只保留占位。
        let stage = self
            .stage_motion
            .filter(|motion| motion.active(now) && motion.touches_compact())
            .map(|motion| motion.geometry(now))
            .filter(|geometry| geometry.is_valid());
        let shared_hidden = stage.is_some() || dock_shared;
        // 布局 2 的歌词文字：用户在右下角/抽屉里打开后，歌词铺满整窗，
        // 播放列让位（与 AMLL 竖向布局「歌词打开时隐藏传输控件」一致）。
        // 页面切换期间先隐藏文字，避免歌词压在滑动中的队列页上。
        let lyrics_on = self.compact_lyrics_visible(cx) && transition.is_none() && !self.eq_open;
        // 竖屏歌词把当前行放进小封面顶栏与底部控件之间，而不是整窗 42%。
        // 这个位置只由窗口尺寸决定；闲置隐藏只淡出控件，不改容器/行高/
        // 字号，也不改这里的值，歌词不会因此重新整形或跳动。
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
        // 歌词只被裁剪，不改变排版视口，因此不会逐帧重排/重绘。
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
        let art_page = page == PortraitPage::Art && transition.is_none() && !self.eq_open;
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
        let handle_top = 0.0;
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
                .child(self.amll_pill(
                    handle_w / 2.0,
                    20.0 - handle_top,
                    "immersive-pill-compact",
                    width - 32.0,
                ))
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
            .opacity(enter)
            .child(lyric_view_el)
            .when_some(drag_band, |sheet, band| sheet.child(band))
            .when_some(dismiss_handle, |sheet, handle| sheet.child(handle));

        // 控件收起时歌词仍可点，第一次点击/触摸应该只唤出控件，不能顺手
        // 命中歌词行跳时间轴。透明 occlude 层挡住歌词命中区，并在这里
        // 吃掉这次鼠标按下；控件恢复后的下一次点击才交给歌词。
        if lyrics_on && self.stage_chrome_hidden {
            sheet = sheet.child(
                div()
                    .id("lyrics-wake-capture")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|app, _, _, cx| {
                            app.note_stage_interaction(cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_move(cx.listener(|app, _, _, cx| {
                        app.note_stage_interaction(cx);
                    })),
            );
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
                        if app.sheet_drag.is_some_and(|drag| !drag.animating()) {
                            app.release_sheet_drag(height, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |app, _: &MouseUpEvent, _, cx| {
                    if app.sheet_drag.is_some() {
                        app.release_sheet_drag(height, cx);
                    }
                }),
            )
            .size_full()
            .relative()
            .overflow_hidden();

        if let Some(tr) = transition {
            let (mut from_off, from_o, mut to_off, to_o) = tr.frames(now, width, height);
            // 从队列页返回播放页时方向相反：子页退出、播放页归位。
            if matches!(tr.from, PortraitPage::Queue) {
                from_off = -from_off;
                to_off = -to_off;
            }
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
            if self.portrait_transition.is_some() {
                self.portrait_transition = None;
            }
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

        // 共享元素舞台画在页面之上：封面从大图连续缩到队列页头，
        // 曲名与艺人同步移动、缩放，而不是随页面一起硬切。
        if let Some(geometry) = stage {
            sheet = sheet.child(self.render_shared_stage(geometry));
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
        overlay.child(sheet).into_any_element()
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

    /// 竖屏「显示歌词」：小封面 + 标题/更多顶栏，歌词铺满中部，控件沉底。
    ///
    /// 几何全部来自 `vertical_lyrics_geometry`，`chrome` 只驱动透明度与命中
    /// 高度；歌词视图仍是 100vw/100vh 的固定容器，淡出不改行高和字号。
    pub(super) fn amll_vertical_lyrics_column(
        &self,
        width: f32,
        height: f32,
        chrome: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let g = vertical_lyrics_geometry(width, height);
        let white = gpui::white();
        let theme = Theme::dark();
        let cover_scale = self.cover_scale_now(Instant::now()).clamp(0.5, 1.1);
        let cover_side = g.cover_side * cover_scale;
        // 底部控件的背景渐隐交给歌词视图的 mask-image（见
        // `compact_lyric_bottom_mask`），这里不再叠任何黑色图层。
        let wrap = |h: f32, el: AnyElement| {
            div()
                .w(px(g.content_w))
                .h(px(h * chrome))
                .overflow_hidden()
                .opacity(chrome)
                .child(el)
        };
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
                        div()
                            .size(px(g.cover_side))
                            .flex_none()
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
                    )
                    .child(div().ml(px(16.0)).w(px(g.info_w)).child(self.amll_info_row(
                        g.info_w,
                        g.info_font,
                        height,
                        false,
                        true,
                        false,
                        chrome,
                        window,
                        cx,
                    ))),
            )
            .child(
                div()
                    .id("vertical-lyrics-controls")
                    .absolute()
                    .left(px(g.left))
                    .top(px(g.controls_top))
                    .w(px(g.content_w))
                    .h(px(g.controls_h))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_between()
                    .child(wrap(
                        g.progress_h,
                        self.amll_progress_block(g.content_w, g.label_font, height, cx),
                    ))
                    .child(wrap(
                        g.btn,
                        self.amll_transport(g.content_w, g.btn, height, cx),
                    ))
                    .child(wrap(g.volume_h, self.amll_volume_row(g.content_w, cx))),
            )
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
        let available = (height
            - COMPACT_TOP
            - crate::layout_morph::COMPACT_BOTTOM
            - crate::layout_morph::COMPACT_DOCK_H
            - 8.0)
            .max(0.0);
        let controls_h = (available - cover - COMPACT_COVER_GAP).max(0.0);
        // 从布局 1 / 队列页切换过来时，播放列跟随共享封面的 x 一起滑动，
        // 而不是整列硬切到居中位置。
        let now = Instant::now();
        let chrome = self.stage_chrome_visibility(now);
        let motion_x = self
            .stage_motion
            .filter(|motion| motion.active(now) && motion.touches_compact())
            .map(|motion| motion.geometry(now).cover.x);
        let left = motion_x.unwrap_or_else(|| ((width - cover) / 2.0).max(0.0));
        // 底栏与内容列同宽，左右边界与主沉浸区对齐；不再把按钮挂在窗口角上。
        let bottom_rect = stage_bottom_rect(
            width,
            height,
            false,
            stable_lyric_pad(self.viewport_width, self.viewport_height),
        );
        let side_panel = matches!(self.stage_view, StageView::Queue | StageView::Audio);
        let mut root = div().size_full().relative();
        if lyrics_on && !side_panel {
            // 竖屏「显示歌词」：封面缩到顶栏左侧，标题/更多贴其右，
            // 中部整块留给歌词，控件沉底并随闲置收起。
            root = root.child(self.amll_vertical_lyrics_column(width, height, chrome, window, cx));
        } else if !lyrics_on && !side_panel {
            root = root.child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(COMPACT_TOP))
                    .w(px(cover))
                    .child(self.amll_player_column(
                        cover,
                        height,
                        controls_h,
                        true,
                        chrome,
                        shared_hidden,
                        !shared_hidden,
                        window,
                        cx,
                    )),
            );
        }
        if side_panel {
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
            root = root.child(panel);
        }
        root.child(
            div()
                .absolute()
                .left(px(bottom_rect.x))
                .top(px(bottom_rect.y))
                .w(px(bottom_rect.w))
                .h(px(bottom_rect.h))
                // 底栏在沉浸页常驻；只有向 Dock 收起时才淡出。
                // 原先直接用 dock_opacity 会让满屏态（p=0）整个底栏不可见。
                .opacity(1.0 - dock_opacity)
                .child(self.immersive_bottom_row(bottom_rect.w, chrome, cx)),
        )
        .into_any_element()
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
        let chrome = self.stage_chrome_visibility(now);
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
        let controls_h = (bottom_row_top - gap - controls_top).max(0.0);
        let info_w = width * 0.45;
        let lyric_left = info_w + gap;

        // 布局 1 固定为「左播放列 + 右歌词」；歌词开关切换时会立即落到
        // 布局 2，因此这里不再维护居中分支。
        let split_x = wide_column_x(width, height);
        let wide_motion = self
            .stage_motion
            .filter(|motion| motion.active(now) && motion.touches_wide())
            .filter(|motion| motion.from.is_valid() && motion.to.is_valid());
        let stage = wide_motion.map(|motion| motion.geometry(now));
        let mut column_x = split_x;
        if let Some(geometry) = stage {
            column_x = geometry.cover.x;
        }
        let shared_hidden = stage.is_some() || dock_shared;

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
        // —— 顶部窗口拖动带：整条空白头部都可拖动窗口（AMLL drag-area）。
        //    中间与封面同宽的区域是下拉关闭把手，其余宽度用来移动窗口。
        // Dock→全屏的位移由外层卡片承担，页面内部不再二次偏移。
        let sheet_offset = 0.0;
        let pill_top = drag + gap + 16.0;
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
        // 避免顶部的 EQ 控件被手势抢走。
        let handle_w = cover.max(120.0).min(width);
        let handle_top = (pill_top - 20.0).max(0.0);
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
                .child(self.amll_pill(
                    handle_w / 2.0,
                    pill_top - handle_top,
                    "immersive-pill",
                    cover,
                ))
        });

        // —— 共用 AMLL 播放器列：封面 / 信息 / 进度 / 传输 / 音量 ——
        let info_column = div()
            .absolute()
            .left(px(column_x))
            .top(px(cover_top))
            .w(px(cover))
            .child(self.amll_player_column(
                cover,
                height,
                controls_h,
                !dock_shared,
                chrome,
                shared_hidden,
                !shared_hidden,
                window,
                cx,
            ));

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

        // —— 统一的底部控制行：与布局 2 共用同一个构建函数 ——
        let bottom_row = div()
            .absolute()
            .left(px(bottom_rect.x))
            .top(px(bottom_rect.y))
            .w(px(bottom_rect.w))
            .h(px(bottom_rect.h))
            .opacity(1.0 - dock_opacity)
            .child(self.immersive_bottom_row(bottom_rect.w, chrome, cx));

        let stage_el = stage.map(|geometry| self.render_shared_stage(geometry));

        // 页面本体：下拉时整块下移，露出背后的主面板。拖动带在下、pill
        // 把手在上（occlude），因此只有 pill 附近是下拉手势，其余顶部拖窗口。
        let sheet = div()
            .id("immersive-sheet")
            .absolute()
            .left_0()
            .top(px(sheet_offset))
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .bg(gpui::black())
            .child(right_area)
            .child(gpui::batch_layer(info_column))
            .when_some(stage_el, |sheet, stage| sheet.child(stage))
            .child(gpui::batch_layer(bottom_row))
            .when_some(drag_band, |sheet, band| sheet.child(band))
            .when_some(dismiss_handle, |sheet, handle| sheet.child(handle));

        div()
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
                        if app.sheet_drag.is_some_and(|drag| !drag.animating()) {
                            app.release_sheet_drag(height, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |app, _: &MouseUpEvent, _, cx| {
                    if app.sheet_drag.is_some() {
                        app.release_sheet_drag(height, cx);
                    }
                }),
            )
            .size_full()
            .relative()
            .overflow_hidden()
            .child(sheet)
            .into_any_element()
    }
}
