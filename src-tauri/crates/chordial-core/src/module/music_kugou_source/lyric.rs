//! 酷狗歌词 — 歌词搜索与下载。
//!
//! 以自由函数形式提供歌词逻辑，由 [`KugouMusicApiSource`] 的 trait 实现委托调用。

use super::api;
use super::entity_id::{build_lyric_entity_id, parse_remote_id};
use super::source::{KugouMusicApiSource, KUGOU_SOURCE_NAME};
use crate::module::music_library::models::Lyric;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};

/// 获取歌词（元数据，含 `text` 字段）。
pub(super) fn get_lyric(
    source: &KugouMusicApiSource,
    song_id: &str,
) -> Result<Option<Lyric>, String> {
    if !source.is_enabled() {
        return Err("酷狗来源未启用".to_string());
    }
    // song_id 可能是 entity_id (song:<hash>) 或纯 hash
    let hash = if song_id.starts_with("song:") {
        parse_remote_id(song_id).unwrap_or_default()
    } else {
        song_id.to_string()
    };
    if hash.is_empty() {
        return Ok(None);
    }

    // 第一步：搜索歌词候选
    // duration=0 表示不指定时长过滤；若 candidate.duration > 0，下次可优化为传入真实时长
    let candidates = api::search_lyric(&source.client, &hash, 0)?;
    let candidate = candidates
        .into_iter()
        .next()
        .ok_or_else(|| "未找到歌词候选".to_string())?;

    // 第二步：下载歌词
    let lyric_resp = api::get_lyric(&source.client, &candidate.id, &candidate.accesskey)?;

    // 优先使用已解码的 decodeContent；若为空则尝试 base64 解码 content 字段
    let lyric_text = if !lyric_resp.decodeContent.is_empty() {
        lyric_resp.decodeContent
    } else if !lyric_resp.content.is_empty() {
        // 兜底：base64 解码 content 字段
        use base64::Engine;
        match base64::engine::general_purpose::STANDARD.decode(&lyric_resp.content) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "[kugou] 歌词 base64 解码后 UTF-8 转换失败 hash={} id={}: {}",
                        hash, candidate.id, e
                    );
                    return Ok(None);
                }
            },
            Err(e) => {
                eprintln!(
                    "[kugou] 歌词 base64 解码失败 hash={} id={}: {}",
                    hash, candidate.id, e
                );
                return Ok(None);
            }
        }
    } else {
        return Ok(None);
    };

    if lyric_text.is_empty() {
        return Ok(None);
    }

    // 歌词库内 ID：基于 candidate id 确定性，便于去重
    let entity_id = build_lyric_entity_id(&candidate.id, &candidate.accesskey);
    Ok(Some(Lyric {
        id: format!("kugou_lyric_{}", candidate.id),
        song_id: song_id.to_string(),
        text: lyric_text,
        source_id: SourceId {
            source_name: source.name.clone(),
            source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
            entity_type: EntityType::Lyric,
            entity_id,
        },
    }))
}

/// 获取歌词文本（原始文本）。
pub(super) fn lyric_text_get(
    source: &KugouMusicApiSource,
    song_id: &str,
) -> Result<String, String> {
    let lyric = get_lyric(source, song_id)?.ok_or_else(|| "未找到歌词".to_string())?;
    Ok(lyric.text)
}