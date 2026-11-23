//! 崩溃日志：panic_hook（由 crash_log.rs 拆分，行为不变）。

use super::*;
use super::report::*;

pub(super) fn install_panic_hook() {
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

pub(super) fn write_panic_report(
    ctx: &CrashContext,
    message: &str,
    location: Option<&str>,
    thread: &str,
    backtrace: &str,
) -> io::Result<PathBuf> {
    let report = build_panic_report(ctx, message, location, thread, backtrace);
    write_report_file(&ctx.log_dir, "crash", &report)
}

// 崩溃弹窗
// ---------------------------------------------------------------------------

/// 默认弹窗；`CHORDIAL_CRASH_DIALOG=0` 可关闭（CI / 自动化脚本）。
pub(super) fn dialog_enabled() -> bool {
    !matches!(
        std::env::var("CHORDIAL_CRASH_DIALOG").as_deref(),
        Ok("0") | Ok("false") | Ok("off")
    )
}

/// 每次进程只弹一次，避免多线程连续 panic 叠出一堆窗口。
pub(super) fn show_crash_dialog(body: &str) {
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
pub(super) fn windows_dialog(body: &str) {
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
pub(super) fn unix_dialog(body: &str) {
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

