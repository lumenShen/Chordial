//! 网易云搜索 — 歌曲 / 歌手 / 专辑的在线搜索。
//!
//! 以自由函数形式提供搜索逻辑，由 [`NeteaseCloudMusicApiSource`] 的 trait 实现及
//! 专门的 Tauri 命令委托调用。

use super::api;
use super::entity_id::{build_entity_id, EntityKind};
use super::source::{NeteaseCloudMusicApiSource, NETEASE_SOURCE_NAME};
use super::time::publish_time_to_year;
use super::types::NeteaseSongDetail;
use crate::module::music_library::models::{Album, Artist, Song};
use crate::module::music_source::types::{EntityType, SourceId, SourceType};

/// 搜索歌曲。
pub(super) fn search_songs(
    source: &NeteaseCloudMusicApiSource,
    query: &str,
) -> Result<Vec<Song>, String> {
    if !source.is_enabled() {
        return Err("网易云来源未启用".to_string());
    }
    let briefs = api::search_songs(&source.client, query, 30, 0)?;
    // 转换为库内 Song 模型（简略信息→统一模型）
    let songs: Vec<Song> = briefs
        .iter()
        .map(|b| {
            let detail = NeteaseSongDetail {
                id: b.id,
                name: b.name.clone(),
                ar: b.artists.clone(),
                al: b.album.clone(),
                dt: b.duration,
                size: None,
            };
            source.build_song(&detail)
        })
        .collect();
    Ok(songs)
}

/// 搜索网易云歌手，返回库内 Artist 模型（附带 netease SourceId）。
pub(super) fn search_artists(
    source: &NeteaseCloudMusicApiSource,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<Artist>, String> {
    if !source.is_enabled() {
        return Err("网易云来源未启用".to_string());
    }
    let briefs = api::search_artists(&source.client, query, limit, offset)?;
    let artists = briefs
        .into_iter()
        .map(|b| Artist {
            id: format!("netease_artist_{}", b.id),
            name: b.name,
            bio: b.briefDesc,
            source_ids: vec![SourceId {
                source_name: source.name.clone(),
                source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
                entity_type: EntityType::Artist,
                entity_id: build_entity_id(EntityKind::artist, b.id),
            }],
        })
        .collect();
    Ok(artists)
}

/// 搜索网易云专辑，返回库内 Album 模型（附带 netease SourceId）。
pub(super) fn search_albums(
    source: &NeteaseCloudMusicApiSource,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<Album>, String> {
    if !source.is_enabled() {
        return Err("网易云来源未启用".to_string());
    }
    let briefs = api::search_albums(&source.client, query, limit, offset)?;
    let albums = briefs
        .into_iter()
        .map(|b| Album {
            id: format!("netease_album_{}", b.id),
            title: b.name,
            artist_id: b
                .artist
                .as_ref()
                .map(|a| format!("netease_artist_{}", a.id))
                .unwrap_or_default(),
            cover_url: b.picUrl,
            song_ids: Vec::new(),
            source_ids: vec![SourceId {
                source_name: source.name.clone(),
                source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
                entity_type: EntityType::Album,
                entity_id: build_entity_id(EntityKind::album, b.id),
            }],
            year: publish_time_to_year(b.publishTime),
        })
        .collect();
    Ok(albums)
}