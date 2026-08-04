//! 酷狗音质字符串与内部 [`AudioQuality`] 的映射。
//!
//! 酷狗 API 的 `quality` 参数取值：
//! - `"128"` → [`AudioQuality::Standard`]（≈128 kbps MP3）
//! - `"320"` → [`AudioQuality::Exhigh`]（≈320 kbps MP3）
//! - `"flac"` / `"high"` → [`AudioQuality::Lossless`]（FLAC 16bit/44.1kHz）
//! - `"super"` → [`AudioQuality::Hires`]（Hi-Res FLAC 24bit/96kHz+）

use crate::module::music_source::types::AudioQuality;

/// 默认音质字符串（酷狗 API 的 quality 参数）。
///
/// "320" 对应 [`AudioQuality::Exhigh`]，约 320 kbps MP3。
pub fn default_quality() -> &'static str {
    "320"
}

/// 酷狗音质字符串 → 内部 [`AudioQuality`] 枚举。
pub fn quality_from_kugou_str(s: &str) -> AudioQuality {
    match s.to_lowercase().as_str() {
        "128" => AudioQuality::Standard,
        "320" => AudioQuality::Exhigh,
        "flac" | "high" => AudioQuality::Lossless,
        "super" => AudioQuality::Hires,
        _ => AudioQuality::Unknown,
    }
}

/// 内部 [`AudioQuality`] → 酷狗音质字符串。
///
/// 反向映射，用于将用户偏好（如"无损"）转换为酷狗 API 的 `quality` 参数。
pub fn quality_to_kugou_str(q: AudioQuality) -> &'static str {
    match q {
        AudioQuality::Standard => "128",
        AudioQuality::Higher => "320", // 酷狗无 192 kbps，退化为 320
        AudioQuality::Exhigh => "320",
        AudioQuality::Lossless => "flac",
        AudioQuality::Hires => "super",
        AudioQuality::Unknown => "320",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_from_kugou_str() {
        assert_eq!(quality_from_kugou_str("128"), AudioQuality::Standard);
        assert_eq!(quality_from_kugou_str("320"), AudioQuality::Exhigh);
        assert_eq!(quality_from_kugou_str("flac"), AudioQuality::Lossless);
        assert_eq!(quality_from_kugou_str("high"), AudioQuality::Lossless);
        assert_eq!(quality_from_kugou_str("super"), AudioQuality::Hires);
        assert_eq!(quality_from_kugou_str("FLAC"), AudioQuality::Lossless);
        assert_eq!(quality_from_kugou_str("unknown"), AudioQuality::Unknown);
        assert_eq!(quality_from_kugou_str(""), AudioQuality::Unknown);
    }

    #[test]
    fn test_quality_to_kugou_str() {
        assert_eq!(quality_to_kugou_str(AudioQuality::Standard), "128");
        assert_eq!(quality_to_kugou_str(AudioQuality::Exhigh), "320");
        assert_eq!(quality_to_kugou_str(AudioQuality::Lossless), "flac");
        assert_eq!(quality_to_kugou_str(AudioQuality::Hires), "super");
        // Higher 酷狗无 192，退化为 320
        assert_eq!(quality_to_kugou_str(AudioQuality::Higher), "320");
    }
}