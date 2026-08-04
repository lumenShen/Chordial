//! JSON 键值写入 / 删除 / 清理子模块。

use super::PersistentStore;
use crate::module::perf;
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::Ordering;

impl PersistentStore {
    /// 按 ID 从 HashMap 值中删除单条记录，返回是否命中。
    ///
    /// **性能关键**：直接操作 JSON Object 的键，避免反序列化整个 HashMap。
    /// 对 3853 条 `albums.remove(id)` 旧实现需 24ms（反序列化整表），
    /// 本方法 ~0.01ms。
    pub fn remove_entry(&self, key: &str, id: &str) -> bool {
        let _scope = perf::scope("persistent.remove_entry");
        let mut guard = self.cache.write();
        let removed = guard
            .get_mut(key)
            .and_then(|v| v.as_object_mut())
            .map_or(false, |obj| obj.remove(id).is_some());
        if removed {
            self.dirty.store(true, Ordering::Release);
        }
        removed
    }

    /// 按 ID 插入/覆盖 HashMap 值中的单条记录。
    ///
    /// **性能关键**：直接序列化目标条目并插入 JSON Object，
    /// 避免反序列化整个 HashMap → 修改 → 重新序列化。
    /// 旧 `add`/`update` 对 3853 条 albums 全量重写需 ~50ms，
    /// 本方法 ~0.1ms。
    pub fn set_subkey<T: Serialize>(&self, key: &str, id: &str, value: &T) -> Result<(), String> {
        let _scope = perf::scope("persistent.set_subkey");
        let json = serde_json::to_value(value).map_err(|e| format!("序列化失败: {}", e))?;
        let mut guard = self.cache.write();
        // 键不存在时自动创建为空 JSON Object，与 `set()` 语义一致。
        // 旧实现要求调用方先 `set(key, &HashMap::new())` 初始化，
        // 这对首次写入 lyrics / blobs 等冷键的场景是个陷阱
        // （典型表现：`键 'lyrics' 不存在` 错误，歌词写入静默失败）。
        let entry = guard
            .entry(key.to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(obj) = entry.as_object_mut() {
            obj.insert(id.to_string(), json);
            drop(guard);
            self.dirty.store(true, Ordering::Release);
            Ok(())
        } else {
            Err(format!("键 '{}' 的值不是 JSON Object", key))
        }
    }

    /// 写入键值对。
    ///
    /// 仅修改内存缓存，**不立即落盘**。需要调用 [`save()`](Self::save)
    /// 或 [`save_if_dirty()`](Self::save_if_dirty) 手动持久化。
    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<(), String> {
        let _scope = perf::scope("persistent.set");
        let json = serde_json::to_value(value)
            .map_err(|e| format!("序列化失败: {}", e))?;
        self.cache.write().insert(key.to_string(), json);
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }

    /// 写入原始 JSON 值，仅修改内存缓存。
    pub fn set_raw(&self, key: &str, value: Value) {
        self.cache.write().insert(key.to_string(), value);
        self.dirty.store(true, Ordering::Release);
    }

    /// 删除指定 key，返回 `true` 表示 key 存在并被删除。
    pub fn remove(&self, key: &str) -> bool {
        let existed = self.cache.write().remove(key).is_some();
        if existed {
            self.dirty.store(true, Ordering::Release);
        }
        existed
    }

    /// 检查 key 是否存在。
    pub fn has(&self, key: &str) -> bool {
        self.cache.read().contains_key(key)
    }

    /// 获取所有已存储 key 的列表。
    pub fn keys(&self) -> Vec<String> {
        self.cache.read().keys().cloned().collect()
    }

    /// 清空所有数据（仅修改内存缓存）。
    pub fn clear(&self) {
        self.cache.write().clear();
        self.dirty.store(true, Ordering::Release);
    }
}