//! 崩溃日志测试（由 crash_log.rs 拆分，行为不变）。

use super::*;

fn test_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("chordial-crash-test-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test dir");
    dir
}

#[test]
fn repeated_log_from_one_callsite_is_rate_limited() {
    let guard = LogStormGuard::new();
    let start = Instant::now();
    // 同一调用点在 1s 内只放行前几条，其余丢弃；风暴不会落在磁盘上。
    for i in 0..50u64 {
        assert_eq!(
            guard.allow_at(7, start + Duration::from_millis(i)),
            i < LOG_BURST_PER_CALLSITE as u64,
            "第 {i} 条重复日志的放行结果不符合限流窗口"
        );
    }
    // 窗口结束后恢复放行。
    assert!(guard.allow_at(7, start + LOG_BURST_WINDOW));
    // 不同调用点互不影响。
    assert!(guard.allow_at(8, start + Duration::from_millis(1)));
}

#[test]
fn guard_table_does_not_grow_without_bound() {
    let guard = LogStormGuard::new();
    let start = Instant::now();
    for key in 0..(LOG_GUARD_MAX_CALLSITES as u64 * 4) {
        guard.allow_at(key, start + Duration::from_millis(key % 10));
    }
    let entries = guard
        .bursts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .len();
    assert!(
        entries <= LOG_GUARD_MAX_CALLSITES,
        "callsite table grew to {entries}"
    );
}

#[derive(Clone)]
struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn repeated_events_are_dropped_before_hitting_the_writer() {
    let output = Arc::new(Mutex::new(Vec::new()));
    let writer = CaptureWriter(output.clone());
    let subscriber = tracing_subscriber::registry().with(
        fmt::layer()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .with_filter(LogStormGuard::new()),
    );
    tracing::subscriber::with_default(subscriber, || {
        for _ in 0..64 {
            tracing::error!(target: "chordial::test_storm", "同一个失败重复记录");
        }
    });
    let text = String::from_utf8(
        output
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
    )
    .expect("utf8 log output");
    let lines = text.lines().count();
    assert!(lines >= 1, "第一条日志必须保留");
    assert!(
        lines <= LOG_BURST_PER_CALLSITE as usize,
        "重复日志未被限流，写出 {lines} 行：{text}"
    );
}

#[test]
fn byte_budget_caps_writes_per_window() {
    let start = Instant::now();
    let mut budget = ByteBudget::new(start);
    // 恰好用满预算的写入全部放行，多出 1 字节即丢弃。
    assert!(budget.try_take(LOG_BYTE_BUDGET_PER_SEC - 1));
    assert!(budget.try_take(1));
    assert!(!budget.try_take(1));
    assert_eq!(budget.dropped_bytes, 1);
    // 窗口未到期不清零。
    assert_eq!(budget.roll_at(start + Duration::from_millis(500)), None);
    assert!(!budget.try_take(1));
    // 到期后恢复，并报出上一窗口的丢弃量。
    assert_eq!(budget.roll_at(start + LOG_BYTE_BUDGET_WINDOW), Some(2));
    assert!(budget.try_take(LOG_BYTE_BUDGET_PER_SEC));
}

#[test]
fn byte_budget_reports_nothing_after_clean_window() {
    let start = Instant::now();
    let mut budget = ByteBudget::new(start);
    assert!(budget.try_take(64));
    assert_eq!(
        budget.roll_at(start + LOG_BYTE_BUDGET_WINDOW - Duration::from_millis(1)),
        None
    );
    assert_eq!(budget.roll_at(start + LOG_BYTE_BUDGET_WINDOW), None);
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
fn log_sink_stops_at_byte_budget() {
    let dir = test_dir("sink-budget");
    let ctx = CrashContext::new(dir.clone());
    let mut sink = LogSink::open_with_limit(&ctx, 8 * 1024 * 1024).expect("open sink");

    // 尝试写入 16 MiB，远超每秒预算。
    let chunk = vec![b'x'; 256 * 1024];
    for _ in 0..64 {
        sink.write(&chunk);
    }
    sink.flush();

    let written = fs::metadata(&ctx.log_path).expect("stat log").len();
    assert!(
        written >= LOG_BYTE_BUDGET_PER_SEC,
        "预算内的日志不应被丢弃：只写入 {written} 字节"
    );
    // 允许跨过一次窗口边界（最多多放行一个预算）。
    assert!(
        written <= 2 * LOG_BYTE_BUDGET_PER_SEC,
        "日志落盘未被字节预算截断：{written} 字节"
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
