//! 托管服务：lifecycle（由 service.rs 拆分，行为不变）。

use super::helpers::*;
use super::*;

impl NeteaseService {
    pub fn from_config(
        mode: NeteaseMode,
        managed: &ManagedConfig,
        endpoint: Arc<Endpoint>,
    ) -> Option<Arc<Self>> {
        if mode == NeteaseMode::External {
            return None;
        }
        Some(Arc::new(Self {
            mode,
            external_base_url: endpoint.get(),
            config: managed.clone(),
            endpoint,
            state: Mutex::new(ServiceState::Stopped),
            child: tokio::sync::Mutex::new(None),
            start_lock: tokio::sync::Mutex::new(()),
            notify: tokio::sync::Notify::new(),
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(200))),
            generation: AtomicU64::new(0),
        }))
    }

    /// 确保服务可用，返回当前 base_url。安装/启动在后台任务中执行，
    /// 调用方超时被 drop 也不会中断下载。
    pub async fn ensure_ready(self: &Arc<Self>) -> CoreResult<String> {
        let guard = self.start_lock.lock().await;
        if let Some(base) = self.current_ready().await? {
            return Ok(base);
        }
        match self.state_snapshot() {
            ServiceState::Installing => {
                let notified = self.notify.notified();
                drop(guard);
                notified.await;
                return self.wait_result().await;
            }
            ServiceState::Failed(e) => {
                return Err(CoreError::source_err("netease", e));
            }
            _ => {}
        }

        // Auto 且没有本地可管理资产：优先复用用户已运行的外部服务。
        if self.mode == NeteaseMode::Auto && !self.has_local_managed() {
            let external = self.external_base_url.clone();
            if probe(&external).await {
                self.endpoint.set(external.clone());
                self.set_state(ServiceState::ExternalReady {
                    base_url: external.clone(),
                });
                return Ok(external);
            }
        }

        self.set_state(ServiceState::Installing);
        let notified = self.notify.notified();
        let this = self.clone();
        tokio::spawn(async move {
            let result = this.prepare_and_start().await;
            if let Err(e) = &result {
                tracing::error!(target: "chordial::netease", error = %e, "api-enhanced prepare failed");
                // 卸载/停止可能已经发生，不要把状态改回 Failed 之外的运行态。
                if matches!(this.state_snapshot(), ServiceState::Installing) {
                    this.set_state(ServiceState::Failed(e.to_string()));
                }
            }
            // 成功时 prepare_and_start 已经写入 Ready/child。
            this.notify.notify_waiters();
        });
        drop(guard);
        notified.await;
        self.wait_result().await
    }

    /// 源卸载时调用；`keep_warm` 为 true 时保留进程。
    pub async fn stop_if_not_keep_warm(&self) {
        if self.config.keep_warm {
            return;
        }
        self.stop().await;
    }

    /// 主动停止 Node 进程。
    pub async fn stop(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut state) = self.state.lock() {
            *state = ServiceState::Stopped;
        }
        self.notify.notify_waiters();
        let mut guard = self.child.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }

    pub fn logs_tail(&self, lines: usize) -> Vec<String> {
        self.logs
            .lock()
            .map(|logs| logs.iter().rev().take(lines).rev().cloned().collect())
            .unwrap_or_default()
    }

    pub(super) async fn current_ready(&self) -> CoreResult<Option<String>> {
        let mut child_guard = self.child.lock().await;
        let (ready, external) = {
            let state = self
                .state
                .lock()
                .map_err(|_| CoreError::source_err("netease", "service state poisoned"))?;
            match &*state {
                ServiceState::Ready { base_url, .. } => (Some(base_url.clone()), false),
                ServiceState::ExternalReady { base_url } => (Some(base_url.clone()), true),
                _ => (None, false),
            }
        };
        let Some(base_url) = ready else {
            return Ok(None);
        };
        if external {
            return Ok(Some(base_url));
        }
        match child_guard.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(None) => Ok(Some(base_url)),
                Ok(Some(status)) => {
                    tracing::warn!(
                        target: "chordial::netease",
                        code = status.code(),
                        "managed api-enhanced exited; will restart on next request"
                    );
                    *child_guard = None;
                    if let Ok(mut state) = self.state.lock() {
                        *state = ServiceState::Stopped;
                    }
                    Ok(None)
                }
                Err(e) => {
                    tracing::warn!(target: "chordial::netease", error = %e, "try_wait failed");
                    *child_guard = None;
                    if let Ok(mut state) = self.state.lock() {
                        *state = ServiceState::Stopped;
                    }
                    Ok(None)
                }
            },
            None => {
                if let Ok(mut state) = self.state.lock() {
                    *state = ServiceState::Stopped;
                }
                Ok(None)
            }
        }
    }

    pub(super) fn state_snapshot(&self) -> ServiceState {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(ServiceState::Stopped)
    }

    pub(super) fn set_state(&self, state: ServiceState) {
        if let Ok(mut current) = self.state.lock() {
            *current = state;
        }
    }

    pub(super) async fn wait_result(&self) -> CoreResult<String> {
        loop {
            match self.state_snapshot() {
                ServiceState::Ready { base_url, .. } => {
                    if self.current_ready().await?.is_some() {
                        return Ok(base_url);
                    }
                }
                ServiceState::ExternalReady { base_url } => return Ok(base_url),
                ServiceState::Failed(e) => return Err(CoreError::source_err("netease", e)),
                ServiceState::Installing => {
                    self.notify.notified().await;
                }
                ServiceState::Stopped => {
                    return Err(CoreError::source_err("netease", "api-enhanced 服务已停止"));
                }
            }
        }
    }
}
