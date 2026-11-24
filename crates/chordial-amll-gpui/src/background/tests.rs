//! background 测试（由 background.rs 拆分，行为不变）。

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
fn slow_raster_shrinks_divisor_one_output() {
    let mut renderer = MeshGradientRenderer::with_seed(3);
    renderer.set_viewport(682, 480);
    renderer.set_render_scale(1.0);
    // 模拟 debug 构建：200k 像素光栅耗时 40ms（0.2us/px）。
    renderer.adapt_pixel_budget(40_000.0, 200_000);
    assert_eq!(renderer.debug_pixel_budget(), 50_000);
    renderer.ensure_output();
    let (rw, rh) = renderer.debug_raster_size();
    let pixels = rw * rh;
    assert!(
        (MIN_ADAPTIVE_PIXELS..=50_000).contains(&pixels),
        "adaptive raster should be ~50k px, got {pixels}"
    );
    assert!(pixels < MAX_OUTPUT_PIXELS);
}

#[test]
fn fast_raster_keeps_hard_cap() {
    let mut renderer = MeshGradientRenderer::new();
    renderer.set_viewport(682, 480);
    renderer.set_render_scale(1.0);
    // release 级速度：5ns/px，预算远大于硬上限，不应降低画质。
    renderer.adapt_pixel_budget(1_000.0, 200_000);
    assert_eq!(renderer.debug_pixel_budget(), MAX_OUTPUT_PIXELS);
    renderer.ensure_output();
    let (rw, rh) = renderer.debug_raster_size();
    assert_eq!(rw * rh, 199_875);
}

#[test]
fn crossfade_budget_accounts_for_both_layers() {
    let mut renderer = MeshGradientRenderer::with_seed(3);
    let (w, h, data) = test_album();
    renderer.set_album_rgba(w, h, data.clone());
    renderer.set_album_rgba(w, h, data);
    assert_eq!(renderer.layers.len(), 2);
    // 光栅 50k 像素 × 两层，总耗时 40ms；预算应只剩 12.5k 光栅像素。
    renderer.adapt_pixel_budget(40_000.0, 50_000);
    assert_eq!(renderer.debug_pixel_budget(), 12_500);
}

#[test]
fn small_estimate_changes_do_not_thrash_output_size() {
    let mut renderer = MeshGradientRenderer::new();
    renderer.set_viewport(682, 480);
    renderer.set_render_scale(1.0);
    renderer.pixel_budget = 50_000;
    // 新预算约 54k，仅比当前高 8%，应被滞回拦住。
    renderer.adapt_pixel_budget(18_520.0, 100_000);
    assert_eq!(renderer.debug_pixel_budget(), 50_000);
    renderer.ensure_output();
    let first = renderer.debug_raster_size();
    renderer.adapt_pixel_budget(18_520.0, 100_000);
    renderer.ensure_output();
    assert_eq!(renderer.debug_raster_size(), first);
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

#[test]
fn recycled_frame_buffer_is_reused_by_next_render() {
    let (w, h, data) = test_album();
    let mut renderer = MeshGradientRenderer::new();
    renderer.set_viewport(64, 64);
    renderer.set_album_rgba(w, h, data);
    let frame = renderer.render(16.0).expect("first frame");
    // 模拟 UI 侧 drop_image 后回收：取出渲染器持有的引用，拿到像素缓冲。
    let owned = renderer.take_last_image().unwrap_or(frame);
    let buffer = Arc::try_unwrap(owned)
        .expect("renderer is the only holder")
        .into_frames()
        .into_iter()
        .next()
        .expect("one frame")
        .into_buffer()
        .into_raw();
    let len = buffer.len();
    renderer.recycle_output(buffer);
    assert_eq!(renderer.debug_recycled(), 1);

    // 下一帧从回收池取回同一尺寸的缓冲（不再分配）。
    renderer.ensure_output();
    assert_eq!(renderer.debug_recycled(), 0);
    assert_eq!(renderer.output.len(), len);
    assert_eq!(renderer.output.capacity(), len);
}

#[test]
fn upscale_keeps_linear_ramp_linear() {
    // 双线性放大在源像素边界处斜率突变；Cubic 重建必须保持线性斜坡平滑。
    const SW: usize = 32;
    const SH: usize = 8;
    const F: usize = 4;
    let mut src = vec![0u8; SW * SH * 4];
    for y in 0..SH {
        for x in 0..SW {
            let v = (24 + x * 6) as u8;
            let o = (y * SW + x) * 4;
            src[o..o + 4].copy_from_slice(&[v, v, v, 255]);
        }
    }
    let (dw, dh) = (SW * F, SH * F);
    let mut dst = vec![0u8; dw * dh * 4];
    let mut scratch = UpscaleScratch::new();
    upscale_catmull_rom(&src, &mut dst, &mut scratch, SW, SH, dw, dh);

    // Cubic 重建必须保持线性（双线性会在每个源像素处出现平台）。
    // 末端抖动只有 ±0.35 LSB，逐像素偏差应远小于一个量化台阶造成的折线。
    for y in [0, dh / 2, dh - 1] {
        for x in 0..dw {
            let v = dst[(y * dw + x) * 4] as f32;
            let expected = 24.0 + x as f32 * (210.0 - 24.0) / (dw - 1) as f32;
            assert!(
                (v - expected).abs() <= 4.0,
                "ramp bent at ({x},{y}): {v} vs {expected:.1}"
            );
        }
    }
}

#[test]
fn upscale_has_no_black_border() {
    let (sw, sh, f) = (9usize, 7usize, 3usize);
    let color = [120u8, 80, 60, 255];
    let src = color.repeat(sw * sh);
    // 边缘钳位取样：纯色源放大后不能出现来自相邻图块的黑边。
    let (dw, dh) = (sw * f, sh * f);
    let mut dst = vec![0u8; dw * dh * 4];
    let mut scratch = UpscaleScratch::new();
    upscale_catmull_rom(&src, &mut dst, &mut scratch, sw, sh, dw, dh);
    for y in 0..dh {
        for x in 0..dw {
            let o = (y * dw + x) * 4;
            for c in 0..4 {
                assert!(
                    dst[o + c].abs_diff(color[c]) <= 1,
                    "border artifact at ({x},{y}) channel {c}: {} vs {}",
                    dst[o + c],
                    color[c]
                );
            }
        }
    }
}

#[test]
fn large_divisor_upscales_to_capped_resolution() {
    for (divisor, want) in [(8u32, 2.0f32), (12, 3.0)] {
        let mut renderer = MeshGradientRenderer::new();
        renderer.set_viewport(1920, 1080);
        renderer.set_render_scale(1.0 / divisor as f32);
        renderer.ensure_output();
        let (rw, rh) = renderer.debug_raster_size();
        let (pw, ph) = renderer.debug_size();
        let factor = renderer.debug_upscale();
        assert!(
            (factor - want).abs() < 0.35,
            "divisor {divisor}: upscale {factor} (raster {rw}x{rh}, present {pw}x{ph})"
        );
        assert!(pw * ph <= MAX_PRESENT_PIXELS + 1_000);
        assert!(pw * ph >= MAX_PRESENT_PIXELS / 2);
    }
}

#[test]
fn upscale_cost_stays_bounded() {
    let (w, h, data) = test_album();
    let mut renderer = MeshGradientRenderer::with_seed(3);
    renderer.set_viewport(1920, 1080);
    renderer.set_render_scale(1.0 / 12.0);
    renderer.set_fps(60);
    renderer.set_album_rgba(w, h, data);
    for _ in 0..4 {
        renderer.render(100.0);
    }
    let start = std::time::Instant::now();
    for _ in 0..20 {
        renderer.render(20.0);
    }
    let per_frame = start.elapsed().as_secs_f64() * 1e6 / 20.0;
    // debug 构建未开优化，这里只做 O(n²) 退化护栏，不作为真机帧率承诺；
    // 真机耗时用 scale_probe / perf 浮窗测量。
    assert!(
        per_frame < 50_000.0,
        "large-divisor background too slow: {per_frame:.0}us/frame"
    );
    assert!(renderer.debug_upscale() > 1.0);
}
