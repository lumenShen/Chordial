//! 持久化键值存储。
//!
//! 数据保存在 JSON 文件中，应用重启后数据保留。
//! 内存中维护一份缓存副本，读取不访问磁盘，写入仅修改缓存，
//! 调用 [`save()`](PersistentStore::save) 或 [`save_if_dirty()`](PersistentStore::save_if_dirty)
//! 时才落盘。
//!
//! 所有条目等效于 `Ttl::Forever`（永久保留）。
//!
//! # Blob 二进制存储
//!
//! 除 JSON 键值对外，本存储也支持以文件形式保存二进制数据
//! （通过 [`set_blob`](PersistentStore::set_blob) / [`get_blob`](PersistentStore::get_blob)），
//! 适用于音乐资源（音频、图片、歌词文本）的本地持久化缓存。
//! Blob 文件保存在 JSON 文件同级的 `blobs/` 目录下。
//!
//! # 并发
//!
//! 内部使用 `parking_lot::RwLock` 保护缓存和脏标记，允许多读单写。
//!
//! # 模块划分
//!
//! 单一职责拆分，避免上帝文件：
//!
//! | 子模块 | 职责 |
//! |--------|------|
//! | [`read`] | JSON 键值读取 / 反序列化 |
//! | [`write`] | JSON 键值写入 / 删除 / 清理 |
//! | [`persist`] | 落盘 / 重载 |
//! | [`blob`] | 二进制 Blob 文件存储 |
//!
//! # 示例
//!
//! ```ignore
//! use std::path::PathBuf;
//! use crate::module::storage::persistent::PersistentStore;
//!
//! let path = dirs::config_dir().unwrap().join("myapp").join("config.json");
//! let store = PersistentStore::new(path);
//!
//! // JSON 键值对
//! store.set("volume", &0.75)?;
//! store.save()?;
//!
//! // 二进制数据
//! store.set_blob("song_audio_001", &audio_bytes)?;
//! let audio = store.get_blob("song_audio_001");
//! ```

mod blob;
mod persist;
mod read;
mod write;

use super::backend::StorageBackend;
use super::file::FileBackend;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

/// 持久化键值存储。
pub struct PersistentStore {
    /// JSON 文件后端
    backend: FileBackend,
    /// 内存缓存，避免每次读取都访问磁盘
    cache: RwLock<HashMap<String, Value>>,
    /// 是否有未落盘的修改（原子操作，无锁）
    dirty: AtomicBool,
    /// Blob 文件存储目录
    blob_dir: PathBuf,
    /// 内存中的 Blob key 集合（避免每次扫描目录）
    blob_keys_cache: RwLock<HashSet<String>>,
}

impl PersistentStore {
    /// 创建持久化存储实例。
    ///
    /// 启动时自动从 `path` 指定的 JSON 文件加载已有数据。
    /// 若文件不存在或解析失败，初始化为空数据集。
    ///
    /// # 参数
    ///
    /// - `path`：JSON 配置文件的完整路径。
    pub fn new(path: PathBuf) -> Self {
        let backend = FileBackend::new(path.clone());
        let cache = backend
            .read()
            .unwrap_or_else(|_| HashMap::new());

        // Blob 目录：JSON 文件同级的 blobs/ 目录
        let blob_dir = path
            .parent()
            .map(|p| p.join("blobs"))
            .unwrap_or_else(|| PathBuf::from("blobs"));

        // 确保 Blob 目录存在并加载已有 key
        let _ = fs::create_dir_all(&blob_dir);
        let blob_keys_cache = blob::scan_blob_keys(&blob_dir);

        Self {
            backend,
            cache: RwLock::new(cache),
            dirty: AtomicBool::new(false),
            blob_dir,
            blob_keys_cache: RwLock::new(blob_keys_cache),
        }
    }

    /// 返回持久化文件的路径。
    pub fn path(&self) -> &PathBuf {
        self.backend.path()
    }
}