//! 网易云来源配置与连通性测试命令。

use chordial_core::module::music_source::traits::MusicSource;
use chordial_core::module::music_web_source::WebSourceConfig;
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

/// 获取网易云来源配置（base_url / cookie / 超时 / 启用 / 下载目录）。
#[tauri::command]
pub fn netease_get_config(ctx: State<'_, Arc<AppContext>>) -> Result<WebSourceConfig, String> {
    Ok(ctx.netease_source.config())
}

/// 更新网易云来源配置并持久化。
///
/// 重建 HTTP 客户端以应用新的 base_url / cookie / timeout。
#[tauri::command]
pub fn netease_update_config(
    ctx: State<'_, Arc<AppContext>>,
    config: WebSourceConfig,
) -> Result<(), String> {
    ctx.netease_source.update_config(config)
}

/// 测试与网易云 API 服务器的连通性。
///
/// 通过发起一次 limit=1 的搜索请求验证 base_url + cookie 是否有效。
#[tauri::command]
pub fn netease_test_connection(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let cfg = ctx.netease_source.config();
    if cfg.base_url.is_empty() {
        return Err("API base_url 未配置".to_string());
    }
    let result = ctx.netease_source.search_songs("test");
    Ok(serde_json::json!({
        "ok": result.is_ok(),
        "error": result.err(),
        "base_url": cfg.base_url,
    }))
}