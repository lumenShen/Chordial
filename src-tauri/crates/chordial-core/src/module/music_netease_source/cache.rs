//! 封面 / picUrl 双层缓存辅助。
//!
//! 通过 FIFO 半量淘汰控制内存占用：封面字节缓存上限 128 条（典型 50-500KB/张，
//! 约 6-64MB），picUrl 字符串缓存上限 512 条。

use std::sync::Arc;

/// 封面字节缓存容量上限（典型 50-500KB/张，128 条 ≈ 6-64MB）。
pub(crate) const NETEASE_COVER_CACHE_CAP: usize = 128;
/// picUrl 缓存容量上限（picUrl 是短字符串，可容纳更多）。
pub(crate) const NETEASE_PIC_URL_CACHE_CAP: usize = 512;

impl super::NeteaseCloudMusicApiSource {
    /// 写入封面字节缓存（带 FIFO 半量淘汰）。
    ///
    /// 在锁外预分配 key 字符串和 `Arc` 包装，临界区内仅做 HashMap 查删与插入。
    pub(crate) fn insert_cover_cache(&self, entity_id: &str, data: Vec<u8>) {
        let key = entity_id.to_string();
        let arc = Arc::new(data);
        let mut cache = self.cover_cache.lock();
        if cache.len() >= NETEASE_COVER_CACHE_CAP {
            let drop_count = NETEASE_COVER_CACHE_CAP / 2;
            let keys: Vec<String> = cache.keys().take(drop_count).cloned().collect();
            for k in keys {
                cache.remove(&k);
            }
        }
        cache.insert(key, arc);
    }

    /// 写入 picUrl 缓存（带 FIFO 半量淘汰）。
    pub(crate) fn insert_pic_url_cache(&self, entity_id: &str, pic_url: String) {
        let key = entity_id.to_string();
        let mut cache = self.pic_url_cache.lock();
        if cache.len() >= NETEASE_PIC_URL_CACHE_CAP {
            let drop_count = NETEASE_PIC_URL_CACHE_CAP / 2;
            let keys: Vec<String> = cache.keys().take(drop_count).cloned().collect();
            for k in keys {
                cache.remove(&k);
            }
        }
        cache.insert(key, pic_url);
    }
}