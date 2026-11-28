//! 托管服务：start（由 service.rs 拆分，行为不变）。

use super::helpers::*;
use super::*;

impl NeteaseService {
    pub(super) fn has_local_managed(&self) -> bool {
        if self
            .config
            .binary_path
            .as_ref()
            .is_some_and(|path| path.is_file())
        {
            return true;
        }
        if resolve_app_dir(&self.config).is_some() {
            return true;
        }
        installed_binary(&self.config).is_some()
    }

    pub(super) async fn prepare_and_start(&self) -> CoreResult<String> {
        let target = self.resolve_launch_target().await?;
        let epoch = self.generation.load(Ordering::Acquire);
        let port = pick_port(self.config.port)?;
        let base_url = format!("http://127.0.0.1:{port}");

        let mut command = Command::new(&target.program);
        for arg in &target.args {
            command.arg(arg);
        }
        command
            .current_dir(&target.cwd)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .env("NODE_ENV", "production")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if target.node {
            command.env("NODE_OPTIONS", "--max-old-space-size=256");
        }
        #[cfg(windows)]
        {
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        tracing::info!(
            target: "chordial::netease",
            program = %target.program.display(),
            label = %target.label,
            port,
            "spawning managed api-enhanced"
        );
        let mut child = command.spawn().map_err(|e| {
            CoreError::source_err(
                "netease",
                format!(
                    "启动 api-enhanced 失败（{}）：{e}",
                    target.program.display()
                ),
            )
        })?;
        let pid = child.id().unwrap_or(0);
        if let Some(stdout) = child.stdout.take() {
            spawn_log_reader(stdout, self.logs.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_log_reader(stderr, self.logs.clone());
        }

        let timeout = Duration::from_millis(self.config.startup_timeout_ms.max(1_000));
        if let Err(e) = wait_ready(&mut child, &base_url, timeout, &self.logs).await {
            let _ = child.kill().await;
            let tail = self.logs_tail(12).join("\n");
            return Err(CoreError::source_err(
                "netease",
                format!("api-enhanced 启动失败：{e}\n{tail}"),
            ));
        }

        // stop() 可能在下载/启动期间发生：此时丢弃刚启动的进程，不写 Ready。
        if epoch != self.generation.load(Ordering::Acquire)
            || !matches!(self.state_snapshot(), ServiceState::Installing)
        {
            let _ = child.kill().await;
            return Err(CoreError::source_err(
                "netease",
                "api-enhanced 启动被取消（源已停止）",
            ));
        }
        self.endpoint.set(base_url.clone());
        self.set_state(ServiceState::Ready {
            base_url: base_url.clone(),
            pid,
        });
        *self.child.lock().await = Some(child);
        // 检查 set_state 与 child 写入之间的 stop 竞态。
        if epoch != self.generation.load(Ordering::Acquire) {
            if let Some(mut child) = self.child.lock().await.take() {
                let _ = child.kill().await;
            }
            self.set_state(ServiceState::Stopped);
            return Err(CoreError::source_err(
                "netease",
                "api-enhanced 启动被取消（源已停止）",
            ));
        }
        tracing::info!(
            target: "chordial::netease",
            pid,
            base_url = %base_url,
            "managed api-enhanced ready"
        );
        Ok(base_url)
    }

    pub(super) async fn resolve_launch_target(&self) -> CoreResult<LaunchTarget> {
        if let Some(binary) = &self.config.binary_path {
            if !binary.is_file() {
                return Err(CoreError::source_err(
                    "netease",
                    format!("managed.binary_path 不存在：{}", binary.display()),
                ));
            }
            return Ok(LaunchTarget::binary(binary.clone()));
        }
        if let Some(dir) = &self.config.app_dir {
            return node_target(dir, &self.config).ok_or_else(|| {
                CoreError::source_err(
                    "netease",
                    format!("{} 中没有 app.js / server.js / index.js", dir.display()),
                )
            });
        }
        if let Some(binary) = installed_binary(&self.config) {
            return Ok(LaunchTarget::binary(binary));
        }
        if let Some(dir) = resolve_app_dir(&self.config) {
            let node = resolve_node(&self.config, &dir);
            let node_usable = node.is_file()
                || std::process::Command::new(&node)
                    .arg("--version")
                    .output()
                    .map(|out| out.status.success())
                    .unwrap_or(false);
            if node_usable && let Some(target) = node_target(&dir, &self.config) {
                return Ok(target);
            }
        }
        if self.config.auto_install {
            let binary = install_binary(&self.config, &self.logs).await?;
            return Ok(LaunchTarget::binary(binary));
        }
        Err(CoreError::source_err(
            "netease",
            format!(
                "未找到 api-enhanced 且 auto_install=false。查找过：{}",
                candidate_app_dirs(&self.config)
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            ),
        ))
    }
}
