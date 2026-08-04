//! 封面缓存管理 — [`LocalMusicSource`] 的封面三级缓存实现。
//!
//! 三级缓存策略（`album_picture_get`）：
//! 1. 内存缓存命中 → ~0.01ms
//! 2. 磁盘缓存命中 → ~1-5ms（文件读取）
//! 3. symphonia 提取 → 117-296ms（写入磁盘 + 内存）
//!
//! 扫描阶段（`batch_index_files`）通过 `probe_file_with_cover` 一次解析
//! 同时提取元数据与封面，写入磁盘缓存预热，浏览阶段直接命中。

use super::source::LocalMusicSource;
use crate::module::platform::{self, PlatformPath};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

/// 封面缓存容量上限（条目数）。
/// 单条目平均 50-500KB，256 条 ≈ 12-128MB（最坏情况）。
const COVER_CACHE_CAP: usize = 256;

impl LocalMusicSource {
    /// 计算 entity_id 的稳定缓存键（16 位十六进制）。
    ///
    /// 使用 `DefaultHasher`（固定种子，跨运行稳定），不引入额外依赖。
    /// entity_id 通常是音频文件规范路径，不同文件 → 不同键。
    fn cover_cache_key(entity_id: &str) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        entity_id.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// 返回 entity_id 对应的磁盘缓存文件路径。
    fn cover_disk_path(&self, entity_id: &str) -> PathBuf {
        self.cover_disk_cache_dir
            .join(format!("{}.bin", Self::cover_cache_key(entity_id)))
    }

    /// 从磁盘缓存加载封面（命中时 ~1-5ms，远快于 symphonia 提取的 117-296ms）。
    pub(super) fn load_cover_from_disk(&self, entity_id: &str) -> Option<Vec<u8>> {
        let path = self.cover_disk_path(entity_id);
        std::fs::read(&path).ok()
    }

    /// 将封面字节写入磁盘缓存（best-effort，失败仅日志不中断）。
    pub fn save_cover_to_disk(&self, entity_id: &str, data: &[u8]) {
        let path = self.cover_disk_path(entity_id);
        if let Err(e) = std::fs::write(&path, data) {
            eprintln!(
                "[local_source] 保存封面到磁盘缓存失败 '{}': {}",
                path.display(),
                e
            );
        }
    }

    /// 清空封面磁盘缓存（维护工具调用）。
    pub fn clear_cover_disk_cache(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.cover_disk_cache_dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// 写入内存缓存（含 FIFO 半量淘汰）。
    ///
    /// 在锁外预分配 key 字符串和 `Arc` 包装，临界区内仅做 HashMap 查删与插入。
    pub(super) fn insert_cover_cache(&self, entity_id: &str, data: Vec<u8>) {
        let key = entity_id.to_string();
        let arc = Arc::new(data);
        let mut cache = self.cover_cache.lock();
        if cache.len() >= COVER_CACHE_CAP {
            let drop_count = COVER_CACHE_CAP / 2;
            let keys: Vec<String> = cache.keys().take(drop_count).cloned().collect();
            for k in keys {
                cache.remove(&k);
            }
        }
        cache.insert(key, arc);
    }

    /// 从磁盘提取专辑封面字节（无缓存）。
    ///
    /// 提取顺序：
    /// 1. 音频文件嵌入封面（FLAC / ID3v2）— symphonia 解析
    /// 2. 同目录同名图片（.jpg/.png/.webp/.bmp）
    /// 3. 目录下常见封面名（cover/folder/albumart/front）
    pub(super) fn extract_album_picture(&self, path: &PlatformPath) -> Result<Vec<u8>, String> {
        // 1. 若为音频文件，尝试提取嵌入封面（FLAC / ID3v2）
        if platform::is_file(path) && super::scanner::is_supported_audio(path) {
            if let Ok(cover_data) = super::scanner::extract_cover_art(path) {
                return Ok(cover_data);
            }
        }

        // 2. 确定搜索目录
        let search_dir = if platform::is_dir(path) {
            path.clone()
        } else if let Some(parent) = platform::path_parent(path) {
            parent
        } else {
            return Err(format!("无法确定搜索目录: {}", platform::path_to_string(path)));
        };

        // 3. 同目录下与音频文件同名的图片文件
        if !platform::is_dir(path) {
            for ext in &["jpg", "jpeg", "png", "webp", "bmp"] {
                let sibling = platform::path_with_extension(path, ext);
                if platform::exists(&sibling) {
                    return platform::read_bytes(&sibling)
                        .map_err(|e| format!("读取封面文件失败 '{}': {}", platform::path_to_string(&sibling), e));
                }
            }
        }

        // 4. 目录下常见封面文件名
        let candidates = [
            platform::path_join(&search_dir, "cover.jpg"),
            platform::path_join(&search_dir, "cover.png"),
            platform::path_join(&search_dir, "folder.jpg"),
            platform::path_join(&search_dir, "folder.png"),
            platform::path_join(&search_dir, "albumart.jpg"),
            platform::path_join(&search_dir, "albumart.png"),
            platform::path_join(&search_dir, "front.jpg"),
            platform::path_join(&search_dir, "front.png"),
        ];
        for candidate in &candidates {
            if platform::exists(candidate) {
                return platform::read_bytes(candidate)
                    .map_err(|e| format!("读取封面文件失败 '{}': {}", platform::path_to_string(candidate), e));
            }
        }

        Err(format!("未找到封面图片: {}", platform::path_to_string(path)))
    }
}