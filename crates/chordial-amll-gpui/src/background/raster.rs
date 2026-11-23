//! background：raster（由 background.rs 拆分，行为不变）。

use super::*;

pub(super) fn rasterize_layer(
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
