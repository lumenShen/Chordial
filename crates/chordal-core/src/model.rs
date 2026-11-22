//! 领域模型。所有实体都是"值对象"：一次性构造、之后只读。
//!
//! 注意 [`Track::dedup_key`] 在构造时预计算一次（O(len)），多源合并去重时
//! 直接哈希查表（O(1)/首），而不是每次比较时重新归一化 —— 这是把合并整体
//! 压在 O(n) 的关键。

use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 音乐源标识（如 "local"、"netease"）。小写 kebab-case。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct SourceId(pub String);

impl SourceId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 跨源唯一曲目标识：`(源, 源内 id)`。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TrackId {
    pub source: SourceId,
    pub id: String,
}

impl TrackId {
    pub fn new(source: SourceId, id: impl Into<String>) -> Self {
        Self {
            source,
            id: id.into(),
        }
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.source, self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistRef {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumRef {
    pub id: Option<String>,
    pub name: String,
    /// 本地源是封面文件路径；在线源是封面 URL。
    pub cover: Option<String>,
}

/// 曲目的来源侧信息，适配器负责填充。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TrackOrigin {
    Local { path: PathBuf },
    Remote { source: SourceId },
}

/// 音质档位。用于“本地同曲 vs 在线”选择，不直接参与播放。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum QualityTier {
    #[default]
    Unknown,
    Standard,
    High,
    Lossless,
    HiRes,
}

/// 结构化的音质信息。`quality` 字符串保留给 UI 展示，本结构用于比较。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AudioQuality {
    pub tier: QualityTier,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub bitrate_kbps: Option<u32>,
    #[serde(default)]
    pub sample_rate_hz: Option<u32>,
    #[serde(default)]
    pub lossless: bool,
}

impl AudioQuality {
    pub fn new(tier: QualityTier, codec: impl Into<String>) -> Self {
        Self {
            tier,
            codec: Some(codec.into()),
            bitrate_kbps: None,
            sample_rate_hz: None,
            lossless: matches!(tier, QualityTier::Lossless | QualityTier::HiRes),
        }
    }

    /// 比较用的综合分：档位是主序，码率只做同档内微调。
    pub fn score(&self) -> u32 {
        let base = match self.tier {
            QualityTier::Unknown => 100,
            QualityTier::Standard => 400,
            QualityTier::High => 600,
            QualityTier::Lossless => 800,
            QualityTier::HiRes => 1000,
        };
        base + self.bitrate_kbps.unwrap_or(0).min(199)
    }

    pub fn label(&self) -> String {
        let mut out = self
            .codec
            .as_deref()
            .map(str::to_uppercase)
            .unwrap_or_else(|| "AUDIO".to_string());
        if let Some(kbps) = self.bitrate_kbps
            && !self.lossless
        {
            out.push(' ');
            out.push_str(&kbps.to_string());
            out.push_str("k");
        }
        out
    }
}

/// 领域曲目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album: Option<AlbumRef>,
    pub duration_ms: Option<u64>,
    pub track_no: Option<u32>,
    pub year: Option<u32>,
    pub origin: TrackOrigin,
    /// 播放 UI 可展示的音质/格式备注（如 "flac 44.1k"、"320kbps"）。
    pub quality: Option<String>,
    /// 结构化音质，用于本地/在线同曲选择；缺失时退回 `quality` 字符串。
    #[serde(default)]
    pub quality_info: Option<AudioQuality>,
    /// 构造期预计算的去重键：`normalize(title)|主艺人`。
    /// 多源合并去重时 O(1) 哈希查表，避免比较时重复归一化。
    #[serde(default)]
    pub dedup_key: String,
    /// 构造期预计算的模糊搜索键：小写归一化的 `title \u{1} artists \u{1} album`。
    /// 搜索时每首只有一次 `contains`，无每查询分配。
    #[serde(skip)]
    pub search_key: String,
}

impl Track {
    pub fn new(
        id: TrackId,
        title: impl Into<String>,
        artists: Vec<ArtistRef>,
        album: Option<AlbumRef>,
        duration_ms: Option<u64>,
        origin: TrackOrigin,
    ) -> Self {
        let title = title.into();
        let primary_artist = artists.first().map(|a| a.name.as_str()).unwrap_or("");
        let dedup_key = format!("{}|{}", normalize(&title), normalize(primary_artist));
        let mut search_key = normalize(&title);
        for artist in &artists {
            search_key.push('\u{1}');
            search_key.push_str(&normalize(&artist.name));
        }
        if let Some(album) = &album {
            search_key.push('\u{1}');
            search_key.push_str(&normalize(&album.name));
        }
        Self {
            id,
            title,
            artists,
            album,
            duration_ms,
            track_no: None,
            year: None,
            origin,
            quality: None,
            quality_info: None,
            dedup_key,
            search_key,
        }
    }

    pub fn primary_artist(&self) -> &str {
        self.artists.first().map(|a| a.name.as_str()).unwrap_or("")
    }

    /// 音质比较分。没有结构化信息时按 `quality` 字符串猜一个最低分，
    /// 避免把未知格式误判为无损。
    pub fn quality_score(&self) -> u32 {
        if let Some(q) = &self.quality_info {
            return q.score();
        }
        let label = self.quality.as_deref().unwrap_or("").to_ascii_lowercase();
        if label.contains("flac") || label.contains("wav") {
            800
        } else if label.contains("320") {
            600
        } else if label.contains("256") || label.contains("192") {
            500
        } else if label.contains("128") {
            400
        } else {
            100
        }
    }
}

/// 小写 + 压缩空白。O(len)，单遍。
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_ws && !out.is_empty() {
                out.push(' ');
            }
            last_ws = true;
        } else {
            out.extend(ch.to_lowercase());
            last_ws = false;
        }
    }
    out.truncate(out.trim_end().len());
    out
}

/// 播放句柄：播放器拿到后自行打开。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamHandle {
    /// 本地文件。
    File(PathBuf),
    /// 可流式播放的远程 URL（含鉴权 query 的临时直链）。
    Url(String),
}

/// 一次流解析的结果：句柄 + 该句柄对应的实际音质（用于本地/在线比较）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedStream {
    pub handle: StreamHandle,
    pub quality: Option<AudioQuality>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    /// 磁盘上的音频文件库。
    LocalLibrary,
    /// 自部署 HTTP API 服务（如网易云 api-enhanced）。
    HttpApi,
}

/// 单行歌词。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

/// 逐词时间（YRC/QRC 等逐词歌词格式）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TimedWord {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// 带逐词时间的歌词行。行级 `lines` 仍用于纯文本/兜底展示。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TimedLine {
    pub start_ms: u64,
    pub end_ms: u64,
    pub words: Vec<TimedWord>,
}

impl TimedLine {
    pub fn text(&self) -> String {
        let capacity: usize = self.words.iter().map(|w| w.text.len()).sum();
        let mut out = String::with_capacity(capacity);
        for word in &self.words {
            out.push_str(&word.text);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    /// 原始 LRC 文本（未解析时的兜底展示）。
    pub raw: String,
    /// 译文，行数与 [`Lyrics::lines`] 对齐（可短于原文）。
    pub translation: Vec<String>,
    /// 逐词时间行（可选；为空时按行级歌词渲染）。
    #[serde(default)]
    pub timed_lines: Vec<TimedLine>,
}

impl Lyrics {
    /// 解析 LRC。单遍 O(n)（n = 文本长度）：逐行扫描，
    /// 每行内用两个游标提取 `[mm:ss.xx]` 标签，无嵌套回溯。
    pub fn parse_lrc(raw: &str) -> Self {
        let mut lines = Vec::new();
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('[') && !line.contains(']') {
                continue;
            }
            // 收集行首连续的多个时间标签（兼容 `[00:01.00][00:05.00]text`）。
            let mut times = Vec::new();
            let mut rest = line;
            loop {
                if !rest.starts_with('[') {
                    break;
                }
                let Some(close) = rest.find(']') else { break };
                let tag = &rest[1..close];
                if let Some(ms) = parse_lrc_time(tag) {
                    times.push(ms);
                    rest = &rest[close + 1..];
                } else {
                    break;
                }
            }
            if times.is_empty() {
                continue;
            }
            let text = rest.trim().to_string();
            for time_ms in times {
                lines.push(LyricLine {
                    time_ms,
                    text: text.clone(),
                });
            }
        }
        lines.sort_unstable_by_key(|l| l.time_ms);
        Self {
            lines,
            raw: raw.to_string(),
            translation: Vec::new(),
            timed_lines: Vec::new(),
        }
    }

    /// 解析 YRC（QQ 音乐逐词格式）：
    /// `[start,dur](wstart,wdur,0)词(wstart,wdur,0)词…`
    ///
    /// 词时间按绝对毫秒处理；若发现整体早于行起点，则按相对时间加上行起点
    /// （部分导出工具会这样写，属于兼容性兜底）。
    pub fn parse_yrc(raw: &str) -> Self {
        let mut timed_lines: Vec<TimedLine> = Vec::new();
        let mut plain_lines: Vec<LyricLine> = Vec::new();
        for line in raw.lines() {
            let line = line.trim();
            if !line.starts_with('[') {
                continue;
            }
            let Some((start, dur, rest)) = parse_range_tag(line) else {
                continue;
            };
            let end = start.saturating_add(dur);
            let mut words = Vec::new();
            let mut cursor = rest;
            while let Some(open) = cursor.find('(') {
                let Some(close_rel) = cursor[open..].find(')') else {
                    break;
                };
                let close = open + close_rel;
                let tag = &cursor[open + 1..close];
                let text_start = close + 1;
                let next_open = cursor[text_start..].find('(').map(|i| text_start + i);
                let text_end = next_open.unwrap_or(cursor.len());
                let text = &cursor[text_start..text_end];
                if let Some((wstart, wdur, _flag)) = parse_word_tag(tag) {
                    let absolute = if wstart < start {
                        start + wstart
                    } else {
                        wstart
                    };
                    words.push(TimedWord {
                        start_ms: absolute,
                        end_ms: absolute.saturating_add(wdur),
                        text: text.to_string(),
                    });
                }
                cursor = &cursor[text_end..];
            }
            // 只有整行所有词都早于行起点时才按相对时间解释；
            // 合法的绝对时间 YRC 偶有首词比行起点早几毫秒，不能被误判。
            let relative = !words.is_empty() && words.iter().all(|w| w.start_ms < start);
            if relative {
                for word in &mut words {
                    word.start_ms = word.start_ms.saturating_add(start);
                    word.end_ms = word.end_ms.saturating_add(start);
                }
            }
            if words.is_empty() {
                let text = rest.trim_start_matches('(').to_string();
                if !text.is_empty() {
                    words.push(TimedWord {
                        start_ms: start,
                        end_ms: end,
                        text,
                    });
                }
            }
            if words.is_empty() {
                continue;
            }
            let line_start = words[0].start_ms.min(start);
            let line_end = words
                .iter()
                .map(|w| w.end_ms)
                .max()
                .unwrap_or(end)
                .max(start);
            let text = {
                let capacity: usize = words.iter().map(|w| w.text.len()).sum();
                let mut out = String::with_capacity(capacity);
                for word in &words {
                    out.push_str(&word.text);
                }
                out
            };
            plain_lines.push(LyricLine {
                time_ms: line_start,
                text: text.clone(),
            });
            timed_lines.push(TimedLine {
                start_ms: line_start,
                end_ms: line_end.max(line_start.saturating_add(1)),
                words,
            });
        }
        timed_lines.sort_by_key(|l| l.start_ms);
        plain_lines.sort_by_key(|l| l.time_ms);
        // 行结束时间 = 下一行开始（更符合 LRC 的展示语义）。
        for i in (0..timed_lines.len()).rev() {
            if let Some(next) = timed_lines.get(i + 1).map(|l| l.start_ms)
                && timed_lines[i].end_ms < next
            {
                timed_lines[i].end_ms = next;
            }
        }
        Self {
            lines: plain_lines,
            raw: raw.to_string(),
            translation: Vec::new(),
            timed_lines,
        }
    }
}

/// 解析 `[start,dur]`，返回剩余文本。
fn parse_range_tag(src: &str) -> Option<(u64, u64, &str)> {
    let rest = src.strip_prefix('[')?;
    let close = rest.find(']')?;
    let tag = &rest[..close];
    let (start, dur) = tag.split_once(',')?;
    let start = start.trim().parse().ok()?;
    let dur = dur.trim().parse().ok()?;
    Some((start, dur, &rest[close + 1..]))
}

/// 解析 `(start,dur,flag)`。
fn parse_word_tag(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.split(',');
    let start = parts.next()?.trim().parse().ok()?;
    let dur = parts.next()?.trim().parse().ok()?;
    let flag = parts
        .next()
        .and_then(|f| f.trim().parse().ok())
        .unwrap_or(0);
    Some((start, dur, flag))
}

/// `mm:ss(.xx)` → 毫秒。O(标签长度)。
fn parse_lrc_time(tag: &str) -> Option<u64> {
    let (min, sec) = tag.split_once(':')?;
    let minutes: u64 = min.trim().parse().ok()?;
    let (sec_str, sub) = match sec.split_once('.') {
        Some((s, f)) => (s, Some(f)),
        None => (sec, None),
    };
    let seconds: u64 = sec_str.trim().parse().ok()?;
    let millis = match sub {
        Some(f) => {
            let f = f.trim();
            match f.len() {
                1 => f.parse::<u64>().ok()? * 100,
                2 => f.parse::<u64>().ok()? * 10,
                _ => format!("{f:<3}").parse::<u64>().unwrap_or(0),
            }
        }
        None => 0,
    };
    Some(minutes * 60_000 + seconds * 1_000 + millis)
}

/// 搜索请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub limit: u32,
    pub offset: u32,
    /// 源内二次过滤（本地源用它筛搜索键；在线源交给远端）。
    pub kind: SearchKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SearchKind {
    #[default]
    Track,
    Album,
    Artist,
    Playlist,
}

impl SearchQuery {
    pub fn track(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            limit: 50,
            offset: 0,
            kind: SearchKind::Track,
        }
    }

    /// 归一化一次，供所有 O(n) 扫描复用。
    pub fn normalized(&self) -> String {
        normalize(&self.text)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResults {
    pub source: SourceId,
    pub tracks: Vec<Track>,
    /// 远端报告的总命中数（本地源 = 命中数）。
    pub total: Option<u64>,
    /// 该源本项搜索的耗时（供 UI 显示与 perf 断言）。
    pub elapsed_ms: Option<u64>,
}

impl SearchResults {
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }
}

/// 源能力位。UI 据此隐藏不支持的交互。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub search: bool,
    pub track_detail: bool,
    pub stream: bool,
    pub lyrics: bool,
    /// 是否持有本地文件（可离线）。
    pub offline: bool,
    /// 是否支持整库浏览（library 视图）。
    pub library: bool,
    /// 是否支持专辑/歌单/歌手集合页。
    pub catalog: bool,
    /// 是否支持账号登录。
    pub account: bool,
    /// 登录后是否能列出用户歌单。
    pub user_playlists: bool,
}

impl Capabilities {
    pub const fn local() -> Self {
        Self {
            search: true,
            track_detail: false,
            stream: true,
            lyrics: false,
            offline: true,
            library: true,
            catalog: true,
            account: false,
            user_playlists: false,
        }
    }

    pub const fn remote_streaming() -> Self {
        Self {
            search: true,
            track_detail: true,
            stream: true,
            lyrics: true,
            offline: false,
            library: false,
            catalog: true,
            account: true,
            user_playlists: true,
        }
    }

    /// 汇总多源能力（并集），用于聚合视图。
    pub fn union_all<'a>(caps: impl IntoIterator<Item = &'a Capabilities>) -> Self {
        let mut out = Self::default();
        for c in caps {
            out.search |= c.search;
            out.track_detail |= c.track_detail;
            out.stream |= c.stream;
            out.lyrics |= c.lyrics;
            out.offline |= c.offline;
            out.library |= c.library;
            out.catalog |= c.catalog;
            out.account |= c.account;
            out.user_playlists |= c.user_playlists;
        }
        out
    }
}

/// 曲目集合（歌单/专辑视图的最小载体）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: TrackId,
    pub title: String,
    pub cover: Option<String>,
    pub track_ids: BTreeSet<TrackId>,
}

/// 目录实体类型。Album / Playlist / Artist 共用一套页面模型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CollectionKind {
    #[default]
    Album,
    Playlist,
    Artist,
}

impl CollectionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Album => "专辑",
            Self::Playlist => "歌单",
            Self::Artist => "歌手",
        }
    }
}

/// 跨源集合标识：`(源, 类型, 源内 id)`。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct CollectionId {
    pub source: SourceId,
    pub kind: CollectionKind,
    pub id: String,
}

impl CollectionId {
    pub fn new(source: SourceId, kind: CollectionKind, id: impl Into<String>) -> Self {
        Self {
            source,
            kind,
            id: id.into(),
        }
    }
}

impl fmt::Display for CollectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{:?}:{}", self.source, self.kind, self.id)
    }
}

/// 集合摘要：侧栏歌单行、专辑/歌手卡片、页面标题都用它。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CollectionSummary {
    pub id: CollectionId,
    pub title: String,
    pub cover: Option<String>,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub track_count: Option<u64>,
    pub year: Option<u32>,
    pub subscribed: bool,
    pub kind: CollectionKind,
}

/// 集合详情。`summary` 允许为空（缓存损坏/仅有曲目时 UI 仍能降级渲染）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectionPage {
    pub summary: Option<CollectionSummary>,
    pub tracks: Vec<Track>,
    /// 歌手页专用：专辑列表；其它类型为空。
    pub related_albums: Vec<CollectionSummary>,
    pub total: u64,
    pub offset: u32,
    pub has_more: bool,
}

/// 分页曲目。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackPage {
    pub tracks: Vec<Track>,
    pub total: u64,
    pub offset: u32,
    pub has_more: bool,
}

/// 账号资料（只保留 UI 需要的字段）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountProfile {
    pub source: SourceId,
    pub uid: String,
    pub nickname: String,
    pub avatar: Option<String>,
    pub vip: bool,
}

/// 源账号状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AccountState {
    /// 该源不支持账号体系。
    #[default]
    Unsupported,
    /// 支持账号但当前未登录。
    LoggedOut,
    LoggedIn(AccountProfile),
    /// 尚未探测（启动时 / 服务不可达）。
    Unknown,
}

impl AccountState {
    pub fn profile(&self) -> Option<&AccountProfile> {
        match self {
            Self::LoggedIn(profile) => Some(profile),
            _ => None,
        }
    }

    pub fn is_logged_in(&self) -> bool {
        matches!(self, Self::LoggedIn(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoginMethod {
    /// 扫码登录（推荐，不接触密码）。
    Qr,
    /// 手机号 + 短信验证码。
    CellphoneCaptcha { phone: String, captcha: String },
}

impl Default for LoginMethod {
    fn default() -> Self {
        Self::Qr
    }
}

/// 一次登录动作的挑战（二维码 key / 图片 / 过期时间）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LoginChallenge {
    pub method: LoginMethod,
    pub key: Option<String>,
    /// `data:image/png;base64,...`；UI 负责解码。
    pub qr_data_url: Option<String>,
    /// Unix 秒。
    pub expires_at: Option<u64>,
}

/// 登录轮询结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoginProgress {
    Waiting,
    Scanned,
    Success(AccountProfile),
    Expired,
    Failed(String),
}

/// 归一化正确性——去重语义依赖它。
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;

    #[test]
    fn normalize_collapses_whitespace_and_case() {
        assert_eq!(normalize("  Beyond   海阔天空 "), "beyond 海阔天空");
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("  "), "");
    }

    #[test]
    fn yrc_timed_lines() {
        let raw = "[1000,500](1000,200,0)你(1200,300,0)好\n[2000,800](2000,400,0)世(2400,400,0)界";
        let lyrics = Lyrics::parse_yrc(raw);
        assert_eq!(lyrics.timed_lines.len(), 2);
        assert_eq!(lyrics.timed_lines[0].words.len(), 2);
        assert_eq!(lyrics.timed_lines[0].words[0].text, "你");
        assert_eq!(lyrics.timed_lines[0].words[0].start_ms, 1000);
        assert_eq!(lyrics.timed_lines[0].words[1].end_ms, 1500);
        assert_eq!(lyrics.lines[1].text, "世界");
    }

    #[test]
    fn dedup_key_stable_across_whitespace() {
        let a = Track::new(
            TrackId::new(SourceId::new("local"), "1"),
            "Hey Jude",
            vec![ArtistRef {
                id: None,
                name: "The Beatles".to_string(),
            }],
            None,
            None,
            TrackOrigin::Local {
                path: PathBuf::from("/a.flac"),
            },
        );
        let b = Track::new(
            TrackId::new(SourceId::new("netease"), "99"),
            "hey  jude ",
            vec![ArtistRef {
                id: None,
                name: "the  beatles".to_string(),
            }],
            None,
            None,
            TrackOrigin::Remote {
                source: SourceId::new("netease"),
            },
        );
        assert_eq!(a.dedup_key, b.dedup_key);
    }

    #[test]
    fn lrc_parse_multi_tags_and_sort() {
        let lrc = Lyrics::parse_lrc("[00:02.50]b\n[00:01.00]a\n[00:01.00][00:03.0]c\n[meta:foo]");
        let times: Vec<u64> = lrc.lines.iter().map(|l| l.time_ms).collect();
        assert_eq!(times, vec![1000, 1000, 2500, 3000]);
    }

    #[test]
    fn lrc_time_formats() {
        assert_eq!(parse_lrc_time("01:02"), Some(62_000));
        assert_eq!(parse_lrc_time("01:02.5"), Some(62_500));
        assert_eq!(parse_lrc_time("01:02.53"), Some(62_530));
        assert_eq!(parse_lrc_time("00:00.123"), Some(123));
    }

    #[test]
    fn track_search_key_matches_artist_and_album() {
        let t = Track::new(
            TrackId::new(SourceId::new("local"), "1"),
            "海阔天空",
            vec![ArtistRef {
                id: None,
                name: "Beyond".to_string(),
            }],
            Some(AlbumRef {
                id: None,
                name: "乐与怒".to_string(),
                cover: None,
            }),
            Some(326_000),
            TrackOrigin::Local {
                path: PathBuf::from("/x.mp3"),
            },
        );
        assert!(t.search_key.contains("海阔天空"));
        assert!(t.search_key.contains("beyond"));
        assert!(t.search_key.contains("乐与怒"));
    }

    #[test]
    fn capabilities_union() {
        let u = Capabilities::union_all(&[Capabilities::local(), Capabilities::remote_streaming()]);
        assert!(u.lyrics && u.offline && u.library && u.search);
    }

    #[test]
    fn quality_scores_are_strictly_tiered() {
        let hires = AudioQuality {
            tier: QualityTier::HiRes,
            bitrate_kbps: Some(24_000),
            lossless: true,
            ..Default::default()
        };
        let flac = AudioQuality {
            tier: QualityTier::Lossless,
            bitrate_kbps: Some(1411),
            lossless: true,
            ..Default::default()
        };
        let mp3_320 = AudioQuality {
            tier: QualityTier::High,
            bitrate_kbps: Some(320),
            lossless: false,
            ..Default::default()
        };
        let mp3_128 = AudioQuality {
            tier: QualityTier::Standard,
            bitrate_kbps: Some(128),
            lossless: false,
            ..Default::default()
        };
        assert!(hires.score() > flac.score());
        assert!(flac.score() > mp3_320.score());
        assert!(mp3_320.score() > mp3_128.score());
    }

    #[test]
    fn no_stream_error_displays() {
        let e = CoreError::NoStream {
            track: "netease:123".into(),
            reason: "vip".into(),
        };
        assert!(e.to_string().contains("vip"));
    }
}
