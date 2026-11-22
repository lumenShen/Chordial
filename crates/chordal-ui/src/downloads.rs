//! 显式下载存储：与对象缓存池完全分离。
//!
//! 用户点“下载”后，音频写入 `download_dir`，并记录 `TrackId → 文件` 索引。
//! 之后播放同一 `TrackId` 直接走 `StreamHandle::File`，既不占对象缓存配额，
//! 也不再发网络请求。索引是 `downloads.json`，随目录一起移动。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

use chordal_core::model::{Track, TrackId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct DownloadIndex {
    /// `source\u{1}id` → 相对 download_dir 的路径。
    #[serde(default)]
    entries: HashMap<String, PathBuf>,
}

pub struct DownloadStore {
    root: PathBuf,
    index_path: PathBuf,
    entries: RwLock<HashMap<String, PathBuf>>,
}

impl DownloadStore {
    pub fn open(root: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&root);
        let index_path = root.join("downloads.json");
        let mut entries = std::fs::read(&index_path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<DownloadIndex>(&raw).ok())
            .map(|index| index.entries)
            .unwrap_or_default();
        // 文件被手动删除的条目自动清理。
        entries.retain(|_, rel| root.join(rel).is_file());
        Self {
            root,
            index_path,
            entries: RwLock::new(entries),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, id: &TrackId) -> Option<PathBuf> {
        self.entries
            .read()
            .ok()?
            .get(&entry_key(id))
            .map(|rel| self.root.join(rel))
            .filter(|path| path.is_file())
    }

    /// 以阻塞方式下载。`format_hint` 来自解析结果（如 `flac`/`mp3`），
    /// 优先于 URL 后缀，避免 CDN 用 `.mp3` 路径承载 flac。
    pub fn download_blocking(
        &self,
        track: &Track,
        url: &str,
        format_hint: Option<&str>,
    ) -> Result<PathBuf, String> {
        if let Some(path) = self.path_for(&track.id) {
            return Ok(path);
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(format!("不支持的下载地址：{url}"));
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("Chordial/2.0 (", env!("CARGO_PKG_VERSION"), ")"))
            .build()
            .map_err(|e| format!("创建下载客户端失败：{e}"))?;
        let mut response = client
            .get(url)
            .send()
            .map_err(|e| format!("下载请求失败：{e}"))?;
        if !response.status().is_success() {
            return Err(format!("下载失败：HTTP {}", response.status()));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let ext = audio_extension(url, &content_type, format_hint);
        let artist = track
            .artists
            .first()
            .map(|a| a.name.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("未知艺人");
        let album = track
            .album
            .as_ref()
            .map(|a| a.name.as_str())
            .filter(|s| !s.is_empty());
        let mut rel = PathBuf::from(sanitize_component(artist));
        if let Some(album) = album {
            rel.push(sanitize_component(album));
        }
        rel.push(format!(
            "{} [{}].{}",
            sanitize_component(&track.title),
            track.id.id,
            ext
        ));
        let dest = self.unique_path(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
        }
        let part = dest.with_extension(format!("{ext}.part"));
        {
            let mut file =
                std::fs::File::create(&part).map_err(|e| format!("创建文件失败：{e}"))?;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = response
                    .read(&mut buf)
                    .map_err(|e| format!("读取下载流失败：{e}"))?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])
                    .map_err(|e| format!("写入文件失败：{e}"))?;
            }
            file.flush().map_err(|e| format!("刷新文件失败：{e}"))?;
        }
        let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if size == 0 {
            let _ = std::fs::remove_file(&part);
            return Err("下载内容为空".into());
        }
        std::fs::rename(&part, &dest).map_err(|e| format!("保存文件失败：{e}"))?;

        if let Ok(mut entries) = self.entries.write() {
            let rel = dest
                .strip_prefix(&self.root)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| dest.clone());
            entries.insert(entry_key(&track.id), rel);
        }
        self.flush();
        tracing::info!(
            target: "chordial::download",
            track = %track.id,
            path = %dest.display(),
            size,
            "track downloaded"
        );
        Ok(dest)
    }

    fn unique_path(&self, rel: &Path) -> PathBuf {
        let candidate = self.root.join(rel);
        if !candidate.exists() {
            return candidate;
        }
        let stem = rel
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "track".into());
        let ext = rel
            .extension()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mp3".into());
        for n in 2..1000 {
            let next = rel.with_file_name(format!("{stem} ({n}).{ext}"));
            if !self.root.join(&next).exists() {
                return self.root.join(next);
            }
        }
        candidate
    }

    fn flush(&self) {
        let Ok(entries) = self.entries.read() else {
            return;
        };
        let index = DownloadIndex {
            entries: entries.clone(),
        };
        if let Ok(json) = serde_json::to_vec_pretty(&index) {
            let tmp = self.index_path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                if self.index_path.exists() {
                    let _ = std::fs::remove_file(&self.index_path);
                }
                let _ = std::fs::rename(&tmp, &self.index_path);
            }
        }
    }
}

fn entry_key(id: &TrackId) -> String {
    format!("{}\u{1}{}", id.source.as_str(), id.id)
}

fn sanitize_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(80));
    for ch in raw.chars() {
        if out.chars().count() >= 80 {
            break;
        }
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let trimmed = out.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "Unknown".into()
    } else {
        trimmed
    }
}

fn audio_extension(url: &str, content_type: &str, format_hint: Option<&str>) -> String {
    if let Some(hint) = format_hint {
        let hint = hint.to_ascii_lowercase();
        if matches!(
            hint.as_str(),
            "mp3" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wav" | "alac"
        ) {
            return if hint == "alac" { "m4a".into() } else { hint };
        }
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
        let ext = ext.to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "mp3" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wav"
        ) {
            return ext;
        }
    }
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("flac") {
        "flac".into()
    } else if ct.contains("mp4") || ct.contains("m4a") {
        "m4a".into()
    } else if ct.contains("mpeg") || ct.contains("mp3") {
        "mp3".into()
    } else if ct.contains("ogg") {
        "ogg".into()
    } else if ct.contains("wav") {
        "wav".into()
    } else if ct.contains("aac") {
        "aac".into()
    } else {
        "mp3".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chordal_core::model::{SourceId, TrackOrigin};

    #[test]
    fn sanitizes_components() {
        assert_eq!(sanitize_component("a/b:c*?"), "a_b_c__");
        assert_eq!(sanitize_component("  . "), "Unknown");
        assert!(sanitize_component(&"长".repeat(200)).chars().count() <= 80);
    }

    #[test]
    fn extension_detection() {
        assert_eq!(
            audio_extension("https://x/a.flac?token=1", "", None),
            "flac"
        );
        assert_eq!(
            audio_extension("https://x/stream", "audio/mpeg", None),
            "mp3"
        );
        assert_eq!(
            audio_extension("https://x/stream", "audio/flac", None),
            "flac"
        );
        // 解析结果的格式优先于 CDN 后缀。
        assert_eq!(audio_extension("https://x/a.mp3", "", Some("flac")), "flac");
        assert_eq!(audio_extension("https://x/a", "", Some("alac")), "m4a");
    }

    #[test]
    fn index_roundtrip_and_missing_file_cleanup() {
        let root = std::env::temp_dir().join(format!("chordial-downloads-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = DownloadStore::open(root.clone());
        let id = TrackId::new(SourceId::new("netease"), "42");
        let track = Track::new(
            id.clone(),
            "海阔天空",
            vec![chordal_core::model::ArtistRef {
                id: None,
                name: "Beyond".into(),
            }],
            Some(chordal_core::model::AlbumRef {
                id: None,
                name: "乐与怒".into(),
                cover: None,
            }),
            Some(326_000),
            TrackOrigin::Remote {
                source: SourceId::new("netease"),
            },
        );
        let rel = PathBuf::from("Beyond/乐与怒/海阔天空 [42].mp3");
        let abs = root.join(&rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::File::create(&abs)
            .unwrap()
            .write_all(b"audio")
            .unwrap();
        {
            let mut entries = store.entries.write().unwrap();
            entries.insert(entry_key(&id), rel.clone());
        }
        store.flush();
        drop(store);

        let store = DownloadStore::open(root.clone());
        assert_eq!(store.path_for(&id), Some(abs.clone()));
        let _ = track;
        std::fs::remove_file(&abs).unwrap();
        drop(store);
        let store = DownloadStore::open(root.clone());
        assert!(store.path_for(&id).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
