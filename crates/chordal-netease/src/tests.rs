//! 网易云源测试（由 lib.rs 拆分，行为不变）。

use super::*;
use chordal_cache::CacheConfig;
use std::io::{Read, Write};
use std::net::TcpListener;

#[test]
fn config_defaults() {
    let c = NeteaseConfig::default();
    assert!(c.base_url.contains("127.0.0.1:3000"));
    assert!(c.cookie.is_none());
}

/// 扫码登录：key → create → check(803) → user/account；Cookie 必须注入。
#[test]
fn qr_login_sets_cookie_and_profile() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("");
            // api-enhanced 的 apicache 会缓存未登录响应；扫码/校验链路
            // 每个请求都必须带 cache-buster，否则会读到旧的 801/匿名账号。
            assert!(
                path.contains("timestamp="),
                "missing cache buster in {path}"
            );
            let cookie = request
                .lines()
                .find(|line| line.to_ascii_lowercase().starts_with("cookie:"))
                .unwrap_or("");
            let body = if path.starts_with("/login/qr/key") {
                r#"{"code":200,"data":{"unikey":"k1"}}"#
            } else if path.starts_with("/login/qr/create") {
                r#"{"code":200,"data":{"qrurl":"u","qrimg":"data:image/png;base64,AAA"}}"#
            } else if path.starts_with("/login/qr/check") {
                // api-enhanced 的 cookie 是整串 Set-Cookie（带属性）拼接。
                r#"{"code":803,"cookie":"MUSIC_U=abc; Max-Age=15552000; Path=/; __csrf=zzz"}"#
            } else if path.starts_with("/user/account") {
                assert!(
                    !cookie.contains("Max-Age") && !cookie.contains("Path="),
                    "cookie attributes leaked into request: {cookie}"
                );
                if cookie.contains("MUSIC_U=abc") {
                    r#"{"code":200,"account":{"id":42},"profile":{"userId":42,"nickname":"n","vipType":0}}"#
                } else {
                    r#"{"code":301}"#
                }
            } else {
                r#"{"code":404}"#
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    let source = NeteaseSource::new(
        SourceId::new("netease"),
        NeteaseConfig {
            base_url: format!("http://{addr}"),
            cookie: None,
            level: None,
            mode: NeteaseMode::External,
            ..Default::default()
        },
        "test",
    );
    let account = source.account().expect("account source");
    let challenge = shared_runtime()
        .block_on(account.login_begin(LoginMethod::Qr))
        .unwrap();
    assert_eq!(challenge.key.as_deref(), Some("k1"));
    let progress = shared_runtime()
        .block_on(account.login_poll(&challenge))
        .unwrap();
    match progress {
        LoginProgress::Success(profile) => {
            assert_eq!(profile.uid, "42");
            assert_eq!(profile.nickname, "n");
        }
        other => panic!("unexpected login progress: {other:?}"),
    }
    assert!(account.account_state().is_logged_in());
    assert!(account.cookie().is_some_and(|c| c.contains("MUSIC_U=abc")));
    server.join().unwrap();
}

/// 手机号登录：captcha/sent → login/cellphone → user/account。
#[test]
fn cellphone_login_sets_cookie_and_profile() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("");
            let cookie = request
                .lines()
                .find(|line| line.to_ascii_lowercase().starts_with("cookie:"))
                .unwrap_or("");
            let body = if path.starts_with("/captcha/sent") {
                r#"{"code":200}"#
            } else if path.starts_with("/login/cellphone") {
                r#"{"code":200,"cookie":"MUSIC_U=phone;"}"#
            } else if path.starts_with("/user/account") {
                if cookie.contains("MUSIC_U=phone") {
                    r#"{"code":200,"account":{"id":7},"profile":{"userId":7,"nickname":"手机用户"}}"#
                } else {
                    r#"{"code":301}"#
                }
            } else {
                r#"{"code":404}"#
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    let source = NeteaseSource::new(
        SourceId::new("netease"),
        NeteaseConfig {
            base_url: format!("http://{addr}"),
            cookie: None,
            level: None,
            mode: NeteaseMode::External,
            ..Default::default()
        },
        "test",
    );
    let account = source.account().expect("account source");
    shared_runtime()
        .block_on(account.send_captcha("13800000000"))
        .unwrap();
    let profile = shared_runtime()
        .block_on(account.login_cellphone("13800000000", "123456"))
        .unwrap();
    assert_eq!(profile.uid, "7");
    assert_eq!(profile.nickname, "手机用户");
    assert!(account.account_state().is_logged_in());
    assert!(
        account
            .cookie()
            .is_some_and(|c| c.contains("MUSIC_U=phone"))
    );
    server.join().unwrap();
}

/// 单曲详情缓存：第一次请求落盘，服务关闭后第二次仍能返回。
#[test]
fn track_cache_serves_after_server_stops() {
    let root = std::env::temp_dir().join(format!("chordial-netease-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let cache = Arc::new(
        ObjectCache::open(CacheConfig {
            root: root.clone(),
            limit_bytes: 8 * 1024 * 1024,
        })
        .unwrap(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        // 只服务一次；第二次调用必须命中磁盘缓存。
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
        let body = r#"{"code":200,"songs":[{"id":123,"name":"海阔天空","ar":[{"id":1,"name":"Beyond"}],"al":{"id":2,"name":"乐与怒","picUrl":null},"dt":326000,"no":1}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    });

    let source = NeteaseSource::new_with_cache(
        SourceId::new("netease"),
        NeteaseConfig {
            base_url: format!("http://{addr}"),
            cookie: None,
            level: None,
            mode: NeteaseMode::External,
            ..Default::default()
        },
        "test",
        Some(cache.clone()),
    );
    let id = TrackId::new(SourceId::new("netease"), "123");
    let first = shared_runtime().block_on(source.track(&id)).unwrap();
    assert_eq!(first.title, "海阔天空");
    assert_eq!(first.primary_artist(), "Beyond");
    server.join().unwrap();

    let second = shared_runtime().block_on(source.track(&id)).unwrap();
    assert_eq!(second.title, first.title);
    assert!(cache.stats().entries >= 1);

    drop(source);
    let _ = std::fs::remove_dir_all(&root);
}

/// 从请求行里取查询参数（`?a=1&b=2`）。
fn query_value(path: &str, key: &str) -> Option<usize> {
    path.split_once('?')?.1.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.parse().ok()).flatten()
    })
}

/// 歌单超过 1000 首：`/playlist/track/all` 必须按 offset 分页续拉，
/// 直到取满 `/playlist/detail` 的 trackCount，而不是只留第一页 1000 首。
#[test]
fn playlist_collection_paginates_past_1000_tracks() {
    use std::time::{Duration, Instant};

    const TOTAL: usize = 2500;
    const PAGE: usize = 1000;
    fn song(id: usize) -> String {
        format!(
            r#"{{"id":{id},"name":"song {id}","ar":[{{"id":1,"name":"artist"}}],"al":{{"id":2,"name":"album","picUrl":null}},"dt":180000,"no":{id}}}"#
        )
    }

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests: Vec<(usize, usize)> = Vec::new();
        // 最多等 5 秒：即使被测代码少发/多发了请求，服务线程也能退出。
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(conn) => conn,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => break,
            };
            stream.set_nonblocking(false).unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("")
                .to_string();
            let body = if path.starts_with("/playlist/detail") {
                format!(
                    r#"{{"code":200,"playlist":{{"id":7,"name":"大歌单","trackCount":{TOTAL},"subscribed":false}}}}"#
                )
            } else if path.starts_with("/playlist/track/all") {
                let offset = query_value(&path, "offset").unwrap_or(0);
                let limit = query_value(&path, "limit").unwrap_or(0);
                requests.push((offset, limit));
                let end = (offset + limit).min(TOTAL);
                let songs: Vec<String> = (offset..end).map(|i| song(i + 1)).collect();
                format!(r#"{{"code":200,"songs":[{}]}}"#, songs.join(","))
            } else {
                r#"{"code":404}"#.to_string()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            // 三页曲目取完后 detail 已先行返回；再多等一轮空 accept 只会
            // 让测试变慢，这里直接结束。
            if requests.len() == TOTAL.div_ceil(PAGE) {
                break;
            }
        }
        requests
    });

    let source = NeteaseSource::new(
        SourceId::new("netease"),
        NeteaseConfig {
            base_url: format!("http://{addr}"),
            cookie: None,
            level: None,
            mode: NeteaseMode::External,
            ..Default::default()
        },
        "test",
    );
    let catalog = source.catalog().expect("catalog source");
    let id = CollectionId::new(SourceId::new("netease"), CollectionKind::Playlist, "7");
    let page = shared_runtime().block_on(catalog.collection(&id)).unwrap();

    assert_eq!(page.total, TOTAL as u64);
    assert_eq!(page.tracks.len(), TOTAL);
    assert_eq!(
        page.tracks.first().map(|t| t.title.as_str()),
        Some("song 1")
    );
    assert_eq!(
        page.tracks.last().map(|t| t.title.as_str()),
        Some("song 2500")
    );
    assert!(!page.has_more);

    let requests = server.join().unwrap();
    assert_eq!(
        requests,
        vec![(0, PAGE), (1000, 1000), (2000, 500)],
        "detail 应把 trackCount 作为续拉依据，短页后停止"
    );
}
