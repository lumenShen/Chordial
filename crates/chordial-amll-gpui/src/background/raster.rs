//! background：raster（由 background.rs 拆分，行为不变）。

use super::*;

pub(super) fn rasterize_layer(
    layer: &MeshLayer,
    out: &mut [u8],
    w: usize,
    h: usize,
    frame_time: f32,
    bands: AudioBands,
) {
    let alpha = layer.alpha;
    // 控制点已经承载局部位移，这里只保留逐三角形/逐像素都能廉价表达的
    // 全局效果：鼓点脉冲、低频呼吸、人声推近、乐器旋转、空气感提亮。
    let kick = bands.kick.clamp(0.0, 1.5);
    let rumble = bands.rumble.clamp(0.0, 1.5);
    let vocals = bands.vocals.clamp(0.0, 1.5);
    let instruments = bands.instruments.clamp(0.0, 1.5);
    let air = bands.air.clamp(0.0, 1.5);
    // 音频只改变颜色与几何，不降低整体不透明度。
    let alpha_volume = alpha;
    let rotation = (frame_time / 10_000.0 + kick * 0.25 + instruments * 0.04) * 2.0;
    let cos_r = rotation.cos();
    let sin_r = rotation.sin();
    let zoom = (1.0 - kick * 0.25 + vocals * 0.02 - rumble * 0.01).max(0.40);
    let mesh_scale = 1.0 + kick * 0.04 + rumble * 0.025;
    let light = 1.0 + kick * 0.18 + rumble * 0.04 + air * 0.06;

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
pub(super) fn sample_texture(tex: &[u8], u: f32, v: f32) -> [u8; 4] {
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

pub(super) fn gradient_noise(x: f32, y: f32) -> f32 {
    let v = (x * 0.067_110_56 + y * 0.005_837_15).fract();
    (52.982_918 * v).fract()
}

pub(super) fn blend_over(dst: &mut [u8], src: &[u8]) {
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

/// 高分分辨率放大用的常驻中间缓冲。由渲染器持有，跨帧复用，
/// 避免 30fps 背景每帧分配几百 KiB 的临时图像。
pub(super) struct UpscaleScratch {
    /// 水平 pass 的 f32 结果（Cubic 允许轻微过冲，保留精度到垂直 pass）。
    tmp: Vec<f32>,
    base_x: Vec<i32>,
    weight_x: Vec<[f32; 4]>,
    base_y: Vec<i32>,
    weight_y: Vec<[f32; 4]>,
}

impl UpscaleScratch {
    pub(super) fn new() -> Self {
        Self {
            tmp: Vec::new(),
            base_x: Vec::new(),
            weight_x: Vec::new(),
            base_y: Vec::new(),
            weight_y: Vec::new(),
        }
    }

    pub(super) fn ensure(&mut self, dst_w: usize, dst_h: usize) {
        let len = dst_w * dst_h * 4;
        if self.tmp.len() != len {
            self.tmp.resize(len, 0.0);
        }
        if self.base_x.len() != dst_w {
            self.base_x.resize(dst_w, 0);
            self.weight_x.resize(dst_w, [0.0; 4]);
        }
        if self.base_y.len() != dst_h {
            self.base_y.resize(dst_h, 0);
            self.weight_y.resize(dst_h, [0.0; 4]);
        }
    }

    pub(super) fn clear(&mut self) {
        self.tmp.clear();
        self.tmp.shrink_to_fit();
        self.base_x.clear();
        self.weight_x.clear();
        self.base_y.clear();
        self.weight_y.clear();
    }

    /// 登记在 `Gauge::Background` 里的常驻字节数。
    pub(super) fn memory_bytes(&self) -> usize {
        self.tmp.len() * std::mem::size_of::<f32>()
            + self.base_x.len() * std::mem::size_of::<i32>()
            + self.weight_x.len() * std::mem::size_of::<[f32; 4]>()
            + self.base_y.len() * std::mem::size_of::<i32>()
            + self.weight_y.len() * std::mem::size_of::<[f32; 4]>()
    }
}

/// Catmull-Rom（a = -0.5）四个抽头的权重。保持线性斜坡不变形，
/// 且一阶导数连续——这正是双线性放大所缺的“丝滑”性质。
#[inline(always)]
fn catmull_rom_weights(t: f32, out: &mut [f32; 4]) {
    let t2 = t * t;
    let t3 = t2 * t;
    out[0] = -0.5 * t3 + t2 - 0.5 * t;
    out[1] = 1.5 * t3 - 2.5 * t2 + 1.0;
    out[2] = -1.5 * t3 + 2.0 * t2 + 0.5 * t;
    out[3] = 0.5 * t3 - 0.5 * t2;
}

fn build_axis_lut(dst: usize, src: usize, base: &mut [i32], weights: &mut [[f32; 4]]) {
    let scale = dst as f32 / src as f32;
    for i in 0..dst {
        // 中心对齐的采样坐标；左边缘钳到 0（放大时 src > 0.5，仅 i=0 可能 < 0）。
        let s = ((i as f32 + 0.5) / scale - 0.5).max(0.0);
        let b = s.floor() as i32;
        base[i] = b;
        catmull_rom_weights(s - b as f32, &mut weights[i]);
    }
}

#[inline(always)]
fn clamp_index(i: i32, max: usize) -> usize {
    (i as isize).clamp(0, max as isize) as usize
}

/// 极轻的确定性抖动，打散 8bit 量化在平滑渐变上的条带。
/// RGB 共用同一个值，避免放大后颜色噪点。
#[inline(always)]
fn upscale_dither(x: usize, y: usize) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h >> 24) as f32 / 255.0 - 0.5) * 0.7
}

/// 把低分辨率 BGRA 光栅用可分离 Catmull-Rom 放大到展示分辨率。
///
/// GPU 侧仍然是廉价双线性拉伸；这里先把源重建为 C1 连续的图像，
/// 消除大除数时双线性在每个源像素边界处的斜率突变（锯齿/不丝滑渐变）。
/// 边缘钳位取样，不会采到图集相邻图块，因此不需要额外的 padding。
pub(super) fn upscale_catmull_rom(
    src: &[u8],
    dst: &mut [u8],
    scratch: &mut UpscaleScratch,
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) {
    debug_assert_eq!(src.len(), src_w * src_h * 4);
    debug_assert_eq!(dst.len(), dst_w * dst_h * 4);
    scratch.ensure(dst_w, dst_h);
    build_axis_lut(dst_w, src_w, &mut scratch.base_x, &mut scratch.weight_x);
    build_axis_lut(dst_h, src_h, &mut scratch.base_y, &mut scratch.weight_y);

    let max_x = src_w - 1;
    let tmp = &mut scratch.tmp;
    for y in 0..src_h {
        let row = &src[y * src_w * 4..];
        for x in 0..dst_w {
            let b = scratch.base_x[x];
            let w = scratch.weight_x[x];
            let i0 = clamp_index(b - 1, max_x) * 4;
            let i1 = clamp_index(b, max_x) * 4;
            let i2 = clamp_index(b + 1, max_x) * 4;
            let i3 = clamp_index(b + 2, max_x) * 4;
            let o = (y * dst_w + x) * 4;
            for c in 0..4 {
                tmp[o + c] = row[i0 + c] as f32 * w[0]
                    + row[i1 + c] as f32 * w[1]
                    + row[i2 + c] as f32 * w[2]
                    + row[i3 + c] as f32 * w[3];
            }
        }
    }

    let max_y = src_h - 1;
    let stride = dst_w * 4;
    for y in 0..dst_h {
        let b = scratch.base_y[y];
        let w = scratch.weight_y[y];
        let r0 = clamp_index(b - 1, max_y) * stride;
        let r1 = clamp_index(b, max_y) * stride;
        let r2 = clamp_index(b + 1, max_y) * stride;
        let r3 = clamp_index(b + 2, max_y) * stride;
        for x in 0..dst_w {
            let o = x * 4;
            let d = upscale_dither(x, y);
            for c in 0..4 {
                let v = tmp[r0 + o + c] * w[0]
                    + tmp[r1 + o + c] * w[1]
                    + tmp[r2 + o + c] * w[2]
                    + tmp[r3 + o + c] * w[3]
                    + d;
                dst[y * stride + o + c] = v.clamp(0.0, 255.0) as u8;
            }
        }
    }
}
