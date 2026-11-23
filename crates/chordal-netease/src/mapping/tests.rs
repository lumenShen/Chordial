//! 网易云映射测试（由 mapping.rs 拆分，行为不变）。

use super::*;

use serde_json::json;

#[test]
fn song_detail_maps_album_year() {
    let body = json!({
        "songs": [{
            "id": 1,
            "name": "终于见到了你",
            "ar": [{"id": 2, "name": "吴琳珂Moske"}],
            "al": {
                "id": 3,
                "name": "终于见到了你",
                "picUrl": "https://x/c.jpg",
                "publishTime": 738_000_000_000u64
            },
            "dt": 182_000,
            "no": 1
        }]
    });
    let track = map_song_detail(&SourceId::new("netease"), &body, "1").unwrap();
    assert_eq!(track.title, "终于见到了你");
    assert_eq!(track.primary_artist(), "吴琳珂Moske");
    assert_eq!(track.album.as_ref().unwrap().name, "终于见到了你");
    assert_eq!(track.album.as_ref().unwrap().cover.as_deref(), Some("https://x/c.jpg"));
    assert_eq!(track.year, Some(1993));
    assert_eq!(track.track_no, Some(1));
}

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
    // 元数据只进 credits，不进入歌词时间轴。
    assert_eq!(lyrics.credits, vec!["作词".to_string()]);
    assert_eq!(lyrics.timed_lines.len(), 1);
    assert!(!lyrics.lines.iter().any(|l| l.text == "作词"));
    let sung = lyrics
        .timed_lines
        .iter()
        .find(|l| l.start_ms == 1000)
        .expect("sung line");
    assert_eq!(sung.words.len(), 2);
    assert_eq!(sung.words[0].text, "你");
    assert!(lyrics.translation.iter().any(|t| t == "hello"));
}

#[test]
fn map_lyrics_new_real_api_fixture() {
    let yrc = concat!(
        "{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"宇多田ヒカル\",\"li\":\"http://x/a.jpg\",\"or\":\"orpheus://nm/artist/home?id=1\"}]}\n",
        "{\"t\":1000,\"c\":[{\"tx\":\"编曲: \"},{\"tx\":\"宇多田ヒカル\"},{\"tx\":\"/\"},{\"tx\":\"A. G. Cook\"}]}\n",
        "{\"t\":2000,\"c\":[{\"tx\":\"制作人: \"},{\"tx\":\"宇多田ヒカル\"},{\"tx\":\"/\"},{\"tx\":\"A. G. Cook\"}]}\n",
        "{\"t\":3000,\"c\":[{\"tx\":\"作曲: \"},{\"tx\":\"宇多田ヒカル\"}]}\n",
        "[20570,1900](20570,360,0)初(20930,150,0)め(21080,120,0)て(21200,170,0)の(21370,60,0)ル(21430,190,0)ー(21620,150,0)ブ(21770,170,0)ル(21940,530,0)は"
    );
    let body = json!({
        "code": 200,
        "yrc": {"lyric": yrc},
        "lrc": {"lyric": "[00:20.57]初めてのルーブルは"},
        "tlyric": {"lyric": "[00:20.54]tlyric 译文"},
        "ytlrc": {"lyric": "[00:20.570]第一次去卢浮宫时"}
    });
    let lyrics = map_lyrics_new(&body).expect("real fixture");
    // 四条制作信息进 credits；歌词时间轴只剩真正演唱的行。
    assert_eq!(
        lyrics.credits,
        vec![
            "作词: 宇多田ヒカル".to_string(),
            "编曲: 宇多田ヒカル/A. G. Cook".to_string(),
            "制作人: 宇多田ヒカル/A. G. Cook".to_string(),
            "作曲: 宇多田ヒカル".to_string(),
        ]
    );
    assert_eq!(lyrics.timed_lines.len(), 1);
    assert_eq!(lyrics.lines.len(), 1);
    let sung = lyrics
        .timed_lines
        .iter()
        .find(|l| l.start_ms == 20_570)
        .expect("first sung line");
    assert_eq!(sung.words.len(), 9);
    assert_eq!(
        sung.words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<String>(),
        "初めてのルーブルは"
    );
    assert_eq!(sung.end_ms, 22_470);
    let sung_index = lyrics
        .lines
        .iter()
        .position(|l| l.time_ms == 20_570)
        .expect("line index");
    assert_eq!(lyrics.translation[sung_index], "第一次去卢浮宫时");
}

#[test]
fn map_lyrics_new_falls_back_to_lrc_when_yrc_missing() {
    let body = json!({
        "lrc": {"lyric": "[00:01.00]你好
    [00:02.00]世界"},
        "yrc": {"lyric": ""}
    });
    let lyrics = map_lyrics_new(&body).expect("lrc fallback");
    assert_eq!(lyrics.lines.len(), 2);
    assert_eq!(lyrics.lines[0].text, "你好");
    assert_eq!(lyrics.lines[1].text, "世界");
    assert!(lyrics.timed_lines.is_empty());
}

#[test]
fn map_lyrics_new_prefers_yrc_over_lrc() {
    let body = json!({
        "lrc": {"lyric": "[00:01.00]LRC 文本"},
        "yrc": {"lyric": "[1000,500](1000,200,0)逐(1200,300,0)字"}
    });
    let lyrics = map_lyrics_new(&body).expect("yrc preferred");
    assert_eq!(lyrics.timed_lines.len(), 1);
    assert_eq!(lyrics.timed_lines[0].words.len(), 2);
    assert_eq!(lyrics.lines[0].text, "逐字");
}

#[test]
fn map_lyrics_new_metadata_only_yrc_still_uses_lrc() {
    let body = json!({
        "lrc": {"lyric": "[00:01.00]你好"},
        "yrc": {"lyric": "{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"某人\"}]}"}
    });
    let lyrics = map_lyrics_new(&body).expect("metadata + lrc");
    assert_eq!(lyrics.credits, vec!["作词: 某人".to_string()]);
    assert_eq!(lyrics.lines.len(), 1);
    assert_eq!(lyrics.lines[0].text, "你好");
    assert!(lyrics.timed_lines.is_empty());
}

#[test]
fn map_lyrics_new_dedupes_repeated_credits() {
    // QQ 的 YRC 常把同一组制作信息在不同时间戳重复下发；
    // 全/半角冒号与空格差异也算同一条。
    let yrc = concat!(
        "{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"萧玮\"}]}\n",
        "{\"t\":1000,\"c\":[{\"tx\":\"作曲：\"},{\"tx\":\"张亚东\"}]}\n",
        "{\"t\":2000,\"c\":[{\"tx\":\"作词 : \"},{\"tx\":\"萧玮\"}]}\n",
        "{\"t\":3000,\"c\":[{\"tx\":\"作曲: 张亚东\"}]}\n",
        "[5000,500](5000,200,0)我(5200,300,0)爱(5300,200,0)你"
    );
    let body = json!({ "yrc": {"lyric": yrc} });
    let lyrics = map_lyrics_new(&body).expect("dedupe");
    assert_eq!(
        lyrics.credits,
        vec!["作词: 萧玮".to_string(), "作曲：张亚东".to_string()]
    );
    assert_eq!(lyrics.timed_lines.len(), 1);
    assert_eq!(lyrics.lines.len(), 1);
}

#[test]
fn map_lyrics_new_moves_inline_credit_lines_out_of_timeline() {
    // 正文里直接写出来的制作信息行（YRC 逐词行 / LRC 行）同样要截获。
    let yrc = concat!(
        "[0,1500](0,300,0)作(300,300,0)词(600,300,0):(900,300,0)萧(1200,300,0)玮\n",
        "[5000,500](5000,200,0)我(5200,300,0)爱(5300,200,0)你"
    );
    let body = json!({
        "lrc": {"lyric": "[00:00.00]编曲: 张亚东\n[00:05.00]我爱你"},
        "yrc": {"lyric": yrc}
    });
    let lyrics = map_lyrics_new(&body).expect("inline credits");
    assert_eq!(
        lyrics.credits,
        vec!["作词:萧玮".to_string(), "编曲: 张亚东".to_string()]
    );
    assert_eq!(lyrics.lines.len(), 1);
    assert_eq!(lyrics.lines[0].text, "我爱你");
    assert_eq!(lyrics.timed_lines.len(), 1);
    assert_eq!(lyrics.timed_lines[0].text(), "我爱你");
}

#[test]
fn credit_line_detection_is_conservative() {
    // 角色词后必须有分隔符，正常歌词不能被误伤。
    assert!(is_credit_line("作词: 某人"));
    assert!(is_credit_line("作曲：某人"));
    assert!(is_credit_line("制作人/某人"));
    assert!(is_credit_line("词: 某人"));
    assert!(is_credit_line("词曲：某人"));
    assert!(is_credit_line("Guitar: John"));
    assert!(!is_credit_line("词不达意"));
    assert!(!is_credit_line("作曲系的我"));
    assert!(!is_credit_line("Guitar solo"));
    assert!(!is_credit_line("我爱你"));
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

    // 匿名账号（api-enhanced 自动注册）不能当成已登录。
    let anonymous = json!({
        "code": 200,
        "account": {"id": 9, "anonimousUser": true, "type": 1000},
        "profile": null
    });
    assert!(map_account(&source, &anonymous).is_none());
    // 过期 Cookie：只有 account.id、没有 profile。
    let stale = json!({"code": 200, "account": {"id": 9}});
    assert!(map_account(&source, &stale).is_none());

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
