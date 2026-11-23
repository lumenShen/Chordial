//! 网易云映射：lyrics（由 mapping.rs 拆分，行为不变）。

use super::*;

#[derive(Deserialize)]
pub(super) struct NewLyricResp {
    #[serde(default)]
    lrc: Option<LyricBody>,
    #[serde(default)]
    yrc: Option<LyricBody>,
    #[serde(default)]
    tlyric: Option<LyricBody>,
    /// YRC 逐字对齐的译文；优先于 tlyric。
    #[serde(default)]
    ytlrc: Option<LyricBody>,
}

#[derive(Deserialize)]
pub(super) struct LyricResp {
    #[serde(default)]
    lrc: Option<LyricBody>,
    #[serde(default)]
    tlyric: Option<LyricBody>,
    /// QQ/网易逐词歌词（YRC 文本）。
    #[serde(default)]
    yrc: Option<LyricBody>,
}

#[derive(Deserialize)]
pub(super) struct LyricBody {
    #[serde(default)]
    lyric: Option<String>,
}

/// `/lyric/new`：优先使用逐词 YRC（新接口返回“JSON 头 + YRC 文本”混合格式）。
pub fn map_lyrics_new(body: &serde_json::Value) -> Option<Lyrics> {
    let resp: NewLyricResp = serde_json::from_value(body.clone()).ok()?;
    let lrc_raw = resp
        .lrc
        .and_then(|l| l.lyric)
        .filter(|s| !s.trim().is_empty());
    let yrc_raw = resp
        .yrc
        .and_then(|l| l.lyric)
        .filter(|s| !s.trim().is_empty());
    let tlyric = resp
        .tlyric
        .and_then(|l| l.lyric)
        .filter(|s| !s.trim().is_empty());
    let ytlrc = resp
        .ytlrc
        .and_then(|l| l.lyric)
        .filter(|s| !s.trim().is_empty());
    let trans_raw = if yrc_raw.is_some() {
        ytlrc.or(tlyric)
    } else {
        tlyric.or(ytlrc)
    };
    build_lyrics(lrc_raw, yrc_raw, trans_raw)
}

/// `/lyric` 兼容接口 → 同一套 YRC 优先/LRC 兜底逻辑。
pub fn map_lyrics(body: &serde_json::Value) -> Option<Lyrics> {
    let resp: LyricResp = serde_json::from_value(body.clone()).ok()?;
    build_lyrics(
        resp.lrc
            .and_then(|l| l.lyric)
            .filter(|s| !s.trim().is_empty()),
        resp.yrc
            .and_then(|l| l.lyric)
            .filter(|s| !s.trim().is_empty()),
        resp.tlyric
            .and_then(|l| l.lyric)
            .filter(|s| !s.trim().is_empty()),
    )
}

/// 统一构建：YRC 有内容就作为逐字歌词源；否则用 LRC。
/// 制作信息（作词/作曲/编曲…）单独收集到 [`Lyrics::credits`]，不进入歌词流。
pub(super) fn build_lyrics(
    lrc_raw: Option<String>,
    yrc_raw: Option<String>,
    trans_raw: Option<String>,
) -> Option<Lyrics> {
    let mut meta: Vec<LyricLine> = Vec::new();
    let mut yrc_text = String::new();
    if let Some(raw) = yrc_raw.as_deref() {
        split_rich_yrc(raw, &mut meta, &mut yrc_text);
    }
    let mut lrc_text = String::new();
    if let Some(raw) = lrc_raw.as_deref() {
        split_rich_yrc(raw, &mut meta, &mut lrc_text);
    }

    let yrc_parsed = if yrc_text.trim().is_empty() {
        None
    } else {
        let parsed = Lyrics::parse_yrc(&yrc_text);
        (!parsed.lines.is_empty()).then_some(parsed)
    };
    let lrc_parsed = if lrc_text.trim().is_empty() {
        None
    } else {
        Some(Lyrics::parse_lrc(&lrc_text))
    };

    let mut lyrics = if let Some(parsed) = yrc_parsed {
        parsed
    } else if let Some(parsed) = lrc_parsed.clone() {
        parsed
    } else if let Some(raw) = lrc_raw.as_deref() {
        Lyrics::parse_lrc(raw)
    } else if !yrc_text.trim().is_empty() {
        Lyrics::parse_yrc(&yrc_text)
    } else {
        Lyrics::default()
    };

    // 正文里混入的「作词: xxx」行同样要摘出来，否则仍会按时间轴进入歌词流。
    strip_inline_credits(&mut lyrics, &mut meta);
    // 即使 YRC 胜出，也保留 LRC 骨架里的制作信息（YRC 常常只有 JSON 元数据）。
    if let Some(lrc) = &lrc_parsed {
        for line in &lrc.lines {
            if is_credit_line(&line.text) {
                meta.push(line.clone());
            }
        }
    }
    attach_credits(&mut lyrics, meta);
    if let Some(raw) = trans_raw {
        align_translation(&mut lyrics, &raw);
    }
    if lyrics.lines.is_empty() && lyrics.timed_lines.is_empty() {
        None
    } else {
        Some(lyrics)
    }
}

/// 拆分 `/lyric/new` 的混合文本：JSON 元信息行 → `meta`，歌词文本行 → `text`。
pub(super) fn split_rich_yrc(raw: &str, meta: &mut Vec<LyricLine>, text: &mut String) {
    let raw = raw.trim_start_matches('\u{feff}');
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('{') {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
                && let Some(meta_line) = meta_line_from_json(&value)
            {
                meta.push(meta_line);
            }
        } else {
            text.push_str(line);
            text.push('\n');
        }
    }
}

pub(super) fn meta_line_from_json(value: &serde_json::Value) -> Option<LyricLine> {
    let time_ms = value.get("t").and_then(json_u64).unwrap_or(0);
    let text = value
        .get("c")
        .and_then(|c| c.as_array())?
        .iter()
        .filter_map(|segment| segment.get("tx").and_then(|t| t.as_str()))
        .collect::<String>();
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(LyricLine { time_ms, text })
    }
}

pub(super) fn json_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
}

/// 常见制作信息角色词。命中还要求后面跟着分隔符（冒号/斜杠/破折号），
/// 避免把「词不达意」这类正常歌词误判成元数据。
pub(super) const CREDIT_ROLES: &[&str] = &[
    "作词人",
    "作曲人",
    "作词",
    "作曲",
    "词曲",
    "编曲人",
    "编曲",
    "制作人",
    "音乐制作人",
    "制作",
    "监制人",
    "监制",
    "出品人",
    "出品",
    "执行制作",
    "制作统筹",
    "统筹",
    "企划",
    "策划",
    "发行人",
    "发行",
    "版权",
    "母带工程师",
    "母带制作",
    "母带",
    "混音工程师",
    "混音师",
    "混音",
    "录音工程师",
    "录音师",
    "录音室",
    "录音",
    "配唱制作人",
    "和声编写",
    "和声",
    "吉他",
    "贝斯",
    "键盘",
    "弦乐",
    "原唱",
    "原曲",
    "演唱",
    "音频编辑",
    "艺术总监",
    "插画",
    "摄影",
    // 英文制作信息（前缀匹配前会转小写）。
    "lyrics by",
    "lyric by",
    "lyrics",
    "composed by",
    "composer",
    "composition",
    "music by",
    "written by",
    "words by",
    "arranged by",
    "arranger",
    "arrangement",
    "produced by",
    "producer",
    "executive producer",
    "mastering engineer",
    "mastered by",
    "mastering",
    "mixed by",
    "mixing",
    "recorded by",
    "recording",
    "backing vocals",
    "guitar",
    "bass",
    "drums",
    "keyboard",
    "strings",
    "vocals",
];

/// 单字/过短的角色词只有紧跟冒号时才算元数据（`词: xxx` / `曲：xxx`）。
pub(super) const STRICT_CREDIT_ROLES: &[&str] = &["词", "曲", "op", "sp", "mix"];

/// 该行是否只是一条制作信息（作词/作曲/编曲…）。
pub(super) fn is_credit_line(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    let matches = |role: &str| -> bool {
        let Some(rest) = lower.strip_prefix(role) else {
            return false;
        };
        let rest = rest.trim_start();
        if rest.is_empty() {
            return true;
        }
        if STRICT_CREDIT_ROLES.contains(&role) {
            matches!(rest.chars().next(), Some(':' | '：'))
        } else {
            matches!(
                rest.chars().next(),
                Some(':' | '：' | '/' | '／' | '-' | '—' | '–')
            )
        }
    };
    CREDIT_ROLES.iter().any(|role| matches(role))
        || STRICT_CREDIT_ROLES.iter().any(|role| matches(role))
}

/// 去重用的归一化键：忽略空白与全/半角冒号差异（`作词: A` == `作词：A`）。
pub(super) fn credit_key(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| match ch {
            '：' => ':',
            _ => ch,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// 把正文里混入的制作信息行从时间轴摘到 `meta`。
///
/// YRC 有时会把「作词: xxx」写成普通逐词行，LRC 也常见同样的首行；
/// 这些行如果留在 `lines` / `timed_lines` 里就会跟着歌词滚动。
pub(super) fn strip_inline_credits(lyrics: &mut Lyrics, meta: &mut Vec<LyricLine>) {
    let mut kept = Vec::with_capacity(lyrics.lines.len());
    for line in lyrics.lines.drain(..) {
        if is_credit_line(&line.text) {
            meta.push(line);
        } else {
            kept.push(line);
        }
    }
    lyrics.lines = kept;

    lyrics.timed_lines.retain(|line| {
        let text = line.text();
        if is_credit_line(&text) {
            meta.push(LyricLine {
                time_ms: line.start_ms,
                text,
            });
            false
        } else {
            true
        }
    });
}

/// 把制作信息写入 [`Lyrics::credits`]，按首次出现顺序去重。
///
/// 这些信息不进入 `lines` / `timed_lines`，因此不会按歌词时间轴滚动；
/// 展示层统一把它们放在歌词末尾作为补充信息。
pub(super) fn attach_credits(lyrics: &mut Lyrics, mut meta: Vec<LyricLine>) {
    if meta.is_empty() {
        return;
    }
    meta.sort_by_key(|l| l.time_ms);

    let mut seen = HashSet::with_capacity(meta.len());
    for line in meta {
        let text = line.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if seen.insert(credit_key(&text)) {
            lyrics.credits.push(text);
        }
    }
}

/// tlyric 时间戳可能有几十毫秒偏差，按最近行匹配。
pub(super) fn align_translation(lyrics: &mut Lyrics, raw: &str) {
    let trans = Lyrics::parse_lrc(raw);
    if trans.lines.is_empty() {
        return;
    }
    let mut out = Vec::with_capacity(lyrics.lines.len());
    for line in &lyrics.lines {
        let idx = trans.lines.partition_point(|t| t.time_ms < line.time_ms);
        let best = [idx.saturating_sub(1), idx]
            .into_iter()
            .filter_map(|i| trans.lines.get(i).map(|t| (i, t)))
            .min_by_key(|(_, t)| t.time_ms.abs_diff(line.time_ms));
        if let Some((_, t)) = best
            && t.time_ms.abs_diff(line.time_ms) <= 120
        {
            out.push(t.text.clone());
        } else {
            out.push(String::new());
        }
    }
    lyrics.translation = out;
}
