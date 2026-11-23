//! background：palette（由 background.rs 拆分，行为不变）。

use super::*;

pub fn album_palette_bytes(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut data = prepare_texture(rgba, width, height)?;
    // 调色板纹理在创建时一次性转成 BGRA（gpui Frame 的字节序），
    // 这样每帧光栅输出可直接复用，无需再对整幅图做通道交换。
    for px in data.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    Some(data)
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

pub(super) fn prepare_texture(rgba: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
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

pub(super) fn contrast(data: &mut [u8], factor: f32) {
    for px in data.chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = (((*c as f32 - 128.0) * factor + 128.0).round()).clamp(0.0, 255.0) as u8;
        }
    }
}

pub(super) fn saturate(data: &mut [u8], factor: f32) {
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

pub(super) fn brightness(data: &mut [u8], factor: f32) {
    for px in data.chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = (*c as f32 * factor).clamp(0.0, 255.0) as u8;
        }
    }
}

/// 可分离 box blur（重复 quality 次近似高斯），原地写回。
pub(super) fn blur_rgba(data: &mut [u8], w: usize, h: usize, radius: usize, quality: usize) {
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
