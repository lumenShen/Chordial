//! emphasis（由 render.rs 拆分，行为不变）。

use super::*;
/// AMLL `initEmphasizeAnimation` 的等价实现（按强调分组而非单字计算）。
pub(super) fn emphasize_glyph(
    chunk: &EmphasisChunk,
    ordinal: usize,
    line: &LyricLine,
    time: u64,
    enabled: bool,
    font_size: f32,
) -> Option<EmphasisFrame> {
    if !chunk.emphasized || time < line.start_time {
        return None;
    }
    let char_count = chunk.char_count.max(1);
    let ordinal = ordinal.min(char_count - 1);

    // 时长/幅度/辉光/延迟在整形时已按分组算好（见 EmphasisChunk），每帧
    // 只剩时间相关的缓动求值。
    let du = chunk.duration_ms;
    let amount = chunk.amount;
    let blur = chunk.blur;
    let delay = chunk.delay_ms;
    let per_char_delay = du / 2.5 / char_count as f32 * ordinal as f32;
    let local = (time - line.start_time) as f32;
    let x = ((local - delay - per_char_delay) / du).clamp(0.0, 1.0);
    if x <= 0.0 {
        return None;
    }
    let trans = emp_easing(x);

    // AMLL 的第二次动画（createCharFloatAnimation）：时长 `du * 1.4`，延迟与
    // 辉光动画相同；Y 在整段内按 `sin(πx)·up + x·up` 上浮（BG 两倍 up），
    // 结束后停在 -up。行离开热行集合时 AMLL 把该动画 playbackRate 设为 -1，
    // 这里用“行结束时刻的相位 - 离场后经过的时间”还原同一倒放轨迹。
    let float_duration = chunk.float_duration_ms;
    let float_delay = delay + per_char_delay;
    let float_phase_at = |local: f32| ((local - float_delay) / float_duration).clamp(0.0, 1.0);
    let float_x = if enabled {
        float_phase_at(local)
    } else {
        let at_end = float_phase_at(line.end_time.saturating_sub(line.start_time) as f32);
        (at_end - (time.saturating_sub(line.end_time)) as f32 / float_duration).clamp(0.0, 1.0)
    };
    let float_up = if line.is_bg { 0.1 } else { 0.05 };
    let float_y = -((float_x * std::f32::consts::PI).sin() + float_x) * float_up * font_size;

    Some(EmphasisFrame {
        scale: 1.0 + trans * 0.1 * amount,
        offset_x: -trans * 0.03 * amount * (char_count as f32 / 2.0 - ordinal as f32) * font_size,
        offset_y: -trans * 0.025 * amount * font_size,
        float_y,
        glow: trans * blur,
        glow_sigma_em: (blur * 0.3).min(0.3),
    })
}

pub(super) fn emp_easing(x: f32) -> f32 {
    let mid = 0.5;
    if x < mid {
        cubic_bezier(0.2, 0.4, 0.58, 1.0, x / mid)
    } else {
        1.0 - cubic_bezier(0.3, 0.0, 0.58, 1.0, (x - mid) / mid)
    }
}

/// 三次贝塞尔缓动（CSS `cubic-bezier(x1,y1,x2,y2)`）。
pub(super) fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
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
    for _ in 0..8 {
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
pub(super) fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x2E80..=0x2EFF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3190..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FA1F
    )
}

pub(super) fn paint_interlude_dots(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    dots: InterludeDotsFrame,
    color: Hsla,
) {
    if dots.scale <= 0.001 {
        return;
    }
    let scale = dots.scale;
    let diameter = px(dots.dot_size.max(1.0) * scale);
    // AMLL `.interludeDots` 的 `transform-origin: center`：缩放围绕点组中心，
    // 间距也随 scale 一起缩放（flex 布局先完成，再整体缩放）。
    let group_w = px(dots.width.max(0.0));
    let center_x = bounds.left() + px(dots.x) + group_w * 0.5;
    let step = px((dots.dot_size + dots.gap) * scale);
    let mut x = center_x - group_w * scale * 0.5;
    let center_y = bounds.top() + px(dots.y) + px(dots.height * 0.5);
    let y = center_y - diameter * 0.5;
    for opacity in dots.dot_opacity {
        let alpha = (opacity * color.a).clamp(0.0, 1.0);
        if alpha > 0.001 {
            let mut quad = fill(
                Bounds::new(point(x, y), gpui::size(diameter, diameter)),
                Hsla { a: alpha, ..color },
            );
            quad.corner_radii = Corners::all(diameter * 0.5);
            window.paint_quad(quad);
        }
        x += step;
    }
}
