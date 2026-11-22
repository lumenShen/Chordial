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

use chordal_core::model::{Lyrics, SourceId, Track, TrackId};
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, Tag};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct DownloadIndex {
    /// `source\u{1}id` → 相对 download_dir 的路径。
    #[serde(default)]
    entries: HashMap<String, PathBuf>,
}

/// 下载时顺带写回文件的附加元数据。
///
/// 音频文件本身只承载流数据；标题/艺人/专辑等字段必须显式写入标签，
/// 否则本地库扫描只能从文件名兜底（标题带 `[id]`、艺人未知）。
#[derive(Debug, Clone, Default)]
pub struct DownloadExtras {
    /// 封面原始字节（JPEG/PNG/WebP…），同时嵌入标签和写边车文件。
    pub cover: Option<Vec<u8>>,
    /// 已渲染为通用 LRC 的歌词文本，写入边车 `.lrc`。
    pub lyrics: Option<String>,
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

    /// 给一个已存在的下载文件补写元数据（下载完成后 / 旧版本下载修复）。
    pub fn enrich_file_blocking(
        &self,
        path: &Path,
        track: &Track,
        extras: &DownloadExtras,
    ) -> Result<(), String> {
        if !path.is_file() {
            return Err(format!("文件不存在：{}", path.display()));
        }
        write_embedded_metadata(path, track, extras)?;
        write_sidecars(path, extras)
    }

    /// 列出“文件在、但标题或艺人标签为空”的旧下载。
    ///
    /// 旧版本只把音频流写盘，没有标签；启动后按 `TrackId` 重新取详情即可
    /// 原地补齐，无需重新下载。
    pub fn entries_missing_metadata(&self) -> Vec<(TrackId, PathBuf)> {
        // 先快照索引再读文件：标签解析是磁盘 IO，不应一直占着索引读锁。
        let entries: Vec<(String, PathBuf)> = {
            let Ok(entries) = self.entries.read() else {
                return Vec::new();
            };
            entries
                .iter()
                .map(|(key, rel)| (key.clone(), self.root.join(rel)))
                .collect()
        };
        entries
            .into_iter()
            .filter_map(|(key, path)| {
                if !path.is_file() || has_track_metadata(&path) {
                    return None;
                }
                Some((track_id_from_entry_key(&key)?, path))
            })
            .collect()
    }

    /// 以阻塞方式下载。`format_hint` 来自解析结果（如 `flac`/`mp3`），
    /// 优先于 URL 后缀，避免 CDN 用 `.mp3` 路径承载 flac。
    ///
    /// 音频落盘后会立即写入标签（标题/艺人/专辑/序号/年份/歌词/封面），
    /// 并写同名 `.lrc` / `.cover.<ext>` 边车文件；这样文件被本地库扫描时
    /// 仍携带完整元数据，而不是只剩一个带 id 的文件名。
    pub fn download_blocking(
        &self,
        track: &Track,
        url: &str,
        format_hint: Option<&str>,
        extras: &DownloadExtras,
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

        // 标签/边车写入失败不应让整次下载失败：音频已经完整落盘，
        // 可以正常播放，只是本地库会退回文件名兜底。
        if let Err(error) = self.enrich_file_blocking(&dest, track, extras) {
            tracing::warn!(
                target: "chordial::download",
                track = %track.id,
                path = %dest.display(),
                %error,
                "failed to write downloaded track metadata"
            );
        }

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

fn track_id_from_entry_key(key: &str) -> Option<TrackId> {
    let (source, id) = key.split_once('\u{1}')?;
    Some(TrackId::new(SourceId::new(source), id))
}

/// 标题 + 艺人齐全才认为标签可用；专辑名允许缺失（单曲下载常见）。
fn has_track_metadata(path: &Path) -> bool {
    let Ok(tagged) = lofty::read_from_path(path) else {
        return false;
    };
    let Some(tag) = tagged.primary_tag() else {
        return false;
    };
    tag.title().is_some_and(|value| !value.trim().is_empty())
        && tag.artist().is_some_and(|value| !value.trim().is_empty())
}

/// 把领域曲目写进音频文件自身的标签。
///
/// `lofty` 同时覆盖 ID3v2（mp3/aac/wav）、Vorbis Comments（flac/ogg/opus）
/// 与 MP4 ilst（m4a/alac），因此调用方不需要按扩展名分支。
fn write_embedded_metadata(
    path: &Path,
    track: &Track,
    extras: &DownloadExtras,
) -> Result<(), String> {
    let mut tagged =
        lofty::read_from_path(path).map_err(|error| format!("读取音频标签失败：{error}"))?;
    // 优先写入该格式的原生标签（例如 mp3 即使只有 ID3v1 也补一个 ID3v2），
    // 否则封面/歌词这类字段会因目标标签能力不足而被静默丢弃。
    let tag_type = tagged.file_type().primary_tag_type();
    if tagged.tag(tag_type).is_none() {
        tagged.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged
        .tag_mut(tag_type)
        .ok_or_else(|| "音频格式不支持写入标签".to_string())?;

    if !track.title.trim().is_empty() {
        tag.set_title(track.title.clone());
    }
    let artists: Vec<&str> = track
        .artists
        .iter()
        .map(|artist| artist.name.trim())
        .filter(|name| !name.is_empty())
        .collect();
    if !artists.is_empty() {
        // 本地库按 `/` 拆分多位艺人，与常见 ID3 约定一致。
        tag.set_artist(artists.join("/"));
    }
    if let Some(album) = track
        .album
        .as_ref()
        .map(|album| album.name.trim())
        .filter(|name| !name.is_empty())
    {
        tag.set_album(album.to_string());
    }
    if let Some(track_no) = track.track_no.filter(|no| *no > 0) {
        tag.set_track(track_no);
    }
    if let Some(year) = track.year.filter(|year| *year > 0) {
        tag.insert_text(ItemKey::RecordingDate, year.to_string());
    }
    if let Some(lyrics) = extras
        .lyrics
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        // USLT/©lyr 的语义是纯文本歌词；时间戳只保留在 `.lrc` 边车里。
        let plain = lrc_to_plain(lyrics);
        if !plain.is_empty() {
            tag.insert_text(ItemKey::UnsyncLyrics, plain);
        }
    }
    if let Some(cover) = extras.cover.as_deref().filter(|bytes| !bytes.is_empty())
        && tag.pictures().is_empty()
    {
        // 部分 CDN 文件已经内嵌封面（只是缺文本标签）；不要重复追加。
        tag.push_picture(
            Picture::unchecked(cover.to_vec())
                .pic_type(PictureType::CoverFront)
                .mime_type(picture_mime(cover))
                .description("Cover")
                .build(),
        );
    }

    tagged
        .save_to_path(path, WriteOptions::default())
        .map_err(|error| format!("写入音频标签失败：{error}"))
}

/// 写同名边车文件。本地库扫描会优先读取 `.lrc` 与 `<stem>.cover.*`，
/// 即使文件被拷贝到不支持标签的工具/设备，元数据也还在目录里。
fn write_sidecars(path: &Path, extras: &DownloadExtras) -> Result<(), String> {
    if let Some(cover) = extras.cover.as_deref().filter(|bytes| !bytes.is_empty()) {
        let sidecar = path.with_extension(format!("cover.{}", image_extension(cover)));
        std::fs::write(&sidecar, cover).map_err(|error| format!("写入封面边车失败：{error}"))?;
    }
    if let Some(lyrics) = extras
        .lyrics
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let sidecar = path.with_extension("lrc");
        std::fs::write(&sidecar, lyrics).map_err(|error| format!("写入歌词边车失败：{error}"))?;
    }
    Ok(())
}

/// 领域歌词 → 通用 LRC。优先使用已解析的行级时间戳；文本本身可能是
/// YRC（逐词），直接存 `Lyrics::raw` 会让本地 LRC 解析器读不到。
pub fn lyrics_to_lrc(lyrics: &Lyrics) -> Option<String> {
    let mut out = String::new();
    if !lyrics.lines.is_empty() {
        for line in &lyrics.lines {
            push_lrc_line(&mut out, line.time_ms, &line.text);
        }
    } else if !lyrics.timed_lines.is_empty() {
        for line in &lyrics.timed_lines {
            push_lrc_line(&mut out, line.start_ms, &line.text());
        }
    } else if !lyrics.raw.trim().is_empty() {
        out.push_str(lyrics.raw.trim_start_matches('\u{feff}'));
        out.push('\n');
    }
    (!out.trim().is_empty()).then_some(out)
}

fn push_lrc_line(out: &mut String, time_ms: u64, text: &str) {
    let minutes = time_ms / 60_000;
    let seconds = (time_ms / 1_000) % 60;
    let centis = (time_ms % 1_000) / 10;
    out.push_str(&format!(
        "[{minutes:02}:{seconds:02}.{centis:02}]{}\n",
        text.trim_end()
    ));
}

/// 去掉 LRC 时间/元信息标签，得到适合写进 `USLT`/`©lyr` 的纯文本。
fn lrc_to_plain(lrc: &str) -> String {
    let mut out = String::new();
    for line in lrc.lines() {
        let mut rest = line.trim();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(close) = stripped.find(']') else {
                break;
            };
            rest = stripped[close + 1..].trim_start();
        }
        if rest.is_empty() {
            continue;
        }
        out.push_str(rest);
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn picture_mime(bytes: &[u8]) -> MimeType {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        MimeType::Jpeg
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        MimeType::Png
    } else if bytes.starts_with(b"GIF8") {
        MimeType::Gif
    } else if bytes.starts_with(b"BM") {
        MimeType::Bmp
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        MimeType::Unknown("image/webp".into())
    } else {
        MimeType::Unknown("application/octet-stream".into())
    }
}

fn image_extension(bytes: &[u8]) -> &'static str {
    match picture_mime(bytes) {
        MimeType::Png => "png",
        MimeType::Gif => "gif",
        MimeType::Bmp => "bmp",
        MimeType::Unknown(kind) if kind == "image/webp" => "webp",
        _ => "jpg",
    }
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

    #[test]
    fn renders_lrc_from_parsed_lines() {
        let lyrics = Lyrics {
            lines: vec![chordal_core::model::LyricLine {
                time_ms: 62_530,
                text: "你好".into(),
            }],
            // YRC 原文不能被本地 LRC 解析器直接使用，必须重新渲染。
            raw: "[62530,1000](62530,100,0)你(62630,100,0)好".into(),
            ..Default::default()
        };
        assert_eq!(lyrics_to_lrc(&lyrics).as_deref(), Some("[01:02.53]你好\n"));
        assert!(lyrics_to_lrc(&Lyrics::default()).is_none());
        assert_eq!(
            lrc_to_plain("[ar:歌手]\n[00:01.00][00:03.00]你好\n\n[00:04.00]世界\n"),
            "你好\n世界"
        );
    }

    /// 最小合法 WAV：lofty 能识别格式并写入 ID3v2 标签。
    fn minimal_wav() -> Vec<u8> {
        let samples: [i16; 8] = [0; 8];
        let data_len = (samples.len() * 2) as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&44_100u32.to_le_bytes());
        wav.extend_from_slice(&176_400u32.to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        wav
    }

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x05, 0x02, 0x00, 0x5E, 0x1D, 0x1D, 0x89, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn writes_tags_cover_and_lyrics_sidecars() {
        let root = std::env::temp_dir().join(format!("chordial-enrich-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
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
        let path = root.join("海阔天空 [42].wav");
        std::fs::write(&path, minimal_wav()).unwrap();

        let store = DownloadStore::open(root.clone());
        {
            let mut entries = store.entries.write().unwrap();
            entries.insert(entry_key(&id), PathBuf::from("海阔天空 [42].wav"));
        }
        assert_eq!(store.entries_missing_metadata().len(), 1);

        let extras = DownloadExtras {
            cover: Some(PNG_1X1.to_vec()),
            lyrics: Some("[00:01.00]hello\n".into()),
        };
        store.enrich_file_blocking(&path, &track, &extras).unwrap();

        let tagged = lofty::read_from_path(&path).unwrap();
        let tag = tagged.primary_tag().expect("written tag");
        assert_eq!(tag.title().as_deref(), Some("海阔天空"));
        assert_eq!(tag.artist().as_deref(), Some("Beyond"));
        assert_eq!(tag.album().as_deref(), Some("乐与怒"));
        assert_eq!(tag.get_string(ItemKey::UnsyncLyrics), Some("hello"));
        assert_eq!(tag.pictures().len(), 1);
        assert!(path.with_extension("lrc").is_file());
        assert!(path.with_extension("cover.png").is_file());
        assert!(store.entries_missing_metadata().is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 0.2s 静音 MP3（64kbps），只用于验证标签写入 + 本地库读回。
    const TINY_MP3: &[u8] = include_bytes!("../tests/fixtures/tiny.mp3");

    #[test]
    fn enriched_file_is_scanned_back_with_metadata() {
        let root = std::env::temp_dir().join(format!("chordial-scanback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("终于见到了你 [2608436149].mp3");
        std::fs::write(&path, TINY_MP3).unwrap();

        let track = Track::new(
            TrackId::new(SourceId::new("netease"), "2608436149"),
            "终于见到了你",
            vec![chordal_core::model::ArtistRef {
                id: None,
                name: "吴琳珂Moske".into(),
            }],
            Some(chordal_core::model::AlbumRef {
                id: None,
                name: "终于见到了你".into(),
                cover: None,
            }),
            Some(182_000),
            TrackOrigin::Remote {
                source: SourceId::new("netease"),
            },
        );
        let extras = DownloadExtras {
            cover: Some(PNG_1X1.to_vec()),
            lyrics: Some("[00:01.00]你好\n".into()),
        };
        write_embedded_metadata(&path, &track, &extras).unwrap();
        write_sidecars(&path, &extras).unwrap();

        // 模拟本地库扫描：标题/艺人/专辑来自文件标签，封面来自单曲边车。
        let source = chordal_local::LocalSource::new(
            SourceId::new("local-test"),
            chordal_local::LocalSourceConfig {
                roots: vec![root.clone()],
            },
            "test",
        );
        source.scan_blocking();
        let tracks = source.all_tracks();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "终于见到了你");
        assert_eq!(tracks[0].primary_artist(), "吴琳珂Moske");
        assert_eq!(
            tracks[0].album.as_ref().map(|album| album.name.as_str()),
            Some("终于见到了你")
        );
        let cover = tracks[0]
            .album
            .as_ref()
            .and_then(|album| album.cover.as_deref());
        assert!(cover.is_some_and(|url| url.starts_with("file://") && url.ends_with(".cover.png")));

        let _ = std::fs::remove_dir_all(&root);
    }
}
