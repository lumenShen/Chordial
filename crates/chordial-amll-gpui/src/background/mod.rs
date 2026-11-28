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
//! - 光栅分辨率严格等于 viewport × render_scale；`render_scale = 1 / 除数`，
//!   不再按实测耗时偷偷缩小分辨率，保证设置档位与实际画面一致。
//! - 三角形拓扑在专辑/宽高比变化时才重建；音频活跃时只复用拓扑更新顶点，
//!   每帧仍只做一次光栅化 + 合成。
//! - 低于目标 FPS 的帧直接复用上一张 `RenderImage`（零分配、零光栅化）。
//! - 所有热路径埋 `amll.bg.raster` 检查点。

use std::sync::Arc;

use chordal_core::mem::{self, Gauge};
use chordal_core::perf::PerfScope;
use chordal_core::{AudioBandKind, AudioBands, AudioControlMapping};
use gpui::{Hsla, RenderImage};
use image::{Frame, RgbaImage};

mod palette;
mod raster;
mod worker;

pub use palette::*;
use raster::*;
pub(crate) use worker::{BackgroundController, FrameOutput};

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
/// 交给 GPU 放大前的展示分辨率预算。除数较大时先把光栅结果用 Cubic 重建
/// 到接近该预算，GPU 那一步双线性拉伸的斜率突变就落在更细的网格上，
/// 渐变更连续；CPU 开销仍远低于按除数直接提高光栅分辨率。
const MAX_PRESENT_PIXELS: usize = 130_000;
/// 预放大的上限倍率，避免展示缓冲过度膨胀。
const MAX_UPSCALE: f32 = 4.0;
const SUBDIVISIONS: usize = 12;
/// 封面交叉淡入淡出的时长（毫秒）。使用真实墙钟时间推进，与视图帧率解耦：
/// 高刷屏上视图可能每秒调用 200+ 次 `render`，但光栅只有 30fps。
const CROSSFADE_MS: f32 = 420.0;
/// 五路视觉包络的跟随时间常数（秒）：顺序为 rumble / kick / vocals / instruments / air。
const BAND_ATTACK_SECS: [f32; 5] = [0.08, 0.012, 0.035, 0.025, 0.008];
const BAND_RELEASE_SECS: [f32; 5] = [0.24, 0.12, 0.18, 0.2, 0.1];

#[derive(Debug, Clone, Copy, Default)]
struct Vertex {
    /// NDC 坐标（-1..1，y 向上）。
    ndc: [f32; 2],
    /// 0..1 纹理坐标。
    uv: [f32; 2],
}

#[derive(Debug, Clone)]
struct MeshLayer {
    /// 不被音频修改的随机网格，作为每帧反应式形变的起点。
    base_preset: ControlPointPreset,
    preset: ControlPointPreset,
    texture: Vec<u8>, // RGBA, 32x32
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    vertex_aspect: f32,
    alpha: f32,
    /// 是否已经从画面中淡出。
    fading_out: bool,
    /// 当前顶点是否包含音频形变；归零后需要恢复一次 base 网格。
    audio_deformed: bool,
}

impl MeshLayer {
    fn rebuild_vertices(&mut self, aspect: f32, force: bool) {
        let topology_dirty =
            (self.vertex_aspect - aspect).abs() >= 0.001 || self.vertices.is_empty();
        if !force && !topology_dirty {
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

        if topology_dirty || self.indices.is_empty() {
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
    // 5×5 提供九个内部锚点，足够让五路频段分别控制不同区域；边界仍保持
    // 规则网格，避免音频形变把封面推出视口。
    let w = 5;
    let h = 5;
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

const CONTROL_EPSILON: f32 = 0.0005;

/// 从 base 控制点重新计算反应式网格。函数不累加历史形变，因此长时间播放
/// 也不会漂移；边界点始终保持不变，保证网格继续覆盖整个视口。
fn apply_audio_controls(
    layer: &mut MeshLayer,
    bands: AudioBands,
    mapping: AudioControlMapping,
    time: f32,
) {
    let MeshLayer {
        base_preset,
        preset,
        ..
    } = layer;
    preset.points.copy_from_slice(&base_preset.points);
    if !bands.is_active(CONTROL_EPSILON) {
        return;
    }

    let rumble_phase = time * 2.4;
    let vocal_phase = time * 0.55;
    let instrument_phase = time * 0.8;
    let air_phase = time * 9.0;
    for y in 1..preset.height.saturating_sub(1) {
        for x in 1..preset.width.saturating_sub(1) {
            let index = y * preset.width + x;
            let base = base_preset.points[index];
            let rumble = mapping.get(AudioBandKind::Rumble).contains(index) as u8 as f32;
            let kick = mapping.get(AudioBandKind::Kick).contains(index) as u8 as f32;
            let vocals = mapping.get(AudioBandKind::Vocals).contains(index) as u8 as f32;
            let instruments = mapping.get(AudioBandKind::Instruments).contains(index) as u8 as f32;
            let air = mapping.get(AudioBandKind::Air).contains(index) as u8 as f32;

            let expansion = 1.0
                + rumble * bands.rumble * 0.018
                + kick * bands.kick * 0.14
                + vocals * bands.vocals * 0.07;
            let p = &mut preset.points[index];
            p.x = base.x * expansion
                + rumble * bands.rumble * (rumble_phase + base.y * 2.2).sin() * 0.018
                + vocals * bands.vocals * vocal_phase.sin() * 0.025
                + instruments * bands.instruments * (0.5 - base.y) * 0.035
                + air * bands.air * (air_phase + base.x * 5.0 + base.y * 3.0).sin() * 0.006;
            p.y = base.y * expansion
                + rumble * bands.rumble * (rumble_phase * 0.73).cos() * 0.012
                + vocals * bands.vocals * 0.035
                + instruments * bands.instruments * instrument_phase.sin() * 0.018
                + air * bands.air * (air_phase + base.x * 3.0 - base.y * 4.0).cos() * 0.005;
            p.u_scale *= 1.0 + kick * bands.kick * 0.22 + vocals * bands.vocals * 0.12
                - air * bands.air * 0.05;
            p.v_scale *= 1.0 + kick * bands.kick * 0.16 + instruments * bands.instruments * 0.1;
            p.u_rot += vocals * bands.vocals * 6.0 + instruments * bands.instruments * 5.0;
            p.v_rot += air * bands.air * (air_phase + base.x * 4.0 + base.y * 2.0).sin() * 4.0
                + instruments * bands.instruments * 3.0;
        }
    }
}

fn restore_base_controls(layer: &mut MeshLayer) {
    layer
        .preset
        .points
        .copy_from_slice(&layer.base_preset.points);
}

/// 生成 32×32 专辑调色纹理（BGRA 字节），供渲染层/视图缓存。
///
/// 相比保留整幅封面 RGBA（512² ≈ 1 MiB），调色纹理只有 4 KiB；切换背景
/// 渲染器时用它重建即可，视觉等价。
pub struct MeshGradientRenderer {
    layers: Vec<MeshLayer>,
    rng: Rng,
    /// 低分辨率软件光栅缓冲（常驻，不交给 GPU）。
    raster: Vec<u8>,
    /// `raster` 的逐图层暂存（常驻）。
    scratch: Vec<u8>,
    /// 交给 GPU 的展示帧：大除数时是 Cubic 重建后的图，否则等于 `raster`。
    output: Vec<u8>,
    /// Cubic 预放大的常驻中间缓冲（f32 水平 pass + 两轴采样 LUT）。
    upscale_scratch: UpscaleScratch,
    /// 从已释放的 `RenderImage` 回收的整帧像素缓冲（尺寸匹配时复用）。
    /// 软件光栅每帧都要交出一块 output，回收可消除 30fps × 帧大小的分配抖动。
    recycled: Vec<Vec<u8>>,
    raster_w: usize,
    raster_h: usize,
    out_w: usize,
    out_h: usize,
    /// raster → output 的放大倍率；1.0 表示不需要预放大。
    upscale: f32,
    viewport_w: usize,
    viewport_h: usize,
    render_scale: f32,
    flow_speed: f32,
    fps: u32,
    fps_accum_ms: f32,
    frame_time: f32,
    /// 外部五频段包络目标。
    band_targets: AudioBands,
    /// 经过独立起音/回落平滑后的五频段强度。
    band_values: AudioBands,
    /// 与 flow_speed 解耦的反应式相位，flow_speed=0 时音频仍可驱动网格。
    reactive_time: f32,
    /// 五路频段分别控制哪些内部点。
    control_mapping: AudioControlMapping,
    /// 全部音频反应视觉的整体倍率（0..=1.5，1.0 = 原始幅度）。
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
            raster: Vec::new(),
            scratch: Vec::new(),
            output: Vec::new(),
            upscale_scratch: UpscaleScratch::new(),
            recycled: Vec::new(),
            raster_w: 0,
            raster_h: 0,
            out_w: 0,
            out_h: 0,
            upscale: 1.0,
            viewport_w: 0,
            viewport_h: 0,
            render_scale: 0.5,
            flow_speed: 4.0,
            fps: 30,
            fps_accum_ms: 0.0,
            frame_time: 0.0,
            band_targets: AudioBands::default(),
            band_values: AudioBands::default(),
            reactive_time: 0.0,
            control_mapping: AudioControlMapping::default(),
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
        self.band_targets.rumble = volume.clamp(0.0, 1.0) / 10.0;
    }

    /// 一次设置五路频段目标；渲染层会在现有光栅帧预算内独立平滑。
    pub fn set_audio_bands(&mut self, bands: AudioBands) {
        self.band_targets = bands.sanitized();
    }

    /// 更新五路频段到内部控制点的分配；边界点始终由渲染器保留。
    pub fn set_audio_control_mapping(&mut self, mapping: AudioControlMapping) {
        let mapping = mapping.sanitized();
        if mapping != self.control_mapping {
            self.control_mapping = mapping;
            self.needs_clear = true;
        }
    }

    /// 0..1 的鼓点包络，兼容旧 API；只驱动 `kick` 通道。
    pub fn set_beat_level(&mut self, level: f32) {
        self.band_targets.kick = level.clamp(0.0, 1.0);
    }

    /// 全部音频反应视觉的整体倍率：0 = 完全关闭（网格仍会缓慢流动），
    /// 1.0 = 原始幅度，上限 1.5。只缩放五路视觉控制，不改变包络时序。
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
        let base_preset = generate_preset(&mut self.rng);
        let preset = base_preset.clone();
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
            base_preset,
            preset,
            texture,
            vertices: Vec::new(),
            indices: Vec::new(),
            vertex_aspect: 0.0,
            alpha: if instant { 1.0 } else { 0.0 },
            fading_out: false,
            audio_deformed: false,
        };
        layer.rebuild_vertices(aspect, false);
        if instant {
            self.layers.clear();
        } else {
            // 快速连切时只保留当前画面最上面的一层作为交叉淡出的底色。
            // 旧实现会累积最多 3 层半透明封面：每次切歌把所有旧层同时淡出、
            // 新层从 0 淡入，合成后的 alpha 明显小于 1；叠在黑色底上就表现
            // 为背景“异常变黑”。两层交叉淡出可以保证底层始终不透。
            if let Some(mut outgoing) = self.layers.pop() {
                self.layers.clear();
                outgoing.fading_out = true;
                // 退场层必须保持不透明。它可能还在淡入（alpha < 1），也可能
                // 已经被 `clear_album` 标记为淡出（停止播放后立刻切歌）：
                // 两种情况下若让它以半透明做底色，新封面淡入期间整块背景
                // 都会透出黑色，看起来就是“换歌瞬间背景变黑”。
                outgoing.alpha = 1.0;
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
        // 设置除数就是唯一的光栅尺寸依据。先取整到像素，再保留渲染所需的
        // 最小 2×2；不能套像素预算或按耗时继续缩小，否则 ÷1/÷2 会悄悄
        // 变成更大的有效除数，用户看到的清晰度与设置不一致。
        let w = ((self.viewport_w as f32) * self.render_scale).ceil() as usize;
        let h = ((self.viewport_h as f32) * self.render_scale).ceil() as usize;
        self.raster_w = w.max(2);
        self.raster_h = h.max(2);

        // 光栅像素太少时，先做一次 Cubic 重建再交给 GPU 拉伸。
        // 展示预算与光栅预算相互独立：大除数下一张 ~480×270 的重建图
        // 足以消除双线性接缝，而真正的网格光栅仍只花 1/N² 的成本。
        let raster_pixels = w.saturating_mul(h) as f32;
        let factor = (MAX_PRESENT_PIXELS as f32 / raster_pixels)
            .sqrt()
            .min(MAX_UPSCALE)
            .max(1.0);
        let mut pw = ((w as f32 * factor).round() as usize).max(w);
        let mut ph = ((h as f32 * factor).round() as usize).max(h);
        // 四舍五入可能略微越过预算，等比缩回一档即可；最多两次迭代。
        for _ in 0..2 {
            if pw.saturating_mul(ph) <= MAX_PRESENT_PIXELS {
                break;
            }
            let k = (MAX_PRESENT_PIXELS as f32 / (pw * ph) as f32).sqrt();
            pw = ((pw as f32 * k).floor() as usize).max(w);
            ph = ((ph as f32 * k).floor() as usize).max(h);
        }
        // 两个轴独立四舍五入，宽度倍率可能正好是 1.0 而高度多出 1 像素。
        // 取两轴较大值，保证倍率准确反映展示缓冲是否真的与光栅同尺寸。
        let upscale = (pw as f32 / w as f32).max(ph as f32 / h as f32);
        // 只有尺寸或放大倍率变化才需要重清缓冲；帧缓冲被 take 走（零拷贝交帧）后
        // 下一帧只需补容量，不能借此触发 needs_clear，否则会绕过 FPS 门控
        // 变成每帧全量光栅。
        if pw != self.out_w || ph != self.out_h || (upscale - self.upscale).abs() > 0.001 {
            self.out_w = pw;
            self.out_h = ph;
            self.upscale = upscale;
            self.needs_clear = true;
            // 尺寸变化后旧尺寸的回收块不再可用。
            self.recycled.clear();
        }
        let raster_len = w * h * 4;
        let present_len = pw * ph * 4;
        if self.raster.len() != raster_len {
            self.raster.resize(raster_len, 0);
        }
        if self.scratch.len() != raster_len {
            self.scratch.resize(raster_len, 0);
        }
        if self.output.len() != present_len {
            // 优先复用刚从旧 RenderImage 回收的整帧缓冲（零分配 + 零清零）。
            if self
                .recycled
                .last()
                .is_some_and(|buf| buf.len() == present_len)
            {
                self.output = self.recycled.pop().expect("checked");
            } else {
                self.output.resize(present_len, 0);
            }
        }
        self.upscale_scratch.ensure(pw, ph);
        // raster/scratch 常驻，output 每帧被 take 走交给 gpui 后再回收；
        // 展示 pass 的 f32 中间缓冲也常驻，统一登记便于核对内存占用。
        mem::set_gauge(
            Gauge::Background,
            raster_len
                .saturating_mul(2)
                .saturating_add(present_len)
                .saturating_add(self.upscale_scratch.memory_bytes()),
        );
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
        // 没有时间推进就不推进、也不裁剪任何图层。视图可能在同一帧内被
        // 多次 `render`（第二次 dt=0），或暂停恢复时先收到一帧 0 时长；
        // 此时 fade=0，若继续走下面的 retain，会把刚加入、alpha 仍为 0 的
        // 新封面直接裁掉。旧封面随后淡出，`layers` 清空后背景就永久黑屏，
        // 直到下一次换封面（表现为切歌后 0.42s 淡出、之后全黑）。
        if !(dt_ms > 0.0) {
            return;
        }
        self.reactive_time = (self.reactive_time + dt_ms * 0.001).rem_euclid(10_000.0);
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
        // 只移除已经淡出的旧图层。新加入的图层即使 alpha 仍为 0 也必须
        // 保留：它是下一帧交叉淡入的主体，提前裁掉会让背景只剩一个正在
        // 淡出的旧层，最终变成透明（黑屏）。
        self.layers
            .retain(|layer| !layer.fading_out || layer.alpha > 0.001);
        // 当前封面完全显示后，退场的旧封面已无意义，立即丢弃。
        if self.layers.len() > 1 && self.layers.last().is_some_and(|l| l.alpha >= 1.0) {
            self.layers.drain(0..self.layers.len() - 1);
        }

        let targets = [
            self.band_targets.rumble,
            self.band_targets.kick,
            self.band_targets.vocals,
            self.band_targets.instruments,
            self.band_targets.air,
        ];
        let mut values = [
            self.band_values.rumble,
            self.band_values.kick,
            self.band_values.vocals,
            self.band_values.instruments,
            self.band_values.air,
        ];
        let dt = dt_ms * 0.001;
        for index in 0..5 {
            let tau = if targets[index] > values[index] {
                BAND_ATTACK_SECS[index]
            } else {
                BAND_RELEASE_SECS[index]
            };
            let k = 1.0 - (-dt / tau).exp();
            values[index] += (targets[index] - values[index]) * k;
            if !values[index].is_finite() {
                values[index] = 0.0;
            }
            values[index] = values[index].clamp(0.0, 1.0);
        }
        self.band_values = AudioBands {
            rumble: values[0],
            kick: values[1],
            vocals: values[2],
            instruments: values[3],
            air: values[4],
        };
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
        } else {
            if !self.needs_clear {
                return self.last_image.clone();
            }
            // 暂停时没有时间推进交叉淡入。若新封面刚加入、alpha 仍为 0，
            // 直接用 alpha=0 光栅化会得到一帧全透明图（停在黑色底上），
            // 且暂停期间不会再有帧把它补上。按完成态显示更符合预期。
            if let Some(top) = self.layers.iter_mut().rev().find(|layer| !layer.fading_out) {
                if top.alpha <= 0.0 {
                    top.alpha = 1.0;
                }
            }
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

        let aspect = self.raster_w as f32 / self.raster_h as f32;
        // 包络只决定“什么时候跳”，强度决定“跳多大幅度”。五路共用同一个
        // 全局倍率，设置为 0 时几何与光栅效果同时归零。
        let strength = self.beat_strength;
        let controls = AudioBands {
            rumble: self.band_values.rumble * strength,
            kick: self.band_values.kick * strength,
            vocals: self.band_values.vocals * strength,
            instruments: self.band_values.instruments * strength,
            air: self.band_values.air * strength,
        };
        let reactive_time = self.reactive_time;
        let scope = PerfScope::enter("amll.bg.raster");
        self.raster.fill(0);
        let mut used = 0u64;

        for layer in &mut self.layers {
            if controls.is_active(CONTROL_EPSILON) {
                apply_audio_controls(layer, controls, self.control_mapping, reactive_time);
                layer.audio_deformed = true;
                layer.rebuild_vertices(aspect, true);
            } else if layer.audio_deformed {
                restore_base_controls(layer);
                layer.audio_deformed = false;
                layer.rebuild_vertices(aspect, true);
            } else {
                layer.rebuild_vertices(aspect, false);
            }
            self.scratch.fill(0);
            rasterize_layer(
                layer,
                &mut self.scratch,
                self.raster_w,
                self.raster_h,
                self.frame_time,
                controls,
            );
            blend_over(&mut self.raster, &mut self.scratch);
            used += (self.raster_w * self.raster_h) as u64;
        }
        scope.finish(used);

        // 低分辨率光栅 → Cubic 重建 → GPU 双线性到全屏。
        // 只有展示缓冲与光栅缓冲尺寸完全一致时才走 memcpy；不能用倍率阈值判断，
        // 因为宽高分别取整后可能只差 1 像素而倍率仍接近 1.0。
        let upscale_scope = PerfScope::enter("amll.bg.upscale");
        if self.out_w != self.raster_w || self.out_h != self.raster_h {
            upscale_catmull_rom(
                &self.raster,
                &mut self.output,
                &mut self.upscale_scratch,
                self.raster_w,
                self.raster_h,
                self.out_w,
                self.out_h,
            );
        } else {
            self.output.copy_from_slice(&self.raster);
        }
        upscale_scope.finish((self.out_w * self.out_h) as u64);
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
        self.raster.clear();
        self.raster.shrink_to_fit();
        self.scratch.clear();
        self.scratch.shrink_to_fit();
        self.upscale_scratch.clear();
        self.recycled.clear();
        mem::set_gauge(Gauge::Background, 0);
    }

    #[doc(hidden)]
    pub fn debug_size(&self) -> (usize, usize) {
        (self.out_w, self.out_h)
    }

    #[doc(hidden)]
    pub fn debug_upscale(&self) -> f32 {
        self.upscale
    }

    #[doc(hidden)]
    pub fn debug_raster_size(&self) -> (usize, usize) {
        (self.raster_w, self.raster_h)
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
    /// 一次设置五路频段包络，见 [`MeshGradientRenderer::set_audio_bands`]。
    pub fn set_audio_bands(&mut self, bands: AudioBands) {
        if let Self::Mesh(r) = self {
            r.set_audio_bands(bands);
        }
    }
    /// 五路频段到控制点的映射，见 [`MeshGradientRenderer::set_audio_control_mapping`]。
    pub fn set_audio_control_mapping(&mut self, mapping: AudioControlMapping) {
        if let Self::Mesh(r) = self {
            r.set_audio_control_mapping(mapping);
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
    pub fn dispose(&mut self) {
        if let Self::Mesh(renderer) = self {
            renderer.dispose();
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
