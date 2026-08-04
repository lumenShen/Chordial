//! 酷狗音乐来源命令 — 配置/测试/搜索/扫码登录。

use chordial_core::module::music_library::models::{Album, Artist, Song};
use chordial_core::module::music_source::traits::MusicSource;
use chordial_core::module::music_web_source::WebSourceConfig;
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

// ══════════════════════════════════════════════════════════════════════════════
// 酷狗音乐来源命令 — 配置/测试/搜索
// ══════════════════════════════════════════════════════════════════════════════

/// 获取酷狗来源配置（base_url / cookie / 超时 / 启用 / 下载目录）。
#[tauri::command]
pub fn kugou_get_config(ctx: State<'_, Arc<AppContext>>) -> Result<WebSourceConfig, String> {
    Ok(ctx.kugou_source.config())
}

/// 更新酷狗来源配置并持久化。
///
/// 重建 HTTP 客户端以应用新的 base_url / cookie / timeout。
#[tauri::command]
pub fn kugou_update_config(
    ctx: State<'_, Arc<AppContext>>,
    config: WebSourceConfig,
) -> Result<(), String> {
    ctx.kugou_source.update_config(config)
}

/// 测试与酷狗 API 服务器的连通性。
///
/// 通过发起一次 page=1 pagesize=1 的搜索请求验证 base_url + cookie 是否有效。
#[tauri::command]
pub fn kugou_test_connection(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let cfg = ctx.kugou_source.config();
    if cfg.base_url.is_empty() {
        return Err("API base_url 未配置".to_string());
    }
    let result = ctx.kugou_source.search_songs("test");
    Ok(serde_json::json!({
        "ok": result.is_ok(),
        "error": result.err(),
        "base_url": cfg.base_url,
    }))
}

/// 在线搜索歌曲（返回库内 Song 模型，附带 kugou SourceId）。
#[tauri::command]
pub fn kugou_search_songs(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
) -> Result<Vec<Song>, String> {
    ctx.kugou_source.search_songs(&query)
}

/// 在线搜索歌手（返回库内 Artist 模型）。
///
/// 酷狗 API 使用 1-based page/pagesize；此处将前端 limit/offset 转换为对应分页。
#[tauri::command]
pub fn kugou_search_artists(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<Artist>, String> {
    let pagesize = limit.unwrap_or(30);
    let offset_val = offset.unwrap_or(0);
    let page = (offset_val / pagesize.max(1)) + 1;
    ctx.kugou_source.search_artists(&query, page, pagesize)
}

/// 在线搜索专辑（返回库内 Album 模型）。
///
/// 酷狗 API 使用 1-based page/pagesize；此处将前端 limit/offset 转换为对应分页。
#[tauri::command]
pub fn kugou_search_albums(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<Album>, String> {
    let pagesize = limit.unwrap_or(30);
    let offset_val = offset.unwrap_or(0);
    let page = (offset_val / pagesize.max(1)) + 1;
    ctx.kugou_source.search_albums(&query, page, pagesize)
}

/// 酷狗扫码登录第 1 步：获取 key。
///
/// 返回 `String` — key，用于后续生成二维码与轮询。
#[tauri::command]
pub fn kugou_login_qr_key(ctx: State<'_, Arc<AppContext>>) -> Result<String, String> {
    ctx.kugou_source.login_qr_key()
}

/// 酷狗扫码登录第 2 步：用 key 生成二维码。
///
/// # 参数
/// - `key`: 从 `kugou_login_qr_key` 获取的 key
/// - `qrimg`: 是否返回 base64 data URL
///
/// # 返回
/// `{ url: string, base64: string }`
#[tauri::command]
pub fn kugou_login_qr_create(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
    qrimg: Option<bool>,
) -> Result<serde_json::Value, String> {
    let data = ctx.kugou_source.login_qr_create(&key, qrimg.unwrap_or(true))?;
    Ok(serde_json::json!({
        "url": data.url,
        "base64": data.base64,
    }))
}

/// 酷狗扫码登录第 3 步：轮询扫码状态。
///
/// # 返回
/// `{ status: number, statusText: string, token: string|null, userid: number|null }`
///
/// 当 `status == 4` 时后端会自动持久化 cookie 到配置。
#[tauri::command]
pub fn kugou_login_qr_check(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
) -> Result<serde_json::Value, String> {
    use chordial_core::module::music_kugou_source::types::KugouQrCheckStatus;
    let resp = ctx.kugou_source.login_qr_check(&key)?;
    let status_enum = KugouQrCheckStatus::from(resp.data.status);
    Ok(serde_json::json!({
        "status": resp.data.status,
        "statusText": status_enum.as_str(),
        "token": resp.data.token,
        "userid": resp.data.userid,
    }))
}