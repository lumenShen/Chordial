#![allow(clippy::chunks_exact_to_as_chunks)]

//! 离屏渲染一张 AMLL Mesh Gradient 背景，便于无 GPU/无窗口环境检查视觉效果。
//!
//! usage: cargo run -p chordial-amll-gpui --example bg_snapshot -- /tmp/amll-bg.png

use chordial_amll_gpui::background::MeshGradientRenderer;

fn album(size: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 / size as f32;
            let fy = y as f32 / size as f32;
            // 蓝紫 → 橙红 的对角渐变 + 两块亮色，接近真实封面的色块分布。
            let r = (40.0 + 210.0 * fx * (0.4 + 0.6 * fy)).clamp(0.0, 255.0);
            let g = (30.0 + 120.0 * (1.0 - fx) * fy).clamp(0.0, 255.0);
            let b = (80.0 + 170.0 * (1.0 - fy) * (0.5 + 0.5 * fx)).clamp(0.0, 255.0);
            data.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
        }
    }
    data
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/amll-bg.png".to_string());
    let mut renderer = MeshGradientRenderer::with_seed(2026);
    renderer.set_viewport(960, 540);
    renderer.set_render_scale(1.0);
    renderer.set_fps(60);
    renderer.set_album_rgba(64, 64, album(64));

    let mut frame = None;
    for _ in 0..40 {
        if let Some(image) = renderer.render(100.0) {
            frame = Some(image);
        }
    }
    let image = frame.expect("background frame");
    let (w, h) = renderer.debug_size();
    let bytes = image.as_bytes(0).expect("frame bytes");
    // RenderImage 是 BGRA，保存为 PNG 前换回 RGBA。
    let mut rgba = bytes[..w * h * 4].to_vec();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    image::save_buffer(&out, &rgba, w as u32, h as u32, image::ColorType::Rgba8).expect("save png");
    println!("wrote {out} ({w}x{h})");
}
