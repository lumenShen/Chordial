//! 本地源：scan（由 lib.rs 拆分，行为不变）。

use super::*;

pub(super) fn file_fingerprint(path: &Path) -> Option<(u64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((mtime, md.len()))
}

/// 单曲封面边车：下载器写入的 `<stem>.cover.jpg/png/...`。
/// 同目录多张专辑/多首单曲时，目录级 cover.jpg 可能张冠李戴。
pub(super) fn find_track_cover(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_string_lossy();
    for ext in ["jpg", "jpeg", "png", "webp", "gif", "bmp"] {
        let candidate = dir.join(format!("{stem}.cover.{ext}"));
        if candidate.is_file() {
            return Some(format!("file://{}", candidate.to_string_lossy()));
        }
    }
    None
}

/// 同目录封面探测：常见命名，返回 `file://` URL。
pub(super) fn find_folder_cover(dir: &Path) -> Option<String> {
    for name in [
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "folder.jpg",
        "folder.png",
        "front.jpg",
        "front.png",
    ] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(format!("file://{}", candidate.to_string_lossy()));
        }
    }
    None
}

/// 从扩展名 + 文件大小/时长估算音质。
///
/// 无损格式按 tier=Lossless；有损格式用 `size*8/duration_ms` 估算平均码率
/// （VBR 只是近似，但足以和在线 128/192/320 档比较）。时长未知时保留
/// `Unknown`，比较时不会被误判为高音质。
pub(super) fn audio_quality_for(path: &Path, duration_ms: Option<u64>) -> Option<AudioQuality> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("?")
        .to_ascii_lowercase();
    let lossless = matches!(ext.as_str(), "flac" | "wav" | "aiff" | "aif" | "alac");
    let bitrate_kbps = duration_ms.filter(|d| *d > 0).and_then(|duration| {
        let size = std::fs::metadata(path).ok()?.len();
        if size == 0 {
            return None;
        }
        let kbps = (size as u128 * 8) / duration as u128;
        Some(kbps.min(20_000) as u32).filter(|kbps| *kbps > 0)
    });
    let tier = if lossless {
        QualityTier::Lossless
    } else {
        match bitrate_kbps {
            Some(kbps) if kbps >= 256 => QualityTier::High,
            Some(kbps) if kbps >= 96 => QualityTier::Standard,
            _ => QualityTier::Unknown,
        }
    };
    Some(AudioQuality {
        tier,
        codec: Some(ext),
        bitrate_kbps,
        sample_rate_hz: None,
        lossless,
    })
}

pub(super) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 本地专辑的稳定合成 ID：专辑名 + 所在目录名（同目录一专辑）。
pub(super) fn local_album_id(name: &str, path: &Path) -> String {
    let dir = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let key = format!(
        "{}|{}",
        chordal_core::model::normalize(name),
        chordal_core::model::normalize(&dir)
    );
    format!("al-{:016x}", fnv1a64(key.as_bytes()))
}

pub(super) fn local_artist_id(name: &str) -> String {
    format!(
        "ar-{:016x}",
        fnv1a64(chordal_core::model::normalize(name).as_bytes())
    )
}

pub(super) fn build_collections(
    tracks: &[Track],
) -> (
    HashMap<String, LocalAlbumAgg>,
    HashMap<String, LocalArtistAgg>,
) {
    let mut albums: HashMap<String, LocalAlbumAgg> = HashMap::new();
    let mut artists: HashMap<String, LocalArtistAgg> = HashMap::new();
    for (idx, track) in tracks.iter().enumerate() {
        if let Some(album) = &track.album
            && let Some(album_id) = &album.id
        {
            let agg = albums
                .entry(album_id.clone())
                .or_insert_with(|| LocalAlbumAgg {
                    title: album.name.clone(),
                    creator: track.artists.first().map(|a| a.name.clone()),
                    cover: album.cover.clone(),
                    year: track.year,
                    search_key: String::new(),
                    tracks: Vec::new(),
                });
            if agg.cover.is_none() {
                agg.cover = album.cover.clone();
            }
            if agg.year.is_none() {
                agg.year = track.year;
            }
            agg.tracks.push(idx as u32);
        }
        for artist in &track.artists {
            let Some(artist_id) = &artist.id else {
                continue;
            };
            let agg = artists
                .entry(artist_id.clone())
                .or_insert_with(|| LocalArtistAgg {
                    name: artist.name.clone(),
                    cover: track.album.as_ref().and_then(|a| a.cover.clone()),
                    search_key: String::new(),
                    album_ids: Vec::new(),
                    tracks: Vec::new(),
                });
            agg.tracks.push(idx as u32);
            if let Some(album) = &track.album
                && let Some(album_id) = &album.id
                && !agg.album_ids.contains(album_id)
            {
                agg.album_ids.push(album_id.clone());
            }
        }
    }
    for agg in albums.values_mut() {
        agg.tracks.sort_by(|&a, &b| {
            let ta = &tracks[a as usize];
            let tb = &tracks[b as usize];
            ta.track_no
                .unwrap_or(u32::MAX)
                .cmp(&tb.track_no.unwrap_or(u32::MAX))
                .then_with(|| ta.title.cmp(&tb.title))
        });
        agg.search_key = format!(
            "{}|{}",
            chordal_core::model::normalize(&agg.title),
            chordal_core::model::normalize(agg.creator.as_deref().unwrap_or(""))
        );
    }
    for agg in artists.values_mut() {
        agg.tracks.sort_by(|&a, &b| {
            let ta = &tracks[a as usize];
            let tb = &tracks[b as usize];
            tb.year
                .unwrap_or(0)
                .cmp(&ta.year.unwrap_or(0))
                .then_with(|| {
                    ta.track_no
                        .unwrap_or(u32::MAX)
                        .cmp(&tb.track_no.unwrap_or(u32::MAX))
                })
                .then_with(|| ta.title.cmp(&tb.title))
        });
        agg.search_key = chordal_core::model::normalize(&agg.name);
    }
    (albums, artists)
}

pub(super) fn build_track(source: &SourceId, id: &str, path: &Path, meta: Option<TrackMeta>) -> Track {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (fallback_title, fallback_artist) = match stem.as_str().split_once(" - ") {
        Some((a, b)) => (b.to_string(), Some(a.to_string())),
        None => (stem.clone(), None),
    };
    let meta = meta.unwrap_or_default();
    let title = meta
        .title
        .filter(|t| !t.is_empty())
        .unwrap_or(fallback_title);
    let mut artists: Vec<ArtistRef> = meta
        .artists
        .iter()
        .filter(|a| !a.is_empty())
        .map(|a| ArtistRef {
            id: Some(local_artist_id(a)),
            name: a.clone(),
        })
        .collect();
    if artists.is_empty()
        && let Some(a) = fallback_artist
    {
        let id = local_artist_id(&a);
        artists.push(ArtistRef {
            id: Some(id),
            name: a,
        });
    }
    let album = meta.album.filter(|a| !a.is_empty()).map(|name| AlbumRef {
        id: Some(local_album_id(&name, path)),
        name,
        cover: None,
    });
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("?")
        .to_ascii_lowercase();
    let mut track = Track::new(
        TrackId::new(source.clone(), id.to_string()),
        title,
        artists,
        album,
        meta.duration_ms,
        TrackOrigin::Local {
            path: path.to_path_buf(),
        },
    );
    track.track_no = meta.track_no;
    track.year = meta.year;
    track.quality_info = audio_quality_for(path, meta.duration_ms);
    track.quality = Some(
        track
            .quality_info
            .as_ref()
            .map(AudioQuality::label)
            .unwrap_or_else(|| ext.to_ascii_uppercase()),
    );
    // 文件名也纳入模糊搜索键（用户常按文件名里的专辑/版本词搜索，
    // 而标签可能已被规范化）。单次 append，扫描期完成，查询期零成本。
    if !stem.is_empty() {
        track.search_key.push('\u{1}');
        track
            .search_key
            .push_str(&chordal_core::model::normalize(&stem));
    }
    track
}
