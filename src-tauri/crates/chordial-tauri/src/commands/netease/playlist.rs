//! 网易云歌单/专辑详情与导入本地音乐库命令。

use crate::commands::common::LIBRARY_CHANGED_EVENT;
use chordial_core::module::playlist::{self, Playlist};
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// 获取歌单详情（元信息 + 全部曲目）。
#[tauri::command]
pub fn netease_get_playlist_full(
    ctx: State<'_, Arc<AppContext>>,
    playlist_id: i64,
) -> Result<serde_json::Value, String> {
    let (detail, tracks) = ctx.netease_source.get_playlist_full(playlist_id)?;
    Ok(serde_json::json!({
        "detail": detail,
        "tracks": tracks,
    }))
}

/// 将在线歌单保存为本地 Playlist，并把所有曲目加入音乐库（自动去重）。
///
/// # 工作流
/// 1. 调用 `get_playlist_full` 拉取歌单元信息 + 全部曲目
/// 2. 每首曲目通过 `library.add_song` 加入库（带 netease SourceId，自动合并去重）
/// 3. 创建本地 Playlist 记录，song_ids 引用库内 Song.id
/// 4. 持久化 + 触发 `library-changed` 事件
#[tauri::command]
pub fn netease_save_playlist_to_library(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
    playlist_id: i64,
) -> Result<Playlist, String> {
    let (detail, tracks) = ctx.netease_source.get_playlist_full(playlist_id)?;

    // 批量构建 Song 后一次性入库（add_songs_batch 只反序列化+回写一次）
    // 旧实现 for-loop + add_song 每首都会全量反序列化+回写，1000 首歌约 80s
    let songs: Vec<_> = tracks
        .iter()
        .map(|t| ctx.netease_source.build_song_from_detail(t))
        .collect();
    let song_ids = ctx.library.add_songs_batch(&songs)?;
    // ⚠ 关键：add_songs_batch 只修改 library.store 的内存缓存并标记 dirty，
    // 必须显式调用 library.save() 落盘 music_library.json，否则重启后歌曲全部丢失。
    // ctx.store.save() 只保存 storage.json（歌单元数据），与 library.store 是两个独立的 PersistentStore。
    ctx.library.save()?;

    let mut playlist = Playlist::new("netease", &detail.name);
    playlist.remote_id = Some(detail.id);
    playlist.cover_url = detail.coverImgUrl;
    playlist.creator = detail.creator.map(|c| c.nickname);
    playlist.description = detail.description;
    playlist.tags = detail.tags;
    playlist.song_ids = song_ids;

    playlist::add(&ctx.store, &playlist)?;
    ctx.store.save()?;

    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(playlist)
}

/// 将网易云专辑导入本地音乐库（不创建本地 Playlist，仅把所有曲目加入 library）。
///
/// # 工作流
/// 1. 调用 `get_album_detail` 拉取专辑元信息 + 全部曲目
/// 2. 每首曲目通过 `library.add_song` 加入库（带 netease SourceId，自动合并去重）
/// 3. 触发 `library-changed` 事件
///
/// # 返回
/// `{ album: {...}, imported_count: number }`
#[tauri::command]
pub fn netease_save_album_to_library(
    ctx: State<'_, Arc<AppContext>>,
    app: AppHandle,
    album_id: i64,
) -> Result<serde_json::Value, String> {
    let (album, songs) = ctx.netease_source.get_album_detail(album_id)?;

    // 批量构建 Song 后一次性入库（add_songs_batch 只反序列化+回写一次）
    let built: Vec<_> = songs
        .iter()
        .map(|t| ctx.netease_source.build_song_from_detail(t))
        .collect();
    let stored_ids = ctx.library.add_songs_batch(&built)?;
    let imported = stored_ids.len();
    // ⚠ 关键：必须调用 library.save() 落盘 music_library.json，
    // 否则重启后歌曲全部丢失（ctx.store.save() 只保存 storage.json）。
    ctx.library.save()?;
    ctx.store.save()?;

    let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    Ok(serde_json::json!({
        "album": {
            "id": album.id,
            "name": album.name,
            "picUrl": album.picUrl,
            "artist": album.artist,
            "artists": album.artists,
            "description": album.description,
            "publishTime": album.publishTime,
            "size": album.size,
        },
        "imported_count": imported,
        "total_count": songs.len(),
    }))
}