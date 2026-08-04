//! 酷狗搜索 — 歌曲 / 歌手 / 专辑的在线搜索。
//!
//! 以自由函数形式提供搜索逻辑，由 [`KugouMusicApiSource`] 的 trait 实现委托调用。

use super::api;
use super::entity_id::{build_entity_id, EntityKind};
use super::source::{KugouMusicApiSource, KUGOU_SOURCE_NAME};
use crate::module::music_library::models::{Album, Artist, Song};
use crate::module::music_source::types::{EntityType, SourceId, SourceType};

/// 搜索歌曲。
pub(super) fn search_songs(source: &KugouMusicApiSource, query: &str) -> Result<Vec<Song>, String> {
    if !source.is_enabled() {
        return Err("酷狗来源未启用".to_string());
    }
    let briefs = api::search_songs(&source.client, query, 1, 30)?;
    Ok(briefs.iter().map(|b| source.build_song(b)).collect())
}

/// 搜索酷狗歌手，返回库内 Artist 模型。
pub(super) fn search_artists(
    source: &KugouMusicApiSource,
    query: &str,
    page: u32,
    pagesize: u32,
) -> Result<Vec<Artist>, String> {
    if !source.is_enabled() {
        return Err("酷狗来源未启用".to_string());
    }
    let briefs = api::search_artists(&source.client, query, page, pagesize)?;
    Ok(briefs
        .into_iter()
        .map(|b| {
            let singer_id = b.singer_id();
            Artist {
                id: if singer_id > 0 {
                    format!("kugou_artist_{}", singer_id)
                } else {
                    String::new()
                },
                name: b.singer_name().to_string(),
                bio: None,
                source_ids: vec![SourceId {
                    source_name: source.name.clone(),
                    source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
                    entity_type: EntityType::Artist,
                    entity_id: build_entity_id(EntityKind::artist, &singer_id.to_string()),
                }],
            }
        })
        .collect())
}

/// 搜索酷狗专辑，返回库内 Album 模型。
pub(super) fn search_albums(
    source: &KugouMusicApiSource,
    query: &str,
    page: u32,
    pagesize: u32,
) -> Result<Vec<Album>, String> {
    if !source.is_enabled() {
        return Err("酷狗来源未启用".to_string());
    }
    let briefs = api::search_albums(&source.client, query, page, pagesize)?;
    Ok(briefs
        .into_iter()
        .map(|b| {
            let album_id_str = b.album_id().to_string();
            let singer_id = b.SingerId;
            Album {
                id: if !album_id_str.is_empty() && album_id_str != "0" {
                    format!("kugou_album_{}", album_id_str)
                } else {
                    String::new()
                },
                title: b.album_name().to_string(),
                artist_id: if singer_id > 0 {
                    format!("kugou_artist_{}", singer_id)
                } else {
                    String::new()
                },
                cover_url: {
                    let img = b.image_url();
                    if img.is_empty() {
                        None
                    } else {
                        Some(img.to_string())
                    }
                },
                song_ids: Vec::new(),
                source_ids: vec![SourceId {
                    source_name: source.name.clone(),
                    source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
                    entity_type: EntityType::Album,
                    entity_id: build_entity_id(EntityKind::album, &album_id_str),
                }],
                year: None,
            }
        })
        .collect())
}