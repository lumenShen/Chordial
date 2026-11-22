//! 本地源复杂度护栏：搜索过滤 O(n·m) 的线性表现 + O(1) 主键查找。
//! 数据通过 inject_index_for_tests 注入，无磁盘依赖。

use std::path::PathBuf;
use std::task::{Context as TaskContext, Poll, RawWaker, RawWakerVTable, Waker};

use chordal_core::model::{ArtistRef, SearchQuery, SourceId, Track, TrackId, TrackOrigin};
use chordal_core::perf;
use chordal_core::source::MusicSource;
use chordal_local::LocalSource;

fn local_track(i: usize) -> Track {
    Track::new(
        TrackId::new(SourceId::new("local"), format!("t{i}")),
        format!("歌曲{i:06}"),
        vec![ArtistRef {
            id: None,
            name: format!("艺人{}", i % 50),
        }],
        None,
        Some(180_000),
        TrackOrigin::Local {
            path: PathBuf::from(format!("/m/{i}.mp3")),
        },
    )
}

fn make_local(n: usize) -> std::sync::Arc<LocalSource> {
    let src = LocalSource::new(
        SourceId::new("local"),
        chordal_local::LocalSourceConfig { roots: vec![] },
        "测试库",
    );
    let tracks: Vec<Track> = (0..n).map(local_track).collect();
    src.inject_index_for_tests(tracks);
    src
}

/// 本 crate 的 async 方法从不开 await 挂起：noop-waker 单步轮询即可。
fn block_test<F: std::future::Future>(fut: F) -> F::Output {
    static VTABLE: RawWakerVTable =
        RawWakerVTable::new(|p| RawWaker::new(p, &VTABLE), |_| {}, |_| {}, |_| {});
    let mut fut = std::pin::pin!(fut);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = TaskContext::from_waker(&waker);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => unreachable!("local futures never await"),
    }
}

fn measure_filter(n: usize) -> f64 {
    let src = make_local(n);
    perf::clear();
    let res = block_test(async { src.search(&SearchQuery::track("歌曲00")).await }).unwrap();
    assert!(!res.tracks.is_empty());
    let (count, total_us) = perf::total_for("local.search.filter");
    assert!(count >= 1, "filter checkpoint must be recorded");
    total_us as f64 / n as f64
}

#[test]
fn search_filter_stays_linear() {
    let per1 = measure_filter(1_000);
    let per2 = measure_filter(8_000);
    let per3 = measure_filter(64_000);
    assert!(
        per2 < per1 * 2.5,
        "per-item 1k→8k grew superlinearly: {per1:.3}us → {per2:.3}us"
    );
    assert!(
        per3 < per2 * 2.5,
        "per-item 8k→64k grew superlinearly: {per2:.3}us → {per3:.3}us"
    );
    for cp in perf::snapshot() {
        if cp.name == "local.search.filter" {
            assert!(!cp.budget_exceeded, "filter exceeded budget: {cp:?}");
        }
    }
}

#[test]
fn search_early_exit_bounded_by_limit() {
    // 命中数远超 limit 时，扫描在 offset+limit 处提前止步。
    let src = make_local(50_000);
    let mut q = SearchQuery::track("歌曲");
    q.limit = 10;
    let res = block_test(async { src.search(&q).await }).unwrap();
    assert_eq!(res.track_count(), 10);
    assert_eq!(res.total, Some(10));
}

#[test]
fn track_lookup_is_o1_constant() {
    let src = make_local(64_000);
    let id = TrackId::new(SourceId::new("local"), "t63999");
    let start = std::time::Instant::now();
    let found = block_test(async { src.track(&id).await }).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(found.id.id, "t63999");
    assert!(
        elapsed < std::time::Duration::from_millis(10),
        "O(1) lookup must be fast, took {elapsed:?}"
    );
}
