//! 均衡器的 UI 状态、预设目录与保存的个人配置。
//!
//! 预设只是 [`EqBand`] 列表的静态模板；加载后仍可自由编辑。内置预设、
//! V4 配置与 AutoEQ 耳机档案都走同一套数据结构，因此「另存为」得到的
//! 个人配置与内置项在 UI 上完全同权。

use serde::{Deserialize, Serialize};

use chordal_player::{EQ_MAX_BANDS, EqBand, EqSettings, GRAPHIC_10_BANDS, fmt_db, fmt_freq};

use crate::autoeq::AutoEqLibrary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EqTab {
    #[default]
    Edit,
    V4,
    AutoEq,
    Builtin,
    Personal,
}

impl EqTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Edit => "编辑",
            Self::V4 => "V4",
            Self::AutoEq => "AutoEQ",
            Self::Builtin => "内置",
            Self::Personal => "个人",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedEqPreset {
    pub name: String,
    #[serde(default)]
    pub settings: EqSettings,
}

/// 一条静态预设。`bands` 是 `(Hz, dB, Q)`，全部为峰值滤波器。
#[derive(Debug, Clone, Copy)]
pub struct PresetEntry {
    pub name: &'static str,
    /// 来源/作者（内置项为空）。
    pub source: &'static str,
    /// AutoEQ 分类：头戴式 / 入耳式 / 耳塞式；非耳机档案为空。
    pub category: &'static str,
    pub bands: &'static [(f32, f32, f32)],
    pub input_gain_db: f32,
    pub output_gain_db: f32,
}

impl PresetEntry {
    pub fn to_settings(&self) -> EqSettings {
        EqSettings {
            enabled: true,
            input_gain_db: self.input_gain_db,
            output_gain_db: self.output_gain_db,
            bands: self
                .bands
                .iter()
                .map(|&(freq, gain, q)| EqBand::new(freq, gain, q))
                .collect(),
        }
        .sanitized()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} 个滤波器 · {}",
            self.bands.len(),
            fmt_db(self.input_gain_db)
        )
    }

    pub fn subtitle(&self) -> String {
        if self.category.is_empty() {
            self.summary()
        } else if self.source.is_empty() {
            self.category.to_string()
        } else {
            format!("{} {}", self.source, self.category)
        }
    }
}

#[derive(Debug, Clone)]
pub struct EqState {
    pub settings: EqSettings,
    pub preset_name: String,
    pub dirty: bool,
    pub selected: Option<usize>,
    pub tab: EqTab,
    pub saved: Vec<SavedEqPreset>,
    pub search: String,
    /// AutoEQ 分类筛选（全部 / 头戴式 / 入耳式 / 耳塞式）。
    pub category: String,
    pub type_menu_open: bool,
    /// 桌面端预设浏览的预览项（不立即生效，右侧先看曲线）。
    pub preview: Option<PresetEntry>,
    /// 个人配置的预览索引。
    pub preview_saved: Option<usize>,
    /// 从 AutoEq 仓库拉取的耳机档案（索引常驻，曲线按需加载）。
    pub autoeq: AutoEqLibrary,
    /// AutoEQ 当前预览的型号（仓库内文件路径，跨索引刷新仍有效）。
    pub autoeq_preview: Option<String>,
}

impl EqState {
    pub fn new(settings: EqSettings, saved: Vec<SavedEqPreset>) -> Self {
        let settings = settings.sanitized();
        let selected = (!settings.bands.is_empty()).then_some(0);
        let untouched = settings.bands.is_empty()
            && settings.input_gain_db.abs() < 0.05
            && settings.output_gain_db.abs() < 0.05;
        Self {
            settings,
            preset_name: if untouched {
                "Original Sound".into()
            } else {
                "自定义曲线".to_string()
            },
            dirty: false,
            selected,
            tab: EqTab::Edit,
            saved,
            search: String::new(),
            category: "全部".into(),
            type_menu_open: false,
            preview: None,
            preview_saved: None,
            autoeq: AutoEqLibrary::default(),
            autoeq_preview: None,
        }
    }

    pub fn display_name(&self) -> String {
        if self.dirty {
            format!("{}（已修改）", self.preset_name)
        } else {
            self.preset_name.clone()
        }
    }

    pub fn selected_band_mut(&mut self) -> Option<&mut EqBand> {
        let index = self.selected?;
        self.settings.bands.get_mut(index)
    }

    pub fn touch(&mut self) {
        self.dirty = true;
    }

    /// 添加一个频段：优先 1 kHz，其次标准 10 段的中心频率。
    pub fn add_band(&mut self) -> Option<usize> {
        if self.settings.bands.len() >= EQ_MAX_BANDS {
            return None;
        }
        let freq = GRAPHIC_10_BANDS
            .iter()
            .copied()
            .chain([60.0, 120.0, 240.0, 480.0, 960.0, 3_000.0, 6_000.0, 12_000.0])
            .chain([1_000.0])
            .find(|f| {
                !self
                    .settings
                    .bands
                    .iter()
                    .any(|b| (b.freq_hz - f).abs() / f.max(1.0) < 0.02)
            })
            .unwrap_or(1_000.0);
        self.settings.bands.push(EqBand::new(freq, 0.0, 0.71));
        let index = self.settings.bands.len() - 1;
        self.selected = Some(index);
        self.dirty = true;
        Some(index)
    }

    pub fn remove_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        if index < self.settings.bands.len() {
            self.settings.bands.remove(index);
            self.selected = if self.settings.bands.is_empty() {
                None
            } else {
                Some(index.saturating_sub(1).min(self.settings.bands.len() - 1))
            };
            self.dirty = true;
        }
    }

    pub fn load_preset(&mut self, preset: &PresetEntry) {
        self.load_settings(preset.name.to_string(), preset.to_settings());
        self.preview = Some(*preset);
        self.preview_saved = None;
    }

    /// AutoEQ 等动态来源：只有名称与设置，没有静态 `PresetEntry`。
    pub fn load_settings(&mut self, name: String, settings: EqSettings) {
        self.settings = settings.sanitized();
        self.preset_name = name;
        self.dirty = false;
        self.selected = (!self.settings.bands.is_empty()).then_some(0);
        self.type_menu_open = false;
        self.preview = None;
        self.preview_saved = None;
    }

    pub fn load_saved(&mut self, preset: &SavedEqPreset) {
        self.settings = preset.settings.clone().sanitized();
        self.preset_name = preset.name.clone();
        self.dirty = false;
        self.selected = (!self.settings.bands.is_empty()).then_some(0);
    }

    /// 桌面端预设浏览器：只切换预览，不改动当前配置。
    pub fn preview_preset(&mut self, preset: PresetEntry) {
        self.preview = Some(preset);
        self.preview_saved = None;
    }

    pub fn preview_saved(&mut self, index: usize) {
        self.preview_saved = Some(index);
        self.preview = None;
    }

    /// 另存为个人配置；返回新名称。
    pub fn save_as(&mut self) -> String {
        let n = self.saved.len() + 1;
        let mut name = format!("自定义 {n}");
        let mut suffix = 2;
        while self.saved.iter().any(|p| p.name == name) {
            name = format!("自定义 {n}-{suffix}");
            suffix += 1;
        }
        self.saved.push(SavedEqPreset {
            name: name.clone(),
            settings: self.settings.clone(),
        });
        self.preset_name = name.clone();
        self.dirty = false;
        name
    }
}

/// 把界面的 0..1 滑杆值转换为频段参数。
pub fn slider_to_freq(value: f32) -> f32 {
    chordal_player::norm_to_freq(value)
}

pub fn freq_to_slider(freq: f32) -> f32 {
    chordal_player::freq_to_norm(freq)
}

pub fn q_to_slider(q: f32) -> f32 {
    let lo = chordal_player::EQ_MIN_Q.ln();
    let hi = chordal_player::EQ_MAX_Q.ln();
    ((q.clamp(chordal_player::EQ_MIN_Q, chordal_player::EQ_MAX_Q)
        .ln()
        - lo)
        / (hi - lo))
        .clamp(0.0, 1.0)
}

pub fn slider_to_q(value: f32) -> f32 {
    let lo = chordal_player::EQ_MIN_Q.ln();
    let hi = chordal_player::EQ_MAX_Q.ln();
    ((hi - lo) * value.clamp(0.0, 1.0) + lo).exp()
}

/// 频段的一行参数摘要（选择卡片下方/提示用）。
pub fn band_summary(index: usize, band: &EqBand) -> String {
    format!(
        "{} {} {} Q {:.2}",
        index + 1,
        fmt_freq(band.freq_hz),
        fmt_db(band.gain_db),
        band.q
    )
}

/// 频响图的坐标映射：20Hz..20kHz 对数横轴，+24..-24 dB 纵轴。
#[derive(Debug, Clone, Copy)]
pub struct GraphGeom {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl GraphGeom {
    pub fn new(width: f32, height: f32) -> Self {
        Self::with_margins(width, height, 38.0, 18.0, 12.0, 24.0)
    }

    /// 紧凑布局：左侧只留 dB 标尺，底部留频率标签。
    pub fn compact(width: f32, height: f32) -> Self {
        Self::with_margins(width, height, 30.0, 18.0, 10.0, 22.0)
    }

    pub fn with_margins(
        width: f32,
        height: f32,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
    ) -> Self {
        Self {
            left,
            top,
            width: (width - left - right).max(1.0),
            height: (height - top - bottom).max(1.0),
        }
    }

    pub fn x(&self, freq_hz: f32) -> f32 {
        self.left + self.width * chordal_player::freq_to_norm(freq_hz)
    }

    pub fn y(&self, db: f32) -> f32 {
        let norm = ((24.0 - db.clamp(-24.0, 24.0)) / 48.0).clamp(0.0, 1.0);
        self.top + self.height * norm
    }

    pub fn freq_at(&self, x: f32) -> f32 {
        let norm = ((x - self.left) / self.width).clamp(0.0, 1.0);
        chordal_player::norm_to_freq(norm)
    }

    pub fn db_at(&self, y: f32) -> f32 {
        let norm = ((y - self.top) / self.height).clamp(0.0, 1.0);
        24.0 - norm * 48.0
    }
}

pub fn builtin_presets() -> Vec<PresetEntry> {
    vec![
        PresetEntry {
            name: "原声",
            source: "",
            category: "",
            bands: &[],
            input_gain_db: 0.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "录音室参考",
            source: "",
            category: "",
            bands: &[
                (120.0, -2.0, 1.0),
                (400.0, -1.0, 1.0),
                (2_500.0, 1.0, 1.2),
                (8_000.0, -1.0, 0.8),
            ],
            input_gain_db: -2.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "混音校准",
            source: "",
            category: "",
            bands: &[
                (60.0, 1.5, 0.9),
                (200.0, -2.0, 1.0),
                (800.0, -1.2, 1.1),
                (3_000.0, 1.5, 1.0),
                (10_000.0, -1.0, 0.7),
            ],
            input_gain_db: -2.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "低中频清理",
            source: "",
            category: "",
            bands: &[
                (180.0, -1.2, 1.2),
                (320.0, -1.5, 1.0),
                (650.0, -1.5, 1.2),
                (2_000.0, 1.0, 1.0),
            ],
            input_gain_db: -1.5,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "柔顺高频",
            source: "",
            category: "",
            bands: &[
                (4_000.0, -1.5, 0.8),
                (6_300.0, -2.0, 0.9),
                (10_000.0, -2.5, 0.8),
                (16_000.0, -2.0, 0.7),
            ],
            input_gain_db: -1.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "空气感与细节",
            source: "",
            category: "",
            bands: &[
                (2_000.0, 1.0, 1.2),
                (5_000.0, 1.5, 1.0),
                (9_000.0, 2.0, 0.9),
                (14_000.0, 1.5, 0.8),
            ],
            input_gain_db: -3.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "超低频延伸",
            source: "",
            category: "",
            bands: &[
                (30.0, 3.0, 0.8),
                (45.0, 2.5, 0.9),
                (60.0, 2.0, 1.0),
                (80.0, 1.0, 0.9),
            ],
            input_gain_db: -5.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "低频解析",
            source: "",
            category: "",
            bands: &[
                (45.0, 2.0, 1.0),
                (90.0, 2.5, 1.1),
                (180.0, 1.5, 1.0),
                (350.0, 0.8, 1.1),
                (700.0, -1.0, 1.0),
            ],
            input_gain_db: -4.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "黑胶",
            source: "",
            category: "",
            bands: &[
                (60.0, 1.0, 0.8),
                (250.0, -0.8, 1.0),
                (1_000.0, 0.5, 1.1),
                (6_000.0, -1.0, 0.9),
                (12_000.0, -1.5, 0.8),
            ],
            input_gain_db: -2.5,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "清澈人声",
            source: "",
            category: "",
            bands: &[
                (200.0, -1.5, 1.0),
                (800.0, 0.8, 1.1),
                (2_500.0, 2.0, 1.0),
                (5_000.0, 1.2, 1.0),
                (9_000.0, -1.0, 0.9),
            ],
            input_gain_db: -4.5,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "女声",
            source: "",
            category: "",
            bands: &[
                (300.0, -1.0, 1.0),
                (1_200.0, 1.2, 1.1),
                (3_000.0, 2.0, 1.0),
                (6_000.0, 1.5, 0.9),
                (10_000.0, 0.8, 0.8),
            ],
            input_gain_db: -4.5,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "男声",
            source: "",
            category: "",
            bands: &[
                (100.0, 1.5, 1.0),
                (400.0, 1.0, 1.1),
                (1_200.0, 0.8, 1.1),
                (3_000.0, -0.8, 1.0),
                (8_000.0, -1.2, 0.9),
            ],
            input_gain_db: -4.0,
            output_gain_db: 0.0,
        },
    ]
}

pub fn v4_presets() -> Vec<PresetEntry> {
    vec![
        PresetEntry {
            name: "Original Sound",
            source: "Moriafly",
            category: "",
            bands: &[],
            input_gain_db: 0.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "低音 Bass",
            source: "Moriafly",
            category: "",
            bands: &[
                (80.0, 3.0, 0.9),
                (200.0, 1.0, 1.0),
                (2_000.0, -0.5, 1.0),
                (8_000.0, -0.5, 0.8),
            ],
            input_gain_db: -3.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "清澈 Clear V2",
            source: "Syaran",
            category: "",
            bands: &[
                (100.0, -1.0, 1.0),
                (500.0, -1.5, 1.1),
                (2_000.0, 1.5, 1.0),
                (6_000.0, 1.0, 0.9),
                (12_000.0, 0.5, 0.8),
            ],
            input_gain_db: -2.0,
            output_gain_db: 0.0,
        },
        PresetEntry {
            name: "清澈悦耳",
            source: "Moriafly",
            category: "",
            bands: &[
                (200.0, -0.8, 1.0),
                (1_000.0, 0.8, 1.1),
                (3_000.0, 1.2, 1.0),
                (8_000.0, 0.8, 0.9),
            ],
            input_gain_db: -2.5,
            output_gain_db: 0.0,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_sanitized_and_enabled() {
        for preset in builtin_presets().into_iter().chain(v4_presets()) {
            let settings = preset.to_settings();
            assert!(settings.enabled);
            assert!(settings.bands.len() <= EQ_MAX_BANDS);
            for band in &settings.bands {
                assert!(
                    (chordal_player::EQ_MIN_HZ..=chordal_player::EQ_MAX_HZ).contains(&band.freq_hz)
                );
                assert!(
                    (chordal_player::EQ_MIN_GAIN_DB..=chordal_player::EQ_MAX_GAIN_DB)
                        .contains(&band.gain_db)
                );
            }
        }
    }

    #[test]
    fn add_and_remove_band_keeps_selection_valid() {
        let mut state = EqState::new(EqSettings::default(), Vec::new());
        assert!(state.add_band().is_some());
        assert_eq!(state.selected, Some(0));
        state.add_band();
        assert_eq!(state.settings.bands.len(), 2);
        state.remove_selected();
        assert_eq!(state.settings.bands.len(), 1);
        assert_eq!(state.selected, Some(0));
        state.remove_selected();
        assert_eq!(state.selected, None);
        assert!(state.add_band().is_none() || state.selected.is_some());
    }

    #[test]
    fn initial_name_distinguishes_custom_curve_from_original_sound() {
        let empty = EqState::new(EqSettings::default(), Vec::new());
        assert_eq!(empty.preset_name, "Original Sound");

        let custom = EqSettings {
            enabled: true,
            input_gain_db: -2.0,
            output_gain_db: 0.0,
            bands: vec![EqBand::new(120.0, 2.0, 1.0)],
        };
        let state = EqState::new(custom, Vec::new());
        assert_eq!(state.preset_name, "自定义曲线");
        assert!(state.selected.is_some());
    }

    #[test]
    fn preview_does_not_mutate_the_working_settings() {
        let mut state = EqState::new(EqSettings::default(), Vec::new());
        let preset = v4_presets()[1];
        state.preview_preset(preset);
        assert_eq!(state.preview.map(|p| p.name), Some(preset.name));
        assert!(state.settings.bands.is_empty());

        state.load_preset(&preset);
        assert_eq!(state.preview.map(|p| p.name), Some(preset.name));
        assert_eq!(state.settings.bands.len(), preset.bands.len());
        assert!(!state.dirty);
    }

    #[test]
    fn save_as_creates_unique_names() {
        let mut state = EqState::new(EqSettings::default(), Vec::new());
        state.add_band();
        let first = state.save_as();
        state.add_band();
        let second = state.save_as();
        assert_ne!(first, second);
        assert_eq!(state.saved.len(), 2);
    }
}
