//! 磁盘对象缓存池。
//!
//! 设计目标（与 Chordial 的多源模型绑定）：
//!
//! - **按源隔离**：key 形如 `{source}/{kind}/{id}`，卸载源时
//!   [`ObjectCache::purge_namespace`] 一次性回收该源全部对象。
//! - **容量硬上限**：默认 5 GiB，超限按 `last_access` LRU 淘汰；
//!   显式下载的文件不进入本缓存（见 UI 层 DownloadStore）。
//! - **内容特征比对**：写入前计算 FNV-1a 内容摘要。云端刷新回来若摘要
//!   未变，[`ObjectCache::put`] 返回 [`PutOutcome::Unchanged`] 并只更新
//!   元信息，不重写磁盘对象。
//! - **stale-while-revalidate 友好**：`get` 不关心新鲜度，调用方先拿旧值
//!   立即展示；是否刷新、是否 304 由调用方决定。`etag` / `last_modified`
//!   / `expires_at` 随元信息持久化。
//!
//! 索引是单个 `index.json`（原子替换），对象文件按 key 摘要分两级目录。
//! 对象本体不解析、不压缩：缓存层是字节池，类型由调用方决定。
//!
//! 性能约定：
//! - `get` / `put` 均摊 O(1)（HashMap + 一次文件 IO）；
//! - 淘汰仅在超限时触发，按访问时间排序 O(n log n)，且一次淘汰到 90%；
//! - 普通读写不重写索引，只有结构变化（put/remove/purge/限容）才落盘；
//!   UI 退出前调用 [`ObjectCache::flush`]。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// 默认缓存上限：5 GiB。
pub const DEFAULT_LIMIT_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// 缓存对象元信息。持久化在 `index.json`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    /// 逻辑 key，如 `netease/lyric/123456`。
    pub key: String,
    /// 相对 root 的对象文件路径。
    pub file: String,
    pub size: u64,
    /// 内容 FNV-1a 摘要（用于“未变化不重写”）。
    pub hash: u64,
    /// HTTP 条件请求标签（可选）。
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    /// 写入/刷新时间（Unix 秒）。
    pub fetched_at: u64,
    /// 最近访问时间（Unix 秒），LRU 依据。
    pub last_access: u64,
    /// 业务过期时间（Unix 秒）；仅直链等时效对象使用。
    #[serde(default)]
    pub expires_at: Option<u64>,
}

impl ObjectMeta {
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at.is_some_and(|at| at <= now)
    }
}

/// 一次读取结果：对象字节 + 元信息。
#[derive(Debug, Clone)]
pub struct CachedObject {
    pub bytes: Vec<u8>,
    pub meta: ObjectMeta,
}

/// 写入时附带的元信息。
#[derive(Debug, Clone, Default)]
pub struct CacheTags {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutOutcome {
    /// 首次写入。
    Inserted,
    /// 内容摘要变化，已替换。
    Replaced,
    /// 摘要与旧值一致，只更新元信息，未重写对象。
    Unchanged,
    /// 单个对象超过缓存上限，拒绝写入（不驱逐其它对象）。
    TooLarge,
    /// 写入失败（IO/锁）。
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub entries: u64,
    pub total_bytes: u64,
    pub limit_bytes: u64,
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub root: PathBuf,
    pub limit_bytes: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            root: default_cache_root(),
            limit_bytes: DEFAULT_LIMIT_BYTES,
        }
    }
}

pub fn default_cache_root() -> PathBuf {
    // Android 等没有 XDG 缓存目录的平台，由 JNI 注入应用私有目录。
    if let Some(root) = std::env::var_os("CHORDIAL_CACHE_DIR") {
        return PathBuf::from(root);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chordial")
        .join("objects")
}

/// 源/对象 key 构造。namespace 中的 `/` 会被替换，避免越界目录。
pub fn object_key(source: &str, kind: &str, id: &str) -> String {
    format!(
        "{}/{}/{}",
        sanitize_segment(source),
        sanitize_segment(kind),
        id
    )
}

fn sanitize_segment(s: &str) -> String {
    s.chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    entries: FxHashMap<String, ObjectMeta>,
}

struct Inner {
    root: PathBuf,
    limit: u64,
    total: u64,
    entries: FxHashMap<String, ObjectMeta>,
}

/// 线程安全的磁盘对象缓存。
pub struct ObjectCache {
    inner: RwLock<Inner>,
    /// 写操作与索引刷盘共用一把锁，避免并发 flush 使用同一临时文件。
    mutation: Mutex<()>,
}

impl std::fmt::Debug for ObjectCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectCache").finish_non_exhaustive()
    }
}

impl ObjectCache {
    /// 打开（或创建）缓存目录并载入索引。
    pub fn open(config: CacheConfig) -> io::Result<Self> {
        fs::create_dir_all(&config.root)?;
        let index = load_index(&config.root);
        let mut entries = index.entries;
        // 索引与磁盘对账：缺失/尺寸不符的对象直接丢弃，避免返回破损数据。
        entries.retain(|_, meta| {
            let path = config.root.join(&meta.file);
            fs::metadata(&path)
                .map(|m| m.is_file() && m.len() == meta.size)
                .unwrap_or(false)
        });
        let total = entries.values().map(|m| m.size).sum();
        let cache = Self {
            inner: RwLock::new(Inner {
                root: config.root,
                limit: config.limit_bytes,
                total,
                entries,
            }),
            mutation: Mutex::new(()),
        };
        cache.evict_if_needed();
        Ok(cache)
    }

    /// 读取并更新 LRU 访问时间。文件读取不持有索引锁；返回的 bytes/meta
    /// 始终来自同一个快照，即使期间发生替换或重配置。
    pub fn get(&self, key: &str) -> Option<CachedObject> {
        let (meta, path) = {
            let inner = self.inner.read().ok()?;
            let meta = inner.entries.get(key)?.clone();
            let path = inner.root.join(&meta.file);
            (meta, path)
        };
        let bytes = match fs::read(&path) {
            Ok(bytes) if bytes.len() as u64 == meta.size => bytes,
            _ => {
                if let Ok(mut inner) = self.inner.write()
                    && inner.entries.get(key).is_some_and(|current| {
                        current.hash == meta.hash
                            && current.file == meta.file
                            && current.size == meta.size
                    })
                {
                    inner.entries.remove(key);
                    inner.total = inner.total.saturating_sub(meta.size);
                }
                return None;
            }
        };
        let now = now_secs();
        if let Ok(mut inner) = self.inner.write()
            && let Some(entry) = inner.entries.get_mut(key)
            && entry.hash == meta.hash
            && entry.file == meta.file
            && entry.size == meta.size
        {
            entry.last_access = now;
            return Some(CachedObject {
                bytes,
                meta: entry.clone(),
            });
        }
        Some(CachedObject { bytes, meta })
    }

    /// 只读取元信息，不读对象、不更新访问时间。
    pub fn meta(&self, key: &str) -> Option<ObjectMeta> {
        self.inner.read().ok()?.entries.get(key).cloned()
    }

    /// 读 JSON 对象。
    pub fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let obj = self.get(key)?;
        serde_json::from_slice(&obj.bytes).ok()
    }

    /// 写入对象。内容摘要未变时返回 [`PutOutcome::Unchanged`]。
    pub fn put(&self, key: &str, bytes: &[u8], tags: CacheTags) -> PutOutcome {
        let Ok(_mutation) = self.mutation.lock() else {
            return PutOutcome::Failed;
        };
        let hash = fnv1a64(bytes);
        let now = now_secs();
        let (root, previous, unchanged) = {
            let Ok(mut inner) = self.inner.write() else {
                return PutOutcome::Failed;
            };
            if inner.limit > 0 && bytes.len() as u64 > inner.limit {
                return PutOutcome::TooLarge;
            }
            if inner.entries.get(key).is_some_and(|old| old.hash == hash) {
                let previous = inner.entries.get(key).cloned();
                let entry = inner.entries.get_mut(key).expect("checked");
                entry.fetched_at = now;
                entry.last_access = now;
                merge_tags(entry, tags.clone());
                (inner.root.clone(), previous, true)
            } else {
                (inner.root.clone(), inner.entries.get(key).cloned(), false)
            }
        };
        if unchanged {
            self.flush_locked();
            return PutOutcome::Unchanged;
        }

        // 内容寻址文件路径：替换时旧 meta 仍指向旧文件，并发 get 不会读到
        // “旧元信息 + 新字节”。写完并提交索引后才删除旧对象。
        let rel = rel_path(key, hash);
        let abs = root.join(&rel);
        if chordal_core::fs::atomic_write(&abs, bytes).is_err() {
            return PutOutcome::Failed;
        }

        {
            let Ok(mut inner) = self.inner.write() else {
                return PutOutcome::Failed;
            };
            if let Some(old) = &previous {
                inner.total = inner.total.saturating_sub(old.size);
            }
            let meta = ObjectMeta {
                key: key.to_string(),
                file: rel.clone(),
                size: bytes.len() as u64,
                hash,
                etag: tags.etag,
                last_modified: tags.last_modified,
                content_type: tags.content_type,
                fetched_at: now,
                last_access: now,
                expires_at: tags.expires_at,
            };
            inner.total = inner.total.saturating_add(meta.size);
            inner.entries.insert(key.to_string(), meta);
        }
        self.evict_if_needed();
        self.flush_locked();
        let replacing = previous.is_some();
        if let Some(old) = previous
            && old.file != rel
        {
            let _ = fs::remove_file(root.join(old.file));
        }
        if replacing {
            PutOutcome::Replaced
        } else {
            PutOutcome::Inserted
        }
    }

    /// 条件刷新命中 304 时调用：不写对象，只更新元信息与 LRU。
    pub fn touch(&self, key: &str, tags: CacheTags) -> bool {
        let Ok(_mutation) = self.mutation.lock() else {
            return false;
        };
        let Ok(mut inner) = self.inner.write() else {
            return false;
        };
        let Some(entry) = inner.entries.get_mut(key) else {
            return false;
        };
        let now = now_secs();
        entry.fetched_at = now;
        entry.last_access = now;
        merge_tags(entry, tags);
        drop(inner);
        self.flush_locked();
        true
    }

    pub fn put_json<T: Serialize>(&self, key: &str, value: &T, tags: CacheTags) -> PutOutcome {
        match serde_json::to_vec(value) {
            Ok(bytes) => self.put(key, &bytes, tags),
            Err(_) => PutOutcome::Failed,
        }
    }

    pub fn remove(&self, key: &str) -> bool {
        let Ok(_mutation) = self.mutation.lock() else {
            return false;
        };
        let Ok(mut inner) = self.inner.write() else {
            return false;
        };
        let Some(meta) = inner.entries.remove(key) else {
            return false;
        };
        inner.total = inner.total.saturating_sub(meta.size);
        let root = inner.root.clone();
        drop(inner);
        let _ = fs::remove_file(root.join(meta.file));
        self.flush_locked();
        true
    }

    /// 回收 key 以 `prefix` 开头的全部对象（账号级缓存用）。
    pub fn purge_prefix(&self, prefix: &str) -> u64 {
        let Ok(_mutation) = self.mutation.lock() else {
            return 0;
        };
        let Ok(mut inner) = self.inner.write() else {
            return 0;
        };
        let keys: Vec<String> = inner
            .entries
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        let mut removed = 0u64;
        let mut files = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(meta) = inner.entries.remove(&key) {
                inner.total = inner.total.saturating_sub(meta.size);
                files.push(meta.file);
                removed += 1;
            }
        }
        let root = inner.root.clone();
        drop(inner);
        for file in files {
            let _ = fs::remove_file(root.join(file));
        }
        self.flush_locked();
        removed
    }

    /// 回收一个源的全部对象（key 前缀 `{source}/`）。
    pub fn purge_namespace(&self, namespace: &str) -> u64 {
        let prefix = format!("{}/", sanitize_segment(namespace));
        let removed = self.purge_prefix(&prefix);
        tracing::info!(namespace, removed, "object cache purged");
        removed
    }

    pub fn purge_all(&self) -> u64 {
        let Ok(_mutation) = self.mutation.lock() else {
            return 0;
        };
        let Ok(mut inner) = self.inner.write() else {
            return 0;
        };
        let mut files = Vec::with_capacity(inner.entries.len());
        for meta in inner.entries.values() {
            files.push(meta.file.clone());
        }
        let removed = inner.entries.len() as u64;
        inner.entries.clear();
        inner.total = 0;
        let root = inner.root.clone();
        drop(inner);
        for file in files {
            let _ = fs::remove_file(root.join(file));
        }
        self.flush_locked();
        removed
    }

    pub fn set_limit(&self, limit: u64) {
        let Ok(_mutation) = self.mutation.lock() else {
            return;
        };
        if let Ok(mut inner) = self.inner.write() {
            inner.limit = limit;
        }
        self.evict_if_needed();
        self.flush_locked();
    }

    /// 切换缓存目录（设置里改地址后即时生效）。旧目录内容保留在磁盘，
    /// 由用户自行清理；新目录载入自己的索引。
    pub fn reconfigure(&self, root: PathBuf, limit: u64) {
        let Ok(_mutation) = self.mutation.lock() else {
            return;
        };
        let same_root = self
            .inner
            .read()
            .ok()
            .is_some_and(|inner| inner.root == root);
        if same_root {
            if let Ok(mut inner) = self.inner.write() {
                inner.limit = limit;
            }
            self.evict_if_needed();
            self.flush_locked();
            return;
        }

        let _ = fs::create_dir_all(&root);
        let index = load_index(&root);
        let mut entries = index.entries;
        entries.retain(|_, meta| {
            let path = root.join(&meta.file);
            fs::metadata(&path)
                .map(|m| m.is_file() && m.len() == meta.size)
                .unwrap_or(false)
        });
        let total = entries.values().map(|m| m.size).sum();
        if let Ok(mut inner) = self.inner.write() {
            inner.total = total;
            inner.root = root;
            inner.limit = limit;
            inner.entries = entries;
        }
        self.evict_if_needed();
        self.flush_locked();
    }

    pub fn stats(&self) -> CacheStats {
        let inner = self.inner.read().expect("cache lock poisoned");
        CacheStats {
            entries: inner.entries.len() as u64,
            total_bytes: inner.total,
            limit_bytes: inner.limit,
            root: inner.root.clone(),
        }
    }

    /// 把索引原子写回磁盘。所有结构变更与 flush 通过 `mutation` 串行化。
    pub fn flush(&self) {
        let Ok(_mutation) = self.mutation.lock() else {
            return;
        };
        self.flush_locked();
    }

    fn flush_locked(&self) {
        let (root, json) = {
            let Ok(inner) = self.inner.read() else {
                return;
            };
            let index = Index {
                entries: inner.entries.clone(),
            };
            let Ok(json) = serde_json::to_vec(&index) else {
                return;
            };
            (inner.root.clone(), json)
        };
        if let Err(error) = chordal_core::fs::atomic_write(&root.join("index.json"), &json) {
            tracing::warn!(root = %root.display(), %error, "cache index flush failed");
        }
    }

    fn evict_if_needed(&self) {
        let (target, order) = {
            let Ok(inner) = self.inner.read() else {
                return;
            };
            if inner.limit == 0 || inner.total <= inner.limit {
                return;
            }
            // 一次淘汰到 90%，避免每次写入都触发排序。
            let target = inner.limit.saturating_mul(9) / 10;
            let mut order: Vec<(String, u64, u64)> = inner
                .entries
                .iter()
                .map(|(k, m)| (k.clone(), m.last_access, m.size))
                .collect();
            order.sort_unstable_by_key(|(_, access, _)| *access);
            (target, order)
        };

        for (key, _, size) in order {
            let current_total = self.inner.read().ok().map(|inner| inner.total).unwrap_or(0);
            if current_total <= target {
                break;
            }
            let Some(meta) = ({
                let Ok(inner) = self.inner.read() else {
                    return;
                };
                inner.entries.get(&key).cloned()
            }) else {
                continue;
            };
            let abs = {
                let Ok(inner) = self.inner.read() else {
                    return;
                };
                inner.root.join(&meta.file)
            };
            let removable = match fs::remove_file(&abs) {
                Ok(()) => true,
                Err(e) if e.kind() == io::ErrorKind::NotFound => true,
                // 文件被占用（Windows）时保留，下一轮再试。
                Err(_) => false,
            };
            if removable
                && let Ok(mut inner) = self.inner.write()
                && inner
                    .entries
                    .get(&key)
                    .is_some_and(|current| current.hash == meta.hash && current.file == meta.file)
            {
                inner.entries.remove(&key);
                inner.total = inner.total.saturating_sub(size);
            }
        }
    }
}

impl Drop for ObjectCache {
    fn drop(&mut self) {
        self.flush();
    }
}

fn merge_tags(entry: &mut ObjectMeta, tags: CacheTags) {
    if tags.etag.is_some() {
        entry.etag = tags.etag;
    }
    if tags.last_modified.is_some() {
        entry.last_modified = tags.last_modified;
    }
    if tags.content_type.is_some() {
        entry.content_type = tags.content_type;
    }
    if tags.expires_at.is_some() {
        entry.expires_at = tags.expires_at;
    }
}

fn load_index(root: &Path) -> Index {
    let path = root.join("index.json");
    let Ok(raw) = fs::read(&path) else {
        return Index::default();
    };
    serde_json::from_slice(&raw).unwrap_or_else(|e| {
        tracing::warn!(path = %path.display(), error = %e, "cache index corrupt; starting empty");
        Index::default()
    })
}

/// key 的 128 位摘要（两路 FNV-1a）+ 内容摘要，文件落在两级目录。
/// 内容摘要进入路径后，替换对象不会原地覆盖旧文件，并发读取始终能拿到
/// 与元信息匹配的字节。
fn rel_path(key: &str, content_hash: u64) -> String {
    let h1 = fnv1a64(key.as_bytes());
    let mut salted = Vec::with_capacity(key.len() + 8);
    salted.extend_from_slice(b"chordial");
    salted.extend_from_slice(key.as_bytes());
    let h2 = fnv1a64(&salted);
    format!(
        "{:02x}/{:016x}{:016x}{:016x}.bin",
        (h1 >> 56) as u8,
        h1,
        h2,
        content_hash
    )
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SERIAL: AtomicU64 = AtomicU64::new(0);

    fn temp_root(name: &str) -> PathBuf {
        let n = SERIAL.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("chordial-cache-{name}-{}-{n}", std::process::id()))
    }

    fn open(root: &Path, limit: u64) -> ObjectCache {
        ObjectCache::open(CacheConfig {
            root: root.to_path_buf(),
            limit_bytes: limit,
        })
        .unwrap()
    }

    #[test]
    fn put_get_roundtrip_and_unchanged() {
        let root = temp_root("roundtrip");
        let cache = open(&root, 1024 * 1024);
        let key = object_key("netease", "lyric", "42");
        assert_eq!(
            cache.put(&key, b"hello", CacheTags::default()),
            PutOutcome::Inserted
        );
        assert_eq!(cache.get(&key).unwrap().bytes, b"hello");
        assert_eq!(
            cache.put(&key, b"hello", CacheTags::default()),
            PutOutcome::Unchanged
        );
        assert_eq!(
            cache.put(&key, b"hello2", CacheTags::default()),
            PutOutcome::Replaced
        );
        assert_eq!(cache.get(&key).unwrap().bytes, b"hello2");
        assert_eq!(cache.stats().entries, 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn eviction_respects_limit_and_keeps_recent() {
        let root = temp_root("evict");
        let cache = open(&root, 10);
        let a = object_key("netease", "cover", "a");
        let b = object_key("netease", "cover", "b");
        let c = object_key("netease", "cover", "c");
        cache.put(&a, &[1u8; 4], CacheTags::default());
        std::thread::sleep(std::time::Duration::from_millis(1100));
        cache.put(&b, &[2u8; 4], CacheTags::default());
        std::thread::sleep(std::time::Duration::from_millis(1100));
        // 4+4+4 > 10，淘汰最久未用的 a，保留 b/c。
        cache.put(&c, &[3u8; 4], CacheTags::default());
        assert_eq!(cache.stats().total_bytes, 8);
        assert!(cache.get(&a).is_none());
        assert!(cache.get(&b).is_some());
        assert!(cache.get(&c).is_some());

        // 超大对象直接拒绝，不驱逐已有对象。
        let big = object_key("netease", "cover", "big");
        assert_eq!(
            cache.put(&big, &[0u8; 11], CacheTags::default()),
            PutOutcome::TooLarge
        );
        assert_eq!(cache.stats().entries, 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn purge_namespace_only_removes_source() {
        let root = temp_root("purge");
        let cache = open(&root, 1024);
        let n = object_key("netease", "track", "1");
        let l = object_key("local", "cover", "x");
        cache.put(&n, b"n", CacheTags::default());
        cache.put(&l, b"l", CacheTags::default());
        assert_eq!(cache.purge_namespace("netease"), 1);
        assert!(cache.get(&n).is_none());
        assert_eq!(cache.get(&l).unwrap().bytes, b"l");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn index_survives_reopen_and_missing_files_are_dropped() {
        let root = temp_root("reopen");
        {
            let cache = open(&root, 1024);
            let key = object_key("netease", "lyric", "7");
            cache.put(&key, b"lyric", CacheTags::default());
            cache.flush();
        }
        let cache = open(&root, 1024);
        let key = object_key("netease", "lyric", "7");
        assert_eq!(cache.get(&key).unwrap().bytes, b"lyric");
        // 手动删掉对象文件后，索引项在下次打开时被丢弃。
        let meta = cache.meta(&key).unwrap();
        fs::remove_file(root.join(meta.file)).unwrap();
        drop(cache);
        let cache = open(&root, 1024);
        assert!(cache.get(&key).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn concurrent_put_flush_keeps_index_and_objects_consistent() {
        let root = temp_root("concurrent");
        let cache = Arc::new(open(&root, 8 * 1024 * 1024));
        let mut workers = Vec::new();
        for worker in 0..4 {
            let cache = cache.clone();
            workers.push(std::thread::spawn(move || {
                for item in 0..20 {
                    let key = object_key("netease", "cover", &format!("{worker}-{item}"));
                    let bytes = format!("{worker}:{item}").into_bytes();
                    assert_eq!(
                        cache.put(&key, &bytes, CacheTags::default()),
                        PutOutcome::Inserted
                    );
                    cache.flush();
                    let object = cache.get(&key).expect("object must be readable");
                    assert_eq!(object.bytes, bytes);
                    assert_eq!(object.meta.hash, fnv1a64(&object.bytes));
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        cache.flush();
        assert_eq!(cache.stats().entries, 80);
        drop(cache);

        let reopened = open(&root, 8 * 1024 * 1024);
        assert_eq!(reopened.stats().entries, 80);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn replacement_never_pairs_old_meta_with_new_bytes() {
        let root = temp_root("replace-race");
        let cache = Arc::new(open(&root, 1024 * 1024));
        let key = object_key("netease", "lyric", "race");
        cache.put(&key, b"initial", CacheTags::default());
        let writer_cache = cache.clone();
        let writer_key = key.clone();
        let writer = std::thread::spawn(move || {
            for value in 0..100u8 {
                writer_cache.put(&writer_key, &[value; 32], CacheTags::default());
            }
        });
        for _ in 0..500 {
            if let Some(object) = cache.get(&key) {
                assert_eq!(object.meta.hash, fnv1a64(&object.bytes));
                assert_eq!(object.meta.size, object.bytes.len() as u64);
            }
        }
        writer.join().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn json_helpers_roundtrip() {
        let root = temp_root("json");
        let cache = open(&root, 1024);
        let key = object_key("netease", "track", "9");
        let value = serde_json::json!({"id": 9, "name": "海阔天空"});
        cache.put_json(&key, &value, CacheTags::default());
        let got: serde_json::Value = cache.get_json(&key).unwrap();
        assert_eq!(got["name"], "海阔天空");
        let _ = fs::remove_dir_all(&root);
    }
}
