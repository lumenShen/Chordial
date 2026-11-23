//! 网易云映射：search（由 mapping.rs 拆分，行为不变）。

use super::*;

#[derive(Deserialize)]
pub(super) struct SearchResp {
    #[serde(default)]
    result: Option<SearchResultBody>,
}

#[derive(Deserialize, Default)]
pub(super) struct SearchResultBody {
    #[serde(default)]
    songs: Option<Vec<SongBrief>>,
    #[serde(rename = "songCount", default)]
    song_count: Option<u64>,
}

#[derive(Deserialize)]
pub(super) struct SongBrief {
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
pub(super) struct NameId {
    #[serde(default)]
    pub(super) id: Option<u64>,
    #[serde(default)]
    pub(super) name: Option<String>,
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

pub(super) fn brief_to_track(source: &SourceId, song: SongBrief) -> Track {
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
