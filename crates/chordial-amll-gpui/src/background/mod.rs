#![allow(clippy::chunks_exact_to_as_chunks)]

//! AMLL 歌词背景渲染器的 Rust/gpui 等价实现。
//!
//! 行为参考 AMLL `bg-render/*`（GPL-3.0），本文件为独立实现，未复制源码。
//!
//! - [`MeshGradientRenderer`]：专辑封面 → 32×32 调色纹理 → BHP 双三次网格
//!   → 软件光栅化 → 旋转/音量/暗角/抖动，等价于 AMLL 的 WebGL 管线；
//! - [`CssBackgroundRenderer`]：纯色背景（AMLL 的 `css-bg`）；
//! - [`LyricBackgroundRenderer`]：react-full 中 `LyricBackgroundRenderer`
//!   配置类型对应的运行时包装（选择具体渲染器并转发调用）。
//!
//! 性能设计：
//! - 输出分辨率 = viewport × render_scale，硬上限 200k 像素；缓冲区跨帧复用。
//! - 三角形网格在专辑/宽高比变化时才重建；每帧只做光栅化 + 合成。
//! - 低于目标 FPS 的帧直接复用上一张 `RenderImage`（零分配、零光栅化）。
//! - 所有热路径埋 `amll.bg.raster` 检查点。

use std::sync::Arc;

use chordal_core::mem::{self, Gauge};
use chordal_core::perf::PerfScope;
use gpui::{Hsla, RenderImage};
use image::{Frame, RgbaImage};

mod palette;
mod raster;

pub use palette::*;
use raster::*;

/// 可选的歌词背景渲染器（对应 AMLL `lyricBackgroundRendererAtom` 的配置）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackgroundRendererKind {
    #[default]
    MeshGradient,
    Css,
    None,
}

/// BHP 网格控制点。
#[derive(Debug, Clone, Copy)]
pub struct ControlPoint {
    pub x: f32,
    pub y: f32,
    /// u 方向切线旋转（度）。
    pub u_rot: f32,
    /// v 方向切线旋转（度）。
    pub v_rot: f32,
    pub u_scale: f32,
    pub v_scale: f32,
}

impl Default for ControlPoint {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            u_rot: 0.0,
            v_rot: 0.0,
            u_scale: 1.0,
            v_scale: 1.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ControlPointPreset {
    pub width: usize,
    pub height: usize,
    /// 行优先：`points[y * width + x]`。
    pub points: Vec<ControlPoint>,
}

impl ControlPointPreset {
    pub fn grid(width: usize, height: usize) -> Self {
        let mut points = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                points.push(ControlPoint {
                    x: if width > 1 {
                        x as f32 / (width - 1) as f32 * 2.0 - 1.0
                    } else {
                        0.0
                    },
                    y: if height > 1 {
                        y as f32 / (height - 1) as f32 * 2.0 - 1.0
                    } else {
                        0.0
                    },
                    u_scale: if width > 1 {
                        2.0 / (width - 1) as f32
                    } else {
                        1.0
                    },
                    v_scale: if height > 1 {
                        2.0 / (height - 1) as f32
                    } else {
                        1.0
                    },
                    ..Default::default()
                });
            }
        }
        Self {
            width,
            height,
            points,
        }
    }
}

const TEXTURE_SIZE: usize = 32;
const MAX_OUTPUT_PIXELS: usize = 200_000;
const SUBDIVISIONS: usize = 12;
/// 封面交叉淡入淡出的时长（毫秒）。使用真实墙钟时间推进，与视图帧率解耦：
/// 高刷屏上视图可能每秒调用 200+ 次 `render`，但光栅只有 30fps。
const CROSSFADE_MS: f32 = 420.0;
/// 鼓点包络的跟随时间常数：起音快、回落慢，脉冲轮廓清晰。
const BEAT_ATTACK_SECS: f32 = 0.035;
const BEAT_RELEASE_SECS: f32 = 0.18;

#[derive(Debug, Clone, Copy, Default)]
struct Vertex {
    /// NDC 坐标（-1..1，y 向上）。
    ndc: [f32; 2],
    /// 0..1 纹理坐标。
    uv: [f32; 2],
}

#[derive(Debug, Clone)]
struct MeshLayer {
    preset: ControlPointPreset,
    texture: Vec<u8>, // RGBA, 32x32
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    vertex_aspect: f32,
    alpha: f32,
    /// 是否已经从画面中淡出。
    fading_out: bool,
}

impl MeshLayer {
    fn rebuild_vertices(&mut self, aspect: f32) {
        if (self.vertex_aspect - aspect).abs() < 0.001 && !self.vertices.is_empty() {
            return;
        }
        self.vertex_aspect = aspect;
        let cp_w = self.preset.width;
        let cp_h = self.preset.height;
        let sub = SUBDIVISIONS;
        let sub_m1 = (sub - 1) as f32;
        let x_count = (cp_w - 1) * sub;
        let y_count = (cp_h - 1) * sub;
        self.vertices.clear();
        self.vertices.resize(x_count * y_count, Vertex::default());

        // 与 AMLL `BHPMesh::updateMesh` 相同的 UV 映射：
        // uvX 沿 x 方向增加，uvY 沿 y 方向减少。
        let inv_th = 1.0 / (sub_m1 * (cp_w as f32 - 1.0));
        let inv_tw = 1.0 / (sub_m1 * (cp_h as f32 - 1.0));

        for cy in 0..cp_h.saturating_sub(1) {
            for cx in 0..cp_w.saturating_sub(1) {
                let p00 = self.preset.points[cy * cp_w + cx];
                // p01 与 p00 在 y 方向相邻，p10 在 x 方向相邻（AMLL 命名）。
                let p01 = self.preset.points[(cy + 1) * cp_w + cx];
                let p10 = self.preset.points[cy * cp_w + cx + 1];
                let p11 = self.preset.points[(cy + 1) * cp_w + cx + 1];
                let mx = coefficients(&p00, &p01, &p10, &p11, Axis::X);
                let my = coefficients(&p00, &p01, &p10, &p11, Axis::Y);
                let s_x = cx as f32 / (cp_w as f32 - 1.0);
                let s_y = cy as f32 / (cp_h as f32 - 1.0);
                for u in 0..sub {
                    let un = u as f32 / sub_m1;
                    for v in 0..sub {
                        let vn = v as f32 / sub_m1;
                        let px = surface(&mx, un, vn);
                        let py = surface(&my, un, vn);
                        let (ndc_x, ndc_y) = if aspect > 1.0 {
                            (px, py * aspect)
                        } else {
                            (px / aspect.max(0.001), py)
                        };
                        let gx = cx * sub + v;
                        let gy = cy * sub + u;
                        self.vertices[gx + gy * x_count] = Vertex {
                            ndc: [ndc_x, ndc_y],
                            uv: [s_x + v as f32 * inv_th, 1.0 - s_y - u as f32 * inv_tw],
                        };
                    }
                }
            }
        }

        self.indices.clear();
        self.indices.reserve((x_count - 1) * (y_count - 1) * 6);
        for gy in 0..y_count.saturating_sub(1) {
            for gx in 0..x_count.saturating_sub(1) {
                let i00 = (gx + gy * x_count) as u32;
                let i10 = (gx + 1 + gy * x_count) as u32;
                let i01 = (gx + (gy + 1) * x_count) as u32;
                let i11 = (gx + 1 + (gy + 1) * x_count) as u32;
                self.indices
                    .extend_from_slice(&[i00, i10, i11, i00, i11, i01]);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn location(self, p: &ControlPoint) -> f32 {
        match self {
            Axis::X => p.x,
            Axis::Y => p.y,
        }
    }
    fn tangent_u(self, p: &ControlPoint) -> f32 {
        let r = p.u_rot.to_radians();
        match self {
            Axis::X => r.cos() * p.u_scale,
            Axis::Y => r.sin() * p.u_scale,
        }
    }
    fn tangent_v(self, p: &ControlPoint) -> f32 {
        let r = p.v_rot.to_radians();
        match self {
            Axis::X => -r.sin() * p.v_scale,
            Axis::Y => r.cos() * p.v_scale,
        }
    }
}

/// AMLL 的 H 矩阵（列主序）。
const H: [f32; 16] = [
    2.0, -2.0, 1.0, 1.0, -3.0, 3.0, -2.0, -1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];

fn coefficients(
    p00: &ControlPoint,
    p01: &ControlPoint,
    p10: &ControlPoint,
    p11: &ControlPoint,
    axis: Axis,
) -> [f32; 16] {
    let mut x = [0.0f32; 16];
    x[0] = axis.location(p00);
    x[1] = axis.location(p01);
    x[2] = axis.tangent_v(p00);
    x[3] = axis.tangent_v(p01);
    x[4] = axis.location(p10);
    x[5] = axis.location(p11);
    x[6] = axis.tangent_v(p10);
    x[7] = axis.tangent_v(p11);
    x[8] = axis.tangent_u(p00);
    x[9] = axis.tangent_u(p01);
    x[12] = axis.tangent_u(p10);
    x[13] = axis.tangent_u(p11);
    let h_t = transpose(&H);
    let mut acc = mat_mul(&h_t, &transpose(&x));
    acc = mat_mul(&acc, &H);
    acc
}

fn surface(m: &[f32; 16], u: f32, v: f32) -> f32 {
    let u_vec = [u * u * u, u * u, u, 1.0];
    let v_vec = [v * v * v, v * v, v, 1.0];
    let tmp = transform(m, &u_vec);
    dot4(&v_vec, &tmp)
}

fn mat_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    out
}

fn transpose(m: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            out[col * 4 + row] = m[row * 4 + col];
        }
    }
    out
}

fn transform(m: &[f32; 16], v: &[f32; 4]) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for row in 0..4 {
        out[row] = m[row] * v[0] + m[4 + row] * v[1] + m[8 + row] * v[2] + m[12 + row] * v[3];
    }
    out
}

fn dot4(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// 一个简单的 xorshift 伪随机源（背景网格随机化，测试可复现）。
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn f32(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.f32()
    }
}

fn generate_preset(rng: &mut Rng) -> ControlPointPreset {
    let w = 4 + (rng.next() % 2) as usize;
    let h = 4 + (rng.next() % 2) as usize;
    let mut preset = ControlPointPreset::grid(w, h);
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let p = &mut preset.points[y * w + x];
            p.x += rng.range(-0.32, 0.32);
            p.y += rng.range(-0.32, 0.32);
            p.u_rot = rng.range(-90.0, 90.0);
            p.v_rot = rng.range(-90.0, 90.0);
            p.u_scale *= rng.range(0.6, 1.4);
            p.v_scale *= rng.range(0.6, 1.4);
        }
    }
    // 3x3 平滑一次，避免控制点折角过于尖锐（保留足够流动性）。
    for _ in 0..1 {
        let old = preset.points.clone();
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                let mut x_sum = 0.0;
                let mut y_sum = 0.0;
                let mut ur = 0.0;
                let mut vr = 0.0;
                let mut up = 0.0;
                let mut vp = 0.0;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let p = old[((y as i32 + dy) as usize) * w + (x as i32 + dx) as usize];
                        let weight = if dx == 0 && dy == 0 {
                            4.0
                        } else if dx == 0 || dy == 0 {
                            2.0
                        } else {
                            1.0
                        };
                        x_sum += p.x * weight;
                        y_sum += p.y * weight;
                        ur += p.u_rot * weight;
                        vr += p.v_rot * weight;
                        up += p.u_scale * weight;
                        vp += p.v_scale * weight;
                    }
                }
                let p = &mut preset.points[y * w + x];
                p.x = p.x * 0.6 + x_sum / 16.0 * 0.4;
                p.y = p.y * 0.6 + y_sum / 16.0 * 0.4;
                p.u_rot = p.u_rot * 0.6 + ur / 16.0 * 0.4;
                p.v_rot = p.v_rot * 0.6 + vr / 16.0 * 0.4;
                p.u_scale = p.u_scale * 0.6 + up / 16.0 * 0.4;
                p.v_scale = p.v_scale * 0.6 + vp / 16.0 * 0.4;
            }
        }
    }
    preset
}

/// 生成 32×32 专辑调色纹理（BGRA 字节），供渲染层/视图缓存。
///
/// 相比保留整幅封面 RGBA（512² ≈ 1 MiB），调色纹理只有 4 KiB；切换背景
/// 渲染器时用它重建即可，视觉等价。
pub struct MeshGradientRenderer {
    layers: Vec<MeshLayer>,
    rng: Rng,
    output: Vec<u8>,
    scratch: Vec<u8>,
    /// 从已释放的 `RenderImage` 回收的整帧像素缓冲（尺寸匹配时复用）。
    /// 软件光栅每帧都要交出一块 output，回收可消除 30fps × 帧大小的分配抖动。
    recycled: Vec<Vec<u8>>,
    out_w: usize,
    out_h: usize,
    viewport_w: usize,
    viewport_h: usize,
    render_scale: f32,
    flow_speed: f32,
    fps: u32,
    fps_accum_ms: f32,
    frame_time: f32,
    /// 外部鼓点包络目标（0..1）。
    beat_target: f32,
    /// 经过起音/回落平滑后的鼓点强度（0..1），供光栅化使用。
    beat_volume: f32,
    /// 鼓点视觉效果的整体倍率（0..=1.5，1.0 = 原始幅度）。
    beat_strength: f32,
    paused: bool,
    static_mode: bool,
    last_image: Option<Arc<RenderImage>>,
    empty_image: Option<Arc<RenderImage>>,
    needs_clear: bool,
    disposed: bool,
}

impl Default for MeshGradientRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl MeshGradientRenderer {
    pub fn new() -> Self {
        Self::with_seed(0x9E37_79B9_7F4A_7C15)
    }

    pub fn with_seed(seed: u64) -> Self {
        Self {
            layers: Vec::new(),
            rng: Rng(seed.max(1)),
            output: Vec::new(),
            scratch: Vec::new(),
            recycled: Vec::new(),
            out_w: 0,
            out_h: 0,
            viewport_w: 0,
            viewport_h: 0,
            render_scale: 0.5,
            flow_speed: 4.0,
            fps: 30,
            fps_accum_ms: 0.0,
            frame_time: 0.0,
            beat_target: 0.0,
            beat_volume: 0.0,
            beat_strength: 1.0,
            paused: false,
            static_mode: false,
            last_image: None,
            empty_image: None,
            needs_clear: false,
            disposed: false,
        }
    }

    pub fn set_flow_speed(&mut self, speed: f32) {
        self.flow_speed = speed.max(0.0);
    }

    pub fn set_render_scale(&mut self, scale: f32) {
        let scale = scale.clamp(0.05, 1.5);
        if (scale - self.render_scale).abs() > 0.001 {
            self.render_scale = scale;
            self.needs_clear = true;
        }
    }

    pub fn set_static_mode(&mut self, enable: bool) {
        self.static_mode = enable;
    }

    pub fn set_fps(&mut self, fps: u32) {
        self.fps = fps;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn set_low_freq_volume(&mut self, volume: f32) {
        self.beat_target = volume.clamp(0.0, 1.0) / 10.0;
    }

    /// 0..1 的鼓点包络，驱动网格的呼吸、缩放、旋转与提亮。
    ///
    /// 映射到内部 0..1：起音阶段由 [`Self::render`] 用约 35ms 时间常数快速
    /// 跟随，回落约 180ms，因此 30fps 光栅也能看清每一次击打。视觉幅度
    /// 进一步乘以 [`Self::set_beat_strength`]，默认倍率由界面设置决定。
    pub fn set_beat_level(&mut self, level: f32) {
        self.beat_target = level.clamp(0.0, 1.0);
    }

    /// 鼓点视觉效果的整体倍率：0 = 完全关闭（网格仍会缓慢流动），
    /// 1.0 = 原始幅度，上限 1.5。只缩放呼吸/旋转/缩放/提亮，不改变包络
    /// 的时序，因此调低后鼓点仍然跟手，只是幅度更克制。
    pub fn set_beat_strength(&mut self, strength: f32) {
        self.beat_strength = strength.clamp(0.0, 1.5);
    }

    pub fn set_has_lyric(&mut self, _has: bool) {}

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        let width = width.max(1) as usize;
        let height = height.max(1) as usize;
        if width != self.viewport_w || height != self.viewport_h {
            self.viewport_w = width;
            self.viewport_h = height;
            self.needs_clear = true;
            self.last_image = None;
        }
    }

    pub fn set_album_rgba(&mut self, width: u32, height: u32, rgba: Vec<u8>) {
        if let Some(texture) = album_palette_bytes(width, height, &rgba) {
            self.set_album_palette(texture);
        }
    }

    /// 直接使用已烘焙的 32×32 BGRA 调色纹理（避免重复缩放/滤镜）。
    pub fn set_album_palette(&mut self, texture: Vec<u8>) {
        let preset = generate_preset(&mut self.rng);
        let aspect = if self.viewport_h > 0 {
            self.viewport_w as f32 / self.viewport_h as f32
        } else if self.out_h > 0 {
            self.out_w as f32 / self.out_h as f32
        } else {
            1.0
        };
        // 暂停时没有下一帧来推进淡入，直接让新封面可见。
        let instant = self.paused;
        let mut layer = MeshLayer {
            preset,
            texture,
            vertices: Vec::new(),
            indices: Vec::new(),
            vertex_aspect: 0.0,
            alpha: if instant { 1.0 } else { 0.0 },
            fading_out: false,
        };
        layer.rebuild_vertices(aspect);
        if instant {
            self.layers.clear();
        } else {
            // 快速连切时只保留当前画面最上面的一层作为交叉淡出的底色。
            // 旧实现会累积最多 3 层半透明封面：每次切歌把所有旧层同时淡出、
            // 新层从 0 淡入，合成后的 alpha 明显小于 1；叠在黑色底上就表现
            // 为背景“异常变黑”。两层交叉淡出可以保证底层始终不透。
            if let Some(mut outgoing) = self.layers.pop() {
                self.layers.clear();
                let was_fading = outgoing.fading_out;
                outgoing.fading_out = true;
                if !was_fading {
                    // 之前的封面可能还在淡入（alpha < 1）。切歌瞬间把它补满，
                    // 让混色只发生在新旧封面之间，而不是混向透明。
                    outgoing.alpha = 1.0;
                }
                self.layers.push(outgoing);
            }
        }
        self.layers.push(layer);
        self.needs_clear = true;
    }

    pub fn clear_album(&mut self) {
        if self.paused {
            // 暂停时没有后续帧推进淡出，直接清除。
            self.layers.clear();
        } else {
            for layer in &mut self.layers {
                layer.fading_out = true;
            }
        }
        self.needs_clear = true;
    }

    pub fn is_animating(&self) -> bool {
        if self.paused || self.disposed || self.fps == 0 || self.layers.is_empty() {
            return false;
        }
        if self.static_mode
            && self.layers.len() == 1
            && self.layers[0].alpha >= 1.0
            && !self.layers[0].fading_out
        {
            return false;
        }
        true
    }

    fn ensure_output(&mut self) {
        let mut w = ((self.viewport_w as f32) * self.render_scale).ceil() as usize;
        let mut h = ((self.viewport_h as f32) * self.render_scale).ceil() as usize;
        w = w.max(2);
        h = h.max(2);
        let pixels = w.saturating_mul(h);
        if pixels > MAX_OUTPUT_PIXELS {
            let scale = (MAX_OUTPUT_PIXELS as f32 / pixels as f32).sqrt();
            w = ((w as f32 * scale).floor() as usize).max(2);
            h = ((h as f32 * scale).floor() as usize).max(2);
        }
        // 只有视口尺寸变化才需要重清缓冲；帧缓冲被 take 走（零拷贝交帧）后
        // 下一帧只需补容量，不能借此触发 needs_clear，否则会绕过 FPS 门控
        // 变成每帧全量光栅。
        if w != self.out_w || h != self.out_h {
            self.out_w = w;
            self.out_h = h;
            self.needs_clear = true;
            // 尺寸变化后旧尺寸的回收块不再可用。
            self.recycled.clear();
        }
        let len = w * h * 4;
        if self.output.len() != len {
            // 优先复用刚从旧 RenderImage 回收的整帧缓冲（零分配 + 零清零）。
            if self.recycled.last().is_some_and(|buf| buf.len() == len) {
                self.output = self.recycled.pop().expect("checked");
            } else {
                self.output.resize(len, 0);
            }
        }
        if self.scratch.len() != len {
            self.scratch.resize(len, 0);
        }
        // 双缓冲的"配置大小"（output 每帧被 take 走交给 gpui，下次重新补齐；
        // scratch 常驻）。登记在案便于把背景分辨率除数与常驻内存对上。
        mem::set_gauge(Gauge::Background, len.saturating_mul(2));
    }

    /// 回收一块已从 sprite atlas 移除的整帧像素缓冲。
    ///
    /// 只接受与当前输出尺寸完全一致的 buffer；保留 1–2 块足够覆盖
    /// "交出本帧 → 释放上帧"的一帧延迟，不会让回收池无限增长。
    pub fn recycle_output(&mut self, data: Vec<u8>) {
        let len = self.out_w.saturating_mul(self.out_h).saturating_mul(4);
        if len > 0 && data.len() == len && self.recycled.len() < 2 {
            self.recycled.push(data);
        }
    }

    #[doc(hidden)]
    pub fn debug_recycled(&self) -> usize {
        self.recycled.len()
    }

    /// 推进与“真实时间”相关的状态。
    ///
    /// 必须在 FPS 门控**之前**调用：视图在 200Hz+ 屏幕上可能每 3ms 调用一次
    /// `render`，而背景光栅只有 30fps。若只在光栅帧累加，交叉淡入淡出会慢
    /// 一个数量级，快速切歌时多层半透明叠加会让背景明显发黑。
    fn advance_animation(&mut self, dt_ms: f32) {
        let fade = dt_ms / CROSSFADE_MS;
        for layer in &mut self.layers {
            if layer.fading_out {
                layer.alpha = (layer.alpha - fade).max(0.0);
            } else {
                layer.alpha = (layer.alpha + fade).min(1.0);
            }
        }
        // 交叉淡出期间，正在退场的旧封面必须保持不透明：新封面以 alpha=t
        // 叠在旧封面上，合成 alpha 恒为 1，颜色在两张封面之间插值。否则
        // 两张半透明封面叠在黑色底上会整体变暗（快速连切时尤其明显）。
        if let Some(top) = self.layers.iter().rposition(|l| !l.fading_out) {
            for layer in &mut self.layers[..top] {
                layer.alpha = 1.0;
            }
        }
        self.layers.retain(|layer| layer.alpha > 0.001);
        // 当前封面完全显示后，退场的旧封面已无意义，立即丢弃。
        if self.layers.len() > 1 && self.layers.last().is_some_and(|l| l.alpha >= 1.0) {
            self.layers.drain(0..self.layers.len() - 1);
        }

        let target = self.beat_target;
        let tau = if target > self.beat_volume {
            BEAT_ATTACK_SECS
        } else {
            BEAT_RELEASE_SECS
        };
        let k = 1.0 - (-(dt_ms / 1000.0) / tau).exp();
        self.beat_volume += (target - self.beat_volume) * k;
        if !self.beat_volume.is_finite() {
            self.beat_volume = 0.0;
        }
        self.beat_volume = self.beat_volume.clamp(0.0, 1.0);
    }

    /// 渲染一帧。返回 `None` 表示本帧无需更新（调用方保留上一张图）。
    pub fn render(&mut self, delta_ms: f32) -> Option<Arc<RenderImage>> {
        if self.disposed || self.viewport_w == 0 || self.viewport_h == 0 {
            return None;
        }
        let dt = delta_ms.clamp(0.0, 100.0);
        if !self.paused {
            self.frame_time += dt * self.flow_speed;
            self.advance_animation(dt);
        } else if !self.needs_clear {
            return self.last_image.clone();
        }
        if self.fps > 0 && !self.needs_clear {
            self.fps_accum_ms += dt;
            let interval = 1000.0 / self.fps as f32;
            if self.fps_accum_ms + 0.001 < interval {
                return self.last_image.clone();
            }
            self.fps_accum_ms = 0.0;
        } else if self.fps == 0 && !self.needs_clear {
            return self.last_image.clone();
        }

        self.ensure_output();
        if self.layers.is_empty() {
            self.needs_clear = false;
            self.last_image = None;
            // 返回一帧全透明图，明确告诉调用方“背景已清除”，
            // 而不是让「None = 保留上一帧」的约定导致旧封面残留。
            if self.empty_image.is_none() {
                let blank = RgbaImage::from_raw(2, 2, vec![0u8; 16])?;
                self.empty_image = Some(Arc::new(RenderImage::new(vec![Frame::new(blank)])));
            }
            return self.empty_image.clone();
        }

        let aspect = self.out_w as f32 / self.out_h as f32;
        // 包络只决定“什么时候跳”，强度决定“跳多大幅度”。
        let volume = self.beat_volume * self.beat_strength;
        let scope = PerfScope::enter("amll.bg.raster");
        self.output.fill(0);
        let mut used = 0u64;

        for layer in &mut self.layers {
            layer.rebuild_vertices(aspect);
            self.scratch.fill(0);
            rasterize_layer(
                layer,
                &mut self.scratch,
                self.out_w,
                self.out_h,
                self.frame_time,
                volume,
            );
            blend_over(&mut self.output, &self.scratch);
            used += (self.out_w * self.out_h) as u64;
        }
        scope.finish(used);
        self.needs_clear = false;
        self.fps_accum_ms = 0.0;

        // output 已是 BGRA 顺序（调色板创建时已转换），直接零拷贝交给 gpui。
        let bgra = std::mem::take(&mut self.output);
        let image = RgbaImage::from_raw(self.out_w as u32, self.out_h as u32, bgra)?;
        let rendered = Arc::new(RenderImage::new(vec![Frame::new(image)]));
        self.last_image = Some(rendered.clone());
        Some(rendered)
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
        self.layers.clear();
        self.last_image = None;
        self.output.clear();
        self.output.shrink_to_fit();
        self.scratch.clear();
        self.scratch.shrink_to_fit();
        self.recycled.clear();
        mem::set_gauge(Gauge::Background, 0);
    }

    #[doc(hidden)]
    pub fn debug_size(&self) -> (usize, usize) {
        (self.out_w, self.out_h)
    }

    /// 取出当前缓存的背景帧（切换渲染器时由调用方从图集释放）。
    #[doc(hidden)]
    pub fn take_last_image(&mut self) -> Option<Arc<RenderImage>> {
        self.last_image.take()
    }
}

pub struct CssBackgroundRenderer {
    color: Hsla,
}

impl CssBackgroundRenderer {
    pub fn new(color: Hsla) -> Self {
        Self { color }
    }
    pub fn set_color(&mut self, color: Hsla) {
        self.color = color;
    }
    pub fn color(&self) -> Hsla {
        self.color
    }
}

/// react-full `LyricBackgroundRenderer` 配置类型对应的运行时包装。
pub enum LyricBackgroundRenderer {
    Mesh(MeshGradientRenderer),
    Css(CssBackgroundRenderer),
    None,
}

impl LyricBackgroundRenderer {
    pub fn new(kind: BackgroundRendererKind) -> Self {
        match kind {
            BackgroundRendererKind::MeshGradient => Self::Mesh(MeshGradientRenderer::new()),
            BackgroundRendererKind::Css => {
                Self::Css(CssBackgroundRenderer::new(gpui::rgb(0x111111).into()))
            }
            BackgroundRendererKind::None => Self::None,
        }
    }

    pub fn kind(&self) -> BackgroundRendererKind {
        match self {
            Self::Mesh(_) => BackgroundRendererKind::MeshGradient,
            Self::Css(_) => BackgroundRendererKind::Css,
            Self::None => BackgroundRendererKind::None,
        }
    }

    pub fn set_flow_speed(&mut self, speed: f32) {
        if let Self::Mesh(r) = self {
            r.set_flow_speed(speed);
        }
    }
    pub fn set_render_scale(&mut self, scale: f32) {
        if let Self::Mesh(r) = self {
            r.set_render_scale(scale);
        }
    }
    pub fn set_static_mode(&mut self, enable: bool) {
        if let Self::Mesh(r) = self {
            r.set_static_mode(enable);
        }
    }
    pub fn set_fps(&mut self, fps: u32) {
        if let Self::Mesh(r) = self {
            r.set_fps(fps);
        }
    }
    /// 回收已从图集释放的整帧像素缓冲（见
    /// [`MeshGradientRenderer::recycle_output`]）。
    pub fn recycle_output(&mut self, data: Vec<u8>) {
        if let Self::Mesh(r) = self {
            r.recycle_output(data);
        }
    }
    pub fn pause(&mut self) {
        if let Self::Mesh(r) = self {
            r.pause();
        }
    }
    pub fn resume(&mut self) {
        if let Self::Mesh(r) = self {
            r.resume();
        }
    }
    pub fn set_album_rgba(&mut self, w: u32, h: u32, rgba: Vec<u8>) {
        if let Self::Mesh(r) = self {
            r.set_album_rgba(w, h, rgba);
        }
    }
    /// 使用已烘焙的 32×32 BGRA 调色纹理重建背景（切换渲染器时复用）。
    pub fn set_album_palette(&mut self, texture: Vec<u8>) {
        if let Self::Mesh(r) = self {
            r.set_album_palette(texture);
        }
    }
    pub fn clear_album(&mut self) {
        if let Self::Mesh(r) = self {
            r.clear_album();
        }
    }
    pub fn set_low_freq_volume(&mut self, volume: f32) {
        if let Self::Mesh(r) = self {
            r.set_low_freq_volume(volume);
        }
    }
    /// 0..1 的鼓点包络，见 [`MeshGradientRenderer::set_beat_level`]。
    pub fn set_beat_level(&mut self, level: f32) {
        if let Self::Mesh(r) = self {
            r.set_beat_level(level);
        }
    }
    /// 鼓点视觉效果倍率，见 [`MeshGradientRenderer::set_beat_strength`]。
    pub fn set_beat_strength(&mut self, strength: f32) {
        if let Self::Mesh(r) = self {
            r.set_beat_strength(strength);
        }
    }
    pub fn set_has_lyric(&mut self, has: bool) {
        if let Self::Mesh(r) = self {
            r.set_has_lyric(has);
        }
    }
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        if let Self::Mesh(r) = self {
            r.set_viewport(width, height);
        }
    }
    pub fn render(&mut self, delta_ms: f32) -> Option<Arc<RenderImage>> {
        match self {
            Self::Mesh(r) => r.render(delta_ms),
            Self::Css(_) | Self::None => None,
        }
    }
    /// 取出缓存的背景帧（切换渲染器/销毁视图时释放图集纹理用）。
    #[doc(hidden)]
    pub fn take_last_image(&mut self) -> Option<Arc<RenderImage>> {
        match self {
            Self::Mesh(r) => r.take_last_image(),
            Self::Css(_) | Self::None => None,
        }
    }
    pub fn is_animating(&self) -> bool {
        match self {
            Self::Mesh(r) => r.is_animating(),
            Self::Css(_) | Self::None => false,
        }
    }

    pub fn solid_color(&self) -> Option<Hsla> {
        match self {
            Self::Css(r) => Some(r.color()),
            _ => None,
        }
    }

    pub fn set_solid_color(&mut self, color: Hsla) {
        if let Self::Css(r) = self {
            r.set_color(color);
        }
    }
}


#[cfg(test)]
mod tests;
