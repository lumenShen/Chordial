//! 本地匹配工具 — 供批量下载去重与音质比较复用。

use chordial_core::module::music_netease_source::AudioQuality;
use chordial_core::module::music_source::types::AudioQualityDescriptor;
use chordial_core::AppContext;
use std::sync::Arc;

/// 本地匹配项（用于去重比较）。
pub struct LocalMatch {
    pub file_path: Option<String>,
    pub quality: AudioQuality,
    pub quality_label: String,
}

/// 按标题+艺术家在本地库中查找匹配歌曲，返回每个匹配项的文件路径与推断音质。
///
/// 复用 `library.search_songs` 的模糊匹配能力，结果与 `source_compare_with_local`
/// 命令保持一致，但返回强类型 [`LocalMatch`] 便于后续比较。
pub fn find_local_matches(ctx: &Arc<AppContext>, title: &str, artist: &str) -> Vec<LocalMatch> {
    let title_lower = title.to_lowercase();
    let artist_lower = artist.to_lowercase();

    let candidates = ctx.library.search_songs(title);
    candidates
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
        .filter_map(|s| {
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
            Some(LocalMatch {
                file_path: local_path,
                quality_label: descriptor.label(),
                quality,
            })
        })
        .collect()
}

/// 从本地匹配项中选出音质最高的（rank 最大；同 rank 任选其一）。
pub fn best_local_quality(matches: &[LocalMatch]) -> Option<&LocalMatch> {
    matches
        .iter()
        .max_by(|a, b| a.quality.cmp(&b.quality))
}