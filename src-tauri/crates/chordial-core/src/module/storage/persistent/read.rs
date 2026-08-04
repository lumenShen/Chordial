//! JSON 键值读取 / 反序列化子模块。

use super::PersistentStore;
use crate::module::perf;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

impl PersistentStore {
    /// 读取值并反序列化为目标类型 `T`。
    ///
    /// key 不存在或类型不匹配时返回 `None`。
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        let _scope = perf::scope("persistent.get");
        self.cache
            .read()
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// 读取原始 [`serde_json::Value`]，不做反序列化。
    pub fn get_raw(&self, key: &str) -> Option<Value> {
        self.cache.read().get(key).cloned()
    }

    /// 按 ID 从 HashMap 值中读取单条记录并反序列化。
    ///
    /// **性能关键**：仅反序列化目标条目，不反序列化整个 HashMap。
    /// 对 3853 张专辑的库，`get::<HashMap<..>>(key).remove(id)` 需 24ms，
    /// 本方法仅需 ~0.1ms。
    ///
    /// 要求存储的 Value 为 JSON Object（即 `{"id1": {...}, "id2": {...}}`）。
    pub fn get_entry<T: for<'de> Deserialize<'de>>(&self, key: &str, id: &str) -> Option<T> {
        let _scope = perf::scope("persistent.get_entry");
        let guard = self.cache.read();
        let value = guard.get(key)?;
        let obj = value.as_object()?;
        let entry_value = obj.get(id)?;
        serde_json::from_value(entry_value.clone()).ok()
    }

    /// 按 ID 检查条目是否存在（不反序列化）。
    ///
    /// 比 `get_entry::<T>(...).is_some()` 更快：不分配反序列化缓冲。
    pub fn has_entry(&self, key: &str, id: &str) -> bool {
        let guard = self.cache.read();
        guard
            .get(key)
            .and_then(|v| v.as_object())
            .map_or(false, |obj| obj.contains_key(id))
    }

    /// 获取 HashMap 值的条目数量，不做反序列化。
    ///
    /// 仅检查 JSON Object 的键数量，开销 O(1)。
    pub fn count_entries(&self, key: &str) -> usize {
        let _scope = perf::scope("persistent.count_entries");
        self.cache
            .read()
            .get(key)
            .and_then(|v| v.as_object())
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// 从 HashMap 值中分页读取并反序列化。
    ///
    /// 仅反序列化 `[offset, offset+limit)` 范围的条目，
    /// 不反序列化整个 HashMap。适合分页列表场景。
    ///
    /// 注意：JSON Object 的迭代顺序由 serde_json 保证（插入序），
    /// 与 `get_all().into_values().skip().take()` 行为一致。
    pub fn get_page_entries<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
        offset: usize,
        limit: usize,
    ) -> Vec<T> {
        let _scope = perf::scope("persistent.get_page_entries");
        let guard = self.cache.read();
        let value = match guard.get(key) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let obj = match value.as_object() {
            Some(o) => o,
            None => return Vec::new(),
        };
        obj.values()
            .skip(offset)
            .take(limit)
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect()
    }

    /// 从 HashMap 值中读取所有条目并反序列化。
    ///
    /// 比 `get::<HashMap<String, T>>` 更高效：
    /// 直接遍历 Object values 逐条反序列化，
    /// 避免 `serde_json::from_value::<HashMap<String, T>>` 的中间 HashMap 分配。
    pub fn get_all_entries<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Vec<T> {
        let _scope = perf::scope("persistent.get_all_entries");
        let guard = self.cache.read();
        let value = match guard.get(key) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let obj = match value.as_object() {
            Some(o) => o,
            None => return Vec::new(),
        };
        let result = Vec::with_capacity(obj.len());
        obj.values()
            .fold(result, |mut acc, v| {
                if let Ok(item) = serde_json::from_value::<T>(v.clone()) {
                    acc.push(item);
                }
                acc
            })
    }

    /// 从 HashMap 值中读取所有条目，返回 `HashMap<String, T>`。
    ///
    /// 与 `get::<HashMap<String, T>>` 功能相同但更直接：
    /// 逐条反序列化，失败条目跳过而非整体失败。
    pub fn get_all_map<T: for<'de> Deserialize<'de>>(&self, key: &str) -> HashMap<String, T> {
        let _scope = perf::scope("persistent.get_all_map");
        let guard = self.cache.read();
        let value = match guard.get(key) {
            Some(v) => v,
            None => return HashMap::new(),
        };
        let obj = match value.as_object() {
            Some(o) => o,
            None => return HashMap::new(),
        };
        let mut result = HashMap::with_capacity(obj.len());
        for (k, v) in obj {
            if let Ok(item) = serde_json::from_value::<T>(v.clone()) {
                result.insert(k.clone(), item);
            }
        }
        result
    }

    /// 按 JSON 谓词过滤条目，仅反序列化匹配项。
    ///
    /// **性能关键**：谓词作用于原始 [`Value`]（无反序列化开销），
    /// 仅匹配条目才执行 `serde_json::from_value::<T>`。
    ///
    /// 例：`get_albums_by_artist` 对 3853 张专辑按 `artist_id` 过滤，
    /// 若艺术家有 10 张专辑，仅反序列化 10 条而非全部 3853 条。
    /// 旧 24ms → ~0.5ms。
    pub fn get_entries_filtered<T, F>(&self, key: &str, predicate: F) -> Vec<T>
    where
        T: for<'de> Deserialize<'de>,
        F: Fn(&Value) -> bool,
    {
        let _scope = perf::scope("persistent.get_entries_filtered");
        let guard = self.cache.read();
        let value = match guard.get(key) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let obj = match value.as_object() {
            Some(o) => o,
            None => return Vec::new(),
        };
        obj.values()
            .filter(|v| predicate(v))
            .filter_map(|v| serde_json::from_value::<T>(v.clone()).ok())
            .collect()
    }

    /// 按 JSON 字符串字段过滤条目（等值匹配）。
    ///
    /// 等价于 `get_entries_filtered` 但针对 `{"field": "value"}` 模式特化，
    /// 避免每条都调用闭包——直接 `value[field].as_str() == Some(target)`。
    pub fn get_entries_by_str_field<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
        field: &str,
        target: &str,
    ) -> Vec<T> {
        let _scope = perf::scope("persistent.get_entries_by_str_field");
        let guard = self.cache.read();
        let value = match guard.get(key) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let obj = match value.as_object() {
            Some(o) => o,
            None => return Vec::new(),
        };
        obj.values()
            .filter(|v| v.get(field).and_then(|f| f.as_str()) == Some(target))
            .filter_map(|v| serde_json::from_value::<T>(v.clone()).ok())
            .collect()
    }

    /// 按 JSON 字符串数组字段过滤条目（包含匹配）。
    ///
    /// 适用于 `{"field": ["id1", "id2"]}` 模式，检查数组是否包含 `target`。
    /// 用于 `Song.artist_ids` 等。
    pub fn get_entries_by_str_array_contains<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
        field: &str,
        target: &str,
    ) -> Vec<T> {
        let _scope = perf::scope("persistent.get_entries_by_str_array_contains");
        let guard = self.cache.read();
        let value = match guard.get(key) {
            Some(v) => v,
            None => return Vec::new(),
        };
        let obj = match value.as_object() {
            Some(o) => o,
            None => return Vec::new(),
        };
        obj.values()
            .filter(|v| {
                v.get(field)
                    .and_then(|f| f.as_array())
                    .map_or(false, |arr| {
                        arr.iter().any(|x| x.as_str() == Some(target))
                    })
            })
            .filter_map(|v| serde_json::from_value::<T>(v.clone()).ok())
            .collect()
    }
}