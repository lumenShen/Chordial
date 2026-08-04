//! 持久化 — save / reload / clear 等落盘与清空操作。

use super::MusicLibrary;

impl MusicLibrary {
    /// 立即将所有未落盘的修改写入磁盘。
    pub fn save(&self) -> Result<(), String> {
        self.store.save()
    }

    /// 仅当存在未保存修改时才写入磁盘。
    pub fn save_if_dirty(&self) -> Result<(), String> {
        self.store.save_if_dirty()
    }

    /// 从磁盘重新加载，丢弃所有未保存的修改。
    pub fn reload(&self) {
        self.store.reload();
        self.bump_version();
    }

    /// 返回持久化存储的文件路径。
    pub fn store_path(&self) -> &std::path::PathBuf {
        self.store.path()
    }

    /// 从内存缓存中删除指定 key（标记脏，下次 save 时落盘）。
    pub fn remove_store_key(&self, key: &str) {
        self.store.remove(key);
        self.bump_version();
    }

    /// 清空整个音乐库（Song / Artist / Album / Lyric）。
    ///
    /// 直接调用 `PersistentStore::clear` 一次性清掉所有 key，
    /// 比 `remove_store_key` 逐个删除快得多。
    /// 调用后需 `save()` 落盘。
    pub fn clear_all(&self) {
        self.store.clear();
        self.bump_version();
    }
}