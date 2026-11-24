//! 网易云映射：detail（由 mapping.rs 拆分，行为不变）。

use super::*;

#[derive(Deserialize)]
pub(super) struct SongDetailResp {
    #[serde(default)]
    songs: Option<Vec<SongDetail>>,
}

#[derive(Deserialize)]
pub(super) struct SongDetail {
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
pub(super) struct AlbumDetail {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "picUrl", default)]
    pic_url: Option<String>,
    /// 发行时间（毫秒），用于写回下载文件的年份标签。
    #[serde(rename = "publishTime", default)]
    publish_time: Option<u64>,
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

pub(super) fn detail_to_track(source: &SourceId, song: SongDetail) -> Track {
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
    let mut year = None;
    let album = song.al.map(|a| {
        year = a.publish_time.and_then(year_from_ms);
        AlbumRef {
            id: a.id.map(|i| i.to_string()),
            name: a.name.unwrap_or_default(),
            cover: a.pic_url,
        }
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
    // track_no 参与跨源身份键，构造后补写必须重算缓存。
    track.rebuild_identity_key();
    track.year = year;
    track
}

// ---------- /song/url/v1 ----------
