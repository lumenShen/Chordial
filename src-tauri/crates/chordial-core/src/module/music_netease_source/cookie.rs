//! Cookie 规范化工具 — 处理 ncmapi 返回的 Set-Cookie 原始字符串。
//!
//! 独立成模块：纯函数、无状态，便于单元测试与复用。

/// 检测 cookie 字符串是否为 Set-Cookie 原始格式（需要规范化）。
///
/// 标准 Cookie 请求头格式只含 `NAME=VALUE` 对，不应出现 `Path=`/`Max-Age=`/
/// `Expires=`/`HttpOnly`/`Secure`/`SameSite=` 等 Set-Cookie 属性。
pub fn needs_cookie_normalization(cookie: &str) -> bool {
    // 标准 Cookie 头不应含这些属性名（不区分大小写）
    const MARKERS: &[&str] = &[
        "path=",
        "max-age=",
        "expires=",
        "httponly",
        "secure",
        "samesite=",
        "domain=",
    ];
    let lower = cookie.to_lowercase();
    MARKERS.iter().any(|m| lower.contains(m))
}

/// 将 ncmapi `/login/qr/check` 返回的 Set-Cookie 串解析为标准 Cookie 请求头格式。
///
/// # 背景
///
/// ncmapi 在 `code==803` 时返回的 `cookie` 字段是多个 Set-Cookie 用 `", "` 连接的字符串，
/// 每条形如：`NAME=VALUE; Path=/; HttpOnly; Expires=Day, DD Mon YYYY HH:MM:SS GMT`。
///
/// 直接当作 Cookie 请求头发送时，ncmapi 会把 `Path`/`Expires`/`Max-Age` 等属性名
/// 当成 cookie 名解析，真正的 `MUSIC_U` 被属性淹没，导致登录态丢失（profile=null）。
///
/// # 解析策略
///
/// 1. 将 `", "` 替换为 `"; "`，把多 cookie 分隔符与属性分隔符统一
///    （注意 `Expires=Fri, 06 Aug` 内的 `", "` 也会被替换，但后续步骤会跳过非 token 段）
/// 2. 按 `;` 切分，对每段 trim 后判断是否为有效 `NAME=VALUE` 对：
///    - 必须含 `=`
///    - `NAME` 必须为合法 token（仅字母数字 `_` `-` `.`）
///    - `NAME` 不在保留属性名集合内（`Path`/`Domain`/`Expires`/`Max-Age`/...）
/// 3. 对重名 cookie 只保留第一次出现的（去重）
/// 4. 用 `"; "` 连接所有有效对
pub fn parse_set_cookie_to_header(set_cookie: &str) -> String {
    // 已知的 Set-Cookie 属性名（不区分大小写）
    const RESERVED: &[&str] = &[
        "Path",
        "Domain",
        "Expires",
        "Max-Age",
        "HttpOnly",
        "Secure",
        "SameSite",
        "Comment",
        "Version",
        "Priority",
    ];

    // 统一分隔符
    let normalized = set_cookie.replace(", ", "; ");

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut pairs: Vec<String> = Vec::new();

    for segment in normalized.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        // 必须含 =（HttpOnly / Secure 等 flag 没有 =）
        let eq_pos = match segment.find('=') {
            Some(p) => p,
            None => continue,
        };
        let name = segment[..eq_pos].trim();
        // name 必须非空且为合法 token
        if name.is_empty() || name.contains(' ') || name.contains(',') {
            continue;
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            continue;
        }
        // 跳过保留属性
        if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) {
            continue;
        }
        // 去重（同名 cookie 只保留首次）
        if !seen.insert(name.to_string()) {
            continue;
        }
        pairs.push(segment.to_string());
    }

    pairs.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_cookie() {
        let raw = "MUSIC_U=abc123; Path=/; HttpOnly";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_U=abc123");
    }

    #[test]
    fn test_parse_multiple_cookies() {
        let raw = "MUSIC_U=abc123; Path=/; HttpOnly, __csrf=xyz; Path=/; HttpOnly";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_U=abc123; __csrf=xyz");
    }

    #[test]
    fn test_parse_cookie_with_expires_date() {
        // Expires 内含 ", "，不应破坏解析
        let raw = "MUSIC_U=abc123; Path=/; HttpOnly; Expires=Fri, 06 Aug 2094 17:42:50 GMT, __csrf=xyz; Path=/; HttpOnly";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_U=abc123; __csrf=xyz");
    }

    #[test]
    fn test_parse_cookie_dedup() {
        let raw = "MUSIC_U=abc; Path=/, MUSIC_U=def; Path=/";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_U=abc");
    }

    #[test]
    fn test_parse_cookie_preserves_order() {
        let raw = "MUSIC_A_T=1513381581471; Max-Age=2147483647; Expires=Fri, 06 Aug 2094 17:42:50 GMT, MUSIC_U=real_token; Path=/; HttpOnly, __csrf=csrf_val; Path=/";
        let parsed = parse_set_cookie_to_header(raw);
        assert_eq!(parsed, "MUSIC_A_T=1513381581471; MUSIC_U=real_token; __csrf=csrf_val");
    }

    #[test]
    fn test_parse_empty() {
        assert_eq!(parse_set_cookie_to_header(""), "");
        assert_eq!(parse_set_cookie_to_header("   "), "");
    }

    #[test]
    fn test_needs_normalization_set_cookie_format() {
        // 含 Set-Cookie 属性 → 需要规范化
        assert!(needs_cookie_normalization("MUSIC_U=abc; Path=/; HttpOnly"));
        assert!(needs_cookie_normalization("MUSIC_U=abc; Max-Age=3600"));
        assert!(needs_cookie_normalization("MUSIC_U=abc; Expires=Fri, 06 Aug 2094"));
        assert!(needs_cookie_normalization("MUSIC_U=abc; Secure"));
        assert!(needs_cookie_normalization("MUSIC_U=abc; SameSite=Lax"));
        assert!(needs_cookie_normalization("MUSIC_U=abc; Domain=.music.163.com"));
    }

    #[test]
    fn test_needs_normalization_standard_cookie() {
        // 标准 Cookie 头格式 → 不需要规范化
        assert!(!needs_cookie_normalization(""));
        assert!(!needs_cookie_normalization("MUSIC_U=abc"));
        assert!(!needs_cookie_normalization("MUSIC_U=abc; __csrf=xyz"));
        assert!(!needs_cookie_normalization(
            "MUSIC_U=abc123; MUSIC_R_U=def456; ntoken_entrance=1"
        ));
    }
}