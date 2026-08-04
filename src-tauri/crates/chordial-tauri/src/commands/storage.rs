//! Storage 命令 — 手动落盘（含 Blob 持久化二进制存储）。

use chordial_core::AppContext;
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn storage_get(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Value, String> {
    ctx.store
        .get_raw(&key)
        .ok_or_else(|| format!("键 '{}' 不存在", key))
}

#[tauri::command]
pub fn storage_set(ctx: State<'_, Arc<AppContext>>, key: String, value: Value) -> Result<(), String> {
    ctx.store.set_raw(&key, value);
    Ok(())
}

#[tauri::command]
pub fn storage_remove(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.store.remove(&key))
}

#[tauri::command]
pub fn storage_has(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.store.has(&key))
}

#[tauri::command]
pub fn storage_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.store.keys())
}

#[tauri::command]
pub fn storage_clear(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.store.clear();
    Ok(())
}

#[tauri::command]
pub fn storage_save(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.store.save()
}

#[tauri::command]
pub fn storage_set_blob(ctx: State<'_, Arc<AppContext>>, key: String, data: Vec<u8>) -> Result<(), String> {
    ctx.store.set_blob(&key, &data)
}

#[tauri::command]
pub fn storage_get_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<Vec<u8>, String> {
    ctx.store
        .get_blob(&key)
        .ok_or_else(|| format!("Blob 存储项 '{}' 不存在", key))
}

#[tauri::command]
pub fn storage_remove_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.store.remove_blob(&key))
}

#[tauri::command]
pub fn storage_has_blob(ctx: State<'_, Arc<AppContext>>, key: String) -> Result<bool, String> {
    Ok(ctx.store.has_blob(&key))
}

#[tauri::command]
pub fn storage_blob_keys(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, String> {
    Ok(ctx.store.blob_keys())
}

#[tauri::command]
pub fn storage_clear_blobs(ctx: State<'_, Arc<AppContext>>) -> Result<(), String> {
    ctx.store.clear_blobs();
    Ok(())
}
