//! 根视图的类型与缓存结构（由 app/mod.rs 拆分，行为不变）。

use std::collections::{HashSet, VecDeque};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewTab {
    Search,
    Library,
    Collection,
}

/// 曲目列表的来源标识。所有列表共用同一套队列操作与同一行渲染器。
///
/// 带数据的变体（歌单 / 推荐流）不能 `Copy`，调用方按需 `clone()`。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum QueueList {
    Search,
    Library,
    Queue,
    Collection,
    /// 本地/订阅歌单页。
    Playlist(chordal_core::playlist::PlaylistId),
    /// 音源推荐流（今日推荐 / 私人 FM）。
    Feed {
        source: SourceId,
        kind: SourceFeedKind,
    },
    /// 页面内的一次性曲目快照（音源页预览等），无需在 App 上单独存状态。
    Snapshot(Arc<Vec<chordal_core::model::Track>>),
}

/// 当前队列的续接追踪：私人 FM 播放时记录所追踪的音源与在途状态。
///
/// `None` 表示有限列表（本地库/搜索/歌单/集合/每日推荐等），播完按播放模式
/// 处理；`Some` 表示接近末尾时要向该音源再次请求 `personal_fm()` 并追加。
#[derive(Clone, PartialEq, Eq)]
pub(super) struct QueueRefill {
    pub(super) source: SourceId,
    pub(super) in_flight: bool,
    /// 上次成功但没拿到新曲目的位置；避免尾部每首歌都重复打接口。
    pub(super) last_empty_at: Option<usize>,
}

/// 主内容区的“整页”视图。`None` = 现有的曲库/搜索/集合三栏内容。
///
/// 音源页/管理页/本地歌单页需要完整内容宽度，因此作为覆盖页渲染，
/// 侧栏与 Dock 保持不变。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MainPage {
    /// 单个音源的状态/功能页。
    Source(SourceId),
    /// 音源端点状态列表。
    ManageSources,
    /// 新增音源：选择类型。
    NewSource,
    /// 二级编辑某个音源实例。
    EditSource(SourceId),
    /// 本地/订阅歌单页。
    Playlist(chordal_core::playlist::PlaylistId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LibraryFilter {
    All,
    Queue,
}

/// 底部 Dock 的弹出菜单。歌词不再有浮层：曲目区直接进入沉浸页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DockMenu {
    /// 播放队列概览：下 3 首 + 打开完整队列。
    Queue,
    /// 播放来源 / 下载 / 均衡器等低频操作。
    More,
}

/// 底部「更多」抽屉的二级页。抽屉本身不改变播放页布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DrawerPage {
    Root,
    Sleep,
    /// 把当前曲目加入本地歌单的二级页；歌单列表直接画在抽屉里。
    AddToPlaylist,
}

/// 沉浸页右下角选择器的四种视图。
///
/// 前三种共用同一个右侧内容区（与宽屏歌词列完全相同的几何），
/// `MainOnly` 则收起整个右侧内容区，只保留中间的播放列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StageView {
    /// 宽屏右列 / 窄窗全屏歌词。
    Lyrics,
    /// 右侧内容区显示播放队列。
    Queue,
    /// 右侧内容区显示当前曲目的音频详情。
    Audio,
    /// 仅显示主播放列。
    MainOnly,
}

impl StageView {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Lyrics => "显示歌词",
            Self::Queue => "显示播放队列",
            Self::Audio => "音频详情",
            Self::MainOnly => "仅显示主内容",
        }
    }

    pub(super) fn icon(self) -> crate::icons::Icon {
        match self {
            Self::Lyrics => crate::icons::Icon::Lyrics,
            Self::Queue => crate::icons::Icon::Queue,
            Self::Audio => crate::icons::Icon::Sliders,
            Self::MainOnly => crate::icons::Icon::Expand,
        }
    }
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

/// 曲目菜单里的“添加到歌单”浮层。
pub(super) struct PlaylistPickerState {
    pub(super) track: Track,
    pub(super) x: f32,
    pub(super) y: f32,
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
pub(super) fn phone_default_page(
    view_tab: ViewTab,
    filter: LibraryFilter,
    has_track: bool,
) -> PhonePage {
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

/// 歌单页的一行：预计算文本 + 可播放性。
pub(super) struct PlaylistRowView {
    pub(super) track: Track,
    pub(super) identity_key: String,
    pub(super) title: SharedString,
    pub(super) artist: SharedString,
    pub(super) source_label: SharedString,
    pub(super) duration: SharedString,
    /// 源未加载且本地没有同身份副本。
    pub(super) unavailable: bool,
    /// 源未加载，但本地库有同身份副本（点击播放本地副本）。
    pub(super) local_substitute: bool,
}

pub(super) struct PlaylistPageCache {
    pub(super) id: chordal_core::playlist::PlaylistId,
    pub(super) updated_at_ms: u64,
    pub(super) registry_version: u64,
    pub(super) library_ptr: usize,
    pub(super) rows: Arc<Vec<PlaylistRowView>>,
}

/// 账号源页面的推荐流快照。各区块独立记录错误与加载态，
/// 单个接口失败不会拖垮整页。
#[derive(Default)]
pub(super) struct SourceFeedState {
    pub(super) daily: Vec<Track>,
    pub(super) fm: Vec<Track>,
    pub(super) recommended: Vec<CollectionSummary>,
    pub(super) radar: Vec<CollectionSummary>,
    pub(super) daily_error: Option<SharedString>,
    pub(super) fm_error: Option<SharedString>,
    pub(super) recommended_error: Option<SharedString>,
    pub(super) radar_error: Option<SharedString>,
    pub(super) loading_daily: bool,
    pub(super) loading_fm: bool,
    pub(super) loading_recommended: bool,
    pub(super) loading_radar: bool,
    pub(super) fetched_at_ms: u64,
}

impl SourceFeedState {
    pub(super) fn loading(&self) -> bool {
        self.loading_daily || self.loading_fm || self.loading_recommended || self.loading_radar
    }

    pub(super) fn has_content(&self) -> bool {
        !self.daily.is_empty()
            || !self.fm.is_empty()
            || !self.recommended.is_empty()
            || !self.radar.is_empty()
    }
}

/// 网易云源页面里「推荐入口」点开后展示的详情类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SourceFeedKind {
    Daily,
    Fm,
    Recommended,
    Radar,
}

impl SourceFeedKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Daily => "今日推荐",
            Self::Fm => "私人 FM",
            Self::Recommended => "每日推荐歌单",
            Self::Radar => "私人雷达",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SourceFeedDetail {
    pub(super) source: SourceId,
    pub(super) kind: SourceFeedKind,
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

/// 所有音乐列表共用的一行样式。差异只体现在列与密度，行结构完全一致：
/// 序号/均衡器 → 封面 → 标题/副标题 → 时长 → ⋮（截图版本）。
#[derive(Clone)]
pub(super) struct TrackRowStyle {
    pub(super) height: f32,
    pub(super) cover: f32,
    pub(super) show_duration: bool,
    pub(super) show_menu: bool,
    /// Dock「接下来播放」的紧凑密度。
    pub(super) compact: bool,
    /// 歌单页用副标题尾部显示来源（替代原来的独立来源列）。
    pub(super) source_label: Option<SharedString>,
    pub(super) unavailable: bool,
    pub(super) local_substitute: bool,
}

impl TrackRowStyle {
    /// 截图中的完整行：52px 高、34px 封面、时长 + ⋮。
    pub(super) fn full() -> Self {
        Self {
            height: crate::theme::size::ROW,
            cover: 34.0,
            show_duration: true,
            show_menu: true,
            compact: false,
            source_label: None,
            unavailable: false,
            local_substitute: false,
        }
    }

    /// Dock 预览：保留同样的列结构，只是更矮、更小。
    pub(super) fn compact() -> Self {
        Self {
            height: 38.0,
            cover: 30.0,
            show_duration: true,
            show_menu: false,
            compact: true,
            source_label: None,
            unavailable: false,
            local_substitute: false,
        }
    }
}

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
