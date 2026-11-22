//! 歌词领域模型。
//!
//! 字段名与 AMLL 的 `LyricWord` / `LyricLine`（packages/core/src/interfaces.ts）
//! 一一对应，时间统一为**毫秒整数**。所有实体构造后只读；播放器内部另有一份
//! 处理后的副本（[`crate::document::LyricDocument`]），渲染层不会修改原始歌词。

use std::fmt;

/// 一行歌词中的一个词（或字）以及它的时间区间。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LyricWord {
    pub start_time: u64,
    pub end_time: u64,
    pub word: String,
    pub roman_word: String,
    /// 是否包含不雅用语（AMLL 的 `obscene` 字段）。
    pub obscene: bool,
}

impl LyricWord {
    pub fn new(word: impl Into<String>, start_time: u64, end_time: u64) -> Self {
        Self {
            start_time,
            end_time,
            word: word.into(),
            roman_word: String::new(),
            obscene: false,
        }
    }

    #[inline]
    pub fn duration_ms(&self) -> u64 {
        self.end_time.saturating_sub(self.start_time)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.word.trim().is_empty()
    }
}

/// 一行歌词。背景行（`is_bg`）与主行成对出现，对唱行（`is_duet`）右对齐。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LyricLine {
    pub words: Vec<LyricWord>,
    pub translated_lyric: String,
    pub roman_lyric: String,
    pub start_time: u64,
    pub end_time: u64,
    pub is_bg: bool,
    pub is_duet: bool,
}

impl LyricLine {
    pub fn new(words: Vec<LyricWord>) -> Self {
        let start_time = words.first().map(|w| w.start_time).unwrap_or(0);
        let end_time = words.last().map(|w| w.end_time).unwrap_or(start_time);
        Self {
            words,
            translated_lyric: String::new(),
            roman_lyric: String::new(),
            start_time,
            end_time,
            is_bg: false,
            is_duet: false,
        }
    }

    #[inline]
    pub fn duration_ms(&self) -> u64 {
        self.end_time.saturating_sub(self.start_time)
    }

    /// 主歌词文本（逐词拼接）。
    pub fn text(&self) -> String {
        let capacity: usize = self.words.iter().map(|w| w.word.len()).sum();
        let mut out = String::with_capacity(capacity);
        for w in &self.words {
            out.push_str(&w.word);
        }
        out
    }

    /// 该行是否为空（没有任何可见文字）。
    #[inline]
    pub fn is_visible_empty(&self) -> bool {
        self.words.iter().all(|w| w.word.trim().is_empty())
            && self.translated_lyric.trim().is_empty()
            && self.roman_lyric.trim().is_empty()
    }
}

impl fmt::Display for LyricLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", if self.is_bg { "BG " } else { "" }, self.text())
    }
}

/// 处理完成的歌词文档。
#[derive(Debug, Clone, Default)]
pub struct LyricDocument {
    pub lines: Vec<LyricLine>,
    /// 是否存在逐词时间（任一行 words.len() > 1）。
    pub is_dynamic: bool,
    /// 是否包含对唱行。
    pub has_duet: bool,
}

impl LyricDocument {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.lines.len()
    }
}
