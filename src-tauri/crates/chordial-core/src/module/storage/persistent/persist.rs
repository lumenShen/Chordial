//! 落盘 / 重载子模块。

use super::PersistentStore;
use crate::module::perf;
use crate::module::storage::backend::StorageBackend;
use std::sync::atomic::Ordering;

impl PersistentStore {
    /// 立即将内存中所有数据写入磁盘。
    ///
    /// 写入成功后清除脏标记。
    ///
    /// 优化：在读锁内序列化为字符串（~10ms for 10k entries），释放锁后写盘，
    /// 避免 `cache.read().clone()` 导致的全量 HashMap clone（O(n) 内存+时间）。
    pub fn save(&self) -> Result<(), String> {
        let _scope = perf::scope("persistent.save");
        let content = {
            let guard = self.cache.read();
            serde_json::to_string(&*guard).map_err(|e| format!("序列化失败: {}", e))?
        };
        self.backend.write_str(&content)?;
        self.dirty.store(false, Ordering::Release);
        Ok(())
    }

    /// 仅当存在未保存修改时才写入磁盘。
    ///
    /// 适合在退出时调用，避免不必要的磁盘 I/O。
    pub fn save_if_dirty(&self) -> Result<(), String> {
        let _scope = perf::scope("persistent.save");
        if self.dirty.load(Ordering::Acquire) {
            self.save()
        } else {
            Ok(())
        }
    }

    /// 从磁盘重新加载数据，**丢弃内存中所有未保存的修改**。
    ///
    /// 加载失败时保持内存数据不变。
    pub fn reload(&self) {
        if let Ok(data) = self.backend.read() {
            *self.cache.write() = data;
            self.dirty.store(false, Ordering::Release);
        }
    }
}