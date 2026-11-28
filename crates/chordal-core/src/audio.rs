//! 音频反应式视觉共用的频段快照与可编辑映射。

use serde::{Deserialize, Serialize};

/// 频段通道数。
pub const AUDIO_BAND_COUNT: usize = 5;
/// BHP 网格每边控制点数。
pub const CONTROL_GRID_SIZE: usize = 5;
/// 5×5 控制点总数；编号 1..=25，行优先、从左到右、从上到下。
pub const CONTROL_POINT_COUNT: usize = CONTROL_GRID_SIZE * CONTROL_GRID_SIZE;
/// 可编辑频率下限（Hz）。
pub const MIN_AUDIO_HZ: f32 = 20.0;
/// 可编辑频率上限（Hz）。
pub const MAX_AUDIO_HZ: f32 = 22_050.0;

/// 五路反应通道的稳定顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioBandKind {
    Rumble,
    Kick,
    Vocals,
    Instruments,
    Air,
}

impl AudioBandKind {
    pub const ALL: [Self; AUDIO_BAND_COUNT] = [
        Self::Rumble,
        Self::Kick,
        Self::Vocals,
        Self::Instruments,
        Self::Air,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Rumble => 0,
            Self::Kick => 1,
            Self::Vocals => 2,
            Self::Instruments => 3,
            Self::Air => 4,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Rumble => "低频振动",
            Self::Kick => "低频鼓点",
            Self::Vocals => "人声频段",
            Self::Instruments => "乐器频段",
            Self::Air => "高频呲呲呲",
        }
    }

    pub const fn default_range(self) -> FrequencyRange {
        match self {
            Self::Rumble => FrequencyRange {
                low_hz: 25.0,
                high_hz: 120.0,
            },
            Self::Kick => FrequencyRange {
                low_hz: 35.0,
                high_hz: 160.0,
            },
            Self::Vocals => FrequencyRange {
                low_hz: 180.0,
                high_hz: 2_000.0,
            },
            Self::Instruments => FrequencyRange {
                low_hz: 700.0,
                high_hz: 6_000.0,
            },
            Self::Air => FrequencyRange {
                low_hz: 6_000.0,
                high_hz: 16_000.0,
            },
        }
    }

    pub const fn default_controls(self) -> ControlPointSet {
        match self {
            Self::Rumble => ControlPointSet::from_numbers(&[7, 8, 9, 12, 13, 14, 17, 18, 19]),
            Self::Kick => ControlPointSet::from_numbers(&[12, 13, 14, 17, 18, 19]),
            Self::Vocals => ControlPointSet::from_numbers(&[7, 8, 12, 13]),
            Self::Instruments => ControlPointSet::from_numbers(&[9, 13, 14, 18, 19]),
            Self::Air => ControlPointSet::from_numbers(&[7, 8, 9]),
        }
    }
}

/// 一个可编辑的闭频率区间。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrequencyRange {
    pub low_hz: f32,
    pub high_hz: f32,
}

impl FrequencyRange {
    /// 非法输入回退到该通道自己的默认范围。
    pub fn sanitized(self, fallback: Self) -> Self {
        normalize_frequency_range(self.low_hz, self.high_hz, fallback)
    }
}

/// 五路频段的频率范围；数组顺序与 [`AudioBandKind`] 一致。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioBandRanges {
    pub bands: [FrequencyRange; AUDIO_BAND_COUNT],
}

impl Default for AudioBandRanges {
    fn default() -> Self {
        Self {
            bands: [
                AudioBandKind::Rumble.default_range(),
                AudioBandKind::Kick.default_range(),
                AudioBandKind::Vocals.default_range(),
                AudioBandKind::Instruments.default_range(),
                AudioBandKind::Air.default_range(),
            ],
        }
    }
}

impl AudioBandRanges {
    pub fn sanitized(mut self) -> Self {
        for kind in AudioBandKind::ALL {
            self.bands[kind.index()] = self.bands[kind.index()].sanitized(kind.default_range());
        }
        self
    }

    pub fn get(&self, kind: AudioBandKind) -> FrequencyRange {
        self.bands[kind.index()]
    }

    pub fn set(&mut self, kind: AudioBandKind, range: FrequencyRange) {
        self.bands[kind.index()] = range.sanitized(kind.default_range());
    }
}

/// 将输入收敛到可分析范围；任一值非有限或区间无效时整体回退。
pub fn normalize_frequency_range(
    low_hz: f32,
    high_hz: f32,
    fallback: FrequencyRange,
) -> FrequencyRange {
    if !low_hz.is_finite() || !high_hz.is_finite() {
        return fallback;
    }
    let low = low_hz.clamp(MIN_AUDIO_HZ, MAX_AUDIO_HZ);
    let high = high_hz.clamp(MIN_AUDIO_HZ, MAX_AUDIO_HZ);
    if high > low {
        FrequencyRange {
            low_hz: low,
            high_hz: high,
        }
    } else {
        fallback
    }
}

/// 是否是允许用户分配的内部点编号（1..=25，边界点保持不动）。
pub const fn is_interior_point_number(number: u8) -> bool {
    matches!(number, 7 | 8 | 9 | 12 | 13 | 14 | 17 | 18 | 19)
}

/// 控制点集合的 25 位掩码；位 0 对应编号 1。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ControlPointSet(pub u32);

impl ControlPointSet {
    pub const fn from_numbers(numbers: &[u8]) -> Self {
        let mut bits = 0u32;
        let mut index = 0;
        while index < numbers.len() {
            let number = numbers[index];
            if is_interior_point_number(number) {
                bits |= 1u32 << (number - 1);
            }
            index += 1;
        }
        Self(bits)
    }

    pub const fn contains_number(self, number: u8) -> bool {
        number >= 1 && number <= CONTROL_POINT_COUNT as u8 && self.0 & (1u32 << (number - 1)) != 0
    }

    /// 渲染层使用 0-based 索引查询。
    pub const fn contains(self, index: usize) -> bool {
        index < CONTROL_POINT_COUNT && self.0 & (1u32 << index) != 0
    }

    pub const fn insert_number(mut self, number: u8) -> Option<Self> {
        if !is_interior_point_number(number) {
            return None;
        }
        self.0 |= 1u32 << (number - 1);
        Some(self)
    }

    pub const fn sanitized(mut self) -> Self {
        let mut number = 1u8;
        while number <= CONTROL_POINT_COUNT as u8 {
            if !is_interior_point_number(number) {
                self.0 &= !(1u32 << (number - 1));
            }
            number += 1;
        }
        self
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn point_numbers(self) -> Vec<u8> {
        (1..=CONTROL_POINT_COUNT as u8)
            .filter(|&number| self.contains_number(number))
            .collect()
    }

    pub fn display(self) -> String {
        self.point_numbers()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// 控制点列表解析错误；UI 负责转成本地化提示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlPointParseError {
    InvalidNumber,
    BoundaryOrOutOfRange(u8),
}

/// 解析逗号、分号、斜杠或空白分隔的 1–25 控制点编号。
pub fn parse_control_point_set(text: &str) -> Result<ControlPointSet, ControlPointParseError> {
    let mut controls = ControlPointSet::default();
    for token in
        text.split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | '，' | ';' | '；' | '/'))
    {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let number = token
            .parse::<u8>()
            .map_err(|_| ControlPointParseError::InvalidNumber)?;
        if !is_interior_point_number(number) {
            return Err(ControlPointParseError::BoundaryOrOutOfRange(number));
        }
        controls = controls
            .insert_number(number)
            .ok_or(ControlPointParseError::BoundaryOrOutOfRange(number))?;
    }
    Ok(controls)
}

/// 五个频段分别控制哪些内部点；允许重叠，空集合表示不驱动局部控制点。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioControlMapping {
    pub bands: [ControlPointSet; AUDIO_BAND_COUNT],
}

impl Default for AudioControlMapping {
    fn default() -> Self {
        Self {
            bands: [
                AudioBandKind::Rumble.default_controls(),
                AudioBandKind::Kick.default_controls(),
                AudioBandKind::Vocals.default_controls(),
                AudioBandKind::Instruments.default_controls(),
                AudioBandKind::Air.default_controls(),
            ],
        }
    }
}

impl AudioControlMapping {
    pub fn sanitized(mut self) -> Self {
        for controls in &mut self.bands {
            *controls = controls.sanitized();
        }
        self
    }

    pub fn get(&self, kind: AudioBandKind) -> ControlPointSet {
        self.bands[kind.index()]
    }

    pub fn set(&mut self, kind: AudioBandKind, controls: ControlPointSet) {
        self.bands[kind.index()] = controls.sanitized();
    }
}

/// 一次分析得到的五路归一化频段包络。
///
/// 这些值描述的是频段能量与瞬态，不是音源分离结果；人声与乐器频段可以
/// 同时响应同一段声音。所有通道一次性发布，避免渲染层拼出不同分析帧。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioBands {
    /// 低频持续能量，适合缓慢呼吸与整体振动。
    pub rumble: f32,
    /// 低频鼓点瞬态，适合快速外扩与脉冲提亮。
    pub kick: f32,
    /// 人声主要频段的近似能量。
    pub vocals: f32,
    /// 乐器主要频段的近似能量。
    pub instruments: f32,
    /// 高频镲片/齿音/空气感能量。
    pub air: f32,
}

impl AudioBands {
    /// 清理跨线程边界上的非有限值并钳制到视觉包络范围。
    pub fn sanitized(self) -> Self {
        let clean = |value: f32| {
            if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        Self {
            rumble: clean(self.rumble),
            kick: clean(self.kick),
            vocals: clean(self.vocals),
            instruments: clean(self.instruments),
            air: clean(self.air),
        }
    }

    /// 逐通道取峰值，用于在一个粗粒度写块内保留最响的瞬态。
    pub fn peak(self, other: Self) -> Self {
        Self {
            rumble: self.rumble.max(other.rumble),
            kick: self.kick.max(other.kick),
            vocals: self.vocals.max(other.vocals),
            instruments: self.instruments.max(other.instruments),
            air: self.air.max(other.air),
        }
    }

    /// 是否还有任何值得驱动视觉的能量。
    pub fn is_active(self, epsilon: f32) -> bool {
        self.rumble > epsilon
            || self.kick > epsilon
            || self.vocals > epsilon
            || self.instruments > epsilon
            || self.air > epsilon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_ranges_fall_back_per_channel() {
        let mut ranges = AudioBandRanges::default();
        ranges.set(
            AudioBandKind::Vocals,
            FrequencyRange {
                low_hz: 2_000.0,
                high_hz: 180.0,
            },
        );
        assert_eq!(
            ranges.get(AudioBandKind::Vocals),
            AudioBandKind::Vocals.default_range()
        );
        assert_eq!(
            ranges.get(AudioBandKind::Kick),
            AudioBandKind::Kick.default_range()
        );
    }

    #[test]
    fn control_point_numbers_are_one_based_and_row_major() {
        let points = ControlPointSet::from_numbers(&[7, 13, 19, 1, 25]);
        assert_eq!(points.point_numbers(), [7, 13, 19]);
        assert!(points.contains(6));
        assert!(points.contains(12));
        assert!(points.contains(18));
        assert!(!points.contains(0));
        assert_eq!(points.display(), "7,13,19");
    }

    #[test]
    fn control_point_parser_accepts_separators_and_rejects_borders() {
        let points = parse_control_point_set("7, 8；12/19，").unwrap();
        assert_eq!(points.display(), "7,8,12,19");
        assert_eq!(
            parse_control_point_set("1"),
            Err(ControlPointParseError::BoundaryOrOutOfRange(1))
        );
        assert_eq!(
            parse_control_point_set("13,x"),
            Err(ControlPointParseError::InvalidNumber)
        );
        assert!(parse_control_point_set("").unwrap().is_empty());
    }

    #[test]
    fn default_mapping_only_uses_interior_points() {
        for kind in AudioBandKind::ALL {
            let points = AudioControlMapping::default().get(kind);
            assert!(!points.is_empty());
            for number in points.point_numbers() {
                assert!(is_interior_point_number(number));
            }
        }
    }
}
