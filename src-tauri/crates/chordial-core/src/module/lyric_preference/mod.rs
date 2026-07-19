//! 歌词优先级系统 — 按来源和格式对多个候选歌词排序，选出最佳歌词。
//!
//! # 设计目标
//!
//! 当一首歌同时存在多个来源的歌词（如本地嵌入 LRC + 网易云 YRC）时，
//! 需要按用户偏好选出"最佳"歌词用于显示。
//!
//! # 排序维度
//!
//! 1. **格式**：逐字歌词（YRC/QRC/TTML）优先于行级歌词（LRC），LRC 优先于纯文本
//! 2. **来源**：按用户配置的来源顺序（默认：本地 → 网易云）
//!
//! 用户可在设置页中调整优先级顺序。
//!
//! # 持久化
//!
//! 优先级配置通过 [`ConfigStore`] 持久化到 `config.json` 的 `lyric_preference` key。

use serde::{Deserialize, Serialize};

/// 歌词格式 — 决定显示效果（逐字 vs 行级 vs 纯文本）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum LyricFormat {
    /// 逐字歌词（网易云 YRC / QQ 音乐 QRC / TTML）
    /// 字级时间戳，支持 AMLL 风格的字符渐变
    Yrc,
    /// QQ 音乐逐字歌词
    Qrc,
    /// Apple Music TTML 格式
    Ttml,
    /// 行级 LRC 歌词（最常见）
    Lrc,
    /// 纯文本歌词（无时间戳）
    Plain,
}

impl LyricFormat {
    /// 从歌词文本启发式检测格式。
    ///
    /// - 包含 `[ti:`/`[ar:`/`[al:` 等 LRC 元数据 → Lrc
    /// - 包含 `[1023,5000]`（YRC 时间戳格式）→ Yrc
    /// - 包含 `<0,500,0>word`（QRC 字级标签）→ Qrc
    /// - 包含 `<p begin="...">`（TTML 标签）→ Ttml
    /// - 其他 → Plain
    pub fn detect(text: &str) -> Self {
        if text.contains("<p begin=") || text.contains("<?xml") {
            return Self::Ttml;
        }
        // YRC: 行首形如 [1234,5678](0,1,0)word
        if text.lines().any(|l| {
            l.trim_start()
                .starts_with('[')
                && l.contains("](")
        }) {
            return Self::Yrc;
        }
        // QRC: 形如 [1234,5000]word<0,500,0>...
        if text.contains("<") && text.contains(">") && text.contains(",") {
            return Self::Qrc;
        }
        // LRC: 行首形如 [mm:ss.xx]
        if text.lines().any(|l| {
            let t = l.trim_start();
            t.starts_with('[')
                && t.len() >= 10
                && t.as_bytes().get(1).map_or(false, |c| c.is_ascii_digit())
        }) {
            return Self::Lrc;
        }
        Self::Plain
    }

    /// 是否为逐字格式。
    pub fn is_word_by_word(self) -> bool {
        matches!(self, Self::Yrc | Self::Qrc | Self::Ttml)
    }

    /// 是否为同步格式（含时间戳）。
    pub fn is_synced(self) -> bool {
        !matches!(self, Self::Plain)
    }

    /// 默认优先级权重（数字越大越优先）。
    pub fn default_weight(self) -> u8 {
        match self {
            Self::Ttml => 50,
            Self::Yrc => 45,
            Self::Qrc => 40,
            Self::Lrc => 20,
            Self::Plain => 0,
        }
    }
}

/// 歌词来源类型 — 与 [`crate::module::music_source::types::SourceType`] 对应，
/// 但仅保留与歌词相关的来源。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum LyricSourceKind {
    /// 本地来源（音频嵌入歌词或同目录 .lrc 文件）
    Local,
    /// 网易云音乐
    Netease,
}

impl LyricSourceKind {
    /// 从来源名称解析。
    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "local" => Some(Self::Local),
            "netease" => Some(Self::Netease),
            _ => None,
        }
    }

    /// 默认优先级权重（数字越大越优先）。
    pub fn default_weight(self) -> u8 {
        match self {
            Self::Local => 10,
            Self::Netease => 5,
        }
    }
}

/// 歌词优先级配置 — 用户可调整的来源/格式顺序。
///
/// 持久化为 `config.json` 的 `lyric_preference` key。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricPreference {
    /// 来源优先级顺序（从前到后）。
    /// 默认：`[Local, Netease]`
    #[serde(default = "default_source_order")]
    pub source_order: Vec<LyricSourceKind>,

    /// 是否优先逐字歌词（YRC/QRC/TTML）。
    /// true: 同一来源中，逐字 > LRC > 纯文本
    /// false: 仅按 source_order 排序，格式不参与
    #[serde(default = "default_prefer_word_by_word")]
    pub prefer_word_by_word: bool,

    /// 是否在本地有逐字歌词时仍尝试从在线获取（用于本地 LRC 升级到 YRC）。
    #[serde(default)]
    pub fallback_to_online_for_word_by_word: bool,
}

fn default_source_order() -> Vec<LyricSourceKind> {
    vec![LyricSourceKind::Local, LyricSourceKind::Netease]
}

fn default_prefer_word_by_word() -> bool {
    true
}

impl Default for LyricPreference {
    fn default() -> Self {
        Self {
            source_order: default_source_order(),
            prefer_word_by_word: default_prefer_word_by_word(),
            fallback_to_online_for_word_by_word: false,
        }
    }
}

impl LyricPreference {
    /// 从 ConfigStore 加载配置。
    pub fn load(config: &crate::module::config::store::ConfigStore) -> Self {
        config
            .get::<Self>("lyric_preference")
            .unwrap_or_default()
    }

    /// 保存到 ConfigStore（自动防抖落盘）。
    pub fn save(&self, config: &crate::module::config::store::ConfigStore) -> Result<(), String> {
        config.set("lyric_preference", self)?;
        Ok(())
    }
}

/// 歌词候选条目 — 用于排序的中间结构。
#[derive(Debug, Clone)]
pub struct LyricCandidate {
    /// 来源类型
    pub source: LyricSourceKind,
    /// 歌词文本
    pub text: String,
    /// 自动检测的格式
    pub format: LyricFormat,
}

impl LyricCandidate {
    /// 从文本创建候选（自动检测格式）。
    pub fn new(source: LyricSourceKind, text: String) -> Self {
        let format = LyricFormat::detect(&text);
        Self { source, text, format }
    }

    /// 计算综合优先级分数（数字越大越优先）。
    ///
    /// 分数 = source_rank * 100 + format_weight
    /// - source_rank: 在 source_order 中的位置（首位=最大），未配置则用 default_weight
    /// - format_weight: 由 `prefer_word_by_word` 控制是否计入
    pub fn score(&self, pref: &LyricPreference) -> i32 {
        let source_rank = pref
            .source_order
            .iter()
            .position(|s| *s == self.source)
            // 未在 source_order 中：用默认权重减去 100（强制排在所有显式配置之后）
            .map(|p| (pref.source_order.len() - p) as i32)
            .unwrap_or_else(|| self.source.default_weight() as i32 - 100);

        let format_weight = if pref.prefer_word_by_word {
            self.format.default_weight() as i32
        } else {
            0
        };

        source_rank * 100 + format_weight
    }
}

/// 从多个候选歌词中选出最佳的一条。
///
/// # 参数
/// - `candidates`: 候选歌词列表（来源 + 文本）
/// - `pref`: 用户偏好配置
///
/// # 返回
/// 综合得分最高的候选。`candidates` 为空返回 `None`。
pub fn select_best<'a>(
    candidates: &'a [LyricCandidate],
    pref: &LyricPreference,
) -> Option<&'a LyricCandidate> {
    candidates
        .iter()
        .max_by_key(|c| c.score(pref))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_lrc() {
        let text = "[00:01.00]Hello world\n[00:05.00]Second line";
        assert_eq!(LyricFormat::detect(text), LyricFormat::Lrc);
    }

    #[test]
    fn test_detect_yrc() {
        let text = "[1234,5000](0,1,0)Hello\n[6234,5000](0,1,0)world";
        assert_eq!(LyricFormat::detect(text), LyricFormat::Yrc);
    }

    #[test]
    fn test_detect_plain() {
        let text = "Hello world\nThis is plain text";
        assert_eq!(LyricFormat::detect(text), LyricFormat::Plain);
    }

    #[test]
    fn test_format_is_word_by_word() {
        assert!(LyricFormat::Yrc.is_word_by_word());
        assert!(LyricFormat::Qrc.is_word_by_word());
        assert!(LyricFormat::Ttml.is_word_by_word());
        assert!(!LyricFormat::Lrc.is_word_by_word());
        assert!(!LyricFormat::Plain.is_word_by_word());
    }

    #[test]
    fn test_prefer_word_by_word_local() {
        let pref = LyricPreference {
            source_order: vec![LyricSourceKind::Local, LyricSourceKind::Netease],
            prefer_word_by_word: true,
            fallback_to_online_for_word_by_word: false,
        };
        let local_lrc = LyricCandidate::new(LyricSourceKind::Local, "[00:01.00]hi".to_string());
        let netease_yrc = LyricCandidate::new(
            LyricSourceKind::Netease,
            "[1000,2000](0,1,0)hi".to_string(),
        );
        // Local 优先级更高（source_rank=2*100），即使 Netease 是逐字
        // 但 YRC 格式权重 45 > LRC 20，所以差距缩小但仍 Local 领先
        let best = select_best(&[local_lrc.clone(), netease_yrc], &pref);
        assert_eq!(best.unwrap().source, LyricSourceKind::Local);
    }

    #[test]
    fn test_prefer_word_by_word_same_source() {
        let pref = LyricPreference::default();
        // 同一来源的两个候选，逐字应优先
        let lrc = LyricCandidate::new(LyricSourceKind::Local, "[00:01.00]hi".to_string());
        let yrc = LyricCandidate::new(LyricSourceKind::Local, "[1000,2000](0,1,0)hi".to_string());
        let best = select_best(&[lrc, yrc], &pref);
        assert_eq!(best.unwrap().format, LyricFormat::Yrc);
    }

    #[test]
    fn test_empty_candidates() {
        let pref = LyricPreference::default();
        assert!(select_best(&[], &pref).is_none());
    }
}
