//! 酷狗下载 — 音频字节与专辑封面图片获取。
//!
//! 以自由函数形式提供下载逻辑，由 [`KugouMusicApiSource`] 的 trait 实现委托调用。

use super::api;
use super::entity_id::parse_remote_id;
use super::quality::default_quality;
use super::source::KugouMusicApiSource;

/// 获取歌曲音频文件字节。
pub(super) fn song_file_get(
    source: &KugouMusicApiSource,
    entity_id: &str,
) -> Result<Vec<u8>, String> {
    if !source.is_enabled() {
        return Err("酷狗来源未启用".to_string());
    }
    let hash = parse_remote_id(entity_id).unwrap_or_default();
    if hash.is_empty() {
        return Err(format!("无效的 entity_id: {}", entity_id));
    }
    // 默认 320 kbps MP3（Exhigh）
    // trait 方法签名不接受音质参数，因此使用默认值；
    // 前端需要指定音质时通过专门的 Tauri 命令（source_download_song）调用
    let (bytes, _url_data) = api::download_song(&source.client, &hash, default_quality(), None)?;
    Ok(bytes)
}

/// 获取专辑封面图片字节。
pub(super) fn album_picture_get(
    source: &KugouMusicApiSource,
    entity_id: &str,
) -> Result<Vec<u8>, String> {
    if !source.is_enabled() {
        return Err("酷狗来源未启用".to_string());
    }
    // entity_id 可能是 http(s):// URL，直接下载
    if entity_id.starts_with("http://") || entity_id.starts_with("https://") {
        return source.client.get_bytes(entity_id);
    }

    // 支持 album:<id> 和 song:<hash>
    // song:<hash> 时先通过 /search/lyric 获取歌曲信息，但该接口不返回 AlbumID，
    // 因此 song:<hash> 路径会失败，建议前端直接用 song.cover_url 或调 album:<id>
    let album_id = if entity_id.starts_with("album:") {
        parse_remote_id(entity_id).unwrap_or_default()
    } else {
        // song:<hash> 无法直接获取专辑封面（需先查 AlbumID，搜索接口未返回）
        return Err(format!(
            "酷狗 album_picture_get 仅支持 album:<id> 或 http(s):// URL，收到: {}；\
             建议前端使用歌曲详情中已缓存的 cover_url",
            entity_id
        ));
    };

    if album_id.is_empty() || album_id == "0" {
        return Err(format!("无效的专辑 ID: {}", entity_id));
    }

    let info = api::get_album_info(&source.client, &album_id)?;
    let cover_url = if !info.sizable_cover.is_empty() {
        info.sizable_cover
    } else if !info.cover.is_empty() {
        info.cover
    } else {
        return Err("专辑无封面 URL".to_string());
    };

    source.client.get_bytes(&cover_url)
}