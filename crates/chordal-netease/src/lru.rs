//! 定容量 LRU（由 lib.rs 内联模块拆出，行为不变）。

//! 定容量 LRU：HashMap + 时间戳惰性淘汰队列，get/put 均摊 O(1)。

use std::collections::{HashMap, VecDeque};

pub struct LruCache<K, V> {
    map: HashMap<K, (V, u64)>,
    order: VecDeque<(K, u64)>,
    capacity: usize,
    tick: u64,
}

impl<K, V> LruCache<K, V>
where
    K: std::hash::Hash + Eq + Clone,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
            tick: 0,
        }
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.tick += 1;
        let entry = self.map.get_mut(key)?;
        entry.1 = self.tick;
        self.order.push_back((key.clone(), self.tick));
        Some(&entry.0)
    }

    pub fn put(&mut self, key: K, value: V) {
        self.tick += 1;
        if self.map.len() >= self.capacity && !self.map.contains_key(&key) {
            // 惰性淘汰：从队首弹出已过期（时间戳仍是最后一次 push 时）的键。
            while let Some((old_key, old_stamp)) = self.order.pop_front() {
                match self.map.get(&old_key) {
                    Some((_, stamp)) if *stamp == old_stamp => {
                        self.map.remove(&old_key);
                        break;
                    }
                    _ => continue,
                }
            }
        }
        self.map.insert(key.clone(), (value, self.tick));
        self.order.push_back((key, self.tick));
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eviction_is_bounded_and_lru() {
        let mut c = LruCache::new(2);
        c.put("a", 1);
        c.put("b", 2);
        assert_eq!(c.get(&"a"), Some(&1)); // a 变最近使用
        c.put("c", 3); // 淘汰 b（最久未用）
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(&"b"), None);
        assert_eq!(c.get(&"a"), Some(&1));
        assert_eq!(c.get(&"c"), Some(&3));
    }

    #[test]
    fn repeated_access_keeps_capacity_bounded() {
        let mut c = LruCache::new(16);
        for i in 0..1000u32 {
            c.put(format!("k{i}"), i);
        }
        assert_eq!(c.len(), 16);
        for _ in 0..10_000 {
            let _ = c.get(&"k999".to_string());
        }
        assert_eq!(c.len(), 16);
    }
}
