//! 歌词文档后处理：与 AMLL `LyricPlayerBase.setLyricLines` 等价的时间/结构整理。
//!
//! 步骤（顺序必须一致，否则滚动与高亮时刻会和 AMLL 不同）：
//! 1. 单词内连续空白压缩为单个空格；
//! 2. 统计逐词歌词 / 对唱行；
//! 3. 每行开始时间最多提前 1 秒，但不早于上一行结束；
//! 4. 背景行与其上一主行同始同终；
//! 5. 兜底修正零长度区间（AMLL 不做，但没有它会让异常文件永久不高亮）。
//!
//! 全流程两遍反向扫描 + 一遍正向扫描，O(n)。

use chordal_core::perf::PerfScope;

use crate::model::{LyricDocument, LyricLine};

/// 处理一组原始歌词行。
pub fn process(mut lines: Vec<LyricLine>) -> LyricDocument {
    let _scope = PerfScope::enter("amll.document");
    let _scope_items = lines.len() as u64;

    if lines.len() > 1 {
        lines.sort_by_key(|l| l.start_time);
    }

    // 1. 单词空白归一化 + 2. 特征统计。O(总字符数)。
    let mut is_dynamic = false;
    let mut has_duet = false;
    for line in &mut lines {
        if line.words.len() > 1 {
            is_dynamic = true;
        }
        has_duet |= line.is_duet;
        for word in &mut line.words {
            if word.word.as_bytes().iter().any(u8::is_ascii_whitespace)
                || word.word.chars().any(char::is_whitespace)
            {
                word.word = collapse_whitespace(&word.word);
            }
        }
    }

    // 3. 开始时间提前（倒序，保证读取到的是尚未被修改的上一行结束时间）。
    for i in (0..lines.len()).rev() {
        if lines[i].is_bg {
            continue;
        }
        let start = lines[i].start_time;
        let new_start = if i > 0 {
            let prev_end = lines[i - 1].end_time;
            start.saturating_sub(1000).max(prev_end.min(start))
        } else {
            start.saturating_sub(1000)
        };
        lines[i].start_time = new_start;
    }

    // 4. 背景行并入上一主行的时间区间（倒序，处理连续多个 BG 的情况）。
    for i in (0..lines.len()).rev() {
        if lines[i].is_bg || i + 1 >= lines.len() || !lines[i + 1].is_bg {
            continue;
        }
        let (line_end, line_start) = (lines[i].end_time, lines[i].start_time);
        let bg = &mut lines[i + 1];

        let mut bg_start = line_start;
        let mut bg_end = line_end;
        for word in bg.words.iter().filter(|w| !w.word.trim().is_empty()) {
            bg_start = bg_start.min(word.start_time);
            bg_end = bg_end.max(word.end_time);
        }
        bg.start_time = bg_start.min(line_start);
        bg.end_time = bg_end.max(line_end);
    }

    // 5. 异常输入（end < start）可能让“提前 1 秒”把开始时间推到上一行之前；
    //    最后稳定排序一次，保证 player 的二分/前缀最大结束时间前提成立。
    lines.sort_by_key(|l| l.start_time);

    // 6. 零长度兜底（异常文件常见：重复时间戳）。
    for line in &mut lines {
        if line.end_time <= line.start_time {
            line.end_time = line.start_time.saturating_add(1);
        }
        for word in &mut line.words {
            if word.end_time <= word.start_time {
                word.end_time = word.start_time.saturating_add(1);
            }
        }
    }

    _scope.finish(_scope_items);
    LyricDocument {
        lines,
        is_dynamic,
        has_duet,
    }
}

/// 连续空白 → 单个空格（不裁剪首尾，保持 AMLL 的词间空格语义）。
fn collapse_whitespace(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_ws = false;
    for ch in src.chars() {
        if ch.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(ch);
            in_ws = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LyricWord;

    fn line(start: u64, end: u64, words: &[(&str, u64, u64)]) -> LyricLine {
        LyricLine {
            words: words
                .iter()
                .map(|(w, s, e)| LyricWord::new(*w, *s, *e))
                .collect(),
            start_time: start,
            end_time: end,
            ..Default::default()
        }
    }

    #[test]
    fn advances_start_but_not_before_previous_end() {
        let doc = process(vec![
            line(0, 10_000, &[("a", 0, 10_000)]),
            line(10_500, 12_000, &[("b", 10_500, 12_000)]),
        ]);
        // 第二行本应提前到 9500，但不早于上一行结束 10000。
        assert_eq!(doc.lines[1].start_time, 10_000);

        let doc = process(vec![
            line(0, 1_000, &[("a", 0, 1_000)]),
            line(8_000, 9_000, &[("b", 8_000, 9_000)]),
        ]);
        assert_eq!(doc.lines[1].start_time, 7_000);
    }

    #[test]
    fn bg_line_inherits_main_line_span() {
        let mut bg = line(20_000, 24_000, &[("bg", 20_000, 24_000)]);
        bg.is_bg = true;
        let doc = process(vec![line(18_000, 22_000, &[("main", 18_000, 22_000)]), bg]);
        // 主行开始时间先提前到 17000，BG 行随主行一起提前（AMLL 步骤顺序）。
        assert_eq!(doc.lines[1].start_time, 17_000);
        assert_eq!(doc.lines[1].end_time, 24_000);
    }

    #[test]
    fn detects_dynamic_and_duet() {
        let mut duet = line(0, 1_000, &[("a", 0, 400), ("b", 400, 1_000)]);
        duet.is_duet = true;
        let doc = process(vec![duet]);
        assert!(doc.is_dynamic);
        assert!(doc.has_duet);
    }

    #[test]
    fn collapses_word_whitespace() {
        let doc = process(vec![line(0, 1_000, &[("a   b\n c", 0, 1_000)])]);
        assert_eq!(doc.lines[0].words[0].word, "a b c");
    }

    #[test]
    fn zero_duration_is_repaired() {
        let doc = process(vec![line(5_000, 5_000, &[("a", 5_000, 5_000)])]);
        assert!(doc.lines[0].end_time > doc.lines[0].start_time);
        assert!(doc.lines[0].words[0].end_time > doc.lines[0].words[0].start_time);
    }
}
