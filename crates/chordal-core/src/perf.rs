//! 性能检查点系统 —— 全仓库热点路径的统一观测层。
//!
//! 设计目标（对应"注意性能消耗"的约束）：
//! 1. **埋点开销恒定**：每次记录 = 1 次 `Instant::elapsed` + O(1) 环形缓冲
//!    写入 + 一条 tracing 事件，无动态扩容、无锁竞争热点（单把 Mutex，
//!    临界区为 memcpy 级别）。
//! 2. **预算告警**：每个检查点有微秒级预算（[`budget_us`]），超预算记为
//!    `budget_exceeded` 并 `tracing::warn!`。测试阶段直接断言关键路径
//!    （搜索合并、本地扫描、UI 渲染）不超预算，把 O(n²) 退化拦在测试期。
//! 3. **复杂度护栏**：`tests/perf_scaling.rs` 用 [`CheckpointLedger`] 的
//!    计时数据对小/中/大三档数据集断言时间缩放比 ≈ 线性。
//! 4. **可扩展观测**：[`set_hook`] 允许外接 profiler（chrome trace / puffin）
//!    而不必改业务代码。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 单次检查点记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub name: &'static str,
    /// 耗时（微秒）。
    pub duration_us: u64,
    /// 本次操作涉及的条目数（合并条数、扫描文件数、渲染行数…）。
    pub items: u64,
    /// 记录时刻（unix ms），用于把检查点对齐到交互时间线。
    pub ts_ms: u64,
    /// 是否超过 [`budget_us`] 预算。
    pub budget_exceeded: bool,
}

impl Checkpoint {
    /// 微秒级吞吐（items/秒），items=0 时为 None。
    pub fn throughput(&self) -> Option<f64> {
        if self.items == 0 || self.duration_us == 0 {
            return None;
        }
        Some(self.items as f64 * 1_000_000.0 / self.duration_us as f64)
    }
}

/// 环形缓冲账本：容量恒定，写入 O(1)，内存 O(capacity)。
/// 有意不做成无界 —— 长时间运行的 GUI 不能让观测层自身变成内存泄漏。
pub struct CheckpointLedger {
    ring: VecDeque<Checkpoint>,
    capacity: usize,
    /// 各检查点名称 → (事件数, 累计微秒)。用于复杂度断言，容量恒定。
    totals: Mutex<Vec<(&'static str, u64, u64)>>,
    total_events: u64,
}

impl CheckpointLedger {
    pub const DEFAULT_CAPACITY: usize = 4096;

    pub fn new(capacity: usize) -> Self {
        Self {
            ring: VecDeque::with_capacity(capacity),
            capacity,
            totals: Mutex::new(Vec::new()),
            total_events: 0,
        }
    }

    /// O(1) 写入；超出容量淘汰最旧记录，内存不增长。
    pub fn record(&mut self, cp: Checkpoint) {
        self.total_events += 1;
        if self.ring.len() == self.capacity {
            self.ring.pop_front();
        }
        self.ring.push_back(cp);
        let mut totals = self.totals.lock().expect("perf totals poisoned");
        match totals.iter_mut().find(|(name, _, _)| *name == cp.name) {
            Some((_, count, us)) => {
                *count += 1;
                *us += cp.duration_us;
            }
            None => totals.push((cp.name, 1, cp.duration_us)),
        }
    }

    pub fn snapshot(&self) -> Vec<Checkpoint> {
        self.ring.iter().copied().collect()
    }

    /// 某检查点的 (事件数, 累计微秒)。O(检查点种类数)，种类是编译期常量级。
    pub fn total_for(&self, name: &str) -> (u64, u64) {
        let totals = self.totals.lock().expect("perf totals poisoned");
        totals
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|&(_, c, us)| (c, us))
            .unwrap_or((0, 0))
    }

    pub fn total_events(&self) -> u64 {
        self.total_events
    }

    pub fn clear(&mut self) {
        self.ring.clear();
        self.totals.lock().expect("perf totals poisoned").clear();
        self.total_events = 0;
    }
}

static LEDGER: OnceLock<Mutex<CheckpointLedger>> = OnceLock::new();
static HOOK: OnceLock<Mutex<Arc<dyn Fn(&Checkpoint) + Send + Sync>>> = OnceLock::new();

fn ledger() -> &'static Mutex<CheckpointLedger> {
    LEDGER.get_or_init(|| Mutex::new(CheckpointLedger::new(CheckpointLedger::DEFAULT_CAPACITY)))
}

/// 注册外部观测 sink（chrome-trace 导出等）。先注册的会被覆盖。
pub fn set_hook(hook: Arc<dyn Fn(&Checkpoint) + Send + Sync>) {
    *HOOK
        .get_or_init(|| Mutex::new(Arc::new(|_| {})))
        .lock()
        .expect("perf hook poisoned") = hook;
}

/// 各检查点的基础耗时预算（微秒）。超预算 = 性能退化信号。
///
/// 固定预算适合与规模无关的路径（UI 渲染、单次查找）。对于处理 n 条目的
/// 批处理路径，用 [`budget_for`] 加上每项配额，否则大输入会在 debug 构建里
/// 误报；真正的复杂度护栏仍是 `perf_scaling` 的 per-item 缩放比。
pub fn budget_us(name: &str) -> Option<u64> {
    // 300Hz 帧预算 3.33ms；UI 路径留 10% 余量取 3.0ms。
    // 网络、磁盘与后台解析继续用 16ms 的交互可接受上限，避免误报。
    const FRAME: u64 = 3_000;
    const BACKGROUND: u64 = 16_000;
    match name {
        "ui.render.pass" => Some(FRAME),
        "registry.merge" => Some(BACKGROUND),
        "registry.search_all" => Some(3_000_000),
        "source.search" => Some(3_000_000),
        "local.scan.index" => Some(5_000_000),
        "local.search.filter" => Some(20_000),
        "local.track_lookup" => Some(1_000),
        "netease.request" => Some(5_000_000),
        "netease.stream_resolve" => Some(3_000_000),
        "player.decode.warmup" => Some(1_000_000),
        "player.seek" => Some(250_000),
        "lyrics.parse" => Some(BACKGROUND),
        // AMLL 等价实现（歌词解析/整理/整形/绘制 + 背景光栅化）的检查点。
        // 解析、整理、布局、文本整形与单次绘制都发生在 UI 帧内，按 300Hz
        // 帧预算；时间线更新（set_time/update）必须远低于帧预算。
        // 背景光栅化按像素配 per_item 配额（见 budget_for），基础值与帧对齐。
        "amll.parse" => Some(FRAME),
        "amll.document" => Some(FRAME),
        "amll.layout" => Some(FRAME),
        "amll.set_time" => Some(1_000),
        "amll.update" => Some(500),
        "amll.text.shape" => Some(FRAME),
        "amll.render.pass" => Some(FRAME),
        "amll.bg.raster" => Some(FRAME),
        "amll.bg.upscale" => Some(FRAME),
        "amll.convert" => Some(2_000),
        "amll.cover.decode" => Some(BACKGROUND),
        _ => None,
    }
}

/// 计入条目数后的预算：`base + items * per_item`。每项配额按 debug 构建
/// 的实测上限设定，同时远低于 O(n²) 退化的量级（32k 条若二次退化是秒级）。
pub fn budget_for(name: &str, items: u64) -> Option<u64> {
    let base = budget_us(name)?;
    let per_item = match name {
        "registry.merge" => 2,
        "local.search.filter" => 1,
        "local.scan.index" => 2,
        "lyrics.parse" => 1,
        // AMLL 路径：items 分别是行/词/字形/像素数。逐项配额取 debug 构建实测
        // 上限的保守值，同时远低于任何 O(n²) 退化的量级。
        "amll.parse" => 1,
        "amll.document" => 2,
        "amll.layout" => 2,
        "amll.text.shape" => 2,
        "amll.bg.raster" => 1,
        "amll.bg.upscale" => 1,
        "amll.convert" => 1,
        _ => 0,
    };
    Some(base.saturating_add(items.saturating_mul(per_item)))
}

fn emit(mut cp: Checkpoint) {
    cp.budget_exceeded = budget_for(cp.name, cp.items).is_some_and(|b| cp.duration_us > b);
    // 检查点每帧都会产生；默认 INFO 会形成日志风暴，改为 TRACE。
    // 超预算单独 WARN，保证性能退化在默认日志级别即可见。
    tracing::event!(
        target: "chordial::perf",
        tracing::Level::TRACE,
        name = cp.name,
        items = cp.items,
        duration_us = cp.duration_us,
        budget_exceeded = cp.budget_exceeded,
        "perf.checkpoint"
    );
    if cp.budget_exceeded {
        tracing::warn!(
            target: "chordial::perf",
            name = cp.name,
            duration_us = cp.duration_us,
            items = cp.items,
            "perf budget exceeded"
        );
    }
    if let Some(hook) = HOOK.get()
        && let Ok(hook) = hook.lock()
    {
        hook(&cp);
    }
    if let Ok(mut ledger) = ledger().lock() {
        ledger.record(cp);
    }
}

/// 记录一次已经完成的操作。
pub fn record(name: &'static str, items: u64, duration: Duration) {
    emit(Checkpoint {
        name,
        duration_us: duration.as_micros().min(u64::MAX as u128) as u64,
        items,
        ts_ms: unix_ms(),
        budget_exceeded: false,
    });
}

/// 同步代码块计时：`perf::measure("registry.merge", total, || merge(...))`。
pub fn measure<T>(name: &'static str, items: u64, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    record(name, items, start.elapsed());
    out
}

/// RAII 作用域检查点。条目数在作用域结束时才能确定的路径用这个：
///
/// ```ignore
/// let _scope = perf::scope("local.scan.index");
/// ... // 扫描
/// _scope.finish(n_files); // 显式收尾；未收尾则 drop 时按 items=0 记录
/// ```
pub struct PerfScope {
    name: &'static str,
    start: Instant,
    done: bool,
}

impl PerfScope {
    pub fn enter(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
            done: false,
        }
    }

    /// 立即结算（附带条目数），之后的 drop 不再记录。
    pub fn finish(mut self, items: u64) {
        self.done = true;
        record(self.name, items, self.start.elapsed());
    }
}

impl Drop for PerfScope {
    fn drop(&mut self) {
        if !self.done {
            record(self.name, 0, self.start.elapsed());
        }
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

/// 测试与 UI 读取账本的快照。
pub fn snapshot() -> Vec<Checkpoint> {
    ledger()
        .lock()
        .map(|l| l.snapshot())
        .expect("perf ledger poisoned")
}

pub fn total_for(name: &str) -> (u64, u64) {
    ledger()
        .lock()
        .map(|l| l.total_for(name))
        .expect("perf ledger poisoned")
}

pub fn clear() {
    if let Ok(mut l) = ledger().lock() {
        l.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    // 账本是进程级全局状态；减少并发测试互相 clear 的干扰。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn test_lock() -> MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn ring_buffer_memory_is_bounded() {
        // 观测层自身的复杂度护栏：写入远超容量的事件，账本不增长。
        let mut led = CheckpointLedger::new(64);
        for i in 0..10_000u64 {
            led.record(Checkpoint {
                name: "test.op",
                duration_us: i,
                items: 1,
                ts_ms: i,
                budget_exceeded: false,
            });
        }
        assert_eq!(led.snapshot().len(), 64);
        assert_eq!(led.total_events(), 10_000);
        // totals 表也恒定：同种事件聚合，不随事件数增长。
        assert_eq!(led.total_for("test.op"), (10_000, (0..10_000u64).sum()));
    }

    #[test]
    fn scope_records_once() {
        let _guard = test_lock();
        clear();
        {
            let s = PerfScope::enter("test.scope");
            s.finish(42);
        } // drop 后不得重复记录
        assert_eq!(total_for("test.scope").0, 1);
    }

    #[test]
    fn scope_drop_records_without_items() {
        let _guard = test_lock();
        clear();
        {
            let _s = PerfScope::enter("test.scope.drop");
        }
        assert_eq!(total_for("test.scope.drop").0, 1);
    }
}
