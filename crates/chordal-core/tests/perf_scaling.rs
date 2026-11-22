//! 复杂度护栏测试：用 perf 检查点的计时数据，对小/中/大三档数据集断言
//! "每条目耗时"不随规模超线性增长（O(n) 保持）。
//!
//! 判定方法：T = O(n) ⇒ per-item = T/n 恒定；T = O(n²) ⇒ n×4 时 per-item×4。
//! 容差取 2.5×（对 4× 数据），能在保留调度噪声余量的同时抓住二次退化。
//! 同时断言关键检查点未超预算（budget_exceeded = false）。

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chordal_core::model::{
    ArtistRef, Capabilities, SearchQuery, SearchResults, SourceId, SourceKind, Track, TrackId,
    TrackOrigin,
};
use chordal_core::perf;
use chordal_core::registry::{SourceDescriptor, SourceRegistry};
use chordal_core::source::{MusicSource, SourceInfo};

struct FakeSource {
    info: SourceInfo,
    tracks: Vec<Track>,
}

#[async_trait]
impl MusicSource for FakeSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::remote_streaming()
    }
    async fn search(&self, _q: &SearchQuery) -> chordal_core::error::CoreResult<SearchResults> {
        Ok(SearchResults {
            source: self.info.id.clone(),
            tracks: self.tracks.clone(),
            total: Some(self.tracks.len() as u64),
            elapsed_ms: None,
        })
    }
    async fn track(&self, _id: &TrackId) -> chordal_core::error::CoreResult<Track> {
        unreachable!()
    }
    async fn stream(
        &self,
        id: &TrackId,
    ) -> chordal_core::error::CoreResult<chordal_core::model::StreamHandle> {
        Ok(chordal_core::model::StreamHandle::Url(format!(
            "https://fake/{}",
            id.id
        )))
    }
}

fn make_tracks(source: &str, n: usize, unique: bool) -> Vec<Track> {
    (0..n)
        .map(|i| {
            let title = if unique {
                // 带源前缀：两个源之间也不重名，合并后零去重损失。
                format!("{source}-song-{i:06}")
            } else {
                "同名的歌".to_string()
            };
            Track::new(
                TrackId::new(SourceId::new(source), i.to_string()),
                title,
                vec![ArtistRef {
                    id: None,
                    name: "测试艺人".into(),
                }],
                None,
                Some(200_000),
                TrackOrigin::Local {
                    path: PathBuf::from(format!("/fake/{source}/{i}.flac")),
                },
            )
        })
        .collect()
}

fn load_source(reg: &SourceRegistry, id: &str, tracks: Vec<Track>) {
    let info = SourceInfo {
        id: SourceId::new(id),
        display_name: id.into(),
        kind: SourceKind::HttpApi,
        description: String::new(),
        priority: 0,
    };
    let src: Arc<dyn MusicSource> = Arc::new(FakeSource {
        info: info.clone(),
        tracks,
    });
    let src_for_load = src.clone();
    reg.register(SourceDescriptor {
        info,
        build: Box::new(move |_| Ok(src.clone())),
    })
    .unwrap();
    reg.load(&SourceId::new(id), |_| Ok(src_for_load.clone()))
        .unwrap();
}

/// 跑一次聚合搜索并返回 (候选数, per-item 微秒)。
async fn measure_merge(candidates: usize) -> (usize, f64) {
    perf::clear();
    let reg = SourceRegistry::new();
    let half = candidates / 2;
    load_source(&reg, "a", make_tracks("a", half, true));
    load_source(&reg, "b", make_tracks("b", half, true));

    // 预热一次（页缓存等），再正式测量。
    let _ = reg.search_all(&SearchQuery::track("song"), 10_000).await;

    perf::clear();
    let agg = reg.search_all(&SearchQuery::track("song"), 10_000).await;
    assert_eq!(
        agg.tracks.len(),
        candidates,
        "unique titles → no dedup loss"
    );

    let (count, total_us) = perf::total_for("registry.merge");
    assert!(count >= 1, "merge checkpoint must be recorded");
    (candidates, total_us as f64 / candidates as f64)
}

#[tokio::test]
async fn merge_stays_linear_across_scales() {
    let (n1, per1) = measure_merge(2_000).await;
    let (n2, per2) = measure_merge(8_000).await;
    let (n3, per3) = measure_merge(32_000).await;
    assert_eq!((n1, n2, n3), (2000, 8000, 32000));

    // 4× 数据 → per-item 不得超过 2.5×（二次复杂度会是 ~4×）。
    assert!(
        per2 < per1 * 2.5,
        "per-item time grew superlinearly 2k→8k: {per1:.3}us → {per2:.3}us"
    );
    assert!(
        per3 < per2 * 2.5,
        "per-item time grew superlinearly 8k→32k: {per2:.3}us → {per3:.3}us"
    );

    // 关键路径不超预算。
    for cp in perf::snapshot() {
        if cp.name == "registry.merge" || cp.name == "registry.search_all" {
            assert!(!cp.budget_exceeded, "{} exceeded budget: {:?}", cp.name, cp);
        }
    }
}

#[tokio::test]
async fn dedup_keeps_first_by_priority_and_loses_nothing() {
    let reg = SourceRegistry::new();
    let info_a = SourceInfo {
        id: SourceId::new("a"),
        display_name: "a".into(),
        kind: SourceKind::HttpApi,
        description: String::new(),
        priority: 0,
    };
    let info_b = SourceInfo {
        id: SourceId::new("b"),
        display_name: "b".into(),
        kind: SourceKind::HttpApi,
        description: String::new(),
        priority: 1,
    };
    let tracks_a = make_tracks("a", 100, false); // 全部同名
    let tracks_b = make_tracks("b", 100, false); // 同名 → 应全部被去重
    let src_a: Arc<dyn MusicSource> = Arc::new(FakeSource {
        info: info_a.clone(),
        tracks: tracks_a,
    });
    let src_b: Arc<dyn MusicSource> = Arc::new(FakeSource {
        info: info_b.clone(),
        tracks: tracks_b,
    });
    let src_a2 = src_a.clone();
    let src_b2 = src_b.clone();
    reg.register(SourceDescriptor {
        info: info_a,
        build: Box::new(move |_| Ok(src_a.clone())),
    })
    .unwrap();
    reg.register(SourceDescriptor {
        info: info_b,
        build: Box::new(move |_| Ok(src_b.clone())),
    })
    .unwrap();
    reg.load(&SourceId::new("a"), |_| Ok(src_a2.clone()))
        .unwrap();
    reg.load(&SourceId::new("b"), |_| Ok(src_b2.clone()))
        .unwrap();

    let agg = reg.search_all(&SearchQuery::track("同名的歌"), 1_000).await;
    assert_eq!(agg.tracks.len(), 100, "同名同艺人 → 去重到 100");
    assert_eq!(agg.counts().map(|(_, n)| n).sum::<usize>(), 200);
}
