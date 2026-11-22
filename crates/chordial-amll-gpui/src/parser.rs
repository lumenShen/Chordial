//! 歌词文本解析：LRC 与 ESLRC（Foobar2000 ESLyric 扩展）。
//!
//! 行为对齐 AMLL `@applemusic-like-lyrics/lyric` 的 `lrc.rs` / `eslrc.rs`：
//! - 一行可带多个时间标签，展开为多行；
//! - 行结束时间 = 下一行开始时间；最后一行沿用 AMLL 的 999:99.999 上限；
//! - 元数据标签（`[ti:]` 等）忽略；
//! - ESLRC 的每个 `[时间]` 表示前一个词的结束时间。
//!
//! 解析为单遍 O(文本长度)：每行只在行内用游标前进，无回溯、无正则。

use chordal_core::perf::PerfScope;

use crate::model::{LyricLine, LyricWord};

/// AMLL 采用的最大时间戳（999:99.999），避免最后一行立刻过期。
pub const MAX_TIME_MS: u64 = 60_039_999;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricFormat {
    Lrc,
    Eslrc,
}

/// 自动识别格式并解析。
pub fn parse_auto(raw: &str) -> Vec<LyricLine> {
    match detect_format(raw) {
        LyricFormat::Eslrc => parse_eslrc(raw),
        LyricFormat::Lrc => parse_lrc(raw),
    }
}

/// 解析 LRC（行级时间）。
pub fn parse_lrc(raw: &str) -> Vec<LyricLine> {
    let _scope = PerfScope::enter("amll.parse");
    let mut lines = Vec::with_capacity(raw.lines().size_hint().0.min(2048));
    for line in raw.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let Some((times, text)) = parse_leading_times(line) else {
            continue;
        };
        for time in times {
            lines.push(LyricLine {
                words: vec![LyricWord {
                    start_time: time,
                    end_time: 0,
                    word: text.to_string(),
                    roman_word: String::new(),
                    obscene: false,
                }],
                translated_lyric: String::new(),
                roman_lyric: String::new(),
                start_time: time,
                end_time: 0,
                is_bg: false,
                is_duet: false,
            });
        }
    }
    finish_lrc_line_times(&mut lines);
    _scope.finish(raw.len() as u64);
    lines
}

/// 解析 ESLRC（逐词时间）。
pub fn parse_eslrc(raw: &str) -> Vec<LyricLine> {
    let _scope = PerfScope::enter("amll.parse");
    let mut lines = Vec::with_capacity(raw.lines().size_hint().0.min(2048));
    for line in raw.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line).trim();
        if line.is_empty() {
            continue;
        }
        let Some((first, mut rest)) = parse_one_time(line) else {
            continue;
        };
        let mut words = Vec::new();
        let mut start = first;
        while !rest.trim().is_empty() {
            let Some(open) = rest.find('[') else { break };
            let text = &rest[..open];
            let Some((end, after)) = parse_one_time(&rest[open..]) else {
                break;
            };
            words.push(LyricWord {
                start_time: start,
                end_time: end.max(start),
                word: text.to_string(),
                roman_word: String::new(),
                obscene: false,
            });
            start = end.max(start);
            rest = after;
        }
        if words.is_empty() {
            continue;
        }
        let start_time = words[0].start_time;
        let end_time = words[words.len() - 1].end_time;
        lines.push(LyricLine {
            words,
            translated_lyric: String::new(),
            roman_lyric: String::new(),
            start_time,
            end_time,
            is_bg: false,
            is_duet: false,
        });
    }
    finish_eslrc_line_times(&mut lines);
    _scope.finish(raw.len() as u64);
    lines
}

/// 把逐行译文并入歌词行（按行下标对齐，超出部分忽略）。
pub fn merge_translations(lines: &mut [LyricLine], translations: &[String]) {
    for (line, trans) in lines.iter_mut().zip(translations.iter()) {
        if !trans.trim().is_empty() {
            line.translated_lyric = trans.clone();
        }
    }
}

/// LRC 行结束时间 = 下一行开始时间；最后一个词的结束时间也同步（AMLL 行为）。
fn finish_lrc_line_times(lines: &mut [LyricLine]) {
    lines.sort_unstable_by_key(|l| l.start_time);

    let mut last_end = MAX_TIME_MS;
    for line in lines.iter_mut().rev() {
        line.end_time = last_end;
        if let Some(first) = line.words.first_mut() {
            first.end_time = last_end;
        }
        last_end = line.start_time.max(1); // 保证行结束 > 行开始
    }
    clamp_times(lines);
}

/// ESLRC 保留逐词时间：行结束 = 最后一个词结束（AMLL `process_lyrics`）。
fn finish_eslrc_line_times(lines: &mut [LyricLine]) {
    lines.sort_unstable_by_key(|l| l.start_time);
    for line in lines.iter_mut() {
        line.start_time = line.words.first().map(|w| w.start_time).unwrap_or(0);
        line.end_time = line
            .words
            .last()
            .map(|w| w.end_time)
            .unwrap_or(line.start_time);
    }
    clamp_times(lines);
}

fn clamp_times(lines: &mut [LyricLine]) {
    for line in lines.iter_mut() {
        line.start_time = line.start_time.min(MAX_TIME_MS);
        line.end_time = line.end_time.min(MAX_TIME_MS);
        for word in &mut line.words {
            word.start_time = word.start_time.min(MAX_TIME_MS);
            word.end_time = word.end_time.min(MAX_TIME_MS).max(word.start_time);
        }
    }
}

fn detect_format(raw: &str) -> LyricFormat {
    for line in raw.lines() {
        let Some((_, rest)) = parse_one_time(line.trim_start()) else {
            continue;
        };
        let Some(open) = rest.find('[') else { continue };
        // 第二个时间标签前有非空文本 → 词级时间戳（ESLRC）。
        if open > 0 && !rest[..open].trim().is_empty() {
            let after_tag = &rest[open..];
            if parse_one_time(after_tag).is_some() {
                return LyricFormat::Eslrc;
            }
        }
    }
    LyricFormat::Lrc
}

/// 解析行首连续的时间标签，返回 `(所有时间, 剩余文本)`。
fn parse_leading_times(line: &str) -> Option<(Vec<u64>, &str)> {
    let mut times = Vec::with_capacity(2);
    let mut rest = line;
    while let Some((t, after)) = parse_one_time(rest) {
        times.push(t);
        rest = after;
    }
    if times.is_empty() {
        None
    } else {
        Some((times, rest))
    }
}

/// 解析一个 `[mm:ss(.fff)]` 标签。失败返回 None（元数据标签也走这里）。
fn parse_one_time(src: &str) -> Option<(u64, &str)> {
    let src = src.strip_prefix('[')?;
    let close = src.find(']')?;
    let tag = &src[..close];
    let after = &src[close + 1..];
    let ms = parse_time_tag(tag)?;
    Some((ms, after))
}

/// `mm:ss` / `mm:ss.ff` / `mm:ss:fff`。分可以任意大（AMLL 允许 168:10）。
pub fn parse_time_tag(tag: &str) -> Option<u64> {
    let (min_str, rest) = tag.split_once(':')?;
    let minutes: u64 = min_str.trim().parse().ok()?;
    let (sec_str, frac) = if let Some((s, f)) = rest.split_once('.') {
        (s, Some(f))
    } else if let Some((s, f)) = rest.split_once(':') {
        (s, Some(f))
    } else {
        (rest, None)
    };
    let seconds: u64 = sec_str.trim().parse().ok()?;
    if seconds >= 60 {
        return None;
    }
    let millis = match frac {
        None => 0,
        Some(f) => {
            let f = f.trim();
            if f.is_empty() || f.len() > 3 || !f.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let value: u64 = f.parse().ok()?;
            match f.len() {
                1 => value * 100,
                2 => value * 10,
                _ => value,
            }
        }
    };
    Some(
        minutes
            .saturating_mul(60_000)
            .saturating_add(seconds.saturating_mul(1_000))
            .saturating_add(millis),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_time_tags() {
        assert_eq!(parse_time_tag("00:01.12"), Some(1020 + 100));
        assert_eq!(parse_time_tag("00:10.254"), Some(10254));
        assert_eq!(parse_time_tag("01:10.1"), Some(70100));
        assert_eq!(parse_time_tag("168:10:254"), Some(10_090_254));
        assert_eq!(parse_time_tag("00:60.000"), None);
        assert_eq!(parse_time_tag("ti"), None);
    }

    #[test]
    fn expands_multi_timestamp_lines() {
        let lines = parse_lrc("[00:01.00]hello\n[00:05.00][00:09.00]world");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text(), "hello");
        assert_eq!(lines[1].start_time, 5000);
        assert_eq!(lines[2].start_time, 9000);
        // 行结束时间 = 下一行开始时间。
        assert_eq!(lines[0].end_time, 5000);
        assert_eq!(lines[1].end_time, 9000);
    }

    #[test]
    fn skips_metadata_and_bad_lines() {
        let lines = parse_lrc("[ti:test]\n[ar:artist]\n[00:01.00]ok\nnot a lyric");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "ok");
    }

    #[test]
    fn eslrc_words_and_line_end() {
        let raw = "[00:10.82]Test[00:10.97] Word[00:12.62]";
        let lines = parse_eslrc(raw);
        assert_eq!(lines.len(), 1);
        let words = &lines[0].words;
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "Test");
        assert_eq!(words[0].start_time, 10_820);
        assert_eq!(words[0].end_time, 10_970);
        assert_eq!(words[1].word, " Word");
        assert_eq!(words[1].end_time, 12_620);
        assert_eq!(lines[0].end_time, 12_620);
    }

    #[test]
    fn detects_eslrc_not_multi_timestamp_lrc() {
        assert_eq!(
            detect_format("[00:01.00][00:05.00]same lyric"),
            LyricFormat::Lrc
        );
        assert_eq!(
            detect_format("[00:10.82]Test[00:10.97] Word[00:12.62]"),
            LyricFormat::Eslrc
        );
    }

    #[test]
    fn merge_translations_aligns_by_index() {
        let mut lines = parse_lrc("[00:01.00]a\n[00:02.00]b");
        merge_translations(&mut lines, &["甲".into()]);
        assert_eq!(lines[0].translated_lyric, "甲");
        assert!(lines[1].translated_lyric.is_empty());
    }
}
