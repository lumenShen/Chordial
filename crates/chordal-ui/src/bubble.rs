//! 气泡提示：hover 延迟出现，触屏长按也能看到，用来标注仅图标按钮的功能。
//!
//! 与普通浮层不同，气泡挂在窗口坐标上：触发按钮的 bounds 在 prepaint
//! 阶段捕获，气泡用 `deferred` + `anchored` 在整棵树之后绘制，因此不会被
//! 标题栏、滚动容器或卡片的 `overflow_hidden` 裁掉。
//!
//! 点击由本组件转发而不是留在触发元素上：长按抬手后需要丢弃这一次点击，
//! 否则触屏上「按住看功能」会顺手触发按钮动作。

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnchoredPositionMode, AnyElement, App, Bounds, BoxShadow, ClickEvent, Component, Corner,
    Entity, Hsla, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels,
    Point, RenderOnce, SharedString, TextRun, Window, anchored, canvas, deferred, div, point, px,
};

use crate::motion::{MotionSpring, PROGRESS_TOL, current_tuning};
use crate::theme::Theme;

/// hover 后延迟出现，避免鼠标扫过一排图标时气泡乱闪。
const HOVER_DELAY: Duration = Duration::from_millis(280);
/// 触屏没有 hover，按住不动超过该时长才出现。
const LONG_PRESS_DELAY: Duration = Duration::from_millis(450);
const BUBBLE_HEIGHT: f32 = 29.0;
const BUBBLE_FONT_SIZE: f32 = 11.5;
const BUBBLE_PAD_X: f32 = 11.0;
const BUBBLE_MIN_W: f32 = 38.0;
const BUBBLE_MAX_W: f32 = 320.0;
const TAIL_W: f32 = 14.0;
const TAIL_H: f32 = 6.0;
/// 气泡与触发按钮之间留出的空隙（不含尾巴本身）。
const GAP: f32 = 3.0;
/// 气泡距窗口边缘的安全距离。
const EDGE: f32 = 8.0;
/// 按住后移动超过该距离视为拖动，取消长按。
const MOVE_SLOP: f32 = 8.0;
/// 出现/消失共用的淡入淡出基准时长；反方向直接打断同一条弹簧。
const FADE_BASE_MS: f32 = 120.0;

/// 气泡配色。单独抽出来是因为均衡器页使用 `EqPalette`，
/// 不直接沿用全局 `Theme`。
#[derive(Clone)]
pub struct BubbleStyle {
    pub bg: Hsla,
    pub fg: Hsla,
    pub shadow: Hsla,
    pub font_family: SharedString,
}

impl BubbleStyle {
    /// 反色胶囊：浅色主题下是墨黑底白字，深色主题下是白底黑字，
    /// 与主按钮使用同一组 `accent` / `on_accent` 令牌。
    pub fn for_theme(theme: &Theme) -> Self {
        Self {
            bg: theme.accent,
            fg: theme.on_accent,
            shadow: theme.shadow_strong,
            font_family: theme.font_family.clone(),
        }
    }

    /// 沉浸层固定深色背景（AMLL 播放页）上的气泡：始终白底深字，
    /// 不跟随窗口明暗，避免浅色主题下出现看不见的黑气泡。
    pub fn on_dark(font_family: SharedString) -> Self {
        Self {
            bg: gpui::white(),
            fg: crate::theme::hex(0x141418),
            shadow: gpui::black().opacity(0.38),
            font_family,
        }
    }
}

struct BubbleState {
    hovered: Cell<bool>,
    pressed: Cell<bool>,
    visible: Cell<bool>,
    /// 长按已经展示过气泡：接下来这次 mouse click 要丢弃。
    suppress_click: Cell<bool>,
    hover_epoch: Cell<u64>,
    press_epoch: Cell<u64>,
    /// 触发元素在窗口坐标系中的 bounds。
    bounds: Cell<Option<Bounds<Pixels>>>,
    press_origin: Cell<Option<Point<Pixels>>>,
    /// 0→1 淡入 / 1→0 淡出共用的弹簧；收起时元素会留到它收敛。
    shown_at: Cell<Option<MotionSpring>>,
}

impl BubbleState {
    fn new() -> Self {
        Self {
            hovered: Cell::new(false),
            pressed: Cell::new(false),
            visible: Cell::new(false),
            suppress_click: Cell::new(false),
            hover_epoch: Cell::new(0),
            press_epoch: Cell::new(0),
            bounds: Cell::new(None),
            press_origin: Cell::new(None),
            shown_at: Cell::new(None),
        }
    }

    fn bump_hover(&self) -> u64 {
        let next = self.hover_epoch.get().wrapping_add(1);
        self.hover_epoch.set(next);
        next
    }

    fn bump_press(&self) -> u64 {
        let next = self.press_epoch.get().wrapping_add(1);
        self.press_epoch.set(next);
        next
    }

    fn cancel_press(&self) {
        self.pressed.set(false);
        self.press_origin.set(None);
        self.bump_press();
    }
}

/// 换目标而不是重建弹簧：中途反打保留当前位置与速度。
fn retarget_fade(state: &mut BubbleState, to: f32, cx: &mut App) {
    let now = Instant::now();
    if let Some(spring) = state.shown_at.get_mut() {
        spring.retarget(to, now);
    } else {
        let from = if to > 0.0 { 0.0 } else { 1.0 };
        let spring = current_tuning(cx).spring(
            FADE_BASE_MS,
            from,
            to,
            0.0,
            now,
            PROGRESS_TOL.0,
            PROGRESS_TOL.1,
        );
        state.shown_at.set(Some(spring));
    }
}

fn show(state: &Entity<BubbleState>, cx: &mut App) {
    state.update(cx, |state, cx| {
        state.visible.set(true);
        retarget_fade(state, 1.0, cx);
        cx.notify();
    });
}

fn hide(state: &Entity<BubbleState>, cx: &mut App) {
    state.update(cx, |state, cx| {
        if !state.visible.get() {
            return;
        }
        // 不立刻撤掉元素：渲染层会按弹簧值淡出，收敛后才不再绘制。
        state.visible.set(false);
        retarget_fade(state, 0.0, cx);
        cx.notify();
    });
}

/// 在窗口上下文中延迟执行；`Window::spawn` 的任务在窗口销毁时自动取消。
fn after_delay(
    window: &mut Window,
    cx: &mut App,
    delay: Duration,
    f: impl FnOnce(&mut App) + 'static,
) {
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(delay).await;
            let _ = cx.update(|_, cx| f(cx));
        })
        .detach();
}

struct Bubble {
    style: BubbleStyle,
    id: SharedString,
    label: SharedString,
    trigger: Option<AnyElement>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl RenderOnce for Bubble {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Bubble {
            style,
            id,
            label,
            mut trigger,
            on_click,
        } = self;
        // 状态随元素树位置存续：同一个图标按钮重绘不会丢 hover / 长按状态。
        let state = window.use_keyed_state(
            SharedString::from(format!("bubble-state-{id}")),
            cx,
            |_, _| BubbleState::new(),
        );

        let mut wrapper = div()
            .on_children_prepainted({
                let state = state.clone();
                move |bounds, _, cx| {
                    if let Some(trigger_bounds) = bounds.first() {
                        state.update(cx, |state, _| state.bounds.set(Some(*trigger_bounds)));
                    }
                }
            })
            .id(SharedString::from(format!("bubble-anchor-{id}")))
            .relative()
            .flex_none();

        // ---- hover：延迟出现，移出立即收起 ----
        {
            let state = state.clone();
            wrapper = wrapper.on_hover(move |hovered, window, cx| {
                let epoch = state.update(cx, |state, _| {
                    state.hovered.set(*hovered);
                    state.bump_hover()
                });
                if *hovered {
                    let state = state.clone();
                    after_delay(window, cx, HOVER_DELAY, move |cx| {
                        // epoch 不匹配说明指针已经移开又回来，这次延迟展示作废。
                        let should_show = state.update(cx, |state, _| {
                            state.hovered.get()
                                && state.hover_epoch.get() == epoch
                                && !state.visible.get()
                        });
                        if should_show {
                            show(&state, cx);
                        }
                    });
                } else {
                    state.update(cx, |state, _| state.cancel_press());
                    hide(&state, cx);
                }
            });
        }

        // ---- 长按：按住不动 450ms 出现 ----
        {
            let state = state.clone();
            wrapper = wrapper.on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    let epoch = state.update(cx, |state, _| {
                        state.pressed.set(true);
                        state.suppress_click.set(false);
                        state.press_origin.set(Some(event.position));
                        state.bump_press()
                    });
                    let state = state.clone();
                    after_delay(window, cx, LONG_PRESS_DELAY, move |cx| {
                        // epoch 不匹配说明手势已经取消，长按不再展示。
                        let should_show = state.update(cx, |state, _| {
                            if !state.pressed.get() || state.press_epoch.get() != epoch {
                                return false;
                            }
                            state.suppress_click.set(true);
                            !state.visible.get()
                        });
                        if should_show {
                            show(&state, cx);
                        }
                    });
                },
            );
        }

        // 按住后移动（滚动/拖动）超过死区就取消，避免气泡跟着手势乱跑。
        {
            let state = state.clone();
            wrapper = wrapper.on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                let moved = state.update(cx, |state, _| {
                    if !state.pressed.get() {
                        return false;
                    }
                    let Some(origin) = state.press_origin.get() else {
                        return false;
                    };
                    let dx = f32::from(event.position.x - origin.x);
                    let dy = f32::from(event.position.y - origin.y);
                    if dx * dx + dy * dy > MOVE_SLOP * MOVE_SLOP {
                        state.cancel_press();
                        true
                    } else {
                        false
                    }
                });
                if moved {
                    hide(&state, cx);
                }
            });
        }

        // 松手后如果指针已经离开，气泡立即收起；仍悬停则保留到移出。
        {
            let state = state.clone();
            let finish = move |cx: &mut App| {
                state.update(cx, |state, _| {
                    state.pressed.set(false);
                    state.press_origin.set(None);
                    state.bump_press();
                });
                if !state.read(cx).hovered.get() {
                    hide(&state, cx);
                }
            };
            let finish_up = finish.clone();
            wrapper = wrapper
                .on_mouse_up(MouseButton::Left, move |_, _, cx| finish_up(cx))
                .on_mouse_up_out(MouseButton::Left, move |_, _, cx| finish(cx));
        }

        // 长按后的抬手不再触发按钮本身的动作。
        if let Some(on_click) = on_click {
            let state = state.clone();
            wrapper = wrapper.on_click(move |event, window, cx| {
                if matches!(event, ClickEvent::Mouse(_)) && state.read(cx).suppress_click.get() {
                    state.update(cx, |state, _| state.suppress_click.set(false));
                    return;
                }
                // 正常点击意味着用户已经知道这个按钮做什么，气泡可以收起了。
                hide(&state, cx);
                (on_click)(event, window, cx);
            });
        }

        wrapper = wrapper.child(trigger.take().expect("bubble trigger must be present"));

        let (visible, trigger_bounds, fade) = {
            let snapshot = state.read(cx);
            (
                snapshot.visible.get(),
                snapshot.bounds.get(),
                snapshot.shown_at.get(),
            )
        };
        let now = Instant::now();
        // 收起时 visible 已经翻成 false，但弹簧还在跑，元素必须留到它收敛。
        let fade_active = fade.is_some_and(|spring| spring.active(now));

        if (visible || fade_active)
            && let Some(trigger_bounds) = trigger_bounds
        {
            let viewport = window.viewport_size();
            let text_w = measure_label(window, &style, &label);
            let width = (text_w + BUBBLE_PAD_X * 2.0).clamp(
                BUBBLE_MIN_W,
                BUBBLE_MAX_W.min((f32::from(viewport.width) - EDGE * 2.0).max(BUBBLE_MIN_W)),
            );
            let placement = place_bubble(
                f32::from(trigger_bounds.left()),
                f32::from(trigger_bounds.top()),
                f32::from(trigger_bounds.size.width),
                f32::from(trigger_bounds.size.height),
                f32::from(viewport.width),
                f32::from(viewport.height),
                width,
            );

            let progress = fade
                .map(|spring| spring.value(now).clamp(0.0, 1.0))
                .unwrap_or(if visible { 1.0 } else { 0.0 });
            if fade_active {
                window.request_animation_frame();
            }
            // 弹簧本身已给出观感曲线；不再叠加幂函数缓动，否则会出现二次加速。
            let opacity = progress;

            let bubble = render_bubble(
                &style,
                label,
                width,
                BUBBLE_HEIGHT,
                placement.tail_x,
                !placement.above,
                opacity,
            );
            wrapper = wrapper.child(
                deferred(
                    anchored()
                        .position(point(px(placement.left), px(placement.top)))
                        .anchor(Corner::TopLeft)
                        .position_mode(AnchoredPositionMode::Window)
                        .snap_to_window_with_margin(px(4.0))
                        .child(bubble),
                )
                .with_priority(100),
            );
        }

        wrapper
    }
}

/// 一次气泡定位的结果。抽成纯函数，方便单测窗口边缘/顶部标题栏等场景。
#[derive(Clone, Copy, Debug, PartialEq)]
struct BubblePlacement {
    left: f32,
    top: f32,
    tail_x: f32,
    above: bool,
}

fn place_bubble(
    trigger_left: f32,
    trigger_top: f32,
    trigger_w: f32,
    trigger_h: f32,
    viewport_w: f32,
    viewport_h: f32,
    width: f32,
) -> BubblePlacement {
    let height = BUBBLE_HEIGHT;
    let center_x = trigger_left + trigger_w * 0.5;

    // 气泡 + 尾巴 + 空隙的总高度；上方/下方都要放得下才不翻转。
    let span = height + TAIL_H + GAP;
    let room_above = trigger_top;
    let room_below = viewport_h - (trigger_top + trigger_h);
    let above = if room_above - EDGE >= span {
        true
    } else if room_below - EDGE >= span {
        false
    } else {
        room_above >= room_below
    };

    let top = if above {
        (trigger_top - span).max(EDGE)
    } else {
        (trigger_top + trigger_h + span)
            .min(viewport_h - EDGE - height)
            .max(EDGE + TAIL_H)
    };

    // 图标在右半屏时向左展开、尾巴靠右；反之向右展开。
    // 这样气泡不会横穿图标，也贴近「右侧按钮 + 右上尾巴」的参考稿。
    let mut left = if center_x > viewport_w * 0.5 {
        trigger_left + trigger_w - width
    } else {
        trigger_left
    };
    left = left.clamp(EDGE, (viewport_w - width - EDGE).max(EDGE));

    let tail_half = TAIL_W * 0.5;
    let tail_x = (center_x - left).clamp(
        tail_half + 2.0,
        (width - tail_half - 2.0).max(tail_half + 2.0),
    );

    BubblePlacement {
        left,
        top,
        tail_x,
        above,
    }
}

/// 仅作展示的气泡：长按后不会丢弃触发元素自己的点击。
#[allow(dead_code)] // 组件 API；当前所有接入点都需要点击转发。
pub fn bubble(
    style: BubbleStyle,
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    trigger: impl IntoElement,
) -> AnyElement {
    Component::new(Bubble {
        style,
        id: id.into(),
        label: label.into(),
        trigger: Some(trigger.into_any_element()),
        on_click: None,
    })
    .into_any_element()
}

/// 带点击转发的气泡：长按查看功能后抬手，不会误触按钮。
pub fn bubble_button<F>(
    style: BubbleStyle,
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    trigger: impl IntoElement,
    on_click: F,
) -> AnyElement
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    Component::new(Bubble {
        style,
        id: id.into(),
        label: label.into(),
        trigger: Some(trigger.into_any_element()),
        on_click: Some(Rc::new(on_click)),
    })
    .into_any_element()
}

fn measure_label(window: &mut Window, style: &BubbleStyle, label: &str) -> f32 {
    if label.is_empty() {
        return 0.0;
    }
    let mut font = gpui::font(style.font_family.clone());
    font.weight = gpui::FontWeight::MEDIUM;
    let run = TextRun {
        len: label.len(),
        font,
        color: style.fg,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(label.to_string()),
        px(BUBBLE_FONT_SIZE),
        std::slice::from_ref(&run),
        None,
    );
    f32::from(line.width)
}

fn render_bubble(
    style: &BubbleStyle,
    label: SharedString,
    width: f32,
    height: f32,
    tail_x: f32,
    tail_on_top: bool,
    opacity: f32,
) -> AnyElement {
    let tail_color = style.bg;
    let tail = canvas(
        move |_bounds, _window, _cx| {},
        move |bounds, _prepaint, window, _cx| {
            paint_tail(bounds, tail_color, tail_on_top, window);
        },
    )
    .absolute()
    .left(px(tail_x - TAIL_W * 0.5))
    .w(px(TAIL_W))
    .h(px(TAIL_H));
    let tail = if tail_on_top {
        tail.top(px(-TAIL_H + 1.0))
    } else {
        tail.bottom(px(-TAIL_H + 1.0))
    };

    div()
        .w(px(width))
        .h(px(height))
        .rounded(px(height * 0.5))
        .bg(style.bg)
        .shadow(vec![BoxShadow {
            color: style.shadow,
            offset: point(px(0.0), px(3.0)),
            blur_radius: px(10.0),
            spread_radius: px(0.0),
        }])
        .flex()
        .items_center()
        .justify_center()
        .font_family(style.font_family.clone())
        .text_size(px(BUBBLE_FONT_SIZE))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(style.fg)
        .whitespace_nowrap()
        .opacity(opacity)
        .child(
            div()
                .max_w(px((width - BUBBLE_PAD_X * 2.0).max(1.0)))
                .overflow_hidden()
                .text_ellipsis()
                .child(label),
        )
        .child(tail)
        .into_any_element()
}

/// 尾巴：底边压在气泡上 1px，尖端用二次曲线收圆。
fn paint_tail(bounds: Bounds<Pixels>, color: Hsla, on_top: bool, window: &mut Window) {
    let center_x = bounds.center().x;
    let (base_y, tip_y, apex_y) = if on_top {
        (
            bounds.bottom() + px(1.0),
            bounds.top() + px(2.0),
            bounds.top() - px(2.0),
        )
    } else {
        (
            bounds.top() - px(1.0),
            bounds.bottom() - px(2.0),
            bounds.bottom() + px(2.0),
        )
    };
    let mut builder = PathBuilder::fill();
    builder.move_to(point(bounds.left(), base_y));
    builder.line_to(point(center_x - px(2.0), tip_y));
    builder.curve_to(point(center_x + px(2.0), tip_y), point(center_x, apex_y));
    builder.line_to(point(bounds.right(), base_y));
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titlebar_button_opens_below_with_tail_near_right() {
        // 1440×900 窗口，标题栏右侧 26px 图标：上方放不下，应翻到下方，
        // 并向左展开，尾巴指回按钮中心。
        let p = place_bubble(1390.0, 7.0, 26.0, 26.0, 1440.0, 900.0, 70.0);
        assert!(!p.above);
        assert_eq!(p.top, 7.0 + 26.0 + BUBBLE_HEIGHT + TAIL_H + GAP);
        assert_eq!(p.left, 1390.0 + 26.0 - 70.0);
        assert!((p.tail_x - (70.0 - 26.0 * 0.5)).abs() < 0.01);
    }

    #[test]
    fn dock_button_opens_above() {
        // 底部 Dock 的图标：上方空间充足，气泡向上弹。
        let p = place_bubble(700.0, 800.0, 28.0, 28.0, 1440.0, 900.0, 80.0);
        assert!(p.above);
        assert_eq!(p.top, 800.0 - BUBBLE_HEIGHT - TAIL_H - GAP);
        // 按钮在左半屏，气泡左对齐并向右展开。
        assert_eq!(p.left, 700.0);
        assert!((p.tail_x - 14.0).abs() < 0.01);
    }

    #[test]
    fn bubbles_stay_inside_narrow_window() {
        // 360px 窄窗：左右两侧都钳进安全边距，尾巴仍在气泡内。
        let width = 320.0;
        let p = place_bubble(340.0, 40.0, 20.0, 20.0, 360.0, 640.0, width);
        assert!(p.left >= EDGE);
        assert!(p.left + width <= 360.0 - EDGE + 0.01);
        assert!(p.tail_x >= TAIL_W * 0.5 + 2.0 - 0.01);
        assert!(p.tail_x <= width - TAIL_W * 0.5 - 2.0 + 0.01);
    }

    #[test]
    fn very_short_viewport_does_not_panic() {
        let p = place_bubble(20.0, 20.0, 20.0, 20.0, 120.0, 40.0, 60.0);
        assert!(p.top.is_finite());
        assert!(p.left.is_finite());
    }
}
