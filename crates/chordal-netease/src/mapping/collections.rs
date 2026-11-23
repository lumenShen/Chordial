//! 网易云映射：collections（由 mapping.rs 拆分，行为不变）。

use super::*;

#[derive(Deserialize)]
pub(super) struct PlaylistDetailResp {
    #[serde(default)]
    playlist: Option<PlaylistDetailBody>,
}

#[derive(Deserialize)]
pub(super) struct PlaylistDetailBody {
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
pub(super) struct SongsResp {
    #[serde(default)]
    songs: Option<Vec<SongDetail>>,
}

pub(super) fn parse_songs(body: &serde_json::Value) -> Vec<SongDetail> {
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
pub(super) struct AlbumResp {
    #[serde(default)]
    album: Option<AlbumBody>,
    #[serde(default)]
    songs: Option<Vec<SongDetail>>,
}

#[derive(Deserialize)]
pub(super) struct AlbumBody {
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

pub(super) fn year_from_ms(ms: u64) -> Option<u32> {
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
pub(super) struct ArtistDetailResp {
    #[serde(default)]
    data: Option<ArtistDetailData>,
}

#[derive(Deserialize)]
pub(super) struct ArtistDetailData {
    #[serde(default)]
    artist: Option<ArtistBody>,
}

#[derive(Deserialize)]
pub(super) struct ArtistBody {
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
pub(super) struct ArtistAlbumsResp {
    #[serde(rename = "hotAlbums", default)]
    hot_albums: Option<Vec<AlbumBrief>>,
    #[serde(default)]
    more: Option<bool>,
}

#[derive(Deserialize)]
pub(super) struct AlbumBrief {
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
pub(super) struct CloudSearchResp {
    #[serde(default)]
    result: Option<CloudSearchResult>,
}

#[derive(Deserialize, Default)]
pub(super) struct CloudSearchResult {
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
