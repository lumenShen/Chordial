//! 崩溃兜底与日志落盘。
//!
//! 启动时最先调用 [`init`]：
//!
//! - 把 tracing 日志同时写到 `<数据目录>/chordial/logs/chordial.log`
//!   （单文件 8 MiB，超出后滚动为 `chordial.1.log`）；
//! - 安装 panic hook：任意线程 panic 都写一份 `crash-*.log`，
//!   内含 panic 信息、位置、栈、当天日志尾部；
//! - Unix 安装 SIGSEGV/SIGABRT 等致命信号处理，Windows 安装
//!   unhandled exception filter，原生崩溃也尽量留一份 `fatal-*.log`；
//! - panic（以及 Windows 原生异常）弹系统对话框告知报告位置
//!   （`CHORDIAL_CRASH_DIALOG=0` 可关闭，调试器在场时不弹）。
//!
//! 最近一次崩溃固定复制为 `crash-latest.log`，用户反馈时只需附上它
//! 和 `chordial.log`。报告里的 `MUSIC_U` / `__csrf`（登录 Cookie）
//! 会自动脱敏。

use std::backtrace::Backtrace;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// 单个日志文件的滚动阈值。当前文件 + 上一份最多约 16 MiB。
const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;
/// panic 报告带上的日志尾部上限。
const LOG_TAIL_BYTES: u64 = 256 * 1024;
/// 上一次运行的日志尾部上限（崩溃发生在启动早期时提供上下文）。
const PREV_LOG_TAIL_BYTES: u64 = 64 * 1024;
/// 日志尾部最多保留的行数。
const LOG_TAIL_LINES: usize = 300;
/// 最多保留多少份崩溃报告。
const CRASH_KEEP: usize = 20;

const LOG_NAME: &str = "chordial.log";
const PREV_LOG_NAME: &str = "chordial.1.log";
const LATEST_CRASH_NAME: &str = "crash-latest.log";

static INIT: OnceLock<()> = OnceLock::new();
static CONTEXT: OnceLock<CrashContext> = OnceLock::new();
static SINK: OnceLock<Mutex<LogSink>> = OnceLock::new();
static REPORT_SEQ: AtomicU64 = AtomicU64::new(0);
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static IN_PANIC_HOOK: AtomicBool = AtomicBool::new(false);
static DIALOG_SHOWN: AtomicBool = AtomicBool::new(false);

/// 日志与崩溃报告所在目录。
///
/// `CHORDIAL_LOG_DIR` 可覆盖（便携版/测试用）。默认跟随平台数据目录：
/// Windows `%LOCALAPPDATA%\chordial\logs`，Linux
/// `~/.local/share/chordial/logs`，macOS
/// `~/Library/Application Support/chordial/logs`。
pub fn log_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CHORDIAL_LOG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("chordial")
        .join("logs")
}

/// 安装文件日志、panic hook 与原生崩溃处理。重复调用无副作用。
pub fn init() {
    if INIT.set(()).is_err() {
        return;
    }

    let ctx = CrashContext::new(log_dir());
    let _ = fs::create_dir_all(&ctx.log_dir);
    let _ = CONTEXT.set(ctx);

    // 先装 hook，后面初始化过程中出问题也能留下报告。
    install_panic_hook();

    let Some(ctx) = CONTEXT.get() else {
        return;
    };

    // 原生崩溃处理只依赖路径，尽早装上。
    #[cfg(unix)]
    fatal::install(ctx);
    #[cfg(windows)]
    fatal::install(ctx);

    // 每次启动开一份新日志，上一会话保留为 chordial.1.log。
    rotate_previous_log(ctx);

    match LogSink::open(ctx) {
        Ok(sink) => {
            let _ = SINK.set(Mutex::new(sink));
        }
        Err(e) => {
            let _ = writeln!(
                io::stderr().lock(),
                "chordial: 无法创建日志文件 {}: {e}",
                ctx.log_path.display()
            );
        }
    }

    install_tracing();

    let _ = prune_reports(&ctx.log_dir, CRASH_KEEP);

    if ctx.log_dir.join(LATEST_CRASH_NAME).is_file() {
        tracing::warn!(
            report = %ctx.log_dir.join(LATEST_CRASH_NAME).display(),
            "上次运行留下了崩溃报告；如仍存在问题，请随反馈附上该文件"
        );
    }
    tracing::info!(
        log = %ctx.log_path.display(),
        "file logging enabled"
    );
}

#[derive(Debug)]
struct CrashContext {
    log_dir: PathBuf,
    log_path: PathBuf,
    prev_log_path: PathBuf,
}

impl CrashContext {
    fn new(log_dir: PathBuf) -> Self {
        Self {
            log_path: log_dir.join(LOG_NAME),
            prev_log_path: log_dir.join(PREV_LOG_NAME),
            log_dir,
        }
    }
}

// ---------------------------------------------------------------------------
// tracing：stdout + 文件
// ---------------------------------------------------------------------------

fn install_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,chordial=debug"));

    // 文件层只在日志文件打开成功时挂上；失败时仅保留 stdout。
    let file_layer = SINK.get().map(|_| {
        fmt::layer()
            .with_writer(SinkWriter)
            .with_ansi(false)
            .with_target(true)
            .with_thread_names(true)
    });

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .with(file_layer)
        .try_init();
    if result.is_err() {
        let _ = writeln!(
            io::stderr().lock(),
            "chordial: tracing subscriber 已初始化，跳过重复安装"
        );
    }
}

struct LogSink {
    path: PathBuf,
    prev_path: PathBuf,
    file: Option<File>,
    written: u64,
    max_bytes: u64,
    disabled: bool,
}

impl LogSink {
    fn open(ctx: &CrashContext) -> io::Result<Self> {
        Self::open_with_limit(ctx, MAX_LOG_BYTES)
    }

    fn open_with_limit(ctx: &CrashContext, max_bytes: u64) -> io::Result<Self> {
        fs::create_dir_all(&ctx.log_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&ctx.log_path)?;
        let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Ok(Self {
            path: ctx.log_path.clone(),
            prev_path: ctx.prev_log_path.clone(),
            file: Some(file),
            written,
            max_bytes,
            disabled: false,
        })
    }

    /// 写日志。失败不向上抛：日志层本身不能拖垮应用。
    fn write(&mut self, buf: &[u8]) {
        if self.disabled {
            return;
        }
        if self.written.saturating_add(buf.len() as u64) > self.max_bytes {
            self.rotate();
        }
        let Some(file) = self.file.as_mut() else {
            self.disabled = true;
            return;
        };
        if file.write_all(buf).is_err() || file.flush().is_err() {
            self.disabled = true;
            return;
        }
        self.written = self.written.saturating_add(buf.len() as u64);
    }

    fn rotate(&mut self) {
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
        }
        self.file = None;
        // Windows 的 rename 不能覆盖已存在目标，先删旧文件。
        let _ = fs::remove_file(&self.prev_path);
        // rename 失败（例如被杀毒软件占用）时直接清空当前文件，
        // 宁可丢上一份日志，也不能让当前文件无限增长。
        if fs::rename(&self.path, &self.prev_path).is_err() {
            let _ = fs::remove_file(&self.path);
        }
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(file) => {
                self.file = Some(file);
                self.written = 0;
            }
            Err(_) => self.disabled = true,
        }
    }

    fn flush(&mut self) {
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
        }
    }
}

#[derive(Clone, Copy)]
struct SinkWriter;

struct SinkGuard;

impl<'a> fmt::MakeWriter<'a> for SinkWriter {
    type Writer = SinkGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SinkGuard
    }
}

impl io::Write for SinkGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(sink) = SINK.get() {
            let mut sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            sink.write(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(sink) = SINK.get() {
            let mut sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            sink.flush();
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// panic hook
// ---------------------------------------------------------------------------

fn install_panic_hook() {
    if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // panic 发生在日志/报告代码里时避免递归。
        if IN_PANIC_HOOK.swap(true, Ordering::SeqCst) {
            previous(info);
            return;
        }

        if let Some(ctx) = CONTEXT.get() {
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<非字符串 panic 载荷>".to_string());
            let location = info
                .location()
                .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()));
            let thread = std::thread::current();
            let thread_name = thread.name().unwrap_or("<unnamed>");
            let backtrace = Backtrace::force_capture().to_string();

            match write_panic_report(ctx, &message, location.as_deref(), thread_name, &backtrace) {
                Ok(path) => {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "\nChordial 发生崩溃，报告已写入：\n  {}\n完整日志：{}\n请将这两个文件附在反馈中。",
                        path.display(),
                        ctx.log_path.display()
                    );
                    // 先走默认 hook 打印真实 panic 位置，再弹模态对话框：
                    // 对话框会泵消息，可能立刻触发的后台任务 panic 会打在这
                    // 两条输出前面，让人误以为是原始崩溃点。
                    previous(info);
                    show_crash_dialog(&format!(
                        "Chordial 遇到问题并已停止。\n\n崩溃报告已保存到：\n{}\n\n完整日志：\n{}\n\n请将这两个文件附在问题反馈中。",
                        path.display(),
                        ctx.log_path.display()
                    ));
                    IN_PANIC_HOOK.store(false, Ordering::SeqCst);
                    return;
                }
                Err(e) => {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "\nChordial 发生崩溃，但崩溃报告写入失败：{e}\n线程：{thread_name}\n位置：{}\npanic：{message}\n\n{backtrace}",
                        location.as_deref().unwrap_or("<unknown>")
                    );
                    previous(info);
                    show_crash_dialog(&format!(
                        "Chordial 遇到问题并已停止。\n\n崩溃报告写入失败：{e}\n\npanic：{message}\n位置：{}\n日志目录：{}",
                        location.as_deref().unwrap_or("<unknown>"),
                        ctx.log_dir.display()
                    ));
                    IN_PANIC_HOOK.store(false, Ordering::SeqCst);
                    return;
                }
            }
        }

        IN_PANIC_HOOK.store(false, Ordering::SeqCst);
        previous(info);
    }));
}

fn write_panic_report(
    ctx: &CrashContext,
    message: &str,
    location: Option<&str>,
    thread: &str,
    backtrace: &str,
) -> io::Result<PathBuf> {
    let report = build_panic_report(ctx, message, location, thread, backtrace);
    write_report_file(&ctx.log_dir, "crash", &report)
}

// ---------------------------------------------------------------------------
// 崩溃弹窗
// ---------------------------------------------------------------------------

/// 默认弹窗；`CHORDIAL_CRASH_DIALOG=0` 可关闭（CI / 自动化脚本）。
fn dialog_enabled() -> bool {
    !matches!(
        std::env::var("CHORDIAL_CRASH_DIALOG").as_deref(),
        Ok("0") | Ok("false") | Ok("off")
    )
}

/// 每次进程只弹一次，避免多线程连续 panic 叠出一堆窗口。
fn show_crash_dialog(body: &str) {
    if !dialog_enabled() || DIALOG_SHOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    let body = redact_secrets(body);

    #[cfg(windows)]
    windows_dialog(&body);

    #[cfg(unix)]
    unix_dialog(&body);

    #[cfg(not(any(unix, windows)))]
    let _ = body;
}

#[cfg(windows)]
fn windows_dialog(body: &str) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Diagnostics::Debug::IsDebuggerPresent;
    use windows::Win32::UI::WindowsAndMessaging::{
        MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW,
    };
    use windows::core::HSTRING;

    // 调试器在场时不要用模态框打断断点。
    if unsafe { IsDebuggerPresent() }.as_bool() {
        return;
    }

    let text = HSTRING::from(body);
    let caption = HSTRING::from("Chordial 崩溃");
    unsafe {
        MessageBoxW(
            HWND::default(),
            &text,
            &caption,
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

#[cfg(unix)]
fn unix_dialog(body: &str) {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        // 参数经 argv 传给 AppleScript，避免拼字符串注入。
        let script = "on run argv\n\
            display dialog (item 1 of argv) with title (item 2 of argv) \
            buttons {\"好\"} default button 1 with icon caution\n\
            end run";
        let _ = Command::new("osascript")
            .args(["-e", script, "--", body, "Chordial 崩溃"])
            .status();
    }

    #[cfg(not(target_os = "macos"))]
    {
        use std::process::Command;

        // 没有图形会话（SSH / CI）就不尝试拉起对话框。
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }

        fn run(program: &str, args: &[&str], body: &str) -> bool {
            Command::new(program).args(args).arg(body).status().is_ok()
        }

        let _ = run(
            "zenity",
            &["--error", "--title", "Chordial 崩溃", "--text"],
            body,
        ) || run("kdialog", &["--title", "Chordial 崩溃", "--error"], body)
            || run("xmessage", &["-center", "-title", "Chordial 崩溃"], body);
    }
}

fn build_panic_report(
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

fn append_log_tail(out: &mut String, ctx: &CrashContext) {
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

fn write_report_file(dir: &Path, prefix: &str, content: &str) -> io::Result<PathBuf> {
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
fn prune_reports(dir: &Path, keep: usize) -> usize {
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

fn rotate_previous_log(ctx: &CrashContext) {
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

fn read_tail(path: &Path, max_bytes: u64, max_lines: usize) -> Option<String> {
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
    let text = String::from_utf8_lossy(slice);
    let lines: Vec<&str> = text.lines().collect();
    let start_line = lines.len().saturating_sub(max_lines);
    Some(lines[start_line..].join("\n"))
}

/// 崩溃报告会被用户公开上传，去掉登录态等敏感值。
fn redact_secrets(input: &str) -> String {
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

fn utc_timestamp() -> String {
    format_utc(now_unix_secs(), false)
}

fn utc_compact() -> String {
    format_utc(now_unix_secs(), true)
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn format_utc(secs: u64, compact: bool) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    if compact {
        format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}Z")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    }
}

/// Howard Hinnant 的 civil_from_days：把 Unix 天数转成公历日期。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

// ---------------------------------------------------------------------------
// 原生崩溃：Unix
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod fatal {
    use super::*;
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int};

    const SIGNALS: [c_int; 5] = [
        libc::SIGSEGV,
        libc::SIGABRT,
        libc::SIGBUS,
        libc::SIGILL,
        libc::SIGFPE,
    ];
    /// 信号处理里只使用栈上缓冲区和 async-signal-safe 的系统调用。
    const TAIL_BYTES: usize = 4096;

    struct Paths {
        log_file: CString,
        log_dir: CString,
        latest: CString,
    }

    static PATHS: OnceLock<Paths> = OnceLock::new();
    static IN_HANDLER: AtomicBool = AtomicBool::new(false);

    pub(super) fn install(ctx: &CrashContext) {
        fn cstring(path: &Path) -> Option<CString> {
            CString::new(path.to_string_lossy().as_bytes()).ok()
        }
        if let (Some(log_file), Some(log_dir), Some(latest)) = (
            cstring(&ctx.log_path),
            cstring(&ctx.log_dir),
            cstring(&ctx.log_dir.join(LATEST_CRASH_NAME)),
        ) {
            let _ = PATHS.set(Paths {
                log_file,
                log_dir,
                latest,
            });
        }

        // SA_RESETHAND：处理一次后恢复默认动作，便于系统生成 core dump。
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = handler as *const () as libc::sighandler_t;
        action.sa_flags = libc::SA_RESETHAND;
        unsafe {
            libc::sigemptyset(&mut action.sa_mask);
            for signal in SIGNALS {
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
    }

    extern "C" fn handler(signal: c_int) {
        if !IN_HANDLER.swap(true, Ordering::SeqCst)
            && let Some(paths) = PATHS.get()
        {
            // SAFETY: 只调用信号安全的系统调用，且不使用堆分配。
            unsafe {
                write_report(paths, signal);
            }
        }
        // 信号在 handler 内被阻塞；SA_RESETHAND 已恢复默认动作，
        // handler 返回后终止（并保留 core dump 行为）。
        unsafe {
            libc::raise(signal);
        }
    }

    unsafe fn write_report(paths: &Paths, signal: c_int) {
        let mut path = [0u8; 4096];
        let mut path_len = 0;
        push_bytes(&mut path, &mut path_len, paths.log_dir.as_bytes());
        push_bytes(&mut path, &mut path_len, b"/fatal-");
        push_u64(&mut path, &mut path_len, epoch_secs());
        push_bytes(&mut path, &mut path_len, b"-");
        push_u64(&mut path, &mut path_len, unsafe { libc::getpid() } as u64);
        push_bytes(&mut path, &mut path_len, b".log\0");
        if path_len >= path.len() {
            return;
        }

        let mut content = [0u8; 8192];
        let mut len = 0;
        push_bytes(
            &mut content,
            &mut len,
            "Chordial 崩溃报告（原生信号）\n".as_bytes(),
        );
        push_bytes(
            &mut content,
            &mut len,
            "请把本文件（以及同目录的 chordial.log）附在问题反馈中。\n\n".as_bytes(),
        );
        push_bytes(&mut content, &mut len, b"version   : ");
        push_bytes(&mut content, &mut len, env!("CARGO_PKG_VERSION").as_bytes());
        push_bytes(&mut content, &mut len, b"\nos        : ");
        push_bytes(&mut content, &mut len, std::env::consts::OS.as_bytes());
        push_bytes(&mut content, &mut len, b" ");
        push_bytes(&mut content, &mut len, std::env::consts::ARCH.as_bytes());
        push_bytes(&mut content, &mut len, b"\nsignal    : ");
        push_signal_name(&mut content, &mut len, signal);
        push_bytes(&mut content, &mut len, b"\nlog       : ");
        push_bytes(&mut content, &mut len, paths.log_file.as_bytes());
        push_bytes(&mut content, &mut len, b"\n\n----- recent log tail -----\n");

        let mut tail = [0u8; TAIL_BYTES];
        let tail_len =
            unsafe { read_tail_fd(paths.log_file.as_ptr(), tail.as_mut_ptr(), tail.len()) };
        if tail_len > 0 {
            push_bytes(&mut content, &mut len, &tail[..tail_len]);
        }

        unsafe {
            let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC;
            let mode = 0o644 as libc::mode_t;
            let fd = libc::open(path.as_ptr() as *const c_char, flags, mode);
            if fd >= 0 {
                libc::write(fd, content.as_ptr() as *const libc::c_void, len);
                libc::close(fd);

                let latest_fd = libc::open(paths.latest.as_ptr(), flags, mode);
                if latest_fd >= 0 {
                    libc::write(latest_fd, content.as_ptr() as *const libc::c_void, len);
                    libc::close(latest_fd);
                }

                let prefix = b"\nChordial crashed. Crash report: ";
                libc::write(2, prefix.as_ptr() as *const libc::c_void, prefix.len());
                libc::write(
                    2,
                    path.as_ptr() as *const libc::c_void,
                    path_len.saturating_sub(1),
                );
                libc::write(2, b"\n".as_ptr() as *const libc::c_void, 1);
            }
        }
    }

    unsafe fn read_tail_fd(path: *const c_char, out: *mut u8, cap: usize) -> usize {
        unsafe {
            let fd = libc::open(path, libc::O_RDONLY);
            if fd < 0 {
                return 0;
            }
            let end = libc::lseek(fd, 0, libc::SEEK_END);
            let mut len = 0usize;
            if end > 0 {
                let start = if end as usize > cap {
                    end - cap as i64
                } else {
                    0
                };
                if libc::lseek(fd, start, libc::SEEK_SET) >= 0 {
                    let read = libc::read(fd, out as *mut libc::c_void, cap);
                    if read > 0 {
                        len = read as usize;
                    }
                }
                // 文件是从中间截取的：丢掉第一行残片。
                if start > 0 && len > 0 {
                    let bytes = std::slice::from_raw_parts(out, len);
                    match bytes.iter().position(|byte| *byte == b'\n') {
                        Some(pos) if pos + 1 < len => {
                            std::ptr::copy(out.add(pos + 1), out, len - pos - 1);
                            len -= pos + 1;
                        }
                        _ => len = 0,
                    }
                }
            }
            libc::close(fd);
            len
        }
    }

    fn epoch_secs() -> u64 {
        let secs = unsafe { libc::time(std::ptr::null_mut()) };
        if secs > 0 { secs as u64 } else { 0 }
    }

    fn push_bytes(buf: &mut [u8], len: &mut usize, bytes: &[u8]) {
        let space = buf.len().saturating_sub(*len);
        let count = bytes.len().min(space);
        buf[*len..*len + count].copy_from_slice(&bytes[..count]);
        *len += count;
    }

    fn push_u64(buf: &mut [u8], len: &mut usize, mut value: u64) {
        let mut digits = [0u8; 20];
        let mut index = digits.len();
        loop {
            index -= 1;
            digits[index] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        push_bytes(buf, len, &digits[index..]);
    }

    fn push_signal_name(buf: &mut [u8], len: &mut usize, signal: c_int) {
        let name = match signal {
            libc::SIGSEGV => "SIGSEGV",
            libc::SIGABRT => "SIGABRT",
            libc::SIGBUS => "SIGBUS",
            libc::SIGILL => "SIGILL",
            libc::SIGFPE => "SIGFPE",
            _ => "SIGNAL",
        };
        push_bytes(buf, len, name.as_bytes());
        push_bytes(buf, len, b" (");
        push_u64(buf, len, signal as u64);
        push_bytes(buf, len, b")");
    }
}

// ---------------------------------------------------------------------------
// 原生崩溃：Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod fatal {
    use super::*;
    use windows::Win32::System::Diagnostics::Debug::{
        EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
    };

    static IN_HANDLER: AtomicBool = AtomicBool::new(false);

    pub(super) fn install(_ctx: &CrashContext) {
        // SAFETY: 回调只写文件后返回 EXCEPTION_CONTINUE_SEARCH，
        // 交给系统原有的崩溃处理链路继续。
        unsafe {
            SetUnhandledExceptionFilter(Some(filter));
        }
    }

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        if IN_HANDLER.swap(true, Ordering::SeqCst) {
            return EXCEPTION_CONTINUE_SEARCH;
        }

        if let Some(ctx) = CONTEXT.get() {
            let (code, address) = unsafe {
                if info.is_null() || (*info).ExceptionRecord.is_null() {
                    (0i32, 0usize)
                } else {
                    let record = &*(*info).ExceptionRecord;
                    (record.ExceptionCode.0, record.ExceptionAddress as usize)
                }
            };

            let mut out = String::with_capacity(8192);
            out.push_str("Chordial 崩溃报告（原生异常）\n");
            out.push_str("请把本文件（以及同目录的 chordial.log）附在问题反馈中。\n\n");
            out.push_str(&format!("version   : {}\n", env!("CARGO_PKG_VERSION")));
            out.push_str(&format!(
                "os        : {} {}\n",
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
            out.push_str(&format!("time      : {}\n", utc_timestamp()));
            out.push_str(&format!("pid       : {}\n", std::process::id()));
            out.push_str(&format!("exception : 0x{code:08X}\n"));
            out.push_str(&format!("address   : 0x{address:016X}\n"));
            out.push_str(&format!("log       : {}\n", ctx.log_path.display()));
            append_log_tail(&mut out, ctx);
            let report = redact_secrets(&out);

            match write_report_file(&ctx.log_dir, "fatal", &report) {
                Ok(path) => {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "\nChordial 发生原生崩溃，报告已写入：\n  {}\n完整日志：{}\n请将这两个文件附在反馈中。",
                        path.display(),
                        ctx.log_path.display()
                    );
                    show_crash_dialog(&format!(
                        "Chordial 遇到原生崩溃并已停止。\n\n崩溃报告已保存到：\n{}\n\n完整日志：\n{}\n\n请将这两个文件附在问题反馈中。",
                        path.display(),
                        ctx.log_path.display()
                    ));
                }
                Err(e) => {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "\nChordial 发生原生崩溃，但报告写入失败：{e}"
                    );
                    show_crash_dialog(&format!(
                        "Chordial 遇到原生崩溃并已停止。\n\n崩溃报告写入失败：{e}\n\n日志目录：{}",
                        ctx.log_dir.display()
                    ));
                }
            }
        }

        IN_HANDLER.store(false, Ordering::SeqCst);
        EXCEPTION_CONTINUE_SEARCH
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("chordial-crash-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn utc_format_matches_known_instants() {
        assert_eq!(format_utc(0, false), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(0, true), "19700101-000000Z");
        assert_eq!(format_utc(1_700_000_000, false), "2023-11-14T22:13:20Z");
        assert_eq!(format_utc(1_700_000_000, true), "20231114-221320Z");
    }

    #[test]
    fn redacts_cookie_values() {
        let input = "cookie: MUSIC_U=abc123; __csrf=xyz; other=1";
        let output = redact_secrets(input);
        assert!(!output.contains("abc123"));
        assert!(!output.contains("xyz"));
        assert!(output.contains("other=1"));
        assert_eq!(output.matches("<redacted>").count(), 2);
    }

    #[test]
    fn panic_report_contains_details_and_log_tail() {
        let dir = test_dir("panic-report");
        let ctx = CrashContext::new(dir.clone());
        fs::write(
            &ctx.log_path,
            "line-1\nline-2 secret MUSIC_U=should-not-leak\nline-3 tail-marker\n",
        )
        .expect("write log");

        let path = write_panic_report(
            &ctx,
            "boom message",
            Some("crates/x.rs:12:34"),
            "main",
            "frame-a\nframe-b",
        )
        .expect("write report");

        let report = fs::read_to_string(&path).expect("read report");
        assert!(report.contains("boom message"));
        assert!(report.contains("crates/x.rs:12:34"));
        assert!(report.contains("frame-a"));
        assert!(report.contains("tail-marker"));
        assert!(report.contains("thread    : main"));
        assert!(!report.contains("should-not-leak"));
        assert!(dir.join(LATEST_CRASH_NAME).is_file());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn log_sink_rotates_to_previous_file() {
        let dir = test_dir("sink-rotate");
        let ctx = CrashContext::new(dir.clone());
        let mut sink = LogSink::open_with_limit(&ctx, 8).expect("open sink");

        sink.write(b"aaaaa");
        sink.write(b"bbbbb");

        assert_eq!(
            fs::read_to_string(&ctx.prev_log_path).expect("read prev"),
            "aaaaa"
        );
        assert_eq!(
            fs::read_to_string(&ctx.log_path).expect("read current"),
            "bbbbb"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn prune_keeps_newest_reports_and_latest() {
        let dir = test_dir("prune");
        for index in 0..25 {
            fs::write(
                dir.join(format!("crash-20260101-0000{index:02}-1-1.log")),
                "x",
            )
            .expect("write report");
        }
        fs::write(dir.join(LATEST_CRASH_NAME), "x").expect("write latest");

        let removed = prune_reports(&dir, 20);
        assert_eq!(removed, 5);
        assert!(dir.join(LATEST_CRASH_NAME).is_file());
        assert!(!dir.join("crash-20260101-000000-1-1.log").exists());
        assert!(dir.join("crash-20260101-000024-1-1.log").is_file());

        let _ = fs::remove_dir_all(dir);
    }
}
