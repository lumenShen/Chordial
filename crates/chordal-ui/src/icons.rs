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

/// 双箭头几何（16 网格）。两个箭头相邻，左箭头的尖端正好是右箭头的底边。
const CHEV_W: f32 = 3.7;
const CHEV_T: f32 = 1.15;
const CHEV_LEFT_X: f32 = 4.3;
const CHEV_RIGHT_X: f32 = 8.0;

fn chevron_right(x: f32) -> [(f32, f32); 6] {
    [
        (x, 3.3),
        (x + CHEV_W, 8.0),
        (x, 12.7),
        (x + CHEV_T, 12.7),
        (x + CHEV_W - CHEV_T, 8.0),
        (x + CHEV_T, 3.3),
    ]
}

fn chevron_left(x: f32) -> [(f32, f32); 6] {
    [
        (x + CHEV_W, 3.3),
        (x, 8.0),
        (x + CHEV_W, 12.7),
        (x + CHEV_W - CHEV_T, 12.7),
        (x + CHEV_T, 8.0),
        (x + CHEV_W - CHEV_T, 3.3),
    ]
}

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
    let advance = CHEV_RIGHT_X - CHEV_LEFT_X;

    // standby：scale(0→1) translateX(-1.79→0)，平移量在缩放内部。
    let tx = -advance * 0.4 * (1.0 - slide);
    let standby = Xf::scale_translate_about(slide, CHEV_LEFT_X, 8.0, tx).then(base);
    fill_poly_xf(
        window,
        color,
        origin,
        s,
        &chevron_right(CHEV_LEFT_X),
        standby,
    );

    // 主左箭头：translateX(0 → 40/134 × 16)。
    let main = Xf {
        s: 1.0,
        tx: advance * slide,
        ty: 0.0,
    }
    .then(base);
    fill_poly_xf(window, color, origin, s, &chevron_right(CHEV_LEFT_X), main);

    // 右箭头：scale → 0 / opacity → 0（0.2s ease-out）。
    let ghost = 1.0 - cover;
    if ghost > 0.002 {
        let xf = Xf::scale_about(ghost, CHEV_RIGHT_X + CHEV_W, 8.0).then(base);
        let faded = Hsla {
            a: color.a * ghost,
            ..color
        };
        fill_poly_xf(window, faded, origin, s, &chevron_right(CHEV_RIGHT_X), xf);
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
    let advance = CHEV_RIGHT_X - CHEV_LEFT_X;

    let tx = advance * 0.4 * (1.0 - slide);
    let standby = Xf::scale_translate_about(slide, CHEV_RIGHT_X + CHEV_W, 8.0, tx).then(base);
    fill_poly_xf(
        window,
        color,
        origin,
        s,
        &chevron_left(CHEV_RIGHT_X),
        standby,
    );

    let main = Xf {
        s: 1.0,
        tx: -advance * slide,
        ty: 0.0,
    }
    .then(base);
    fill_poly_xf(window, color, origin, s, &chevron_left(CHEV_RIGHT_X), main);

    let ghost = 1.0 - cover;
    if ghost > 0.002 {
        let xf = Xf::scale_about(ghost, CHEV_LEFT_X, 8.0).then(base);
        let faded = Hsla {
            a: color.a * ghost,
            ..color
        };
        fill_poly_xf(window, faded, origin, s, &chevron_left(CHEV_LEFT_X), xf);
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
            let pts = arc_pts(9.0, 8.0, r, 0.96, 12);
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
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
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
    fn arrow_animation_settles_to_two_chevrons() {
        // progress = 1 时主箭头覆盖右箭头位置，standby 回到原位。
        let slide = cubic_bezier(0.4, 1.51, 0.4, 1.0, 1.0);
        assert!((slide - 1.0).abs() < 1e-6);
        let advance = CHEV_RIGHT_X - CHEV_LEFT_X;
        assert!((advance - CHEV_W).abs() < 1e-6);
    }
}
