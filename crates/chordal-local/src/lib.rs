//! 本地音乐库适配器。
//!
//! 职责边界（性能关键）：
//! - **扫描/标签解析是纯阻塞 IO+CPU**，走 [`LocalSource::scan_blocking`]，
//!   由宿主（gpui background executor / 任意线程池）调度，绝不在 trait 的
//!   async 方法里做重磁盘 IO。
//! - async 方法（search/track/stream）只读内存索引：O(n) 单遍过滤 + O(1) 主键查找。
//! - 搜索键（[`chordal_core::model::Track::search_key`]）在扫描期预计算，
//!   查询期零字符串分配。
//! - 增量扫描：mtime+len 指纹未变的文件直接复用旧 [`Track`]，不读标签。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chordal_core::error::{CoreError, CoreResult};
use chordal_core::model::{
    AlbumRef, ArtistRef, AudioQuality, Capabilities, CollectionId, CollectionKind, CollectionPage,
    CollectionSummary, Lyrics, QualityTier, ResolvedStream, SearchKind, SearchQuery, SearchResults,
    SourceId, SourceKind, StreamHandle, Track, TrackId, TrackOrigin, TrackPage,
};
use chordal_core::perf::PerfScope;
use chordal_core::source::{CatalogSource, MusicSource, SourceInfo};

/// 支持的音频扩展名（小写）。
const AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "wav", "m4a", "aac"];

/// 标签读取抽象。真实实现走 symphonia；测试注入假实现（无需音频文件）。
pub trait TagReader: Send + Sync {
    fn read(&self, path: &Path) -> Option<TrackMeta>;
}

#[derive(Debug, Clone, Default)]
pub struct TrackMeta {
    pub title: Option<String>,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub track_no: Option<u32>,
    pub year: Option<u32>,
}

/// 增量扫描指纹：路径 → (mtime 秒, len)。变更检测 O(n) 单遍 stat。
type Fingerprints = HashMap<PathBuf, (u64, u64)>;

#[derive(Default)]
struct Index {
    tracks: Vec<Track>,
    /// 源内 id（= 规范路径字符串）→ tracks 下标。O(1) 查找。
    by_id: HashMap<String, u32>,
    /// `dedup_key` → tracks 下标。用于“在线播放时找本地同曲”。
    by_dedup: HashMap<String, Vec<u32>>,
    /// 合成专辑 id → 聚合。扫描期 O(n) 派生，不额外持久化。
    albums: HashMap<String, LocalAlbumAgg>,
    /// 合成歌手 id → 聚合。
    artists: HashMap<String, LocalArtistAgg>,
    fingerprints: Fingerprints,
}

#[derive(Default, Clone)]
struct LocalAlbumAgg {
    title: String,
    creator: Option<String>,
    cover: Option<String>,
    year: Option<u32>,
    /// 预归一化的“标题 艺人”搜索键。
    search_key: String,
    tracks: Vec<u32>,
}

#[derive(Default, Clone)]
struct LocalArtistAgg {
    name: String,
    cover: Option<String>,
    search_key: String,
    album_ids: Vec<String>,
    tracks: Vec<u32>,
}

/// 本地库源配置。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocalSourceConfig {
    pub roots: Vec<PathBuf>,
}

impl Default for LocalSourceConfig {
    fn default() -> Self {
        Self {
            roots: vec![default_music_dir()],
        }
    }
}

fn default_music_dir() -> PathBuf {
    std::env::var("XDG_MUSIC_DIR")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join("Music")))
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 扫描结果摘要。
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ScanSummary {
    pub tracks: u64,
    pub scan_generation: u64,
}

/// 本地音乐源。
pub struct LocalSource {
    info: SourceInfo,
    /// 设置面板可在运行时替换目录；扫描开始前取快照，避免持锁遍历文件系统。
    config: RwLock<LocalSourceConfig>,
    tag_reader: RwLock<Box<dyn TagReader>>,
    index: RwLock<Index>,
    /// 扫描代数：每次扫描自增，UI 可观察。
    scan_generation: AtomicU64,
}

impl LocalSource {
    /// 创建源（不扫描）。扫描由宿主线程池显式触发。
    pub fn new(
        id: SourceId,
        config: LocalSourceConfig,
        display_name: impl Into<String>,
    ) -> Arc<Self> {
        Self::new_with_tag_reader(id, config, display_name, Box::new(SymphoniaTagReader))
    }

    /// 注入标签读取器（测试 / 未来外部元数据提供方）。
    pub fn new_with_tag_reader(
        id: SourceId,
        config: LocalSourceConfig,
        display_name: impl Into<String>,
        tag_reader: Box<dyn TagReader>,
    ) -> Arc<Self> {
        let info = SourceInfo {
            description: format!(
                "本地库 · {}",
                config
                    .roots
                    .iter()
                    .map(|r| r.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            display_name: display_name.into(),
            id,
            kind: SourceKind::LocalLibrary,
            priority: 0,
        };
        Arc::new(Self {
            info,
            config: RwLock::new(config),
            tag_reader: RwLock::new(tag_reader),
            index: RwLock::new(Index::default()),
            scan_generation: AtomicU64::new(0),
        })
    }

    /// 替换音乐目录（空列表忽略）。下一次 `scan_blocking` 生效。
    pub fn set_roots(&self, roots: Vec<PathBuf>) {
        let roots: Vec<PathBuf> = roots
            .into_iter()
            .filter(|root| !root.as_os_str().is_empty())
            .collect();
        if roots.is_empty() {
            return;
        }
        self.config
            .write()
            .expect("local config lock poisoned")
            .roots = roots;
    }

    /// 当前音乐目录快照。
    pub fn roots(&self) -> Vec<PathBuf> {
        self.config
            .read()
            .expect("local config lock poisoned")
            .roots
            .clone()
    }

    /// 完整/增量扫描（阻塞，调用方负责线程池）。
    ///
    /// 复杂度：O(文件数) 单遍 walkdir + 单遍 stat + 仅变更文件读标签。
    /// 检查点 `local.scan.index`（items = 音频文件数）。
    pub fn scan_blocking(&self) -> ScanSummary {
        let scope = PerfScope::enter("local.scan.index");

        // 旧索引只读快照：增量复用的数据源。整个扫描持读锁，
        // 并发 search 不受影响；只在最后短暂取写锁换入新索引。
        let old_index = self.index.read().expect("local index lock poisoned");
        let old_by_path: HashMap<&Path, usize> = old_index
            .tracks
            .iter()
            .enumerate()
            .filter_map(|(i, t)| match &t.origin {
                TrackOrigin::Local { path } => Some((path.as_path(), i)),
                _ => None,
            })
            .collect();

        let exts: HashSet<&str> = AUDIO_EXTS.iter().copied().collect();
        let roots = self
            .config
            .read()
            .expect("local config lock poisoned")
            .roots
            .clone();
        let mut files: Vec<PathBuf> = Vec::new();
        for root in &roots {
            let walker = walkdir::WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok());
            for entry in walker {
                if !entry.file_type().is_file() {
                    continue;
                }
                let is_audio = entry
                    .path()
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| exts.contains(e.to_ascii_lowercase().as_str()))
                    .unwrap_or(false);
                if is_audio {
                    files.push(entry.into_path());
                }
            }
        }

        let mut tracks: Vec<Track> = Vec::with_capacity(files.len());
        let mut by_id: HashMap<String, u32> = HashMap::with_capacity(files.len());
        let mut by_dedup: HashMap<String, Vec<u32>> = HashMap::with_capacity(files.len());
        let mut fingerprints: Fingerprints = HashMap::with_capacity(files.len());
        let mut tag_reads = 0u64;
        let mut reused = 0u64;
        // 每个目录只探测一次封面文件，避免 O(文件数 × 候选名) 的 stat。
        let mut folder_covers: HashMap<PathBuf, Option<String>> = HashMap::new();

        for path in &files {
            let fp = file_fingerprint(path);
            let track_id = path.to_string_lossy().into_owned();

            let track = match fp.as_ref().and_then(|fp| {
                // 增量：指纹未变 → 复用旧 Track（零标签读取）。
                if old_index.fingerprints.get(path) == Some(fp) {
                    old_by_path
                        .get(path.as_path())
                        .map(|&i| old_index.tracks[i].clone())
                } else {
                    None
                }
            }) {
                Some(old) => {
                    reused += 1;
                    old
                }
                None => {
                    tag_reads += 1;
                    let meta = self
                        .tag_reader
                        .read()
                        .expect("tag reader lock poisoned")
                        .read(path);
                    let mut track = build_track(&self.info.id, &track_id, path, meta);
                    if let Some(parent) = path.parent() {
                        // 下载器会给每首歌写 `<stem>.cover.<ext>` 边车；它比
                        // 目录级 cover.jpg 更精确（同目录可能有多张专辑）。
                        let cover = find_track_cover(path).or_else(|| {
                            folder_covers
                                .entry(parent.to_path_buf())
                                .or_insert_with(|| find_folder_cover(parent))
                                .clone()
                        });
                        if cover.is_some() {
                            if let Some(album) = track.album.as_mut() {
                                album.cover = cover;
                            } else {
                                // 没有专辑标签时也保留同目录封面（以目录名作专辑名）。
                                let name = parent
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                let album_id = local_album_id(&name, path);
                                track.album = Some(AlbumRef {
                                    id: Some(album_id),
                                    name,
                                    cover,
                                });
                            }
                        }
                    }
                    track
                }
            };

            fingerprints.insert(path.clone(), fp.unwrap_or((0, 0)));
            by_id.insert(track_id, tracks.len() as u32);
            by_dedup
                .entry(track.dedup_key.clone())
                .or_default()
                .push(tracks.len() as u32);
            tracks.push(track);
        }
        drop(old_index);

        let (albums, artists) = build_collections(&tracks);
        {
            let mut index = self.index.write().expect("local index lock poisoned");
            index.tracks = tracks;
            index.by_id = by_id;
            index.by_dedup = by_dedup;
            index.albums = albums;
            index.artists = artists;
            index.fingerprints = fingerprints;
        }
        self.scan_generation.fetch_add(1, Ordering::Release);

        scope.finish(files.len() as u64);
        tracing::info!(
            target: "chordial::local",
            files = files.len(),
            tag_reads,
            reused,
            "local library scanned"
        );
        self.last_summary()
    }

    /// 最近一次扫描摘要。
    pub fn last_summary(&self) -> ScanSummary {
        let index = self.index.read().expect("local index lock poisoned");
        ScanSummary {
            tracks: index.tracks.len() as u64,
            scan_generation: self.scan_generation.load(Ordering::Acquire),
        }
    }

    /// 测试注入：绕过磁盘扫描直接建索引（性能护栏测试用）。
    #[doc(hidden)]
    pub fn inject_index_for_tests(&self, tracks: Vec<Track>) {
        let mut by_id = HashMap::with_capacity(tracks.len());
        let mut by_dedup: HashMap<String, Vec<u32>> = HashMap::with_capacity(tracks.len());
        let mut fingerprints = Fingerprints::with_capacity(tracks.len());
        for (i, t) in tracks.iter().enumerate() {
            by_id.insert(t.id.id.clone(), i as u32);
            by_dedup
                .entry(t.dedup_key.clone())
                .or_default()
                .push(i as u32);
            if let TrackOrigin::Local { path } = &t.origin {
                fingerprints.insert(path.clone(), (0, 0));
            }
        }
        let (albums, artists) = build_collections(&tracks);
        let mut index = self.index.write().expect("local index lock poisoned");
        index.tracks = tracks;
        index.by_id = by_id;
        index.by_dedup = by_dedup;
        index.albums = albums;
        index.artists = artists;
        index.fingerprints = fingerprints;
    }

    pub fn scan_generation(&self) -> u64 {
        self.scan_generation.load(Ordering::Acquire)
    }

    /// 全库曲目快照（library 视图用；UI 侧持有到下一次扫描为止）。
    pub fn all_tracks(&self) -> Vec<Track> {
        self.index
            .read()
            .expect("local index lock poisoned")
            .tracks
            .clone()
    }

    /// 查找与给定在线曲目“同名同主艺人同专辑”的本地文件。
    ///
    /// 先按构造期预计算的 `dedup_key` O(1) 定位候选，再在同专辑候选中
    /// 选时长最接近的一首；没有专辑匹配时允许 ±3s 的时长兜底（同名
    /// 不同专辑的电台版/精选集也能命中）。
    pub fn find_equivalent(&self, track: &Track) -> Option<Track> {
        if matches!(track.origin, TrackOrigin::Local { .. }) {
            return None;
        }
        let index = self.index.read().expect("local index lock poisoned");
        let candidates = index.by_dedup.get(&track.dedup_key)?;
        let album_key = track
            .album
            .as_ref()
            .map(|a| chordal_core::model::normalize(&a.name))
            .filter(|s| !s.is_empty());
        let mut fallback: Option<&Track> = None;
        for &i in candidates {
            let Some(candidate) = index.tracks.get(i as usize) else {
                continue;
            };
            let candidate_album = candidate
                .album
                .as_ref()
                .map(|a| chordal_core::model::normalize(&a.name))
                .filter(|s| !s.is_empty());
            if album_key.is_some() && album_key == candidate_album {
                return Some(candidate.clone());
            }
            if fallback.is_none()
                && let (Some(a), Some(b)) = (track.duration_ms, candidate.duration_ms)
                && a.abs_diff(b) <= 3_000
            {
                fallback = Some(candidate);
            }
        }
        fallback.cloned()
    }
}

fn file_fingerprint(path: &Path) -> Option<(u64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((mtime, md.len()))
}

/// 单曲封面边车：下载器写入的 `<stem>.cover.jpg/png/...`。
/// 同目录多张专辑/多首单曲时，目录级 cover.jpg 可能张冠李戴。
fn find_track_cover(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_string_lossy();
    for ext in ["jpg", "jpeg", "png", "webp", "gif", "bmp"] {
        let candidate = dir.join(format!("{stem}.cover.{ext}"));
        if candidate.is_file() {
            return Some(format!("file://{}", candidate.to_string_lossy()));
        }
    }
    None
}

/// 同目录封面探测：常见命名，返回 `file://` URL。
fn find_folder_cover(dir: &Path) -> Option<String> {
    for name in [
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "folder.jpg",
        "folder.png",
        "front.jpg",
        "front.png",
    ] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(format!("file://{}", candidate.to_string_lossy()));
        }
    }
    None
}

/// 从扩展名 + 文件大小/时长估算音质。
///
/// 无损格式按 tier=Lossless；有损格式用 `size*8/duration_ms` 估算平均码率
/// （VBR 只是近似，但足以和在线 128/192/320 档比较）。时长未知时保留
/// `Unknown`，比较时不会被误判为高音质。
fn audio_quality_for(path: &Path, duration_ms: Option<u64>) -> Option<AudioQuality> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("?")
        .to_ascii_lowercase();
    let lossless = matches!(ext.as_str(), "flac" | "wav" | "aiff" | "aif" | "alac");
    let bitrate_kbps = duration_ms.filter(|d| *d > 0).and_then(|duration| {
        let size = std::fs::metadata(path).ok()?.len();
        if size == 0 {
            return None;
        }
        let kbps = (size as u128 * 8) / duration as u128;
        Some(kbps.min(20_000) as u32).filter(|kbps| *kbps > 0)
    });
    let tier = if lossless {
        QualityTier::Lossless
    } else {
        match bitrate_kbps {
            Some(kbps) if kbps >= 256 => QualityTier::High,
            Some(kbps) if kbps >= 96 => QualityTier::Standard,
            _ => QualityTier::Unknown,
        }
    };
    Some(AudioQuality {
        tier,
        codec: Some(ext),
        bitrate_kbps,
        sample_rate_hz: None,
        lossless,
    })
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 本地专辑的稳定合成 ID：专辑名 + 所在目录名（同目录一专辑）。
fn local_album_id(name: &str, path: &Path) -> String {
    let dir = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let key = format!(
        "{}|{}",
        chordal_core::model::normalize(name),
        chordal_core::model::normalize(&dir)
    );
    format!("al-{:016x}", fnv1a64(key.as_bytes()))
}

fn local_artist_id(name: &str) -> String {
    format!(
        "ar-{:016x}",
        fnv1a64(chordal_core::model::normalize(name).as_bytes())
    )
}

fn build_collections(
    tracks: &[Track],
) -> (
    HashMap<String, LocalAlbumAgg>,
    HashMap<String, LocalArtistAgg>,
) {
    let mut albums: HashMap<String, LocalAlbumAgg> = HashMap::new();
    let mut artists: HashMap<String, LocalArtistAgg> = HashMap::new();
    for (idx, track) in tracks.iter().enumerate() {
        if let Some(album) = &track.album
            && let Some(album_id) = &album.id
        {
            let agg = albums
                .entry(album_id.clone())
                .or_insert_with(|| LocalAlbumAgg {
                    title: album.name.clone(),
                    creator: track.artists.first().map(|a| a.name.clone()),
                    cover: album.cover.clone(),
                    year: track.year,
                    search_key: String::new(),
                    tracks: Vec::new(),
                });
            if agg.cover.is_none() {
                agg.cover = album.cover.clone();
            }
            if agg.year.is_none() {
                agg.year = track.year;
            }
            agg.tracks.push(idx as u32);
        }
        for artist in &track.artists {
            let Some(artist_id) = &artist.id else {
                continue;
            };
            let agg = artists
                .entry(artist_id.clone())
                .or_insert_with(|| LocalArtistAgg {
                    name: artist.name.clone(),
                    cover: track.album.as_ref().and_then(|a| a.cover.clone()),
                    search_key: String::new(),
                    album_ids: Vec::new(),
                    tracks: Vec::new(),
                });
            agg.tracks.push(idx as u32);
            if let Some(album) = &track.album
                && let Some(album_id) = &album.id
                && !agg.album_ids.contains(album_id)
            {
                agg.album_ids.push(album_id.clone());
            }
        }
    }
    for agg in albums.values_mut() {
        agg.tracks.sort_by(|&a, &b| {
            let ta = &tracks[a as usize];
            let tb = &tracks[b as usize];
            ta.track_no
                .unwrap_or(u32::MAX)
                .cmp(&tb.track_no.unwrap_or(u32::MAX))
                .then_with(|| ta.title.cmp(&tb.title))
        });
        agg.search_key = format!(
            "{}|{}",
            chordal_core::model::normalize(&agg.title),
            chordal_core::model::normalize(agg.creator.as_deref().unwrap_or(""))
        );
    }
    for agg in artists.values_mut() {
        agg.tracks.sort_by(|&a, &b| {
            let ta = &tracks[a as usize];
            let tb = &tracks[b as usize];
            tb.year
                .unwrap_or(0)
                .cmp(&ta.year.unwrap_or(0))
                .then_with(|| {
                    ta.track_no
                        .unwrap_or(u32::MAX)
                        .cmp(&tb.track_no.unwrap_or(u32::MAX))
                })
                .then_with(|| ta.title.cmp(&tb.title))
        });
        agg.search_key = chordal_core::model::normalize(&agg.name);
    }
    (albums, artists)
}

fn build_track(source: &SourceId, id: &str, path: &Path, meta: Option<TrackMeta>) -> Track {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (fallback_title, fallback_artist) = match stem.as_str().split_once(" - ") {
        Some((a, b)) => (b.to_string(), Some(a.to_string())),
        None => (stem.clone(), None),
    };
    let meta = meta.unwrap_or_default();
    let title = meta
        .title
        .filter(|t| !t.is_empty())
        .unwrap_or(fallback_title);
    let mut artists: Vec<ArtistRef> = meta
        .artists
        .iter()
        .filter(|a| !a.is_empty())
        .map(|a| ArtistRef {
            id: Some(local_artist_id(a)),
            name: a.clone(),
        })
        .collect();
    if artists.is_empty()
        && let Some(a) = fallback_artist
    {
        let id = local_artist_id(&a);
        artists.push(ArtistRef {
            id: Some(id),
            name: a,
        });
    }
    let album = meta.album.filter(|a| !a.is_empty()).map(|name| AlbumRef {
        id: Some(local_album_id(&name, path)),
        name,
        cover: None,
    });
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("?")
        .to_ascii_lowercase();
    let mut track = Track::new(
        TrackId::new(source.clone(), id.to_string()),
        title,
        artists,
        album,
        meta.duration_ms,
        TrackOrigin::Local {
            path: path.to_path_buf(),
        },
    );
    track.track_no = meta.track_no;
    track.year = meta.year;
    track.quality_info = audio_quality_for(path, meta.duration_ms);
    track.quality = Some(
        track
            .quality_info
            .as_ref()
            .map(AudioQuality::label)
            .unwrap_or_else(|| ext.to_ascii_uppercase()),
    );
    // 文件名也纳入模糊搜索键（用户常按文件名里的专辑/版本词搜索，
    // 而标签可能已被规范化）。单次 append，扫描期完成，查询期零成本。
    if !stem.is_empty() {
        track.search_key.push('\u{1}');
        track
            .search_key
            .push_str(&chordal_core::model::normalize(&stem));
    }
    track
}

#[async_trait]
impl MusicSource for LocalSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::local()
    }

    fn catalog(&self) -> Option<&dyn CatalogSource> {
        Some(self)
    }

    async fn search(&self, query: &SearchQuery) -> CoreResult<SearchResults> {
        let needle = query.normalized();
        let limit = query.limit.max(1) as usize;
        let offset = query.offset as usize;

        let scope = PerfScope::enter("local.search.filter");
        let index = self.index.read().expect("local index lock poisoned");
        let mut hits: Vec<&Track> = Vec::new();
        for track in &index.tracks {
            // 预计算键上的单次 substring 匹配。提前终止：命中数达到
            // offset+limit 即止，典型复杂度 O(命中数·m) 而非 O(n·m)。
            if track.search_key.contains(&needle) {
                if hits.len() >= offset + limit {
                    break;
                }
                hits.push(track);
            }
        }
        scope.finish(index.tracks.len() as u64);

        let total = hits.len() as u64;
        let tracks = hits.into_iter().skip(offset).take(limit).cloned().collect();
        Ok(SearchResults {
            source: self.info.id.clone(),
            tracks,
            total: Some(total),
            elapsed_ms: None,
        })
    }

    async fn track(&self, id: &TrackId) -> CoreResult<Track> {
        let _scope = PerfScope::enter("local.track_lookup");
        let index = self.index.read().expect("local index lock poisoned");
        index
            .by_id
            .get(&id.id) // O(1)
            .and_then(|&idx| index.tracks.get(idx as usize))
            .cloned()
            .ok_or_else(|| CoreError::source_err(self.info.id.as_str(), format!("no track {id}")))
    }

    async fn stream(&self, id: &TrackId) -> CoreResult<StreamHandle> {
        let track = self.track(id).await?;
        match track.origin {
            TrackOrigin::Local { path } => Ok(StreamHandle::File(path)),
            _ => Err(CoreError::source_err(
                self.info.id.as_str(),
                "not a local track",
            )),
        }
    }

    async fn resolve_stream(&self, id: &TrackId) -> CoreResult<ResolvedStream> {
        let track = self.track(id).await?;
        match track.origin {
            TrackOrigin::Local { path } => Ok(ResolvedStream {
                handle: StreamHandle::File(path),
                quality: track.quality_info,
            }),
            _ => Err(CoreError::source_err(
                self.info.id.as_str(),
                "not a local track",
            )),
        }
    }

    /// 同名边车歌词：`.lrc` / `.LRC` / `.txt`。一次命中即返回（O(1)）。
    /// 编码按 UTF-8(BOM) → UTF-16(BOM) → GBK 回退，兼容中文 LRC 生态。
    async fn lyrics(&self, id: &TrackId) -> CoreResult<Option<Lyrics>> {
        let track = self.track(id).await?;
        let TrackOrigin::Local { path } = track.origin else {
            return Ok(None);
        };
        for ext in ["lrc", "LRC", "txt", "TXT"] {
            let candidate = path.with_extension(ext);
            if candidate == path {
                continue;
            }
            // 边车歌词不应把 UI 线程拖死：限制 4MB，避免误读大文本文件。
            let Ok(meta) = std::fs::metadata(&candidate) else {
                continue;
            };
            if meta.len() > 4 * 1024 * 1024 {
                continue;
            }
            let Ok(bytes) = std::fs::read(&candidate) else {
                continue;
            };
            let raw = decode_lyric_bytes(&bytes);
            let mut lyrics = Lyrics::parse_lrc(&raw);
            if lyrics.lines.len() > 5_000 {
                lyrics.lines.truncate(5_000);
            }
            if !lyrics.lines.is_empty() {
                return Ok(Some(lyrics));
            }
        }
        Ok(None)
    }
}

#[async_trait]
impl CatalogSource for LocalSource {
    async fn collection(&self, id: &CollectionId) -> CoreResult<CollectionPage> {
        let _scope = PerfScope::enter("local.collection");
        let index = self.index.read().expect("local index lock poisoned");
        match id.kind {
            CollectionKind::Album => {
                let album = index.albums.get(&id.id).ok_or_else(|| {
                    CoreError::source_err(self.info.id.as_str(), format!("no album {id}"))
                })?;
                let tracks: Vec<Track> = album
                    .tracks
                    .iter()
                    .filter_map(|&i| index.tracks.get(i as usize).cloned())
                    .collect();
                let summary = CollectionSummary {
                    id: id.clone(),
                    title: album.title.clone(),
                    cover: album.cover.clone(),
                    creator: album.creator.clone(),
                    description: None,
                    track_count: Some(tracks.len() as u64),
                    year: album.year,
                    subscribed: false,
                    kind: CollectionKind::Album,
                };
                Ok(CollectionPage {
                    total: tracks.len() as u64,
                    summary: Some(summary),
                    tracks,
                    related_albums: Vec::new(),
                    offset: 0,
                    has_more: false,
                })
            }
            CollectionKind::Artist => {
                let artist = index.artists.get(&id.id).ok_or_else(|| {
                    CoreError::source_err(self.info.id.as_str(), format!("no artist {id}"))
                })?;
                let tracks: Vec<Track> = artist
                    .tracks
                    .iter()
                    .filter_map(|&i| index.tracks.get(i as usize).cloned())
                    .collect();
                let related_albums: Vec<CollectionSummary> = artist
                    .album_ids
                    .iter()
                    .filter_map(|album_id| {
                        let album = index.albums.get(album_id)?;
                        Some(CollectionSummary {
                            id: CollectionId::new(
                                self.info.id.clone(),
                                CollectionKind::Album,
                                album_id.clone(),
                            ),
                            title: album.title.clone(),
                            cover: album.cover.clone(),
                            creator: album.creator.clone(),
                            description: None,
                            track_count: Some(album.tracks.len() as u64),
                            year: album.year,
                            subscribed: false,
                            kind: CollectionKind::Album,
                        })
                    })
                    .collect();
                let summary = CollectionSummary {
                    id: id.clone(),
                    title: artist.name.clone(),
                    cover: artist.cover.clone(),
                    creator: None,
                    description: None,
                    track_count: Some(tracks.len() as u64),
                    year: None,
                    subscribed: false,
                    kind: CollectionKind::Artist,
                };
                Ok(CollectionPage {
                    total: tracks.len() as u64,
                    summary: Some(summary),
                    tracks,
                    related_albums,
                    offset: 0,
                    has_more: false,
                })
            }
            CollectionKind::Playlist => Err(CoreError::Config(
                "本地源暂不支持歌单页；请使用本地库视图".into(),
            )),
        }
    }

    async fn collection_tracks(
        &self,
        id: &CollectionId,
        offset: u32,
        limit: u32,
    ) -> CoreResult<TrackPage> {
        let page = self.collection(id).await?;
        let total = page.total;
        let tracks: Vec<Track> = page
            .tracks
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        Ok(TrackPage {
            has_more: offset as u64 + (tracks.len() as u64) < total,
            tracks,
            total,
            offset,
        })
    }

    async fn search_collections(&self, query: &SearchQuery) -> CoreResult<Vec<CollectionSummary>> {
        let needle = query.normalized();
        let limit = query.limit.max(1) as usize;
        let index = self.index.read().expect("local index lock poisoned");
        let mut out: Vec<CollectionSummary> = match query.kind {
            SearchKind::Album => index
                .albums
                .iter()
                .filter(|(_, album)| album.search_key.contains(&needle))
                .take(limit)
                .map(|(album_id, album)| CollectionSummary {
                    id: CollectionId::new(
                        self.info.id.clone(),
                        CollectionKind::Album,
                        album_id.clone(),
                    ),
                    title: album.title.clone(),
                    cover: album.cover.clone(),
                    creator: album.creator.clone(),
                    description: None,
                    track_count: Some(album.tracks.len() as u64),
                    year: album.year,
                    subscribed: false,
                    kind: CollectionKind::Album,
                })
                .collect(),
            SearchKind::Artist => index
                .artists
                .iter()
                .filter(|(_, artist)| artist.search_key.contains(&needle))
                .take(limit)
                .map(|(artist_id, artist)| CollectionSummary {
                    id: CollectionId::new(
                        self.info.id.clone(),
                        CollectionKind::Artist,
                        artist_id.clone(),
                    ),
                    title: artist.name.clone(),
                    cover: artist.cover.clone(),
                    creator: None,
                    description: None,
                    track_count: Some(artist.tracks.len() as u64),
                    year: None,
                    subscribed: false,
                    kind: CollectionKind::Artist,
                })
                .collect(),
            SearchKind::Track | SearchKind::Playlist => Vec::new(),
        };
        out.sort_by(|a, b| a.title.cmp(&b.title));
        Ok(out)
    }
}

/// 歌词文件编码探测：BOM 优先，非法 UTF-8 回退 GBK（华语 LRC 常见）。
fn decode_lyric_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let (text, _, _) = encoding_rs::UTF_16LE.decode(rest);
        return text.into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let (text, _, _) = encoding_rs::UTF_16BE.decode(rest);
        return text.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    }
}

/// symphonia 标签读取（真实实现）。
pub struct SymphoniaTagReader;

impl TagReader for SymphoniaTagReader {
    fn read(&self, path: &Path) -> Option<TrackMeta> {
        // symphonia 对部分 MP3 的 ID3v2 不可见，补一个轻量 ID3v2 解析器。
        let mut meta = read_tags_symphonia(path).unwrap_or_default();
        read_id3_into(path, &mut meta);
        if meta.title.is_none()
            && meta.artists.is_empty()
            && meta.album.is_none()
            && meta.duration_ms.is_none()
        {
            None
        } else {
            Some(meta)
        }
    }
}

/// 轻量 ID3v2.3/2.4 文本标签解析（标题/艺人/专辑/音轨/年份）。
fn read_id3_into(path: &Path, meta: &mut TrackMeta) {
    use std::io::Read;

    let Ok(mut file) = std::fs::File::open(path) else {
        return;
    };
    let mut header = [0u8; 10];
    if file.read_exact(&mut header).is_err() || &header[..3] != b"ID3" {
        return;
    }
    let major = header[3];
    if !(3..=4).contains(&major) {
        return;
    }
    let size = synchsafe(&header[6..10]) as usize;
    if size == 0 || size > 16 * 1024 * 1024 {
        return;
    }
    let mut data = vec![0u8; size];
    if file.read_exact(&mut data).is_err() {
        return;
    }

    let mut pos = 0usize;
    if header[5] & 0x40 != 0 {
        // Extended header：v2.3 是 4 字节大小，v2.4 是 synchsafe。
        if pos + 4 > data.len() {
            return;
        }
        let ext = if major == 4 {
            synchsafe(&data[pos..pos + 4]) as usize
        } else {
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize
        };
        pos += ext.max(4);
    }

    while pos + 10 <= data.len() {
        let id = &data[pos..pos + 4];
        if id[0] == 0 {
            break;
        }
        let frame_size = if major == 4 {
            synchsafe(&data[pos + 4..pos + 8]) as usize
        } else {
            u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize
        };
        if frame_size == 0 || pos + 10 + frame_size > data.len() {
            break;
        }
        if id[0] == b'T' {
            let payload = &data[pos + 10..pos + 10 + frame_size];
            let text = decode_id3_text(payload);
            match id {
                b"TIT2" if meta.title.is_none() => meta.title = Some(text),
                b"TPE1" if meta.artists.is_empty() => {
                    meta.artists = text
                        .split(['/', ';'])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                }
                b"TALB" if meta.album.is_none() => meta.album = Some(text),
                b"TRCK" if meta.track_no.is_none() => {
                    meta.track_no = text
                        .split('/')
                        .next()
                        .and_then(|s| s.trim().parse::<u32>().ok())
                }
                b"TDRC" | b"TYER" if meta.year.is_none() => {
                    meta.year = text.get(..4).and_then(|s| s.parse::<u32>().ok())
                }
                _ => {}
            }
        }
        pos += 10 + frame_size;
    }
}

fn synchsafe(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .take(4)
        .fold(0u32, |acc, b| (acc << 7) | u32::from(b & 0x7f))
}

fn decode_id3_text(payload: &[u8]) -> String {
    let Some((&encoding, body)) = payload.split_first() else {
        return String::new();
    };
    let text = match encoding {
        0 => body.iter().map(|&b| b as char).collect::<String>(),
        1 => {
            if body.len() < 2 {
                return String::new();
            }
            let (le, body) = if body.starts_with(&[0xFF, 0xFE]) {
                (true, &body[2..])
            } else if body.starts_with(&[0xFE, 0xFF]) {
                (false, &body[2..])
            } else {
                (true, body)
            };
            let units = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if le {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .take_while(|&u| u != 0)
                .collect::<Vec<_>>();
            String::from_utf16_lossy(&units)
        }
        2 => {
            let units = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect::<Vec<_>>();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(body).to_string(),
    };
    text.trim_matches(['\0', ' ']).to_string()
}

/// symphonia `meta::Value` → 字符串（0.5.4 无 to_str 访问器，按变体匹配）。
fn value_str(v: &symphonia::core::meta::Value) -> Option<&str> {
    if let symphonia::core::meta::Value::String(s) = v {
        Some(s)
    } else {
        None
    }
}

fn value_uint(v: &symphonia::core::meta::Value) -> Option<u64> {
    if let symphonia::core::meta::Value::UnsignedInt(i) = v {
        Some(*i)
    } else {
        None
    }
}

fn read_tags_symphonia(path: &Path) -> Option<TrackMeta> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::{MetadataOptions, StandardTagKey};
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut reader = probed.format;

    let mut meta = TrackMeta::default();
    // MP3 的 ID3v2 常作为后续 metadata revision 出现；先读一个 packet 再跳到最新。
    let _ = reader.next_packet();
    if let Some(md) = reader.metadata().skip_to_latest() {
        for tag in md.tags() {
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) => {
                    meta.title = value_str(&tag.value).map(str::to_string)
                }
                Some(StandardTagKey::Artist) => {
                    if let Some(v) = value_str(&tag.value) {
                        meta.artists.push(v.to_string());
                    }
                }
                Some(StandardTagKey::AlbumArtist) => {
                    if let Some(v) = value_str(&tag.value) {
                        meta.artists.push(v.to_string());
                    }
                }
                Some(StandardTagKey::Album) => {
                    meta.album = value_str(&tag.value).map(str::to_string)
                }
                Some(StandardTagKey::TrackNumber) => {
                    meta.track_no = value_uint(&tag.value).map(|i| i as u32)
                }
                Some(StandardTagKey::Date) => {
                    meta.year = value_str(&tag.value)
                        .and_then(|s| s.get(0..4))
                        .and_then(|s| s.parse().ok());
                }
                _ => {}
            }
        }
    }
    // 容器时长：n_frames × time_base（symphonia 0.5.4 的 trait 上没有
    // duration()/time_base()，信息在 CodecParameters 里）。
    if let Some(track) = reader
        .tracks()
        .iter()
        .find(|t| t.codec_params.sample_rate.is_some())
    {
        let params = &track.codec_params;
        if let (Some(tb), Some(frames)) = (params.time_base, params.n_frames) {
            let t = tb.calc_time(frames);
            let secs = t.seconds as f64 + t.frac;
            if secs > 0.0 {
                meta.duration_ms = Some((secs * 1000.0) as u64);
            }
        }
    }
    meta.artists.dedup();
    Some(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicU64;

    /// 计数假标签读取器：断言增量扫描只重读变更文件。
    struct SharedReader(Arc<AtomicU64>);

    impl TagReader for SharedReader {
        fn read(&self, _path: &Path) -> Option<TrackMeta> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some(TrackMeta {
                title: Some("tagged title".into()),
                artists: vec!["fake artist".into()],
                album: Some("fake album".into()),
                duration_ms: Some(100_000),
                track_no: None,
                year: Some(2024),
            })
        }
    }

    fn make_source(roots: Vec<PathBuf>) -> (Arc<LocalSource>, Arc<AtomicU64>) {
        let calls = Arc::new(AtomicU64::new(0));
        let src = LocalSource::new_with_tag_reader(
            SourceId::new("local-test"),
            LocalSourceConfig { roots },
            "test",
            Box::new(SharedReader(calls.clone())),
        );
        (src, calls)
    }

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, b"fake audio bytes").unwrap();
        p
    }

    #[test]
    fn scan_index_and_search() {
        let tmp = std::env::temp_dir().join(format!("chordial-local-{}", std::process::id()));
        let sub = tmp.join("sub");
        fs::create_dir_all(&sub).unwrap();
        write_file(&tmp, "Beyond - 海阔天空.mp3");
        write_file(&sub, "fake artist - tune.flac");
        write_file(&tmp, "notes.txt"); // 非音频，忽略

        let (src, reader_calls) = make_source(vec![tmp.clone()]);
        let summary = src.scan_blocking();
        assert_eq!(summary.tracks, 2);
        assert_eq!(reader_calls.load(Ordering::SeqCst), 2);

        // O(1) 主键查找
        let any_id = src.all_tracks()[0].id.id.clone();
        let track = block_test(async {
            src.track(&TrackId::new(SourceId::new("local-test"), any_id))
                .await
        });
        assert!(track.is_ok());

        // 搜索：中文子串 + 大小写不敏感
        let results = block_test(async { src.search(&SearchQuery::track("海阔")).await }).unwrap();
        assert_eq!(results.track_count(), 1);
        assert_eq!(results.tracks[0].title, "tagged title");

        // 两首文件的标签都来自假读取器（artist=fake artist），按艺人搜索应两首都命中。
        let results =
            block_test(async { src.search(&SearchQuery::track("FAKE ARTIST")).await }).unwrap();
        assert_eq!(results.track_count(), 2);

        // 增量：立即重扫，0 次标签读取
        src.scan_blocking();
        assert_eq!(
            reader_calls.load(Ordering::SeqCst),
            2,
            "unchanged files must not be re-read"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn incremental_rescan_only_rechanges() {
        let tmp = std::env::temp_dir().join(format!("chordial-inc-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        write_file(&tmp, "a - one.mp3");
        write_file(&tmp, "b - two.mp3");

        let (src, reader_calls) = make_source(vec![tmp.clone()]);
        src.scan_blocking();
        assert_eq!(reader_calls.load(Ordering::SeqCst), 2);

        // 新增一个文件：只读新文件的标签
        write_file(&tmp, "c - three.mp3");
        src.scan_blocking();
        assert_eq!(reader_calls.load(Ordering::SeqCst), 3);
        assert_eq!(src.last_summary().tracks, 3);

        // 删除一个文件：索引同步收缩
        fs::remove_file(tmp.join("b - two.mp3")).unwrap();
        src.scan_blocking();
        assert_eq!(src.last_summary().tracks, 2);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn filename_fallback_parsing() {
        let t = build_track(
            &SourceId::new("local"),
            "x",
            Path::new("/m/Beyond - 海阔天空.mp3"),
            None,
        );
        assert_eq!(t.title, "海阔天空");
        assert_eq!(t.primary_artist(), "Beyond");

        let t = build_track(
            &SourceId::new("local"),
            "y",
            Path::new("/m/just_title.flac"),
            None,
        );
        assert_eq!(t.title, "just_title");
        assert_eq!(t.primary_artist(), "");
    }

    #[test]
    fn quality_estimation_prefers_lossless() {
        let tmp = std::env::temp_dir().join(format!("chordial-quality-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let flac = tmp.join("a.flac");
        let mp3 = tmp.join("b.mp3");
        fs::write(&flac, vec![0u8; 1_250_000]).unwrap(); // 10s ≈ 1000 kbps
        fs::write(&mp3, vec![0u8; 400_000]).unwrap(); //  10s ≈ 320 kbps
        let flac_q = audio_quality_for(&flac, Some(10_000)).unwrap();
        let mp3_q = audio_quality_for(&mp3, Some(10_000)).unwrap();
        assert!(flac_q.lossless);
        assert!(!mp3_q.lossless);
        assert!(flac_q.score() > mp3_q.score());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn local_catalog_builds_album_and_artist_pages() {
        let tmp = std::env::temp_dir().join(format!("chordial-catalog-{}", std::process::id()));
        fs::create_dir_all(tmp.join("album-dir")).unwrap();
        write_file(&tmp.join("album-dir"), "a.mp3");
        write_file(&tmp.join("album-dir"), "b.mp3");
        let (src, _) = make_source(vec![tmp.clone()]);
        src.scan_blocking();
        let tracks = src.all_tracks();
        assert_eq!(tracks.len(), 2);
        let album_ref = tracks[0].album.as_ref().unwrap();
        let album_id = album_ref.id.clone().expect("album id");
        let artist_ref = tracks[0].artists.first().unwrap();
        let artist_id = artist_ref.id.clone().expect("artist id");

        let catalog = src.catalog().expect("catalog");
        let album = block_test(async {
            catalog
                .collection(&CollectionId::new(
                    SourceId::new("local-test"),
                    CollectionKind::Album,
                    album_id.clone(),
                ))
                .await
        })
        .unwrap();
        assert_eq!(album.tracks.len(), 2);
        assert_eq!(album.summary.as_ref().unwrap().kind, CollectionKind::Album);

        let artist = block_test(async {
            catalog
                .collection(&CollectionId::new(
                    SourceId::new("local-test"),
                    CollectionKind::Artist,
                    artist_id.clone(),
                ))
                .await
        })
        .unwrap();
        assert_eq!(artist.tracks.len(), 2);
        assert_eq!(artist.related_albums.len(), 1);

        let found = block_test(async {
            catalog
                .search_collections(&SearchQuery {
                    text: "fake album".into(),
                    limit: 10,
                    offset: 0,
                    kind: SearchKind::Album,
                })
                .await
        })
        .unwrap();
        assert_eq!(found.len(), 1);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn finds_local_equivalent_by_album_then_duration() {
        let src = LocalSource::new(
            SourceId::new("local-test"),
            LocalSourceConfig {
                roots: vec![std::env::temp_dir()],
            },
            "test",
        );
        let local = Track::new(
            TrackId::new(SourceId::new("local-test"), "/music/a.flac"),
            "海阔天空",
            vec![ArtistRef {
                id: Some("ar-1".into()),
                name: "Beyond".into(),
            }],
            Some(AlbumRef {
                id: Some("al-1".into()),
                name: "乐与怒".into(),
                cover: None,
            }),
            Some(326_000),
            TrackOrigin::Local {
                path: PathBuf::from("/music/a.flac"),
            },
        );
        src.inject_index_for_tests(vec![local.clone()]);

        // 同专辑 → 命中。
        let remote = Track::new(
            TrackId::new(SourceId::new("netease"), "42"),
            "海阔天空",
            vec![ArtistRef {
                id: Some("1".into()),
                name: "Beyond".into(),
            }],
            Some(AlbumRef {
                id: Some("9".into()),
                name: "乐与怒".into(),
                cover: None,
            }),
            Some(325_000),
            TrackOrigin::Remote {
                source: SourceId::new("netease"),
            },
        );
        assert_eq!(src.find_equivalent(&remote).unwrap().id, local.id);

        // 不同专辑且时长差 >3s → 不命中。
        let live = Track::new(
            TrackId::new(SourceId::new("netease"), "43"),
            "海阔天空",
            vec![ArtistRef {
                id: Some("1".into()),
                name: "Beyond".into(),
            }],
            Some(AlbumRef {
                id: Some("10".into()),
                name: "Live 1993".into(),
                cover: None,
            }),
            Some(400_000),
            TrackOrigin::Remote {
                source: SourceId::new("netease"),
            },
        );
        assert!(src.find_equivalent(&live).is_none());
    }

    /// 极简 async 测试驱动：本 crate 的 async 方法从不开 await 挂起，
    /// 用 noop-waker 单步轮询即可，避免引入 tokio dev-dep。
    fn block_test<F: std::future::Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        static VTABLE: RawWakerVTable =
            RawWakerVTable::new(|p| RawWaker::new(p, &VTABLE), |_| {}, |_| {}, |_| {});
        let mut fut = std::pin::pin!(fut);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => unreachable!("local-source futures never await"),
        }
    }

    #[test]
    fn decodes_utf16_bom_lyrics() {
        let bytes = [0xFF, 0xFE, 0x60, 0x4F, 0x7D, 0x59]; // “你好” UTF-16LE
        assert_eq!(decode_lyric_bytes(&bytes), "你好");
    }

    #[test]
    fn detects_folder_cover() {
        let tmp = std::env::temp_dir().join(format!("chordial-cover-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        write_file(&tmp, "song.mp3");
        fs::write(tmp.join("cover.jpg"), b"fake image").unwrap();
        let (src, _) = make_source(vec![tmp.clone()]);
        src.scan_blocking();
        let track = &src.all_tracks()[0];
        let cover = track.album.as_ref().and_then(|a| a.cover.as_deref());
        assert!(cover.is_some_and(|c| c.starts_with("file://") && c.ends_with("cover.jpg")));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn prefers_per_track_cover_sidecar() {
        let tmp = std::env::temp_dir().join(format!("chordial-track-cover-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        write_file(&tmp, "song.mp3");
        fs::write(tmp.join("cover.jpg"), b"folder image").unwrap();
        fs::write(tmp.join("song.cover.png"), b"track image").unwrap();
        let (src, _) = make_source(vec![tmp.clone()]);
        src.scan_blocking();
        let track = &src.all_tracks()[0];
        let cover = track.album.as_ref().and_then(|a| a.cover.as_deref());
        assert!(cover.is_some_and(|c| c.starts_with("file://") && c.ends_with("song.cover.png")));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn reads_sidecar_lrc_and_gbk() {
        let tmp = std::env::temp_dir().join(format!("chordial-lrc-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let audio = write_file(&tmp, "song.mp3");
        let (src, _) = make_source(vec![tmp.clone()]);
        src.scan_blocking();
        let id = src.all_tracks()[0].id.clone();

        // UTF-8 边车歌词。
        fs::write(
            audio.with_extension("lrc"),
            "[00:01.00]你好\n[00:02.00]世界",
        )
        .unwrap();
        let lyrics = block_test(async { src.lyrics(&id).await })
            .unwrap()
            .unwrap();
        assert_eq!(lyrics.lines.len(), 2);
        assert_eq!(lyrics.lines[0].text, "你好");

        // 非法 UTF-8 → GBK 回退（“海阔天空”的 GBK 字节）。
        let gbk = [0xBA, 0xA3, 0xC0, 0xAB, 0xCC, 0xEC, 0xBF, 0xD5];
        let mut file = b"[00:03.00]".to_vec();
        file.extend_from_slice(&gbk);
        fs::write(audio.with_extension("lrc"), file).unwrap();
        let lyrics = block_test(async { src.lyrics(&id).await })
            .unwrap()
            .unwrap();
        assert_eq!(lyrics.lines[0].text, "海阔天空");

        let _ = fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod id3_tests {
    use super::*;

    fn frame(id: &[u8; 4], text: &str) -> Vec<u8> {
        let mut payload = vec![3u8]; // UTF-8
        payload.extend_from_slice(text.as_bytes());
        let mut out = Vec::new();
        out.extend_from_slice(id);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&payload);
        out
    }

    fn synchsafe_encode(n: usize) -> [u8; 4] {
        [
            ((n >> 21) & 0x7f) as u8,
            ((n >> 14) & 0x7f) as u8,
            ((n >> 7) & 0x7f) as u8,
            (n & 0x7f) as u8,
        ]
    }

    #[test]
    fn reads_id3v23_text_frames() {
        let mut body = Vec::new();
        body.extend(frame(b"TIT2", "夜曲"));
        body.extend(frame(b"TPE1", "周杰伦"));
        body.extend(frame(b"TALB", "十一月的萧邦"));
        body.extend(frame(b"TRCK", "3/12"));
        let mut bytes = b"ID3\x03\x00\x00".to_vec();
        bytes.extend_from_slice(&synchsafe_encode(body.len()));
        bytes.extend_from_slice(&body);

        let path = std::env::temp_dir().join(format!("chordial-id3-{}.mp3", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let mut meta = TrackMeta::default();
        read_id3_into(&path, &mut meta);
        let _ = std::fs::remove_file(&path);
        assert_eq!(meta.title.as_deref(), Some("夜曲"));
        assert_eq!(meta.artists, vec!["周杰伦".to_string()]);
        assert_eq!(meta.album.as_deref(), Some("十一月的萧邦"));
        assert_eq!(meta.track_no, Some(3));
    }
}
