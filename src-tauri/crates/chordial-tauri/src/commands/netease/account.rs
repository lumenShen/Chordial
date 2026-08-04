//! 网易云用户账号与"我的资源"命令。

use chordial_core::module::music_netease_source::NeteasePlaylistBrief;
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

/// 获取当前登录账号信息（依赖 cookie）。
///
/// # 返回
/// `{ code, profile: { userId, nickname, avatarUrl, signature } | null,
///    account: { id, userName } | null }`
#[tauri::command]
pub fn netease_get_user_account(
    ctx: State<'_, Arc<AppContext>>,
) -> Result<serde_json::Value, String> {
    // 调试日志：确认 cookie 是否被正确传递到 HTTP 客户端
    let cookie = ctx.netease_source.client().cookie();
    let cookie_preview: String = cookie.chars().take(80).collect();
    eprintln!(
        "[netease] get_user_account 调用，cookie 长度={}，预览=\"{}...\"",
        cookie.len(),
        cookie_preview
    );

    let resp = ctx.netease_source.get_user_account()?;
    eprintln!(
        "[netease] get_user_account 响应：code={}，profile={}，account={}",
        resp.code,
        resp.profile.is_some(),
        resp.account.is_some()
    );

    let json = serde_json::json!({
        "code": resp.code,
        "profile": resp.profile,
        "account": resp.account,
    });
    eprintln!(
        "[netease] get_user_account 返回 JSON：{}",
        serde_json::to_string(&json).unwrap_or_default()
    );
    Ok(json)
}

/// 获取指定用户的全部歌单（创建 + 收藏，通过 `subscribed` 字段区分）。
///
/// # 返回
/// `Vec<NeteasePlaylistBrief>` — 每个 item 含 `subscribed: bool`（true=收藏，false=创建）
#[tauri::command]
pub fn netease_get_user_playlists(
    ctx: State<'_, Arc<AppContext>>,
    uid: i64,
) -> Result<Vec<NeteasePlaylistBrief>, String> {
    ctx.netease_source.get_user_playlists(uid)
}

/// 获取当前用户收藏的专辑（依赖 cookie，分页）。
///
/// # 返回
/// `{ albums: [...], total: number, hasMore: bool }`
#[tauri::command]
pub fn netease_get_user_albums(
    ctx: State<'_, Arc<AppContext>>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<serde_json::Value, String> {
    let lim = limit.unwrap_or(50);
    let off = offset.unwrap_or(0);
    let (albums, total) = ctx.netease_source.get_user_albums(lim, off)?;
    Ok(serde_json::json!({
        "albums": albums,
        "total": total,
        "offset": off,
        "limit": lim,
        "hasMore": (off + albums.len() as u32) < total as u32,
    }))
}