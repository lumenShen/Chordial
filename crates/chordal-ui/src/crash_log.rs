//! 崩溃兜底与日志落盘。
//!
//! 启动时最先调用 [`init`]：
//!
//! - 把 tracing 日志同时写到 `<数据目录>/chordial/logs/chordial.log`
//!   （单文件 8 MiB，超出后滚动为 `chordial.1.log`）；
//! - 同一调用点的重复日志按秒限流，且文件落盘另有每秒字节预算：
//!   底层 API 批量失败时（例如 Windows 配额耗尽，GPUI 每秒记录上万条
//!   相同错误），日志本身不会写满磁盘；
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
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::{Context, Filter, Layer as _, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;

mod panic_hook;
mod report;
mod time_util;

use panic_hook::*;
use report::*;
use time_util::*;

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

/// 同一调用点在窗口内最多写出的日志条数。
///
/// Windows 资源耗尽时，底层 API（消息队列/线程池/GDI 配额）会以每秒数万次
/// 的频率失败，GPUI 对每次失败都走 `log_err()`。此前这些记录会原样写进
/// stdout 与日志文件：实测 10 分钟写出 4.2 GiB，既占满磁盘又让 UI 线程
/// 把时间耗在 `FormatMessageW`/格式化/落盘上，进一步加剧资源耗尽。
/// 限流直接丢弃重复调用点的事件，且不触发消息格式化。
const LOG_BURST_PER_CALLSITE: u32 = 4;
const LOG_BURST_WINDOW: Duration = Duration::from_secs(1);

/// 限流表上限。程序里的调用点数量有限；超限时先清掉过期窗口。
const LOG_GUARD_MAX_CALLSITES: usize = 1024;

/// 日志落盘的全局字节预算：每秒最多写入的字节数。
///
/// 单点限流只能挡住"同一行代码反复报错"。若一次故障横跨大量不同调用点
/// （GPUI 的 Win32 封装、线程池、D3D 各有各的失败点），各点各自合规仍可能
/// 叠加成每秒几十 MB。这里在落盘唯一入口再设一道总量闸门：正常调试日志
/// 每秒只有几十 KB，1 MiB/s 留了足够余量；风暴则被截断为每秒最多 1 MiB，
/// 并在预算恢复时补一行丢弃统计，不会静默吞掉。
const LOG_BYTE_BUDGET_PER_SEC: u64 = 1024 * 1024;
const LOG_BYTE_BUDGET_WINDOW: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
struct Burst {
    window_start: Instant,
    emitted: u32,
}

/// 按调用点（level + target + file + line）限流的 tracing 过滤器。
///
/// 键只依赖事件元数据，不读取字段、不格式化消息：即使每秒有上万个
/// 相同失败，被抑制的事件也不会付出格式化成本。每个输出层各持一份状态，
/// 因此 stdout 与文件各自保留每个调用点的前几条记录。
#[derive(Clone)]
struct LogStormGuard {
    bursts: Arc<Mutex<HashMap<u64, Burst>>>,
}

impl LogStormGuard {
    fn new() -> Self {
        Self {
            bursts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn allow_at(&self, key: u64, now: Instant) -> bool {
        let mut bursts = self.bursts.lock().unwrap_or_else(|e| e.into_inner());
        if bursts.len() > LOG_GUARD_MAX_CALLSITES {
            bursts.retain(|_, burst| now.duration_since(burst.window_start) < LOG_BURST_WINDOW);
            if bursts.len() > LOG_GUARD_MAX_CALLSITES {
                bursts.clear();
            }
        }
        match bursts.get_mut(&key) {
            Some(burst) if now.duration_since(burst.window_start) < LOG_BURST_WINDOW => {
                if burst.emitted >= LOG_BURST_PER_CALLSITE {
                    return false;
                }
                burst.emitted += 1;
                true
            }
            _ => {
                bursts.insert(
                    key,
                    Burst {
                        window_start: now,
                        emitted: 1,
                    },
                );
                true
            }
        }
    }
}

fn event_key(meta: &Metadata<'_>) -> u64 {
    let mut hasher = DefaultHasher::new();
    meta.level().as_str().hash(&mut hasher);
    meta.target().hash(&mut hasher);
    meta.file().hash(&mut hasher);
    meta.line().hash(&mut hasher);
    hasher.finish()
}

impl<S: Subscriber> Filter<S> for LogStormGuard {
    fn enabled(&self, _meta: &Metadata<'_>, _cx: &Context<'_, S>) -> bool {
        true
    }

    fn event_enabled(&self, event: &Event<'_>, _cx: &Context<'_, S>) -> bool {
        self.allow_at(event_key(event.metadata()), Instant::now())
    }
}

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
            .with_filter(LogStormGuard::new())
    });

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true).with_filter(LogStormGuard::new()))
        .with(file_layer)
        .try_init();
    if result.is_err() {
        let _ = writeln!(
            io::stderr().lock(),
            "chordial: tracing subscriber 已初始化，跳过重复安装"
        );
    }
}

/// 日志写入的字节速率限制器：每个窗口重新计数。
///
/// 与 [`LogStormGuard`] 互补——那个按调用点去重，这个管总量。
#[derive(Clone, Copy)]
struct ByteBudget {
    window_start: Instant,
    window_bytes: u64,
    dropped_bytes: u64,
}

impl ByteBudget {
    fn new(now: Instant) -> Self {
        Self {
            window_start: now,
            window_bytes: 0,
            dropped_bytes: 0,
        }
    }

    /// 窗口到期则清零计数，返回上一窗口被丢弃的字节数（用于补一行统计）。
    fn roll_at(&mut self, now: Instant) -> Option<u64> {
        if now.duration_since(self.window_start) < LOG_BYTE_BUDGET_WINDOW {
            return None;
        }
        self.window_start = now;
        self.window_bytes = 0;
        let dropped = std::mem::take(&mut self.dropped_bytes);
        (dropped > 0).then_some(dropped)
    }

    /// 预算内则记账放行；超预算只累计丢弃量。
    fn try_take(&mut self, len: u64) -> bool {
        if self.window_bytes.saturating_add(len) > LOG_BYTE_BUDGET_PER_SEC {
            self.dropped_bytes = self.dropped_bytes.saturating_add(len);
            return false;
        }
        self.window_bytes += len;
        true
    }
}

struct LogSink {
    path: PathBuf,
    prev_path: PathBuf,
    file: Option<File>,
    written: u64,
    max_bytes: u64,
    disabled: bool,
    budget: ByteBudget,
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
            budget: ByteBudget::new(Instant::now()),
        })
    }

    /// 写日志。失败不向上抛：日志层本身不能拖垮应用。
    fn write(&mut self, buf: &[u8]) {
        if self.disabled {
            return;
        }
        let now = Instant::now();
        if let Some(dropped) = self.budget.roll_at(now) {
            // 预算恢复时补一行统计：限流不能静默。
            let marker = format!(
                "[日志限流] 上一窗口丢弃 {dropped} 字节日志（上限 {LOG_BYTE_BUDGET_PER_SEC} B/s）\n"
            );
            self.write_raw(marker.as_bytes());
        }
        if !self.budget.try_take(buf.len() as u64) {
            return;
        }
        self.write_raw(buf);
    }

    fn write_raw(&mut self, buf: &[u8]) {
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
pub(super) struct SinkWriter;

pub(super) struct SinkGuard;

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
mod tests;
