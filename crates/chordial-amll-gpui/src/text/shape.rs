//! 文本整形辅助（由 text.rs 拆分，行为不变）。

use super::*;

// ---------------------------------------------------------------------------
// 字符分类
// ---------------------------------------------------------------------------

/// 是否为 CJK / 全角字符（这些字符之间允许逐字换行，且宽度约为 1em）。
pub(super) fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF      // Hangul Jamo
        | 0x2E80..=0x2FDF    // CJK Radicals / Kangxi
        | 0x2FF0..=0x2FFF    // Ideographic description
        | 0x3000..=0x303F    // CJK Symbols and Punctuation
        | 0x3040..=0x30FF    // Hiragana / Katakana
        | 0x3100..=0x312F    // Bopomofo
        | 0x3130..=0x318F    // Hangul Compatibility Jamo
        | 0x31A0..=0x31BF    // Bopomofo Extended
        | 0x31F0..=0x31FF    // Katakana Phonetic Extensions
        | 0x3400..=0x4DBF    // CJK Extension A
        | 0x4E00..=0x9FFF    // CJK Unified Ideographs
        | 0xA960..=0xA97F    // Hangul Jamo Extended-A
        | 0xAC00..=0xD7AF    // Hangul Syllables
        | 0xF900..=0xFAFF    // CJK Compatibility Ideographs
        | 0xFE10..=0xFE1F    // Vertical forms
        | 0xFE30..=0xFE4F    // CJK Compatibility Forms
        | 0xFF00..=0xFFEF    // Halfwidth/Fullwidth Forms
        | 0x1B000..=0x1B16F  // Kana Supplement / Extended
        | 0x20000..=0x2FA1F  // CJK Extension B+
        | 0x30000..=0x3134F  // CJK Extension G
    )
}

// ---------------------------------------------------------------------------
// 度量
// ---------------------------------------------------------------------------

#[inline]
pub(super) fn main_font_size(style: &LyricTextStyle, is_bg: bool) -> f32 {
    let scale = if is_bg {
        style.bg_line_scale.max(0.05)
    } else {
        1.0
    };
    let size = style.base_font_size.max(0.0) * scale;
    if size.is_finite() {
        size.max(MIN_FONT_SIZE)
    } else {
        MIN_FONT_SIZE
    }
}

#[inline]
pub(super) fn sub_font_size(style: &LyricTextStyle, main: f32) -> f32 {
    let size = main * style.sub_font_size_ratio.max(0.05);
    if size.is_finite() {
        size.max(MIN_FONT_SIZE)
    } else {
        MIN_FONT_SIZE
    }
}

#[inline]
pub(super) fn padding_x(style: &LyricTextStyle, main: f32) -> f32 {
    (main * style.padding_x_em.max(0.0)).max(0.0)
}

#[inline]
pub(super) fn padding_top(style: &LyricTextStyle) -> f32 {
    // AMLL `.lyricLineWrapper { padding: 0.4em }`：em 相对播放器基准字号，
    // 与视口高度无关。按视口取比例会在竖屏（高 >> 宽、字号被宽度卡住）
    // 时把行距撑到字号的数倍。
    (main_font_size(style, false) * style.padding_y_em.max(0.0)).max(0.0)
}

#[inline]
pub(super) fn content_width(viewport_width: f32, padding_x: f32, has_duet: bool) -> f32 {
    let reserve = if has_duet { viewport_width * 0.15 } else { 0.0 };
    (viewport_width - 2.0 * padding_x - reserve).max(1.0)
}

pub(super) fn estimate_rows(chars: u32, wide: u32, font_size: f32, width: f32) -> usize {
    if chars == 0 {
        return 0;
    }
    let narrow = chars.saturating_sub(wide) as f32;
    // 拉丁字符平均约 0.55em，CJK 约 1em。
    let estimated = (narrow * 0.55 + wide as f32) * font_size;
    let rows = (estimated / width.max(1.0)).ceil();
    if rows.is_finite() {
        (rows as usize).max(1)
    } else {
        1
    }
}

/// 未整形行的估算高度：必须 > 0，播放器前缀和依赖它。
pub(super) fn estimate_height(
    style: &LyricTextStyle,
    viewport_width: f32,
    has_duet: bool,
    hint: &LineHint,
) -> f32 {
    let main = main_font_size(style, hint.is_bg);
    let sub = sub_font_size(style, main);
    let pad_x = padding_x(style, main);
    let width = content_width(viewport_width, pad_x, has_duet);
    let pad_y = padding_top(style);

    let main_rows = estimate_rows(hint.main_chars, hint.main_wide, main, width).max(1);
    let sub_rows = estimate_rows(hint.trans_chars, 0, sub, width)
        + estimate_rows(hint.roman_chars, 0, sub, width);

    let height = 2.0 * pad_y
        + main_rows as f32 * main * style.line_height_ratio.max(0.0)
        + sub_rows as f32 * sub * style.sub_line_height_ratio.max(0.0);
    if height.is_finite() {
        height.max(1.0)
    } else {
        1.0
    }
}

#[inline]
pub(super) fn font_for(style: &LyricTextStyle) -> Font {
    let mut font = gpui::font(style.font_family.clone());
    font.weight = style.font_weight;
    font
}

// ---------------------------------------------------------------------------
// 单行排版
// ---------------------------------------------------------------------------

pub(super) fn build_line_shape(
    style: &LyricTextStyle,
    viewport_width: f32,
    has_duet: bool,
    index: usize,
    line: &LyricLine,
    window: &Window,
) -> LineShape {
    let main_size = main_font_size(style, line.is_bg);
    let sub_size = sub_font_size(style, main_size);
    let pad_x = padding_x(style, main_size);
    let pad_y = padding_top(style);
    let width = content_width(viewport_width, pad_x, has_duet);
    let font = font_for(style);

    let main = shape_main_rows(
        window,
        line,
        &font,
        main_size,
        width,
        style.line_height_ratio,
    );
    let sub_height = sub_size * style.sub_line_height_ratio.max(0.0);
    let translation = shape_sub_rows(
        window,
        &line.translated_lyric,
        &font,
        sub_size,
        width,
        sub_height,
        true,
    );
    let roman = shape_sub_rows(
        window,
        &line.roman_lyric,
        &font,
        sub_size,
        width,
        sub_height,
        false,
    );

    let mut height = 2.0 * pad_y
        + main.len() as f32 * main_size * style.line_height_ratio.max(0.0)
        + (translation.len() + roman.len()) as f32 * sub_height;
    if !height.is_finite() || height <= 0.0 {
        height = main_size.max(1.0);
    }

    let (emphasis_chunks, word_chunks) = build_emphasis_chunks(line);

    LineShape {
        index,
        main,
        translation,
        roman,
        height: px(height),
        main_font_size: px(main_size),
        sub_font_size: px(sub_size),
        padding_top: px(pad_y),
        padding_x: px(pad_x),
        content_width: px(width),
        emphasis_chunks,
        word_chunks,
    }
}

/// 构建强调分组：与 AMLL `chunkAndSplitLyricWords` 的**可见行为**等价。
///
/// - 英文等非 CJK 文本：没有空白的相邻词合并为一个分组，用合并时长判断是否
///   达到 1s、2..=7 个字符；同时保留 AMLL 的「其中任一词自身达标」规则。
/// - CJK 文本：AMLL 会先拆成单字 atom，因此一个多字词的强调阈值是**单字时长**
///   ≥ 1s，而不是整词时长。这里按每字均分时长近似。
///
/// 纯空白词不进入分组（AMLL 中它们是独立的文本节点）。
pub(super) fn build_emphasis_chunks(line: &LyricLine) -> (Vec<EmphasisChunk>, Vec<Option<usize>>) {
    let n = line.words.len();
    let mut chunks = Vec::new();
    let mut word_chunks = vec![None; n];
    let mut i = 0usize;

    while i < n {
        let text = line.words[i].word.trim();
        if text.is_empty() {
            i += 1;
            continue;
        }

        let cjk = is_cjk_text(text);
        let mut end = i + 1;
        if !cjk {
            // 只合并“视觉上仍是同一个词”的相邻片段：两侧都没有空白。
            while end < n {
                let prev = &line.words[end - 1].word;
                let next = &line.words[end].word;
                let next_trim = next.trim();
                if next_trim.is_empty()
                    || is_cjk_text(next_trim)
                    || prev.ends_with(char::is_whitespace)
                    || next.starts_with(char::is_whitespace)
                {
                    break;
                }
                end += 1;
            }
        }

        let mut start_time = u64::MAX;
        let mut end_time = 0u64;
        let mut char_count = 0usize;
        let mut word_char_offsets = Vec::with_capacity(end - i);
        let mut merged = String::new();
        for word in &line.words[i..end] {
            start_time = start_time.min(word.start_time);
            end_time = end_time.max(word.end_time);
            word_char_offsets.push(char_count);
            let chars = word.word.trim().chars().count();
            char_count += chars;
            merged.push_str(word.word.trim());
        }
        let duration = end_time.saturating_sub(start_time);

        // 强调动画常量：只依赖分组时间戳与行信息，整形时算一次。
        let mut du = duration.max(1000) as f32;
        let mut amount = du / 2000.0;
        amount = if amount > 1.0 {
            amount.sqrt()
        } else {
            amount * amount * amount
        };
        let mut blur = du / 3000.0;
        blur = if blur > 1.0 {
            blur.sqrt()
        } else {
            blur * blur * blur
        };
        amount *= 0.6;
        blur *= 0.5;
        if end == line.words.len() {
            amount *= 1.6;
            blur *= 1.5;
            du *= 1.2;
        }
        let amount = amount.min(1.2);
        let blur = blur.min(0.8);
        let delay_ms = start_time.saturating_sub(line.start_time) as f32;
        let float_duration_ms = (du * 1.4).max(1.0);

        let mut emphasized = line.words[i..end].iter().any(should_emphasize_group_word);
        if is_cjk_text(&merged) {
            if char_count > 0 {
                // 单字均分时长 ≥ 1s（AMLL 拆字后的 atom 时长）。
                emphasized |= duration / char_count as u64 >= 1000;
            }
        } else {
            emphasized |= duration >= 1000 && (2..=7).contains(&char_count);
        }

        let chunk_index = chunks.len();
        for slot in &mut word_chunks[i..end] {
            *slot = Some(chunk_index);
        }
        chunks.push(EmphasisChunk {
            word_start: i,
            word_end: end,
            start_time,
            end_time,
            emphasized,
            char_count,
            word_char_offsets,
            duration_ms: du,
            amount,
            blur,
            delay_ms,
            float_duration_ms,
        });
        i = end;
    }

    (chunks, word_chunks)
}

pub(super) fn is_cjk_text(text: &str) -> bool {
    let mut any = false;
    for ch in text.chars().filter(|c| !c.is_whitespace()) {
        any = true;
        if !is_cjk(ch) {
            return false;
        }
    }
    any
}

/// AMLL `LyricLineBase.shouldEmphasize` 的单 atom 版本（CJK 多字词见上方说明）。
pub(super) fn should_emphasize_group_word(word: &crate::model::LyricWord) -> bool {
    let text = word.word.trim();
    let chars = text.chars().count();
    if chars == 0 {
        return false;
    }
    let duration = word.end_time.saturating_sub(word.start_time);
    if is_cjk_text(text) {
        chars == 1 && duration >= 1000
    } else {
        duration >= 1000 && (2..=7).contains(&chars)
    }
}

/// 整形单个视觉行。空文本绝不进入 gpui（见模块文档）。
pub(super) fn shape_one(
    window: &Window,
    text: SharedString,
    font: &Font,
    font_size: f32,
) -> ShapedLine {
    let scope = PerfScope::enter(SHAPE_CHECKPOINT);
    let len = text.len();
    let run = TextRun {
        len,
        font: font.clone(),
        color: white(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped =
        window
            .text_system()
            .shape_line(text, px(font_size), std::slice::from_ref(&run), None);
    scope.finish(len as u64);
    shaped
}

pub(super) fn empty_row(height: f32) -> ShapedRow {
    ShapedRow {
        line: ShapedLine::default(),
        words: Vec::new(),
        width: px(0.0),
        height: px(height.max(1.0)),
        is_translation: false,
    }
}

pub(super) type WordRanges = Vec<Range<usize>>;

/// 主歌词拼接 + 词在整行文本中的字节范围。
///
/// 空白（含 `\n`/`\t` 等控制字符）统一压成单个空格，避免 `shape_line` 的换行断言。
pub(super) fn build_main_text(line: &LyricLine) -> (String, WordRanges) {
    let capacity = line
        .words
        .iter()
        .map(|word| word.word.len())
        .sum::<usize>()
        .max(8);
    let mut text = String::with_capacity(capacity);
    let mut ranges = Vec::with_capacity(line.words.len());
    let mut last_space = false;

    for word in &line.words {
        let start = text.len();
        for ch in word.word.chars() {
            if ch.is_whitespace() || ch.is_control() {
                if !last_space {
                    text.push(' ');
                    last_space = true;
                }
            } else {
                text.push(ch);
                last_space = false;
            }
        }
        ranges.push(start..text.len());
    }

    (text, ranges)
}

/// 译文/音译的空白归一化（保留首尾空格，换行时再裁）。
pub(super) fn normalize_plain(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut last_space = false;
    for ch in src.chars() {
        if ch.is_whitespace() || ch.is_control() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

pub(super) fn row_byte_range(row: &Range<usize>, fragments: &[MeasuredFragment]) -> (usize, usize) {
    if row.start >= row.end || row.end > fragments.len() {
        (0, 0)
    } else {
        (
            fragments[row.start].byte_start,
            fragments[row.end - 1].byte_end,
        )
    }
}

pub(super) fn word_index_at(
    word_ranges: Option<&[Range<usize>]>,
    cursor: &mut usize,
    byte: usize,
) -> usize {
    let Some(ranges) = word_ranges else {
        return 0;
    };
    if ranges.is_empty() {
        return 0;
    }
    let mut c = (*cursor).min(ranges.len() - 1);
    while c + 1 < ranges.len() && ranges[c].end <= byte {
        c += 1;
    }
    *cursor = c;
    c
}

/// 构建可换行片段：空白单独成段；CJK 词与超长拉丁词按字符切分。
pub(super) fn build_fragments(
    text: &str,
    xs: &CharXs,
    word_ranges: Option<&[Range<usize>]>,
    max_width: f32,
) -> Vec<MeasuredFragment> {
    let mut fragments = Vec::with_capacity(text.len());
    let mut word_cursor = 0usize;
    let mut i = 0usize;
    // 行首视为「前面有空白」，允许第一个词成为断点。
    let mut prev_space = true;
    let mut prev_wide = false;

    while i < text.len() {
        let ch = text[i..]
            .chars()
            .next()
            .expect("byte index is a char boundary");
        if ch.is_whitespace() || ch.is_control() {
            let start = i;
            let mut end = i;
            while end < text.len() {
                let c = text[end..].chars().next().unwrap();
                if c.is_whitespace() || c.is_control() {
                    end += c.len_utf8();
                } else {
                    break;
                }
            }
            let x0 = xs.x_at(start);
            let x1 = xs.x_at(end);
            fragments.push(MeasuredFragment {
                word_index: word_index_at(word_ranges, &mut word_cursor, start),
                byte_start: start,
                byte_end: end,
                x: px(x0),
                width: px((x1 - x0).max(0.0)),
                breakable_before: false,
                is_space: true,
            });
            prev_space = true;
            prev_wide = false;
            i = end;
            continue;
        }

        // 非空白 token：先测量整词，决定是否按字符细分。
        let token_start = i;
        let mut token_end = i;
        let mut has_wide = false;
        let mut last_ch = ch;
        while token_end < text.len() {
            let c = text[token_end..].chars().next().unwrap();
            if c.is_whitespace() || c.is_control() {
                break;
            }
            has_wide |= is_cjk(c);
            last_ch = c;
            token_end += c.len_utf8();
        }
        let token_x = xs.x_at(token_start);
        let token_x_end = xs.x_at(token_end);
        let token_width = (token_x_end - token_x).max(0.0);
        let word_index = word_index_at(word_ranges, &mut word_cursor, token_start);
        let break_at_start = prev_space || prev_wide;

        if has_wide || token_width > max_width {
            let mut pos = token_start;
            let mut first = true;
            while pos < token_end {
                let c = text[pos..].chars().next().unwrap();
                let next = pos + c.len_utf8();
                let x0 = xs.x_at(pos);
                let x1 = xs.x_at(next);
                // 词首：前面是空白或 CJK 时可断；词内：仅 CJK 字符之间可断。
                let breakable = if first {
                    break_at_start
                } else {
                    has_wide && is_cjk(c)
                };
                fragments.push(MeasuredFragment {
                    word_index,
                    byte_start: pos,
                    byte_end: next,
                    x: px(x0),
                    width: px((x1 - x0).max(0.0)),
                    breakable_before: breakable,
                    is_space: false,
                });
                first = false;
                pos = next;
            }
        } else {
            fragments.push(MeasuredFragment {
                word_index,
                byte_start: token_start,
                byte_end: token_end,
                x: px(token_x),
                width: px(token_width),
                breakable_before: break_at_start,
                is_space: false,
            });
        }

        prev_space = false;
        prev_wide = is_cjk(last_ch);
        i = token_end;
    }

    fragments
}

/// 从 row 文本的范围计算该视觉行内每个词的几何。
///
/// 词可能跨视觉行：此时每一行都会得到一个 `WordSpan`（`byte_range` 是裁剪后的
/// 部分），渲染层按 `word_index` 各自计算 mask 进度即可。
pub(super) fn word_spans(
    text: &str,
    word_ranges: &[Range<usize>],
    row_start: usize,
    row_end: usize,
    xs: &CharXs,
) -> Vec<WordSpan> {
    let mut spans = Vec::with_capacity(8);
    for (word_index, range) in word_ranges.iter().enumerate() {
        let start = range.start.max(row_start);
        let end = range.end.min(row_end);
        if start >= end {
            continue;
        }
        let local_start = start - row_start;
        let local_end = end - row_start;
        let x0 = xs.x_at(local_start);
        let x1 = xs.x_at(local_end);
        // 词内跨视觉行时，强调动画的 stagger 序号要延续整词计数。
        let char_offset = text
            .get(range.start..start)
            .map(|s| s.chars().filter(|c| !c.is_whitespace()).count())
            .unwrap_or(0);
        spans.push(WordSpan {
            word_index,
            byte_range: local_start..local_end,
            char_offset,
            x: px(x0),
            width: px((x1 - x0).max(0.0)),
        });
    }
    spans
}

pub(super) fn shape_main_rows(
    window: &Window,
    line: &LyricLine,
    font: &Font,
    font_size: f32,
    max_width: f32,
    line_height_ratio: f32,
) -> Vec<ShapedRow> {
    let row_height = (font_size * line_height_ratio.max(0.0)).max(1.0);
    let (raw_text, word_ranges) = build_main_text(line);
    if raw_text.trim().is_empty() {
        return vec![empty_row(row_height)];
    }

    let text = SharedString::new(raw_text);
    let text_len = text.len();
    let full = shape_one(window, text.clone(), font, font_size);
    let full_xs = CharXs::build(&text, &full);
    let fragments = build_fragments(&text, &full_xs, Some(&word_ranges), max_width);
    let rows = wrap_fragments(&fragments, px(max_width));

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (start, end) = row_byte_range(&row, &fragments);
        if start >= end {
            out.push(empty_row(row_height));
            continue;
        }
        if start == 0 && end == text_len {
            out.push(ShapedRow {
                words: word_spans(&text, &word_ranges, 0, text_len, &full_xs),
                width: full.width,
                height: px(row_height),
                is_translation: false,
                line: full.clone(),
            });
        } else {
            let row_text = SharedString::new(&text[start..end]);
            let shaped = shape_one(window, row_text.clone(), font, font_size);
            let xs = CharXs::build(&row_text, &shaped);
            out.push(ShapedRow {
                words: word_spans(&text, &word_ranges, start, end, &xs),
                width: shaped.width,
                height: px(row_height),
                is_translation: false,
                line: shaped,
            });
        }
    }

    if out.is_empty() {
        out.push(empty_row(row_height));
    }
    out
}

pub(super) fn shape_sub_rows(
    window: &Window,
    source: &str,
    font: &Font,
    font_size: f32,
    max_width: f32,
    row_height: f32,
    is_translation: bool,
) -> Vec<ShapedRow> {
    let normalized = normalize_plain(source);
    if normalized.trim().is_empty() {
        return Vec::new();
    }
    let row_height = row_height.max(1.0);
    let text = SharedString::new(normalized);
    let text_len = text.len();
    let full = shape_one(window, text.clone(), font, font_size);

    if f32::from(full.width) <= max_width + WRAP_EPSILON {
        return vec![ShapedRow {
            line: full.clone(),
            words: Vec::new(),
            width: full.width,
            height: px(row_height),
            is_translation,
        }];
    }

    let full_xs = CharXs::build(&text, &full);
    let fragments = build_fragments(&text, &full_xs, None, max_width);
    let rows = wrap_fragments(&fragments, px(max_width));

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (start, end) = row_byte_range(&row, &fragments);
        if start >= end {
            out.push(empty_row(row_height));
            continue;
        }
        if start == 0 && end == text_len {
            out.push(ShapedRow {
                line: full.clone(),
                words: Vec::new(),
                width: full.width,
                height: px(row_height),
                is_translation,
            });
        } else {
            let row_text = SharedString::new(&text[start..end]);
            let shaped = shape_one(window, row_text, font, font_size);
            out.push(ShapedRow {
                width: shaped.width,
                line: shaped,
                words: Vec::new(),
                height: px(row_height),
                is_translation,
            });
        }
    }

    if out.is_empty() {
        out.push(empty_row(row_height));
    }
    out
}
