//! Config 命令 — 自动防抖落盘。

use chordial_core::AppContext;
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn config_get(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Value, String> {
    ctx.config
        .get_raw(&key)
        .ok_or_else(|| format!("配置项 '{}' 不存在", key))
}

#[tauri::command]
pub fn config_set(ctx: State<'_, Arc<AppContext>>, key: String, value: Value) -> Result<(), String> {
    ctx.config.set_raw(&key, value);
    Ok(())
}

#[tauri::command]
pub fn config_remove(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.config.remove(&key))
}

#[tauri::command]
pub fn config_has(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.config.has(&key))
}

#[tauri::command]
pub fn config_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.config.keys())
}

#[tauri::command]
pub fn config_clear(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.config.clear();
    Ok(())
}

#[tauri::command]
pub fn config_flush(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.config.flush()
}

#[tauri::command]
pub fn config_reload(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.config.reload();
    Ok(())
}
