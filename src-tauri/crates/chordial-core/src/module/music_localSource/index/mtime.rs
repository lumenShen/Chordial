//! mtime 缓存 — 文件变化检测与持久化。
//!
//! 负责 `check_file_unchanged` / `update_file_mtime` / `save_mtime_cache`。
//! 通过对比文件 mtime+size 与缓存，启动时跳过未变化文件的重扫描，
//! 避免对每个文件重复 symphonia 探测。

use super::super::source::LocalMusicSource;
use crate::module::platform::{self, PlatformPath};

impl LocalMusicSource {
    /// 检查文件 mtime+size 是否与缓存一致，返回缓存的 song_id（若未变化）。
    ///
    /// `path` 应为已规范化的路径。
    pub fn check_file_unchanged(&self, path: &PlatformPath) -> Option<String> {
        let path_str = platform::path_to_string(path);
        let mtimes = self.file_mtimes.read();
        let (cached_mtime, cached_size, song_id) = mtimes.get(&path_str)?;

        let current_mtime = platform::file_modified_secs(
            &PlatformPath::from(path_str.as_str()),
        )
        .ok()?;
        let current_size = platform::file_size(
            &PlatformPath::from(path_str.as_str()),
        )
        .ok()?;

        if current_mtime == *cached_mtime && current_size == *cached_size {
            Some(song_id.clone())
        } else {
            None
        }
    }

    /// 更新文件的 mtime 缓存条目。
    ///
    /// `path` 应为已规范化的路径。
    pub fn update_file_mtime(&self, path: &PlatformPath, song_id: &str) {
        let path_str = platform::path_to_string(path);
        let platform_path = PlatformPath::from(path_str.as_str());
        if let (Ok(mtime), Ok(size)) = (
            platform::file_modified_secs(&platform_path),
            platform::file_size(&platform_path),
        ) {
            self.file_mtimes.write().insert(
                path_str,
                (mtime, size, song_id.to_string()),
            );
        }
    }

    /// 将 mtime 缓存持久化到独立存储（与 library 分离，避免每次保存都序列化全部歌曲）。
    pub fn save_mtime_cache(&self) -> Result<(), String> {
        let mtimes = self.file_mtimes.read().clone();
        self.mtime_store.set("file_mtimes", &mtimes)?;
        self.mtime_store.save()
    }
}