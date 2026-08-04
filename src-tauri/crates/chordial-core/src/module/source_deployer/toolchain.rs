//! 工具链检测：git / node / npm 是否可用及版本。

use super::types::ToolchainCheck;
use crate::module::perf;
use std::process::Stdio;

impl super::SourceDeployer {
    /// 同步检测 git / node / npm 是否可用（同步阻塞，启动时调用）。
    pub fn check_toolchain(&self) -> ToolchainCheck {
        let _scope = perf::scope("source_deployer.check_toolchain");
        let git_out = run_version_command("git", &["--version"]);
        let node_out = run_version_command("node", &["--version"]);
        let npm_out = run_version_command("npm", &["--version"]);
        ToolchainCheck {
            git_available: git_out.is_some(),
            node_available: node_out.is_some(),
            npm_available: npm_out.is_some(),
            git_version: git_out.unwrap_or_default(),
            node_version: node_out.unwrap_or_default(),
            npm_version: npm_out.unwrap_or_default(),
        }
    }
}

/// 同步执行版本命令，返回 stdout 字符串（trim 后）。失败返回 None。
///
/// Windows 上通过 `cmd /c` 调用以正确解析 .cmd/.bat 扩展名并继承用户 PATH。
#[cfg(target_os = "windows")]
fn run_version_command(cmd: &str, args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut full: Vec<&str> = vec!["/c", cmd];
    full.extend(args);
    let output = std::process::Command::new("cmd")
        .args(&full)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() || !output.status.success() {
        None
    } else {
        Some(s)
    }
}

#[cfg(not(target_os = "windows"))]
fn run_version_command(cmd: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}