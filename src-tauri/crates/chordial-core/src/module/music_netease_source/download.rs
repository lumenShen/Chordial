//! 网易云下载 — 音频字节与专辑封面图片获取。
//!
//! 以自由函数形式提供下载逻辑，由 [`NeteaseCloudMusicApiSource`] 的 trait 实现委托调用。
//! 封面获取走双层缓存（cover_cache / pic_url_cache），避免重复下载与重复调接口。

use super::api;
use super::entity_id::parse_remote_id;
use super::source::NeteaseCloudMusicApiSource;
use super::types::AudioQuality;
use std::sync::Arc;

/// 获取歌曲音频文件字节。
pub(super) fn song_file_get(
    source: &NeteaseCloudMusicApiSource,
    entity_id: &str,
) -> Result<Vec<u8>, String> {
    if !source.is_enabled() {
        return Err("网易云来源未启用".to_string());
    }
    let Some(remote_id) = parse_remote_id(entity_id) else {
        return Err(format!("无效的 entity_id: {}", entity_id));
    };
    // 使用音质降级下载：从 exhigh 开始向下降级，确保尽可能返回可播放字节
    // 相比固定 exhigh 的 download_song，能避免 VIP/无版权等级导致的播放失败
    let result = api::download_song_with_fallback(
        &source.client,
        remote_id,
        Some(AudioQuality::Exhigh.to_level_str()),
    )?;
    Ok(result.bytes)
}

/// 获取专辑封面图片字节。
pub(super) fn album_picture_get(
    source: &NeteaseCloudMusicApiSource,
    entity_id: &str,
) -> Result<Vec<u8>, String> {
    if !source.is_enabled() {
        return Err("网易云来源未启用".to_string());
    }

    // 1. 先查字节缓存：命中则直接返回
    //    持锁期间只 clone Arc（廉价），锁外再 clone Vec<u8>，避免持锁深拷贝
    {
        let cache = source.cover_cache.lock();
        if let Some(hit) = cache.get(entity_id) {
            let arc = Arc::clone(hit);
            drop(cache); // 显式释放锁
            return Ok((*arc).clone()); // 锁外深拷贝
        }
    }

    // 2. entity_id 可能是 album:<id> / song:<id>，也可能直接是图片 URL
    //    若是 URL，直接下载并以 URL 本身作为缓存 key
    if entity_id.starts_with("http://") || entity_id.starts_with("https://") {
        let bytes = source.client.get_bytes(entity_id)?;
        source.insert_cover_cache(entity_id, bytes.clone());
        return Ok(bytes);
    }

    let Some(remote_id) = parse_remote_id(entity_id) else {
        return Err(format!("无效的 entity_id: {}", entity_id));
    };

    // 3. 查 picUrl 缓存：命中则跳过 /song/detail 或 /album 接口
    let pic_url_cached: Option<String> = {
        let cache = source.pic_url_cache.lock();
        cache.get(entity_id).cloned()
    };

    // 4. picUrl 未命中 → 走原有 /song/detail 或 /album 逻辑拿 picUrl
    let pic_url = match pic_url_cached {
        Some(cached) => cached,
        None => {
            // 根据 entity_id 前缀选择 API：
            // - album:<id> → /album?id=<id> 拿 album.picUrl
            // - song:<id>  → /song/detail?ids=<id> 拿 songs[0].al.picUrl
            //   （旧实现统一调 /album，把 song id 当 album id 导致 404）
            let fetched = if entity_id.starts_with("song:") {
                #[derive(serde::Deserialize)]
                struct SongDetailResp {
                    #[serde(default)]
                    code: i32,
                    #[serde(default)]
                    songs: Vec<super::types::NeteaseSongDetail>,
                }
                let resp: SongDetailResp = source.client.get_json(
                    "/song/detail",
                    &[("ids", &remote_id.to_string())],
                )?;
                if resp.code != 200 {
                    return Err(format!("获取歌曲详情失败 code={}", resp.code));
                }
                resp.songs
                    .into_iter()
                    .next()
                    .and_then(|s| s.al.and_then(|a| a.picUrl))
                    .ok_or_else(|| "歌曲无封面 URL".to_string())
            } else {
                // album:<id> 或裸 id → /album 接口
                #[derive(serde::Deserialize)]
                struct AlbumResp {
                    #[serde(default)]
                    code: i32,
                    #[serde(default)]
                    album: Option<super::types::NeteaseAlbumBrief>,
                }
                let resp: AlbumResp = source
                    .client
                    .get_json("/album", &[("id", &remote_id.to_string())])?;
                if resp.code != 200 {
                    return Err(format!("获取专辑失败 code={}", resp.code));
                }
                resp.album
                    .and_then(|a| a.picUrl)
                    .ok_or_else(|| "专辑无封面 URL".to_string())
            };
            // picUrl 获取成功才写缓存，避免空字符串污染缓存
            let fetched = fetched?;
            if !fetched.is_empty() {
                source.insert_pic_url_cache(entity_id, fetched.clone());
            }
            fetched
        }
    };

    // 5. 用 picUrl 调 get_bytes，写入 cover_cache，返回字节
    let bytes = source.client.get_bytes(&pic_url)?;
    source.insert_cover_cache(entity_id, bytes.clone());
    Ok(bytes)
}