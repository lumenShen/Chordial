//! 文件索引维护 — [`LocalMusicSource`] 的纯索引方法。
//!
//! 按单一职责拆分为四个原子子模块：
//! - `single`：单文件索引增删改查（`index_file` / `unindex_file` / `reindex_file`）
//! - `batch`：批量处理（`batch_index_files` / `batch_unindex_files`）
//! - `restore`：索引重建（`clear_all_index` / `restore_index_from_library`）
//! - `mtime`：mtime 缓存（`check_file_unchanged` / `update_file_mtime` / `save_mtime_cache`）

mod batch;
mod mtime;
mod restore;
mod single;