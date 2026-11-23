//! render 测试（由 render.rs 拆分，行为不变）。

use super::*;

#[test]
fn cjk_detection() {
    assert!(is_cjk_char('海'));
    assert!(!is_cjk_char('a'));
    assert!(is_cjk_char('あ'));
}

fn span(word_index: usize, x: f32, width: f32) -> WordSpan {
    WordSpan {
        word_index,
        byte_range: 0..1,
        char_offset: 0,
        x: px(x),
        width: px(width),
    }
}

fn line_with(words: Vec<LyricWord>) -> LyricLine {
    LyricLine {
        start_time: words.first().map_or(0, |w| w.start_time),
        end_time: words.last().map_or(0, |w| w.end_time),
        words,
        ..Default::default()
    }
}

#[test]
fn row_sweep_is_one_continuous_front_across_words() {
    // 两个字紧挨着（CJK），词 0 在 [100, 150]，词 1 在 [150, 200]。
    let line = line_with(vec![
        LyricWord::new("字", 1_000, 2_000),
        LyricWord::new("词", 2_000, 3_000),
    ]);
    let words = [span(0, 100.0, 50.0), span(1, 150.0, 50.0)];
    let fade = 30.0;

    // 首词开始：前沿从词左侧之外进入，整个词在暗侧。
    let (light, dark) = row_sweep_bounds(&words, &line, 1_000, fade).unwrap();
    assert_eq!((light, dark), (70.0, 100.0));

    // 首词中段：渐变带横跨词 0 的右半和词 1 的左半。
    let (light, _) = row_sweep_bounds(&words, &line, 1_500, fade).unwrap();
    assert!((light - 110.0).abs() < 1e-4, "light = {light}");

    // 词 0 结束、词 1 开始：前沿停在 [150, 180]，没有按词重置。
    let at_end = row_sweep_bounds(&words, &line, 2_000, fade).unwrap();
    let just_started = row_sweep_bounds(&words, &line, 2_000 + 1, fade).unwrap();
    assert_eq!(at_end, (150.0, 180.0));
    assert!((just_started.0 - 150.05).abs() < 0.01, "{just_started:?}");

    // 整行播完：整行处于亮侧。
    assert_eq!(
        row_sweep_bounds(&words, &line, 3_000, fade),
        Some((200.0, 230.0))
    );
}

#[test]
fn row_sweep_holds_during_word_gaps() {
    let line = line_with(vec![
        LyricWord::new("a", 0, 1_000),
        LyricWord::new("b", 2_000, 3_000),
    ]);
    let words = [span(0, 0.0, 100.0), span(1, 200.0, 100.0)];
    let fade = 40.0;

    // 词 0 结束到词 1 开始之间的停顿：停在词 0 词尾。
    assert_eq!(
        row_sweep_bounds(&words, &line, 1_500, fade),
        Some((100.0, 140.0))
    );
    assert_eq!(
        row_sweep_bounds(&words, &line, 2_000, fade),
        Some((100.0, 140.0))
    );
    // 词 1 播放时继续向前，覆盖空白 + 本词宽度。
    let (light, _) = row_sweep_bounds(&words, &line, 2_500, fade).unwrap();
    assert!((light - 200.0).abs() < 1e-4, "light = {light}");
}

#[test]
fn row_sweep_without_words_falls_back() {
    let line = line_with(vec![LyricWord::new("x", 0, 1_000)]);
    assert_eq!(row_sweep_bounds(&[], &line, 500, 30.0), None);
}

#[test]
fn word_float_advances_and_reverses_after_line_end() {
    let word = LyricWord::new("x", 1_000, 2_000);
    let line = LyricLine {
        words: vec![word.clone()],
        start_time: 0,
        end_time: 3_000,
        ..Default::default()
    };
    assert_eq!(word_float_offset(&word, &line, 500, true, 100.0), 0.0);
    let mid = word_float_offset(&word, &line, 1_500, true, 100.0);
    assert!((-5.0..0.0).contains(&mid), "mid = {mid}");
    let end = word_float_offset(&word, &line, 2_000, true, 100.0);
    assert!((end + 5.0).abs() < 0.01, "end = {end}");
    // 行结束后的倒放：应回到 0 的过程中。
    let back = word_float_offset(&word, &line, 3_500, false, 100.0);
    assert!((-5.0..0.0).contains(&back), "back = {back}");
    assert_eq!(word_float_offset(&word, &line, 5_000, false, 100.0), 0.0);
}

#[test]
fn emphasized_char_float_ends_lifted_and_reverses() {
    let word = LyricWord::new("字", 1_000, 2_000);
    let line = LyricLine {
        words: vec![word],
        start_time: 0,
        end_time: 3_000,
        ..Default::default()
    };
    let chunk = EmphasisChunk {
        word_start: 0,
        word_end: 1,
        start_time: 1_000,
        end_time: 2_000,
        emphasized: true,
        char_count: 1,
        word_char_offsets: vec![0],
        // 与 EmphasisChunk 的整形期预计算保持一致（行尾分组 boost 已计入）。
        duration_ms: 1_200.0,
        amount: 0.12,
        blur: 1.0 / 36.0,
        delay_ms: 1_000.0,
        float_duration_ms: 1_680.0,
    };
    // 动画结束后（含 sin + 线性项）停在 -0.05em = -5px。
    let settled = emphasize_glyph(&chunk, 0, &line, 10_000, true, 100.0).unwrap();
    assert!(
        (settled.float_y + 5.0).abs() < 0.01,
        "float_y = {}",
        settled.float_y
    );
    assert!((settled.scale - 1.0).abs() < 1e-4);
    // 离场后倒放：sin 峰值处会比最终位置更高，随后回到 0。
    let back = emphasize_glyph(&chunk, 0, &line, 3_500, false, 100.0).unwrap();
    assert!(back.float_y < -5.0, "back = {}", back.float_y);
    assert_eq!(
        emphasize_glyph(&chunk, 0, &line, 10_000, false, 100.0)
            .unwrap()
            .float_y,
        0.0
    );
}

#[test]
fn bezier_endpoints() {
    assert_eq!(cubic_bezier(0.2, 0.4, 0.58, 1.0, 0.0), 0.0);
    assert_eq!(cubic_bezier(0.2, 0.4, 0.58, 1.0, 1.0), 1.0);
    let mid = cubic_bezier(0.2, 0.4, 0.58, 1.0, 0.5);
    assert!(mid > 0.4 && mid < 0.9, "mid = {mid}");
}

#[test]
fn line_xform_identity_at_scale_one() {
    let xform = LineXform {
        scale: 1.0,
        pivot_x: px(100.0),
        pivot_y: px(50.0),
    };
    assert_eq!(xform.x(px(37.0)), px(37.0));
    assert_eq!(xform.y(px(210.0)), px(210.0));
}

#[test]
fn line_xform_scales_around_left_center_pivot() {
    // AMLL `.lyricLine { transform-origin: left }`：左边缘 + 垂直中心。
    let xform = LineXform {
        scale: 0.97,
        pivot_x: px(10.0),
        pivot_y: px(100.0),
    };
    // 锚点自身不动。
    assert!((f32::from(xform.x(px(10.0))) - 10.0).abs() < 1e-4);
    assert!((f32::from(xform.y(px(100.0))) - 100.0).abs() < 1e-4);
    // 左侧文本向右收缩、下方文本向上收缩。
    assert!((f32::from(xform.x(px(210.0))) - 204.0).abs() < 1e-3);
    assert!((f32::from(xform.y(px(200.0))) - 197.0).abs() < 1e-3);
}

#[test]
fn emphasize_scales_around_char_box_center() {
    // 行不缩放，只有强调缩放：字符盒中心必须保持不动（AMLL 默认
    // transform-origin: 50% 50%）。
    let xform = LineXform {
        scale: 1.0,
        pivot_x: px(0.0),
        pivot_y: px(0.0),
    };
    let glyph_x = px(100.0);
    let baseline = px(80.0);
    let center_x = glyph_x + px(10.0);
    let center_y = px(60.0);
    let scale = 1.1;
    let origin = transformed_glyph_origin(
        xform,
        glyph_x,
        baseline,
        center_x,
        center_y,
        scale,
        point(px(0.0), px(0.0)),
    );
    // 把字形原点映射回字符中心，应与未缩放时的中心重合。
    let mapped_center_x = origin.x + (center_x - glyph_x) * scale;
    let mapped_center_y = origin.y + (center_y - baseline) * scale;
    assert!((f32::from(mapped_center_x) - f32::from(center_x)).abs() < 1e-4);
    assert!((f32::from(mapped_center_y) - f32::from(center_y)).abs() < 1e-4);
}

#[test]
fn emphasis_without_scale_is_identity() {
    let xform = LineXform {
        scale: 1.0,
        pivot_x: px(0.0),
        pivot_y: px(0.0),
    };
    let origin = transformed_glyph_origin(
        xform,
        px(12.5),
        px(44.25),
        px(18.0),
        px(30.0),
        1.0,
        point(px(0.0), px(0.0)),
    );
    assert_eq!(origin, point(px(12.5), px(44.25)));
}

#[test]
fn emphasis_offset_is_scaled_with_line_scale() {
    // AMLL 的 `matrix(scale) translate(offset)` 中 offset 会被缩放，
    // 且行级缩放先生效：offset * line_scale * emp_scale。
    let xform = LineXform {
        scale: 0.97,
        pivot_x: px(10.0),
        pivot_y: px(100.0),
    };
    let no_offset = transformed_glyph_origin(
        xform,
        px(100.0),
        px(80.0),
        px(110.0),
        px(60.0),
        0.97 * 1.1,
        point(px(0.0), px(0.0)),
    );
    let with_offset = transformed_glyph_origin(
        xform,
        px(100.0),
        px(80.0),
        px(110.0),
        px(60.0),
        0.97 * 1.1,
        point(px(2.0), px(-1.0)),
    );
    let sx = (f32::from(with_offset.x) - f32::from(no_offset.x)) / (0.97 * 1.1);
    let sy = (f32::from(with_offset.y) - f32::from(no_offset.y)) / (0.97 * 1.1);
    assert!((sx - 2.0).abs() < 1e-3, "sx = {sx}");
    assert!((sy + 1.0).abs() < 1e-3, "sy = {sy}");
}
