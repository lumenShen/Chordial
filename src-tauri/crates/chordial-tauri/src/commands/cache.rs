//! Cache 命令 — 纯内存 TTL 过期 + Blob 磁盘缓存。

use super::common::TtlArg;
use chordial_core::AppContext;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn cache_get(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Value, String> {
    ctx.cache
        .get_raw(&key)
        .ok_or_else(|| format!("缓存项 '{}' 不存在或已过期", key))
}

#[tauri::command]
pub fn cache_set(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
    value: Value,
    ttl: TtlArg,
) -> Result<(), String> {
    ctx.cache.set_raw(&key, value, &ttl.into());
    Ok(())
}

#[tauri::command]
pub fn cache_remove(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.cache.remove(&key))
}

#[tauri::command]
pub fn cache_has(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.cache.has(&key))
}

#[tauri::command]
pub fn cache_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.cache.keys())
}

#[tauri::command]
pub fn cache_clear(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.cache.clear();
    Ok(())
}

#[tauri::command]
pub fn cache_clear_expired(ctx: State<'_, Arc<AppContext>>) -> Result<usize, String> {
    Ok(ctx.cache.clear_expired())
}

#[tauri::command]
pub fn cache_touch(ctx: State<'_, Arc<AppContext>>, key: String, ttl: TtlArg) -> Result<bool, String> {
    Ok(ctx.cache.touch(&key, &ttl.into()))
}

#[tauri::command]
pub fn cache_enable_blob_storage(ctx: State<'_, Arc<AppContext>>, dir: String) -> Result<(), String> {
    ctx.cache.enable_blob_storage(PathBuf::from(dir))
}

#[tauri::command]
pub fn cache_blob_storage_enabled(ctx: State<'_, Arc<AppContext>>) -> Result<bool, String> {
    Ok(ctx.cache.blob_storage_enabled())
}

#[tauri::command]
pub fn cache_set_blob(
    ctx: State<'_, Arc<AppContext>>,
    key: String,
    data: Vec<u8>,
    ttl: TtlArg,
) -> Result<(), String> {
    ctx.cache.set_blob(&key, &data, &ttl.into())
}

#[tauri::command]
pub fn cache_get_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Vec<u8>, String> {
    ctx.cache
        .get_blob(&key)
        .ok_or_else(|| format!("Blob 缓存项 '{}' 不存在或已过期", key))
}

#[tauri::command]
pub fn cache_remove_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.cache.remove_blob(&key))
}

#[tauri::command]
pub fn cache_has_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.cache.has_blob(&key))
}

#[tauri::command]
pub fn cache_blob_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.cache.blob_keys())
}

#[tauri::command]
pub fn cache_clear_blobs(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.cache.clear_blobs();
    Ok(())
}

#[tauri::command]
pub fn cache_clear_expired_blobs(ctx: State<'_, Arc<AppContext>>) -> Result<usize, String> {
    Ok(ctx.cache.clear_expired_blobs())
}
