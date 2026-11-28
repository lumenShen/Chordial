//! 均衡器的 UI 状态：耳机档案 + 内置预设链路 + 自定义滤波器。
//!
//! 编辑器不再把「预设」当成一份整体快照，而是把声音拆成三层链路：
//! 耳机档案（AutoEQ / 测量校准）、可叠加的内置单点预设、用户自定义频段。
//! 内置预设以「启用式」存在：每一枚都可以单独开关、调整增幅乘数（预设
//! 分贝 × 乘数）或从链路移除，因此多个预设与手调频段可以自由组合。
//!
//! 播放线程只接收最终合成后的 [`EqSettings`]；曲线绘制与播放使用同一
//! 组 biquad 系数，屏幕上看到的就是实际听到的。

use serde::{Deserialize, Serialize};

use chordal_player::{EQ_MAX_BANDS, EqBand, EqSettings, GRAPHIC_10_BANDS, fmt_db, fmt_freq};

use crate::autoeq::AutoEqLibrary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EqTab {
    #[default]
    Edit,
    AutoEq,
    Builtin,
    Personal,
}

impl EqTab {
    pub const ALL: [Self; 4] = [Self::Edit, Self::AutoEq, Self::Builtin, Self::Personal];

    pub fn label(self) -> &'static str {
        match self {
            Self::Edit => "编辑",
            Self::AutoEq => "耳机档案",
            Self::Builtin => "内置预设",
            Self::Personal => "我的配置",
        }
    }
}

/// 内置预设增幅乘数范围。0× 等于旁路，2× 等于预设分贝翻倍。
pub const PRESET_AMOUNT_MIN: f32 = 0.0;
pub const PRESET_AMOUNT_MAX: f32 = 2.0;
pub const PRESET_AMOUNT_DEFAULT: f32 = 1.0;

fn default_amount() -> f32 {
    PRESET_AMOUNT_DEFAULT
}

fn default_true() -> bool {
    true
}

/// 耳机档案：AutoEQ 校准曲线，或用户从个人配置里另存的档案。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EqProfile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub settings: EqSettings,
}

impl EqProfile {
    pub fn band_count(&self) -> usize {
        self.settings.bands.len()
    }

    pub fn subtitle(&self) -> String {
        if self.source.is_empty() {
            format!("{} 个频段", self.band_count())
        } else {
            format!("{} · {} 个频段", self.source, self.band_count())
        }
    }
}

/// 链路中的一枚内置预设。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetBlock {
    pub preset_id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 增幅乘数：预设分贝 × amount。
    #[serde(default = "default_amount")]
    pub amount: f32,
}

impl PresetBlock {
    pub fn new(preset_id: &str) -> Self {
        Self {
            preset_id: preset_id.to_string(),
            enabled: true,
            amount: PRESET_AMOUNT_DEFAULT,
        }
    }

    pub fn sanitized(mut self) -> Self {
        self.amount = if self.amount.is_finite() {
            self.amount.clamp(PRESET_AMOUNT_MIN, PRESET_AMOUNT_MAX)
        } else {
            PRESET_AMOUNT_DEFAULT
        };
        self
    }
}

/// 可持久化的完整链路。旧版本的扁平 `EqSettings` 会迁移到自定义频段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EqRecipe {
    #[serde(default)]
    pub profile: Option<EqProfile>,
    #[serde(default)]
    pub blocks: Vec<PresetBlock>,
    #[serde(default)]
    pub custom: Vec<EqBand>,
    #[serde(default)]
    pub input_gain_db: f32,
    #[serde(default)]
    pub output_gain_db: f32,
}

impl EqRecipe {
    pub fn sanitized(mut self) -> Self {
        self.input_gain_db = finite_or_zero(self.input_gain_db);
        self.output_gain_db = finite_or_zero(self.output_gain_db);
        if let Some(profile) = &mut self.profile {
            profile.settings = profile.settings.clone().sanitized();
        }
        self.blocks = self
            .blocks
            .into_iter()
            .map(PresetBlock::sanitized)
            .take(EQ_MAX_BANDS)
            .collect();
        // 预设表更新后，未知 id 直接丢弃；重复项只保留第一份。
        let mut kept = Vec::with_capacity(self.blocks.len());
        for block in self.blocks.drain(..) {
            if builtin_preset(&block.preset_id).is_some()
                && !kept
                    .iter()
                    .any(|existing: &PresetBlock| existing.preset_id == block.preset_id)
            {
                kept.push(block);
            }
        }
        self.blocks = kept;
        self.custom = self
            .custom
            .into_iter()
            .take(EQ_MAX_BANDS)
            .map(EqBand::sanitized)
            .collect();
        self
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-24.0, 24.0)
    } else {
        0.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedEqPreset {
    pub name: String,
    #[serde(default)]
    pub settings: EqSettings,
    /// 新版本保存完整链路；旧配置没有该字段时退回 `settings`。
    #[serde(default)]
    pub recipe: Option<EqRecipe>,
}

/// 合成后的频段来源，用于曲线着色、点击命中与选择回填。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandOrigin {
    Profile(usize),
    Block { block: usize, band: usize },
    Custom(usize),
}

/// 当前选中的链路节点。预设块以整块为单位编辑增幅乘数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqSelection {
    Profile,
    Block(usize),
    Custom(usize),
}

impl EqSelection {
    pub fn from_origin(origin: BandOrigin) -> Self {
        match origin {
            BandOrigin::Profile(_) => Self::Profile,
            BandOrigin::Block { block, .. } => Self::Block(block),
            BandOrigin::Custom(index) => Self::Custom(index),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetGroup {
    Low,
    Vocal,
    High,
    Style,
}

impl PresetGroup {
    pub const ALL: [Self; 4] = [Self::Low, Self::Vocal, Self::High, Self::Style];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "低频",
            Self::Vocal => "人声",
            Self::High => "高频",
            Self::Style => "风格",
        }
    }
}

/// 一条内置单点预设。`bands` 是 `(Hz, dB, Q)`，多数只有一枚峰值节点。
#[derive(Debug, Clone, Copy)]
pub struct BuiltinPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub group: PresetGroup,
    pub bands: &'static [(f32, f32, f32)],
    /// 1× 时的前级补偿，避免叠加提升后削波。
    pub input_gain_db: f32,
}

impl BuiltinPreset {
    pub fn to_settings(self, amount: f32) -> EqSettings {
        let amount = amount.clamp(PRESET_AMOUNT_MIN, PRESET_AMOUNT_MAX);
        EqSettings {
            enabled: true,
            input_gain_db: self.input_gain_db * amount,
            output_gain_db: 0.0,
            bands: self
                .bands
                .iter()
                .map(|&(freq, gain, q)| EqBand::new(freq, gain * amount, q))
                .collect(),
        }
        .sanitized()
    }

    /// 例如 `+3.0 dB @ 11k`，用于预设行右侧的短徽标。
    pub fn gain_label(&self) -> String {
        match self.bands.first() {
            Some(&(freq, gain, _)) => format!("{} @ {}", fmt_db(gain), fmt_freq(freq)),
            None => "平直".into(),
        }
    }

    pub fn base_gain_db(&self) -> f32 {
        self.bands.first().map(|band| band.1).unwrap_or(0.0)
    }
}

pub fn fmt_amount(value: f32) -> String {
    let value = value.clamp(PRESET_AMOUNT_MIN, PRESET_AMOUNT_MAX);
    if (value - value.round()).abs() < 0.005 {
        format!("{}×", value.round() as i32)
    } else if (value * 10.0 - (value * 10.0).round()).abs() < 0.005 {
        format!("{:.1}×", value)
    } else {
        format!("{:.2}×", value)
    }
}

/// 内置预设是编译期静态表：渲染时零分配，查找也是 24 项的线性扫描。
const BUILTIN_PRESETS: &[BuiltinPreset] = &[
    // ---------- 低频 ----------
    BuiltinPreset {
        id: "sub_extension",
        name: "超低频下潜",
        description: "30–50 Hz 的下潜与包围感，适合电子、电影原声。",
        group: PresetGroup::Low,
        bands: &[(32.0, 4.0, 0.7)],
        input_gain_db: -3.0,
    },
    BuiltinPreset {
        id: "bass_weight",
        name: "低频量感",
        description: "增加鼓点厚度，适合偏薄、偏干的耳机。",
        group: PresetGroup::Low,
        bands: &[(70.0, 3.5, 0.8)],
        input_gain_db: -2.5,
    },
    BuiltinPreset {
        id: "bass_detail",
        name: "低频解析",
        description: "收束贝斯线条，让低频更清晰而不浑浊。",
        group: PresetGroup::Low,
        bands: &[(105.0, 3.0, 1.4)],
        input_gain_db: -2.0,
    },
    BuiltinPreset {
        id: "bass_tight",
        name: "低频收紧",
        description: "削减松散的低频驻波，速度感更利落。",
        group: PresetGroup::Low,
        bands: &[(90.0, -2.5, 1.2)],
        input_gain_db: 0.0,
    },
    BuiltinPreset {
        id: "kick_punch",
        name: "底鼓冲击",
        description: "加强底鼓的击打感与节奏轮廓。",
        group: PresetGroup::Low,
        bands: &[(60.0, 3.0, 1.1)],
        input_gain_db: -2.0,
    },
    BuiltinPreset {
        id: "mud_clean",
        name: "浑浊清理",
        description: "清理中低频堆积，人声与乐器更分离。",
        group: PresetGroup::Low,
        bands: &[(250.0, -2.5, 1.0)],
        input_gain_db: 0.0,
    },
    BuiltinPreset {
        id: "warm_body",
        name: "暖厚中低频",
        description: "增加木吉他与男声的木质厚度。",
        group: PresetGroup::Low,
        bands: &[(180.0, 2.5, 0.9)],
        input_gain_db: -1.5,
    },
    // ---------- 人声 ----------
    BuiltinPreset {
        id: "male_vocal",
        name: "男声",
        description: "增厚男声胸腔共鸣，减少单薄感。",
        group: PresetGroup::Vocal,
        bands: &[(150.0, 3.0, 0.9)],
        input_gain_db: -2.0,
    },
    BuiltinPreset {
        id: "female_vocal",
        name: "女声",
        description: "提亮女声的清晰度与穿透力。",
        group: PresetGroup::Vocal,
        bands: &[(2_800.0, 3.0, 1.1)],
        input_gain_db: -2.0,
    },
    BuiltinPreset {
        id: "vocal_forward",
        name: "人声靠前",
        description: "把人声从伴奏里托出来，适合流行乐。",
        group: PresetGroup::Vocal,
        bands: &[(1_200.0, 2.5, 1.0)],
        input_gain_db: -1.5,
    },
    BuiltinPreset {
        id: "nasal_cut",
        name: "鼻音抑制",
        description: "削弱鼻音与箱声，让人声更自然。",
        group: PresetGroup::Vocal,
        bands: &[(900.0, -2.0, 1.2)],
        input_gain_db: 0.0,
    },
    BuiltinPreset {
        id: "de_esser",
        name: "齿音抑制",
        description: "压低刺耳的 s / sh 齿音，久听不累。",
        group: PresetGroup::Vocal,
        bands: &[(6_500.0, -2.5, 1.4)],
        input_gain_db: 0.0,
    },
    BuiltinPreset {
        id: "chest_voice",
        name: "胸腔厚度",
        description: "增加低声部的共鸣与重量。",
        group: PresetGroup::Vocal,
        bands: &[(120.0, 2.0, 0.8)],
        input_gain_db: -1.5,
    },
    // ---------- 高频 ----------
    BuiltinPreset {
        id: "air_detail",
        name: "空气感与细节",
        description: "打开 10 kHz 以上的空气感，细节更通透。",
        group: PresetGroup::High,
        bands: &[(11_000.0, 3.0, 0.7)],
        input_gain_db: -2.0,
    },
    BuiltinPreset {
        id: "brilliance",
        name: "明亮度",
        description: "增加存在感与亮度，适合偏暗的耳机。",
        group: PresetGroup::High,
        bands: &[(4_000.0, 2.5, 0.9)],
        input_gain_db: -1.5,
    },
    BuiltinPreset {
        id: "treble_soft",
        name: "高频柔化",
        description: "收敛过亮的高频，长时间聆听更耐听。",
        group: PresetGroup::High,
        bands: &[(7_000.0, -2.5, 0.8)],
        input_gain_db: 0.0,
    },
    BuiltinPreset {
        id: "super_air",
        name: "极高频延伸",
        description: "延伸极高频，增加空间泛音与光泽。",
        group: PresetGroup::High,
        bands: &[(15_000.0, 2.5, 0.8)],
        input_gain_db: -1.5,
    },
    BuiltinPreset {
        id: "hiss_control",
        name: "嘶声控制",
        description: "抑制录音底噪与高频毛刺。",
        group: PresetGroup::High,
        bands: &[(8_500.0, -2.0, 1.6)],
        input_gain_db: 0.0,
    },
    // ---------- 风格 ----------
    BuiltinPreset {
        id: "pop_polish",
        name: "流行润色",
        description: "流行人声与合成器更亮眼、更有轮廓。",
        group: PresetGroup::Style,
        bands: &[(2_500.0, 2.0, 1.0)],
        input_gain_db: -1.5,
    },
    BuiltinPreset {
        id: "vinyl_warm",
        name: "黑胶温暖",
        description: "中低频更温暖，高频更顺滑。",
        group: PresetGroup::Style,
        bands: &[(100.0, 2.0, 0.7)],
        input_gain_db: -1.5,
    },
    BuiltinPreset {
        id: "monitor_flat",
        name: "参考校准",
        description: "削减中频染色，更接近监听参考。",
        group: PresetGroup::Style,
        bands: &[(400.0, -1.5, 1.0)],
        input_gain_db: 0.0,
    },
    BuiltinPreset {
        id: "classical_hall",
        name: "古典空间",
        description: "弦乐泛音与音乐厅残响更丰富。",
        group: PresetGroup::Style,
        bands: &[(12_000.0, 1.5, 0.6)],
        input_gain_db: -1.0,
    },
    BuiltinPreset {
        id: "edm_impact",
        name: "电子冲击",
        description: "为电子舞曲加强 Sub 冲击与下潜。",
        group: PresetGroup::Style,
        bands: &[(55.0, 4.0, 1.0)],
        input_gain_db: -3.0,
    },
    BuiltinPreset {
        id: "acoustic_wood",
        name: "原声木质",
        description: "木吉他、钢琴的琴腔共鸣更自然。",
        group: PresetGroup::Style,
        bands: &[(220.0, 2.0, 0.9)],
        input_gain_db: -1.5,
    },
    BuiltinPreset {
        id: "late_night",
        name: "夜间低频",
        description: "小音量下补足低频感知，不吵到别人。",
        group: PresetGroup::Style,
        bands: &[(45.0, 2.0, 0.8)],
        input_gain_db: -1.5,
    },
];

pub fn builtin_presets() -> &'static [BuiltinPreset] {
    BUILTIN_PRESETS
}

pub fn builtin_preset(id: &str) -> Option<&'static BuiltinPreset> {
    BUILTIN_PRESETS.iter().find(|preset| preset.id == id)
}

#[derive(Debug, Clone)]
pub struct EqState {
    /// 合成后的最终配置：播放线程与曲线绘制都只用这一份。
    pub settings: EqSettings,
    /// 与 `settings.bands` 一一对应的来源。
    pub origins: Vec<BandOrigin>,
    pub profile: Option<EqProfile>,
    pub blocks: Vec<PresetBlock>,
    pub custom: Vec<EqBand>,
    pub master_input_db: f32,
    pub master_output_db: f32,
    pub preset_name: String,
    pub dirty: bool,
    pub selected: Option<EqSelection>,
    pub tab: EqTab,
    pub saved: Vec<SavedEqPreset>,
    pub search: String,
    /// AutoEQ 分类筛选（全部 / 头戴式 / 入耳式 / 耳塞式）。
    pub category: String,
    /// 内置预设库的分组筛选；`None` 表示全部。
    pub builtin_group: Option<PresetGroup>,
    pub type_menu_open: bool,
    /// 内置预设库当前预览的预设 id。
    pub preview_builtin: Option<&'static str>,
    /// 个人配置的预览索引。
    pub preview_saved: Option<usize>,
    /// 从 AutoEq 仓库拉取的耳机档案（索引常驻，曲线按需加载）。
    pub autoeq: AutoEqLibrary,
    /// AutoEQ 当前预览的型号（仓库内文件路径，跨索引刷新仍有效）。
    pub autoeq_preview: Option<String>,
}

impl EqState {
    pub fn new(settings: EqSettings, saved: Vec<SavedEqPreset>, recipe: Option<EqRecipe>) -> Self {
        let settings = settings.sanitized();
        let enabled = settings.enabled;
        let untouched = settings.bands.is_empty()
            && settings.input_gain_db.abs() < 0.05
            && settings.output_gain_db.abs() < 0.05;

        let mut state = match recipe {
            Some(recipe) => {
                let recipe = recipe.sanitized();
                let name = preset_name_for(&recipe);
                Self {
                    settings: EqSettings {
                        enabled,
                        ..EqSettings::default()
                    },
                    origins: Vec::new(),
                    preset_name: name,
                    profile: recipe.profile,
                    blocks: recipe.blocks,
                    custom: recipe.custom,
                    master_input_db: recipe.input_gain_db,
                    master_output_db: recipe.output_gain_db,
                    dirty: false,
                    selected: None,
                    tab: EqTab::Edit,
                    saved,
                    search: String::new(),
                    category: "全部".into(),
                    builtin_group: None,
                    type_menu_open: false,
                    preview_builtin: None,
                    preview_saved: None,
                    autoeq: AutoEqLibrary::default(),
                    autoeq_preview: None,
                }
            }
            None => Self {
                settings: settings.clone(),
                origins: Vec::new(),
                preset_name: if untouched {
                    "Original Sound".into()
                } else {
                    "自定义曲线".to_string()
                },
                profile: None,
                blocks: Vec::new(),
                // 旧版本把一切存在扁平的 bands 里；迁移为手调频段，保持听感。
                custom: settings.bands.clone(),
                master_input_db: settings.input_gain_db,
                master_output_db: settings.output_gain_db,
                dirty: false,
                selected: None,
                tab: EqTab::Edit,
                saved,
                search: String::new(),
                category: "全部".into(),
                builtin_group: None,
                type_menu_open: false,
                preview_builtin: None,
                preview_saved: None,
                autoeq: AutoEqLibrary::default(),
                autoeq_preview: None,
            },
        };
        state.rebuild();
        state.selected = state.first_selection();
        state
    }

    pub fn display_name(&self) -> String {
        if self.dirty {
            format!("{}（已修改）", self.preset_name)
        } else {
            self.preset_name.clone()
        }
    }

    pub fn recipe(&self) -> EqRecipe {
        EqRecipe {
            profile: self.profile.clone(),
            blocks: self.blocks.clone(),
            custom: self.custom.clone(),
            input_gain_db: self.master_input_db,
            output_gain_db: self.master_output_db,
        }
        .sanitized()
    }

    /// 合成最终滤波器链。容量优先级：自定义 > 内置预设 > 耳机档案，
    /// 这样 32 段上限时不会悄悄丢掉用户手调的频段。
    fn compose(&self) -> (EqSettings, Vec<BandOrigin>) {
        let cap = EQ_MAX_BANDS;
        let mut bands = Vec::with_capacity(cap);
        let mut origins = Vec::with_capacity(cap);
        let mut input_gain = self.master_input_db;
        let output_gain = self.master_output_db;

        for (index, band) in self.custom.iter().enumerate() {
            if bands.len() >= cap {
                break;
            }
            bands.push(band.clone());
            origins.push(BandOrigin::Custom(index));
        }

        for (block_index, block) in self.blocks.iter().enumerate() {
            if !block.enabled {
                continue;
            }
            let Some(preset) = builtin_preset(&block.preset_id) else {
                continue;
            };
            input_gain += preset.input_gain_db * block.amount;
            for (band_index, &(freq, gain, q)) in preset.bands.iter().enumerate() {
                if bands.len() >= cap {
                    break;
                }
                bands.push(EqBand::new(freq, gain * block.amount, q));
                origins.push(BandOrigin::Block {
                    block: block_index,
                    band: band_index,
                });
            }
        }

        if let Some(profile) = &self.profile {
            for (index, band) in profile.settings.bands.iter().enumerate() {
                if bands.len() >= cap {
                    break;
                }
                bands.push(band.clone());
                origins.push(BandOrigin::Profile(index));
            }
            input_gain += profile.settings.input_gain_db;
        }

        (
            EqSettings {
                enabled: self.settings.enabled,
                input_gain_db: input_gain,
                output_gain_db: output_gain,
                bands,
            }
            .sanitized(),
            origins,
        )
    }

    /// 任一链路变化后重算合成配置；选中项失效时清空。
    pub fn rebuild(&mut self) {
        let (settings, origins) = self.compose();
        self.settings = settings;
        self.origins = origins;
        self.selected = match self.selected {
            Some(EqSelection::Custom(index)) if index < self.custom.len() => {
                Some(EqSelection::Custom(index))
            }
            Some(EqSelection::Block(index)) if index < self.blocks.len() => {
                Some(EqSelection::Block(index))
            }
            Some(EqSelection::Profile) if self.profile.is_some() => Some(EqSelection::Profile),
            _ => None,
        };
    }

    fn first_selection(&self) -> Option<EqSelection> {
        self.origins.first().copied().map(EqSelection::from_origin)
    }

    pub fn touch(&mut self) {
        self.dirty = true;
    }

    pub fn total_bands(&self) -> usize {
        self.origins.len()
    }

    pub fn remaining_bands(&self) -> usize {
        EQ_MAX_BANDS.saturating_sub(self.origins.len())
    }

    /// 更换耳机档案时可用的节点数：现有档案占用的节点会被释放。
    pub fn bands_available_for_profile(&self) -> usize {
        let current = self
            .origins
            .iter()
            .filter(|origin| matches!(origin, BandOrigin::Profile(_)))
            .count();
        self.remaining_bands() + current
    }

    pub fn profile_band_count(&self) -> usize {
        self.profile
            .as_ref()
            .map(EqProfile::band_count)
            .unwrap_or(0)
    }

    pub fn enabled_block_count(&self) -> usize {
        self.blocks.iter().filter(|block| block.enabled).count()
    }

    pub fn block_index(&self, preset_id: &str) -> Option<usize> {
        self.blocks
            .iter()
            .position(|block| block.preset_id == preset_id)
    }

    pub fn block_preset(&self, index: usize) -> Option<&'static BuiltinPreset> {
        self.blocks
            .get(index)
            .and_then(|block| builtin_preset(&block.preset_id))
    }

    /// 加入一枚内置预设；重复加入时只重新启用，不新增节点。
    pub fn add_block(&mut self, preset: &'static BuiltinPreset) -> Result<usize, &'static str> {
        if let Some(index) = self.block_index(preset.id) {
            if !self.blocks[index].enabled {
                self.blocks[index].enabled = true;
                self.rebuild();
            }
            return Ok(index);
        }
        if self.remaining_bands() < preset.bands.len() {
            return Err("频段已达 32 段上限，请先移除一些滤波器");
        }
        self.blocks.push(PresetBlock::new(preset.id));
        let index = self.blocks.len() - 1;
        self.selected = Some(EqSelection::Block(index));
        self.dirty = true;
        self.rebuild();
        Ok(index)
    }

    pub fn remove_block(&mut self, index: usize) {
        if index >= self.blocks.len() {
            return;
        }
        self.blocks.remove(index);
        if let Some(EqSelection::Block(selected)) = self.selected {
            self.selected = if self.blocks.is_empty() {
                None
            } else if selected > index {
                Some(EqSelection::Block(selected - 1))
            } else {
                Some(EqSelection::Block(selected.min(self.blocks.len() - 1)))
            };
        }
        self.dirty = true;
        self.rebuild();
    }

    pub fn toggle_block_enabled(&mut self, index: usize) {
        if let Some(block) = self.blocks.get_mut(index) {
            block.enabled = !block.enabled;
            self.dirty = true;
            self.rebuild();
        }
    }

    pub fn set_block_amount(&mut self, index: usize, amount: f32) {
        if let Some(block) = self.blocks.get_mut(index) {
            let amount = if amount.is_finite() {
                amount.clamp(PRESET_AMOUNT_MIN, PRESET_AMOUNT_MAX)
            } else {
                PRESET_AMOUNT_DEFAULT
            };
            block.amount = amount;
            self.dirty = true;
            self.rebuild();
        }
    }

    pub fn set_profile(&mut self, profile: EqProfile) {
        self.preset_name = profile.name.clone();
        self.profile = Some(profile);
        self.selected = Some(EqSelection::Profile);
        self.dirty = true;
        self.rebuild();
    }

    pub fn clear_profile(&mut self) {
        if self.profile.take().is_some() {
            if self.selected == Some(EqSelection::Profile) {
                self.selected = None;
            }
            self.dirty = true;
            self.rebuild();
        }
    }

    /// 添加一个自定义频段：优先 1 kHz，其次标准 10 段的中心频率。
    pub fn add_custom_band(&mut self) -> Option<usize> {
        if self.remaining_bands() == 0 {
            return None;
        }
        let freq = GRAPHIC_10_BANDS
            .iter()
            .copied()
            .chain([60.0, 120.0, 240.0, 480.0, 960.0, 3_000.0, 6_000.0, 12_000.0])
            .chain([1_000.0])
            .find(|f| {
                !self
                    .custom
                    .iter()
                    .any(|b| (b.freq_hz - f).abs() / f.max(1.0) < 0.02)
            })
            .unwrap_or(1_000.0);
        self.custom.push(EqBand::new(freq, 0.0, 0.71));
        let index = self.custom.len() - 1;
        self.selected = Some(EqSelection::Custom(index));
        self.dirty = true;
        self.rebuild();
        Some(index)
    }

    pub fn remove_custom(&mut self, index: usize) {
        if index >= self.custom.len() {
            return;
        }
        self.custom.remove(index);
        if let Some(EqSelection::Custom(selected)) = self.selected {
            self.selected = if self.custom.is_empty() {
                None
            } else if selected > index {
                Some(EqSelection::Custom(selected - 1))
            } else {
                Some(EqSelection::Custom(selected.min(self.custom.len() - 1)))
            };
        }
        self.dirty = true;
        self.rebuild();
    }

    pub fn remove_selected(&mut self) {
        match self.selected {
            Some(EqSelection::Custom(index)) => self.remove_custom(index),
            Some(EqSelection::Block(index)) => self.remove_block(index),
            Some(EqSelection::Profile) => self.clear_profile(),
            None => {}
        }
    }

    pub fn selected_custom_index(&self) -> Option<usize> {
        match self.selected {
            Some(EqSelection::Custom(index)) if index < self.custom.len() => Some(index),
            _ => None,
        }
    }

    pub fn selected_custom_mut(&mut self) -> Option<&mut EqBand> {
        let index = self.selected_custom_index()?;
        self.custom.get_mut(index)
    }

    /// 选中项在合成链中的首个频段下标（曲线高亮用）。
    pub fn selected_band_index(&self) -> Option<usize> {
        let selection = self.selected?;
        self.origins
            .iter()
            .position(|origin| Self::origin_matches(*origin, selection))
    }

    fn origin_matches(origin: BandOrigin, selection: EqSelection) -> bool {
        match (origin, selection) {
            (BandOrigin::Profile(_), EqSelection::Profile) => true,
            (BandOrigin::Block { block, .. }, EqSelection::Block(selected)) => block == selected,
            (BandOrigin::Custom(index), EqSelection::Custom(selected)) => index == selected,
            _ => false,
        }
    }

    pub fn load_saved(&mut self, preset: &SavedEqPreset) {
        let enabled = preset.settings.enabled;
        match &preset.recipe {
            Some(recipe) => {
                let recipe = recipe.clone().sanitized();
                self.profile = recipe.profile;
                self.blocks = recipe.blocks;
                self.custom = recipe.custom;
                self.master_input_db = recipe.input_gain_db;
                self.master_output_db = recipe.output_gain_db;
            }
            None => {
                // 旧配置：整份扁平设置视为自定义频段。
                self.profile = None;
                self.blocks.clear();
                self.custom = preset.settings.bands.clone();
                self.master_input_db = preset.settings.input_gain_db;
                self.master_output_db = preset.settings.output_gain_db;
            }
        }
        self.settings.enabled = enabled;
        self.preset_name = preset.name.clone();
        self.selected = None;
        self.dirty = false;
        self.rebuild();
        self.selected = self.first_selection();
    }

    pub fn preview_saved(&mut self, index: usize) {
        self.preview_saved = Some(index);
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
            recipe: Some(self.recipe()),
        });
        self.preset_name = name.clone();
        self.dirty = false;
        name
    }
}

fn preset_name_for(recipe: &EqRecipe) -> String {
    if let Some(profile) = &recipe.profile {
        profile.name.clone()
    } else if recipe.blocks.is_empty() {
        "自定义曲线".into()
    } else {
        "自定义链路".into()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_presets_are_valid_and_mostly_single_node() {
        assert!(builtin_presets().len() >= 20);
        for preset in builtin_presets() {
            assert!(!preset.id.is_empty());
            assert!(!preset.name.is_empty());
            assert!(!preset.bands.is_empty());
            assert_eq!(preset.bands.len(), 1, "{} 应该是单点预设", preset.id);
            let settings = preset.to_settings(1.0);
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
        let mut ids: Vec<&str> = builtin_presets().iter().map(|preset| preset.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), builtin_presets().len());
    }

    #[test]
    fn composition_scales_preset_gain_and_compensation() {
        let mut state = EqState::new(EqSettings::default(), Vec::new(), None);
        let preset = builtin_preset("bass_detail").unwrap();
        state.add_block(preset).unwrap();
        state.set_block_amount(0, 0.5);
        assert_eq!(state.settings.bands.len(), 1);
        assert!((state.settings.bands[0].gain_db - 1.5).abs() < 0.01);
        assert!((state.settings.input_gain_db - (-1.0)).abs() < 0.01);
        assert_eq!(state.origins, vec![BandOrigin::Block { block: 0, band: 0 }]);

        state.toggle_block_enabled(0);
        assert!(state.settings.bands.is_empty());
        assert_eq!(state.settings.input_gain_db, 0.0);
    }

    #[test]
    fn custom_bands_and_profile_stack_in_pipeline_order() {
        let mut state = EqState::new(EqSettings::default(), Vec::new(), None);
        state.add_custom_band();
        state.custom[0].gain_db = 2.0;
        state.rebuild();
        state.set_profile(EqProfile {
            name: "Test".into(),
            source: "AutoEQ".into(),
            settings: EqSettings::with_bands([EqBand::new(1_000.0, -1.0, 1.0)]),
        });
        state
            .add_block(builtin_preset("air_detail").unwrap())
            .unwrap();
        assert_eq!(state.settings.bands.len(), 3);
        assert!(state.origins.contains(&BandOrigin::Profile(0)));
        assert!(state.origins.contains(&BandOrigin::Custom(0)));
        assert!(
            state
                .origins
                .contains(&BandOrigin::Block { block: 0, band: 0 })
        );
        assert_eq!(state.profile_band_count(), 1);
        assert_eq!(state.enabled_block_count(), 1);
    }

    #[test]
    fn add_and_remove_custom_keeps_selection_valid() {
        let mut state = EqState::new(EqSettings::default(), Vec::new(), None);
        assert!(state.add_custom_band().is_some());
        assert_eq!(state.selected, Some(EqSelection::Custom(0)));
        state.add_custom_band();
        assert_eq!(state.custom.len(), 2);
        state.remove_custom(1);
        assert_eq!(state.custom.len(), 1);
        assert_eq!(state.selected, Some(EqSelection::Custom(0)));
        state.remove_custom(0);
        assert_eq!(state.selected, None);
        assert!(state.add_custom_band().is_some());
    }

    #[test]
    fn adding_block_twice_does_not_duplicate() {
        let mut state = EqState::new(EqSettings::default(), Vec::new(), None);
        let preset = builtin_preset("male_vocal").unwrap();
        state.add_block(preset).unwrap();
        state.toggle_block_enabled(0);
        state.add_block(preset).unwrap();
        assert_eq!(state.blocks.len(), 1);
        assert!(state.blocks[0].enabled);
        assert_eq!(state.settings.bands.len(), 1);
    }

    #[test]
    fn recipe_round_trip_preserves_layers_and_amount() {
        let mut state = EqState::new(EqSettings::default(), Vec::new(), None);
        state
            .add_block(builtin_preset("air_detail").unwrap())
            .unwrap();
        state.set_block_amount(0, 0.8);
        state.add_custom_band();
        state.custom[0].freq_hz = 120.0;
        state.custom[0].gain_db = -1.5;
        state.set_profile(EqProfile {
            name: "Test Profile".into(),
            source: "AutoEQ".into(),
            settings: EqSettings::with_bands([EqBand::new(1_000.0, -1.0, 1.0)]),
        });
        state.rebuild();
        let recipe = state.recipe();
        let rebuilt = EqState::new(EqSettings::default(), Vec::new(), Some(recipe));
        assert_eq!(
            rebuilt
                .profile
                .as_ref()
                .map(|profile| profile.name.as_str()),
            Some("Test Profile")
        );
        assert_eq!(rebuilt.blocks.len(), 1);
        assert!((rebuilt.blocks[0].amount - 0.8).abs() < 0.001);
        assert_eq!(rebuilt.custom.len(), 1);
        assert!((rebuilt.custom[0].gain_db + 1.5).abs() < 0.01);
        assert_eq!(rebuilt.settings.bands.len(), 3);
    }

    #[test]
    fn removing_earlier_nodes_keeps_selection_on_the_same_item() {
        let mut state = EqState::new(EqSettings::default(), Vec::new(), None);
        state
            .add_block(builtin_preset("air_detail").unwrap())
            .unwrap();
        state
            .add_block(builtin_preset("male_vocal").unwrap())
            .unwrap();
        state.selected = Some(EqSelection::Block(1));
        state.remove_block(0);
        assert_eq!(state.selected, Some(EqSelection::Block(0)));
        assert_eq!(state.blocks[0].preset_id, "male_vocal");

        state.add_custom_band();
        state.add_custom_band();
        state.selected = Some(EqSelection::Custom(1));
        state.remove_custom(0);
        assert_eq!(state.selected, Some(EqSelection::Custom(0)));
    }

    #[test]
    fn old_flat_settings_migrate_to_custom_bands() {
        let settings = EqSettings {
            enabled: true,
            input_gain_db: -2.0,
            output_gain_db: 0.0,
            bands: vec![EqBand::new(120.0, 2.0, 1.0)],
        };
        let state = EqState::new(settings, Vec::new(), None);
        assert!(state.profile.is_none());
        assert_eq!(state.custom.len(), 1);
        assert_eq!(state.settings.bands.len(), 1);
        assert!((state.settings.input_gain_db + 2.0).abs() < 0.01);
        assert_eq!(state.selected, Some(EqSelection::Custom(0)));
    }

    #[test]
    fn old_saved_preset_without_recipe_loads_as_custom_bands() {
        let mut state = EqState::new(EqSettings::default(), Vec::new(), None);
        let preset = SavedEqPreset {
            name: "旧配置".into(),
            settings: EqSettings {
                enabled: true,
                input_gain_db: -1.0,
                output_gain_db: 0.0,
                bands: vec![EqBand::new(400.0, 1.0, 0.9)],
            },
            recipe: None,
        };
        state.load_saved(&preset);
        assert_eq!(state.preset_name, "旧配置");
        assert_eq!(state.custom.len(), 1);
        assert_eq!(state.settings.bands.len(), 1);
        assert!(!state.dirty);
    }

    #[test]
    fn composition_respects_band_cap() {
        let mut state = EqState::new(EqSettings::default(), Vec::new(), None);
        for _ in 0..EQ_MAX_BANDS {
            state.add_custom_band().unwrap();
        }
        assert_eq!(state.total_bands(), EQ_MAX_BANDS);
        assert!(state.add_custom_band().is_none());
        assert!(
            state
                .add_block(builtin_preset("air_detail").unwrap())
                .is_err()
        );
        assert_eq!(state.total_bands(), EQ_MAX_BANDS);
    }

    #[test]
    fn save_as_persists_recipe() {
        let mut state = EqState::new(EqSettings::default(), Vec::new(), None);
        state
            .add_block(builtin_preset("edm_impact").unwrap())
            .unwrap();
        state.add_custom_band();
        let name = state.save_as();
        let saved = state
            .saved
            .iter()
            .find(|preset| preset.name == name)
            .unwrap();
        assert!(saved.recipe.is_some());
        assert_eq!(saved.recipe.as_ref().unwrap().blocks.len(), 1);
        assert_eq!(saved.recipe.as_ref().unwrap().custom.len(), 1);
    }

    #[test]
    fn unknown_block_ids_are_dropped_on_load() {
        let recipe = EqRecipe {
            profile: None,
            blocks: vec![
                PresetBlock::new("does_not_exist"),
                PresetBlock::new("air_detail"),
            ],
            custom: Vec::new(),
            input_gain_db: 0.0,
            output_gain_db: 0.0,
        };
        let state = EqState::new(EqSettings::default(), Vec::new(), Some(recipe));
        assert_eq!(state.blocks.len(), 1);
        assert_eq!(state.blocks[0].preset_id, "air_detail");
    }
}
