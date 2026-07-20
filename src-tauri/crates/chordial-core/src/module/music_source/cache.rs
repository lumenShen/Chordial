//! 音乐缓存机制 — 在线来源下载到本地指定目录，并按音质级别决定是否替换。
//!
//! # 工作流程
//!
//! 1. 用户请求播放一首在线来源（如网易云）的歌曲
//! 2. 系统检查本地是否已有相同歌曲（按 标题+艺术家+专辑 模糊匹配）
//! 3. 若本地已有：
//!    - 比较音质（按 [`AudioQuality`]）
//!    - 本地音质 ≥ 在线音质 → 直接用本地文件
//!    - 本地音质 < 在线音质 → 下载在线版本到 download_dir，并更新库内引用
//! 4. 若本地没有：直接下载在线版本到 download_dir，并加入库
//!
//! # 持久化
//!
//! - download_dir 路径通过 [`WebSourceConfig::download_dir`] 持久化（每个来源独立）
//! - 默认 download_dir 由 [`default_download_dir`] 提供（应用数据目录下 `downloads/`）
//!
//! # 文件命名
//!
//! `download_dir/<source>/<artist> - <title>.<ext>`
//! - `<source>`: 来源名（如 `netease`）
//! - `<artist>`: 主艺术家名（去掉非法文件名字符）
//! - `<title>`: 歌曲标题（去掉非法字符）
//! - `<ext>`: 文件扩展名（从 `Content-Type` 或 URL 推断）

use crate::module::music_netease_source::types::AudioQuality;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};
use crate::module::platform::{self, PlatformPath};
use std::path::PathBuf;

/// 默认下载目录名（应用数据目录下）。
pub const DEFAULT_DOWNLOAD_DIR_NAME: &str = "downloads";

/// 获取默认下载目录路径。
///
/// 优先使用 `dirs::audio_dir()`；若不可用则使用应用数据目录下的 `downloads/`。
pub fn default_download_dir(app_data_dir: &std::path::Path) -> PathBuf {
    if let Some(audio_dir) = dirs::audio_dir() {
        let candidate = audio_dir.join("Chordial");
        return candidate;
    }
    app_data_dir.join(DEFAULT_DOWNLOAD_DIR_NAME)
}

/// 规范化文件名 — 移除文件系统不允许的字符。
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// 构建下载文件的相对路径（相对于 download_dir）。
///
/// 格式：`<source>/<artist> - <title>.<ext>`
pub fn build_relative_path(
    source_name: &str,
    artist: &str,
    title: &str,
    ext: &str,
) -> PathBuf {
    let source_dir = sanitize_filename(source_name);
    let artist = if artist.is_empty() {
        "Unknown".to_string()
    } else {
        sanitize_filename(artist)
    };
    let title = sanitize_filename(title);
    let ext = ext.trim_start_matches('.');
    let filename = format!("{} - {}.{}", artist, title, ext);
    PathBuf::from(source_dir).join(filename)
}

/// 比较本地和在线音质。
///
/// # 返回
/// - `Greater`: 本地音质更高，无需下载
/// - `Equal`: 音质相同，无需下载
/// - `Less`: 在线音质更高，应下载替换
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityComparison {
    /// 本地更好
    LocalBetter,
    /// 相同
    Equal,
    /// 在线更好
    OnlineBetter,
}

pub fn compare_quality(local: AudioQuality, online: AudioQuality) -> QualityComparison {
    use std::cmp::Ordering;
    match local.cmp(&online) {
        Ordering::Greater => QualityComparison::LocalBetter,
        Ordering::Equal => QualityComparison::Equal,
        Ordering::Less => QualityComparison::OnlineBetter,
    }
}

/// 从 URL 推断文件扩展名。
pub fn ext_from_url(url: &str) -> &'static str {
    // 去掉 query string
    let path = url.split('?').next().unwrap_or(url);
    let lower = path.to_lowercase();
    if lower.ends_with(".flac") {
        "flac"
    } else if lower.ends_with(".mp3") {
        "mp3"
    } else if lower.ends_with(".m4a") {
        "m4a"
    } else if lower.ends_with(".wav") {
        "wav"
    } else if lower.ends_with(".ape") {
        "ape"
    } else if lower.ends_with(".aac") {
        "aac"
    } else {
        // 默认 mp3
        "mp3"
    }
}

/// 从 Content-Type 头推断扩展名。
pub fn ext_from_content_type(content_type: &str) -> Option<&'static str> {
    match content_type.to_lowercase().as_str() {
        s if s.contains("flac") => Some("flac"),
        s if s.contains("mpeg") || s.contains("mp3") => Some("mp3"),
        s if s.contains("mp4") || s.contains("m4a") || s.contains("aac") => Some("m4a"),
        s if s.contains("wav") => Some("wav"),
        s if s.contains("ape") => Some("ape"),
        _ => None,
    }
}

/// 将下载的字节数据写入 download_dir 下的文件。
///
/// # 参数
/// - `download_dir`: 完整下载目录路径
/// - `source_name`: 来源名（如 `"netease"`）
/// - `artist`: 主艺术家名
/// - `title`: 歌曲标题
/// - `ext`: 文件扩展名（不带 `.`）
/// - `bytes`: 音频文件字节
///
/// # 返回
/// 写入后的完整文件路径。
pub fn write_downloaded_file(
    download_dir: &std::path::Path,
    source_name: &str,
    artist: &str,
    title: &str,
    ext: &str,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    let relative = build_relative_path(source_name, artist, title, ext);
    let full_path = download_dir.join(&relative);

    // 确保父目录存在
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建下载目录失败 '{}': {}", parent.display(), e))?;
    }

    std::fs::write(&full_path, bytes)
        .map_err(|e| format!("写入下载文件失败 '{}': {}", full_path.display(), e))?;

    Ok(full_path)
}

/// 仅准备下载文件路径（创建父目录），不写入内容。
///
/// 用于流式下载场景：先调用此函数得到目标路径并确保父目录存在，
/// 再用 `WebHttpClient::download_to_file` 流式写入。
///
/// 相比 `write_downloaded_file`：避免把整个文件字节读入 `Vec<u8>` 再写盘。
pub fn prepare_download_path(
    download_dir: &std::path::Path,
    source_name: &str,
    artist: &str,
    title: &str,
    ext: &str,
) -> Result<PathBuf, String> {
    let relative = build_relative_path(source_name, artist, title, ext);
    let full_path = download_dir.join(&relative);

    // 确保父目录存在
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建下载目录失败 '{}': {}", parent.display(), e))?;
    }

    Ok(full_path)
}

/// 为已下载到本地的文件构造一个 local [`SourceId`]。
///
/// 用于在库中将歌曲的 source_ids 替换/追加一个 local 引用，
/// 使得后续播放直接从本地读取，无需再访问在线源。
pub fn build_local_source_id(file_path: &PathBuf) -> SourceId {
    let entity_id = platform::path_to_string(&PlatformPath::from(
        file_path.to_string_lossy().as_ref(),
    ));
    SourceId {
        source_name: "local".to_string(),
        source_type: SourceType::Local,
        entity_type: EntityType::Song,
        entity_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("hello/world"), "hello_world");
        assert_eq!(sanitize_filename("a:b*c?d"), "a_b_c_d");
        assert_eq!(sanitize_filename("normal name"), "normal name");
    }

    #[test]
    fn test_build_relative_path() {
        let p = build_relative_path("netease", "周杰伦", "晴天", "flac");
        assert_eq!(p, PathBuf::from("netease").join("周杰伦 - 晴天.flac"));
    }

    #[test]
    fn test_build_relative_path_empty_artist() {
        let p = build_relative_path("netease", "", "song", "mp3");
        assert_eq!(p, PathBuf::from("netease").join("Unknown - song.mp3"));
    }

    #[test]
    fn test_compare_quality() {
        use AudioQuality::*;
        assert_eq!(compare_quality(Lossless, Exhigh), QualityComparison::LocalBetter);
        assert_eq!(compare_quality(Exhigh, Exhigh), QualityComparison::Equal);
        assert_eq!(compare_quality(Standard, Lossless), QualityComparison::OnlineBetter);
    }

    #[test]
    fn test_ext_from_url() {
        assert_eq!(ext_from_url("https://example.com/song.flac?param=1"), "flac");
        assert_eq!(ext_from_url("https://example.com/song.mp3"), "mp3");
        assert_eq!(ext_from_url("https://example.com/song"), "mp3");
    }

    #[test]
    fn test_ext_from_content_type() {
        assert_eq!(ext_from_content_type("audio/flac"), Some("flac"));
        assert_eq!(ext_from_content_type("audio/mpeg"), Some("mp3"));
        assert_eq!(ext_from_content_type("audio/mp4"), Some("m4a"));
        assert_eq!(ext_from_content_type("application/octet-stream"), None);
    }
}
