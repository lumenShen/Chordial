//! 部署生命周期操作：git 克隆 / 拉取 / npm 安装依赖。

use super::spec::find_spec;
use super::types::DeployStatus;
use crate::module::perf;
use std::process::Stdio;
use tokio::process::Command;

impl super::SourceDeployer {
    /// 克隆项目（`git clone --depth 1`，浅克隆节省时间与磁盘）。
    pub async fn clone_source(&self, source_id: &str) -> Result<(), String> {
        let spec = find_spec(source_id)
            .ok_or_else(|| format!("未知来源: {}", source_id))?;

        let target_dir = self.source_dir(source_id);
        if target_dir.exists() {
            if target_dir.join(".git").exists() {
                // 幂等：目录已是完整克隆（状态丢失/不同步场景），跳过克隆直接修正状态
                eprintln!(
                    "[source_deployer] {} already cloned at {}, skip clone",
                    source_id,
                    target_dir.display()
                );
                let mut configs = self.configs.write();
                let cfg = configs.entry(source_id.to_string()).or_default();
                cfg.install_path = target_dir.to_string_lossy().to_string();
                cfg.last_status = DeployStatus::Cloned.as_str().to_string();
                cfg.last_error.clear();
                let _ = self.persist(&configs);
                return Ok(());
            }
            // 无 .git 的残留半成品（如上次克隆中断），清理后重新克隆
            eprintln!(
                "[source_deployer] removing stale incomplete dir {}",
                target_dir.display()
            );
            tokio::fs::remove_dir_all(&target_dir)
                .await
                .map_err(|e| format!("清理残留目录失败: {}", e))?;
        }
        if let Some(parent) = target_dir.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("创建父目录失败: {}", e))?;
        }

        let _scope = perf::scope("source_deployer.clone");
        eprintln!(
            "[source_deployer] cloning {} from {}",
            source_id, spec.git_url
        );

        // 尝试主 URL 克隆；失败时若配置了 mirror_url 则回退重试
        let mut clone_err = match try_clone(&spec.git_url, &target_dir).await {
            Ok(()) => None,
            Err(e) => {
                eprintln!(
                    "[source_deployer] {} primary clone failed: {}",
                    source_id, e
                );
                Some(e)
            }
        };

        if clone_err.is_some() {
            if let Some(ref mirror) = spec.mirror_url {
                eprintln!(
                    "[source_deployer] {} retrying with mirror: {}",
                    source_id, mirror
                );
                // 清理半成品目录（上次克隆可能残留）
                let _ = tokio::fs::remove_dir_all(&target_dir).await;
                if let Some(parent) = target_dir.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                clone_err = match try_clone(mirror, &target_dir).await {
                    Ok(()) => None,
                    Err(e) => Some(e),
                };
            }
        }

        if let Some(e) = clone_err {
            return Err(e);
        }

        // 更新配置
        let mut configs = self.configs.write();
        let cfg = configs.entry(source_id.to_string()).or_default();
        cfg.install_path = target_dir.to_string_lossy().to_string();
        cfg.last_status = DeployStatus::Cloned.as_str().to_string();
        cfg.last_error.clear();
        let _ = self.persist(&configs);

        eprintln!("[source_deployer] cloned {} to {}", source_id, target_dir.display());
        Ok(())
    }

    /// 拉取最新代码（`git pull`）。仅在已克隆时可用。
    pub async fn pull_source(&self, source_id: &str) -> Result<(), String> {
        let target_dir = self.source_dir(source_id);
        if !target_dir.exists() {
            return Err(format!("项目尚未克隆: {}", target_dir.display()));
        }
        // 运行中的服务不可 pull（避免文件锁冲突）
        if self.processes.read().contains_key(source_id) {
            return Err("服务运行中，请先停止后再拉取代码".to_string());
        }

        let output = Command::new("git")
            .arg("-C")
            .arg(&target_dir)
            .arg("pull")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("执行 git pull 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git pull 失败: {}", stderr.trim()));
        }
        Ok(())
    }

    /// 安装依赖（`npm install`）。仅在已克隆时可用。
    pub async fn install_source(&self, source_id: &str) -> Result<(), String> {
        let spec = find_spec(source_id)
            .ok_or_else(|| format!("未知来源: {}", source_id))?;

        let target_dir = self.source_dir(source_id);
        if !target_dir.exists() {
            return Err(format!("项目尚未克隆，请先 clone: {}", target_dir.display()));
        }
        if self.processes.read().contains_key(source_id) {
            return Err("服务运行中，请先停止后再安装依赖".to_string());
        }

        let _scope = perf::scope("source_deployer.install");
        eprintln!("[source_deployer] installing deps for {}", source_id);
        let output = spawn_npm(&spec.install_command, &spec.install_args, Some(&target_dir), None)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("执行 {} 失败: {}", spec.install_command, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let err_msg = format!("npm install 失败: {}", stderr.trim());
            let mut configs = self.configs.write();
            let cfg = configs.entry(source_id.to_string()).or_default();
            cfg.last_status = DeployStatus::Error.as_str().to_string();
            cfg.last_error = err_msg.clone();
            let _ = self.persist(&configs);
            return Err(err_msg);
        }

        // 更新配置：标记为 installed
        let mut configs = self.configs.write();
        let cfg = configs.entry(source_id.to_string()).or_default();
        cfg.last_status = DeployStatus::Installed.as_str().to_string();
        cfg.last_error.clear();
        let _ = self.persist(&configs);

        eprintln!("[source_deployer] installed deps for {}", source_id);
        Ok(())
    }
}

// ── 私有辅助 ────────────────────────────────────────────────────────────

/// 执行 `git clone --depth 1 <url> <target_dir>` 并返回 Result。
async fn try_clone(url: &str, target_dir: &std::path::Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(url)
        .arg(target_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("执行 git clone 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone 失败: {}", stderr.trim()));
    }
    Ok(())
}

/// 构造一个调用 `npm` 或 `node` 的异步 `Command`。
///
/// Windows 上 `npm` 是 `npm.cmd` 批处理文件，`Command::new("npm")` 在 Tauri 进程的
/// PATH 下可能找不到 `.cmd` 扩展名，因此通过 `cmd /c npm ...` 调用，让 shell 解析
/// 扩展名并继承用户 PATH。`node` 是 `.exe` 文件，可直接调用，无需 shell 包装。
///
/// 返回的 `Command` 已设置好 `current_dir` 与首个环境变量，调用方可继续链式添加
/// `.env(...)` / `.stdout(...)` / `.stderr(...)` / `.kill_on_drop(...)` / `.spawn()`
/// 或 `.output()`。
pub(super) fn spawn_npm(
    cmd: &str,
    args: &[String],
    current_dir: Option<&std::path::Path>,
    first_env: Option<(&str, String)>,
) -> Command {
    // 判断是否需要通过 shell 调用（Windows 上 npm/npx 等是 .cmd 文件）
    #[cfg(target_os = "windows")]
    {
        let needs_shell = matches!(cmd, "npm" | "npx" | "yarn" | "pnpm");
        if needs_shell {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            // cmd /c <cmd> <args...>
            let mut full: Vec<String> = vec!["/c".into(), cmd.into()];
            full.extend(args.iter().cloned());
            let mut command = Command::new("cmd");
            command.args(&full).creation_flags(CREATE_NO_WINDOW);
            if let Some(dir) = current_dir {
                command.current_dir(dir);
            }
            if let Some((k, v)) = first_env {
                command.env(k, v);
            }
            return command;
        }
    }

    // node 或其它 .exe 程序：直接调用
    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    if let Some((k, v)) = first_env {
        command.env(k, v);
    }
    command
}