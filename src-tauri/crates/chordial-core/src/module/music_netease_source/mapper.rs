//! 网易云数据模型 → 库内 [`Song`] 模型的映射。

use super::entity_id::{build_entity_id, EntityKind};
use crate::module::music_library::models::Song;
use crate::module::music_source::types::{EntityType, SourceId, SourceType};
use super::types::NeteaseSongDetail;

impl super::NeteaseCloudMusicApiSource {
    /// 从 [`NeteaseSongDetail`] 构建 [`Song`] 模型（公开版本，供 Tauri 命令使用）。
    pub fn build_song_from_detail(&self, detail: &NeteaseSongDetail) -> Song {
        self.build_song(detail)
    }

    /// 从 [`NeteaseSongDetail`] 构建 [`Song`] 模型。
    pub(super) fn build_song(&self, detail: &NeteaseSongDetail) -> Song {
        let entity_id = build_entity_id(EntityKind::song, detail.id);
        let song_id = format!("netease_song_{}", detail.id);
        let artist_names: Vec<String> = if detail.ar.is_empty() {
            vec!["未知艺术家".to_string()]
        } else {
            detail.ar.iter().map(|a| a.name.clone()).collect()
        };
        let artist_ids: Vec<String> = if detail.ar.is_empty() {
            vec!["netease_artist_unknown".to_string()]
        } else {
            detail.ar.iter().map(|a| format!("netease_artist_{}", a.id)).collect()
        };
        let album_title = detail.al.as_ref().map(|a| a.name.clone());
        let album_id = detail.al.as_ref().map(|a| format!("netease_album_{}", a.id));
        let lyric_id = Some(format!("netease_lyric_{}", detail.id));

        let source_id = SourceId {
            source_name: self.name.clone(),
            source_type: SourceType::Web(super::NETEASE_SOURCE_NAME.to_string()),
            entity_type: EntityType::Song,
            entity_id: entity_id.clone(),
        };

        Song {
            id: song_id,
            title: detail.name.clone(),
            artist_names,
            album_title,
            duration: if detail.dt > 0 {
                Some((detail.dt / 1000) as u64)
            } else {
                None
            },
            artist_ids,
            album_id,
            lyric_id,
            source_ids: vec![source_id],
            year: None,
            // 网易云 al.picUrl 直接作为封面 URL，前端无需通过 chordial:// 协议二次拉取
            // 避免song id 被当成 album id 调 /album 接口导致 404/405 限流
            cover_url: detail.al.as_ref().and_then(|a| a.picUrl.clone()),
        }
    }
}