//! text 测试（由 text.rs 拆分，行为不变）。

use super::*;
use crate::model::LyricWord;

fn frag(word: usize, width: f32, breakable: bool) -> MeasuredFragment {
    MeasuredFragment {
        word_index: word,
        byte_start: 0,
        byte_end: 0,
        x: px(0.0),
        width: px(width),
        breakable_before: breakable,
        is_space: false,
    }
}

fn space(width: f32) -> MeasuredFragment {
    MeasuredFragment {
        word_index: 0,
        byte_start: 0,
        byte_end: 0,
        x: px(0.0),
        width: px(width),
        breakable_before: false,
        is_space: true,
    }
}

fn rows(fragments: &[MeasuredFragment], width: f32) -> Vec<(usize, usize)> {
    wrap_fragments(fragments, px(width))
        .into_iter()
        .map(|row| (row.start, row.end))
        .collect()
}

#[test]
fn empty_input_has_one_empty_row() {
    assert_eq!(rows(&[], 100.0), vec![(0, 0)]);
}

#[test]
fn single_fragment_no_wrap() {
    let fragments = [frag(0, 40.0, true)];
    assert_eq!(rows(&fragments, 100.0), vec![(0, 1)]);
}

#[test]
fn multiple_words_wrap_at_space() {
    let fragments = [frag(0, 30.0, true), space(10.0), frag(1, 30.0, true)];
    assert_eq!(rows(&fragments, 100.0), vec![(0, 3)]);
    // 50px 放不下两个词：在第二个词前断行，行尾空格被裁掉。
    assert_eq!(rows(&fragments, 50.0), vec![(0, 1), (2, 3)]);
}

#[test]
fn overlong_latin_word_hard_breaks() {
    let mut fragments = Vec::new();
    fragments.push(frag(0, 10.0, true));
    for _ in 0..7 {
        fragments.push(frag(0, 10.0, false));
    }
    assert_eq!(rows(&fragments, 50.0), vec![(0, 5), (5, 8)]);
}

#[test]
fn cjk_breaks_between_chars() {
    let fragments: Vec<_> = (0..5).map(|_| frag(0, 20.0, true)).collect();
    assert_eq!(rows(&fragments, 45.0), vec![(0, 2), (2, 4), (4, 5)]);
}

#[test]
fn whitespace_only_input_is_empty_row() {
    let fragments = [space(10.0), space(10.0)];
    assert_eq!(rows(&fragments, 100.0), vec![(0, 0)]);
}

#[test]
fn leading_and_trailing_spaces_are_trimmed() {
    let fragments = [
        space(10.0),
        frag(0, 20.0, true),
        space(10.0),
        frag(1, 20.0, true),
        space(10.0),
    ];
    assert_eq!(rows(&fragments, 100.0), vec![(1, 4)]);
}

#[test]
fn wrap_spans_convenience_matches_word_level() {
    let spans = [(0usize, 0..3usize, px(30.0)), (1, 3..6, px(30.0))];
    assert_eq!(wrap_spans(&spans, px(50.0)), vec![vec![0], vec![1]]);
    assert_eq!(wrap_spans(&spans, px(100.0)), vec![vec![0, 1]]);
}

#[test]
fn main_text_collapses_whitespace_and_keeps_word_ranges() {
    let line = LyricLine::new(vec![
        LyricWord::new("Hello", 0, 100),
        LyricWord::new("  big\nworld", 100, 200),
    ]);
    let (text, ranges) = build_main_text(&line);
    assert_eq!(text, "Hello big world");
    assert_eq!(&text[ranges[0].clone()], "Hello");
    assert_eq!(&text[ranges[1].clone()], " big world");
}

fn doc_with(n: usize) -> LyricDocument {
    let lines = (0..n)
        .map(|i| {
            let mut line = LyricLine::new(vec![LyricWord::new(
                format!("line {i} text"),
                i as u64 * 1000,
                i as u64 * 1000 + 900,
            )]);
            line.translated_lyric = format!("translation {i}");
            line
        })
        .collect();
    LyricDocument {
        lines,
        is_dynamic: true,
        has_duet: false,
    }
}

fn cache_with(n: usize) -> TextLayoutCache {
    let mut cache = TextLayoutCache::new(LyricTextStyle::default());
    cache.set_viewport(1280.0, 720.0);
    cache.set_lyrics(&doc_with(n));
    cache
}

#[test]
fn heights_are_estimated_and_non_zero() {
    let cache = cache_with(7);
    assert_eq!(cache.heights().len(), 7);
    assert!(cache.heights().iter().all(|height| *height > 0.0));
    assert_eq!(cache.shaped_count(), 0);
}

#[test]
fn metrics_follow_amll_ratios() {
    let style = LyricTextStyle::default();
    assert_eq!(main_font_size(&style, false), 24.0);
    assert!((main_font_size(&style, true) - 16.8).abs() < 0.001);
    assert_eq!(sub_font_size(&style, 24.0), 12.0);
    // 背景行副字号 16.8 * 0.5 = 8.4，被抬到 10px 下限。
    assert_eq!(sub_font_size(&style, main_font_size(&style, true)), 10.0);
    assert_eq!(padding_x(&style, 24.0), 24.0);
    assert_eq!(content_width(1280.0, 24.0, false), 1232.0);
    // 内容宽度永远 >= 1。
    assert_eq!(content_width(10.0, 100.0, false), 1.0);

    let hint = LineHint {
        main_chars: 10,
        main_wide: 0,
        trans_chars: 5,
        roman_chars: 0,
        is_bg: false,
    };
    let height = estimate_height(&style, 1280.0, false, &hint);
    // 2 * padding（0.4em × 基准字号）+ 1 行主歌词 + 1 行副歌词。
    let expected = 2.0 * 24.0 * 0.4 + 24.0 * 1.2 + 12.0 * 1.5;
    assert!((height - expected).abs() < 0.01, "{height} vs {expected}");
}

#[test]
fn empty_lyrics_have_no_heights() {
    let mut cache = TextLayoutCache::new(LyricTextStyle::default());
    cache.set_lyrics(&LyricDocument::default());
    assert!(cache.heights().is_empty());
    assert_eq!(cache.shaped_count(), 0);
}

#[test]
fn viewport_change_invalidates_shapes_but_keeps_heights() {
    let mut cache = cache_with(32);
    for i in 0..8 {
        cache.insert_fake(i);
    }
    assert_eq!(cache.shaped_count(), 8);
    let generation = cache.generation();

    cache.set_viewport(640.0, 480.0);
    assert_eq!(cache.shaped_count(), 0);
    assert!(cache.get(0).is_none());
    assert_ne!(cache.generation(), generation);
    assert_eq!(cache.heights().len(), 32);
    assert!(cache.heights().iter().all(|height| *height > 0.0));

    // 相同尺寸不应再次清空。
    cache.insert_fake(0);
    let generation = cache.generation();
    cache.set_viewport(640.0, 480.0);
    assert_eq!(cache.generation(), generation);
    assert!(cache.get(0).is_some());
}

#[test]
fn style_change_recomputes_heights() {
    let mut cache = cache_with(4);
    let before = cache.heights().to_vec();
    let mut style = cache.style().clone();
    style.base_font_size = 48.0;
    cache.set_style(style, 1280.0, 720.0);
    assert_eq!(cache.heights().len(), 4);
    assert!(
        cache
            .heights()
            .iter()
            .zip(before.iter())
            .all(|(new, old)| *new > *old),
        "larger font must estimate a taller line"
    );
}

#[test]
fn lru_eviction_keeps_heights_and_cap() {
    let mut cache = cache_with(64);
    let heights_before = cache.heights().to_vec();
    cache.set_max_shaped_lines(16);
    for i in 0..40 {
        cache.insert_fake(i);
    }
    assert_eq!(cache.max_shaped_lines(), 16);
    assert_eq!(cache.shaped_count(), 16);
    assert!(cache.get(0).is_none());
    assert!(cache.get(23).is_none());
    assert!(cache.get(24).is_some());
    assert!(cache.get(39).is_some());
    assert_eq!(cache.heights().len(), 64);
    assert!(cache.heights().iter().all(|height| *height > 0.0));
    // 淘汰只丢弃整形结果，不改变播放器使用的高度。
    assert_eq!(cache.heights(), &heights_before[..]);

    // 再次使用旧行时重新整形并淘汰队首。
    cache.insert_fake(5);
    assert!(cache.get(5).is_some());
    assert_eq!(cache.shaped_count(), 16);
}

#[test]
fn max_shaped_lines_has_a_floor() {
    let mut cache = cache_with(2);
    cache.set_max_shaped_lines(1);
    assert_eq!(cache.max_shaped_lines(), MIN_MAX_SHAPED_LINES);
}

#[test]
fn emphasis_merges_adjacent_latin_fragments() {
    // YRC 常见：一个英文单词被拆成多段、之间没有空格。
    let line = LyricLine {
        words: vec![
            LyricWord::new(" su", 0, 500),
            LyricWord::new("gar", 500, 1_200),
            LyricWord::new(" so", 1_200, 1_700),
            LyricWord::new(" sweet", 1_700, 2_400),
        ],
        start_time: 0,
        end_time: 2_400,
        ..Default::default()
    };
    let (chunks, word_chunks) = build_emphasis_chunks(&line);
    // " sugar" 合并（1.2s, 5 字）；" so"+"sweet" 因首词以空格开头而不合并。
    assert_eq!(chunks.len(), 3);
    assert!(chunks[0].emphasized);
    assert_eq!(chunks[0].word_start, 0);
    assert_eq!(chunks[0].word_end, 2);
    assert_eq!(chunks[0].char_count, 5);
    assert_eq!(chunks[0].word_char_offsets, vec![0, 2]);
    assert!(!chunks[1].emphasized);
    assert!(!chunks[2].emphasized);
    assert_eq!(word_chunks[1], Some(0));
}

#[test]
fn emphasis_does_not_merge_cjk_runs() {
    // CJK 逐字歌词不能把整句合并：每个字的时长都远小于 1s。
    let line = LyricLine {
        words: vec![
            LyricWord::new("我", 0, 200),
            LyricWord::new("们", 200, 400),
            LyricWord::new("走", 400, 600),
        ],
        start_time: 0,
        end_time: 600,
        ..Default::default()
    };
    let (chunks, _) = build_emphasis_chunks(&line);
    assert_eq!(chunks.len(), 3);
    assert!(chunks.iter().all(|chunk| !chunk.emphasized));
}

#[test]
fn emphasis_uses_per_char_duration_for_multi_cjk_word() {
    // AMLL 会把多字 CJK 词拆成单字 atom：3 个字 / 2.4s = 每字 0.8s，不强调。
    let line = LyricLine {
        words: vec![LyricWord::new(" landslide ", 0, 2_400)],
        start_time: 0,
        end_time: 2_400,
        ..Default::default()
    };
    let (chunks, _) = build_emphasis_chunks(&line);
    assert_eq!(chunks.len(), 1);
    assert!(!chunks[0].emphasized);

    let line = LyricLine {
        words: vec![LyricWord::new("大海", 0, 2_400)],
        start_time: 0,
        end_time: 2_400,
        ..Default::default()
    };
    let (chunks, _) = build_emphasis_chunks(&line);
    assert!(chunks[0].emphasized, "2 chars / 2.4s = 1.2s per char");
}

impl TextLayoutCache {
    /// 测试用：模拟「整形完成」，槽位与 LRU 行为与 `ensure` 一致（高度不动）。
    fn insert_fake(&mut self, index: usize) {
        let shape = Arc::new(LineShape {
            index,
            main: Vec::new(),
            translation: Vec::new(),
            roman: Vec::new(),
            height: px(1.0),
            main_font_size: px(10.0),
            sub_font_size: px(10.0),
            padding_top: px(0.0),
            padding_x: px(0.0),
            content_width: px(100.0),
            emphasis_chunks: Vec::new(),
            word_chunks: Vec::new(),
        });
        self.store(index, shape);
    }
}
