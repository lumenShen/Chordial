//! 网易云歌词 — 歌词获取（YRC 逐字优先，退化为 LRC）。
//!
//! 以自由函数形式提供歌词逻辑，由 [`NeteaseCloudMusicApiSource`] 的 trait 实现委托调用。

use super::api;
use super::entity_id::{build_lyric_entity_id, parse_remote_id};
use super::source::{NeteaseCloudMusicApiSource, NETEASE_SOURCE_NAME};
use crate::module::music_library::models::Lyric;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};

/// 获取歌词（元数据，含 `text` 字段）。
pub(super) fn get_lyric(
    source: &NeteaseCloudMusicApiSource,
    song_id: &str,
) -> Result<Option<Lyric>, String> {
    if !source.is_enabled() {
        return Err("网易云来源未启用".to_string());
    }
    // song_id 可能是库内 UUID 或 entity_id；尝试解析 entity_id
    let remote_id = parse_remote_id(song_id).unwrap_or_else(|| {
        // 如果不是 entity_id 格式，尝试直接 parse
        song_id.parse::<i64>().unwrap_or(0)
    });
    if remote_id == 0 {
        return Ok(None);
    }

    let resp = api::get_lyric(&source.client, remote_id)?;
    // 优先 YRC（逐字），其次 LRC（行级）
    let (text, format) = if let Some(yrc) = resp.yrc {
        if !yrc.lyric.is_empty() {
            (yrc.lyric, "yrc")
        } else if let Some(lrc) = resp.lrc {
            if !lrc.lyric.is_empty() {
                (lrc.lyric, "lrc")
            } else {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
    } else if let Some(lrc) = resp.lrc {
        if lrc.lyric.is_empty() {
            return Ok(None);
        }
        (lrc.lyric, "lrc")
    } else {
        return Ok(None);
    };

    // 如果有翻译，附加到歌词末尾（简易合并方案）
    // 完整的翻译合并由前端 lyricConverter.js 处理
    let final_text = if let Some(tlyric) = resp.tlyric {
        if !tlyric.lyric.is_empty() && format == "lrc" {
            format!("{}\n[translations]\n{}", text, tlyric.lyric)
        } else {
            text
        }
    } else {
        text
    };

    let entity_id = build_lyric_entity_id(remote_id, format);
    Ok(Some(Lyric {
        id: format!("netease_lyric_{}_{}", remote_id, format),
        song_id: song_id.to_string(),
        text: final_text,
        source_id: SourceId {
            source_name: source.name.clone(),
            source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
            entity_type: EntityType::Lyric,
            entity_id,
        },
    }))
}

/// 获取歌词文本（原始文本）。
pub(super) fn lyric_text_get(
    source: &NeteaseCloudMusicApiSource,
    song_id: &str,
) -> Result<String, String> {
    let lyric = get_lyric(source, song_id)?.ok_or_else(|| "未找到歌词".to_string())?;
    Ok(lyric.text)
}