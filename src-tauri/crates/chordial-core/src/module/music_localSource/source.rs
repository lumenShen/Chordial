//! 本地音乐来源 — [`LocalMusicSource`] 是 [`MusicSource`] 的具体实现。
//!
//! 负责管理本地文件系统中的音乐文件，通过 symphonia 读取元数据，
//! 并通过 notify 监听文件夹变化实现增量同步到 [`MusicLibrary`]。
//!
//! # 设计要点
//!
//! - **entity_id 使用文件绝对路径**：`SourceId.entity_id` 即为音频文件的规范化路径。
//!   这使得资源获取（`song_file_get`）成为简单的文件读取操作。
//! - **must-source**：本地来源在初始化时自动注册，不允许注销。
//! - **初始文件夹**：首次启动时自动添加系统音乐目录（`dirs::audio_dir()`）。
//! - **跨平台路径**：通过 [`crate::module::platform::PlatformPath`] 适配桌面（`PathBuf`）
//!   和 Android（`String` / content URI）。
//!
//! # 模块划分
//!
//! - `cover.rs`：封面三级缓存（内存 → 磁盘 → 提取）。
//! - `index.rs`：文件索引维护（增删改查 + mtime 缓存）。

use super::artists::split_artist_names;
use super::folder::FolderManager;
use super::scanner::{self, AudioMeta};
use crate::module::music_library::library::MusicLibrary;
use crate::module::music_library::models::{Album, Artist, Lyric, Song};
use crate::module::music_source::traits::MusicSource;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};
use crate::module::perf;
use crate::module::platform::{self, PlatformPath};
use crate::module::storage::persistent::PersistentStore;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

/// 本地音乐来源的名称常量。
pub const LOCAL_SOURCE_NAME: &str = "local";

/// 本地音乐来源。
///
/// 实现 [`MusicSource`]，将所有查询和资源操作映射到本地文件系统。
pub struct LocalMusicSource {
    /// 来源名称（固定为 "local"）
    name: String,
    /// 文件夹管理器（持久化 + 运行时）— 供命令层访问
    pub folder_manager: Arc<FolderManager>,
    /// 音乐库引用 — 用于增量同步，供命令层访问
    pub library: Arc<MusicLibrary>,
    /// 文件索引：规范路径 → 库内 Song ID，供命令层访问
    pub file_index: RwLock<HashMap<PlatformPath, String>>,
    /// 反向索引：库内 Song ID → 规范路径
    pub id_to_path: RwLock<HashMap<String, PlatformPath>>,
    /// 文件 mtime 缓存：规范路径字符串 → (modified_secs, file_size, song_id)
    /// 启动时对比文件系统 mtime，跳过未变化文件的重扫描
    pub file_mtimes: RwLock<HashMap<String, (u64, u64, String)>>,
    /// 独立的 mtime 持久化存储（与 library 分离，避免每次保存都序列化全部歌曲数据）
    pub(super) mtime_store: PersistentStore,
    /// 封面图内存缓存：entity_id（路径）→ 图片字节
    /// 避免每次 chordial://image 请求都触发 extract_cover_art（5-50ms/次）
    pub(super) cover_cache: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    /// 封面图磁盘缓存目录：`<data_dir>/cover_cache/<hash>.bin`
    ///
    /// 三级缓存策略（album_picture_get）：
    /// 1. 内存缓存命中 → ~0.01ms
    /// 2. 磁盘缓存命中 → ~1-5ms（文件读取）
    /// 3. symphonia 提取 → 117-296ms（写入磁盘 + 内存）
    ///
    /// 扫描阶段（batch_index_files）通过 probe_file_with_cover 一次解析
    /// 同时提取元数据与封面，写入磁盘缓存预热，浏览阶段直接命中。
    pub(super) cover_disk_cache_dir: PathBuf,
}

impl LocalMusicSource {
    /// 创建本地音乐来源。
    ///
    /// # 参数
    /// - `folder_manager`: 文件夹管理器（持久化 + 运行时文件夹集合）
    /// - `library`: 音乐库引用
    /// - `mtime_store`: 文件 mtime 持久化存储
    /// - `cover_disk_cache_dir`: 封面磁盘缓存目录（不存在时自动创建）
    pub fn new(
        folder_manager: Arc<FolderManager>,
        library: Arc<MusicLibrary>,
        mtime_store: PersistentStore,
        cover_disk_cache_dir: PathBuf,
    ) -> Self {
        // 确保封面缓存目录存在（首次启动自动创建）
        if let Err(e) = std::fs::create_dir_all(&cover_disk_cache_dir) {
            eprintln!(
                "[local_source] 创建封面缓存目录失败 '{}': {}",
                cover_disk_cache_dir.display(),
                e
            );
        }
        Self {
            name: LOCAL_SOURCE_NAME.to_string(),
            folder_manager,
            library,
            file_index: RwLock::new(HashMap::new()),
            id_to_path: RwLock::new(HashMap::new()),
            file_mtimes: RwLock::new(HashMap::new()),
            mtime_store,
            cover_cache: Mutex::new(HashMap::new()),
            cover_disk_cache_dir,
        }
    }

    /// 从 AudioMeta 构建 Song 模型。
    ///
    /// 关键逻辑：
    /// - 将 `meta.artist` 按 `/`、`&`、`、`、`，`、` feat. `、` ft. `、` featuring ` 等
    ///   分隔符拆分为多个独立 artist，每个生成独立 UUID。
    /// - 写入 `song.year = meta.year`，供后续 album 聚合使用。
    pub fn build_song(&self, file_path: &PlatformPath, meta: &AudioMeta) -> Song {
        let entity_id = platform::path_to_string(file_path);
        let song_id = Uuid::new_v4().to_string();
        let artist_names = split_artist_names(meta.artist.as_deref());
        let artist_ids: Vec<String> = (0..artist_names.len())
            .map(|_| Uuid::new_v4().to_string())
            .collect();
        let album_title = meta.album.clone();
        let album_id = album_title.as_ref().map(|_| Uuid::new_v4().to_string());
        let lyric_id = Some(Uuid::new_v4().to_string());

        let source_id = SourceId {
            source_name: LOCAL_SOURCE_NAME.to_string(),
            source_type: SourceType::Local,
            entity_type: EntityType::Song,
            entity_id: entity_id.clone(),
        };

        Song {
            id: song_id,
            title: meta.title.clone().unwrap_or_else(|| "未知歌曲".to_string()),
            artist_names,
            album_title,
            duration: meta.duration_secs,
            artist_ids,
            album_id,
            lyric_id,
            source_ids: vec![source_id],
            year: meta.year,
            // 本地来源通过 chordial:// 协议从音频标签提取封面，无直接 URL
            cover_url: None,
        }
    }

    /// 按文件路径查找对应的库内 Song ID。
    pub fn find_song_id_by_path(&self, path: &PlatformPath) -> Option<String> {
        let canonical = platform::canonicalize(path)
            .unwrap_or_else(|_| path.clone());
        self.file_index.read().get(&canonical).cloned()
    }

    /// 按文件路径查找对应的 SourceId。
    pub fn find_source_id_by_path(&self, path: &PlatformPath) -> Option<SourceId> {
        let canonical = platform::canonicalize(path)
            .unwrap_or_else(|_| path.clone());
        let entity_id = platform::path_to_string(&canonical);
        Some(SourceId {
            source_name: LOCAL_SOURCE_NAME.to_string(),
            source_type: SourceType::Local,
            entity_type: EntityType::Song,
            entity_id,
        })
    }
}

impl MusicSource for LocalMusicSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> SourceType {
        SourceType::Local
    }

    fn search_songs(&self, query: &str) -> Result<Vec<Song>, String> {
        let _scope = perf::scope("source.search_songs");
        // 优化：委托给 library 内存搜索，避免每次查询重新探测文件（O(n) 磁盘 I/O）
        // 旧实现：`self.file_index.read().keys().cloned()` + 循环 `scanner::probe_file()`
        // 每次查询触发 N 次磁盘读取，对 1000 首歌 = 1000 次 I/O。
        let results: Vec<Song> = self
            .library
            .search_songs(query)
            .into_iter()
            .filter(|song| {
                song.source_ids
                    .iter()
                    .any(|sid| sid.source_name == LOCAL_SOURCE_NAME)
            })
            .collect();
        Ok(results)
    }

    fn get_song(&self, id: &str) -> Result<Option<Song>, String> {
        // id 可能是库内 UUID 或文件路径
        let path = if let Some(p) = self.id_to_path.read().get(id) {
            p.clone()
        } else {
            let p = PlatformPath::from(id);
            if platform::exists(&p) {
                p
            } else {
                return Ok(None);
            }
        };

        match scanner::probe_file(&path) {
            Ok(meta) => Ok(Some(self.build_song(&path, &meta))),
            Err(_) => Ok(None),
        }
    }

    fn get_artist(&self, _id: &str) -> Result<Option<Artist>, String> {
        // 本地来源的艺人信息从歌曲标签派生，直接从音乐库查询即可
        Ok(None)
    }

    fn get_album(&self, _id: &str) -> Result<Option<Album>, String> {
        // 同上
        Ok(None)
    }

    fn get_lyric(&self, _song_id: &str) -> Result<Option<Lyric>, String> {
        Ok(None)
    }

    fn song_file_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        let path = PlatformPath::from(entity_id);
        platform::read_bytes(&path)
            .map_err(|e| format!("读取音频文件失败 '{}': {}", entity_id, e))
    }

    fn song_file_path(&self, entity_id: &str) -> Option<String> {
        let path = PlatformPath::from(entity_id);
        if platform::is_file(&path) {
            Some(entity_id.to_string())
        } else {
            // 尝试通过 id_to_path 查找
            self.id_to_path
                .read()
                .get(entity_id)
                .map(|p| platform::path_to_string(p))
        }
    }

    fn album_picture_get(&self, entity_id: &str) -> Result<Vec<u8>, String> {
        // 三级缓存策略，将浏览阶段的 117-296ms 提取降至 ~0.01ms（内存）/ ~1-5ms（磁盘）

        // 1. 内存缓存命中（~0.01ms）
        //    典型命中：浏览/播放同一专辑时多次请求封面
        //    临界区仅做 HashMap lookup + Arc::clone（引用计数拷贝，无堆分配），
        //    深拷贝 Vec<u8> 放到锁外执行，避免高并发下多线程排队等锁。
        let arc_hit = {
            let cache = self.cover_cache.lock();
            cache.get(entity_id).map(Arc::clone)
        };
        if let Some(arc) = arc_hit {
            // 锁外深拷贝（典型 50-500KB），不阻塞其他线程访问缓存
            return Ok((*arc).clone());
        }

        // 2. 磁盘缓存命中（~1-5ms）
        //    典型命中：重启后首次浏览；扫描阶段已通过 probe_file_with_cover 预热
        if let Some(data) = self.load_cover_from_disk(entity_id) {
            // 回填内存缓存，后续请求直接命中内存
            self.insert_cover_cache(entity_id, data.clone());
            return Ok(data);
        }

        // 3. 从文件提取（慢路径，117-296ms symphonia 解析）
        //    典型场景：扫描未覆盖的文件（如外部 cover.jpg 回退路径）
        let path = PlatformPath::from(entity_id);
        let data = self.extract_album_picture(&path)?;

        // 写入磁盘 + 内存缓存（后续请求不再触发提取）
        self.save_cover_to_disk(entity_id, &data);
        self.insert_cover_cache(entity_id, data.clone());

        Ok(data)
    }

    fn lyric_text_get(&self, song_id: &str) -> Result<String, String> {
        // 通过 song_id（或直接当作路径）定位音频文件，再读同目录 .lrc / .txt
        // 复用 scanner::read_lyric_file，与扫描时入库的逻辑保持一致
        let audio_path = if let Some(p) = self.id_to_path.read().get(song_id) {
            p.clone()
        } else {
            PlatformPath::from(song_id)
        };

        scanner::read_lyric_file(&audio_path)
            .ok_or_else(|| format!("未找到歌词文件: {}", platform::path_to_string(&audio_path)))
    }
}