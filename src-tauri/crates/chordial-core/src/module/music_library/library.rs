//! 音乐库 — 所有音乐实体的统一管理入口。
//!
//! 本文件仅保留结构体定义与核心状态（版本号、搜索索引缓存），
//! 具体 CRUD / 搜索 / 持久化逻辑按单一职责拆分到 `library/` 子模块：
//!
//! | 子模块 | 职责 |
//! |--------|------|
//! | [`songs`]     | Song 增删改查 + 搜索 |
//! | [`artists`]   | Artist 增删改查 + 搜索 |
//! | [`albums`]    | Album 增删改查 + 搜索 + 首页统计 |
//! | [`lyrics`]    | Lyric 增删改查 + 搜索 |
//! | [`relations`] | 跨实体关系追溯 + 来源 ID |
//! | [`batch`]     | 批量添加歌曲（智能去重合并） |
//! | [`persistence`] | 持久化（save / reload / clear） |
//! | [`search`]    | 统一搜索引擎入口（trigram 倒排索引） |
//! | [`source_cleanup`] | `SourceCleanup` trait 实现 |
//!
//! 内部持有 [`PersistentStore`]，启动时自动加载已有数据，
//! 并通过各子模块提供 CRUD、搜索和关系追溯功能。

use crate::module::storage::persistent::PersistentStore;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub mod albums;
pub mod artists;
pub mod batch;
pub mod lyrics;
pub mod persistence;
pub mod relations;
pub mod search;
pub mod songs;
pub mod source_cleanup;

/// 音乐库 — 所有音乐实体的统一管理入口。
pub struct MusicLibrary {
    pub(crate) store: PersistentStore,
    /// 库版本号 — 任何写操作递增，用于搜索索引失效检测。
    version: AtomicU64,
    /// 搜索索引缓存 — 首次查询时构建，写操作使其失效。
    /// `Arc<SearchIndex>` 允许并发查询无锁读取。
    search_index: RwLock<Option<Arc<super::search::SearchIndex>>>,
}

impl MusicLibrary {
    /// 创建音乐库实例，从 `path` 指定的 JSON 文件加载已有数据。
    pub fn new(path: PathBuf) -> Self {
        Self {
            store: PersistentStore::new(path),
            version: AtomicU64::new(0),
            search_index: RwLock::new(None),
        }
    }

    /// 返回当前库版本号（每次写操作后递增）。
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    /// 标记搜索索引失效，下次查询时重建。
    /// 由所有写操作调用。
    pub(crate) fn bump_version(&self) {
        self.version.fetch_add(1, Ordering::Release);
        // 直接丢弃缓存的索引；正在使用旧索引的查询线程仍持有 Arc，安全继续。
        *self.search_index.write() = None;
    }

    /// 强制使搜索索引失效（外部数据变更时调用，如 reload 后）。
    pub fn invalidate_search_index(&self) {
        self.bump_version();
    }

    /// 获取或构建缓存的搜索索引（按版本号校验）。
    ///
    /// 临界区仅做版本比较与 `Arc::clone`，构建在临界区外完成
    /// （构建期间不阻塞其他读线程，仅最后写入时短暂加锁）。
    fn get_or_build_search_index(&self) -> Arc<super::search::SearchIndex> {
        // 快速路径：读锁检查已有缓存且版本匹配
        {
            let guard = self.search_index.read();
            if let Some(arc) = guard.as_ref() {
                if arc.version() == self.version() {
                    return Arc::clone(arc);
                }
            }
        }

        // 慢速路径：构建新索引。构建在锁外完成，避免阻塞其他读者。
        let new_index = Arc::new(super::search::SearchIndex::build(&self.store, self.version()));

        // 写入缓存。若期间另一线程已抢先构建并写入，丢弃本线程结果即可。
        // 双重检查版本号，避免覆盖更新鲜的索引。
        let mut guard = self.search_index.write();
        if let Some(existing) = guard.as_ref() {
            if existing.version() == self.version() {
                return Arc::clone(existing);
            }
        }
        *guard = Some(Arc::clone(&new_index));
        new_index
    }
}