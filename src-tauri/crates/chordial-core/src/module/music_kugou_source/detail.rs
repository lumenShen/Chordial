//! 酷狗详情 — 单曲 / 歌手 / 专辑详情获取。
//!
//! 以自由函数形式提供详情逻辑，由 [`KugouMusicApiSource`] 的 trait 实现委托调用。

use super::api;
use super::entity_id::{build_entity_id, parse_remote_id, EntityKind};
use super::quality::default_quality;
use super::source::{parse_year_from_date, KugouMusicApiSource, KUGOU_SOURCE_NAME};
use crate::module::music_library::models::{Album, Artist, Song};
use crate::module::music_source::types::{EntityType, SourceId, SourceType};

/// 获取单曲详情。
pub(super) fn get_song(source: &KugouMusicApiSource, id: &str) -> Result<Option<Song>, String> {
    if !source.is_enabled() {
        return Err("酷狗来源未启用".to_string());
    }
    // id 是 song:<hash>
    let hash = parse_remote_id(id).unwrap_or_default();
    if hash.is_empty() {
        return Ok(None);
    }
    let song_id = format!("kugou_song_{}", hash);
    let source_id = SourceId {
        source_name: source.name.clone(),
        source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
        entity_type: EntityType::Song,
        entity_id: id.to_string(),
    };

    // 优先通过 /search/lyric 获取歌曲元信息（歌曲名 + 歌手 + 时长）
    // 这个接口仅需 hash，返回的 KugouLyricCandidate 含 song/singer/duration 字段
    let candidates = api::search_lyric(&source.client, &hash, 0).unwrap_or_default();
    if let Some(candidate) = candidates.into_iter().next() {
        let artist_names: Vec<String> = if candidate.singer.is_empty() {
            vec!["未知艺术家".to_string()]
        } else {
            candidate
                .singer
                .split('/')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        // candidate.duration 是毫秒，转换为秒
        let duration = if candidate.duration > 0 {
            Some((candidate.duration / 1000) as u64)
        } else {
            None
        };
        return Ok(Some(Song {
            id: song_id,
            title: if candidate.song.is_empty() {
                hash.clone()
            } else {
                candidate.song
            },
            artist_names,
            album_title: None,
            duration,
            artist_ids: Vec::new(), // search/lyric 不返回 SingerId
            album_id: None,
            lyric_id: None,
            source_ids: vec![source_id],
            year: None,
            cover_url: None,
        }));
    }

    // 退化为 /song/url 验证 hash 有效性并从 URL 数据提取 name/duration
    match api::get_song_url(&source.client, &hash, default_quality(), None) {
        Ok(url_data) => {
            let Some(url_entry) = url_data.url.first() else {
                return Ok(None);
            };
            let duration = if url_entry.duration > 0 {
                Some(url_entry.duration as u64)
            } else {
                None
            };
            Ok(Some(Song {
                id: song_id,
                title: url_entry.name.clone(),
                artist_names: vec!["未知艺术家".to_string()],
                album_title: None,
                duration,
                artist_ids: Vec::new(),
                album_id: None,
                lyric_id: None,
                source_ids: vec![source_id],
                year: None,
                cover_url: None,
            }))
        }
        Err(_) => Ok(None),
    }
}

/// 获取歌手详情。
pub(super) fn get_artist(
    source: &KugouMusicApiSource,
    id: &str,
) -> Result<Option<Artist>, String> {
    if !source.is_enabled() {
        return Err("酷狗来源未启用".to_string());
    }
    let singer_id = parse_remote_id(id)
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    if singer_id == 0 {
        return Ok(None);
    }

    // 调用 /artist/detail 获取歌手详情（含简介 brief/intro、头像 img、粉丝数等）
    let detail = api::get_artist_detail(&source.client, singer_id)?;
    let name = detail.name();
    let bio = {
        let intro = detail.intro();
        if intro.is_empty() {
            None
        } else {
            Some(intro.to_string())
        }
    };

    Ok(Some(Artist {
        id: format!("kugou_artist_{}", singer_id),
        name: if name.is_empty() {
            format!("singer_{}", singer_id)
        } else {
            name.to_string()
        },
        bio,
        source_ids: vec![SourceId {
            source_name: source.name.clone(),
            source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
            entity_type: EntityType::Artist,
            entity_id: build_entity_id(EntityKind::artist, &singer_id.to_string()),
        }],
    }))
}

/// 获取专辑详情。
pub(super) fn get_album(source: &KugouMusicApiSource, id: &str) -> Result<Option<Album>, String> {
    if !source.is_enabled() {
        return Err("酷狗来源未启用".to_string());
    }
    let album_id = parse_remote_id(id).unwrap_or_default();
    if album_id.is_empty() || album_id == "0" {
        return Ok(None);
    }

    // 1. 获取专辑基础信息（含封面 URL、author_name、publish_date）
    let info = api::get_album_info(&source.client, &album_id)?;

    // 2. 尝试获取专辑详情（含 singer_id 数值，便于填充 Album.artist_id）
    //    /album/detail 是较新的接口，部分自建 ncmapi 风格服务端可能未实现，
    //    失败时降级为仅使用 /album 信息（artist_id 留空）
    let (artist_id, year_from_detail) = match api::get_album_detail(&source.client, &album_id) {
        Ok(detail) => {
            let artist_id = if detail.singer_id > 0 {
                format!("kugou_artist_{}", detail.singer_id)
            } else {
                String::new()
            };
            let year = parse_year_from_date(&detail.publish_date);
            (artist_id, year)
        }
        Err(e) => {
            eprintln!(
                "[kugou] get_album: /album/detail 失败 album_id={}，降级使用 /album 信息: {}",
                album_id, e
            );
            (String::new(), None)
        }
    };

    // 3. 获取专辑歌曲列表（用于填充 song_ids）
    //    默认拉取前 1000 首（pagesize=500, 最多 2 页）
    let mut song_ids: Vec<String> = Vec::new();
    let pagesize: u32 = 500;
    let max_pages: u32 = 4; // 最多 2000 首
    let mut page = 1u32;
    let mut total: i64 = -1;
    loop {
        match api::get_album_songs(&source.client, &album_id, page, pagesize) {
            Ok((list, t)) => {
                if total < 0 {
                    total = t;
                }
                for song in &list {
                    let hash = song.file_hash_lower();
                    if !hash.is_empty() {
                        song_ids.push(format!("kugou_song_{}", hash));
                    }
                }
                let len = list.len() as u32;
                if len < pagesize
                    || (total > 0 && song_ids.len() as i64 >= total)
                    || page >= max_pages
                {
                    break;
                }
                page += 1;
            }
            Err(e) => {
                eprintln!(
                    "[kugou] get_album: /album/songs 失败 album_id={} page={}：{}",
                    album_id, page, e
                );
                // 失败不致命，继续返回专辑基本信息（song_ids 可能部分为空）
                break;
            }
        }
    }

    // 4. 年份：优先 /album/detail 的 publish_date，回退到 /album 的 publish_date
    let year = year_from_detail.or_else(|| parse_year_from_date(&info.publish_date));

    // 5. 封面 URL：优先 sizable_cover（可调整尺寸），回退到 cover
    let cover_url = if !info.sizable_cover.is_empty() {
        Some(info.sizable_cover)
    } else if !info.cover.is_empty() {
        Some(info.cover)
    } else {
        None
    };

    Ok(Some(Album {
        id: format!("kugou_album_{}", album_id),
        title: info.album_name,
        artist_id,
        cover_url,
        song_ids,
        source_ids: vec![SourceId {
            source_name: source.name.clone(),
            source_type: SourceType::Web(KUGOU_SOURCE_NAME.to_string()),
            entity_type: EntityType::Album,
            entity_id: build_entity_id(EntityKind::album, &album_id),
        }],
        year,
    }))
}