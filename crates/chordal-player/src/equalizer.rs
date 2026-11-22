//! 参数均衡器：RBJ 双二阶滤波器链。
//!
//! 播放线程在把样本写入输出设备之前调用 [`Equalizer::process`]：
//! 输入增益 → 每个频段（峰值 / 低架 / 高架 / 低通 / 高通）→ 输出增益。
//! 频段数量、频率、增益、Q 都由 UI 编辑；参数变化时系数在 64 帧粒度上
//! 向目标值收敛，增益按 5ms 一阶平滑，避免拖动滑杆时的 zipper noise。
//!
//! 该模块只依赖 `serde` 与标准库，UI 也能用 [`EqSettings::response_db`]
//! 绘制同一条响应曲线——屏幕上看到的曲线与实际听到的滤波是同一组系数。

use std::f32::consts::TAU;

use serde::{Deserialize, Serialize};

pub const EQ_MIN_HZ: f32 = 20.0;
pub const EQ_MAX_HZ: f32 = 20_000.0;
pub const EQ_MIN_GAIN_DB: f32 = -24.0;
pub const EQ_MAX_GAIN_DB: f32 = 24.0;
pub const EQ_MIN_Q: f32 = 0.1;
pub const EQ_MAX_Q: f32 = 12.0;
pub const EQ_MAX_BANDS: usize = 32;

/// UI 绘图与默认配置使用的参考采样率。
pub const EQ_REFERENCE_RATE: f32 = 48_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    /// 峰值（视频里的「峰值」）。
    #[default]
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

impl FilterKind {
    pub const ALL: [Self; 5] = [
        Self::Peaking,
        Self::LowShelf,
        Self::HighShelf,
        Self::LowPass,
        Self::HighPass,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Peaking => "峰值",
            Self::LowShelf => "低架",
            Self::HighShelf => "高架",
            Self::LowPass => "低通",
            Self::HighPass => "高通",
        }
    }

    /// 只有增益类滤波器受 gain 影响。
    pub fn uses_gain(self) -> bool {
        !matches!(self, Self::LowPass | Self::HighPass)
    }
}

fn default_q() -> f32 {
    0.71
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub kind: FilterKind,
    pub freq_hz: f32,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default = "default_q")]
    pub q: f32,
}

impl Default for EqBand {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: FilterKind::Peaking,
            freq_hz: 1_000.0,
            gain_db: 0.0,
            q: 0.71,
        }
    }
}

impl EqBand {
    pub fn new(freq_hz: f32, gain_db: f32, q: f32) -> Self {
        Self {
            freq_hz,
            gain_db,
            q,
            ..Self::default()
        }
    }

    pub fn sanitized(mut self) -> Self {
        self.freq_hz = if self.freq_hz.is_finite() {
            self.freq_hz.clamp(EQ_MIN_HZ, EQ_MAX_HZ)
        } else {
            1_000.0
        };
        self.gain_db = if self.gain_db.is_finite() {
            self.gain_db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB)
        } else {
            0.0
        };
        self.q = if self.q.is_finite() {
            self.q.clamp(EQ_MIN_Q, EQ_MAX_Q)
        } else {
            default_q()
        };
        self
    }

    fn coeffs(&self, sample_rate: f32) -> Biquad {
        if !self.enabled {
            return Biquad::identity();
        }
        Biquad::new(self.kind, sample_rate, self.freq_hz, self.gain_db, self.q)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub input_gain_db: f32,
    #[serde(default)]
    pub output_gain_db: f32,
    #[serde(default)]
    pub bands: Vec<EqBand>,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            input_gain_db: 0.0,
            output_gain_db: 0.0,
            bands: Vec::new(),
        }
    }
}

impl EqSettings {
    pub fn sanitized(mut self) -> Self {
        self.input_gain_db = if self.input_gain_db.is_finite() {
            self.input_gain_db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB)
        } else {
            0.0
        };
        self.output_gain_db = if self.output_gain_db.is_finite() {
            self.output_gain_db.clamp(EQ_MIN_GAIN_DB, EQ_MAX_GAIN_DB)
        } else {
            0.0
        };
        self.bands = self
            .bands
            .into_iter()
            .take(EQ_MAX_BANDS)
            .map(EqBand::sanitized)
            .collect();
        self
    }

    pub fn with_bands(bands: impl IntoIterator<Item = EqBand>) -> Self {
        Self {
            enabled: true,
            bands: bands.into_iter().collect(),
            ..Self::default()
        }
    }

    /// 组合频响（dB）。UI 曲线与实际滤波器共用这一函数。
    pub fn response_db(&self, freq_hz: f32, sample_rate: f32) -> f32 {
        let mut db = self.input_gain_db + self.output_gain_db;
        for band in &self.bands {
            if band.enabled {
                db += band
                    .coeffs(sample_rate)
                    .magnitude_db(freq_hz.max(1.0), sample_rate);
            }
        }
        db
    }
}

#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn identity() -> Self {
        Self::IDENTITY
    }

    fn new(kind: FilterKind, sample_rate: f32, freq_hz: f32, gain_db: f32, q: f32) -> Self {
        let fs = sample_rate.max(8_000.0);
        // 频率必须留在 Nyquist 以下；44.1k 播放 20kHz 时自动收敛到 0.45fs。
        let f = freq_hz.clamp(EQ_MIN_HZ, fs * 0.45);
        let w0 = TAU * f / fs;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let q = q.clamp(EQ_MIN_Q, EQ_MAX_Q);

        let mut c = match kind {
            FilterKind::Peaking => {
                let a = 10f32.powf(gain_db / 40.0);
                let alpha = sin_w0 / (2.0 * q);
                let b0 = 1.0 + alpha * a;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0 - alpha * a;
                let a0 = 1.0 + alpha / a;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha / a;
                normalize(b0, b1, b2, a0, a1, a2)
            }
            FilterKind::LowShelf => {
                let a = 10f32.powf(gain_db / 40.0);
                let sqrt_a = a.sqrt();
                let s = q.clamp(0.1, 4.0);
                let alpha = sin_w0 / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
                let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
                let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
                let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
                let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
                let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;
                normalize(b0, b1, b2, a0, a1, a2)
            }
            FilterKind::HighShelf => {
                let a = 10f32.powf(gain_db / 40.0);
                let sqrt_a = a.sqrt();
                let s = q.clamp(0.1, 4.0);
                let alpha = sin_w0 / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
                let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
                let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
                let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
                let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
                let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;
                normalize(b0, b1, b2, a0, a1, a2)
            }
            FilterKind::LowPass => {
                let alpha = sin_w0 / (2.0 * q);
                let b0 = (1.0 - cos_w0) / 2.0;
                let b1 = 1.0 - cos_w0;
                let b2 = (1.0 - cos_w0) / 2.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                normalize(b0, b1, b2, a0, a1, a2)
            }
            FilterKind::HighPass => {
                let alpha = sin_w0 / (2.0 * q);
                let b0 = (1.0 + cos_w0) / 2.0;
                let b1 = -(1.0 + cos_w0);
                let b2 = (1.0 + cos_w0) / 2.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                normalize(b0, b1, b2, a0, a1, a2)
            }
        };

        // 极端参数下出现非有限系数时退化为直通，绝不让 NaN 进入输出。
        if !c.b0.is_finite()
            || !c.b1.is_finite()
            || !c.b2.is_finite()
            || !c.a1.is_finite()
            || !c.a2.is_finite()
        {
            c = Self::IDENTITY;
        }
        c
    }

    fn magnitude_db(&self, freq_hz: f32, sample_rate: f32) -> f32 {
        let w = TAU * freq_hz.clamp(1.0, sample_rate * 0.49) / sample_rate.max(8_000.0);
        let (sin_w, cos_w) = w.sin_cos();
        let cos_2w = 2.0 * cos_w * cos_w - 1.0;
        let sin_2w = 2.0 * sin_w * cos_w;

        let num_re = self.b0 + self.b1 * cos_w + self.b2 * cos_2w;
        let num_im = -(self.b1 * sin_w + self.b2 * sin_2w);
        let den_re = 1.0 + self.a1 * cos_w + self.a2 * cos_2w;
        let den_im = -(self.a1 * sin_w + self.a2 * sin_2w);
        let num = (num_re * num_re + num_im * num_im).sqrt();
        let den = (den_re * den_re + den_im * den_im).sqrt();
        if den <= f32::EPSILON {
            0.0
        } else {
            20.0 * (num / den).max(1e-9).log10()
        }
    }
}

#[inline]
fn normalize(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Biquad {
    if a0.abs() <= f32::EPSILON {
        return Biquad::IDENTITY;
    }
    let inv = 1.0 / a0;
    Biquad {
        b0: b0 * inv,
        b1: b1 * inv,
        b2: b2 * inv,
        a1: a1 * inv,
        a2: a2 * inv,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, c: Biquad, x: f32) -> f32 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}

/// 播放线程独占的均衡器实例。每个声道维护独立的滤波器状态。
pub struct Equalizer {
    sample_rate: f32,
    channels: usize,
    settings: EqSettings,
    version: u64,
    active: bool,
    target: Vec<Biquad>,
    current: Vec<Biquad>,
    states: Vec<BiquadState>,
    smoothing_frames: usize,
    current_input: f32,
    current_output: f32,
    target_input: f32,
    target_output: f32,
    gain_alpha: f32,
}

impl Equalizer {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let channels = channels.max(1);
        let sample_rate = sample_rate.max(8_000) as f32;
        Self {
            sample_rate,
            channels,
            settings: EqSettings::default(),
            version: 0,
            active: false,
            target: Vec::new(),
            current: Vec::new(),
            states: Vec::new(),
            smoothing_frames: 0,
            current_input: 1.0,
            current_output: 1.0,
            target_input: 1.0,
            target_output: 1.0,
            // 5ms 一阶平滑：α = 1 - e^(-1/(0.005·fs))。
            gain_alpha: 1.0 - (-1.0 / (0.005 * sample_rate)).exp(),
        }
    }

    pub fn settings(&self) -> &EqSettings {
        &self.settings
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// 载入新配置。版本一致时零开销返回。
    pub fn load(&mut self, settings: EqSettings, version: u64) -> bool {
        if version == self.version && self.active == settings.enabled {
            return false;
        }
        self.version = version;
        self.settings = settings.sanitized();
        self.target = self
            .settings
            .bands
            .iter()
            .map(|b| b.coeffs(self.sample_rate))
            .collect();
        self.target_input = 10f32.powf(self.settings.input_gain_db / 20.0);
        self.target_output = 10f32.powf(self.settings.output_gain_db / 20.0);
        self.active = self.settings.enabled;
        if !self.active {
            self.current.clear();
            self.states.clear();
            self.smoothing_frames = 0;
            return true;
        }
        let need_states = self.channels * self.target.len();
        if self.states.len() != need_states || self.current.len() != self.target.len() {
            self.current = self.target.clone();
            self.states = vec![BiquadState::default(); need_states];
        } else {
            self.smoothing_frames = 64;
        }
        true
    }

    pub fn reset(&mut self) {
        self.states.fill(BiquadState::default());
        self.current_input = self.target_input;
        self.current_output = self.target_output;
    }

    /// 就地处理交错样本；未启用时是空操作。
    #[inline]
    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.active {
            return;
        }
        let channels = self.channels;
        let bands = self.current.len();
        if bands == 0 {
            // 没有频段：仅平滑增益。
            for frame in samples.chunks_mut(channels) {
                self.current_input += (self.target_input - self.current_input) * self.gain_alpha;
                self.current_output += (self.target_output - self.current_output) * self.gain_alpha;
                for s in frame.iter_mut() {
                    *s *= self.current_input * self.current_output;
                }
            }
            return;
        }

        for frame in samples.chunks_mut(channels) {
            if self.smoothing_frames > 0 {
                self.smoothing_frames -= 1;
                if self.smoothing_frames == 0 {
                    for (cur, target) in self.current.iter_mut().zip(self.target.iter()) {
                        *cur = *target;
                    }
                } else {
                    // 64 帧内线性靠近目标系数；避免直接替换造成可闻的 click。
                    for (cur, target) in self.current.iter_mut().zip(self.target.iter()) {
                        cur.b0 += (target.b0 - cur.b0) * 0.18;
                        cur.b1 += (target.b1 - cur.b1) * 0.18;
                        cur.b2 += (target.b2 - cur.b2) * 0.18;
                        cur.a1 += (target.a1 - cur.a1) * 0.18;
                        cur.a2 += (target.a2 - cur.a2) * 0.18;
                    }
                }
            }
            self.current_input += (self.target_input - self.current_input) * self.gain_alpha;
            self.current_output += (self.target_output - self.current_output) * self.gain_alpha;
            for (ch, sample) in frame.iter_mut().enumerate() {
                let mut x = *sample * self.current_input;
                let base = ch * bands;
                for (i, coeff) in self.current.iter().enumerate() {
                    x = self.states[base + i].process(*coeff, x);
                }
                *sample = x * self.current_output;
            }
        }
    }
}

/// 常用 10 段图示均衡器的中心频率。
pub const GRAPHIC_10_BANDS: [f32; 10] = [
    31.5, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];

/// 对数频率轴 0..1 映射（20Hz..20kHz）。
pub fn freq_to_norm(freq_hz: f32) -> f32 {
    let lo = EQ_MIN_HZ.ln();
    let hi = EQ_MAX_HZ.ln();
    ((freq_hz.clamp(EQ_MIN_HZ, EQ_MAX_HZ).ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
}

pub fn norm_to_freq(norm: f32) -> f32 {
    let lo = EQ_MIN_HZ.ln();
    let hi = EQ_MAX_HZ.ln();
    ((hi - lo) * norm.clamp(0.0, 1.0) + lo).exp()
}

/// 频率显示：`60 Hz` / `1.2 kHz` / `16k`。
pub fn fmt_freq(freq_hz: f32) -> String {
    if freq_hz >= 10_000.0 {
        format!("{:.0}k", freq_hz / 1_000.0)
    } else if freq_hz >= 1_000.0 {
        let k = freq_hz / 1_000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("{:.0}k", k)
        } else {
            format!("{k:.1}k")
        }
    } else {
        format!("{:.0}", freq_hz)
    }
}

/// 增益显示：`+2.0 dB` / `-1.2 dB` / `0.0 dB`。
pub fn fmt_db(db: f32) -> String {
    if db.abs() < 0.05 {
        "0.0 dB".to_string()
    } else {
        format!("{db:+.1} dB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_settings_are_transparent() {
        let settings = EqSettings::default();
        for f in [20.0, 100.0, 1_000.0, 10_000.0] {
            assert!(settings.response_db(f, EQ_REFERENCE_RATE).abs() < 1e-3);
        }
        let mut eq = Equalizer::new(48_000, 2);
        eq.load(
            EqSettings {
                enabled: true,
                ..EqSettings::default()
            },
            1,
        );
        let mut samples = vec![0.25f32; 64];
        eq.process(&mut samples);
        for s in samples {
            assert!((s - 0.25).abs() < 1e-5);
        }
    }

    #[test]
    fn peaking_band_boosts_its_center() {
        let settings = EqSettings {
            enabled: true,
            bands: vec![EqBand::new(1_000.0, 12.0, 1.0)],
            ..EqSettings::default()
        };
        let center = settings.response_db(1_000.0, 48_000.0);
        let far = settings.response_db(8_000.0, 48_000.0);
        assert!(center > 10.0, "center = {center}");
        assert!(far.abs() < 1.0, "far = {far}");
    }

    #[test]
    fn gain_smoothing_reaches_target() {
        let mut eq = Equalizer::new(48_000, 1);
        eq.load(
            EqSettings {
                enabled: true,
                output_gain_db: 6.0,
                ..EqSettings::default()
            },
            2,
        );
        let mut samples = vec![0.1f32; 48_000 / 2];
        eq.process(&mut samples);
        let last = *samples.last().unwrap();
        let expected = 0.1 * 10f32.powf(6.0 / 20.0);
        assert!((last - expected).abs() < 1e-3, "last = {last}");
    }

    #[test]
    fn freq_axis_roundtrip() {
        for f in [20.0, 100.0, 1_000.0, 20_000.0] {
            let n = freq_to_norm(f);
            let back = norm_to_freq(n);
            assert!((back - f).abs() / f < 1e-4);
        }
    }
}
