//! 应用级有界后台执行器。
//!
//! GPUI 的后台执行器在 Windows 上使用系统线程池的高优先级工作项，不适合扫描、
//! 图片解码等重任务。这里提供固定 worker、有界排队和较低优先级的执行边界；
//! 播放线程与实时音频回调不进入该池。

use std::future::Future;
use std::num::NonZeroUsize;
use std::panic::AssertUnwindSafe;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::FutureExt;
use futures::channel::oneshot;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use futures::future::{AbortHandle, Abortable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolError {
    /// 队列已满，调用方应合并、丢弃或稍后重试。
    Busy,
    /// 任务 panic；worker 会继续处理后续任务。
    Panicked,
    /// 执行器关闭或回复通道被丢弃。
    Cancelled,
}

pub struct BackgroundPool {
    pool: ThreadPool,
    queued: std::sync::Arc<AtomicUsize>,
    capacity: usize,
    workers: usize,
}

impl std::fmt::Debug for BackgroundPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundPool")
            .field("workers", &self.workers)
            .field("capacity", &self.capacity)
            .field("queued", &self.queued_len())
            .finish()
    }
}

impl BackgroundPool {
    pub fn new(workers: usize, capacity: usize) -> Self {
        let workers = workers.max(1);
        let capacity = capacity.max(1);
        let queued = std::sync::Arc::new(AtomicUsize::new(0));
        let worker_queued = queued.clone();
        let pool = ThreadPoolBuilder::new()
            .pool_size(workers)
            .name_prefix("chordial-bg")
            .after_start(move |_| lower_current_thread_priority())
            .create()
            .expect("create chordal background pool");
        Self {
            pool,
            queued: worker_queued,
            capacity,
            workers,
        }
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn queued_len(&self) -> usize {
        self.queued.load(Ordering::Acquire)
    }

    /// 提交一个可并行的 future。返回值可直接被 GPUI/Tokio 等 executor await。
    ///
    /// 计数同时包含正在执行与排队的任务；任务 panic 会被转换为 [`PoolError`]，
    /// 不会让 worker 静默退出。
    pub fn spawn<R, F>(
        &self,
        task: F,
    ) -> Result<impl Future<Output = Result<R, PoolError>>, PoolError>
    where
        R: Send + 'static,
        F: Future<Output = R> + Send + 'static,
    {
        let previous = self
            .queued
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < self.capacity).then_some(current + 1)
            })
            .map_err(|_| PoolError::Busy)?;

        debug_assert!(previous < self.capacity);
        let queued = self.queued.clone();
        let (tx, rx) = oneshot::channel();
        let (abort, registration) = AbortHandle::new_pair();
        self.pool.spawn_ok(async move {
            let result = AssertUnwindSafe(Abortable::new(task, registration))
                .catch_unwind()
                .await;
            queued.fetch_sub(1, Ordering::AcqRel);
            let outcome = match result {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(_)) => Err(PoolError::Cancelled),
                Err(_) => Err(PoolError::Panicked),
            };
            let _ = tx.send(outcome);
        });

        Ok(async move {
            let _cancel = CancelOnDrop(abort);
            rx.await.map_err(|_| PoolError::Cancelled)?
        })
    }
}

struct CancelOnDrop(AbortHandle);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// 进程共享的固定后台池。worker 数可通过 `CHORDIAL_BACKGROUND_WORKERS` 覆盖，
/// 队列容量可通过 `CHORDIAL_BACKGROUND_QUEUE` 覆盖。
pub fn background_pool() -> &'static BackgroundPool {
    static POOL: OnceLock<BackgroundPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let default_workers = std::thread::available_parallelism()
            .unwrap_or_else(|_| NonZeroUsize::new(2).expect("nonzero"))
            .get()
            .clamp(2, 4);
        let workers = std::env::var("CHORDIAL_BACKGROUND_WORKERS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(default_workers)
            .min(8);
        let capacity = std::env::var("CHORDIAL_BACKGROUND_QUEUE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(|| workers.saturating_mul(16));
        BackgroundPool::new(workers, capacity)
    })
}

#[cfg(windows)]
pub fn lower_current_thread_priority() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY: GetCurrentThread 返回当前线程的伪句柄，SetThreadPriority 只影响
    // 当前 worker 的调度优先级，不改变进程其它线程。
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(windows))]
pub fn lower_current_thread_priority() {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn spawned_future_completes_off_caller_thread() {
        let pool = BackgroundPool::new(1, 4);
        let caller = std::thread::current().id();
        let task = pool
            .spawn(async move { (std::thread::current().id(), 42) })
            .expect("submit");
        let worker = futures::executor::block_on(task).expect("run");
        assert_ne!(worker.0, caller);
        assert_eq!(worker.1, 42);
        assert_eq!(pool.queued_len(), 0);
    }

    #[test]
    fn full_queue_is_rejected_without_blocking() {
        let pool = BackgroundPool::new(1, 2);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let first = pool
            .spawn(async move {
                let _ = started_tx.send(());
                let _ = release_rx.await;
            })
            .expect("first");
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let second = pool.spawn(async {}).expect("queued");
        assert_eq!(pool.queued_len(), 2);
        assert_eq!(pool.spawn(async {}).err(), Some(PoolError::Busy));

        release_tx.send(()).unwrap();
        futures::executor::block_on(first).expect("first");
        futures::executor::block_on(second).expect("second");
        assert_eq!(pool.queued_len(), 0);
    }

    #[test]
    fn dropping_awaiter_aborts_queued_or_running_task() {
        struct SignalOnDrop(std::sync::mpsc::Sender<()>);
        impl Drop for SignalOnDrop {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }

        let pool = BackgroundPool::new(1, 2);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (dropped_tx, dropped_rx) = std::sync::mpsc::channel();
        let mut task = Box::pin(
            pool.spawn(async move {
                let _signal = SignalOnDrop(dropped_tx);
                let _ = started_tx.send(());
                std::future::pending::<()>().await
            })
            .expect("submit"),
        );
        let waker = futures::task::noop_waker();
        let mut context = std::task::Context::from_waker(&waker);
        assert!(task.as_mut().poll(&mut context).is_pending());
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(task);
        dropped_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("task must be aborted");
        let deadline = Instant::now() + Duration::from_secs(2);
        while pool.queued_len() != 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(pool.queued_len(), 0);
    }

    #[test]
    fn panic_becomes_error_and_worker_remains_usable() {
        let pool = BackgroundPool::new(1, 2);
        let task = pool
            .spawn(async {
                panic!("intentional background panic");
            })
            .expect("submit");
        let result = futures::executor::block_on(task);
        assert_eq!(result.unwrap_err(), PoolError::Panicked);
        assert_eq!(pool.queued_len(), 0);

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match pool.spawn(async { 7 }) {
                Ok(task) => {
                    assert_eq!(futures::executor::block_on(task).expect("run"), 7);
                    break;
                }
                Err(PoolError::Busy) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("worker did not recover: {error:?}"),
            }
        }
    }
}
