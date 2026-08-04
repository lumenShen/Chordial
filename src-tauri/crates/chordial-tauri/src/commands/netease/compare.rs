//! 网易云在线歌曲与本地库音质比较命令。

use chordial_core::module::music_netease_source::AudioQuality;
use chordial_core::module::music_source::types::AudioQualityDescriptor;
use chordial_core::AppContext;
use std::sync::Arc;
use tauri::State;

/// 比较 online 歌曲与本地库中相同歌曲的音质。
///
/// 通过 标题+艺术家 模糊匹配本地库，返回所有匹配项及其推断音质。
#[tauri::command]
pub fn netease_compare_with_local(
    ctx: State<'_, Arc<AppContext>>,
    title: String,
    artist: String,
    _album: Option<String>,
) -> Result<serde_json::Value, String> {
    let title_lower = title.to_lowercase();
    let artist_lower = artist.to_lowercase();

    let candidates = ctx.library.search_songs(&title);
    let matches: Vec<serde_json::Value> = candidates
        .into_iter()
        .filter(|s| s.title.to_lowercase().contains(&title_lower))
        .filter(|s| {
            if artist.is_empty() {
                return true;
            }
            s.artist_names
                .iter()
                .any(|a| a.to_lowercase().contains(&artist_lower))
        })
        .map(|s| {
            let local_path = s
                .source_ids
                .iter()
                .find(|sid| sid.source_name == "local")
                .map(|sid| sid.entity_id.clone());
            let descriptor = local_path
                .as_ref()
                .and_then(|p| std::path::Path::new(p).extension())
                .and_then(|e| e.to_str())
                .map(AudioQualityDescriptor::from_extension)
                .unwrap_or_else(AudioQualityDescriptor::unknown);
            let quality = AudioQuality::from(descriptor.rank);
            serde_json::json!({
                "song_id": s.id,
                "title": s.title,
                "artist_names": s.artist_names,
                "album_title": s.album_title,
                "file_path": local_path,
                "quality": quality.to_level_str(),
                "quality_rank": quality as u8,
                "quality_descriptor": descriptor,
                "quality_label": descriptor.label(),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "matches": matches,
        "has_local_match": !matches.is_empty(),
    }))
}