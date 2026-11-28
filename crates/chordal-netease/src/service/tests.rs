//! 托管服务测试（由 service.rs 拆分，行为不变）。

use super::helpers::*;
use super::*;

#[test]
fn auto_uses_manager_but_external_stays_direct() {
    let config = ManagedConfig {
        app_dir: Some(std::env::temp_dir().join("chordial-no-such-app")),
        ..Default::default()
    };
    assert!(resolve_app_dir(&config).is_none());
    let endpoint = Arc::new(Endpoint::new("http://127.0.0.1:3000"));
    let auto = NeteaseService::from_config(NeteaseMode::Auto, &config, endpoint.clone());
    assert!(auto.is_some(), "Auto may install/fallback at runtime");
    let external = NeteaseService::from_config(NeteaseMode::External, &config, endpoint);
    assert!(external.is_none());
}

#[test]
fn release_asset_and_digest_helpers() {
    let asset = release_asset_name().unwrap();
    assert!(asset.starts_with("ncm-api-"));
    assert_eq!(normalize_sha256("SHA256:ABCdef"), "abcdef");
    assert_eq!(normalize_sha256(" abc "), "abc");
}

#[test]
fn installed_binary_respects_pinned_version() {
    let dir = std::env::temp_dir().join(format!("chordial-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let binary = dir.join(binary_filename());
    std::fs::write(&binary, b"stub").unwrap();
    let config = ManagedConfig {
        install_dir: Some(dir.clone()),
        api_version: Some("v1.0.0".into()),
        ..Default::default()
    };
    // 没有 installed.json：宁可重新安装，也不执行未知版本。
    assert!(installed_binary(&config).is_none());
    std::fs::write(
        dir.join("installed.json"),
        br#"{"version":"v1.0.0","asset":"x","sha256":"y"}"#,
    )
    .unwrap();
    assert_eq!(installed_binary(&config), Some(binary.clone()));
    let other = ManagedConfig {
        api_version: Some("v2.0.0".into()),
        ..config.clone()
    };
    assert!(installed_binary(&other).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn explicit_app_dir_is_detected() {
    let dir = std::env::temp_dir().join(format!("chordial-sidecar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.js"), "console.log('stub')").unwrap();
    let config = ManagedConfig {
        app_dir: Some(dir.clone()),
        ..Default::default()
    };
    assert_eq!(resolve_app_dir(&config), Some(dir.clone()));
    assert_eq!(resolve_entry(&dir), Some(dir.join("app.js")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn redacts_cookie_fragments() {
    let line = "cookie: MUSIC_U=abc123; __csrf=xyz; other=1";
    let redacted = redact(line);
    assert!(!redacted.contains("abc123"));
    assert!(!redacted.contains("xyz"));
    assert!(redacted.contains("other=1"));
}

/// 真实下载官方 api-enhanced release 二进制（约 60-80MB）。
/// 默认忽略；网络可用时用 `cargo test -p chordal-netease -- --ignored` 跑。
#[test]
#[ignore = "downloads api-enhanced release from GitHub"]
fn installs_official_binary_from_release() {
    let dir =
        std::env::temp_dir().join(format!("chordial-official-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ManagedConfig {
        install_dir: Some(dir.clone()),
        ..Default::default()
    };
    let logs = Arc::new(Mutex::new(VecDeque::new()));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let binary = runtime
        .block_on(install_binary(&config, &logs))
        .expect("official api-enhanced download");
    assert!(binary.is_file());
    let meta: InstalledMeta =
        serde_json::from_str(&std::fs::read_to_string(dir.join("installed.json")).unwrap())
            .unwrap();
    assert!(!meta.version.is_empty());
    assert_eq!(meta.sha256.len(), 64);

    // 用刚安装的官方二进制完整跑一遍启动/就绪/停止。
    let endpoint = Arc::new(Endpoint::new("http://127.0.0.1:1"));
    let service =
        NeteaseService::from_config(NeteaseMode::Managed, &config, endpoint.clone()).unwrap();
    let base = runtime.block_on(service.ensure_ready()).unwrap();
    assert!(base.starts_with("http://127.0.0.1:"));
    assert_eq!(endpoint.get(), base);
    runtime.block_on(service.stop());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 真实 spawn 一个最小 Node HTTP 服务，验证端口/就绪/停止。
/// 没有 Node 的环境自动跳过。
#[test]
fn spawns_and_stops_a_node_process() {
    let node_available = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !node_available {
        eprintln!("node not available; skipping sidecar spawn test");
        return;
    }
    let dir = std::env::temp_dir().join(format!("chordial-sidecar-spawn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("app.js"),
        r#"
const http = require('http');
const port = Number(process.env.PORT);
http.createServer((req, res) => {
  res.setHeader('content-type', 'application/json');
  res.end(JSON.stringify({ code: 200, data: { account: null } }));
}).listen(port, '127.0.0.1', () => console.log('stub-ready'));
"#,
    )
    .unwrap();
    let config = ManagedConfig {
        app_dir: Some(dir.clone()),
        startup_timeout_ms: 8_000,
        ..Default::default()
    };
    let endpoint = Arc::new(Endpoint::new("http://127.0.0.1:1"));
    let service =
        NeteaseService::from_config(NeteaseMode::Managed, &config, endpoint.clone()).unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let base = runtime.block_on(service.ensure_ready()).unwrap();
    assert!(base.starts_with("http://127.0.0.1:"));
    assert_eq!(endpoint.get(), base);
    // 停止后再次 ensure_ready 应能重新拉起（端口可能变化）。
    runtime.block_on(service.stop());
    let base2 = runtime.block_on(service.ensure_ready()).unwrap();
    assert!(base2.starts_with("http://127.0.0.1:"));
    assert_eq!(endpoint.get(), base2);
    runtime.block_on(service.stop());
    let _ = std::fs::remove_dir_all(&dir);
}
