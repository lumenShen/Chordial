//! 歌词优先级命令 — 多来源歌词选择最佳。

use chordial_core::module::lyric_preference::{self, LyricCandidate, LyricPreference, LyricSourceKind};
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

/// 获取歌词优先级配置。
#[tauri::command]
pub fn lyric_preference_get(ctx: State<'_, Arc<AppContext>>) -> Result<LyricPreference, String> {
    Ok(LyricPreference::load(&ctx.config))
}

/// 更新歌词优先级配置（自动防抖落盘到 config.json）。
#[tauri::command]
pub fn lyric_preference_set(
    ctx: State<'_, Arc<AppContext>>,
    preference: LyricPreference,
) -> Result<(), String> {
    preference.save(&ctx.config)
}

/// 列出所有可用的歌词来源（用于设置页构建可拖拽优先级列表）。
///
/// 返回顺序：固定 `Local` 在前，其余按 `LyricSourceKind` 枚举顺序。
/// 未注册的来源也会列出（标记 `available=false`），便于用户在 UI 中看到全部可选项。
#[tauri::command]
pub fn lyric_source_list(ctx: State<'_, Arc<AppContext>>) -> Result<serde_json::Value, String> {
    let registered: std::collections::HashSet<String> =
        ctx.registrar.list_names().into_iter().collect();

    // Local 永远视为可用（库内 Lyric 实体，不依赖 registrar）
    let all_kinds = [
        LyricSourceKind::Local,
        LyricSourceKind::Netease,
        LyricSourceKind::Kugou,
    ];

    let list: Vec<serde_json::Value> = all_kinds
        .iter()
        .map(|k| {
            let name = k.source_name();
            let available = *k == LyricSourceKind::Local || registered.contains(name);
            serde_json::json!({
                "kind": k,
                "source_name": name,
                "display_name": k.display_name(),
                "available": available,
            })
        })
        .collect();

    Ok(serde_json::json!({ "sources": list }))
}

/// 获取指定歌曲的最佳歌词（综合所有可用来源，按用户偏好选择）。
///
/// # 工作流
/// 1. 加载用户偏好 `LyricPreference`（自动规范化 source_order）
/// 2. 收集候选歌词：
///    - 本地：库内通过 `song_id` 索引的 Lyric 实体
///    - 在线：遍历 `song.source_ids`，跳过 Local，对每个来源通过 registrar 查找实现并调用 `get_lyric`
/// 3. 用 `select_best` 选出得分最高的候选
/// 4. 返回歌词文本 + 格式 + 来源
///
/// # 容错
/// 单个来源失败不影响其他来源；只要有一个候选成功即返回。
#[tauri::command]
pub fn lyric_get_preferred(
    ctx: State<'_, Arc<AppContext>>,
    song_id: String,
) -> Result<serde_json::Value, String> {
    let pref = LyricPreference::load(&ctx.config);

    let mut candidates: Vec<LyricCandidate> = Vec::new();

    // 候选 1：本地歌词（库内 Lyric 实体，按 song_id 索引）
    if let Some(local_lyric) = ctx.library.get_lyric_of_song(&song_id) {
        if !local_lyric.text.is_empty() {
            candidates.push(LyricCandidate::new(
                LyricSourceKind::Local,
                local_lyric.text,
            ));
        }
    }

    // 候选 2..N：遍历 song.source_ids 中所有在线来源
    // 每个 SourceId 携带 source_name + entity_id，通过 registrar 动态查找来源实现
    if let Some(song) = ctx.library.get_song(&song_id) {
        for sid in &song.source_ids {
            // Local 来源已通过库内 Lyric 实体处理，跳过
            if sid.source_name == LyricSourceKind::Local.source_name() {
                continue;
            }
            // 仅处理已知 LyricSourceKind
            let kind = match LyricSourceKind::from_source_name(&sid.source_name) {
                Some(k) => k,
                None => continue,
            };
            // 仅处理 Song 实体类型（避免拿到 Album/Artist 等错误类型）
            if sid.entity_type != chordial_core::module::music_source::types::EntityType::Song {
                continue;
            }
            if sid.entity_id.is_empty() {
                continue;
            }
            // 通过 registrar 查找已注册的来源实现
            let source = match ctx.registrar.get(&sid.source_name) {
                Some(s) => s,
                None => continue, // 来源未注册（未启用），静默跳过
            };
            // 调用 get_lyric；失败时记录但不影响其他来源
            match source.get_lyric(&sid.entity_id) {
                Ok(Some(lyric)) if !lyric.text.is_empty() => {
                    candidates.push(LyricCandidate::new(kind, lyric.text));
                }
                Ok(_) => {} // None：来源中无歌词
                Err(e) => {
                    eprintln!(
                        "[lyric_get_preferred] 来源 {} 获取歌词失败 song_id={} entity_id={}: {}",
                        sid.source_name, song_id, sid.entity_id, e
                    );
                }
            }
        }
    }

    // 选最佳
    if let Some(best) = lyric_preference::select_best(&candidates, &pref) {
        Ok(serde_json::json!({
            "text": best.text,
            "format": best.format,
            "source": best.source,
            "is_word_by_word": best.format.is_word_by_word(),
            "candidates_count": candidates.len(),
        }))
    } else {
        Ok(serde_json::json!({
            "text": null,
            "format": null,
            "source": null,
            "candidates_count": 0,
        }))
    }
}