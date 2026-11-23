//! 崩溃日志：report（由 crash_log.rs 拆分，行为不变）。

use super::*;
use super::time_util::*;

pub(super) fn build_panic_report(
    ctx: &CrashContext,
    message: &str,
    location: Option<&str>,
    thread: &str,
    backtrace: &str,
) -> String {
    let mut out = String::with_capacity(8192);
    out.push_str("Chordial 崩溃报告\n");
    out.push_str("请把本文件（以及同目录的 chordial.log）附在问题反馈中。\n\n");
    out.push_str(&format!("version   : {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!(
        "os        : {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    out.push_str(&format!("time      : {}\n", utc_timestamp()));
    out.push_str(&format!("pid       : {}\n", std::process::id()));
    out.push_str(&format!("thread    : {thread}\n"));
    out.push_str(&format!("panic     : {message}\n"));
    if let Some(location) = location {
        out.push_str(&format!("location  : {location}\n"));
    }
    out.push_str(&format!("log       : {}\n", ctx.log_path.display()));

    out.push_str("\n----- backtrace -----\n");
    out.push_str(backtrace.trim_end());
    out.push('\n');

    append_log_tail(&mut out, ctx);
    redact_secrets(&out)
}

pub(super) fn append_log_tail(out: &mut String, ctx: &CrashContext) {
    if let Some(tail) = read_tail(&ctx.log_path, LOG_TAIL_BYTES, LOG_TAIL_LINES)
        && !tail.trim().is_empty()
    {
        out.push_str("\n----- 最近日志（本次运行） -----\n");
        out.push_str(&tail);
        if !tail.ends_with('\n') {
            out.push('\n');
        }
    }
    if let Some(tail) = read_tail(&ctx.prev_log_path, PREV_LOG_TAIL_BYTES, LOG_TAIL_LINES)
        && !tail.trim().is_empty()
    {
        out.push_str("\n----- 最近日志（上次运行） -----\n");
        out.push_str(&tail);
        if !tail.ends_with('\n') {
            out.push('\n');
        }
    }
}

pub(super) fn write_report_file(dir: &Path, prefix: &str, content: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let seq = REPORT_SEQ.fetch_add(1, Ordering::Relaxed);
    let name = format!(
        "{prefix}-{}-{}-{seq}.log",
        utc_compact(),
        std::process::id()
    );
    let path = dir.join(&name);
    let tmp = dir.join(format!(".{name}.tmp"));

    // 先写临时文件再改名，避免崩溃时留下半截报告。
    fs::write(&tmp, content)?;
    if fs::rename(&tmp, &path).is_err() {
        // 同名报告已存在或临时文件被占用：直接写最终文件兜底。
        fs::write(&path, content)?;
        let _ = fs::remove_file(&tmp);
    }
    // latest 只是方便用户找文件，写失败不影响主报告。
    let _ = fs::write(dir.join(LATEST_CRASH_NAME), content);
    let _ = prune_reports(dir, CRASH_KEEP);
    Ok(path)
}

/// 只保留最新的 `keep` 份报告（按文件名中的时间戳排序）。
pub(super) fn prune_reports(dir: &Path, keep: usize) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut reports: Vec<(String, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_report = (name.starts_with("crash-") || name.starts_with("fatal-"))
                && name.ends_with(".log")
                && name != LATEST_CRASH_NAME;
            is_report.then(|| (name, entry.path()))
        })
        .collect();
    // 文件名以 UTC 时间戳开头，字典序即时间序。
    reports.sort_by(|a, b| b.0.cmp(&a.0));
    let mut removed = 0;
    for (_, path) in reports.into_iter().skip(keep) {
        if fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

pub(super) fn rotate_previous_log(ctx: &CrashContext) {
    let Ok(meta) = fs::metadata(&ctx.log_path) else {
        return;
    };
    if meta.len() == 0 {
        return;
    }
    let _ = fs::remove_file(&ctx.prev_log_path);
    let _ = fs::rename(&ctx.log_path, &ctx.prev_log_path);
}

// ---------------------------------------------------------------------------
// 日志尾部 / 脱敏
// ---------------------------------------------------------------------------

pub(super) fn read_tail(path: &Path, max_bytes: u64, max_lines: usize) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start)).ok()?;

    let mut buf = Vec::with_capacity((len - start).min(max_bytes) as usize);
    file.take(max_bytes).read_to_end(&mut buf).ok()?;

    // 从中间截断时丢掉第一行残片。
    let slice = if start > 0 {
        let newline = buf.iter().position(|byte| *byte == b'\n')?;
        &buf[newline + 1..]
    } else {
        &buf[..]
    };
// ---------------------------------------------------------------------------
    let text = String::from_utf8_lossy(slice);
    let lines: Vec<&str> = text.lines().collect();
    let start_line = lines.len().saturating_sub(max_lines);
    Some(lines[start_line..].join("\n"))
}

/// 崩溃报告会被用户公开上传，去掉登录态等敏感值。
pub(super) fn redact_secrets(input: &str) -> String {
    const NEEDLES: [&str; 2] = ["MUSIC_U=", "__csrf="];
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    loop {
        let mut next: Option<(usize, &str)> = None;
        for needle in NEEDLES {
            if let Some(pos) = rest.find(needle)
                && next.is_none_or(|(best, _)| pos < best)
            {
                next = Some((pos, needle));
            }
        }
        let Some((pos, needle)) = next else {
            out.push_str(rest);
            break;
        };

        let value_start = pos + needle.len();
        out.push_str(&rest[..value_start]);
        let value = &rest[value_start..];
        let value_end = value
            .find(|c: char| {
                c.is_whitespace() || matches!(c, ';' | '&' | ',' | '"' | '\'' | '}' | ']')
            })
            .unwrap_or(value.len());
        out.push_str("<redacted>");
        rest = &value[value_end..];
    }
    out
}

// ---------------------------------------------------------------------------
// UTC 时间戳（不额外引入时间库）
// ---------------------------------------------------------------------------

