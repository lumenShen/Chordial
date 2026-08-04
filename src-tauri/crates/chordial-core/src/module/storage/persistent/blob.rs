//! 二进制 Blob 文件存储子模块。
//!
//! Blob 文件保存在 JSON 文件同级的 `blobs/` 目录下，
//! 文件名基于 key 的哈希值（`{hash}.blob`）。

use super::PersistentStore;
use crate::module::perf;
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// 扫描 Blob 目录，返回已有文件的 key 集合。
///
/// 由于文件名基于哈希（无法反推原始 key），启动时集合为空。
/// 仅维护由本次进程写入的 key。
pub(super) fn scan_blob_keys(blob_dir: &Path) -> HashSet<String> {
    let keys = HashSet::new();
    if let Ok(entries) = fs::read_dir(blob_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if name.ends_with(".blob") {
                // 文件名格式：{hash}.blob，无法反推出原始 key。
                // 此处仅统计文件数量，key 集合通过 set_blob 时注入。
            }
        }
    }
    // 无法从哈希文件名反推原始 key，启动时集合为空。
    // has_blob 会直接检查文件是否存在，因此这里不维护 key 集合也可以。
    // 我们只维护由本次进程写入的 key。
    keys
}

/// 将 key 转换为安全的文件名。
fn key_to_filename(key: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}.blob", hasher.finish())
}

impl PersistentStore {
    /// 获取 Blob 文件的完整路径。
    fn blob_path(&self, key: &str) -> PathBuf {
        self.blob_dir.join(key_to_filename(key))
    }

    /// 写入二进制数据到 Blob 文件（立即落盘）。
    ///
    /// Blob 文件保存在 JSON 文件同级的 `blobs/` 目录下，
    /// 文件名基于 key 的哈希值。
    ///
    /// 若 key 对应文件已存在则覆盖。
    pub fn set_blob(&self, key: &str, data: &[u8]) -> Result<(), String> {
        let _scope = perf::scope("persistent.set_blob");
        let path = self.blob_path(key);
        fs::write(&path, data).map_err(|e| format!("写入 Blob 文件失败: {}", e))?;
        self.blob_keys_cache.write().insert(key.to_string());
        Ok(())
    }

    /// 读取 Blob 文件。
    ///
    /// 若 key 不存在或文件丢失则返回 `None`。
    pub fn get_blob(&self, key: &str) -> Option<Vec<u8>> {
        let _scope = perf::scope("persistent.get_blob");
        let path = self.blob_path(key);
        if !path.exists() {
            return None;
        }
        fs::read(&path).ok()
    }

    /// 读取 Blob 文件的路径（不读入内存）。
    ///
    /// 适用于需要流式读取或由其他组件直接访问文件的场景。
    /// 返回 `Some(path)` 仅当文件确实存在。
    pub fn get_blob_path(&self, key: &str) -> Option<PathBuf> {
        let path = self.blob_path(key);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// 删除指定的 Blob 文件。
    ///
    /// 返回 `true` 表示文件存在并被删除。
    pub fn remove_blob(&self, key: &str) -> bool {
        self.blob_keys_cache.write().remove(key);
        let path = self.blob_path(key);
        if path.exists() {
            let _ = fs::remove_file(&path);
            true
        } else {
            false
        }
    }

    /// 检查 Blob key 是否存在（文件存在性检查）。
    pub fn has_blob(&self, key: &str) -> bool {
        self.blob_path(key).exists()
    }

    /// 获取当前内存中已知的 Blob key 列表。
    ///
    /// 注意：这仅包含本次进程通过 [`set_blob`](Self::set_blob) 写入的 key，
    /// 以及启动时扫描到的 key。若外部程序修改了 Blob 目录，可能不一致。
    /// 对于可靠性需求，请使用 [`has_blob`](Self::has_blob) 逐 key 检查。
    pub fn blob_keys(&self) -> Vec<String> {
        self.blob_keys_cache.read().iter().cloned().collect()
    }

    /// 清空所有 Blob 文件。
    pub fn clear_blobs(&self) {
        let keys: Vec<String> = self.blob_keys_cache.read().iter().cloned().collect();
        for key in &keys {
            let path = self.blob_path(key);
            let _ = fs::remove_file(&path);
        }
        self.blob_keys_cache.write().clear();
    }

    /// 返回 Blob 存储目录路径。
    pub fn blob_dir(&self) -> &PathBuf {
        &self.blob_dir
    }
}