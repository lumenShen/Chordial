//! 自绘图标：16×16 设计网格，按目标尺寸缩放。
//! 不使用 ▶/⏸ 等字符，避免字体把它们渲染成彩色 emoji。

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::TAU;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Hsla, IntoElement, Path, PathBuilder, Pixels, Styled, Window, canvas, div,
    point, px,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Search,
    Volume,
    VolumeMute,
    Queue,
    Lyrics,
    Sun,
    Moon,
    Close,
    Refresh,
    ChevronDown,
    Alert,
    Note,
    More,
    /// AMLL `MenuButton/icon_more.svg` 的比例（24 网格），用于播放列圆钮。
    MoreDot,
    Check,
    Expand,
    ChevronLeft,
    ChevronRight,
    Cast,
    /// 三段竖向推子（播放页 Dock 的均衡器入口）。
    Equalizer,
    Headphones,
    Person,
    Plus,
    Shuffle,
    Repeat,
    /// AMLL `repeat-one-active.svg` 的语义：循环箭头 + 中间的「1」。
    RepeatOne,
    Clock,
    Drive,
    Cloud,
    Menu,
    Layers,
    Download,
    Settings,
    Sliders,
    /// AMLL `icon_rewind.svg`：双左箭头（上一首）。
    Rewind,
    /// AMLL `icon_forward.svg`：双右箭头（下一首）。
    Forward,
    /// AMLL `icon_play.svg` 的三角比例（38 viewBox），用于播放列大按钮。
    PlayMedia,
    /// AMLL `icon_pause.svg` 的双竖条比例（38 viewBox）。
    PauseMedia,
    /// 音量低：只有喇叭本体（AMLL `icon_speaker.svg`）。
    VolumeLow,
    /// 音量高：喇叭 + 3 道声波（AMLL `icon_speaker_3.svg`）。
    VolumeHigh,
}

/// 图标动画状态。进度全部是 0..1 的**线性时间**，缓动在绘制时应用。
#[derive(Clone, Copy, Debug)]
pub struct IconMotion {
    /// AMLL `pressed-animation`：1 → 0.85 → 1.1 → 1，0.7s。
    /// 0 与 1 都是静止状态。
    pub press: f32,
    /// 上/下一首双箭头的入场合并动画，0.6s；`1.0` 表示静止（默认）。
    pub arrows: f32,
    /// 音量到达 0 / 1 时喇叭部件的弹跳，总窗口 0.75s。
    /// 0 与 1 都是静止状态。
    pub speaker: f32,
}

impl Default for IconMotion {
    fn default() -> Self {
        Self {
            press: 0.0,
            arrows: 1.0,
            speaker: 0.0,
        }
    }
}

thread_local! {
    /// 静止图标的已 tessellate 路径（局部坐标，原点 0,0）。
    /// 首次绘制时捕获，之后每帧只做平移 + 克隆，不再跑 lyon。
    static ICON_PATH_CACHE: RefCell<HashMap<(u8, u32), Rc<Vec<Path<Pixels>>>>> =
        RefCell::new(HashMap::new());
    static CAPTURE_PATHS: RefCell<Option<Vec<Path<Pixels>>>> = const { RefCell::new(None) };
}

fn emit_path(window: &mut Window, path: Path<Pixels>, color: Hsla) {
    let capturing = CAPTURE_PATHS.with(|capture| capture.borrow().is_some());
    if capturing {
        CAPTURE_PATHS.with(|capture| {
            if let Some(paths) = capture.borrow_mut().as_mut() {
                paths.push(path);
            }
        });
    } else {
        window.paint_path(path, color);
    }
}

pub fn icon(name: Icon, size: f32, color: Hsla) -> AnyElement {
    icon_motion(name, size, color, IconMotion::default())
}

pub fn icon_motion(name: Icon, size: f32, color: Hsla, motion: IconMotion) -> AnyElement {
    div()
        .w(px(size))
        .h(px(size))
        .child(
            canvas(
                move |bounds, _window, _cx| (bounds, name, color, size, motion),
                |bounds, (_, name, color, size, motion), window, _cx| {
                    paint_cached(bounds, name, color, size, motion, window);
                },
            )
            .size_full(),
        )
        .into_any_element()
}

/// 静止图标走几何缓存；只有按压/双箭头/喇叭弹跳等动画图标才每帧重建。
fn paint_cached(
    bounds: Bounds<Pixels>,
    name: Icon,
    color: Hsla,
    size: f32,
    motion: IconMotion,
    window: &mut Window,
) {
    let still = (motion.press <= 0.0 || motion.press >= 1.0)
        && motion.arrows >= 1.0
        && (motion.speaker <= 0.0 || motion.speaker >= 1.0);
    if !still {
        paint(bounds.origin, name, color, size, motion, window);
        return;
    }

    let key = (name as u8, size.to_bits());
    if let Some(paths) = ICON_PATH_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        for path in paths.iter() {
            window.paint_path_translated(path, bounds.origin, color);
        }
        return;
    }

    CAPTURE_PATHS.with(|capture| *capture.borrow_mut() = Some(Vec::new()));
    paint(point(px(0.0), px(0.0)), name, color, size, motion, window);
    let captured = CAPTURE_PATHS
        .with(|capture| capture.borrow_mut().take())
        .unwrap_or_default();
    let paths = Rc::new(captured);
    for path in paths.iter() {
        window.paint_path_translated(path, bounds.origin, color);
    }
    ICON_PATH_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() > 4096 {
            cache.clear();
        }
        cache.insert(key, paths);
    });
}

/// 2D 仿射变换：`p' = p * s + t`。
///
/// 只有缩放与平移，足够表达 AMLL 的 `scale()` / `translateX()` 组合；
/// 组合顺序与 CSS 变换列表一致（先应用的变换在右侧）。
#[derive(Clone, Copy)]
struct Xf {
    s: f32,
    tx: f32,
    ty: f32,
}

impl Xf {
    const IDENTITY: Self = Self {
        s: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    fn scale_about(s: f32, ox: f32, oy: f32) -> Self {
        Self {
            s,
            tx: ox * (1.0 - s),
            ty: oy * (1.0 - s),
        }
    }

    /// CSS `scale(s) translateX(tx)`：平移发生在缩放**内部**（平移量会被缩放）。
    fn scale_translate_about(s: f32, ox: f32, oy: f32, tx: f32) -> Self {
        Self {
            s,
            tx: ox * (1.0 - s) + s * tx,
            ty: oy * (1.0 - s),
        }
    }

    /// 先应用 `self`，再应用 `outer`。
    fn then(self, outer: Self) -> Self {
        Self {
            s: outer.s * self.s,
            tx: outer.s * self.tx + outer.tx,
            ty: outer.s * self.ty + outer.ty,
        }
    }

    fn apply(self, x: f32, y: f32) -> (f32, f32) {
        (self.s * x + self.tx, self.s * y + self.ty)
    }

    fn visible(self) -> bool {
        self.s.abs() > 0.002
    }
}

fn fill_poly(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    pts: &[(f32, f32)],
) {
    fill_poly_xf(window, color, origin, s, pts, Xf::IDENTITY);
}

fn fill_poly_xf(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    pts: &[(f32, f32)],
    xf: Xf,
) {
    if pts.len() < 3 || !xf.visible() {
        return;
    }
    let mut b = PathBuilder::fill();
    let at = |x: f32, y: f32| {
        let (x, y) = xf.apply(x, y);
        point(origin.x + px(x * s), origin.y + px(y * s))
    };
    b.move_to(at(pts[0].0, pts[0].1));
    for p in &pts[1..] {
        b.line_to(at(p.0, p.1));
    }
    b.close();
    if let Ok(path) = b.build() {
        emit_path(window, path, color);
    }
}

fn stroke_poly(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    pts: &[(f32, f32)],
) {
    stroke_poly_xf(window, color, origin, s, pts, Xf::IDENTITY);
}

fn stroke_poly_xf(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    pts: &[(f32, f32)],
    xf: Xf,
) {
    stroke_poly_xf_width(window, color, origin, s, pts, xf, 1.5);
}

fn stroke_poly_xf_width(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    pts: &[(f32, f32)],
    xf: Xf,
    width_grid: f32,
) {
    if pts.len() < 2 || !xf.visible() {
        return;
    }
    let mut b = PathBuilder::stroke(px((width_grid * s * xf.s.abs()).max(0.75)));
    let at = |x: f32, y: f32| {
        let (x, y) = xf.apply(x, y);
        point(origin.x + px(x * s), origin.y + px(y * s))
    };
    b.move_to(at(pts[0].0, pts[0].1));
    for p in &pts[1..] {
        b.line_to(at(p.0, p.1));
    }
    if let Ok(path) = b.build() {
        emit_path(window, path, color);
    }
}

const CIRCLE_STEPS: usize = 28;

/// 圆形填充：直接写入 path，避免每帧为 `circle_pts` 分配 Vec。
fn fill_circle(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    cx: f32,
    cy: f32,
    r: f32,
) {
    fill_circle_xf(window, color, origin, s, cx, cy, r, Xf::IDENTITY);
}

fn fill_circle_xf(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    cx: f32,
    cy: f32,
    r: f32,
    xf: Xf,
) {
    if r <= 0.0 || !xf.visible() {
        return;
    }
    let mut b = PathBuilder::fill();
    for i in 0..=CIRCLE_STEPS {
        let a = -TAU / 4.0 + i as f32 / CIRCLE_STEPS as f32 * TAU;
        let (x, y) = xf.apply(cx + r * a.cos(), cy + r * a.sin());
        let p = point(origin.x + px(x * s), origin.y + px(y * s));
        if i == 0 {
            b.move_to(p);
        } else {
            b.line_to(p);
        }
    }
    b.close();
    if let Ok(path) = b.build() {
        emit_path(window, path, color);
    }
}

/// 圆角矩形填充（用于 AMLL 双竖条的圆角端点）。
fn fill_round_rect_xf(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    radius: f32,
    xf: Xf,
) {
    if w <= 0.0 || h <= 0.0 || !xf.visible() {
        return;
    }
    let r = radius.min(w * 0.5).min(h * 0.5).max(0.0);
    let (x0, y0) = (cx - w * 0.5, cy - h * 0.5);
    let (x1, y1) = (cx + w * 0.5, cy + h * 0.5);
    let mut b = PathBuilder::fill();
    let mut push = |x: f32, y: f32, first: bool| {
        let (x, y) = xf.apply(x, y);
        let p = point(origin.x + px(x * s), origin.y + px(y * s));
        if first {
            b.move_to(p);
        } else {
            b.line_to(p);
        }
    };
    let steps = 6usize;
    let corners = [
        (x1 - r, y0 + r, -std::f32::consts::FRAC_PI_2),
        (x1 - r, y1 - r, 0.0),
        (x0 + r, y1 - r, std::f32::consts::FRAC_PI_2),
        (x0 + r, y0 + r, std::f32::consts::PI),
    ];
    let mut first = true;
    for (ccx, ccy, start) in corners {
        for i in 0..=steps {
            let a = start + i as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
            push(ccx + r * a.cos(), ccy + r * a.sin(), first);
            first = false;
        }
    }
    b.close();
    if let Ok(path) = b.build() {
        emit_path(window, path, color);
    }
}

/// 圆形描边：同样避免 Vec 分配。
fn stroke_circle(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    cx: f32,
    cy: f32,
    r: f32,
) {
    if r <= 0.0 {
        return;
    }
    let mut b = PathBuilder::stroke(px((1.5 * s).max(0.75)));
    for i in 0..=CIRCLE_STEPS {
        let a = -TAU / 4.0 + i as f32 / CIRCLE_STEPS as f32 * TAU;
        let p = point(
            origin.x + px((cx + r * a.cos()) * s),
            origin.y + px((cy + r * a.sin()) * s),
        );
        if i == 0 {
            b.move_to(p);
        } else {
            b.line_to(p);
        }
    }
    if let Ok(path) = b.build() {
        emit_path(window, path, color);
    }
}

/// 圆弧描边（`Refresh` / `Headphones` 的开口弧）。
fn stroke_arc(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    cx: f32,
    cy: f32,
    r: f32,
    start: f32,
    span: f32,
    steps: usize,
) {
    if steps == 0 {
        return;
    }
    let mut b = PathBuilder::stroke(px((1.5 * s).max(0.75)));
    for i in 0..=steps {
        let a = start + i as f32 / steps as f32 * span;
        let p = point(
            origin.x + px((cx + r * a.cos()) * s),
            origin.y + px((cy + r * a.sin()) * s),
        );
        if i == 0 {
            b.move_to(p);
        } else {
            b.line_to(p);
        }
    }
    if let Ok(path) = b.build() {
        emit_path(window, path, color);
    }
}

fn paint(
    origin: gpui::Point<Pixels>,
    name: Icon,
    color: Hsla,
    size: f32,
    motion: IconMotion,
    window: &mut Window,
) {
    let s = size / 16.0;
    let o = origin;
    let line = |window: &mut Window, pts: &[(f32, f32)]| stroke_poly(window, color, o, s, pts);
    // AMLL MediaButton 的 content 在挂载/播放状态切换时播放 pressed 弹簧；
    // 围绕图标中心缩放，对所有绘制（含描边宽度）生效。
    let base = Xf::scale_about(press_scale(motion.press), 8.0, 8.0);
    let line_xf =
        |window: &mut Window, pts: &[(f32, f32)]| stroke_poly_xf(window, color, o, s, pts, base);
    let fill_xf =
        |window: &mut Window, pts: &[(f32, f32)]| fill_poly_xf(window, color, o, s, pts, base);

    match name {
        Icon::Play => fill_xf(window, &[(5.0, 3.2), (12.4, 8.0), (5.0, 12.8)]),
        Icon::Pause => {
            fill_xf(window, &[(5.0, 3.2), (7.4, 3.2), (7.4, 12.8), (5.0, 12.8)]);
            fill_xf(
                window,
                &[(8.6, 3.2), (11.0, 3.2), (11.0, 12.8), (8.6, 12.8)],
            );
        }
        // AMLL 播放列专用：三角/双竖条占满 38 viewBox 的比例。
        Icon::PlayMedia => {
            fill_xf(window, &[(2.445, 0.813), (15.273, 8.0), (2.445, 15.187)])
        }
        Icon::PauseMedia => {
            fill_round_rect_xf(window, color, o, s, 4.596, 8.0, 4.767, 15.158, 2.2, base);
            fill_round_rect_xf(window, color, o, s, 11.406, 8.0, 4.767, 15.158, 2.2, base);
        }
        Icon::Rewind => paint_rewind(window, color, o, s, base, motion.arrows),
        Icon::Forward => paint_forward(window, color, o, s, base, motion.arrows),
        Icon::Prev => {
            fill_poly(
                window,
                color,
                o,
                s,
                &[(4.0, 3.2), (6.0, 3.2), (6.0, 12.8), (4.0, 12.8)],
            );
            fill_poly(
                window,
                color,
                o,
                s,
                &[(12.0, 3.2), (12.0, 12.8), (6.6, 8.0)],
            );
        }
        Icon::Next => {
            fill_poly(
                window,
                color,
                o,
                s,
                &[(12.0, 3.2), (10.0, 3.2), (10.0, 12.8), (12.0, 12.8)],
            );
            fill_poly(window, color, o, s, &[(4.0, 3.2), (4.0, 12.8), (9.4, 8.0)]);
        }
        Icon::Search => {
            stroke_circle(window, color, o, s, 7.0, 7.0, 4.3);
            line(window, &[(10.2, 10.2), (13.2, 13.2)]);
        }
        Icon::Volume => {
            fill_poly(
                window,
                color,
                o,
                s,
                &[
                    (3.0, 6.2),
                    (6.0, 6.2),
                    (9.0, 3.6),
                    (9.0, 12.4),
                    (6.0, 9.8),
                    (3.0, 9.8),
                ],
            );
            line(
                window,
                &[
                    (10.0, 5.2),
                    (11.5, 5.8),
                    (12.0, 8.0),
                    (11.5, 10.2),
                    (10.0, 10.8),
                ],
            );
        }
        Icon::VolumeMute => {
            fill_poly(
                window,
                color,
                o,
                s,
                &[
                    (3.0, 6.2),
                    (6.0, 6.2),
                    (9.0, 3.6),
                    (9.0, 12.4),
                    (6.0, 9.8),
                    (3.0, 9.8),
                ],
            );
            line(window, &[(10.4, 6.4), (13.4, 9.4)]);
            line(window, &[(13.4, 6.4), (10.4, 9.4)]);
        }
        Icon::VolumeLow => paint_speaker(window, color, o, s, motion.speaker, false),
        Icon::VolumeHigh => paint_speaker(window, color, o, s, motion.speaker, true),
        Icon::Queue => {
            line(window, &[(3.0, 4.0), (13.0, 4.0)]);
            line(window, &[(3.0, 8.0), (10.0, 8.0)]);
            line(window, &[(3.0, 12.0), (13.0, 12.0)]);
        }
        Icon::Lyrics => {
            line(window, &[(3.0, 4.0), (13.0, 4.0)]);
            line(window, &[(3.0, 8.0), (11.0, 8.0)]);
            line(window, &[(3.0, 12.0), (8.0, 12.0)]);
            fill_circle(window, color, o, s, 11.4, 12.0, 0.85);
        }
        Icon::Sun => {
            stroke_circle(window, color, o, s, 8.0, 8.0, 3.0);
            for i in 0..8 {
                let a = i as f32 / 8.0 * TAU;
                line(
                    window,
                    &[
                        (8.0 + 5.0 * a.cos(), 8.0 + 5.0 * a.sin()),
                        (8.0 + 6.6 * a.cos(), 8.0 + 6.6 * a.sin()),
                    ],
                );
            }
        }
        Icon::Moon => fill_poly(
            window,
            color,
            o,
            s,
            &[
                (10.2, 2.8),
                (8.0, 3.2),
                (6.2, 5.0),
                (5.6, 7.4),
                (6.4, 9.8),
                (8.2, 11.6),
                (10.6, 12.4),
                (9.2, 10.4),
                (8.8, 8.0),
                (9.4, 5.6),
            ],
        ),
        Icon::Close => {
            line(window, &[(4.0, 4.0), (12.0, 12.0)]);
            line(window, &[(12.0, 4.0), (4.0, 12.0)]);
        }
        Icon::Refresh => {
            stroke_arc(
                window,
                color,
                o,
                s,
                8.0,
                8.0,
                4.6,
                -TAU * 0.15,
                TAU * 0.8,
                24,
            );
            fill_poly(window, color, o, s, &[(12.8, 2.6), (13.4, 7.0), (9.2, 5.8)]);
        }
        Icon::ChevronDown => {
            line(window, &[(4.5, 6.5), (8.0, 10.0)]);
            line(window, &[(11.5, 6.5), (8.0, 10.0)]);
            fill_circle(window, color, o, s, 8.0, 10.0, 0.85);
        }
        Icon::Alert => {
            line(window, &[(8.0, 2.6), (14.0, 13.0), (2.0, 13.0), (8.0, 2.6)]);
            line(window, &[(8.0, 6.2), (8.0, 9.4)]);
            fill_circle(window, color, o, s, 8.0, 11.4, 0.8);
        }
        Icon::Note => {
            fill_circle(window, color, o, s, 5.4, 11.4, 2.2);
            line(window, &[(7.4, 11.0), (7.4, 3.8), (12.0, 4.8)]);
        }
        Icon::More => {
            for x in [3.6, 8.0, 12.4] {
                fill_circle(window, color, o, s, x, 8.0, 1.1);
            }
        }
        Icon::MoreDot => {
            // AMLL icon_more.svg：24 网格中圆心 x=5.5/12.31/19.3、
            // 半径 1.1；换算到 16 网格。
            for x in [3.67, 8.21, 12.87] {
                fill_circle(window, color, o, s, x, 8.21, 0.733);
            }
        }
        Icon::Check => {
            line(window, &[(3.5, 8.2), (6.6, 11.2), (12.4, 4.6)]);
        }
        Icon::Expand => {
            line(window, &[(6.0, 3.0), (3.0, 3.0), (3.0, 6.0)]);
            line(window, &[(10.0, 3.0), (13.0, 3.0), (13.0, 6.0)]);
            line(window, &[(3.0, 10.0), (3.0, 13.0), (6.0, 13.0)]);
            line(window, &[(13.0, 10.0), (13.0, 13.0), (10.0, 13.0)]);
        }
        Icon::ChevronLeft => {
            line(window, &[(11.0, 4.5), (7.5, 8.0), (11.0, 11.5)]);
        }
        Icon::ChevronRight => {
            line(window, &[(5.0, 4.5), (8.5, 8.0), (5.0, 11.5)]);
        }
        Icon::Cast => {
            line(
                window,
                &[
                    (2.6, 6.2),
                    (2.6, 4.0),
                    (13.4, 4.0),
                    (13.4, 12.0),
                    (9.0, 12.0),
                ],
            );
            fill_circle(window, color, o, s, 3.2, 11.0, 0.75);
            line(window, &[(2.8, 8.0), (5.8, 11.0)]);
            line(window, &[(2.8, 4.6), (8.6, 10.4)]);
        }
        Icon::Equalizer => {
            for (x, y) in [(4.2, 9.6), (8.0, 5.2), (11.8, 8.0)] {
                line(window, &[(x, 3.2), (x, 12.8)]);
                fill_circle(window, color, o, s, x, y, 1.25);
            }
        }
        Icon::Headphones => {
            stroke_arc(
                window,
                color,
                o,
                s,
                8.0,
                8.4,
                5.2,
                std::f32::consts::PI,
                std::f32::consts::PI,
                28,
            );
            line(window, &[(2.8, 8.4), (2.8, 11.6)]);
            line(window, &[(13.2, 8.4), (13.2, 11.6)]);
            line(window, &[(2.8, 11.6), (5.0, 11.6), (5.0, 9.2), (2.8, 9.2)]);
            line(
                window,
                &[(13.2, 11.6), (11.0, 11.6), (11.0, 9.2), (13.2, 9.2)],
            );
        }
        Icon::Person => {
            stroke_circle(window, color, o, s, 8.0, 5.6, 2.4);
            let arc: Vec<_> = (0..20)
                .map(|i| {
                    let a = std::f32::consts::PI + i as f32 / 20.0 * std::f32::consts::PI;
                    (8.0 + 4.6 * a.cos(), 13.4 + 3.6 * a.sin())
                })
                .collect();
            line(window, &arc);
        }
        Icon::Plus => {
            line(window, &[(8.0, 3.4), (8.0, 12.6)]);
            line(window, &[(3.4, 8.0), (12.6, 8.0)]);
        }
        Icon::Shuffle => {
            line_xf(window, &[(3.0, 4.5), (6.6, 4.5), (9.4, 11.5), (12.2, 11.5)]);
            line_xf(window, &[(3.0, 11.5), (6.6, 11.5), (9.4, 4.5), (12.2, 4.5)]);
            fill_xf(window, &[(12.2, 3.0), (14.2, 4.5), (12.2, 6.0)]);
            fill_xf(window, &[(12.2, 10.0), (14.2, 11.5), (12.2, 13.0)]);
        }
        Icon::Repeat => {
            line_xf(window, &[(4.0, 5.0), (12.0, 5.0), (12.0, 7.0)]);
            fill_xf(window, &[(10.6, 7.0), (12.0, 9.2), (13.4, 7.0)]);
            line_xf(window, &[(12.0, 11.0), (4.0, 11.0), (4.0, 9.0)]);
            fill_xf(window, &[(2.6, 9.0), (4.0, 6.8), (5.4, 9.0)]);
        }
        Icon::RepeatOne => {
            line_xf(window, &[(3.6, 4.6), (12.4, 4.6), (12.4, 6.6)]);
            fill_xf(window, &[(11.0, 6.6), (12.4, 8.8), (13.8, 6.6)]);
            line_xf(window, &[(12.4, 11.4), (3.6, 11.4), (3.6, 9.4)]);
            fill_xf(window, &[(2.2, 9.4), (3.6, 7.2), (5.0, 9.4)]);
            // 中间的单曲「1」。
            line_xf(window, &[(7.1, 7.2), (8.0, 6.4), (8.0, 9.6)]);
        }
        Icon::Clock => {
            stroke_circle(window, color, o, s, 8.0, 8.0, 5.2);
            line(window, &[(8.0, 4.9), (8.0, 8.2), (10.6, 9.6)]);
        }
        Icon::Drive => {
            line(
                window,
                &[
                    (2.8, 5.4),
                    (13.2, 5.4),
                    (14.2, 8.0),
                    (14.2, 11.0),
                    (1.8, 11.0),
                    (1.8, 8.0),
                    (2.8, 5.4),
                ],
            );
            line(window, &[(4.2, 9.2), (7.2, 9.2)]);
            fill_circle(window, color, o, s, 11.4, 9.2, 0.7);
        }
        Icon::Cloud => {
            line(
                window,
                &[
                    (4.8, 12.2),
                    (3.8, 11.2),
                    (3.6, 9.8),
                    (4.2, 8.6),
                    (5.4, 7.9),
                    (6.8, 8.0),
                    (7.6, 7.0),
                    (8.8, 6.4),
                    (10.2, 6.6),
                    (11.2, 7.6),
                    (11.6, 8.8),
                    (12.4, 9.2),
                    (12.8, 10.4),
                    (12.4, 11.5),
                    (11.4, 12.2),
                    (4.8, 12.2),
                ],
            );
        }
        Icon::Menu => {
            line(window, &[(3.0, 5.0), (13.0, 5.0)]);
            line(window, &[(3.0, 8.0), (13.0, 8.0)]);
            line(window, &[(3.0, 11.0), (13.0, 11.0)]);
        }
        Icon::Layers => {
            line(
                window,
                &[(8.0, 2.8), (13.6, 6.0), (8.0, 9.2), (2.4, 6.0), (8.0, 2.8)],
            );
            line(window, &[(2.8, 8.6), (8.0, 11.6)]);
            line(window, &[(13.2, 8.6), (8.0, 11.6)]);
            line(window, &[(2.8, 11.0), (8.0, 14.0)]);
            line(window, &[(13.2, 11.0), (8.0, 14.0)]);
        }
        Icon::Download => {
            line(window, &[(8.0, 2.8), (8.0, 10.0)]);
            fill_poly(window, color, o, s, &[(5.0, 7.2), (8.0, 10.6), (11.0, 7.2)]);
            line(window, &[(3.0, 12.4), (13.0, 12.4)]);
        }
        Icon::Settings => {
            stroke_circle(window, color, o, s, 8.0, 8.0, 4.6);
            stroke_circle(window, color, o, s, 8.0, 8.0, 1.5);
            for i in 0..8 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                line(
                    window,
                    &[
                        (8.0 + 5.4 * a.cos(), 8.0 + 5.4 * a.sin()),
                        (8.0 + 6.6 * a.cos(), 8.0 + 6.6 * a.sin()),
                    ],
                );
            }
        }
        Icon::Sliders => {
            line(window, &[(2.6, 5.0), (13.4, 5.0)]);
            line(window, &[(2.6, 11.0), (13.4, 11.0)]);
            fill_circle(window, color, o, s, 6.0, 5.0, 1.4);
            fill_circle(window, color, o, s, 10.0, 11.0, 1.4);
        }
    }
}

// ---------------------------------------------------------------------------
// AMLL 控件动画
// ---------------------------------------------------------------------------

/// AMLL `icon_forward.svg` / `icon_rewind.svg` 的精确轮廓。
///
/// 路径按 134 viewBox 采样（三次贝塞尔每段 10 步）后换算到 16 网格：
/// 左箭头 x=3.463..8.239、右箭头 x=8.239..13.015，y=5.336..10.664。
/// 两个箭头之间的水平位移 40/134*16 ≈ 4.776 与 AMLL 的
/// `translateX(40px)` 关键帧一致。
const MEDIA_ARROW_LEFT: [(f32, f32); 93] = [
    (7.4030, 7.1727), (7.5371, 7.2503), (7.6575, 7.3208), (7.7648, 7.3852),
    (7.8594, 7.4447), (7.9418, 7.5003), (8.0125, 7.5531), (8.0719, 7.6041),
    (8.1205, 7.6544), (8.1588, 7.7051), (8.1872, 7.7572), (8.2058, 7.8041),
    (8.2202, 7.8522), (8.2305, 7.9010), (8.2367, 7.9504), (8.2388, 8.0000),
    (8.2367, 8.0496), (8.2305, 8.0990), (8.2202, 8.1478), (8.2058, 8.1958),
    (8.1872, 8.2428), (8.1588, 8.2949), (8.1205, 8.3456), (8.0719, 8.3959),
    (8.0125, 8.4469), (7.9418, 8.4997), (7.8594, 8.5553), (7.7648, 8.6148),
    (7.6575, 8.6792), (7.5371, 8.7497), (7.4030, 8.8272), (4.8955, 10.2749),
    (4.7613, 10.3523), (4.6401, 10.4214), (4.5306, 10.4820), (4.4318, 10.5342),
    (4.3424, 10.5778), (4.2614, 10.6126), (4.1875, 10.6385), (4.1197, 10.6555),
    (4.0566, 10.6633), (3.9973, 10.6619), (3.9473, 10.6545), (3.8985, 10.6429),
    (3.8510, 10.6275), (3.8052, 10.6082), (3.7612, 10.5851), (3.7193, 10.5585),
    (3.6796, 10.5285), (3.6425, 10.4951), (3.6081, 10.4586), (3.5767, 10.4190),
    (3.5458, 10.3683), (3.5211, 10.3099), (3.5018, 10.2426), (3.4873, 10.1657),
    (3.4769, 10.0781), (3.4700, 9.9789), (3.4658, 9.8673), (3.4636, 9.7421),
    (3.4628, 9.6026), (3.4627, 9.4477), (3.4627, 6.5523), (3.4628, 6.3974),
    (3.4636, 6.2579), (3.4658, 6.1327), (3.4700, 6.0210), (3.4769, 5.9219),
    (3.4873, 5.8343), (3.5018, 5.7574), (3.5211, 5.6901), (3.5458, 5.6316),
    (3.5767, 5.5810), (3.6081, 5.5414), (3.6425, 5.5048), (3.6796, 5.4715),
    (3.7193, 5.4414), (3.7612, 5.4148), (3.8052, 5.3918), (3.8510, 5.3725),
    (3.8985, 5.3570), (3.9473, 5.3455), (3.9973, 5.3381), (4.0566, 5.3367),
    (4.1197, 5.3445), (4.1875, 5.3615), (4.2614, 5.3874), (4.3424, 5.4222),
    (4.4318, 5.4657), (4.5306, 5.5179), (4.6401, 5.5786), (4.7613, 5.6477),
    (4.8955, 5.7251),
];

const MEDIA_ARROW_RIGHT: [(f32, f32); 93] = [
    (12.1791, 7.1727), (12.3132, 7.2503), (12.4336, 7.3208), (12.5409, 7.3852),
    (12.6356, 7.4447), (12.7179, 7.5003), (12.7886, 7.5531), (12.8480, 7.6041),
    (12.8966, 7.6544), (12.9349, 7.7051), (12.9633, 7.7572), (12.9819, 7.8041),
    (12.9963, 7.8522), (13.0067, 7.9010), (13.0129, 7.9504), (13.0149, 8.0000),
    (13.0129, 8.0496), (13.0067, 8.0990), (12.9963, 8.1478), (12.9819, 8.1958),
    (12.9633, 8.2428), (12.9349, 8.2949), (12.8966, 8.3456), (12.8480, 8.3959),
    (12.7886, 8.4469), (12.7179, 8.4997), (12.6356, 8.5553), (12.5409, 8.6148),
    (12.4336, 8.6792), (12.3132, 8.7497), (12.1791, 8.8272), (9.6716, 10.2749),
    (9.5374, 10.3523), (9.4162, 10.4214), (9.3067, 10.4820), (9.2079, 10.5342),
    (9.1185, 10.5778), (9.0375, 10.6126), (8.9636, 10.6385), (8.8958, 10.6555),
    (8.8328, 10.6633), (8.7734, 10.6619), (8.7234, 10.6545), (8.6746, 10.6429),
    (8.6272, 10.6275), (8.5813, 10.6082), (8.5373, 10.5851), (8.4954, 10.5585),
    (8.4557, 10.5285), (8.4186, 10.4951), (8.3842, 10.4586), (8.3528, 10.4190),
    (8.3219, 10.3683), (8.2972, 10.3099), (8.2779, 10.2426), (8.2634, 10.1657),
    (8.2531, 10.0781), (8.2461, 9.9789), (8.2419, 9.8673), (8.2397, 9.7421),
    (8.2389, 9.6026), (8.2388, 9.4477), (8.2388, 6.5523), (8.2389, 6.3974),
    (8.2397, 6.2579), (8.2419, 6.1327), (8.2461, 6.0210), (8.2531, 5.9219),
    (8.2634, 5.8343), (8.2779, 5.7574), (8.2972, 5.6901), (8.3219, 5.6316),
    (8.3528, 5.5810), (8.3842, 5.5414), (8.4186, 5.5048), (8.4557, 5.4715),
    (8.4954, 5.4414), (8.5373, 5.4148), (8.5813, 5.3918), (8.6272, 5.3725),
    (8.6746, 5.3570), (8.7234, 5.3455), (8.7734, 5.3381), (8.8328, 5.3367),
    (8.8958, 5.3445), (8.9636, 5.3615), (9.0375, 5.3874), (9.1185, 5.4222),
    (9.2079, 5.4657), (9.3067, 5.5179), (9.4162, 5.5786), (9.5374, 5.6477),
    (9.6716, 5.7251),
];

const MEDIA_REWIND_RIGHT: [(f32, f32); 93] = [
    (8.5970, 7.1727), (8.4629, 7.2503), (8.3425, 7.3208), (8.2352, 7.3852),
    (8.1406, 7.4447), (8.0582, 7.5003), (7.9875, 7.5531), (7.9281, 7.6041),
    (7.8795, 7.6544), (7.8412, 7.7051), (7.8128, 7.7572), (7.7942, 7.8041),
    (7.7798, 7.8522), (7.7695, 7.9010), (7.7633, 7.9504), (7.7612, 8.0000),
    (7.7633, 8.0496), (7.7695, 8.0990), (7.7798, 8.1478), (7.7942, 8.1958),
    (7.8128, 8.2428), (7.8412, 8.2949), (7.8795, 8.3456), (7.9281, 8.3959),
    (7.9875, 8.4469), (8.0582, 8.4997), (8.1406, 8.5553), (8.2352, 8.6148),
    (8.3425, 8.6792), (8.4629, 8.7497), (8.5970, 8.8272), (11.1045, 10.2749),
    (11.2387, 10.3523), (11.3599, 10.4214), (11.4694, 10.4820), (11.5682, 10.5342),
    (11.6576, 10.5778), (11.7386, 10.6126), (11.8125, 10.6385), (11.8803, 10.6555),
    (11.9434, 10.6633), (12.0027, 10.6619), (12.0527, 10.6545), (12.1015, 10.6429),
    (12.1490, 10.6275), (12.1948, 10.6082), (12.2388, 10.5851), (12.2807, 10.5585),
    (12.3204, 10.5285), (12.3575, 10.4951), (12.3919, 10.4586), (12.4233, 10.4190),
    (12.4542, 10.3683), (12.4789, 10.3099), (12.4982, 10.2426), (12.5127, 10.1657),
    (12.5231, 10.0781), (12.5300, 9.9789), (12.5342, 9.8673), (12.5364, 9.7421),
    (12.5372, 9.6026), (12.5373, 9.4477), (12.5373, 6.5523), (12.5372, 6.3974),
    (12.5364, 6.2579), (12.5342, 6.1327), (12.5300, 6.0210), (12.5231, 5.9219),
    (12.5127, 5.8343), (12.4982, 5.7574), (12.4789, 5.6901), (12.4542, 5.6316),
    (12.4233, 5.5810), (12.3919, 5.5414), (12.3575, 5.5048), (12.3204, 5.4715),
    (12.2807, 5.4414), (12.2388, 5.4148), (12.1948, 5.3918), (12.1490, 5.3725),
    (12.1015, 5.3570), (12.0527, 5.3455), (12.0027, 5.3381), (11.9434, 5.3367),
    (11.8803, 5.3445), (11.8125, 5.3615), (11.7386, 5.3874), (11.6576, 5.4222),
    (11.5682, 5.4657), (11.4694, 5.5179), (11.3599, 5.5786), (11.2387, 5.6477),
    (11.1045, 5.7251),
];

const MEDIA_REWIND_LEFT: [(f32, f32); 93] = [
    (3.8209, 7.1727), (3.6868, 7.2503), (3.5664, 7.3208), (3.4591, 7.3852),
    (3.3644, 7.4447), (3.2821, 7.5003), (3.2114, 7.5531), (3.1520, 7.6041),
    (3.1034, 7.6544), (3.0651, 7.7051), (3.0367, 7.7572), (3.0181, 7.8041),
    (3.0037, 7.8522), (2.9933, 7.9010), (2.9871, 7.9504), (2.9851, 8.0000),
    (2.9871, 8.0496), (2.9933, 8.0990), (3.0037, 8.1478), (3.0181, 8.1958),
    (3.0367, 8.2428), (3.0651, 8.2949), (3.1034, 8.3456), (3.1520, 8.3959),
    (3.2114, 8.4469), (3.2821, 8.4997), (3.3644, 8.5553), (3.4591, 8.6148),
    (3.5664, 8.6792), (3.6868, 8.7497), (3.8209, 8.8272), (6.3284, 10.2749),
    (6.4626, 10.3523), (6.5838, 10.4214), (6.6933, 10.4820), (6.7921, 10.5342),
    (6.8815, 10.5778), (6.9625, 10.6126), (7.0364, 10.6385), (7.1042, 10.6555),
    (7.1672, 10.6633), (7.2266, 10.6619), (7.2766, 10.6545), (7.3254, 10.6429),
    (7.3728, 10.6275), (7.4187, 10.6082), (7.4627, 10.5851), (7.5046, 10.5585),
    (7.5443, 10.5285), (7.5814, 10.4951), (7.6158, 10.4586), (7.6472, 10.4190),
    (7.6781, 10.3683), (7.7028, 10.3099), (7.7221, 10.2426), (7.7366, 10.1657),
    (7.7469, 10.0781), (7.7539, 9.9789), (7.7581, 9.8673), (7.7603, 9.7421),
    (7.7611, 9.6026), (7.7612, 9.4477), (7.7612, 6.5523), (7.7611, 6.3974),
    (7.7603, 6.2579), (7.7581, 6.1327), (7.7539, 6.0210), (7.7469, 5.9219),
    (7.7366, 5.8343), (7.7221, 5.7574), (7.7028, 5.6901), (7.6781, 5.6316),
    (7.6472, 5.5810), (7.6158, 5.5414), (7.5814, 5.5048), (7.5443, 5.4715),
    (7.5046, 5.4414), (7.4627, 5.4148), (7.4187, 5.3918), (7.3728, 5.3725),
    (7.3254, 5.3570), (7.2766, 5.3455), (7.2266, 5.3381), (7.1672, 5.3367),
    (7.1042, 5.3445), (7.0364, 5.3615), (6.9625, 5.3874), (6.8815, 5.4222),
    (6.7921, 5.4657), (6.6933, 5.5179), (6.5838, 5.5786), (6.4626, 5.6477),
    (6.3284, 5.7251),
];

const MEDIA_ARROW_ADVANCE: f32 = 40.0 / 134.0 * 16.0;
const MEDIA_ARROW_LEFT_EDGE: f32 = 3.463;
const MEDIA_ARROW_RIGHT_EDGE: f32 = 13.015;
const MEDIA_REWIND_LEFT_EDGE: f32 = 2.985;
const MEDIA_REWIND_RIGHT_EDGE: f32 = 12.537;

/// AMLL `icon-animations.css` 的 forward：左箭头滑到右箭头位置，右箭头缩小
/// 消失，同时一个新的左箭头从左侧缩放出现。终点仍是两个箭头。
fn paint_forward(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    base: Xf,
    progress: f32,
) {
    let t = progress.clamp(0.0, 1.0);
    let slide = cubic_bezier(0.4, 1.51, 0.4, 1.0, t);
    let cover = {
        let x = (t * 3.0).min(1.0); // 0.2s / 0.6s
        cubic_bezier(0.0, 0.0, 0.58, 1.0, x)
    };

    // standby：scale(0→1) translateX(-15px→0)，origin 22% 50%。
    let tx = -(15.0 / 134.0 * 16.0) * (1.0 - slide);
    let standby = Xf::scale_translate_about(slide, MEDIA_ARROW_LEFT_EDGE, 8.0, tx).then(base);
    fill_poly_xf(window, color, origin, s, &MEDIA_ARROW_LEFT, standby);

    // 主左箭头：translateX(0 → 40px)。
    let main = Xf {
        s: 1.0,
        tx: MEDIA_ARROW_ADVANCE * slide,
        ty: 0.0,
    }
    .then(base);
    fill_poly_xf(window, color, origin, s, &MEDIA_ARROW_LEFT, main);

    // 右箭头：scale → 0 / opacity → 0（0.2s ease-out），origin 81% 50%。
    let ghost = 1.0 - cover;
    if ghost > 0.002 {
        let xf = Xf::scale_about(ghost, MEDIA_ARROW_RIGHT_EDGE, 8.0).then(base);
        let faded = Hsla {
            a: color.a * ghost,
            ..color
        };
        fill_poly_xf(window, faded, origin, s, &MEDIA_ARROW_RIGHT, xf);
    }
}

/// AMLL `icon-animations.css` 的 rewind：forward 的水平镜像。
fn paint_rewind(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    base: Xf,
    progress: f32,
) {
    let t = progress.clamp(0.0, 1.0);
    let slide = cubic_bezier(0.4, 1.51, 0.4, 1.0, t);
    let cover = {
        let x = (t * 3.0).min(1.0);
        cubic_bezier(0.0, 0.0, 0.58, 1.0, x)
    };

    let tx = (15.0 / 134.0 * 16.0) * (1.0 - slide);
    let standby =
        Xf::scale_translate_about(slide, MEDIA_REWIND_RIGHT_EDGE, 8.0, tx).then(base);
    fill_poly_xf(window, color, origin, s, &MEDIA_REWIND_RIGHT, standby);

    let main = Xf {
        s: 1.0,
        tx: -MEDIA_ARROW_ADVANCE * slide,
        ty: 0.0,
    }
    .then(base);
    fill_poly_xf(window, color, origin, s, &MEDIA_REWIND_RIGHT, main);

    let ghost = 1.0 - cover;
    if ghost > 0.002 {
        let xf = Xf::scale_about(ghost, MEDIA_REWIND_LEFT_EDGE, 8.0).then(base);
        let faded = Hsla {
            a: color.a * ghost,
            ..color
        };
        fill_poly_xf(window, faded, origin, s, &MEDIA_REWIND_LEFT, xf);
    }
}

fn arc_pts(cx: f32, cy: f32, r: f32, half_angle: f32, n: usize) -> Vec<(f32, f32)> {
    (0..=n)
        .map(|i| {
            let a = -half_angle + i as f32 / n as f32 * half_angle * 2.0;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// AMLL `VolumeControlSlider`：音量到达 0 / 1 时，喇叭本体与 3 道声波按
/// 50ms 间隔依次 `1 → 1.2 → 0.85 → 1` 弹跳。
fn paint_speaker(
    window: &mut Window,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    s: f32,
    progress: f32,
    high: bool,
) {
    let cone = [
        (3.0, 6.2),
        (6.0, 6.2),
        (9.0, 3.6),
        (9.0, 12.4),
        (6.0, 9.8),
        (3.0, 9.8),
    ];
    let cone_xf = Xf::scale_about(speaker_bounce_scale(progress, 0), 6.0, 8.0);
    fill_poly_xf(window, color, origin, s, &cone, cone_xf);

    if high {
        for (i, r) in [2.4f32, 4.0, 5.6].into_iter().enumerate() {
            let scale = speaker_bounce_scale(progress, i + 1);
            // 声波围绕自身 bbox 中心缩放（约 cx + r*0.79）。
            let xf = Xf::scale_about(scale, 9.0 + r * 0.79, 8.0);
            // 0.78 对应 AMLL icon_speaker_3.svg 的弧线开口（高度/宽度比）。
            let pts = arc_pts(9.0, 8.0, r, 0.78, 12);
            // 声波比常规描边细：AMLL 的 3 道弧线之间有明确间隙。
            stroke_poly_xf_width(window, color, origin, s, &pts, xf, 1.05);
        }
    }
}

/// AMLL `@keyframes pressed-animation`（0.7s）：
/// 1 → 0.85 (20%) → 1.1 (50%) → 1 (100%)，段内使用 CSS `ease`。
fn press_scale(progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    if p <= 0.0 || p >= 1.0 {
        return 1.0;
    }
    let ease = |x: f32| cubic_bezier(0.25, 0.1, 0.25, 1.0, x);
    if p < 0.2 {
        1.0 + (0.85 - 1.0) * ease(p / 0.2)
    } else if p < 0.5 {
        0.85 + (1.1 - 0.85) * ease((p - 0.2) / 0.3)
    } else {
        1.1 + (1.0 - 1.1) * ease((p - 0.5) / 0.5)
    }
}

/// AMLL speaker-bounce：每个部件 3 段 × 0.2s，逐部件延迟 50ms，总 0.75s。
fn speaker_bounce_scale(progress: f32, part: usize) -> f32 {
    let local = progress.clamp(0.0, 1.0) * 0.75 - 0.05 * part as f32;
    if local <= 0.0 || local >= 0.6 {
        return 1.0;
    }
    let segment = (local / 0.2).floor() as i32;
    let x = (local - segment as f32 * 0.2) / 0.2;
    let e = cubic_bezier(0.42, 0.0, 0.58, 1.0, x); // CSS ease-in-out
    match segment {
        0 => 1.0 + 0.2 * e,
        1 => 1.2 - 0.35 * e,
        _ => 0.85 + 0.15 * e,
    }
}

/// 三次贝塞尔缓动（CSS `cubic-bezier(x1,y1,x2,y2)`），y 允许超出 [0,1]。
pub(crate) fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;

    let sample_x = |t: f32| ((ax * t + bx) * t + cx) * t;
    let sample_y = |t: f32| ((ay * t + by) * t + cy) * t;

    let (mut low, mut high) = (0.0f32, 1.0f32);
    let mut t = x;
    for _ in 0..10 {
        let sx = sample_x(t);
        if (sx - x).abs() < 1e-4 {
            break;
        }
        if sx < x {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) * 0.5;
    }
    sample_y(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_animation_hits_amll_keyframes() {
        assert!((press_scale(0.0) - 1.0).abs() < 1e-6);
        // 20% 处约为 0.85（段内 ease 在端点精确等于目标值）。
        assert!((press_scale(0.2) - 0.85).abs() < 1e-5);
        assert!((press_scale(0.5) - 1.1).abs() < 1e-5);
        assert!((press_scale(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn speaker_bounce_is_staggered_and_settles() {
        assert_eq!(speaker_bounce_scale(0.0, 0), 1.0);
        assert_eq!(speaker_bounce_scale(1.0, 0), 1.0);
        assert_eq!(speaker_bounce_scale(1.0, 3), 1.0);
        // 第一个部件先到峰值，第四个部件仍在静息（延迟 150ms）。
        let mid = 0.2 / 0.75;
        assert!(speaker_bounce_scale(mid, 0) > 1.1);
        assert_eq!(speaker_bounce_scale(0.1 / 0.75, 3), 1.0);
    }

    #[test]
    fn arrow_outline_matches_amll_viewbox() {
        // AMLL icon_forward.svg 的内容 bbox：x=29..108.9 / 134 * 16。
        let (min_x, max_x) = MEDIA_ARROW_LEFT.iter().fold(
            (f32::INFINITY, f32::NEG_INFINITY),
            |(lo, hi), p| (lo.min(p.0), hi.max(p.0)),
        );
        assert!((min_x - 3.463).abs() < 0.05);
        // 两个箭头形状相同，右箭头整体平移 40/134*16。
        let right_min = MEDIA_ARROW_RIGHT
            .iter()
            .map(|p| p.0)
            .fold(f32::INFINITY, f32::min);
        assert!((right_min - (min_x + MEDIA_ARROW_ADVANCE)).abs() < 0.05);
        assert!((max_x - min_x - 4.776).abs() < 0.05);
        // 动画在 progress=1 时回到标准双箭头排布。
        let slide = cubic_bezier(0.4, 1.51, 0.4, 1.0, 1.0);
        assert!((slide - 1.0).abs() < 1e-6);
    }
}
