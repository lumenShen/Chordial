//! render 测试（由 render.rs 拆分，行为不变）。

use super::*;

#[test]
fn background_viewport_uses_device_pixels() {
    let bounds = Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(682.5), px(480.25)));
    for (scale, want) in [
        (1.0, (683, 481)),
        (1.25, (854, 601)),
        (1.5, (1024, 721)),
        (2.0, (1365, 961)),
    ] {
        assert_eq!(background_viewport_size(bounds, scale), want);
    }
    // 平台尚未报告缩放时回退到逻辑像素，不能除以零或产生空视口。
    assert_eq!(background_viewport_size(bounds, 0.0), (683, 481));
    assert_eq!(background_viewport_size(bounds, f32::NAN), (683, 481));
}

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
    let xform = LineXform::plain(1.0, px(100.0), px(50.0));
    assert_eq!(xform.x(px(37.0)), px(37.0));
    assert_eq!(xform.y(px(210.0)), px(210.0));
}

#[test]
fn line_xform_scales_around_left_center_pivot() {
    // AMLL `.lyricLine { transform-origin: left }`：左边缘 + 垂直中心。
    let xform = LineXform::plain(0.97, px(10.0), px(100.0));
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
    let xform = LineXform::plain(1.0, px(0.0), px(0.0));
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
    let xform = LineXform::plain(1.0, px(0.0), px(0.0));
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
    let xform = LineXform::plain(0.97, px(10.0), px(100.0));
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

// ---- 单曲歌词替换的 600ms 生命周期 ----

#[test]
fn lyric_switch_matches_0_to_600ms_timeline() {
    let mut switch = LyricSwitch::new();
    // t=0：旧层完全可见、无附加模糊（随后逐渐模糊）；新层全透明、模糊、缩到 0.95。
    let out0 = switch.outgoing_fade();
    let in0 = switch.incoming_fade();
    assert!((out0.opacity - 1.0).abs() < 1e-6);
    assert!(out0.blur_em.abs() < 1e-6);
    assert!((out0.content_scale - 1.0).abs() < 1e-6);
    assert!(in0.opacity.abs() < 1e-6);
    assert!((in0.blur_em - LYRIC_SWITCH_BLUR_EM).abs() < 1e-6);
    assert!((in0.content_scale - LYRIC_SWITCH_SCALE).abs() < 1e-6);
    assert!((out0.opacity + in0.opacity - 1.0).abs() < 1e-6);

    // 中途两层总可见量恒为 1（交叉淡化，不会闪黑）。
    for _ in 0..18 {
        assert!(switch.advance(16.0));
        let out = switch.outgoing_fade();
        let inc = switch.incoming_fade();
        assert!((out.opacity + inc.opacity - 1.0).abs() < 1e-5);
        assert!(out.opacity <= 1.0 && inc.opacity >= 0.0);
        assert!(out.content_scale >= LYRIC_SWITCH_SCALE);
        assert!(inc.content_scale <= 1.0);
    }

    // t=600ms：旧层归零并被移除；新层恢复自身的模糊/透明度/scale。
    assert!(!switch.advance(600.0));
    let out = switch.outgoing_fade();
    let inc = switch.incoming_fade();
    assert!(out.opacity.abs() < 1e-6);
    assert!((out.content_scale - LYRIC_SWITCH_SCALE).abs() < 1e-6);
    assert!((inc.opacity - 1.0).abs() < 1e-6);
    assert!(inc.blur_em.abs() < 1e-6);
    assert!((inc.content_scale - 1.0).abs() < 1e-6);
}

fn install_one_line(inner: &mut Inner) {
    inner.player.set_lyrics(
        vec![LyricLine {
            words: vec![LyricWord::new("词", 0, 1_000)],
            start_time: 0,
            end_time: 1_000,
            ..Default::default()
        }],
        0,
    );
    let doc = inner.player.document().clone();
    inner.text.set_lyrics(&doc);
}

#[test]
fn rapid_song_switch_keeps_exactly_one_outgoing_layer() {
    let mut inner = Inner::new();
    install_one_line(&mut inner);

    // 连续三次切歌：每次都只有一层淡出，更早的层被释放。
    for _ in 0..3 {
        inner.begin_switch();
        assert!(inner.switch.is_some());
        assert!(inner.outgoing.is_some());
        install_one_line(&mut inner);
    }
    assert_eq!(inner.player.document().len(), 1);
    assert_eq!(inner.outgoing.as_ref().unwrap().player.document().len(), 1);

    // 到点后彻底清空，不留悬空缓存；下一次切换从零开始。
    inner.finish_switch();
    assert!(inner.switch.is_none());
    assert!(inner.outgoing.is_none());
}

#[test]
fn song_switch_rebinds_viewport_when_bounds_do_not_change() {
    let mut inner = Inner::new();
    inner.sync_viewport(390.0, 844.0);
    install_one_line(&mut inner);

    // 切歌会替换整份 TextLayoutCache；新缓存从 1280×720 兜底视口开始。
    inner.begin_switch();
    let long_text = "很长的一句歌词".repeat(80);
    LyricPlayerView::install_lyrics(
        &mut inner,
        vec![LyricLine {
            words: vec![LyricWord::new(long_text, 0, 1_000)],
            start_time: 0,
            end_time: 1_000,
            ..Default::default()
        }],
        0,
    );

    // 真实 prepaint 的 bounds 没变，也必须把新缓存重新绑定到窄窗尺寸。
    inner.sync_viewport(390.0, 844.0);
    let doc = inner.player.document();
    let mut narrow = TextLayoutCache::new(inner.text.style().clone());
    narrow.set_viewport(390.0, 844.0);
    narrow.set_lyrics(doc);
    let mut fallback = TextLayoutCache::new(inner.text.style().clone());
    fallback.set_lyrics(doc);

    assert!(narrow.heights()[0] > fallback.heights()[0]);
    assert!(
        (inner.text.heights()[0] - narrow.heights()[0]).abs() < 0.01,
        "switched lyrics must use the current viewport: actual={}, narrow={}, fallback={}",
        inner.text.heights()[0],
        narrow.heights()[0],
        fallback.heights()[0]
    );
}

#[test]
fn clear_lyrics_does_not_keep_outgoing_layer() {
    let mut inner = Inner::new();
    install_one_line(&mut inner);
    inner.begin_switch();
    assert!(inner.outgoing.is_some());
    inner.finish_switch();
    assert!(inner.outgoing.is_none());
}

#[test]
fn background_worker_keeps_raf_until_current_output_arrives() {
    let mut inner = Inner::new();
    inner.background_visible = true;
    inner.background.set_kind(BackgroundRendererKind::Css);
    inner
        .background
        .publish((8, 8), AudioBands::default(), true);
    assert!(inner.animation_state().background_animating);

    let deadline = Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if matches!(inner.background.poll(), Some(FrameOutput::Clear { .. })) {
            break;
        }
        assert!(Instant::now() < deadline, "worker did not publish clear");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!inner.animation_state().background_animating);
}

#[test]
fn paused_prepare_started_spring_still_wants_animation_frame() {
    let mut inner = Inner::new();
    inner.background_visible = false;
    inner.playing = false;
    install_one_line(&mut inner);

    // 模拟 render 在 prepare 前留下的静止快照；prepare 的布局阶段随后才
    // 安装首次入场弹簧。暂停不能成为丢掉下一帧的理由。
    inner.animating = false;
    inner.sync_metrics(1_000.0, 600.0);
    inner.advance_animation();
    let state = inner.animation_state();
    assert!(
        state.lyric_animating,
        "paused first-entry spring must stay active"
    );
    assert_eq!(state.interval_ms(), Some(0.0));
    assert!(state.wants_frame(0.0));
}

#[test]
fn frame_animation_state_composes_sources_and_background_throttle() {
    let idle = FrameAnimationState::default();
    assert_eq!(idle.interval_ms(), None);
    assert!(!idle.wants_frame(1_000.0));

    let lyrics = FrameAnimationState::new(true, false, 0.0);
    assert_eq!(lyrics.interval_ms(), Some(0.0));
    assert!(lyrics.wants_frame(0.0));

    let background = FrameAnimationState::new(false, true, 1000.0 / 30.0);
    assert!(!background.wants_frame(29.0));
    assert!(background.wants_frame(31.0));
    assert_eq!(lyrics.merge(background).interval_ms(), Some(0.0));

    let slower = FrameAnimationState::new(false, true, 50.0);
    assert_eq!(background.merge(slower).interval_ms(), Some(1000.0 / 30.0));
    assert_eq!(idle.merge(lyrics), lyrics);
}
