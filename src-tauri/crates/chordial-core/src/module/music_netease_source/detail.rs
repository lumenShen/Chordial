//! 网易云详情 — 单曲 / 歌手 / 专辑详情获取。
//!
//! 以自由函数形式提供详情逻辑，由 [`NeteaseCloudMusicApiSource`] 的 trait 实现委托调用。

use super::api;
use super::entity_id::{build_entity_id, parse_remote_id, EntityKind};
use super::source::{NeteaseCloudMusicApiSource, NETEASE_SOURCE_NAME};
use super::time::publish_time_to_year;
use crate::module::music_library::models::{Album, Artist, Song};
use crate::module::music_source::types::{EntityType, SourceId, SourceType};

/// 获取单曲详情。
pub(super) fn get_song(
    source: &NeteaseCloudMusicApiSource,
    id: &str,
) -> Result<Option<Song>, String> {
    if !source.is_enabled() {
        return Err("网易云来源未启用".to_string());
    }
    let Some(remote_id) = parse_remote_id(id) else {
        return Ok(None);
    };
    let ids_json = format!("[{}]", remote_id);
    let songs = api::get_song_details(&source.client, &ids_json)?;
    Ok(songs.into_iter().next().map(|d| source.build_song(&d)))
}

/// 获取歌手详情。
pub(super) fn get_artist(
    source: &NeteaseCloudMusicApiSource,
    id: &str,
) -> Result<Option<Artist>, String> {
    if !source.is_enabled() {
        return Err("网易云来源未启用".to_string());
    }
    let Some(remote_id) = parse_remote_id(id) else {
        return Ok(None);
    };
    // /artist/detail 接口返回艺术家详情
    #[derive(serde::Deserialize)]
    struct ArtistDetailResp {
        #[serde(default)]
        code: i32,
        #[serde(default)]
        data: Option<ArtistDetailData>,
    }
    #[derive(serde::Deserialize, Default)]
    struct ArtistDetailData {
        #[serde(default)]
        artist: Option<super::types::NeteaseArtistBrief>,
    }
    let resp: ArtistDetailResp =
        source
            .client
            .get_json("/artist/detail", &[("id", &remote_id.to_string())])?;
    if resp.code != 200 {
        return Err(format!("获取艺术家失败 code={}", resp.code));
    }
    let brief = resp.data.and_then(|d| d.artist);
    Ok(brief.map(|b| Artist {
        id: format!("netease_artist_{}", remote_id),
        name: b.name,
        bio: b.briefDesc,
        source_ids: vec![SourceId {
            source_name: source.name.clone(),
            source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
            entity_type: EntityType::Artist,
            entity_id: build_entity_id(EntityKind::artist, remote_id),
        }],
    }))
}

/// 获取专辑详情。
pub(super) fn get_album(
    source: &NeteaseCloudMusicApiSource,
    id: &str,
) -> Result<Option<Album>, String> {
    if !source.is_enabled() {
        return Err("网易云来源未启用".to_string());
    }
    let Some(remote_id) = parse_remote_id(id) else {
        return Ok(None);
    };
    let (album, songs) = api::get_album_detail(&source.client, remote_id)?;
    let artist_id = album
        .artist
        .as_ref()
        .map(|a| format!("netease_artist_{}", a.id))
        .unwrap_or_default();
    let song_ids: Vec<String> = songs
        .iter()
        .map(|s| format!("netease_song_{}", s.id))
        .collect();
    let year = publish_time_to_year(album.publishTime);
    Ok(Some(Album {
        id: format!("netease_album_{}", remote_id),
        title: album.name,
        artist_id,
        cover_url: album.picUrl,
        song_ids,
        source_ids: vec![SourceId {
            source_name: source.name.clone(),
            source_type: SourceType::Web(NETEASE_SOURCE_NAME.to_string()),
            entity_type: EntityType::Album,
            entity_id: build_entity_id(EntityKind::album, remote_id),
        }],
        year,
    }))
}