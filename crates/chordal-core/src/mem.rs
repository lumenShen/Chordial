//! 运行资源观测层 —— 分配器计数、进程常驻内存、进程 CPU 时间与命名计量。
//!
//! 这是"内存 / 占用分析工具"的数据面：
//!
//! 1. [`TrackingAllocator`]：包装系统分配器，用原子计数记录**当前堆活跃字节**、
//!    历史峰值、分配/释放次数。二进制的 `#[global_allocator]` 指向它即可，
//!    每次分配只增加一次原子加法（热路径分配本就很稀少），不改变分配语义。
//! 2. [`snapshot`]：一次读取 OS 常驻内存（Windows `K32GetProcessMemoryInfo`、
//!    Linux `/proc/self`；其它平台尽力而为）与堆计数，供界面/日志展示。
//! 3. [`start_sampler`]：后台线程按固定间隔采样到容量恒定的环形账本，
//!    观测层自身的内存有上界（[`SAMPLE_CAPACITY`] × [`MemSample`]）。
//!    `CHORDIAL_MEM_TRACE=1` 时每采样一行 `tracing` 日志。
//! 4. [`Gauge`]：业务侧主动登记的"已知大块内存"（音频流缓冲、封面纹理、
//!    歌词模型、背景光栅）。全局活跃字节只能说明总量，gauge 用来说明
//!    **是谁**在占。
//! 5. [`process_cpu_time`]：进程累计用户态 + 内核态 CPU 时间。采样两次
//!    做差即可得到区间占用率（画面性能浮窗用 500ms 间隔）。
//!
//! 环境变量：
//! - `CHORDIAL_MEM_INTERVAL_MS`：采样间隔，默认 2000，`0` 关闭采样线程；
//! - `CHORDIAL_MEM_TRACE=1`：采样写入 `target=chordial::mem` 的 INFO 日志；
//! - `CHORDIAL_MEM_WARN_MB`：常驻内存告警阈值（MiB），默认 2048，`0` 关闭。
//!
//! 注意：分配器计数是**进程级**的；`chordal-core` 的单元测试不会安装全局
//! 分配器，因此只验证计数器算术，不依赖真实分配。

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ===================== 全局分配器计数 =====================

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static FREES: AtomicU64 = AtomicU64::new(0);

/// 统计型全局分配器。转发到系统分配器，只增加 O(1) 原子计数。
#[derive(Debug, Default, Clone, Copy)]
pub struct TrackingAllocator;

impl TrackingAllocator {
    #[inline]
    fn note_alloc(size: usize) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        if size == 0 {
            return;
        }
        let live = LIVE_BYTES.fetch_add(size, Ordering::Relaxed) + size;
        // 峰值用 CAS 更新：并发分配时不能把大值覆盖回小值。
        let mut peak = PEAK_BYTES.load(Ordering::Relaxed);
        while live > peak {
            match PEAK_BYTES.compare_exchange_weak(peak, live, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break,
                Err(observed) => peak = observed,
            }
        }
    }

    #[inline]
    fn note_free(size: usize) {
        FREES.fetch_add(1, Ordering::Relaxed);
        if size == 0 {
            return;
        }
        // 防御性饱和：realloc 记账误差不应把活跃字节减成天文数字。
        let _ = LIVE_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |live| {
            Some(live.saturating_sub(size))
        });
    }
}

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            Self::note_alloc(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            Self::note_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        Self::note_free(layout.size());
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            // realloc 成功：旧块已释放，只记账净变化。
            if new_size >= layout.size() {
                Self::note_alloc(new_size - layout.size());
            } else {
                Self::note_free(layout.size() - new_size);
            }
        }
        new_ptr
    }
}

// ===================== 命名计量 =====================

/// 业务侧登记的大块内存类别。数量固定，避免观测层自己成为分配来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Gauge {
    /// 当前 HTTP 渐进式音频流缓存的已下载字节（含 Vec 容量冗余）。
    AudioStream = 0,
    /// 当前专辑封面纹理（RGBA/RenderImage 源数据）字节。
    Cover = 1,
    /// 当前歌词模型（含预览副本）的估算字节。
    Lyrics = 2,
    /// 流体背景光栅双缓冲的配置字节（output + scratch）。
    Background = 3,
    /// 本地音乐库曲目模型的估算字节。
    Library = 4,
}

/// gauge 数量；新增类别时同步 [`Gauge`] 与 [`Gauge::ALL`]。
pub const GAUGE_COUNT: usize = 5;

impl Gauge {
    pub const ALL: [Gauge; GAUGE_COUNT] = [
        Gauge::AudioStream,
        Gauge::Cover,
        Gauge::Lyrics,
        Gauge::Background,
        Gauge::Library,
    ];

    /// 日志/导出用的稳定英文键。
    pub fn key(self) -> &'static str {
        match self {
            Gauge::AudioStream => "audio_stream",
            Gauge::Cover => "cover",
            Gauge::Lyrics => "lyrics",
            Gauge::Background => "background",
            Gauge::Library => "library",
        }
    }
}

static GAUGES: [AtomicUsize; GAUGE_COUNT] = [const { AtomicUsize::new(0) }; GAUGE_COUNT];

/// 覆盖式写入某类别的当前占用。
pub fn set_gauge(gauge: Gauge, bytes: usize) {
    GAUGES[gauge as usize].store(bytes, Ordering::Relaxed);
}

/// 在现有值上增减（用于容量增长/释放）；下界饱和到 0。
pub fn add_gauge(gauge: Gauge, delta: isize) {
    let slot = &GAUGES[gauge as usize];
    if delta >= 0 {
        slot.fetch_add(delta as usize, Ordering::Relaxed);
    } else {
        let _ = slot.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_sub(delta.unsigned_abs()))
        });
    }
}

/// 减少某类别占用（下界 0）。
pub fn sub_gauge(gauge: Gauge, bytes: usize) {
    let _ = GAUGES[gauge as usize].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_sub(bytes))
    });
}

pub fn gauge(gauge: Gauge) -> usize {
    GAUGES[gauge as usize].load(Ordering::Relaxed)
}

// ===================== 进程内存 =====================

/// 一次内存快照。`rss_bytes` 在平台不支持时为 `None`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemSnapshot {
    /// 当前堆活跃字节（分配器统计，不含 GPU 纹理与 mmap）。
    pub live_bytes: usize,
    /// 进程生命周期内的堆活跃峰值。
    pub peak_live_bytes: usize,
    /// 分配/释放调用次数。
    pub allocations: u64,
    pub frees: u64,
    /// 常驻内存（Working Set / VmRSS）。
    pub rss_bytes: Option<u64>,
    /// 常驻内存历史峰值。
    pub peak_rss_bytes: Option<u64>,
    /// 虚拟地址空间大小（Linux 可用）。
    pub virtual_bytes: Option<u64>,
    /// 命名计量，按 [`Gauge`] 索引。
    pub gauges: [usize; GAUGE_COUNT],
}

impl MemSnapshot {
    pub fn gauge(&self, gauge: Gauge) -> usize {
        self.gauges[gauge as usize]
    }

    /// 展示用的"主内存数字"：优先常驻内存，其次堆活跃。
    pub fn resident(&self) -> u64 {
        self.rss_bytes.unwrap_or(self.live_bytes as u64)
    }

    /// 峰值同上。
    pub fn peak_resident(&self) -> u64 {
        self.peak_rss_bytes.unwrap_or(self.peak_live_bytes as u64)
    }
}

/// 读取当前进程内存。不会分配大对象；Windows 走一次系统调用。
pub fn snapshot() -> MemSnapshot {
    let (rss, peak_rss, virt) = platform::process_memory();
    let mut gauges = [0usize; GAUGE_COUNT];
    for gauge in Gauge::ALL {
        gauges[gauge as usize] = GAUGES[gauge as usize].load(Ordering::Relaxed);
    }
    MemSnapshot {
        live_bytes: LIVE_BYTES.load(Ordering::Relaxed),
        peak_live_bytes: PEAK_BYTES.load(Ordering::Relaxed),
        allocations: ALLOCATIONS.load(Ordering::Relaxed),
        frees: FREES.load(Ordering::Relaxed),
        rss_bytes: rss,
        peak_rss_bytes: peak_rss,
        virtual_bytes: virt,
        gauges,
    }
}

// ===================== 采样环形账本 =====================

/// 采样历史容量：2s 间隔约 10 分钟。
pub const SAMPLE_CAPACITY: usize = 300;

/// 一条采样记录，保留时间线与当时各 gauge 的值。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemSample {
    pub ts_ms: u64,
    pub snapshot: MemSnapshot,
}

static SAMPLES: OnceLock<Mutex<VecDeque<MemSample>>> = OnceLock::new();
static SAMPLER: OnceLock<()> = OnceLock::new();

fn samples() -> &'static Mutex<VecDeque<MemSample>> {
    SAMPLES.get_or_init(|| Mutex::new(VecDeque::with_capacity(SAMPLE_CAPACITY)))
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

/// 记录一条采样；超出容量淘汰最旧记录（O(1)）。
pub fn record_sample(snapshot: MemSnapshot) {
    if let Ok(mut ring) = samples().lock() {
        if ring.len() == SAMPLE_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(MemSample {
            ts_ms: unix_ms(),
            snapshot,
        });
    }
}

/// 采样历史（最旧 → 最新）。
pub fn sample_history() -> Vec<MemSample> {
    samples()
        .lock()
        .map(|ring| ring.iter().copied().collect())
        .unwrap_or_default()
}

pub fn clear_samples() {
    if let Ok(mut ring) = samples().lock() {
        ring.clear();
    }
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok()?.trim().parse().ok()
}

/// 启动后台采样线程。重复调用无副作用；`CHORDIAL_MEM_INTERVAL_MS=0` 关闭。
pub fn start_sampler() {
    if SAMPLER.set(()).is_err() {
        return;
    }
    let interval_ms = env_u64("CHORDIAL_MEM_INTERVAL_MS").unwrap_or(2_000);
    if interval_ms == 0 {
        tracing::info!(target: "chordial::mem", "memory sampler disabled");
        return;
    }
    let interval = Duration::from_millis(interval_ms.clamp(100, 60_000));
    let trace = std::env::var_os("CHORDIAL_MEM_TRACE").is_some();
    let warn_bytes = env_u64("CHORDIAL_MEM_WARN_MB")
        .unwrap_or(2_048)
        .saturating_mul(1024 * 1024);
    let spawn = std::thread::Builder::new()
        .name("chordial-mem-sampler".into())
        .spawn(move || {
            let mut last_warn: Option<Instant> = None;
            loop {
                std::thread::sleep(interval);
                let snap = snapshot();
                record_sample(snap);
                if trace {
                    tracing::info!(
                        target: "chordial::mem",
                        rss_mb = snap.rss_bytes.map(|b| b / (1024 * 1024)),
                        peak_rss_mb = snap.peak_rss_bytes.map(|b| b / (1024 * 1024)),
                        live_kb = snap.live_bytes / 1024,
                        peak_live_kb = snap.peak_live_bytes / 1024,
                        allocs = snap.allocations,
                        frees = snap.frees,
                        audio_stream_kb = snap.gauge(Gauge::AudioStream) / 1024,
                        cover_kb = snap.gauge(Gauge::Cover) / 1024,
                        lyrics_kb = snap.gauge(Gauge::Lyrics) / 1024,
                        background_kb = snap.gauge(Gauge::Background) / 1024,
                        library_kb = snap.gauge(Gauge::Library) / 1024,
                        "mem.sample"
                    );
                }
                // 常驻超阈值时限频告警，避免每 2s 刷屏。
                if warn_bytes > 0
                    && snap.rss_bytes.is_some_and(|rss| rss > warn_bytes)
                    && last_warn
                        .map(|at| at.elapsed() >= Duration::from_secs(60))
                        .unwrap_or(true)
                {
                    last_warn = Some(Instant::now());
                    tracing::warn!(
                        target: "chordial::mem",
                        rss_mb = snap.rss_bytes.map(|b| b / (1024 * 1024)),
                        peak_rss_mb = snap.peak_rss_bytes.map(|b| b / (1024 * 1024)),
                        live_kb = snap.live_bytes / 1024,
                        "resident memory above configured warning threshold"
                    );
                }
            }
        });
    if let Err(err) = spawn {
        tracing::warn!(target: "chordial::mem", error = %err, "memory sampler spawn failed");
    }
}

/// 进程累计 CPU 时间（用户态 + 内核态）。
///
/// 采样两次做差即可得到区间 CPU 占用率。值来自 OS 计数器（Windows
/// `GetProcessTimes`、Linux `/proc/self/stat`、macOS `getrusage`），
/// 单次调用无分配，可安全用于 2~4Hz 的界面刷新。
pub fn process_cpu_time() -> Option<Duration> {
    platform::process_cpu_time()
}

/// 人类可读字节数（二进制单位）。日志与界面共用。
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

// ===================== 平台实现 =====================

mod platform {
    use std::time::Duration;

    /// 返回 `(rss, peak_rss, virtual)`，单位字节。
    pub fn process_memory() -> (Option<u64>, Option<u64>, Option<u64>) {
        imp::process_memory()
    }

    /// 累计 CPU 时间（用户态 + 内核态）。
    pub fn process_cpu_time() -> Option<Duration> {
        imp::process_cpu_time()
    }

    #[cfg(windows)]
    mod imp {
        use core::ffi::c_void;
        use std::time::Duration;

        #[repr(C)]
        #[derive(Default)]
        struct ProcessMemoryCounters {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }

        #[repr(C)]
        #[derive(Default)]
        struct FileTime {
            low: u32,
            high: u32,
        }

        impl FileTime {
            fn to_u64(&self) -> u64 {
                ((self.high as u64) << 32) | self.low as u64
            }
        }

        // K32GetProcessMemoryInfo 是 kernel32 自 Vista 起的导出，避免依赖
        // 已弃用的 psapi.lib；PROCESS_MEMORY_COUNTERS 布局与 SDK 一致。
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
            fn K32GetProcessMemoryInfo(
                process: *mut c_void,
                counters: *mut ProcessMemoryCounters,
                cb: u32,
            ) -> i32;
            fn GetProcessTimes(
                process: *mut c_void,
                creation: *mut FileTime,
                exit: *mut FileTime,
                kernel: *mut FileTime,
                user: *mut FileTime,
            ) -> i32;
        }

        pub fn process_memory() -> (Option<u64>, Option<u64>, Option<u64>) {
            let mut counters = ProcessMemoryCounters {
                cb: core::mem::size_of::<ProcessMemoryCounters>() as u32,
                ..Default::default()
            };
            let cb = counters.cb;
            let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, cb) };
            if ok == 0 {
                return (None, None, None);
            }
            (
                Some(counters.working_set_size as u64),
                Some(counters.peak_working_set_size as u64),
                None,
            )
        }

        pub fn process_cpu_time() -> Option<Duration> {
            let mut creation = FileTime::default();
            let mut exit = FileTime::default();
            let mut kernel = FileTime::default();
            let mut user = FileTime::default();
            let ok = unsafe {
                GetProcessTimes(
                    GetCurrentProcess(),
                    &mut creation,
                    &mut exit,
                    &mut kernel,
                    &mut user,
                )
            };
            if ok == 0 {
                return None;
            }
            // FILETIME 以 100ns 为单位；只取用户 + 内核，不含空闲等待。
            let ticks_100ns = kernel.to_u64().saturating_add(user.to_u64());
            Some(Duration::from_nanos(ticks_100ns.saturating_mul(100)))
        }
    }

    #[cfg(target_os = "linux")]
    mod imp {
        use std::fs;
        use std::time::Duration;

        fn page_size() -> u64 {
            // SAFETY: sysconf 是纯查询，无前置条件。
            let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            size.max(1) as u64
        }

        fn clock_ticks_per_sec() -> u64 {
            // SAFETY: sysconf 是纯查询，无前置条件。
            let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
            ticks.max(1) as u64
        }

        pub fn process_cpu_time() -> Option<Duration> {
            let stat = fs::read_to_string("/proc/self/stat").ok()?;
            // comm 字段可能包含空格与右括号；从最后一个 ')' 之后开始切分。
            let rest = stat.rsplit_once(')')?.1;
            let mut fields = rest.split_whitespace();
            // state 是字段 3；utime/stime 是字段 14/15，对应 rest 的索引 11/12。
            let utime: u64 = fields.nth(11)?.parse().ok()?;
            let stime: u64 = fields.next()?.parse().ok()?;
            let ticks = utime.saturating_add(stime);
            let hz = clock_ticks_per_sec();
            Some(Duration::from_secs_f64(ticks as f64 / hz as f64))
        }

        pub fn process_memory() -> (Option<u64>, Option<u64>, Option<u64>) {
            let page = page_size();
            let statm = fs::read_to_string("/proc/self/statm").ok();
            let (virt, rss) = statm
                .as_deref()
                .and_then(|text| {
                    let mut fields = text.split_whitespace();
                    let size_pages: u64 = fields.next()?.parse().ok()?;
                    let resident_pages: u64 = fields.next()?.parse().ok()?;
                    Some((Some(size_pages * page), Some(resident_pages * page)))
                })
                .unwrap_or((None, None));
            let peak = fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|status| {
                    status.lines().find_map(|line| {
                        line.strip_prefix("VmHWM:")?
                            .split_whitespace()
                            .next()?
                            .parse::<u64>()
                            .ok()
                            .map(|kb| kb * 1024)
                    })
                });
            (rss, peak, virt)
        }
    }

    #[cfg(target_os = "macos")]
    mod imp {
        use std::time::Duration;

        pub fn process_memory() -> (Option<u64>, Option<u64>, Option<u64>) {
            // macOS 的 getrusage 只给峰值（ru_maxrss 单位为字节）；当前 RSS
            // 需要 task_info，此处尽力而为，界面回退到堆活跃字节。
            let mut usage: libc::rusage = unsafe { core::mem::zeroed() };
            let ok = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
            if ok == 0 {
                (None, Some(usage.ru_maxrss as u64), None)
            } else {
                (None, None, None)
            }
        }

        pub fn process_cpu_time() -> Option<Duration> {
            let mut usage: libc::rusage = unsafe { core::mem::zeroed() };
            let ok = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
            if ok != 0 {
                return None;
            }
            let user = usage.ru_utime.tv_sec as u64 * 1_000_000 + usage.ru_utime.tv_usec as u64;
            let system = usage.ru_stime.tv_sec as u64 * 1_000_000 + usage.ru_stime.tv_usec as u64;
            Some(Duration::from_micros(user.saturating_add(system)))
        }
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    mod imp {
        use std::time::Duration;

        pub fn process_memory() -> (Option<u64>, Option<u64>, Option<u64>) {
            (None, None, None)
        }

        pub fn process_cpu_time() -> Option<Duration> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn gauge_is_saturating_and_overwritable() {
        let _guard = lock();
        set_gauge(Gauge::Cover, 1024);
        assert_eq!(gauge(Gauge::Cover), 1024);
        sub_gauge(Gauge::Cover, 4096);
        assert_eq!(gauge(Gauge::Cover), 0, "must not underflow");
        add_gauge(Gauge::Cover, 128);
        add_gauge(Gauge::Cover, -64);
        assert_eq!(gauge(Gauge::Cover), 64);
        set_gauge(Gauge::Cover, 0);
    }

    #[test]
    fn process_cpu_time_is_monotonic() {
        let Some(first) = process_cpu_time() else {
            return; // 平台不支持时跳过。
        };
        std::thread::sleep(Duration::from_millis(20));
        let second = process_cpu_time().expect("same platform should still report");
        assert!(second >= first);
    }

    #[test]
    fn snapshot_reports_all_gauges() {
        let _guard = lock();
        set_gauge(Gauge::AudioStream, 5 * 1024 * 1024);
        let snap = snapshot();
        assert_eq!(snap.gauge(Gauge::AudioStream), 5 * 1024 * 1024);
        assert_eq!(snap.gauges.len(), GAUGE_COUNT);
        set_gauge(Gauge::AudioStream, 0);
    }

    #[test]
    fn sample_ring_is_bounded() {
        let _guard = lock();
        clear_samples();
        for _ in 0..(SAMPLE_CAPACITY * 2) {
            record_sample(MemSnapshot::default());
        }
        assert_eq!(sample_history().len(), SAMPLE_CAPACITY);
        clear_samples();
    }

    #[test]
    fn format_bytes_uses_binary_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
    }
}
