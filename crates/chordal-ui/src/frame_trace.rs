//! 临时帧内分段计时（`CHORDIAL_FRAME_TRACE=1`）。
//!
//! 不是产品观测层：只在诊断构建/诊断运行时启用，默认一次原子读即返回。
//! 用来把 GPUI 的 draw/prepaint 总耗时拆到具体页面区域，定位帧预算热点。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

const REPORT_SAMPLES: usize = 512;

#[derive(Default)]
struct Section {
    samples: Vec<u32>,
}

static ENABLED: OnceLock<bool> = OnceLock::new();

pub(crate) fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("CHORDIAL_FRAME_TRACE").is_some())
}

thread_local! {
    static SECTIONS: RefCell<HashMap<&'static str, Section>> = RefCell::new(HashMap::new());
}

pub(crate) fn add(label: &'static str, elapsed: Duration) {
    if !enabled() {
        return;
    }
    let us = elapsed.as_micros().min(u32::MAX as u128) as u32;
    SECTIONS.with(|sections| {
        let mut sections = sections.borrow_mut();
        let section = sections.entry(label).or_default();
        section.samples.push(us);
        if section.samples.len() < REPORT_SAMPLES {
            return;
        }
        section.samples.sort_unstable();
        let n = section.samples.len();
        let at = |p: f64| section.samples[((n - 1) as f64 * p).round() as usize];
        // 直接写 stdout：tracing 的统一调用点会被 LogStormGuard 按“每秒
        // 4 条”限流，而多个 section 往往同一帧达到报告阈值，后半会被丢掉。
        println!(
            "FRAME_SECTION label={label} samples={n} p50_us={} p95_us={} p99_us={} max_us={}",
            at(0.50),
            at(0.95),
            at(0.99),
            section.samples[n - 1],
        );
        section.samples.clear();
    });
}
