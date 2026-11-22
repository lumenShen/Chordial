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

use chordal_core::perf::PerfScope;
use gpui::{Hsla, RenderImage};
use image::{Frame, RgbaImage};

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

/// 生成 32×32 专辑调色纹理（BGRA `RenderImage`），供 UI 拉伸成全窗口色场。
pub fn album_palette_image(width: u32, height: u32, rgba: &[u8]) -> Option<Arc<RenderImage>> {
    let mut data = prepare_texture(rgba, width, height)?;
    // 32×32 调色纹理拉伸后作为大背景；轻微提亮/饱和，避免大屏发灰。
    for px in data.as_chunks_mut::<4>().0 {
        for c in &mut px[..3] {
            *c = ((u16::from(*c) * 5 / 4 + 6).min(255)) as u8;
        }
        px.swap(0, 2);
    }
    let image = RgbaImage::from_raw(TEXTURE_SIZE as u32, TEXTURE_SIZE as u32, data)?;
    Some(Arc::new(RenderImage::new(vec![Frame::new(image)])))
}

fn prepare_texture(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || rgba.len() < (width as usize * height as usize * 4) {
        return None;
    }
    let img = RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let resized = image::imageops::resize(
        &img,
        TEXTURE_SIZE as u32,
        TEXTURE_SIZE as u32,
        image::imageops::FilterType::Triangle,
    );
    let mut data = resized.into_raw();

    contrast(&mut data, 0.4);
    saturate(&mut data, 3.0);
    contrast(&mut data, 1.7);
    brightness(&mut data, 0.75);
    blur_rgba(&mut data, TEXTURE_SIZE, TEXTURE_SIZE, 2, 4);
    Some(data)
}

fn contrast(data: &mut [u8], factor: f32) {
    for px in data.chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = (((*c as f32 - 128.0) * factor + 128.0).round()).clamp(0.0, 255.0) as u8;
        }
    }
}

fn saturate(data: &mut [u8], factor: f32) {
    for px in data.chunks_exact_mut(4) {
        let r = px[0] as f32;
        let g = px[1] as f32;
        let b = px[2] as f32;
        let gray = r * 0.2126 + g * 0.7152 + b * 0.0722;
        px[0] = (gray * (1.0 - factor) + r * factor).clamp(0.0, 255.0) as u8;
        px[1] = (gray * (1.0 - factor) + g * factor).clamp(0.0, 255.0) as u8;
        px[2] = (gray * (1.0 - factor) + b * factor).clamp(0.0, 255.0) as u8;
    }
}

fn brightness(data: &mut [u8], factor: f32) {
    for px in data.chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = (*c as f32 * factor).clamp(0.0, 255.0) as u8;
        }
    }
}

/// 可分离 box blur（重复 quality 次近似高斯），原地写回。
fn blur_rgba(data: &mut [u8], w: usize, h: usize, radius: usize, quality: usize) {
    if radius == 0 || w == 0 || h == 0 {
        return;
    }
    let mut tmp = vec![0u8; data.len()];
    for _ in 0..quality {
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0u32; 4];
                let mut count = 0u32;
                for dx in -(radius as i32)..=(radius as i32) {
                    let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let idx = (y * w + sx) * 4;
                    for c in 0..4 {
                        acc[c] += data[idx + c] as u32;
                    }
                    count += 1;
                }
                let idx = (y * w + x) * 4;
                for c in 0..4 {
                    tmp[idx + c] = (acc[c] / count) as u8;
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0u32; 4];
                let mut count = 0u32;
                for dy in -(radius as i32)..=(radius as i32) {
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let idx = (sy * w + x) * 4;
                    for c in 0..4 {
                        acc[c] += tmp[idx + c] as u32;
                    }
                    count += 1;
                }
                let idx = (y * w + x) * 4;
                for c in 0..4 {
                    data[idx + c] = (acc[c] / count) as u8;
                }
            }
        }
    }
}

/// AMLL 的 Mesh Gradient 渲染器（CPU 软件光栅化版）。
pub struct MeshGradientRenderer {
    layers: Vec<MeshLayer>,
    rng: Rng,
    output: Vec<u8>,
    scratch: Vec<u8>,
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
        let Some(mut texture) = prepare_texture(&rgba, width, height) else {
            return;
        };
        // 调色板纹理在创建时一次性转成 BGRA（gpui Frame 的字节序），
        // 这样每帧光栅输出可直接复用，无需再对整幅图做通道交换。
        for px in texture.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
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
        }
        let len = w * h * 4;
        if self.output.len() != len {
            self.output.resize(len, 0);
        }
        if self.scratch.len() != len {
            self.scratch.resize(len, 0);
        }
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

fn rasterize_layer(
    layer: &MeshLayer,
    out: &mut [u8],
    w: usize,
    h: usize,
    frame_time: f32,
    volume: f32,
) {
    let alpha = layer.alpha;
    // 鼓点同时驱动四种变化，保证在低分辨率、平滑封面上也看得出来：
    // 1) 网格整体向外呼吸（顶点级，颜色边界会移动）；
    // 2) UV 旋转加速；3) UV 缩小（纹理被“推近”）；4) 整体提亮。
    // 幅度相对上一版整体减半，避免安静场景里也显得过于剧烈；倍率由
    // `set_beat_strength` 再统一缩放。
    let beat = volume.clamp(0.0, 1.5);
    // 鼓点只改变颜色与几何，不降低整体不透明度：叠在黑色底上时任何 alpha
    // 下降都会被看成“背景变暗”。
    let alpha_volume = alpha;
    let rotation = (frame_time / 10_000.0 + beat * 0.35) * 2.0;
    let cos_r = rotation.cos();
    let sin_r = rotation.sin();
    let zoom = (1.0 - beat * 0.35).max(0.40);
    let mesh_scale = 1.0 + beat * 0.10;
    let light = 1.0 + beat * 0.25;

    for tri in layer.indices.chunks_exact(3) {
        let v0 = layer.vertices[tri[0] as usize];
        let v1 = layer.vertices[tri[1] as usize];
        let v2 = layer.vertices[tri[2] as usize];
        let p0 = (
            (v0.ndc[0] * mesh_scale + 1.0) * 0.5 * w as f32,
            (1.0 - v0.ndc[1] * mesh_scale) * 0.5 * h as f32,
        );
        let p1 = (
            (v1.ndc[0] * mesh_scale + 1.0) * 0.5 * w as f32,
            (1.0 - v1.ndc[1] * mesh_scale) * 0.5 * h as f32,
        );
        let p2 = (
            (v2.ndc[0] * mesh_scale + 1.0) * 0.5 * w as f32,
            (1.0 - v2.ndc[1] * mesh_scale) * 0.5 * h as f32,
        );
        let area = (p1.0 - p0.0) * (p2.1 - p0.1) - (p1.1 - p0.1) * (p2.0 - p0.0);
        if area.abs() < 1e-6 {
            continue;
        }
        let min_x = p0.0.min(p1.0).min(p2.0).floor().max(0.0) as usize;
        let max_x = (p0.0.max(p1.0).max(p2.0).ceil() as isize).clamp(0, w as isize) as usize;
        let min_y = p0.1.min(p1.1).min(p2.1).floor().max(0.0) as usize;
        let max_y = (p0.1.max(p1.1).max(p2.1).ceil() as isize).clamp(0, h as isize) as usize;
        let inv_area = 1.0 / area;

        // 重心坐标对 (px, py) 是线性的：每行/每像素只做加法递推。
        // w0 = A0 + B0*px + C0*py，其余由 1 - w0 - w1 得到。
        let b0 = -(p2.1 - p1.1) * inv_area;
        let c0 = (p2.0 - p1.0) * inv_area;
        let b1 = -(p0.1 - p2.1) * inv_area;
        let c1 = (p0.0 - p2.0) * inv_area;
        let a0 = (p1.0 * p2.1 - p1.1 * p2.0).mul_add(inv_area, 0.0);
        let a1 = (p2.0 * p0.1 - p2.1 * p0.0).mul_add(inv_area, 0.0);

        for y in min_y..max_y {
            let py = y as f32 + 0.5;
            let mut w0 = a0 + b0 * (min_x as f32 + 0.5) + c0 * py;
            let mut w1 = a1 + b1 * (min_x as f32 + 0.5) + c1 * py;
            let row_base = y * w;
            for x in min_x..max_x {
                let w2 = 1.0 - w0 - w1;
                if w0 >= -0.0001 && w1 >= -0.0001 && w2 >= -0.0001 {
                    let px = x as f32 + 0.5;
                    let iu = v0.uv[0] * w0 + v1.uv[0] * w1 + v2.uv[0] * w2;
                    let iv = v0.uv[1] * w0 + v1.uv[1] * w1 + v2.uv[1] * w2;
                    let uv_x = iu - 0.2;
                    let uv_y = iv - 0.2;
                    // GLSL mat2(c,-s,s,c) 是列主序：[c,s;-s,c]，即顺时针旋转。
                    let rx = (cos_r * uv_x + sin_r * uv_y) * zoom + 0.5;
                    let ry = (-sin_r * uv_x + cos_r * uv_y) * zoom + 0.5;
                    let texel = sample_texture(&layer.texture, rx, ry);
                    let dither = gradient_noise(px, py) / 255.0 - 0.5 / 255.0;
                    // AMLL: distance(v_uv, vec2(0.5))，中心在纹理 UV 中心；
                    // smoothstep(0.8, 0.3, dist) 的内联展开（无 powf）。
                    let du = iu - 0.5;
                    let dv = iv - 0.5;
                    let dist = (du * du + dv * dv).sqrt();
                    let t = ((0.8 - dist) * 2.0).clamp(0.0, 1.0);
                    let vignette = t * t * (3.0 - 2.0 * t);
                    let mask = 0.6 + vignette * 0.4;
                    let gain = mask * light;
                    let idx = (row_base + x) * 4;
                    out[idx] = ((texel[0] as f32 * gain).clamp(0.0, 255.0) + dither * 255.0)
                        .clamp(0.0, 255.0) as u8;
                    out[idx + 1] = ((texel[1] as f32 * gain).clamp(0.0, 255.0) + dither * 255.0)
                        .clamp(0.0, 255.0) as u8;
                    out[idx + 2] = ((texel[2] as f32 * gain).clamp(0.0, 255.0) + dither * 255.0)
                        .clamp(0.0, 255.0) as u8;
                    out[idx + 3] = (texel[3] as f32 * alpha_volume).clamp(0.0, 255.0) as u8;
                }
                w0 += b0;
                w1 += b1;
            }
        }
    }
}
#[inline(always)]
fn sample_texture(tex: &[u8], u: f32, v: f32) -> [u8; 4] {
    // MIRRORED_REPEAT（周期 2）：UV 折到 [0,1]。用「x - 2*floor(x/2)」代替
    // rem_euclid（fmod ~15ns 是逐像素热点）。
    let wrap = |x: f32| {
        let m = x - 2.0 * (x * 0.5).floor();
        if m > 1.0 { 2.0 - m } else { m }
    };
    let u = wrap(u);
    let v = wrap(v);
    let fx = u * TEXTURE_SIZE as f32 - 0.5;
    let fy = v * TEXTURE_SIZE as f32 - 0.5;
    let x0 = fx.floor() as i32;
    let y0 = fy.floor() as i32;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    // 纹素空间镜像重复：周期 64 = 2^6，位与代替整数取模（负数同样正确）。
    const PERIOD_MASK: i32 = TEXTURE_SIZE as i32 * 2 - 1;
    #[inline(always)]
    fn mirror(v: i32) -> usize {
        let m = (v & PERIOD_MASK) as usize;
        if m >= TEXTURE_SIZE {
            TEXTURE_SIZE * 2 - 1 - m
        } else {
            m
        }
    }
    #[inline(always)]
    fn at(tex: &[u8], x: i32, y: i32) -> [u8; 4] {
        let i = (mirror(y) * TEXTURE_SIZE + mirror(x)) * 4;
        [tex[i], tex[i + 1], tex[i + 2], tex[i + 3]]
    }
    let p00 = at(tex, x0, y0);
    let p10 = at(tex, x0 + 1, y0);
    let p01 = at(tex, x0, y0 + 1);
    let p11 = at(tex, x0 + 1, y0 + 1);
    #[inline(always)]
    fn lerp(a: u8, b: u8, t: f32) -> f32 {
        a as f32 + (b as f32 - a as f32) * t
    }
    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = lerp(p00[c], p10[c], tx);
        let bottom = lerp(p01[c], p11[c], tx);
        out[c] = (top + (bottom - top) * ty) as u8;
    }
    out
}

fn gradient_noise(x: f32, y: f32) -> f32 {
    let v = (x * 0.067_110_56 + y * 0.005_837_15).fract();
    (52.982_918 * v).fract()
}

fn blend_over(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let sa = s[3] as f32 / 255.0;
        if sa <= 0.0 {
            continue;
        }
        let da = d[3] as f32 / 255.0;
        let out_a = sa + da * (1.0 - sa);
        if out_a <= 0.0 {
            continue;
        }
        for c in 0..3 {
            let out = (s[c] as f32 * sa + d[c] as f32 * da * (1.0 - sa)) / out_a;
            d[c] = out.clamp(0.0, 255.0) as u8;
        }
        d[3] = (out_a * 255.0).clamp(0.0, 255.0) as u8;
    }
}

/// 纯色背景（AMLL `css-bg`）。实际绘制由 UI 层完成。
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
mod tests {
    use super::*;

    fn test_album() -> (u32, u32, Vec<u8>) {
        let mut data = vec![0u8; 64 * 64 * 4];
        for (i, px) in data.chunks_exact_mut(4).enumerate() {
            px[0] = (i % 256) as u8;
            px[1] = ((i / 3) % 256) as u8;
            px[2] = ((i / 7) % 256) as u8;
            px[3] = 255;
        }
        (64, 64, data)
    }

    #[test]
    fn texture_pipeline_is_fixed_size() {
        let (w, h, data) = test_album();
        let tex = prepare_texture(&data, w, h).unwrap();
        assert_eq!(tex.len(), TEXTURE_SIZE * TEXTURE_SIZE * 4);
        assert!(tex.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn mesh_rasterizes_cover() {
        let mut renderer = MeshGradientRenderer::with_seed(7);
        renderer.set_viewport(320, 180);
        renderer.set_render_scale(0.5);
        renderer.set_fps(60);
        let (w, h, data) = test_album();
        renderer.set_album_rgba(w, h, data);
        let mut image = None;
        for _ in 0..10 {
            image = renderer.render(1000.0).or(image);
        }
        let image = image.expect("render should produce an image");
        let frame = image.as_bytes(0).expect("frame bytes");
        assert!(frame.len() >= renderer.out_w * renderer.out_h * 4);
        let pixels = renderer.out_w * renderer.out_h;
        let opaque = frame.chunks_exact(4).filter(|p| p[3] > 200).count();
        assert!(
            opaque > pixels * 9 / 10,
            "mesh should cover the viewport: {opaque}/{pixels} opaque"
        );
    }

    #[test]
    fn non_square_preset_stays_in_bounds() {
        for (w, h) in [(4, 5), (5, 4)] {
            let mut layer = MeshLayer {
                preset: ControlPointPreset::grid(w, h),
                texture: vec![255; TEXTURE_SIZE * TEXTURE_SIZE * 4],
                vertices: Vec::new(),
                indices: Vec::new(),
                vertex_aspect: 0.0,
                alpha: 1.0,
                fading_out: false,
            };
            layer.rebuild_vertices(16.0 / 9.0);
            assert_eq!(
                layer.vertices.len(),
                (w - 1) * SUBDIVISIONS * (h - 1) * SUBDIVISIONS
            );
            let max_index = layer.indices.iter().copied().max().unwrap() as usize;
            assert!(max_index < layer.vertices.len());
            let x_count = (w - 1) * SUBDIVISIONS;
            let y_count = (h - 1) * SUBDIVISIONS;
            assert_eq!(layer.indices.len(), (x_count - 1) * (y_count - 1) * 6);
        }
    }

    #[test]
    fn vignette_is_centered() {
        let mut renderer = MeshGradientRenderer::with_seed(11);
        renderer.set_viewport(120, 120);
        renderer.set_render_scale(1.0);
        renderer.set_fps(60);
        let mut data = vec![0u8; 16 * 16 * 4];
        for px in data.chunks_exact_mut(4) {
            px.copy_from_slice(&[200, 200, 200, 255]);
        }
        renderer.set_album_rgba(16, 16, data);
        let mut image = None;
        for _ in 0..40 {
            if let Some(frame) = renderer.render(100.0) {
                image = Some(frame);
            }
        }
        let bytes = image.expect("frame").as_bytes(0).unwrap().to_vec();
        let (w, h) = renderer.debug_size();
        let center = (h / 2 * w + w / 2) * 4;
        let corner = 0;
        assert!(
            bytes[center] > bytes[corner].saturating_add(30),
            "center={} corner={}",
            bytes[center],
            bytes[corner]
        );
    }

    #[test]
    fn semi_transparent_album_preserves_alpha() {
        let mut renderer = MeshGradientRenderer::with_seed(7);
        renderer.set_viewport(60, 60);
        renderer.set_render_scale(1.0);
        renderer.set_fps(60);
        let mut data = vec![0u8; 8 * 8 * 4];
        for px in data.chunks_exact_mut(4) {
            px.copy_from_slice(&[200, 40, 40, 128]);
        }
        renderer.set_album_rgba(8, 8, data);
        let mut image = None;
        for _ in 0..40 {
            if let Some(frame) = renderer.render(100.0) {
                image = Some(frame);
            }
        }
        let image = image.expect("frame");
        let bytes = image.as_bytes(0).expect("bytes");
        let (w, h) = renderer.debug_size();
        let idx = ((h / 2) * w + w / 2) * 4;
        assert!(
            (100..=160).contains(&bytes[idx + 3]),
            "alpha = {}",
            bytes[idx + 3]
        );
    }

    #[test]
    fn output_pixels_are_capped() {
        let mut renderer = MeshGradientRenderer::new();
        renderer.set_viewport(4000, 3000);
        renderer.set_render_scale(1.0);
        renderer.ensure_output();
        assert!(renderer.out_w * renderer.out_h <= MAX_OUTPUT_PIXELS);
    }

    #[test]
    fn no_cover_returns_transparent_frame() {
        let mut renderer = MeshGradientRenderer::new();
        renderer.set_viewport(100, 100);
        let frame = renderer.render(16.0).expect("transparent frame");
        let bytes = frame.as_bytes(0).expect("bytes");
        assert!(bytes.chunks_exact(4).all(|p| p[3] == 0));
    }

    #[test]
    fn raster_cost_per_pixel_is_bounded() {
        let mut renderer = MeshGradientRenderer::with_seed(3);
        renderer.set_viewport(200, 120);
        renderer.set_render_scale(0.5);
        renderer.set_fps(60);
        let (w, h, data) = test_album();
        renderer.set_album_rgba(w, h, data);
        // 预热并让图层完全不透明。
        for _ in 0..8 {
            renderer.render(500.0);
        }
        let start = std::time::Instant::now();
        for _ in 0..20 {
            // > 1000/60：确保每次都真正光栅化，而不是命中 FPS 缓存。
            renderer.render(20.0);
        }
        let elapsed = start.elapsed().as_secs_f64() * 1e6;
        let per_frame = elapsed / 20.0;
        let pixels = (renderer.out_w * renderer.out_h) as f64;
        assert!(
            per_frame / pixels < 1.0,
            "bg raster too slow: {per_frame:.1}us/frame for {pixels} px"
        );
    }

    fn solid_album(rgb: [u8; 3]) -> (u32, u32, Vec<u8>) {
        let (w, h) = (32u32, 32u32);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for px in data.chunks_exact_mut(4) {
            px.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        (w, h, data)
    }

    fn mean_luma(image: &RenderImage) -> f64 {
        let bytes = image.as_bytes(0).expect("frame bytes");
        let mut sum = 0.0;
        let pixels = bytes.len() / 4;
        for px in bytes.chunks_exact(4) {
            // BGRA 顺序；三者之和的均值足以比较整体亮度。
            sum += px[0] as f64 * 0.114 + px[1] as f64 * 0.587 + px[2] as f64 * 0.299;
        }
        sum / pixels as f64
    }

    #[test]
    fn crossfade_advances_across_fps_gated_calls() {
        let mut renderer = MeshGradientRenderer::with_seed(5);
        renderer.set_viewport(200, 120);
        renderer.set_render_scale(0.5);
        renderer.set_fps(30);
        renderer.set_album_rgba(32, 32, solid_album([180, 90, 60]).2);
        // 模拟 200Hz 视图：每 5ms 调用一次 render，只有约每 7 次才真正光栅化。
        // 交叉淡入必须按墙钟推进，800ms 后应完全显示。
        let mut last = None;
        for _ in 0..160 {
            last = renderer.render(5.0).or(last);
        }
        let image = last.expect("frame");
        let frame = image.as_bytes(0).expect("bytes");
        let pixels = frame.len() / 4;
        let opaque = frame.chunks_exact(4).filter(|p| p[3] > 200).count();
        assert!(
            opaque * 100 >= pixels * 95,
            "crossfade too slow: {opaque}/{pixels} opaque"
        );
    }

    #[test]
    fn rapid_album_switches_never_darken() {
        const COLORS: [[u8; 3]; 4] = [
            [220, 60, 50],
            [40, 160, 210],
            [80, 200, 90],
            [220, 180, 60],
        ];
        let mut renderer = MeshGradientRenderer::with_seed(9);
        renderer.set_viewport(160, 92);
        renderer.set_render_scale(0.5);
        renderer.set_fps(30);
        renderer.set_album_rgba(32, 32, solid_album(COLORS[0]).2);
        for _ in 0..40 {
            renderer.render(8.0);
        }
        // 每 ~30ms 切一首（远快于 420ms 交叉淡出），持续 12 次。
        for i in 0..12 {
            renderer.set_album_rgba(32, 32, solid_album(COLORS[(i + 1) % COLORS.len()]).2);
            let mut last = None;
            for _ in 0..6 {
                last = renderer.render(5.0).or(last);
            }
            let image = last.expect("frame");
            let frame = image.as_bytes(0).expect("bytes");
            let pixels = frame.len() / 4;
            let opaque = frame.chunks_exact(4).filter(|p| p[3] > 200).count();
            assert!(
                opaque * 100 >= pixels * 90,
                "switch {i} darkened the mesh: {opaque}/{pixels} opaque"
            );
        }
    }

    #[test]
    fn beat_attack_is_fast_across_fps_gated_calls() {
        let mut renderer = MeshGradientRenderer::with_seed(13);
        renderer.set_viewport(160, 92);
        renderer.set_render_scale(0.5);
        renderer.set_fps(30);
        renderer.set_album_rgba(32, 32, solid_album([200, 120, 80]).2);
        for _ in 0..40 {
            renderer.render(8.0);
        }
        renderer.set_beat_level(1.0);
        // 100ms 内视图调用 20 次，但只会有约 3 个光栅帧。鼓点平滑也必须
        // 在这些被门控的调用中推进，否则 30fps 下起音会被拖慢。
        for _ in 0..20 {
            renderer.render(5.0);
        }
        assert!(
            renderer.beat_volume > 0.8,
            "beat attack too slow: {}",
            renderer.beat_volume
        );
    }

    #[test]
    fn beat_level_visibly_brightens_the_mesh() {
        let mut renderer = MeshGradientRenderer::with_seed(17);
        renderer.set_viewport(240, 135);
        renderer.set_render_scale(0.6);
        renderer.set_fps(60);
        let mut data = vec![0u8; 64 * 64 * 4];
        for (i, px) in data.chunks_exact_mut(4).enumerate() {
            px.copy_from_slice(&[
                90 + (i % 17) as u8,
                70 + (i % 23) as u8,
                50 + (i % 29) as u8,
                255,
            ]);
        }
        renderer.set_album_rgba(64, 64, data);
        renderer.set_beat_level(0.0);
        let mut quiet = None;
        for _ in 0..40 {
            quiet = renderer.render(16.0).or(quiet);
        }
        renderer.set_beat_level(1.0);
        let mut loud = None;
        for _ in 0..20 {
            loud = renderer.render(16.0).or(loud);
        }
        let quiet_luma = mean_luma(&quiet.expect("quiet frame"));
        let loud_luma = mean_luma(&loud.expect("loud frame"));
        assert!(
            loud_luma > quiet_luma * 1.08,
            "beat pulse too subtle: quiet={quiet_luma:.1} loud={loud_luma:.1}"
        );
    }

    #[test]
    fn zero_beat_strength_disables_the_pulse() {
        let (w, h, data) = test_album();
        let mut quiet = MeshGradientRenderer::with_seed(23);
        let mut loud = MeshGradientRenderer::with_seed(23);
        for renderer in [&mut quiet, &mut loud] {
            renderer.set_viewport(180, 100);
            renderer.set_render_scale(0.6);
            renderer.set_fps(60);
            renderer.set_beat_strength(0.0);
            renderer.set_album_rgba(w, h, data.clone());
            for _ in 0..40 {
                renderer.render(16.0);
            }
        }
        quiet.set_beat_level(0.0);
        loud.set_beat_level(1.0);
        let mut quiet_frame = None;
        let mut loud_frame = None;
        for _ in 0..12 {
            quiet_frame = quiet.render(16.0).or(quiet_frame);
            loud_frame = loud.render(16.0).or(loud_frame);
        }
        let quiet_frame = quiet_frame.expect("quiet frame");
        let loud_frame = loud_frame.expect("loud frame");
        assert_eq!(
            quiet_frame.as_bytes(0),
            loud_frame.as_bytes(0),
            "strength 0 should fully disable the beat pulse"
        );
    }
}
