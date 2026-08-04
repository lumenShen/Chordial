//! 通用云端来源命令 — 从网络来源实例选择一个云端音乐源进行下载/比较。

use chordial_core::module::music_netease_source::AudioQuality;
use chordial_core::module::music_source::cache;
use chordial_core::module::music_source::types::AudioQualityDescriptor;
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

/// 列出已注册的云端（Web）音乐来源。
///
/// 用于下载对话框的"来源选择"下拉框。仅返回已通过 `registrar.register` 注册的 Web 类型来源。
/// 本地来源不会出现在此列表中。
///
/// # 返回
/// `[{ name, display_name, source_type, enabled }]`
#[tauri::command]
pub fn source_list_web_sources(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<serde_json::Value>, String> {
    let entries = ctx.registrar.get_entries();
    let result: Vec<serde_json::Value> = entries
        .into_iter()
        .filter(|e| e.source_type.is_web())
        .map(|e| {
            // 显示名映射：未来可通过 trait 方法提供；当前硬编码 netease/kugou
            let display_name = match e.name.as_str() {
                "netease" => "网易云音乐",
                "kugou" => "酷狗音乐",
                _ => &e.name,
            };
            serde_json::json!({
                "name": e.name,
                "display_name": display_name,
                "source_type": match &e.source_type {
                    chordial_core::module::music_source::types::SourceType::Web(name) => name.clone(),
                    _ => String::new(),
                },
                "enabled": true, // 已注册即视为启用
            })
        })
        .collect();
    Ok(result)
}

/// 从指定云端来源下载歌曲到本地 download_dir。
///
/// # 参数
/// - `source_name`: 来源名（如 `"netease"`），必须为已注册的 Web 来源
/// - `song_entity_id`: 来源内部的歌曲 entity_id（如 `"song:123456"`）
/// - `title`: 歌曲标题（用于文件命名）
/// - `artist`: 主艺术家名（用于文件命名）
/// - `level`: 音质等级（可选，默认 `"exhigh"`）
///
/// # 返回
/// `{ path: string, quality_descriptor: AudioQualityDescriptor }`
#[tauri::command]
pub fn source_download_song(
    ctx: State<'_, Arc<AppContext>>,
    source_name: String,
    song_entity_id: String,
    title: String,
    artist: String,
    level: Option<String>,
) -> Result<serde_json::Value, String> {
    // 校验来源已注册
    let _ = ctx
        .registrar
        .get(&source_name)
        .ok_or_else(|| format!("来源 '{}' 未注册或未启用", source_name))?;

    use chordial_core::module::music_kugou_source;
    use chordial_core::module::music_netease_source;

    // 分支处理：netease / kugou 各自走对应的下载 API
    let (bytes, quality_descriptor, ext, download_dir_str) = if source_name
        == music_netease_source::NETEASE_SOURCE_NAME
    {
        // ── 网易云下载 ──
        let remote_id = song_entity_id
            .splitn(3, ':')
            .nth(1)
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| format!("无效的 entity_id: {}", song_entity_id))?;

        let level_str = level.unwrap_or_else(|| AudioQuality::Exhigh.to_level_str().to_string());
        let (b, url_data) = music_netease_source::api::download_song(
            ctx.netease_source.client(),
            remote_id,
            &level_str,
        )?;
        let qd = url_data.to_quality_descriptor();
        let e = url_data
            .r#type
            .as_deref()
            .and_then(|t| match t.to_lowercase().as_str() {
                "flac" => Some("flac"),
                "mp3" => Some("mp3"),
                "m4a" => Some("m4a"),
                _ => None,
            })
            .unwrap_or_else(|| match level_str.as_str() {
                "lossless" | "hires" => "flac",
                _ => "mp3",
            });
        (b, qd, e.to_string(), ctx.netease_source.download_dir())
    } else if source_name == music_kugou_source::KUGOU_SOURCE_NAME {
        // ── 酷狗下载 ──
        // entity_id 格式：song:<hash>，hash 是 32 位十六进制字符串
        let hash = song_entity_id
            .splitn(3, ':')
            .nth(1)
            .ok_or_else(|| format!("无效的 entity_id: {}", song_entity_id))?;
        if hash.is_empty() {
            return Err(format!("无效的 hash: {}", song_entity_id));
        }
        // level 可选：酷狗音质字符串（"128"/"320"/"flac"/"high"/"super"），默认 "320"
        let quality_str = level.unwrap_or_else(|| "320".to_string());
        let (b, url_data) = music_kugou_source::api::download_song(
            ctx.kugou_source.client(),
            hash,
            &quality_str,
            None,
        )?;
        // 从酷狗返回的 URL 数据构造音质描述符
        let url_entry = url_data
            .url
            .first()
            .ok_or_else(|| "酷狗返回空 URL 列表".to_string())?;
        let qd = build_kugou_quality_descriptor(&url_entry.quality, url_entry.bitRate, &url_entry.r#type);
        let e = match url_entry.r#type.to_lowercase().as_str() {
            "flac" => "flac",
            "mp3" => "mp3",
            "m4a" | "aac" => "m4a",
            _ => match url_entry.quality.as_str() {
                "flac" | "high" | "super" => "flac",
                _ => "mp3",
            },
        };
        (b, qd, e.to_string(), ctx.kugou_source.download_dir())
    } else {
        return Err(format!(
            "来源 '{}' 暂不支持下载（仅支持 netease/kugou）",
            source_name
        ));
    };

    // 解析 download_dir：优先来源配置；为空则回退到 app_data_dir/downloads
    let download_dir = if download_dir_str.is_empty() {
        let config_path = ctx.config.path();
        let parent = config_path
            .parent()
            .ok_or_else(|| "无法定位应用数据目录".to_string())?;
        parent.join("downloads")
    } else {
        std::path::PathBuf::from(download_dir_str)
    };

    let path = cache::write_downloaded_file(&download_dir, &source_name, &artist, &title, &ext, &bytes)?;

    Ok(serde_json::json!({
        "path": path.to_string_lossy(),
        "quality_descriptor": quality_descriptor,
        "quality_label": quality_descriptor.label(),
    }))
}

/// 从酷狗返回数据构造音质描述符。
///
/// 酷狗 quality 字段为字符串（"128"/"320"/"flac"/"high"/"super"），
/// 映射到内部 [`AudioQuality`] 后再生成描述符。
fn build_kugou_quality_descriptor(
    quality: &str,
    bit_rate: i64,
    file_type: &str,
) -> AudioQualityDescriptor {
    use chordial_core::module::music_source::types::{AudioCodec, AudioQuality};
    let aq = match quality.to_lowercase().as_str() {
        "128" => AudioQuality::Standard,
        "320" => AudioQuality::Exhigh,
        "flac" | "high" => AudioQuality::Lossless,
        "super" => AudioQuality::Hires,
        _ => AudioQuality::Unknown,
    };
    let mut desc = aq.descriptor();
    // 若接口返回了真实码率，覆盖默认值
    if bit_rate > 0 {
        desc.bitrate_kbps = bit_rate as u32;
    }
    // 若接口返回了真实编码格式，覆盖默认值
    if !file_type.is_empty() {
        desc.codec = match file_type.to_lowercase().as_str() {
            "flac" => AudioCodec::Flac,
            "mp3" => AudioCodec::Mp3,
            "m4a" | "aac" => AudioCodec::Aac,
            _ => desc.codec,
        };
    }
    desc
}

/// 比较 online 歌曲与本地库中相同歌曲的音质（通用版本）。
///
/// 与 [`netease_compare_with_local`] 行为一致，但接受 `source_name` 参数以便未来支持多来源。
/// 当前仅 netease 来源可用，其他来源返回空匹配。
///
/// # 返回
/// `{ matches: [...], has_local_match: bool }`
/// 每个 match 包含完整的 `quality_descriptor`（rank + bitrate_kbps + codec + 采样率 + 位深）
#[tauri::command]
pub fn source_compare_with_local(
    ctx: State<'_, Arc<AppContext>>,
    source_name: String,
    title: String,
    artist: String,
    album: Option<String>,
) -> Result<serde_json::Value, String> {
    // 校验来源已注册
    if ctx.registrar.get(&source_name).is_none() {
        return Err(format!("来源 '{}' 未注册或未启用", source_name));
    }

    let title_lower = title.to_lowercase();
    let artist_lower = artist.to_lowercase();
    let album_lower = album
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);

    let candidates = ctx.library.search_songs(&title);
    let matches: Vec<serde_json::Value> = candidates
        .into_iter()
        .filter(|s| s.title.to_lowercase().contains(&title_lower))
        .filter(|s| {
            if artist.is_empty() {
                return true;
            }
            s.artist_names
                .iter()
                .any(|a| a.to_lowercase().contains(&artist_lower))
        })
        .filter(|s| match (&album_lower, &s.album_title) {
            (None, _) | (_, None) => true,
            (Some(al), Some(at)) => at.to_lowercase().contains(al),
        })
        .map(|s| {
            let local_path = s
                .source_ids
                .iter()
                .find(|sid| sid.source_name == "local")
                .map(|sid| sid.entity_id.clone());
            let descriptor = local_path
                .as_ref()
                .and_then(|p| std::path::Path::new(p).extension())
                .and_then(|e| e.to_str())
                .map(AudioQualityDescriptor::from_extension)
                .unwrap_or_else(AudioQualityDescriptor::unknown);
            let quality = AudioQuality::from(descriptor.rank);
            serde_json::json!({
                "song_id": s.id,
                "title": s.title,
                "artist_names": s.artist_names,
                "album_title": s.album_title,
                "file_path": local_path,
                "quality": quality.to_level_str(),
                "quality_rank": quality as u8,
                "quality_descriptor": descriptor,
                "quality_label": descriptor.label(),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "matches": matches,
        "has_local_match": !matches.is_empty(),
    }))
}
