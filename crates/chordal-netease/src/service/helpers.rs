//! 托管服务辅助（由 service.rs 拆分，行为不变）。

use super::*;

pub(super) struct LaunchTarget {
    pub(super) program: PathBuf,
    pub(super) args: Vec<PathBuf>,
    pub(super) cwd: PathBuf,
    pub(super) node: bool,
    pub(super) label: String,
}

impl LaunchTarget {
    pub(super) fn binary(path: PathBuf) -> Self {
        let cwd = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            label: format!("binary {}", path.display()),
            program: path,
            args: Vec::new(),
            cwd,
            node: false,
        }
    }

    pub(super) fn node(program: PathBuf, entry: PathBuf, app_dir: PathBuf) -> Self {
        Self {
            label: format!("node {} {}", program.display(), entry.display()),
            program,
            args: vec![entry],
            cwd: app_dir,
            node: true,
        }
    }
}

pub(super) fn node_target(dir: &Path, config: &ManagedConfig) -> Option<LaunchTarget> {
    let entry = resolve_entry(dir)?;
    let node = resolve_node(config, dir);
    Some(LaunchTarget::node(node, entry, dir.to_path_buf()))
}

pub(super) fn install_root(config: &ManagedConfig) -> PathBuf {
    config.install_dir.clone().unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| std::env::temp_dir())
            .join("chordial")
            .join("netease-api")
    })
}

pub(super) fn binary_filename() -> &'static str {
    if cfg!(windows) {
        "api-enhanced.exe"
    } else {
        "api-enhanced"
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct InstalledMeta {
    pub(super) version: String,
    asset: String,
    pub(super) sha256: String,
}

pub(super) fn installed_binary(config: &ManagedConfig) -> Option<PathBuf> {
    let binary = install_root(config).join(binary_filename());
    if !binary.is_file() {
        return None;
    }
    let meta: Option<InstalledMeta> =
        std::fs::read_to_string(binary.with_file_name("installed.json"))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok());
    if let Some(requested) = config
        .api_version
        .as_deref()
        .filter(|v| !v.is_empty() && *v != "latest")
    {
        match &meta {
            Some(meta) if meta.version == requested => {}
            _ => return None,
        }
    }
    if let Some(expected) = config.api_sha256.as_deref() {
        match &meta {
            Some(meta) if meta.sha256 == normalize_sha256(expected) => {}
            _ => return None,
        }
    }
    Some(binary)
}

pub(super) fn release_asset_name() -> CoreResult<&'static str> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Ok("ncm-api-win-x64.exe");
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return Ok("ncm-api-linux-x64");
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return Ok("ncm-api-macos-x64");
    }
    #[allow(unreachable_code)]
    Err(CoreError::source_err(
        "netease",
        format!(
            "api-enhanced 暂无当前平台预编译包（{}/{}）",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    ))
}

pub(super) fn normalize_sha256(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("sha256:")
        .trim_start_matches("SHA256:")
        .to_ascii_lowercase()
}

/// 下载官方 api-enhanced release 二进制，校验 SHA-256 后原子安装。
pub(super) async fn install_binary(
    config: &ManagedConfig,
    logs: &Arc<Mutex<VecDeque<String>>>,
) -> CoreResult<PathBuf> {
    if let Some(binary) = installed_binary(config) {
        return Ok(binary);
    }
    let root = install_root(config);
    tokio::fs::create_dir_all(&root).await.map_err(|e| {
        CoreError::source_err(
            "netease",
            format!("创建安装目录失败 {}：{e}", root.display()),
        )
    })?;
    let asset_name = release_asset_name()?;
    let version = config.api_version.as_deref().unwrap_or("latest");
    let release_url = if version == "latest" {
        "https://api.github.com/repos/NeteaseCloudMusicApiEnhanced/api-enhanced/releases/latest"
            .to_string()
    } else {
        format!(
            "https://api.github.com/repos/NeteaseCloudMusicApiEnhanced/api-enhanced/releases/tags/{version}"
        )
    };
    push_log(
        logs,
        format!("fetching api-enhanced release: {release_url}"),
    );
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(1_800))
        .user_agent(concat!("Chordial/2.0 (", env!("CARGO_PKG_VERSION"), ")"))
        .build()
        .map_err(|e| CoreError::source_err("netease", format!("创建下载客户端失败：{e}")))?;
    let release: serde_json::Value = client
        .get(&release_url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| CoreError::source_err("netease", format!("获取 release 失败：{e}")))?
        .error_for_status()
        .map_err(|e| CoreError::source_err("netease", format!("release HTTP 错误：{e}")))?
        .json()
        .await
        .map_err(|e| CoreError::source_err("netease", format!("解析 release 失败：{e}")))?;
    let tag = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let asset = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .and_then(|assets| {
            assets
                .iter()
                .find(|a| a.get("name").and_then(serde_json::Value::as_str) == Some(asset_name))
        })
        .ok_or_else(|| {
            CoreError::source_err("netease", format!("{tag} 中没有资产 {asset_name}"))
        })?;
    // 优先用 API asset URL（Accept: application/octet-stream）：它重定向到
    // release-assets.githubusercontent.com，比 github.com/releases/download
    // 在受限网络下更容易下载。
    let api_asset_url = asset
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let url = api_asset_url
        .or_else(|| {
            asset
                .get("browser_download_url")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| CoreError::source_err("netease", "release 资产缺少下载地址"))?;
    let api_digest = asset
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .map(normalize_sha256);
    if let (Some(expected), Some(api)) = (&config.api_sha256, &api_digest)
        && normalize_sha256(expected) != *api
    {
        return Err(CoreError::source_err(
            "netease",
            "managed.api_sha256 与 GitHub release 摘要不一致",
        ));
    }
    let expected = config
        .api_sha256
        .as_deref()
        .map(normalize_sha256)
        .or(api_digest)
        .ok_or_else(|| {
            CoreError::source_err(
                "netease",
                "release 未提供 SHA-256 且未配置 managed.api_sha256；拒绝安装未校验的二进制",
            )
        })?;

    push_log(logs, format!("downloading {url}"));
    let response = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .map_err(|e| CoreError::source_err("netease", format!("下载失败：{e}")))?
        .error_for_status()
        .map_err(|e| CoreError::source_err("netease", format!("下载 HTTP 错误：{e}")))?;
    let temp = root.join(format!(".download-{}.part", std::process::id()));
    let mut file = tokio::fs::File::create(&temp)
        .await
        .map_err(|e| CoreError::source_err("netease", format!("创建临时文件失败：{e}")))?;
    use futures::StreamExt;
    use sha2::Digest;
    use tokio::io::AsyncWriteExt;
    let mut stream = response.bytes_stream();
    let mut total: u64 = 0;
    let mut hasher = sha2::Sha256::new();
    const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(e) => {
                let _ = tokio::fs::remove_file(&temp).await;
                return Err(CoreError::source_err(
                    "netease",
                    format!("读取下载流失败：{e}"),
                ));
            }
        };
        total += chunk.len() as u64;
        if total > MAX_DOWNLOAD {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(CoreError::source_err(
                "netease",
                "api-enhanced 下载超过 300MB，已中止",
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| CoreError::source_err("netease", format!("写入文件失败：{e}")))?;
    }
    file.flush().await.ok();
    drop(file);
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != expected {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(CoreError::source_err(
            "netease",
            format!("api-enhanced 校验失败：期望 {expected}，实际 {actual}"),
        ));
    }
    let binary = root.join(binary_filename());
    let _ = tokio::fs::remove_file(&binary).await;
    tokio::fs::rename(&temp, &binary)
        .await
        .map_err(|e| CoreError::source_err("netease", format!("安装二进制失败：{e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755));
    }
    let meta = InstalledMeta {
        version: tag.clone(),
        asset: asset_name.to_string(),
        sha256: actual.clone(),
    };
    if let Ok(json) = serde_json::to_vec_pretty(&meta) {
        let _ = tokio::fs::write(root.join("installed.json"), json).await;
    }
    push_log(
        logs,
        format!(
            "installed api-enhanced {tag} ({:.1} MB, sha256 {})",
            total as f64 / 1_048_576.0,
            actual
        ),
    );
    Ok(binary)
}

pub(super) fn push_log(logs: &Arc<Mutex<VecDeque<String>>>, line: impl Into<String>) {
    if let Ok(mut logs) = logs.lock() {
        if logs.len() >= 200 {
            logs.pop_front();
        }
        logs.push_back(redact(&line.into()));
    }
}

pub(super) async fn wait_ready(
    child: &mut Child,
    base_url: &str,
    timeout: Duration,
    logs: &Arc<Mutex<VecDeque<String>>>,
) -> CoreResult<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let tail = logs
                .lock()
                .map(|logs| {
                    logs.iter()
                        .rev()
                        .take(8)
                        .rev()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            return Err(CoreError::source_err(
                "netease",
                format!("Node 进程提前退出（{:?}）\n{tail}", status.code()),
            ));
        }
        if probe(base_url).await {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(CoreError::source_err(
                "netease",
                format!("等待 {base_url} 就绪超时"),
            ));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// 探测 `/login/status`：未登录也返回 JSON（code/data），足以确认服务身份。
pub(super) async fn probe(base_url: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    let Ok(response) = client.get(format!("{base_url}/login/status")).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    response
        .json::<serde_json::Value>()
        .await
        .map(|body| body.get("code").is_some() || body.get("data").is_some())
        .unwrap_or(false)
}

pub(super) fn spawn_log_reader<R>(stream: R, logs: Arc<Mutex<VecDeque<String>>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // 不把 Cookie/URL token 打进日志尾部。
            let line = redact(&line);
            if let Ok(mut logs) = logs.lock() {
                if logs.len() >= 200 {
                    logs.pop_front();
                }
                logs.push_back(line);
            }
        }
    });
}

pub(super) fn redact(line: &str) -> String {
    let mut out = line.to_string();
    for needle in ["MUSIC_U=", "__csrf="] {
        if let Some(pos) = out.find(needle) {
            let end = out[pos..]
                .find(|c: char| c == ';' || c == '&' || c.is_whitespace())
                .map(|i| pos + i)
                .unwrap_or(out.len());
            out.replace_range(pos + needle.len()..end, "***");
        }
    }
    out
}

pub(super) fn pick_port(configured: Option<u16>) -> CoreResult<u16> {
    if let Some(port) = configured {
        // 提前确认端口可用；绑定后立即释放，交给 Node 监听。
        TcpListener::bind(("127.0.0.1", port))
            .map(|_| port)
            .map_err(|e| {
                CoreError::source_err("netease", format!("managed.port {port} 不可用（{e}）"))
            })
    } else {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| CoreError::source_err("netease", format!("挑选本地端口失败：{e}")))?;
        listener
            .local_addr()
            .map(|addr| addr.port())
            .map_err(|e| CoreError::source_err("netease", format!("读取本地端口失败：{e}")))
    }
}

/// 解析 api-enhanced 目录。显式配置优先，其次环境变量、随包 resources、开发期 vendor。
pub fn resolve_app_dir(config: &ManagedConfig) -> Option<PathBuf> {
    candidate_app_dirs(config)
        .into_iter()
        .find(|dir| resolve_entry(dir).is_some())
}

pub(super) fn candidate_app_dirs(config: &ManagedConfig) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = &config.app_dir {
        candidates.push(dir.clone());
    }
    if let Ok(dir) = std::env::var("CHORDIAL_NETEASE_APP") {
        if !dir.trim().is_empty() {
            candidates.push(PathBuf::from(dir));
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("resources").join("netease-api"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("resources").join("netease-api"));
        candidates.push(cwd.join("vendor").join("netease-api"));
    }
    candidates
}

pub(super) fn resolve_entry(app_dir: &Path) -> Option<PathBuf> {
    ["app.js", "server.js", "index.js"]
        .into_iter()
        .map(|name| app_dir.join(name))
        .find(|path| path.is_file())
}

pub(super) fn resolve_node(config: &ManagedConfig, app_dir: &Path) -> PathBuf {
    if let Some(node) = &config.node_path {
        return node.clone();
    }
    let bundled = if cfg!(windows) {
        app_dir.join("node.exe")
    } else {
        app_dir.join("node")
    };
    if bundled.is_file() {
        return bundled;
    }
    PathBuf::from(if cfg!(windows) { "node.exe" } else { "node" })
}
