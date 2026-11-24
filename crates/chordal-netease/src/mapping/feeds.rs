//! 网易云映射：账号推荐流（每日推荐 / 私人 FM）。
//!
//! 推荐接口的歌曲对象有两种历史形态：`/recommend/songs` 用 `ar`/`al`/`dt`，
//! `/personal_fm` 用 `artists`/`album`/`duration`。这里用一份宽容 DTO 同时接住，
//! 缺字段按空处理，绝不因为某条脏数据丢掉整页。

use super::*;

#[derive(Deserialize, Default)]
pub(super) struct FeedAlbum {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "picUrl", default)]
    pic_url: Option<String>,
    /// 部分推荐接口用 `blurPicUrl` / `picUrl` 之外的字段名。
    #[serde(rename = "blurPicUrl", default)]
    blur_pic_url: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct FeedSong {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    ar: Option<Vec<NameId>>,
    #[serde(default)]
    artists: Option<Vec<NameId>>,
    #[serde(default)]
    al: Option<FeedAlbum>,
    #[serde(default)]
    album: Option<FeedAlbum>,
    #[serde(default)]
    dt: Option<u64>,
    #[serde(default)]
    duration: Option<u64>,
    #[serde(default)]
    no: Option<u32>,
}

fn feed_song_to_track(source: &SourceId, song: FeedSong) -> Track {
    let artists: Vec<ArtistRef> = song
        .ar
        .or(song.artists)
        .unwrap_or_default()
        .into_iter()
        .map(|artist| ArtistRef {
            id: artist.id.map(|id| id.to_string()),
            name: artist.name.unwrap_or_default(),
        })
        .filter(|artist| !artist.name.is_empty())
        .collect();
    let album = song.al.or(song.album).map(|album| AlbumRef {
        id: album.id.map(|id| id.to_string()),
        name: album.name.unwrap_or_default(),
        cover: album.pic_url.or(album.blur_pic_url),
    });
    let mut track = Track::new(
        TrackId::new(source.clone(), song.id.to_string()),
        song.name.unwrap_or_else(|| format!("track {}", song.id)),
        artists,
        album,
        song.dt.or(song.duration),
        TrackOrigin::Remote {
            source: source.clone(),
        },
    );
    track.track_no = song.no;
    // no 参与跨源身份键：构造后补写必须重算。
    track.rebuild_identity_key();
    track
}

fn items<'a>(body: &'a serde_json::Value, pointers: &[&str]) -> &'a [serde_json::Value] {
    pointers
        .iter()
        .find_map(|pointer| body.pointer(pointer).and_then(serde_json::Value::as_array))
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// `/recommend/songs` → 每日推荐曲目。兼容 `data.dailySongs` 与顶层 `dailySongs`。
pub fn map_daily_songs(source: &SourceId, body: &serde_json::Value) -> Vec<Track> {
    items(body, &["/data/dailySongs", "/dailySongs", "/data/songs"])
        .iter()
        .filter_map(|item| serde_json::from_value::<FeedSong>(item.clone()).ok())
        .map(|song| feed_song_to_track(source, song))
        .collect()
}

/// `/personal_fm` → 私人 FM 曲目。兼容顶层 `data` 数组与 `result` 数组。
pub fn map_personal_fm(source: &SourceId, body: &serde_json::Value) -> Vec<Track> {
    items(body, &["/data", "/result", "/data/songs"])
        .iter()
        .filter_map(|item| serde_json::from_value::<FeedSong>(item.clone()).ok())
        .map(|song| feed_song_to_track(source, song))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_daily_songs_and_keeps_track_no() {
        let source = SourceId::new("netease");
        let body = serde_json::json!({
            "data": {
                "dailySongs": [{
                    "id": 1,
                    "name": "A",
                    "ar": [{"id": 2, "name": "X"}],
                    "al": {"id": 3, "name": "Album", "picUrl": "cover"},
                    "dt": 180000,
                    "no": 4
                }]
            }
        });
        let tracks = map_daily_songs(&source, &body);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].track_no, Some(4));
        assert_eq!(tracks[0].artists[0].name, "X");
        assert_eq!(
            tracks[0].album.as_ref().map(|a| a.name.as_str()),
            Some("Album")
        );
        assert!(!tracks[0].identity_key.is_empty());
    }

    #[test]
    fn maps_personal_fm_with_alternate_shape() {
        let source = SourceId::new("netease");
        let body = serde_json::json!({
            "data": [{
                "id": 9,
                "name": "B",
                "artists": [{"id": 10, "name": "Y"}],
                "album": {"name": "Other"},
                "duration": 200000
            }]
        });
        let tracks = map_personal_fm(&source, &body);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].duration_ms, Some(200000));
        assert_eq!(tracks[0].artists[0].name, "Y");
    }

    #[test]
    fn bad_items_are_skipped_not_fatal() {
        let source = SourceId::new("netease");
        let body = serde_json::json!({ "data": [{"name": "missing id"}, {"id": 5, "name": "ok"}] });
        let tracks = map_personal_fm(&source, &body);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "ok");
    }
}
