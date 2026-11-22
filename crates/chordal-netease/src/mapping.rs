//! api-enhanced 响应 DTO → 领域模型的映射。
//!
//! 全部字段 `Option` + 宽松匹配：api-enhanced 是社区逆向项目，响应结构
//! 偶有漂移，映射必须"缺什么补什么"，绝不 panic。
//! 所有 map 函数均为 O(返回条目数) 单遍。

use serde::Deserialize;

use chordal_core::error::{CoreError, CoreResult};
use chordal_core::model::{
    AccountProfile, AlbumRef, ArtistRef, AudioQuality, CollectionId, CollectionKind,
    CollectionPage, CollectionSummary, LyricLine, Lyrics, QualityTier, ResolvedStream, SearchKind,
    SearchResults, SourceId, StreamHandle, Track, TrackId, TrackOrigin, TrackPage,
};

/// 音质档位（/song/url/v1 的 level 参数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SongQualityLevel {
    Standard,
    Higher,
    #[default]
    ExHigh,
    Lossless,
    HireS,
}

impl SongQualityLevel {
    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Higher => "higher",
            Self::ExHigh => "exhigh",
            Self::Lossless => "lossless",
            Self::HireS => "hires",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "128k",
            Self::Higher => "192k",
            Self::ExHigh => "320k",
            Self::Lossless => "FLAC",
            Self::HireS => "Hi-Res",
        }
    }
}

// ---------- /search ----------

#[derive(Deserialize)]
struct SearchResp {
    #[serde(default)]
    result: Option<SearchResultBody>,
}

#[derive(Deserialize, Default)]
struct SearchResultBody {
    #[serde(default)]
    songs: Option<Vec<SongBrief>>,
    #[serde(rename = "songCount", default)]
    song_count: Option<u64>,
}

#[derive(Deserialize)]
struct SongBrief {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    artists: Option<Vec<NameId>>,
    #[serde(default)]
    album: Option<NameId>,
    /// 毫秒。
    #[serde(default)]
    duration: Option<u64>,
}

#[derive(Deserialize)]
struct NameId {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
}

/// `/search` 响应 → 搜索结果。O(songs) 单遍。
pub fn map_search(source: &SourceId, body: &serde_json::Value) -> CoreResult<SearchResults> {
    let resp: SearchResp = serde_json::from_value(body.clone())
        .map_err(|e| CoreError::source_err("netease", e.to_string()))?;
    let result = resp.result.unwrap_or_default();
    let tracks = result
        .songs
        .unwrap_or_default()
        .into_iter()
        .map(|song| brief_to_track(source, song))
        .collect();
    Ok(SearchResults {
        source: source.clone(),
        tracks,
        total: result.song_count,
        elapsed_ms: None,
    })
}

fn brief_to_track(source: &SourceId, song: SongBrief) -> Track {
    let artists: Vec<ArtistRef> = song
        .artists
        .unwrap_or_default()
        .into_iter()
        .map(|a| ArtistRef {
            id: a.id.map(|i| i.to_string()),
            name: a.name.unwrap_or_default(),
        })
        .filter(|a| !a.name.is_empty())
        .collect();
    let album = song.album.map(|a| AlbumRef {
        id: a.id.map(|i| i.to_string()),
        name: a.name.unwrap_or_default(),
        cover: None,
    });
    Track::new(
        TrackId::new(source.clone(), song.id.to_string()),
        song.name.unwrap_or_else(|| format!("track {}", song.id)),
        artists,
        album,
        song.duration,
        TrackOrigin::Remote {
            source: source.clone(),
        },
    )
}

// ---------- /song/detail ----------

#[derive(Deserialize)]
struct SongDetailResp {
    #[serde(default)]
    songs: Option<Vec<SongDetail>>,
}

#[derive(Deserialize)]
struct SongDetail {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    ar: Option<Vec<NameId>>,
    #[serde(default)]
    al: Option<AlbumDetail>,
    /// 毫秒。
    #[serde(default)]
    dt: Option<u64>,
    #[serde(default)]
    no: Option<u32>,
}

#[derive(Deserialize)]
struct AlbumDetail {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "picUrl", default)]
    pic_url: Option<String>,
}

/// `/song/detail` 响应 → Track。O(songs) 单遍；找不到 id 视为错误。
pub fn map_song_detail(
    source: &SourceId,
    body: &serde_json::Value,
    want_id: &str,
) -> CoreResult<Track> {
    let resp: SongDetailResp = serde_json::from_value(body.clone())
        .map_err(|e| CoreError::source_err("netease", e.to_string()))?;
    let songs = resp.songs.unwrap_or_default();
    songs
        .into_iter()
        .find(|s| s.id.to_string() == want_id)
        .map(|s| detail_to_track(source, s))
        .ok_or_else(|| {
            CoreError::source_err("netease", format!("song/detail missing id {want_id}"))
        })
}

fn detail_to_track(source: &SourceId, song: SongDetail) -> Track {
    let artists: Vec<ArtistRef> = song
        .ar
        .unwrap_or_default()
        .into_iter()
        .map(|a| ArtistRef {
            id: a.id.map(|i| i.to_string()),
            name: a.name.unwrap_or_default(),
        })
        .filter(|a| !a.name.is_empty())
        .collect();
    let album = song.al.map(|a| AlbumRef {
        id: a.id.map(|i| i.to_string()),
        name: a.name.unwrap_or_default(),
        cover: a.pic_url,
    });
    let mut track = Track::new(
        TrackId::new(source.clone(), song.id.to_string()),
        song.name.unwrap_or_else(|| format!("track {}", song.id)),
        artists,
        album,
        song.dt,
        TrackOrigin::Remote {
            source: source.clone(),
        },
    );
    track.track_no = song.no;
    track
}

// ---------- /song/url/v1 ----------

#[derive(Deserialize)]
struct SongUrlResp {
    #[serde(default)]
    data: Option<Vec<SongUrlData>>,
}

#[derive(Deserialize)]
struct SongUrlData {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(rename = "type", default)]
    format: Option<String>,
    #[serde(default)]
    br: Option<u64>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default, rename = "freeTrialInfo")]
    free_trial_info: Option<serde_json::Value>,
}

/// `/song/url/v1` 响应 → 播放句柄 + 实际音质。`url` 为空 = VIP/无版权。
pub fn map_song_url(body: &serde_json::Value, want_id: &str) -> CoreResult<ResolvedStream> {
    let resp: SongUrlResp = serde_json::from_value(body.clone())
        .map_err(|e| CoreError::source_err("netease", e.to_string()))?;
    let data = resp.data.unwrap_or_default();
    let entry = data
        .iter()
        .find(|d| d.id.map(|i| i.to_string()) == Some(want_id.to_string()))
        .or_else(|| data.first());

    let Some(entry) = entry else {
        return Err(CoreError::NoStream {
            track: format!("netease:{want_id}"),
            reason: "empty data".into(),
        });
    };
    let Some(url) = entry.url.as_deref().filter(|u| !u.is_empty()) else {
        return Err(CoreError::NoStream {
            track: format!("netease:{want_id}"),
            reason: "no url (vip / region-locked / removed)".into(),
        });
    };
    if let Some(info) = &entry.free_trial_info {
        // 未登录 / 非会员时网易云返回 30-45s 试听片段；仍可播放，但 UI 应提示。
        tracing::warn!(
            target: "chordial::netease",
            track = want_id,
            trial = %info,
            "netease returned a trial fragment (login/VIP required for full track)"
        );
    }
    let codec = entry
        .format
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("mp3")
        .to_ascii_lowercase();
    let bitrate_kbps = entry.br.and_then(|br| {
        let kbps = (br / 1000) as u32;
        (kbps > 0).then_some(kbps)
    });
    let lossless = codec.contains("flac") || codec.contains("alac");
    let hires = entry
        .level
        .as_deref()
        .is_some_and(|l| l.eq_ignore_ascii_case("hires"));
    let tier = if hires {
        QualityTier::HiRes
    } else if lossless {
        QualityTier::Lossless
    } else {
        match bitrate_kbps {
            Some(kbps) if kbps >= 256 => QualityTier::High,
            Some(_) => QualityTier::Standard,
            None => QualityTier::Unknown,
        }
    };
    let quality = AudioQuality {
        tier,
        codec: Some(codec),
        bitrate_kbps,
        sample_rate_hz: None,
        lossless,
    };
    Ok(ResolvedStream {
        handle: StreamHandle::Url(url.to_string()),
        quality: Some(quality),
    })
}

// ---------- /lyric ----------

#[derive(Deserialize)]
struct NewLyricResp {
    #[serde(default)]
    lrc: Option<LyricBody>,
    #[serde(default)]
    yrc: Option<LyricBody>,
    #[serde(default)]
    tlyric: Option<LyricBody>,
}

#[derive(Deserialize)]
struct LyricResp {
    #[serde(default)]
    lrc: Option<LyricBody>,
    #[serde(default)]
    tlyric: Option<LyricBody>,
    /// QQ/网易逐词歌词（YRC 文本）。
    #[serde(default)]
    yrc: Option<LyricBody>,
}

#[derive(Deserialize)]
struct LyricBody {
    #[serde(default)]
    lyric: Option<String>,
}

/// `/lyric/new`：优先使用逐词 YRC（新接口返回“JSON 头 + YRC 文本”混合格式）。
pub fn map_lyrics_new(body: &serde_json::Value) -> Option<Lyrics> {
    let resp: NewLyricResp = serde_json::from_value(body.clone()).ok()?;
    let yrc_raw = resp
        .yrc
        .and_then(|l| l.lyric)
        .filter(|s| !s.trim().is_empty());
    let lrc_raw = resp
        .lrc
        .and_then(|l| l.lyric)
        .filter(|s| !s.trim().is_empty());
    let trans_raw = resp
        .tlyric
        .and_then(|l| l.lyric)
        .filter(|s| !s.trim().is_empty());

    let mut meta_lines: Vec<LyricLine> = Vec::new();
    let mut yrc_text = String::new();
    if let Some(raw) = &yrc_raw {
        split_rich_yrc(raw, &mut meta_lines, &mut yrc_text);
    }

    let mut lyrics = if !yrc_text.trim().is_empty() {
        Lyrics::parse_yrc(&yrc_text)
    } else if let Some(raw) = &lrc_raw {
        Lyrics {
            lines: parse_rich_lines(raw),
            raw: lrc_raw.clone().unwrap_or_default(),
            translation: Vec::new(),
            timed_lines: Vec::new(),
        }
    } else {
        return None;
    };

    if !meta_lines.is_empty() {
        lyrics.lines.extend(meta_lines);
        lyrics.lines.sort_by_key(|l| l.time_ms);
    }

    if let Some(raw) = trans_raw {
        let trans = Lyrics::parse_lrc(&raw);
        if !trans.lines.is_empty() {
            // tlyric 与 lrc 时间戳可能有几十毫秒偏差，按最近行匹配。
            for line in &lyrics.lines {
                let idx = trans.lines.partition_point(|t| t.time_ms < line.time_ms);
                let best = [idx.saturating_sub(1), idx]
                    .into_iter()
                    .filter_map(|i| trans.lines.get(i).map(|t| (i, t)))
                    .min_by_key(|(_, t)| t.time_ms.abs_diff(line.time_ms));
                if let Some((_, t)) = best
                    && t.time_ms.abs_diff(line.time_ms) <= 120
                {
                    lyrics.translation.push(t.text.clone());
                } else {
                    lyrics.translation.push(String::new());
                }
            }
        }
    }

    if lyrics.lines.is_empty() {
        None
    } else {
        Some(lyrics)
    }
}

fn parse_rich_lines(raw: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    split_rich_yrc(raw, &mut lines, &mut String::new());
    lines.sort_by_key(|l| l.time_ms);
    lines
}

/// 拆分 `/lyric/new` 的 YRC：JSON 元信息行 → `meta`，YRC 文本行 → `text`。
fn split_rich_yrc(raw: &str, meta: &mut Vec<LyricLine>, text: &mut String) {
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('{') {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                let t = value.get("t").and_then(|v| v.as_u64()).unwrap_or(0);
                let text = value
                    .get("c")
                    .and_then(|c| c.as_array())
                    .map(|segments| {
                        segments
                            .iter()
                            .filter_map(|s| s.get("tx").and_then(|t| t.as_str()))
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                if !text.trim().is_empty() {
                    meta.push(LyricLine {
                        time_ms: t,
                        text: text.trim_end().to_string(),
                    });
                }
            }
        } else {
            text.push_str(line);
            text.push('\n');
        }
    }
}

/// `/lyric` 响应 → Lyrics。原文 + 译文按时间戳对齐：
/// 构建 `time_ms → 译文` 哈希表 O(t)，逐行查表 O(n)，无嵌套循环。
pub fn map_lyrics(body: &serde_json::Value) -> Option<Lyrics> {
    let resp: LyricResp = serde_json::from_value(body.clone()).ok()?;
    // 空歌词（未收录 / 纯音乐）视为无歌词，避免 UI 显示一块空面板。
    let lrc_raw = resp
        .lrc
        .and_then(|l| l.lyric)
        .filter(|s| !s.trim().is_empty());
    let yrc_raw = resp
        .yrc
        .and_then(|l| l.lyric)
        .filter(|s| !s.trim().is_empty());

    let mut lyrics = if let Some(raw) = lrc_raw {
        Lyrics::parse_lrc(&raw)
    } else {
        // 没有 LRC 时，YRC 同时提供行级与逐词歌词。
        Lyrics::parse_yrc(yrc_raw.as_deref()?)
    };
    if lyrics.timed_lines.is_empty()
        && let Some(raw) = yrc_raw
    {
        // 有 LRC 行级歌词时，把 YRC 的逐词时间挂上去。
        let yrc = Lyrics::parse_yrc(&raw);
        lyrics.timed_lines = yrc.timed_lines;
    }
    if lyrics.lines.is_empty() {
        return None;
    }
    if let Some(trans_raw) = resp.tlyric.and_then(|t| t.lyric).filter(|s| !s.is_empty()) {
        let trans = Lyrics::parse_lrc(&trans_raw);
        let by_time: std::collections::HashMap<u64, &str> = trans
            .lines
            .iter()
            .filter(|l| !l.text.is_empty())
            .map(|l| (l.time_ms, l.text.as_str()))
            .collect();
        lyrics.translation = lyrics
            .lines
            .iter()
            .map(|l| {
                by_time
                    .get(&l.time_ms)
                    .map(|s| (*s).to_string())
                    .unwrap_or_default()
            })
            .collect();
    }
    Some(lyrics)
}

// ---------- 账号 / 集合 ----------

#[derive(Deserialize)]
struct UserAccountResp {
    #[serde(default)]
    account: Option<AccountBody>,
    #[serde(default)]
    profile: Option<ProfileBody>,
}

#[derive(Deserialize)]
struct AccountBody {
    #[serde(default)]
    id: Option<u64>,
    #[serde(rename = "vipType", default)]
    vip_type: Option<u64>,
}

#[derive(Deserialize, Default)]
struct ProfileBody {
    #[serde(rename = "userId", default)]
    user_id: Option<u64>,
    #[serde(default)]
    nickname: Option<String>,
    #[serde(rename = "avatarUrl", default)]
    avatar_url: Option<String>,
    #[serde(rename = "vipType", default)]
    vip_type: Option<u64>,
}

/// `/user/account` → 账号资料。未登录返回 None。
pub fn map_account(source: &SourceId, body: &serde_json::Value) -> Option<AccountProfile> {
    let resp: UserAccountResp = serde_json::from_value(body.clone()).ok()?;
    let profile = resp.profile.unwrap_or_default();
    let uid = profile
        .user_id
        .or_else(|| resp.account.as_ref().and_then(|a| a.id))?;
    let vip_type = profile
        .vip_type
        .or_else(|| resp.account.as_ref().and_then(|a| a.vip_type))
        .unwrap_or(0);
    Some(AccountProfile {
        source: source.clone(),
        uid: uid.to_string(),
        nickname: profile
            .nickname
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("用户 {uid}")),
        avatar: profile.avatar_url.filter(|u| !u.trim().is_empty()),
        vip: vip_type > 0,
    })
}

#[derive(Deserialize, Default)]
struct PlaylistListResp {
    #[serde(default)]
    playlist: Option<Vec<PlaylistBrief>>,
}

#[derive(Deserialize)]
struct PlaylistBrief {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "coverImgUrl", default)]
    cover: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "trackCount", default)]
    track_count: Option<u64>,
    #[serde(default)]
    subscribed: Option<bool>,
    #[serde(default)]
    creator: Option<Creator>,
}

#[derive(Deserialize)]
struct Creator {
    #[serde(default)]
    nickname: Option<String>,
}

fn playlist_summary(
    source: &SourceId,
    p: PlaylistBrief,
    kind: CollectionKind,
) -> Option<CollectionSummary> {
    let id = p.id?;
    Some(CollectionSummary {
        id: CollectionId::new(source.clone(), kind, id.to_string()),
        title: p.name.unwrap_or_else(|| format!("歌单 {id}")),
        cover: p.cover,
        creator: p.creator.and_then(|c| c.nickname),
        description: p.description,
        track_count: p.track_count,
        year: None,
        subscribed: p.subscribed.unwrap_or(false),
        kind,
    })
}

/// `/user/playlist` → 用户歌单摘要。
pub fn map_user_playlists(source: &SourceId, body: &serde_json::Value) -> Vec<CollectionSummary> {
    let resp: PlaylistListResp = match serde_json::from_value(body.clone()) {
        Ok(resp) => resp,
        Err(_) => return Vec::new(),
    };
    resp.playlist
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| playlist_summary(source, p, CollectionKind::Playlist))
        .collect()
}

#[derive(Deserialize)]
struct PlaylistDetailResp {
    #[serde(default)]
    playlist: Option<PlaylistDetailBody>,
}

#[derive(Deserialize)]
struct PlaylistDetailBody {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "coverImgUrl", default)]
    cover: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "trackCount", default)]
    track_count: Option<u64>,
    #[serde(default)]
    subscribed: Option<bool>,
    #[serde(default)]
    creator: Option<Creator>,
    #[serde(default)]
    tracks: Option<Vec<SongDetail>>,
}

#[derive(Deserialize)]
struct SongsResp {
    #[serde(default)]
    songs: Option<Vec<SongDetail>>,
}

fn parse_songs(body: &serde_json::Value) -> Vec<SongDetail> {
    serde_json::from_value::<SongsResp>(body.clone())
        .ok()
        .and_then(|r| r.songs)
        .unwrap_or_default()
}

/// `/playlist/detail` + `/playlist/track/all` → 歌单页。
pub fn map_playlist_page(
    source: &SourceId,
    detail_body: &serde_json::Value,
    tracks_body: &serde_json::Value,
) -> CoreResult<CollectionPage> {
    let resp: PlaylistDetailResp = serde_json::from_value(detail_body.clone())
        .map_err(|e| CoreError::source_err("netease", e.to_string()))?;
    let detail = resp
        .playlist
        .ok_or_else(|| CoreError::source_err("netease", "playlist/detail missing playlist"))?;
    let id = detail
        .id
        .ok_or_else(|| CoreError::source_err("netease", "playlist detail missing id"))?;
    let summary = CollectionSummary {
        id: CollectionId::new(source.clone(), CollectionKind::Playlist, id.to_string()),
        title: detail.name.unwrap_or_else(|| format!("歌单 {id}")),
        cover: detail.cover,
        creator: detail.creator.and_then(|c| c.nickname),
        description: detail.description,
        track_count: detail.track_count,
        year: None,
        subscribed: detail.subscribed.unwrap_or(false),
        kind: CollectionKind::Playlist,
    };
    let songs = {
        let from_tracks = parse_songs(tracks_body);
        if from_tracks.is_empty() {
            detail.tracks.unwrap_or_default()
        } else {
            from_tracks
        }
    };
    let tracks: Vec<Track> = songs
        .into_iter()
        .map(|song| detail_to_track(source, song))
        .collect();
    let total = summary.track_count.unwrap_or(tracks.len() as u64);
    let has_more = (tracks.len() as u64) < total;
    Ok(CollectionPage {
        summary: Some(summary),
        tracks,
        related_albums: Vec::new(),
        total,
        offset: 0,
        has_more,
    })
}

#[derive(Deserialize)]
struct AlbumResp {
    #[serde(default)]
    album: Option<AlbumBody>,
    #[serde(default)]
    songs: Option<Vec<SongDetail>>,
}

#[derive(Deserialize)]
struct AlbumBody {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "picUrl", default)]
    cover: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "publishTime", default)]
    publish_time: Option<u64>,
    #[serde(default)]
    artist: Option<NameId>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    company: Option<String>,
}

fn year_from_ms(ms: u64) -> Option<u32> {
    if ms == 0 {
        return None;
    }
    let secs = ms / 1000;
    Some((1970 + secs / 31_556_952) as u32)
}

/// `/album?id=` → 专辑页（信息 + 曲目一次返回）。
pub fn map_album_page(source: &SourceId, body: &serde_json::Value) -> CoreResult<CollectionPage> {
    let resp: AlbumResp = serde_json::from_value(body.clone())
        .map_err(|e| CoreError::source_err("netease", e.to_string()))?;
    let album = resp
        .album
        .ok_or_else(|| CoreError::source_err("netease", "album missing album"))?;
    let id = album
        .id
        .ok_or_else(|| CoreError::source_err("netease", "album missing id"))?;
    let creator = album.artist.as_ref().and_then(|a| a.name.clone());
    let summary = CollectionSummary {
        id: CollectionId::new(source.clone(), CollectionKind::Album, id.to_string()),
        title: album.name.unwrap_or_else(|| format!("专辑 {id}")),
        cover: album.cover,
        creator,
        description: album.description.or(album.company),
        track_count: album.size,
        year: album.publish_time.and_then(year_from_ms),
        subscribed: false,
        kind: CollectionKind::Album,
    };
    let tracks: Vec<Track> = resp
        .songs
        .unwrap_or_default()
        .into_iter()
        .map(|song| detail_to_track(source, song))
        .collect();
    let total = summary.track_count.unwrap_or(tracks.len() as u64);
    Ok(CollectionPage {
        summary: Some(summary),
        total,
        has_more: (tracks.len() as u64) < total,
        tracks,
        related_albums: Vec::new(),
        offset: 0,
    })
}

#[derive(Deserialize)]
struct ArtistDetailResp {
    #[serde(default)]
    data: Option<ArtistDetailData>,
}

#[derive(Deserialize)]
struct ArtistDetailData {
    #[serde(default)]
    artist: Option<ArtistBody>,
}

#[derive(Deserialize)]
struct ArtistBody {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "picUrl", default)]
    cover: Option<String>,
    #[serde(rename = "briefDesc", default)]
    description: Option<String>,
    #[serde(rename = "albumSize", default)]
    album_size: Option<u64>,
    #[serde(rename = "musicSize", default)]
    music_size: Option<u64>,
}

#[derive(Deserialize)]
struct ArtistAlbumsResp {
    #[serde(rename = "hotAlbums", default)]
    hot_albums: Option<Vec<AlbumBrief>>,
    #[serde(default)]
    more: Option<bool>,
}

#[derive(Deserialize)]
struct AlbumBrief {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "picUrl", default)]
    cover: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(rename = "publishTime", default)]
    publish_time: Option<u64>,
    #[serde(default)]
    artist: Option<NameId>,
}

/// `/artist/detail` + `/artist/top/song` + `/artist/album` → 歌手页。
pub fn map_artist_page(
    source: &SourceId,
    detail_body: &serde_json::Value,
    songs_body: &serde_json::Value,
    albums_body: &serde_json::Value,
) -> CoreResult<CollectionPage> {
    let resp: ArtistDetailResp = serde_json::from_value(detail_body.clone())
        .map_err(|e| CoreError::source_err("netease", e.to_string()))?;
    let artist = resp
        .data
        .and_then(|d| d.artist)
        .ok_or_else(|| CoreError::source_err("netease", "artist/detail missing artist"))?;
    let id = artist
        .id
        .ok_or_else(|| CoreError::source_err("netease", "artist/detail missing id"))?;
    let summary = CollectionSummary {
        id: CollectionId::new(source.clone(), CollectionKind::Artist, id.to_string()),
        title: artist.name.unwrap_or_else(|| format!("歌手 {id}")),
        cover: artist.cover,
        creator: None,
        description: artist.description,
        track_count: artist.music_size.or(artist.album_size),
        year: None,
        subscribed: false,
        kind: CollectionKind::Artist,
    };
    let tracks: Vec<Track> = parse_songs(songs_body)
        .into_iter()
        .map(|song| detail_to_track(source, song))
        .collect();
    let (albums, more_albums) =
        match serde_json::from_value::<ArtistAlbumsResp>(albums_body.clone()) {
            Ok(resp) => (
                resp.hot_albums.unwrap_or_default(),
                resp.more.unwrap_or(false),
            ),
            Err(_) => (Vec::new(), false),
        };
    let related_albums: Vec<CollectionSummary> = albums
        .into_iter()
        .filter_map(|album| {
            let album_id = album.id?;
            Some(CollectionSummary {
                id: CollectionId::new(source.clone(), CollectionKind::Album, album_id.to_string()),
                title: album.name.unwrap_or_else(|| format!("专辑 {album_id}")),
                cover: album.cover,
                creator: album.artist.and_then(|a| a.name),
                description: None,
                track_count: album.size,
                year: album.publish_time.and_then(year_from_ms),
                subscribed: false,
                kind: CollectionKind::Album,
            })
        })
        .collect();
    Ok(CollectionPage {
        total: tracks.len() as u64,
        summary: Some(summary),
        tracks,
        related_albums,
        offset: 0,
        has_more: more_albums,
    })
}

#[derive(Deserialize, Default)]
struct CloudSearchResp {
    #[serde(default)]
    result: Option<CloudSearchResult>,
}

#[derive(Deserialize, Default)]
struct CloudSearchResult {
    #[serde(default)]
    albums: Option<Vec<AlbumBrief>>,
    #[serde(default)]
    artists: Option<Vec<ArtistBody>>,
    #[serde(default)]
    playlists: Option<Vec<PlaylistBrief>>,
}

/// `/cloudsearch` 的专辑/歌手/歌单结果。
pub fn map_search_collections(
    source: &SourceId,
    body: &serde_json::Value,
    kind: SearchKind,
) -> Vec<CollectionSummary> {
    let resp: CloudSearchResp = match serde_json::from_value(body.clone()) {
        Ok(resp) => resp,
        Err(_) => return Vec::new(),
    };
    let result = resp.result.unwrap_or_default();
    match kind {
        SearchKind::Track => Vec::new(),
        SearchKind::Album => result
            .albums
            .unwrap_or_default()
            .into_iter()
            .filter_map(|album| {
                let id = album.id?;
                Some(CollectionSummary {
                    id: CollectionId::new(source.clone(), CollectionKind::Album, id.to_string()),
                    title: album.name.unwrap_or_else(|| format!("专辑 {id}")),
                    cover: album.cover,
                    creator: album.artist.and_then(|a| a.name),
                    description: None,
                    track_count: album.size,
                    year: album.publish_time.and_then(year_from_ms),
                    subscribed: false,
                    kind: CollectionKind::Album,
                })
            })
            .collect(),
        SearchKind::Artist => result
            .artists
            .unwrap_or_default()
            .into_iter()
            .filter_map(|artist| {
                let id = artist.id?;
                Some(CollectionSummary {
                    id: CollectionId::new(source.clone(), CollectionKind::Artist, id.to_string()),
                    title: artist.name.unwrap_or_else(|| format!("歌手 {id}")),
                    cover: artist.cover,
                    creator: None,
                    description: artist.description,
                    track_count: artist.music_size.or(artist.album_size),
                    year: None,
                    subscribed: false,
                    kind: CollectionKind::Artist,
                })
            })
            .collect(),
        SearchKind::Playlist => result
            .playlists
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| playlist_summary(source, p, CollectionKind::Playlist))
            .collect(),
    }
}

/// `/playlist/track/all` 分页曲目。
pub fn map_songs_page(source: &SourceId, body: &serde_json::Value, offset: u32) -> TrackPage {
    let tracks: Vec<Track> = parse_songs(body)
        .into_iter()
        .map(|song| detail_to_track(source, song))
        .collect();
    TrackPage {
        total: tracks.len() as u64,
        tracks,
        offset,
        has_more: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn map_search_defensive_against_missing_fields() {
        let body = json!({
            "code": 200,
            "result": {
                "songCount": 2,
                "songs": [
                    {
                        "id": 347230,
                        "name": "海阔天空",
                        "artists": [{"id": 1, "name": "Beyond"}],
                        "album": {"id": 19064, "name": "乐与怒"},
                        "duration": 326000
                    },
                    {"id": 999}  // 最小响应：只有 id
                ]
            }
        });
        let res = map_search(&SourceId::new("netease"), &body).unwrap();
        assert_eq!(res.tracks.len(), 2);
        assert_eq!(res.total, Some(2));
        assert_eq!(res.tracks[0].title, "海阔天空");
        assert_eq!(res.tracks[0].primary_artist(), "Beyond");
        assert_eq!(res.tracks[0].duration_ms, Some(326000));
        assert_eq!(res.tracks[1].title, "track 999");
    }

    #[test]
    fn map_song_url_vip_rejected() {
        let body = json!({"code": 200, "data": [{"id": 1, "url": null}]});
        let err = map_song_url(&body, "1").unwrap_err();
        assert!(matches!(err, CoreError::NoStream { .. }));

        let body = json!({"code": 200, "data": [{"id": 1, "url": "http://m7.example/s.mp3", "type": "mp3", "br": 320000}]});
        let resolved = map_song_url(&body, "1").unwrap();
        match resolved.handle {
            StreamHandle::Url(u) => assert_eq!(u, "http://m7.example/s.mp3"),
            _ => panic!("expected url"),
        }
        let quality = resolved.quality.expect("quality");
        assert_eq!(quality.bitrate_kbps, Some(320));
        assert_eq!(quality.tier, QualityTier::High);
    }

    #[test]
    fn map_lyrics_aligns_translation_by_time() {
        let body = json!({
            "code": 200,
            "lrc": {"lyric": "[00:01.00]hello\n[00:02.00]world"},
            "tlyric": {"lyric": "[00:02.00]世界\n[00:01.00]你好"}
        });
        let l = map_lyrics(&body).unwrap();
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.translation[0], "你好");
        assert_eq!(l.translation[1], "世界");
    }

    #[test]
    fn map_lyrics_none_when_missing() {
        assert!(map_lyrics(&json!({"code": 200})).is_none());
        assert!(map_lyrics(&json!({"code": 200, "lrc": {"lyric": ""}})).is_none());
    }

    #[test]
    fn map_lyrics_new_rich_yrc() {
        let yrc = "{\"t\":0,\"c\":[{\"tx\":\"作词\"}]}\n[1000,500](1000,200,0)你(1200,300,0)好\n";
        let body = json!({
            "lrc": {"lyric": ""},
            "yrc": {"lyric": yrc},
            "tlyric": {"lyric": "[00:01.00]hello"}
        });
        let lyrics = map_lyrics_new(&body).expect("lyrics");
        assert!(lyrics.lines.iter().any(|l| l.text == "作词"));
        assert_eq!(lyrics.timed_lines.len(), 1);
        assert_eq!(lyrics.timed_lines[0].words.len(), 2);
        assert!(lyrics.translation.iter().any(|t| t == "hello"));
    }

    #[test]
    fn map_lyrics_attaches_yrc_words() {
        let body = json!({
            "code": 200,
            "lrc": {"lyric": "[00:01.00]你好"},
            "yrc": {"lyric": "[1000,500](1000,200,0)你(1200,300,0)好"}
        });
        let lyrics = map_lyrics(&body).unwrap();
        assert_eq!(lyrics.lines.len(), 1);
        assert_eq!(lyrics.timed_lines.len(), 1);
        let words = &lyrics.timed_lines[0].words;
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "你");
        assert_eq!(words[0].start_ms, 1000);
        assert_eq!(words[1].text, "好");
        assert_eq!(words[1].end_ms, 1500);
    }

    #[test]
    fn map_account_and_user_playlists() {
        let source = SourceId::new("netease");
        let account = json!({
            "code": 200,
            "account": {"id": 42, "vipType": 11},
            "profile": {
                "userId": 42,
                "nickname": "听歌的人",
                "avatarUrl": "https://x/a.jpg",
                "vipType": 11
            }
        });
        let profile = map_account(&source, &account).unwrap();
        assert_eq!(profile.uid, "42");
        assert_eq!(profile.nickname, "听歌的人");
        assert!(profile.vip);

        let playlists = json!({
            "code": 200,
            "playlist": [
                {
                    "id": 7,
                    "name": "我喜欢的音乐",
                    "coverImgUrl": "https://x/c.jpg",
                    "trackCount": 100,
                    "subscribed": false,
                    "creator": {"nickname": "听歌的人"}
                },
                {"id": 8, "name": "收藏歌单", "trackCount": 3, "subscribed": true}
            ]
        });
        let list = map_user_playlists(&source, &playlists);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id.kind, CollectionKind::Playlist);
        assert_eq!(list[0].id.id, "7");
        assert_eq!(list[0].track_count, Some(100));
        assert!(list[1].subscribed);
    }

    #[test]
    fn map_playlist_album_and_artist_pages() {
        let source = SourceId::new("netease");
        let detail = json!({
            "code": 200,
            "playlist": {
                "id": 7,
                "name": "测试歌单",
                "coverImgUrl": "https://x/c.jpg",
                "trackCount": 1,
                "subscribed": false,
                "creator": {"nickname": "me"}
            }
        });
        let songs = json!({
            "code": 200,
            "songs": [{
                "id": 123,
                "name": "海阔天空",
                "ar": [{"id": 1, "name": "Beyond"}],
                "al": {"id": 2, "name": "乐与怒", "picUrl": "https://x/a.jpg"},
                "dt": 326000,
                "no": 1
            }]
        });
        let page = map_playlist_page(&source, &detail, &songs).unwrap();
        assert_eq!(page.summary.as_ref().unwrap().title, "测试歌单");
        assert_eq!(page.tracks.len(), 1);
        assert_eq!(page.tracks[0].primary_artist(), "Beyond");
        assert_eq!(
            page.tracks[0].album.as_ref().unwrap().id.as_deref(),
            Some("2")
        );

        let album = json!({
            "code": 200,
            "album": {
                "id": 2,
                "name": "乐与怒",
                "picUrl": "https://x/a.jpg",
                "artist": {"id": 1, "name": "Beyond"},
                "publishTime": 738000000000u64,
                "size": 1
            },
            "songs": [{
                "id": 123,
                "name": "海阔天空",
                "ar": [{"id": 1, "name": "Beyond"}],
                "al": {"id": 2, "name": "乐与怒"}
            }]
        });
        let album_page = map_album_page(&source, &album).unwrap();
        assert_eq!(
            album_page.summary.as_ref().unwrap().kind,
            CollectionKind::Album
        );
        assert_eq!(album_page.summary.as_ref().unwrap().year, Some(1993));

        let artist_detail = json!({
            "code": 200,
            "data": {"artist": {
                "id": 1,
                "name": "Beyond",
                "picUrl": "https://x/b.jpg",
                "briefDesc": "香港乐队",
                "albumSize": 10,
                "musicSize": 100
            }}
        });
        let artist_albums = json!({
            "code": 200,
            "hotAlbums": [{
                "id": 2,
                "name": "乐与怒",
                "picUrl": "https://x/a.jpg",
                "size": 10,
                "artist": {"id": 1, "name": "Beyond"}
            }],
            "more": true
        });
        let artist_page = map_artist_page(&source, &artist_detail, &songs, &artist_albums).unwrap();
        assert_eq!(
            artist_page.summary.as_ref().unwrap().kind,
            CollectionKind::Artist
        );
        assert_eq!(artist_page.tracks.len(), 1);
        assert_eq!(artist_page.related_albums.len(), 1);
        assert!(artist_page.has_more);
    }

    #[test]
    fn map_search_collections_by_kind() {
        let source = SourceId::new("netease");
        let body = json!({
            "code": 200,
            "result": {
                "albums": [{"id": 2, "name": "乐与怒", "picUrl": "https://x/a.jpg", "size": 10, "artist": {"name": "Beyond"}}],
                "artists": [{"id": 1, "name": "Beyond", "musicSize": 100}],
                "playlists": [{"id": 7, "name": "港乐", "trackCount": 30}]
            }
        });
        let albums = map_search_collections(&source, &body, SearchKind::Album);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].kind, CollectionKind::Album);
        let artists = map_search_collections(&source, &body, SearchKind::Artist);
        assert_eq!(artists[0].kind, CollectionKind::Artist);
        let playlists = map_search_collections(&source, &body, SearchKind::Playlist);
        assert_eq!(playlists[0].kind, CollectionKind::Playlist);
    }
}
