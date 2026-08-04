//! 诊断命令 — 歌词问题排查。

use chordial_core::module::platform::{self, PlatformPath};
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

/// 诊断命令：检查指定歌曲的歌词状态。
///
/// 用于排查"部分歌曲歌词无法显示"问题。返回：
/// - `song`: 库中的 Song 实体（含 source_ids、lyric_id 等）
/// - `lyric`: 库中通过 song_id 查到的 Lyric 实体（可能为 null）
/// - `file_path`: 实际音频文件路径
/// - `file_exists`: 文件是否存在
/// - `external_lyric_file`: 同目录 .lrc/.txt 文件内容预览（若存在）
/// - `raw_tags`: symphonia 探测到的所有原始标签键值对
/// - `embedded_lyrics_in_meta`: scanner.probe_file 提取出的嵌入歌词预览
///
/// 前端可通过此命令一次性看到：
///   1. 库里是否真的有 Lyric 实体（lyric 字段）
///   2. 若没有，是因为外部歌词文件缺失 + 嵌入歌词也没被提取（看 raw_tags 里是否有歌词键）
///   3. 嵌入歌词的 raw tag key 实际叫什么（可能不在我们的匹配列表里）
#[tauri::command]
pub fn debug_inspect_song_lyrics(
    ctx: State<'_, Arc<AppContext>>,
    song_id: String,
) -> Result<serde_json::Value, String> {
    use chordial_core::module::music_localSource::scanner;

    // 1. 查 Song 实体
    let song = ctx.library.get_song(&song_id);
    let song_json = serde_json::to_value(&song)
        .map_err(|e| format!("序列化 song 失败: {}", e))?;

    // 2. 查 Lyric 实体（按 song_id 索引）
    let lyric = ctx.library.get_lyric_of_song(&song_id);
    let lyric_json = serde_json::to_value(&lyric)
        .map_err(|e| format!("序列化 lyric 失败: {}", e))?;

    // 3. 找到实际音频文件路径
    let path_str = ctx
        .local_source
        .id_to_path
        .read()
        .get(&song_id)
        .map(|p| platform::path_to_string(p))
        .or_else(|| {
            // fallback：从 song.source_ids 找 local 来源的 entity_id
            song.as_ref().and_then(|s| {
                s.source_ids
                    .iter()
                    .find(|sid| sid.source_name == "local")
                    .map(|sid| sid.entity_id.clone())
            })
        });

    let Some(path_str) = path_str else {
        return Ok(serde_json::json!({
            "song": song_json,
            "lyric": lyric_json,
            "file_path": null,
            "error": "无法定位音频文件路径（song_id 既不在 id_to_path，也无 local source_id）",
        }));
    };

    let path = PlatformPath::from(path_str.as_str());
    let file_exists = platform::exists(&path);

    // 4. 外部 .lrc/.txt 文件
    let external_lyric = if file_exists {
        scanner::read_lyric_file(&path)
    } else {
        None
    };
    let external_lyric_preview = external_lyric
        .as_ref()
        .map(|t| {
            if t.len() > 300 {
                format!("{}...(len={})", &t[..300], t.len())
            } else {
                t.clone()
            }
        });

    // 5. 列出所有原始标签
    let raw_tags: Vec<(String, String)> = if file_exists {
        scanner::list_raw_tags(&path).unwrap_or_else(|e| vec![("<error>".to_string(), e)])
    } else {
        vec![]
    };

    // 6. 提取嵌入歌词（与扫描时逻辑一致）
    let embedded_lyrics_in_meta = if file_exists {
        scanner::probe_file(&path)
            .ok()
            .and_then(|m| m.lyrics)
            .map(|t| {
                if t.len() > 300 {
                    format!("{}...(len={})", &t[..300], t.len())
                } else {
                    t
                }
            })
    } else {
        None
    };

    Ok(serde_json::json!({
        "song": song_json,
        "lyric": lyric_json,
        "file_path": path_str,
        "file_exists": file_exists,
        "external_lyric_file": external_lyric_preview,
        "embedded_lyrics_in_meta": embedded_lyrics_in_meta,
        "raw_tags": raw_tags.into_iter().map(|(k, v)| {
            serde_json::json!({ "key": k, "value": v })
        }).collect::<Vec<_>>(),
    }))
}