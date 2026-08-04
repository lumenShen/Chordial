//! 网易云扫码登录命令（与 kugou 的 qr 流程保持一致的命名约定）。

use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

/// 扫码登录第 1 步：获取 unikey。
///
/// 返回 `String` — 32 位 unikey，用于后续生成二维码与轮询。
#[tauri::command]
pub fn netease_login_qr_key(ctx: State<'_, Arc<AppContext>>) -> Result<String, String> {
    ctx.netease_source.login_qr_key()
}

/// 扫码登录第 2 步：用 unikey 生成二维码。
///
/// # 参数
/// - `key`: 从 `netease_login_qr_key` 获取的 unikey
/// - `qrimg`: 是否返回 base64 data URL（直接作为 `<img src>`）
///
/// # 返回
/// `{ qrurl: string, qrimg: string|null }`
#[tauri::command]
pub fn netease_login_qr_create(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
    qrimg: Option<bool>,
) -> Result<serde_json::Value, String> {
    let data = ctx.netease_source.login_qr_create(&key, qrimg.unwrap_or(true))?;
    Ok(serde_json::json!({
        "qrurl": data.qrurl,
        "qrimg": data.qrimg,
    }))
}

/// 扫码登录第 3 步：轮询扫码状态。
///
/// # 返回
/// `{ code: number, status: string, message: string|null, avatarUrl: string|null,
///    nickname: string|null, cookie: string|null }`
///
/// 当 `code == 803` 时后端会自动持久化 cookie 到配置，调用方无需再保存。
#[tauri::command]
pub fn netease_login_qr_check(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
) -> Result<serde_json::Value, String> {
    let resp = ctx.netease_source.login_qr_check(&key)?;
    Ok(serde_json::json!({
        "code": resp.code,
        "status": resp.status().as_str(),
        "message": resp.message,
        "avatarUrl": resp.avatarUrl,
        "nickname": resp.nickname,
        "cookie": resp.cookie,
    }))
}