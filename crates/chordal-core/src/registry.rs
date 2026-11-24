//! 多源注册表：注册 / 加载 / 卸载 / 聚合搜索。
//!
//! 聚合搜索的两条性能红线（由 `tests/perf_scaling.rs` 护栏测试守卫）：
//! 1. 合并 + 去重是**单遍 O(total)**：按源优先级轮询各结果队列，每首曲目
//!    恰好检查一次 `dedup_key`（构造期预计算，见 [`crate::model::Track`]）。
//! 2. 单源故障隔离：每个源带独立超时（futures-timer，executor 无关），
//!    任何源超时/报错都不影响其它源的结果，也不阻塞聚合返回。
//!
//! 锁纪律：`RwLock` 临界区内只做克隆/查找（memcpy 级别），**绝不持锁
//! await 或持锁执行工厂构造**，避免慢源拖住读路径。

use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use futures::future::{Either, join_all, select};
use futures_timer::Delay;

use crate::error::{CoreError, CoreResult};
use crate::model::{
    Capabilities, CollectionSummary, SearchQuery, SearchResults, SourceId, Track, normalize,
};
use crate::perf::{PerfScope, measure};
use crate::source::{MusicSource, SourceInfo};

/// 源槽位状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceStatus {
    /// 已注册未加载（适配器未构造）。
    Registered,
    Loaded,
    /// 上次加载/操作失败。
    Failed(String),
}

/// UI 可见的槽位快照。
#[derive(Debug, Clone)]
pub struct SourceSlotSnapshot {
    pub info: SourceInfo,
    pub status: SourceStatus,
    pub capabilities: Capabilities,
}

/// 注册描述符：静态信息 + 延迟构造适配器的工厂。
/// 工厂捕获自己的配置（目录、API 地址、Cookie…），registry 不感知具体类型。
pub struct SourceDescriptor {
    pub info: SourceInfo,
    pub build: Box<dyn Fn(&SourceInfo) -> CoreResult<Arc<dyn MusicSource>> + Send + Sync>,
}

/// 聚合搜索单源结果。
#[derive(Debug)]
pub enum SourceSearchOutcome {
    Ok(SearchResults),
    Err(String),
    Timeout(Duration),
}

#[derive(Debug, Clone)]
pub struct SourceSearchReport {
    pub source: SourceId,
    pub outcome: Result<(usize, Option<u64>), String>,
}

/// 一次聚合搜索的完整结果。
#[derive(Debug, Default)]
pub struct AggregatedSearch {
    /// 轮询交错 + 跨源去重后的曲目（相对顺序稳定）。
    pub tracks: Vec<Track>,
    pub per_source: Vec<SourceSearchReport>,
    pub total_elapsed_ms: u64,
}

impl AggregatedSearch {
    /// UI 源徽章：每个源的命中数。
    pub fn counts(&self) -> impl Iterator<Item = (&SourceId, usize)> + '_ {
        self.per_source
            .iter()
            .filter_map(|r| r.outcome.as_ref().ok().map(|(n, _)| (&r.source, *n)))
    }
}

/// 一次聚合集合搜索的结果。
#[derive(Debug, Default)]
pub struct AggregatedCollections {
    pub collections: Vec<CollectionSummary>,
    pub elapsed_ms: u64,
}

struct SourceSlot {
    info: SourceInfo,
    status: SourceStatus,
    source: Option<Arc<dyn MusicSource>>,
}

/// 多源注册表。线程安全；所有方法不阻塞调用线程。
pub struct SourceRegistry {
    slots: RwLock<BTreeMap<SourceId, SourceSlot>>,
    /// 每次结构变化自增；UI 观察它决定是否刷新。
    version: AtomicU64,
}

impl Default for SourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self {
            slots: RwLock::new(BTreeMap::new()),
            version: AtomicU64::new(0),
        }
    }

    fn bump(&self) {
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    /// 注册描述符（状态 = Registered，未加载）。
    pub fn register(&self, descriptor: SourceDescriptor) -> CoreResult<()> {
        let id = descriptor.info.id.clone();
        {
            let mut slots = self.slots.write().expect("registry lock poisoned");
            if slots.contains_key(&id) {
                return Err(CoreError::SourceExists { src: id.0.clone() });
            }
            slots.insert(
                id,
                SourceSlot {
                    info: descriptor.info,
                    status: SourceStatus::Registered,
                    source: None,
                },
            );
        }
        self.bump();
        Ok(())
    }

    /// 注册一个已经构造好的实例并直接标记为 Loaded。
    ///
    /// 用于音源实例管理：启动时从配置批量装配/重建实例；实例本身由调用方
    /// 持有，卸载后再启用可复用同一实例（本地索引零丢失）。
    pub fn register_loaded(
        &self,
        info: SourceInfo,
        source: Arc<dyn MusicSource>,
    ) -> CoreResult<()> {
        let id = info.id.clone();
        {
            let mut slots = self.slots.write().expect("registry lock poisoned");
            if slots.contains_key(&id) {
                return Err(CoreError::SourceExists { src: id.0.clone() });
            }
            slots.insert(
                id,
                SourceSlot {
                    info,
                    status: SourceStatus::Loaded,
                    source: Some(source),
                },
            );
        }
        self.bump();
        Ok(())
    }

    /// 软关闭：把实例从活跃槽位移除，但**不**调用 `shutdown`。
    ///
    /// 与 [`SourceRegistry::unload`] 的区别：关闭音源不应清空缓存/停止
    /// 托管服务，用户随时可能再打开；真正删除实例时才走 unload。
    pub fn deactivate(&self, id: &SourceId) -> CoreResult<()> {
        {
            let mut slots = self.slots.write().expect("registry lock poisoned");
            let Some(slot) = slots.get_mut(id) else {
                return Err(CoreError::SourceNotLoaded { src: id.0.clone() });
            };
            slot.source = None;
            slot.status = SourceStatus::Registered;
        }
        self.bump();
        Ok(())
    }

    /// 删除槽位（保留实例所有权给调用方）。用于删除音源实例：
    /// 调用方应先 `unload`（触发 shutdown），再 `unregister`。
    pub fn unregister(&self, id: &SourceId) -> Option<SourceInfo> {
        let removed = {
            let mut slots = self.slots.write().expect("registry lock poisoned");
            slots.remove(id).map(|slot| slot.info)
        };
        if removed.is_some() {
            self.bump();
        }
        removed
    }

    /// 加载已注册的源。工厂在锁外执行（构造本地源可能扫盘），
    /// 失败则槽位标记 [`SourceStatus::Failed`]。
    pub fn load(
        &self,
        id: &SourceId,
        factory: impl FnOnce(&SourceInfo) -> CoreResult<Arc<dyn MusicSource>>,
    ) -> CoreResult<()> {
        let info = {
            let slots = self.slots.read().expect("registry lock poisoned");
            let Some(slot) = slots.get(id) else {
                return Err(CoreError::SourceNotLoaded { src: id.0.clone() });
            };
            slot.info.clone()
        };

        let result = factory(&info);

        let mut slots = self.slots.write().expect("registry lock poisoned");
        let Some(slot) = slots.get_mut(id) else {
            return Err(CoreError::SourceNotLoaded { src: id.0.clone() });
        };
        match result {
            Ok(source) => {
                slot.source = Some(source);
                slot.status = SourceStatus::Loaded;
            }
            Err(e) => slot.status = SourceStatus::Failed(e.to_string()),
        }
        drop(slots);
        self.bump();
        Ok(())
    }

    /// 卸载：调用适配器 `shutdown` 钩子后移除实例，槽位回到 Registered。
    pub async fn unload(&self, id: &SourceId) -> CoreResult<()> {
        let instance = {
            let mut slots = self.slots.write().expect("registry lock poisoned");
            let Some(slot) = slots.get_mut(id) else {
                return Err(CoreError::SourceNotLoaded { src: id.0.clone() });
            };
            slot.source.take()
        };
        if let Some(source) = instance {
            source.shutdown().await;
        }
        {
            let mut slots = self.slots.write().expect("registry lock poisoned");
            if let Some(slot) = slots.get_mut(id) {
                slot.status = SourceStatus::Registered;
            }
        }
        self.bump();
        Ok(())
    }

    /// 只更新展示元信息（重命名实例），不动实例/状态。
    pub fn update_info(&self, id: &SourceId, info: SourceInfo) -> CoreResult<()> {
        {
            let mut slots = self.slots.write().expect("registry lock poisoned");
            let Some(slot) = slots.get_mut(id) else {
                return Err(CoreError::SourceNotLoaded { src: id.0.clone() });
            };
            slot.info = info;
        }
        self.bump();
        Ok(())
    }

    pub fn status(&self, id: &SourceId) -> Option<SourceStatus> {
        self.slots
            .read()
            .expect("registry lock poisoned")
            .get(id)
            .map(|s| s.status.clone())
    }

    /// 已加载实例（不存在或未加载返回 None）。O(log n)。
    pub fn get(&self, id: &SourceId) -> Option<Arc<dyn MusicSource>> {
        self.slots
            .read()
            .expect("registry lock poisoned")
            .get(id)
            .and_then(|s| s.source.clone())
    }

    pub fn slot_info(&self, id: &SourceId) -> Option<SourceInfo> {
        self.slots
            .read()
            .expect("registry lock poisoned")
            .get(id)
            .map(|slot| slot.info.clone())
    }

    /// 所有槽位快照（UI 源面板）。O(n) 单遍。
    pub fn snapshot(&self) -> Vec<SourceSlotSnapshot> {
        self.slots
            .read()
            .expect("registry lock poisoned")
            .values()
            .map(|slot| SourceSlotSnapshot {
                capabilities: slot
                    .source
                    .as_ref()
                    .map(|s| s.capabilities())
                    .unwrap_or_default(),
                info: slot.info.clone(),
                status: slot.status.clone(),
            })
            .collect()
    }

    pub fn loaded_sources(&self) -> Vec<SourceInfo> {
        self.slots
            .read()
            .expect("registry lock poisoned")
            .values()
            .filter(|s| s.source.is_some())
            .map(|s| s.info.clone())
            .collect()
    }

    /// 聚合搜索：对所有已加载且具备 search 能力的源并发 fan-out，
    /// 单源超时兜底，最后单遍合并去重。
    ///
    /// `timeout_ms` 为每源上限（网络源建议 6000，本地源天然远低于它）。
    pub async fn search_all(&self, query: &SearchQuery, timeout_ms: u64) -> AggregatedSearch {
        let overall = PerfScope::enter("registry.search_all");
        let start = Instant::now();

        // 1) 取快照：持锁时间 = 克隆 Arc 的开销，不 await。
        let sources: Vec<(SourceInfo, Arc<dyn MusicSource>)> = {
            let slots = self.slots.read().expect("registry lock poisoned");
            slots
                .values()
                .filter(|s| s.source.is_some())
                .map(|s| (s.info.clone(), s.source.clone().expect("checked")))
                .collect()
        };
        let sources: Vec<_> = sources
            .into_iter()
            .filter(|(_, s)| s.capabilities().search)
            .collect();

        // 2) fan-out：单源超时（futures_timer::Delay 与任何 executor 兼容）。
        let timeout = Duration::from_millis(timeout_ms);
        let futs = sources.iter().map(|(info, source)| {
            let query = query.clone();
            async move {
                let start = Instant::now();
                let mut search_fut = pin!(source.search(&query));
                let timer = Delay::new(timeout);
                let outcome = match select(search_fut.as_mut(), timer).await {
                    Either::Left((res, _)) => match res {
                        Ok(mut results) => {
                            results.elapsed_ms =
                                Some(start.elapsed().as_millis().min(u64::MAX as u128) as u64);
                            SourceSearchOutcome::Ok(results)
                        }
                        Err(e) => SourceSearchOutcome::Err(e.to_string()),
                    },
                    Either::Right(_) => SourceSearchOutcome::Timeout(timeout),
                };
                let _ = info;
                outcome
            }
        });
        let outcomes: Vec<SourceSearchOutcome> = join_all(futs).await;

        // 3) 合并去重：单遍 O(total)，带检查点（条目数 = 候选总数）。
        let order: Vec<SourceId> = sources.iter().map(|(i, _)| i.id.clone()).collect();
        let total_candidates: usize = outcomes
            .iter()
            .map(|o| match o {
                SourceSearchOutcome::Ok(r) => r.tracks.len(),
                _ => 0,
            })
            .sum();

        let (tracks, per_source) = measure("registry.merge", total_candidates as u64, || {
            merge_round_robin(&order, outcomes)
        });

        let total_elapsed_ms = per_source
            .iter()
            .filter_map(|r| r.outcome.as_ref().ok().and_then(|(_, ms)| *ms))
            .max()
            .unwrap_or_else(|| start.elapsed().as_millis().min(u64::MAX as u128) as u64);

        overall.finish(total_candidates as u64);
        AggregatedSearch {
            tracks,
            per_source,
            total_elapsed_ms,
        }
    }

    /// 聚合集合搜索（专辑/歌手/歌单）：对具备 catalog 能力的已加载源并发
    /// fan-out，单源超时，最后按 `(kind, 标题, 创建者)` 单遍去重。
    pub async fn search_collections_all(
        &self,
        query: &SearchQuery,
        timeout_ms: u64,
    ) -> AggregatedCollections {
        let overall = PerfScope::enter("registry.search_collections");
        let start = Instant::now();
        let sources: Vec<Arc<dyn MusicSource>> = {
            let slots = self.slots.read().expect("registry lock poisoned");
            slots
                .values()
                .filter(|s| s.source.is_some())
                .filter_map(|s| s.source.clone())
                .filter(|s| s.capabilities().catalog)
                .collect()
        };
        let timeout = Duration::from_millis(timeout_ms);
        let futs = sources.into_iter().map(|source| {
            let query = query.clone();
            async move {
                let Some(catalog) = source.catalog() else {
                    return Vec::new();
                };
                let mut search_fut = pin!(catalog.search_collections(&query));
                let timer = Delay::new(timeout);
                match select(search_fut.as_mut(), timer).await {
                    Either::Left((Ok(list), _)) => list,
                    _ => Vec::new(),
                }
            }
        });
        let results: Vec<Vec<CollectionSummary>> = join_all(futs).await;
        let total: usize = results.iter().map(Vec::len).sum();
        let collections = measure("registry.collection_merge", total as u64, || {
            merge_collections(results)
        });
        overall.finish(total as u64);
        AggregatedCollections {
            collections,
            elapsed_ms: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
        }
    }
}

/// 轮询交错合并集合，按 `(kind, 标题, 创建者)` 去重。
fn merge_collections(results: Vec<Vec<CollectionSummary>>) -> Vec<CollectionSummary> {
    let estimated: usize = results.iter().map(Vec::len).sum();
    let mut queues: Vec<std::vec::IntoIter<CollectionSummary>> =
        results.into_iter().map(Vec::into_iter).collect();
    let mut merged = Vec::with_capacity(estimated);
    let mut seen: FxHashSet<String> = FxHashSet::default();
    seen.reserve(estimated);
    let mut active = queues.len();
    while active > 0 {
        active = 0;
        for queue in queues.iter_mut() {
            if let Some(collection) = queue.next() {
                let key = format!(
                    "{:?}\u{1}{}\u{1}{}",
                    collection.kind,
                    normalize(&collection.title),
                    normalize(collection.creator.as_deref().unwrap_or(""))
                );
                if seen.insert(key) {
                    merged.push(collection);
                }
                active += 1;
            }
        }
    }
    merged
}

/// 轮询交错合并：优先级靠前的源先出。每个候选恰好被检查一次，
/// 跨源去重用预计算的 `dedup_key`（O(1) 哈希查表）。
///
/// 消耗 `outcomes`（避免克隆整批曲目）；报告先于曲目移动构造。
fn merge_round_robin(
    order: &[SourceId],
    outcomes: Vec<SourceSearchOutcome>,
) -> (Vec<Track>, Vec<SourceSearchReport>) {
    let mut reports = Vec::with_capacity(order.len());
    let mut queues: Vec<std::vec::IntoIter<Track>> = Vec::with_capacity(order.len());

    for outcome in outcomes {
        match outcome {
            SourceSearchOutcome::Ok(results) => {
                let SearchResults {
                    tracks, elapsed_ms, ..
                } = results;
                let count = tracks.len();
                reports.push(SourceSearchReport {
                    source: SourceId::default(), // 占位，下面按 order 顺序回填
                    outcome: Ok((count, elapsed_ms)),
                });
                queues.push(tracks.into_iter());
            }
            SourceSearchOutcome::Err(e) => reports.push(SourceSearchReport {
                source: SourceId::default(),
                outcome: Err(e),
            }),
            SourceSearchOutcome::Timeout(t) => reports.push(SourceSearchReport {
                source: SourceId::default(),
                outcome: Err(format!("timeout after {t:?}")),
            }),
        }
    }
    // outcomes 与 order 顺序一致，回填真实 SourceId。
    for (report, id) in reports.iter_mut().zip(order.iter()) {
        report.source = id.clone();
    }

    let estimated_total: usize = queues.iter().map(|q| q.size_hint().0).sum();
    let mut merged: Vec<Track> = Vec::with_capacity(estimated_total);
    // 去重表容量预分配 = 候选总数，过程中不 rehash。
    // 值记录首次命中的源：同一源内同名曲目（同名不同版本）保留，
    // 只有跨源重复才丢弃 —— 单遍 O(1)/首，无二次扫描。
    let mut seen: FxHashMap<String, SourceId> = FxHashMap::default();
    seen.reserve(estimated_total);

    let mut queues = queues.into_iter().collect::<Vec<_>>();
    let mut active = queues.len();
    while active > 0 {
        active = 0;
        for queue in queues.iter_mut() {
            if let Some(track) = queue.next() {
                // 每个候选一次 O(1) 哈希检查；跨源重复直接丢弃。
                match seen.get(&track.dedup_key) {
                    Some(owner) if *owner != track.id.source => {}
                    Some(_) => merged.push(track),
                    None => {
                        seen.insert(track.dedup_key.clone(), track.id.source.clone());
                        merged.push(track);
                    }
                }
                active += 1;
            }
        }
    }

    (merged, reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArtistRef, SourceKind, TrackId, TrackOrigin};
    use async_trait::async_trait;

    struct FakeSource {
        info: SourceInfo,
        tracks: Vec<Track>,
        delay: Option<Duration>,
    }

    #[async_trait]
    impl MusicSource for FakeSource {
        fn info(&self) -> &SourceInfo {
            &self.info
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::remote_streaming()
        }
        async fn search(&self, _query: &SearchQuery) -> CoreResult<SearchResults> {
            if let Some(d) = self.delay {
                Delay::new(d).await;
            }
            Ok(SearchResults {
                source: self.info.id.clone(),
                tracks: self.tracks.clone(),
                total: Some(self.tracks.len() as u64),
                elapsed_ms: None,
            })
        }
        async fn track(&self, id: &TrackId) -> CoreResult<Track> {
            self.tracks
                .iter()
                .find(|t| t.id.id == id.id)
                .cloned()
                .ok_or_else(|| CoreError::Other("no such track".into()))
        }
        async fn stream(&self, id: &TrackId) -> CoreResult<crate::model::StreamHandle> {
            Ok(crate::model::StreamHandle::Url(format!(
                "https://fake/{}.mp3",
                id.id
            )))
        }
    }

    fn fake_track(source: &str, idx: usize, same_title: bool) -> Track {
        let title = if same_title {
            "同名的歌".to_string()
        } else {
            format!("song {idx}")
        };
        Track::new(
            TrackId::new(SourceId::new(source), idx.to_string()),
            title,
            vec![ArtistRef {
                id: None,
                name: "artist".into(),
            }],
            None,
            Some(180_000),
            TrackOrigin::Remote {
                source: SourceId::new(source),
            },
        )
    }

    fn make_info(id: &str) -> SourceInfo {
        SourceInfo {
            id: SourceId::new(id),
            display_name: id.to_string(),
            kind: SourceKind::HttpApi,
            description: String::new(),
            priority: 0,
        }
    }

    fn register_fake(
        reg: &SourceRegistry,
        id: &str,
        n: usize,
        same_title: bool,
        delay: Option<Duration>,
    ) {
        let info = make_info(id);
        let tracks: Vec<Track> = (0..n).map(|i| fake_track(id, i, same_title)).collect();
        let src: Arc<dyn MusicSource> = Arc::new(FakeSource {
            info: info.clone(),
            tracks,
            delay,
        });
        reg.register(SourceDescriptor {
            info,
            build: Box::new(move |_| Ok(src.clone())),
        })
        .unwrap();
    }

    /// 直接给已注册源提供一个返回"预构造实例"的工厂并加载。
    fn load_with_instance(reg: &SourceRegistry, id: &str, src: Arc<dyn MusicSource>) {
        let sid = SourceId::new(id);
        reg.load(&sid, |_| Ok(src.clone())).unwrap();
    }

    fn load_normal(reg: &SourceRegistry, id: &str, n: usize) {
        let info = make_info(id);
        let tracks: Vec<Track> = (0..n).map(|i| fake_track(id, i, false)).collect();
        let src: Arc<dyn MusicSource> = Arc::new(FakeSource {
            info: info.clone(),
            tracks,
            delay: None,
        });
        load_with_instance(reg, id, src);
    }

    #[tokio::test]
    async fn load_unload_lifecycle() {
        let reg = SourceRegistry::new();
        let id = SourceId::new("a");
        register_fake(&reg, "a", 3, false, None);
        assert_eq!(reg.status(&id), Some(SourceStatus::Registered));
        assert!(reg.get(&id).is_none());

        load_normal(&reg, "a", 3);
        assert_eq!(reg.status(&id), Some(SourceStatus::Loaded));
        assert!(reg.get(&id).is_some());

        reg.unload(&id).await.unwrap();
        assert_eq!(reg.status(&id), Some(SourceStatus::Registered));
        assert!(reg.get(&id).is_none());
    }

    #[tokio::test]
    async fn double_register_rejected() {
        let reg = SourceRegistry::new();
        register_fake(&reg, "a", 1, false, None);
        assert!(matches!(
            reg.register(SourceDescriptor {
                info: make_info("a"),
                build: Box::new(|_| Err(CoreError::Config("unused".into()))),
            }),
            Err(CoreError::SourceExists { .. })
        ));
    }

    #[tokio::test]
    async fn search_dedupes_across_sources() {
        let reg = SourceRegistry::new();
        for id in ["a", "b"] {
            let info = make_info(id);
            let tracks: Vec<Track> = (0..5).map(|i| fake_track(id, i, true)).collect();
            let src: Arc<dyn MusicSource> = Arc::new(FakeSource {
                info: info.clone(),
                tracks,
                delay: None,
            });
            register_fake(&reg, id, 5, true, None);
            load_with_instance(&reg, id, src);
        }
        let agg = reg.search_all(&SearchQuery::track("同名的歌"), 1_000).await;
        // 两个源各 5 首同名同艺人的歌 → 去重后只剩 5。
        assert_eq!(agg.tracks.len(), 5);
        assert_eq!(agg.per_source.len(), 2);
        assert_eq!(agg.counts().map(|(_, n)| n).sum::<usize>(), 10);
    }

    #[tokio::test]
    async fn slow_source_does_not_block_others() {
        let reg = SourceRegistry::new();
        for (name, delay) in [("slow", Some(Duration::from_millis(3_000))), ("fast", None)] {
            let info = make_info(name);
            let tracks: Vec<Track> = (0..2).map(|i| fake_track(name, i, false)).collect();
            let src: Arc<dyn MusicSource> = Arc::new(FakeSource {
                info: info.clone(),
                tracks,
                delay,
            });
            reg.register(SourceDescriptor {
                info,
                build: Box::new(move |_| Ok(src.clone())),
            })
            .unwrap();
            load_with_instance(&reg, name, {
                let info = make_info(name);
                let tracks: Vec<Track> = (0..2).map(|i| fake_track(name, i, false)).collect();
                Arc::new(FakeSource {
                    info,
                    tracks,
                    delay,
                })
            });
        }
        let agg = reg.search_all(&SearchQuery::track("x"), 200).await;
        assert_eq!(agg.tracks.len(), 2);
        let slow = agg
            .per_source
            .iter()
            .find(|r| r.source.as_str() == "slow")
            .unwrap();
        assert!(slow.outcome.is_err(), "slow source must time out");
        let fast = agg
            .per_source
            .iter()
            .find(|r| r.source.as_str() == "fast")
            .unwrap();
        assert!(fast.outcome.is_ok());
    }

    #[test]
    fn collection_merge_dedupes_across_sources() {
        use crate::model::{CollectionId, CollectionKind, CollectionSummary};
        let mk = |source: &str, title: &str| CollectionSummary {
            id: CollectionId::new(
                SourceId::new(source),
                CollectionKind::Album,
                format!("{source}-1"),
            ),
            title: title.into(),
            creator: Some("Beyond".into()),
            track_count: Some(10),
            kind: CollectionKind::Album,
            ..Default::default()
        };
        let merged = merge_collections(vec![
            vec![mk("local", "乐与怒"), mk("local", "海阔天空")],
            vec![mk("netease", "乐与怒"), mk("netease", "继续革命")],
        ]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].id.source.as_str(), "local");
        assert_eq!(merged[1].title, "海阔天空");
        assert_eq!(merged[2].title, "继续革命");
    }
}
