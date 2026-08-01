//! 艺术家名称拆分工具 — 将单个 artist 标签字符串按常见分隔符拆分为多个独立 artist。
//!
//! 支持的分隔符包括半角（`/`、`&`、`,`、`;`）、全角（`、`、`，`、`；`）
//! 以及关键字（`feat.`、`ft.`、`featuring`、`with`、`vs.`、`versus`）。

/// 将单个 artist 标签字符串按常见分隔符拆分为多个独立 artist 名称。
///
/// 支持的分隔符（不区分大小写）：
/// - 半角：`/`、`&`、`,`、`;`
/// - 全角：`、`、`，`、`；`
/// - 关键字：`feat.`、`ft.`、`featuring`、`with`、`vs.`、`versus`
///
/// 例：`"周杰伦 & 方文山"` → `["周杰伦", "方文山"]`
/// 例：`"A feat. B"` → `["A", "B"]`
/// 例：`"AC/DC"` → `["AC/DC"]`（不含空格的 `/` 视为乐队名一部分，不拆分）
///
/// 空字符串返回 `["未知艺术家"]`，保留与旧实现兼容的默认值。
pub(super) fn split_artist_names(raw: Option<&str>) -> Vec<String> {
    let raw = match raw {
        Some(s) => s.trim(),
        None => return vec!["未知艺术家".to_string()],
    };
    if raw.is_empty() {
        return vec!["未知艺术家".to_string()];
    }

    // 策略：先按 ASCII 分隔符 (`/`、`&`、`,`、`;`) 和全角分隔符拆分，
    // 但 `/` 仅在两侧有空格时才视为分隔符（避免误拆 "AC/DC"）。
    // 关键字分隔符（feat./ft./featuring/with/vs./versus）使用正则式分割。
    let mut parts: Vec<String> = Vec::new();

    // 第一轮：按 ASCII & 全角分隔符拆分
    for segment in raw.split(|c: char| matches!(c, '&' | ',' | ';' | '、' | '，' | '；')) {
        // `/` 仅在两侧均有空格时才拆分
        for sub in split_on_slash_with_spaces(segment) {
            let trimmed = sub.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }

    // 第二轮：按关键字分隔符拆分（feat./ft./featuring/with/vs./versus）
    let mut final_parts: Vec<String> = Vec::with_capacity(parts.len());
    for part in parts {
        for sub in split_on_keywords(&part) {
            let trimmed = sub.trim();
            if !trimmed.is_empty() {
                final_parts.push(trimmed.to_string());
            }
        }
    }

    if final_parts.is_empty() {
        vec!["未知艺术家".to_string()]
    } else {
        final_parts
    }
}

/// 拆分形如 `"A / B"` 的字符串（要求 `/` 两侧至少有一个空格）。
///
/// `"AC/DC"` → `["AC/DC"]`（不拆）
/// `"A / B"` → `["A", "B"]`
/// `"A/B"` → `["A/B"]`（不拆）
fn split_on_slash_with_spaces(s: &str) -> Vec<&str> {
    let mut result: Vec<&str> = Vec::new();
    let bytes = s.as_bytes();
    let mut last = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'/' && i > 0 && i + 1 < bytes.len() {
            let left_space = bytes[i - 1] == b' ';
            let right_space = bytes[i + 1] == b' ';
            if left_space && right_space {
                result.push(&s[last..i]);
                last = i + 1;
            }
        }
        i += 1;
    }
    result.push(&s[last..]);
    result
}

/// 按关键字分隔符拆分（不区分大小写）。
///
/// 支持的关键字：`feat.`、`ft.`、`featuring`、`with`、`vs.`、`versus`
/// 关键字前后必须是空格或字符串边界。
fn split_on_keywords(s: &str) -> Vec<String> {
    let lower = s.to_lowercase();
    let keywords = ["feat.", "ft.", "featuring", "with", "vs.", "versus"];

    let mut indices: Vec<usize> = Vec::new();
    for kw in &keywords {
        let mut start = 0usize;
        while let Some(pos) = lower[start..].find(kw) {
            let abs = start + pos;
            // 关键字前必须是字符串开头或空格
            let before_ok = abs == 0 || s.as_bytes()[abs - 1] == b' ';
            // 关键字后必须是字符串结尾或空格
            let after_pos = abs + kw.len();
            let after_ok = after_pos >= s.len()
                || s.as_bytes()[after_pos] == b' '
                || s.as_bytes()[after_pos] == b'(';
            if before_ok && after_ok {
                indices.push(abs);
            }
            start = abs + kw.len();
            if start >= s.len() {
                break;
            }
        }
    }

    if indices.is_empty() {
        return vec![s.to_string()];
    }

    indices.sort();
    let mut result: Vec<String> = Vec::with_capacity(indices.len() + 1);
    let mut last = 0usize;
    for idx in indices {
        result.push(s[last..idx].trim().to_string());
        // 跳过关键字本身和后续空格
        let mut kw_end = idx;
        while kw_end < s.len() && s.as_bytes()[kw_end] != b' ' {
            kw_end += 1;
        }
        last = kw_end;
    }
    if last < s.len() {
        result.push(s[last..].trim().to_string());
    }
    result.retain(|s| !s.is_empty());
    if result.is_empty() {
        vec![s.to_string()]
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::split_artist_names;

    #[test]
    fn test_split_single_artist() {
        assert_eq!(split_artist_names(Some("周杰伦")), vec!["周杰伦"]);
    }

    #[test]
    fn test_split_ampersand() {
        assert_eq!(
            split_artist_names(Some("周杰伦 & 方文山")),
            vec!["周杰伦", "方文山"]
        );
    }

    #[test]
    fn test_split_slash_with_spaces() {
        assert_eq!(
            split_artist_names(Some("A / B / C")),
            vec!["A", "B", "C"]
        );
    }

    #[test]
    fn test_no_split_ac_dc() {
        // AC/DC 不应被拆分（`/` 两侧无空格）
        assert_eq!(split_artist_names(Some("AC/DC")), vec!["AC/DC"]);
    }

    #[test]
    fn test_split_feat() {
        assert_eq!(
            split_artist_names(Some("A feat. B")),
            vec!["A", "B"]
        );
    }

    #[test]
    fn test_split_ft() {
        assert_eq!(
            split_artist_names(Some("A ft. B")),
            vec!["A", "B"]
        );
    }

    #[test]
    fn test_split_chinese_comma() {
        assert_eq!(
            split_artist_names(Some("周杰伦、方文山")),
            vec!["周杰伦", "方文山"]
        );
    }

    #[test]
    fn test_split_none() {
        assert_eq!(split_artist_names(None), vec!["未知艺术家"]);
    }

    #[test]
    fn test_split_empty() {
        assert_eq!(split_artist_names(Some("")), vec!["未知艺术家"]);
    }

    #[test]
    fn test_split_mixed() {
        assert_eq!(
            split_artist_names(Some("A & B feat. C、D")),
            vec!["A", "B", "C", "D"]
        );
    }
}
