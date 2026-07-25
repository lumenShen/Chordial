use serde::{Deserialize, Serialize};

/// 实体类型 — 标记一个来源引用指向哪种音乐实体。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EntityType {
    Song,
    Artist,
    Album,
    Lyric,
}

/// 来源类型 — 区分本地来源与不同的网络来源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SourceType {
    /// 本地来源
    Local,
    /// 网络来源（携带来源名称，如 `"netease"`, `"spotify"`）
    Web(String),
}

impl SourceType {
    /// 是否为本地来源。
    pub fn is_local(&self) -> bool {
        matches!(self, SourceType::Local)
    }

    /// 是否为网络来源。
    pub fn is_web(&self) -> bool {
        matches!(self, SourceType::Web(_))
    }

    /// 网络来源的名称（本地来源返回 None）。
    pub fn web_name(&self) -> Option<&str> {
        match self {
            SourceType::Web(name) => Some(name),
            _ => None,
        }
    }
}

/// 来源标识 — 描述某个实体在特定来源中的身份。
///
/// 一个实体（如歌曲）可以同时拥有多个 [`SourceId`]，表示它来自多个来源。
///
/// # 示例
///
/// ```ignore
/// SourceId {
///     source_name: "my_local".into(),
///     source_type: SourceType::Local,
///     entity_type: EntityType::Song,
///     entity_id: "song_001".into(),
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SourceId {
    /// 已注册的来源名称，如 `"my_local"`, `"netease"`, `"spotify"`
    pub source_name: String,
    /// 来源类型
    pub source_type: SourceType,
    /// 实体类型
    pub entity_type: EntityType,
    /// 该来源内部的实体 ID
    pub entity_id: String,
}

impl SourceId {
    /// 创建一个新的 [`SourceId`]。
    pub fn new(
        source_name: impl Into<String>,
        source_type: SourceType,
        entity_type: EntityType,
        entity_id: impl Into<String>,
    ) -> Self {
        Self {
            source_name: source_name.into(),
            source_type,
            entity_type,
            entity_id: entity_id.into(),
        }
    }

    /// 创建一个实体类型不同的副本（保留来源名、来源类型和实体 ID）。
    ///
    /// 用于将歌曲的来源引用传播到其关联的艺人和专辑上。
    pub fn with_entity_type(&self, entity_type: EntityType) -> Self {
        Self {
            source_name: self.source_name.clone(),
            source_type: self.source_type.clone(),
            entity_type,
            entity_id: self.entity_id.clone(),
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// 音质评级 — 统一的音质描述（跨来源通用）
// ───────────────────────────────────────────────────────────────────────────

/// 音频编码格式 — 用于音质描述与显示。
///
/// 与 [`AudioQuality`] 不同，`AudioCodec` 描述的是「编码方式」而非「等级」，
/// 同一等级（如 `Lossless`）可能对应不同编码（FLAC / WAV / ALAC）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AudioCodec {
    /// 未知编码
    Unknown,
    /// MP3 (MPEG-1/2 Audio Layer III)
    Mp3,
    /// AAC / M4A (Advanced Audio Coding)
    Aac,
    /// FLAC (Free Lossless Audio Codec)
    Flac,
    /// ALAC (Apple Lossless)
    Alac,
    /// WAV / PCM (未压缩)
    Wav,
    /// APE (Monkey's Audio)
    Ape,
    /// Ogg Vorbis
    Vorbis,
    /// Opus
    Opus,
}

impl AudioCodec {
    /// 从文件扩展名推断编码格式。
    pub fn from_extension(ext: &str) -> Self {
        let lower = ext.to_lowercase();
        match lower.trim_start_matches('.') {
            "mp3" => Self::Mp3,
            "m4a" | "mp4" | "alac" => Self::Aac,
            "flac" => Self::Flac,
            "wav" | "wave" => Self::Wav,
            "ape" => Self::Ape,
            "ogg" | "oga" => Self::Vorbis,
            "opus" => Self::Opus,
            _ => Self::Unknown,
        }
    }

    /// 从 MIME 类型或 Content-Type 头推断编码格式。
    pub fn from_content_type(content_type: &str) -> Self {
        let s = content_type.to_lowercase();
        if s.contains("flac") {
            Self::Flac
        } else if s.contains("mpeg") || s.contains("mp3") {
            Self::Mp3
        } else if s.contains("mp4") || s.contains("aac") || s.contains("m4a") {
            Self::Aac
        } else if s.contains("wav") {
            Self::Wav
        } else if s.contains("ape") {
            Self::Ape
        } else if s.contains("ogg") {
            Self::Vorbis
        } else if s.contains("opus") {
            Self::Opus
        } else {
            Self::Unknown
        }
    }

    /// 人类可读的编码名称（如 "MP3", "FLAC"）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Unknown => "未知",
            Self::Mp3 => "MP3",
            Self::Aac => "AAC",
            Self::Flac => "FLAC",
            Self::Alac => "ALAC",
            Self::Wav => "WAV",
            Self::Ape => "APE",
            Self::Vorbis => "Vorbis",
            Self::Opus => "Opus",
        }
    }

    /// 是否为无损编码。
    pub fn is_lossless(self) -> bool {
        matches!(self, Self::Flac | Self::Alac | Self::Wav | Self::Ape)
    }
}

impl Default for AudioCodec {
    fn default() -> Self {
        Self::Unknown
    }
}

/// 音质等级 — 跨来源通用的音质评级。
///
/// 不同云音乐源对音质有不同的等级命名（如网易云的 `exhigh`），
/// 本枚举将其归一化为 6 个等级，便于在本地/在线间进行有序比较。
///
/// 顺序由低到高，便于通过 `as u8` 直接比较。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum AudioQuality {
    /// 未知 / 无可用音源
    Unknown = 0,
    /// 标准音质（≈128 kbps 有损）
    Standard = 1,
    /// 较高音质（≈192 kbps 有损）
    Higher = 2,
    /// 极高音质（≈320 kbps 有损）
    Exhigh = 3,
    /// 无损音质（FLAC 16bit/44.1kHz）
    Lossless = 4,
    /// Hi-Res（FLAC 24bit/96kHz+）
    Hires = 5,
}

impl Default for AudioQuality {
    fn default() -> Self {
        Self::Unknown
    }
}

impl From<u8> for AudioQuality {
    /// 从 u8 rank 还原 [`AudioQuality`]。
    ///
    /// 越界值（>5）映射为 [`AudioQuality::Unknown`]。
    fn from(rank: u8) -> Self {
        match rank {
            1 => Self::Standard,
            2 => Self::Higher,
            3 => Self::Exhigh,
            4 => Self::Lossless,
            5 => Self::Hires,
            _ => Self::Unknown,
        }
    }
}

impl AudioQuality {
    /// 从网易云 API 的 `level` 字符串解析。
    ///
    /// 兼容网易云的 `standard` / `higher` / `exhigh` / `lossless` / `hires` 命名。
    pub fn from_level_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "standard" => Self::Standard,
            "higher" => Self::Higher,
            "exhigh" | "exhighs" => Self::Exhigh,
            "lossless" => Self::Lossless,
            "hires" => Self::Hires,
            _ => Self::Unknown,
        }
    }

    /// 转换为网易云 API 的 `level` 字符串。
    pub fn to_level_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Higher => "higher",
            Self::Exhigh => "exhigh",
            Self::Lossless => "lossless",
            Self::Hires => "hires",
            Self::Unknown => "standard",
        }
    }

    /// 从文件扩展名推断音质（粗略）。
    ///
    /// `.flac` → Lossless，`.wav` / `.ape` → Hires，`.mp3` / `.m4a` / `.aac` → Exhigh（假设 320 kbps）。
    /// 真实音质需要读取文件元数据。
    pub fn from_extension(ext: &str) -> Self {
        let lower = ext.to_lowercase();
        match lower.trim_start_matches('.') {
            "flac" => Self::Lossless,
            "wav" | "ape" => Self::Hires,
            "mp3" | "m4a" | "aac" => Self::Exhigh,
            _ => Self::Unknown,
        }
    }

    /// 是否为无损及以上音质。
    pub fn is_lossless_or_above(self) -> bool {
        self >= Self::Lossless
    }

    /// 典型码率（kbps）。
    ///
    /// 仅作展示用，实际码率以 [`AudioQualityDescriptor::bitrate_kbps`] 为准。
    pub fn typical_bitrate_kbps(self) -> u32 {
        match self {
            Self::Standard => 128,
            Self::Higher => 192,
            Self::Exhigh => 320,
            Self::Lossless => 900, // FLAC ≈ 900 kbps（16bit/44.1kHz）
            Self::Hires => 2300,   // Hi-Res FLAC ≈ 2300 kbps（24bit/96kHz）
            Self::Unknown => 0,
        }
    }

    /// 典型编码格式。
    ///
    /// 有损等级 → MP3 / AAC，无损等级 → FLAC。
    pub fn typical_codec(self) -> AudioCodec {
        match self {
            Self::Standard | Self::Higher | Self::Exhigh => AudioCodec::Mp3,
            Self::Lossless | Self::Hires => AudioCodec::Flac,
            Self::Unknown => AudioCodec::Unknown,
        }
    }

    /// 典型采样率（Hz）。
    pub fn typical_sample_rate_hz(self) -> Option<u32> {
        match self {
            Self::Lossless => Some(44_100),
            Self::Hires => Some(96_000),
            _ => None,
        }
    }

    /// 典型位深。
    pub fn typical_bit_depth(self) -> Option<u8> {
        match self {
            Self::Lossless => Some(16),
            Self::Hires => Some(24),
            _ => None,
        }
    }

    /// 返回该等级的典型音质描述符（用于 UI 显示与比较）。
    ///
    /// 若实际下载时获取到了真实码率/格式，应优先使用 [`AudioQualityDescriptor::new`] 构造。
    pub fn descriptor(self) -> AudioQualityDescriptor {
        AudioQualityDescriptor {
            rank: self as u8,
            bitrate_kbps: self.typical_bitrate_kbps(),
            codec: self.typical_codec(),
            sample_rate_hz: self.typical_sample_rate_hz(),
            bit_depth: self.typical_bit_depth(),
        }
    }
}

/// 音质描述符 — 用于 UI 显示与跨来源比较。
///
/// 包含具体的音频参数（码率、编码格式、采样率、位深）以及一个归一化的 [`AudioQuality`] 等级。
/// 前端在显示「音质」时，应优先使用 [`Self::label`] 生成的描述字符串，
/// 而非简单显示数字 rank。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioQualityDescriptor {
    /// 归一化等级 rank（0-5，对应 [`AudioQuality`] 的 `as u8`）
    #[serde(default)]
    pub rank: u8,
    /// 码率（kbps）。无损格式时此值为 0（实际由采样率×位深决定）。
    #[serde(default)]
    pub bitrate_kbps: u32,
    /// 编码格式
    #[serde(default)]
    pub codec: AudioCodec,
    /// 采样率（Hz）。仅无损 / Hi-Res 时有意义。
    #[serde(default)]
    pub sample_rate_hz: Option<u32>,
    /// 位深。仅无损 / Hi-Res 时有意义。
    #[serde(default)]
    pub bit_depth: Option<u8>,
}

impl Default for AudioQualityDescriptor {
    fn default() -> Self {
        AudioQuality::Unknown.descriptor()
    }
}

impl AudioQualityDescriptor {
    /// 构造一个未知音质的描述符（用于无法确定音质的情况）。
    pub fn unknown() -> Self {
        AudioQuality::Unknown.descriptor()
    }

    /// 从等级构造描述符（使用典型参数）。
    pub fn from_quality(quality: AudioQuality) -> Self {
        quality.descriptor()
    }

    /// 从文件扩展名构造描述符（粗略推断）。
    pub fn from_extension(ext: &str) -> Self {
        let quality = AudioQuality::from_extension(ext);
        let codec = AudioCodec::from_extension(ext);
        // 若扩展名未匹配到等级（如 ogg/opus），但能识别 codec，
        // 仍返回有 codec 信息的描述符
        if quality == AudioQuality::Unknown && codec != AudioCodec::Unknown {
            Self {
                rank: AudioQuality::Standard as u8,
                bitrate_kbps: 0,
                codec,
                sample_rate_hz: None,
                bit_depth: None,
            }
        } else {
            quality.descriptor()
        }
    }

    /// 生成人类可读的音质标签。
    ///
    /// # 返回示例
    /// - `"320 kbps MP3"` — 极高音质
    /// - `"FLAC 16bit/44.1kHz"` — 无损
    /// - `"FLAC 24bit/96kHz"` — Hi-Res
    /// - `"未知"` — 无法识别
    pub fn label(&self) -> String {
        if self.codec.is_lossless() {
            // 无损格式：显示 FLAC 16bit/44.1kHz
            let codec_name = self.codec.display_name();
            let bit_depth = self.bit_depth.unwrap_or(16);
            let sample_rate = self.sample_rate_hz.unwrap_or(44_100);
            // 采样率换算为 kHz 显示
            let sr_khz = (sample_rate as f64 / 1000.0).round() as u32;
            format!("{} {}/{}kHz", codec_name, bit_depth, sr_khz)
        } else if self.bitrate_kbps > 0 {
            format!("{} kbps {}", self.bitrate_kbps, self.codec.display_name())
        } else if self.codec != AudioCodec::Unknown {
            self.codec.display_name().to_string()
        } else {
            "未知".to_string()
        }
    }

    /// 简短等级名称（用于标签、徽章等紧凑 UI）。
    pub fn tier_name(&self) -> &'static str {
        match self.rank {
            0 => "未知",
            1 => "标准",
            2 => "较高",
            3 => "极高",
            4 => "无损",
            5 => "Hi-Res",
            _ => "未知",
        }
    }

    /// 是否为高音质（rank ≥ 4，即无损及以上）。
    pub fn is_high_quality(&self) -> bool {
        self.rank >= 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_quality_ordering() {
        assert!(AudioQuality::Lossless > AudioQuality::Exhigh);
        assert!(AudioQuality::Hires > AudioQuality::Lossless);
        assert!(AudioQuality::Standard < AudioQuality::Higher);
    }

    #[test]
    fn test_audio_quality_from_extension() {
        assert_eq!(AudioQuality::from_extension("flac"), AudioQuality::Lossless);
        assert_eq!(AudioQuality::from_extension("MP3"), AudioQuality::Exhigh);
        assert_eq!(AudioQuality::from_extension("wav"), AudioQuality::Hires);
    }

    #[test]
    fn test_audio_codec_from_extension() {
        assert_eq!(AudioCodec::from_extension("flac"), AudioCodec::Flac);
        assert_eq!(AudioCodec::from_extension("mp3"), AudioCodec::Mp3);
        assert_eq!(AudioCodec::from_extension("m4a"), AudioCodec::Aac);
        assert_eq!(AudioCodec::from_extension("opus"), AudioCodec::Opus);
    }

    #[test]
    fn test_descriptor_label_lossless() {
        let d = AudioQuality::Lossless.descriptor();
        assert_eq!(d.label(), "FLAC 16/44kHz");
    }

    #[test]
    fn test_descriptor_label_hires() {
        let d = AudioQuality::Hires.descriptor();
        assert_eq!(d.label(), "FLAC 24/96kHz");
    }

    #[test]
    fn test_descriptor_label_exhigh() {
        let d = AudioQuality::Exhigh.descriptor();
        assert_eq!(d.label(), "320 kbps MP3");
    }

    #[test]
    fn test_descriptor_label_unknown() {
        let d = AudioQualityDescriptor::unknown();
        assert_eq!(d.label(), "未知");
    }

    #[test]
    fn test_descriptor_from_extension_ogg() {
        // ogg 能识别 codec 但不在 AudioQuality 等级中
        let d = AudioQualityDescriptor::from_extension("ogg");
        assert_eq!(d.codec, AudioCodec::Vorbis);
        assert_eq!(d.bitrate_kbps, 0);
    }
}
