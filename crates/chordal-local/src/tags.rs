//! 本地源：tags（由 lib.rs 拆分，行为不变）。

use super::*;

pub(super) fn decode_lyric_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let (text, _, _) = encoding_rs::UTF_16LE.decode(rest);
        return text.into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let (text, _, _) = encoding_rs::UTF_16BE.decode(rest);
        return text.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    }
}

/// symphonia 标签读取（真实实现）。
pub struct SymphoniaTagReader;

impl TagReader for SymphoniaTagReader {
    fn read(&self, path: &Path) -> Option<TrackMeta> {
        // symphonia 对部分 MP3 的 ID3v2 不可见，补一个轻量 ID3v2 解析器。
        let mut meta = read_tags_symphonia(path).unwrap_or_default();
        read_id3_into(path, &mut meta);
        if meta.title.is_none()
            && meta.artists.is_empty()
            && meta.album.is_none()
            && meta.duration_ms.is_none()
        {
            None
        } else {
            Some(meta)
        }
    }
}

/// 轻量 ID3v2.3/2.4 文本标签解析（标题/艺人/专辑/音轨/年份）。
pub(super) fn read_id3_into(path: &Path, meta: &mut TrackMeta) {
    use std::io::Read;

    let Ok(mut file) = std::fs::File::open(path) else {
        return;
    };
    let mut header = [0u8; 10];
    if file.read_exact(&mut header).is_err() || &header[..3] != b"ID3" {
        return;
    }
    let major = header[3];
    if !(3..=4).contains(&major) {
        return;
    }
    let size = synchsafe(&header[6..10]) as usize;
    if size == 0 || size > 16 * 1024 * 1024 {
        return;
    }
    let mut data = vec![0u8; size];
    if file.read_exact(&mut data).is_err() {
        return;
    }

    let mut pos = 0usize;
    if header[5] & 0x40 != 0 {
        // Extended header：v2.3 是 4 字节大小，v2.4 是 synchsafe。
        if pos + 4 > data.len() {
            return;
        }
        let ext = if major == 4 {
            synchsafe(&data[pos..pos + 4]) as usize
        } else {
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize
        };
        pos += ext.max(4);
    }

    while pos + 10 <= data.len() {
        let id = &data[pos..pos + 4];
        if id[0] == 0 {
            break;
        }
        let frame_size = if major == 4 {
            synchsafe(&data[pos + 4..pos + 8]) as usize
        } else {
            u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize
        };
        if frame_size == 0 || pos + 10 + frame_size > data.len() {
            break;
        }
        if id[0] == b'T' {
            let payload = &data[pos + 10..pos + 10 + frame_size];
            let text = decode_id3_text(payload);
            match id {
                b"TIT2" if meta.title.is_none() => meta.title = Some(text),
                b"TPE1" if meta.artists.is_empty() => {
                    meta.artists = text
                        .split(['/', ';'])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect();
                }
                b"TALB" if meta.album.is_none() => meta.album = Some(text),
                b"TRCK" if meta.track_no.is_none() => {
                    meta.track_no = text
                        .split('/')
                        .next()
                        .and_then(|s| s.trim().parse::<u32>().ok())
                }
                b"TDRC" | b"TYER" if meta.year.is_none() => {
                    meta.year = text.get(..4).and_then(|s| s.parse::<u32>().ok())
                }
                _ => {}
            }
        }
        pos += 10 + frame_size;
    }
}

pub(super) fn synchsafe(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .take(4)
        .fold(0u32, |acc, b| (acc << 7) | u32::from(b & 0x7f))
}

pub(super) fn decode_id3_text(payload: &[u8]) -> String {
    let Some((&encoding, body)) = payload.split_first() else {
        return String::new();
    };
    let text = match encoding {
        0 => body.iter().map(|&b| b as char).collect::<String>(),
        1 => {
            if body.len() < 2 {
                return String::new();
            }
            let (le, body) = if body.starts_with(&[0xFF, 0xFE]) {
                (true, &body[2..])
            } else if body.starts_with(&[0xFE, 0xFF]) {
                (false, &body[2..])
            } else {
                (true, body)
            };
            let units = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if le {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .take_while(|&u| u != 0)
                .collect::<Vec<_>>();
            String::from_utf16_lossy(&units)
        }
        2 => {
            let units = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect::<Vec<_>>();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(body).to_string(),
    };
    text.trim_matches(['\0', ' ']).to_string()
}

/// symphonia `meta::Value` → 字符串（0.5.4 无 to_str 访问器，按变体匹配）。
pub(super) fn value_str(v: &symphonia::core::meta::Value) -> Option<&str> {
    if let symphonia::core::meta::Value::String(s) = v {
        Some(s)
    } else {
        None
    }
}

pub(super) fn value_uint(v: &symphonia::core::meta::Value) -> Option<u64> {
    if let symphonia::core::meta::Value::UnsignedInt(i) = v {
        Some(*i)
    } else {
        None
    }
}

pub(super) fn read_tags_symphonia(path: &Path) -> Option<TrackMeta> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::{MetadataOptions, StandardTagKey};
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut reader = probed.format;

    let mut meta = TrackMeta::default();
    // MP3 的 ID3v2 常作为后续 metadata revision 出现；先读一个 packet 再跳到最新。
    let _ = reader.next_packet();
    if let Some(md) = reader.metadata().skip_to_latest() {
        for tag in md.tags() {
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) => {
                    meta.title = value_str(&tag.value).map(str::to_string)
                }
                Some(StandardTagKey::Artist) => {
                    if let Some(v) = value_str(&tag.value) {
                        meta.artists.push(v.to_string());
                    }
                }
                Some(StandardTagKey::AlbumArtist) => {
                    if let Some(v) = value_str(&tag.value) {
                        meta.artists.push(v.to_string());
                    }
                }
                Some(StandardTagKey::Album) => {
                    meta.album = value_str(&tag.value).map(str::to_string)
                }
                Some(StandardTagKey::TrackNumber) => {
                    meta.track_no = value_uint(&tag.value).map(|i| i as u32)
                }
                Some(StandardTagKey::Date) => {
                    meta.year = value_str(&tag.value)
                        .and_then(|s| s.get(0..4))
                        .and_then(|s| s.parse().ok());
                }
                _ => {}
            }
        }
    }
    // 容器时长：n_frames × time_base（symphonia 0.5.4 的 trait 上没有
    // duration()/time_base()，信息在 CodecParameters 里）。
    if let Some(track) = reader
        .tracks()
        .iter()
        .find(|t| t.codec_params.sample_rate.is_some())
    {
        let params = &track.codec_params;
        if let (Some(tb), Some(frames)) = (params.time_base, params.n_frames) {
            let t = tb.calc_time(frames);
            let secs = t.seconds as f64 + t.frac;
            if secs > 0.0 {
                meta.duration_ms = Some((secs * 1000.0) as u64);
            }
        }
    }
    meta.artists.dedup();
    Some(meta)
}
