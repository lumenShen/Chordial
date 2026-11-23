//! 根视图的类型与缓存结构（由 app/mod.rs 拆分，行为不变）。

use std::collections::{HashSet, VecDeque};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewTab {
    Search,
    Library,
    Collection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueueList {
    Search,
    Library,
    Queue,
    Collection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LibraryFilter {
    All,
    Queue,
}

/// 底部「更多」抽屉的二级页。抽屉本身不改变播放页布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DrawerPage {
    Root,
    Sleep,
}

/// 手机壳（单列）当前页面。宽屏没有这个概念：侧栏/曲目板/右栏同时存在。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PhonePage {
    /// 播放页：封面 + 曲目信息 + 进度 + 传输 + 音量。
    Player,
    /// 歌词页：封面驱动的 Mesh + 全屏滚动歌词。
    Lyrics,
    /// 曲库页：复用桌面曲目板（本地库 / 队列 / 搜索结果 / 集合）。
    Browse,
}

/// 手机壳与 `lyric_view` 的共享状态。
///
/// 播放页把歌词视图当背景（隐藏文字）；歌词页显示文字。两者共用同一个
/// 实体，必须显式区分，不能只用一个 bool 表示“占用”。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PhoneBackdrop {
    Off,
    Player,
    Lyrics,
}

/// 曲目行右键 / 「⋮」菜单。坐标是窗口坐标，渲染时再夹到视口内。
pub(super) struct TrackMenuState {
    pub(super) list: QueueList,
    pub(super) index: usize,
    pub(super) track: Track,
    pub(super) x: f32,
    pub(super) y: f32,
    /// 当前曲目才显示「暂停 / 继续」。
    pub(super) is_current: bool,
    pub(super) has_album: bool,
    pub(super) has_artist: bool,
    pub(super) in_queue: bool,
}

/// 手机/桌面的多选快照。进入多选后列表仍可继续更换，但批量操作只
/// 作用于进入时的快照，避免用户滚动/切页后误删到别的行。
pub(super) struct SelectionState {
    pub(super) list: QueueList,
    pub(super) tracks: Arc<Vec<Track>>,
    pub(super) selected: Vec<bool>,
}

impl SelectionState {
    pub(super) fn count(&self) -> usize {
        self.selected.iter().filter(|v| **v).count()
    }

    pub(super) fn toggle(&mut self, index: usize) {
        if let Some(slot) = self.selected.get_mut(index) {
            *slot = !*slot;
        }
    }

    pub(super) fn select_all(&mut self) {
        let all = !self.selected.iter().all(|v| *v);
        self.selected.fill(all);
    }

    pub(super) fn selected_tracks(&self) -> Vec<Track> {
        self.selected
            .iter()
            .enumerate()
            .filter(|(_, chosen)| **chosen)
            .filter_map(|(index, _)| self.tracks.get(index).cloned())
            .collect()
    }
}

/// 多选批量动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectionAction {
    Play,
    PlayNext,
    Append,
    Remove,
}

/// 曲目行拖拽排序（队列内）。`from` 在起手时固定，`to` 跟随指针。
#[derive(Clone, Copy, Debug)]
pub(super) struct QueueDragState {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) start_y: f32,
    pub(super) engaged: bool,
}

/// 收窄到手机宽度时默认落在哪一页。
///
/// 用户正在搜索/看集合/看队列时，保留他的浏览上下文；只有在普通曲库
/// 或播放中时才落到播放页——否则“拉窄窗口”会把正在找歌的人踢回播放器。
pub(super) fn phone_default_page(view_tab: ViewTab, filter: LibraryFilter, has_track: bool) -> PhonePage {
    let browsing = match view_tab {
        ViewTab::Search | ViewTab::Collection => true,
        ViewTab::Library => filter == LibraryFilter::Queue,
    };
    if browsing || !has_track {
        PhonePage::Browse
    } else {
        PhonePage::Player
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlayMode {
    Sequential,
    /// 列表循环：播到队尾回绕到第一首。
    RepeatAll,
    Shuffle,
    RepeatOne,
}

impl PlayMode {
    pub(super) fn full_label(self) -> &'static str {
        match self {
            Self::Sequential => "顺序播放模式",
            Self::RepeatAll => "列表循环播放模式",
            Self::Shuffle => "随机播放模式",
            Self::RepeatOne => "单曲循环模式",
        }
    }

    pub(super) fn short_label(self) -> &'static str {
        match self {
            Self::Sequential => "顺序",
            Self::RepeatAll => "循环",
            Self::Shuffle => "随机",
            Self::RepeatOne => "单曲",
        }
    }

    pub(super) fn next(self) -> Self {
        match self {
            Self::Sequential => Self::RepeatAll,
            Self::RepeatAll => Self::RepeatOne,
            Self::RepeatOne => Self::Shuffle,
            Self::Shuffle => Self::Sequential,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoginTab {
    Qr,
    Phone,
}

/// AMLL `TextMarquee` 的悬停滚动状态。
#[derive(Clone, Copy)]
pub(super) struct InfoMarquee {
    /// 0 = 曲名，1 = 艺人。
    pub(super) line: u8,
    pub(super) start: Instant,
    /// 需要滚动的距离（px）。
    pub(super) distance: f32,
    /// 单程时长（ms）；AMLL 的公式是 distance*2/64*1000。
    pub(super) duration_ms: f32,
}

impl InfoMarquee {
    pub(super) fn active(self, now: Instant) -> bool {
        now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 < self.duration_ms * 2.0
    }

    pub(super) fn offset(self, now: Instant) -> f32 {
        let dt = now.saturating_duration_since(self.start).as_secs_f32() * 1000.0;
        let t = dt / self.duration_ms.max(1.0);
        if t >= 2.0 {
            0.0
        } else if t <= 1.0 {
            self.distance * t
        } else {
            self.distance * (2.0 - t)
        }
    }
}

/// 右栏歌词预览的一行（已压平成纯文本 + 译文）。
#[derive(Clone)]
pub(super) struct PreviewLyric {
    pub(super) start_ms: u64,
    pub(super) text: SharedString,
    pub(super) translation: SharedString,
}

/// 估算当前歌词模型常驻字节（AMLL 行/词 + 右栏预览副本）。
///
/// 只统计 `String` 容量与结构体本身，不含 gpui 整形缓存（那部分由
/// `chordial-amll-gpui::text` 的 LRU 上界约束）。用于内存诊断的归因，
/// 数值偏保守但对"歌词是否是内存大头"足够判断。
pub(super) fn estimate_lyrics_bytes(
    lines: &[chordial_amll_gpui::LyricLine],
    preview: &[PreviewLyric],
) -> usize {
    use std::mem::size_of;
    let mut bytes = lines.len() * size_of::<chordial_amll_gpui::LyricLine>();
    bytes += preview.len() * size_of::<PreviewLyric>();
    for line in lines {
        bytes += line.translated_lyric.capacity() + line.roman_lyric.capacity();
        bytes += line.words.len() * size_of::<chordial_amll_gpui::LyricWord>();
        for word in &line.words {
            bytes += word.word.capacity() + word.roman_word.capacity();
        }
    }
    for line in preview {
        bytes += line.text.len() + line.translation.len();
    }
    bytes
}

/// 估算本地音乐库的曲目模型常驻字节（结构体 + 主要字符串/索引）。
pub(super) fn estimate_library_bytes(tracks: &[Track]) -> usize {
    use std::mem::size_of;
    let mut bytes = tracks.len() * size_of::<Track>();
    for track in tracks {
        bytes += track.id.id.capacity()
            + track.id.source.0.capacity()
            + track.title.capacity()
            + track.dedup_key.capacity()
            + track.search_key.capacity()
            + track.quality.as_ref().map_or(0, |q| q.capacity());
        for artist in &track.artists {
            bytes += size_of::<chordal_core::model::ArtistRef>()
                + artist.name.capacity()
                + artist.id.as_ref().map_or(0, |id| id.capacity());
        }
        if let Some(album) = &track.album {
            bytes += size_of::<chordal_core::model::AlbumRef>()
                + album.name.capacity()
                + album.id.as_ref().map_or(0, |id| id.capacity())
                + album.cover.as_ref().map_or(0, |cover| cover.capacity());
        }
        if let chordal_core::model::TrackOrigin::Local { path } = &track.origin {
            bytes += path.as_os_str().len();
        }
    }
    bytes
}

/// 当前打开的集合页（歌单 / 专辑 / 歌手）。
#[derive(Clone)]
pub(super) struct CollectionView {
    pub(super) id: CollectionId,
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) creator: Option<String>,
    pub(super) description: Option<String>,
    pub(super) tracks: Arc<Vec<Track>>,
    pub(super) related_albums: Arc<Vec<CollectionSummary>>,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
}

pub(super) struct PlayerClock(pub(super) Arc<Player>);

impl PlaybackClock for PlayerClock {
    fn position_ms(&self) -> u64 {
        self.0.clock().0
    }

    fn is_playing(&self) -> bool {
        self.0.clock().1
    }

    fn snapshot(&self) -> (u64, bool) {
        self.0.clock()
    }
}

/// 列表封面的缓存键：同一个封面 URL 在不同音源下走各自的对象缓存命名空间。
pub(super) fn track_cover_key(track: &Track) -> Option<String> {
    let url = cover_url(track)?;
    Some(format!("{}|{}", track.id.source.as_str(), url))
}

/// 曲目行封面缩略图缓存。只缓存当前列表实际滚动到的行，避免一次性为
/// 上千首曲目下载封面；`pending`/`failed` 保证同一 URL 不会重复请求。
pub(super) struct TrackCovers {
    pub(super) images: HashMap<String, Arc<RenderImage>>,
    pub(super) pending: HashSet<String>,
    pub(super) failed: HashSet<String>,
    /// `images` 的插入顺序，超出上限时回收最久未更新的纹理。
    pub(super) order: VecDeque<String>,
}

impl TrackCovers {
    pub(super) fn new() -> Self {
        Self {
            images: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            order: VecDeque::new(),
        }
    }
}

/// 列表缩略图最多保留的纹理数。每张 72×72 BGRA ≈ 20KB，上限同时约束
/// sprite atlas 的膨胀；超限时最旧的图片排队交给 `cx.drop_image` 回收。
pub(super) const MAX_TRACK_COVERS: usize = 160;
pub(super) const TRACK_COVER_SIDE: u32 = 72;
/// 同时进行的曲目封面下载上限。
///
/// 每次下载都在 gpui 后台执行器线程里做阻塞 IO（`fetch_cover_blocking`）；
/// 列表滚动时若每帧都无上限地追加请求，会堆积数百个阻塞线程，耗尽
/// Windows 线程池并触发系统级配额失败。限制在途数量即可保持在 8 个线程。
pub(super) const MAX_INFLIGHT_TRACK_COVERS: usize = 8;

/// 曲目行的预计算显示文本。列表快照变化时一次性构建，render 期间只做
/// `SharedString` 克隆（引用计数），不再 `format!`/`join`/`to_string`。
pub(super) struct TrackRowText {
    pub(super) id: chordal_core::model::TrackId,
    pub(super) cover_key: Option<String>,
    pub(super) number: SharedString,
    pub(super) title: SharedString,
    pub(super) subtitle_search: SharedString,
    pub(super) subtitle_default: SharedString,
    pub(super) duration: SharedString,
}

impl TrackRowText {
    pub(super) fn new(index: usize, track: &Track) -> Self {
        let artists = track
            .artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        let album = track.album.as_ref().map(|a| a.name.as_str()).unwrap_or("");
        let source_label = match &track.origin {
            TrackOrigin::Local { .. } => "本地",
            TrackOrigin::Remote { source } => match source.0.as_str() {
                "netease" => "网易云",
                other => other,
            },
        };
        let subtitle_search = if artists.is_empty() {
            source_label.to_string()
        } else {
            format!("{artists}  ·  {source_label}")
        };
        let subtitle_default = if !artists.is_empty() && !album.is_empty() {
            format!("{artists}  ·  {album}")
        } else if !artists.is_empty() {
            artists
        } else {
            album.to_string()
        };
        let duration = track
            .duration_ms
            .map(fmt_ms)
            .unwrap_or_else(|| "--:--".into());
        Self {
            id: track.id.clone(),
            cover_key: track_cover_key(track),
            number: SharedString::from((index + 1).to_string()),
            title: SharedString::from(track.title.clone()),
            subtitle_search: SharedString::from(subtitle_search),
            subtitle_default: SharedString::from(subtitle_default),
            duration: SharedString::from(duration),
        }
    }
}

/// 「正在播放」面板的预计算文本，曲目变化时才重建。
pub(super) struct NowPlayingText {
    pub(super) title: SharedString,
    pub(super) artists: SharedString,
    pub(super) album: SharedString,
    pub(super) source_label: SharedString,
    pub(super) quality: SharedString,
}

impl NowPlayingText {
    pub(super) fn new(track: Option<&Track>, fallback_title: Option<&str>) -> Self {
        let title = track
            .map(|t| t.title.as_str())
            .or(fallback_title)
            .unwrap_or("未在播放");
        let artists = track
            .map(|t| {
                t.artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        let album = track
            .and_then(|t| t.album.as_ref())
            .map(|a| a.name.as_str())
            .unwrap_or("");
        let source_label = track
            .map(|t| match &t.origin {
                TrackOrigin::Local { .. } => "本地",
                TrackOrigin::Remote { source } => match source.0.as_str() {
                    "netease" => "网易云",
                    other => other,
                },
            })
            .unwrap_or("");
        let quality = track.and_then(|t| t.quality.as_deref()).unwrap_or("");
        Self {
            title: SharedString::from(title.to_string()),
            artists: SharedString::from(artists),
            album: SharedString::from(album.to_string()),
            source_label: SharedString::from(source_label.to_string()),
            quality: SharedString::from(quality.to_string()),
        }
    }
}

/// 侧栏「我的歌单」行的预计算文本/ID。
pub(super) struct PlaylistRowText {
    pub(super) id: CollectionId,
    pub(super) element_id: SharedString,
    pub(super) title: SharedString,
    pub(super) count: SharedString,
}

impl PlaylistRowText {
    pub(super) fn new(playlist: &CollectionSummary) -> Self {
        Self {
            id: playlist.id.clone(),
            element_id: SharedString::from(format!("user-playlist-{}", playlist.id.id)),
            title: SharedString::from(playlist.title.clone()),
            count: SharedString::from(
                playlist
                    .track_count
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
            ),
        }
    }
}
