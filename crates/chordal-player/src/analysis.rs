//! 多频段能量分析：歌词流体背景的音频反应数据源。
//!
//! 播放线程把解码后的交错样本送入 [`BandAnalyzer`]；它维护一个滑动窗口，
//! 按约 10ms 的 hop 分析一次，FFT 窗口仍覆盖约 40ms 音频。短促鼓点/镲片
//! 会落在多个重叠窗口里，播放线程再保留每个写块内的峰值，避免碎瞬态被
//! 下一次分析或粗粒度状态发布覆盖。
//!
//! 五路输出共享一次分析时刻：[`AudioBands`] 由播放器整份发布，渲染层不会
//! 拼出不同频段来自不同帧的“撕裂快照”。低频持续能量与低频鼓点使用不同的
//! 包络策略：前者跟绝对能量，后者跟自适应基线之上的瞬态。

use chordal_core::{AudioBandKind, AudioBandRanges, AudioBands, normalize_frequency_range};

/// 背景鼓点默认监听频段（Hz）：底鼓的主要能量区。
pub const DEFAULT_BEAT_BAND: (f32, f32) = (35.0, 160.0);

/// 分析节奏（次/秒）。10ms hop 用于捕捉碎片化鼓点/镲片，与屏幕刷新率无关。
const ANALYSIS_RATE_HZ: f32 = 100.0;
/// FFT 窗口目标时长（秒）：保留低频分辨率，同时通过高重叠率捕捉短瞬态。
const WINDOW_SECS: f32 = 0.04;
const MIN_WINDOW: usize = 1024;
const MAX_WINDOW: usize = 4096;
/// 映射下限：-60 dBFS 视为静音，0 dBFS 映射到 1。
const DB_FLOOR: f32 = -60.0;

const RUMBLE_BAND: (f32, f32) = (25.0, 120.0);
const VOCALS_BAND: (f32, f32) = (180.0, 2_000.0);
const INSTRUMENTS_BAND: (f32, f32) = (700.0, 6_000.0);
const AIR_BAND: (f32, f32) = (6_000.0, 16_000.0);

#[derive(Clone, Copy)]
struct BandSpec {
    low_hz: f32,
    high_hz: f32,
    attack_secs: f32,
    release_secs: f32,
    baseline_secs: f32,
    /// 绝对能量保留比例；越高越适合持续纹理，越低越突出击打。
    floor_mix: f32,
    /// 相对自适应基线的正向瞬态增益。
    onset_gain: f32,
    db_floor: f32,
}

const KICK_SPEC: BandSpec = BandSpec {
    low_hz: DEFAULT_BEAT_BAND.0,
    high_hz: DEFAULT_BEAT_BAND.1,
    attack_secs: 0.006,
    release_secs: 0.16,
    baseline_secs: 0.9,
    floor_mix: 0.35,
    onset_gain: 2.2,
    db_floor: DB_FLOOR,
};

const FIXED_SPECS: [BandSpec; 4] = [
    BandSpec {
        low_hz: RUMBLE_BAND.0,
        high_hz: RUMBLE_BAND.1,
        attack_secs: 0.08,
        release_secs: 0.24,
        baseline_secs: 1.2,
        floor_mix: 0.9,
        onset_gain: 0.25,
        db_floor: -55.0,
    },
    BandSpec {
        low_hz: VOCALS_BAND.0,
        high_hz: VOCALS_BAND.1,
        attack_secs: 0.025,
        release_secs: 0.18,
        baseline_secs: 0.9,
        floor_mix: 0.65,
        onset_gain: 1.0,
        db_floor: -55.0,
    },
    BandSpec {
        low_hz: INSTRUMENTS_BAND.0,
        high_hz: INSTRUMENTS_BAND.1,
        attack_secs: 0.018,
        release_secs: 0.22,
        baseline_secs: 0.75,
        floor_mix: 0.55,
        onset_gain: 1.3,
        db_floor: -55.0,
    },
    BandSpec {
        low_hz: AIR_BAND.0,
        high_hz: AIR_BAND.1,
        attack_secs: 0.008,
        release_secs: 0.14,
        baseline_secs: 0.65,
        floor_mix: 0.4,
        onset_gain: 1.8,
        db_floor: -50.0,
    },
];

#[derive(Clone, Copy, Default)]
struct BandEnvelope {
    baseline: f32,
    level: f32,
}

impl BandEnvelope {
    fn update(&mut self, absolute: f32, spec: &BandSpec, dt: f32) {
        let baseline_coeff = 1.0 - (-dt / spec.baseline_secs).exp();
        self.baseline += (absolute - self.baseline) * baseline_coeff;
        if !self.baseline.is_finite() {
            self.baseline = 0.0;
        }
        let onset = (absolute - self.baseline).max(0.0);
        let target = (absolute * spec.floor_mix + onset * spec.onset_gain).clamp(0.0, 1.0);
        let tau = if target > self.level {
            spec.attack_secs
        } else {
            spec.release_secs
        };
        let coeff = 1.0 - (-dt / tau).exp();
        self.level += (target - self.level) * coeff;
        if !self.level.is_finite() {
            self.level = 0.0;
        }
        self.level = self.level.clamp(0.0, 1.0);
    }
}

/// 把用户输入的频段收敛到可分析范围；上限不高于下限时回退默认值。
pub fn normalize_band(low_hz: f32, high_hz: f32) -> (f32, f32) {
    let range = normalize_frequency_range(low_hz, high_hz, AudioBandKind::Kick.default_range());
    (range.low_hz, range.high_hz)
}

/// 播放线程独占的滑窗频谱分析器。
pub struct BandAnalyzer {
    sample_rate: f32,
    window_len: usize,
    hop: usize,
    window: Vec<f32>,
    window_energy: f32,
    /// 单声道环形缓冲，长度 = `window_len`。
    mono: Vec<f32>,
    write: usize,
    filled: usize,
    since_hop: usize,
    re: Vec<f32>,
    im: Vec<f32>,
    rev: Vec<u32>,
    tw_re: Vec<f32>,
    tw_im: Vec<f32>,
    envelopes: [BandEnvelope; 5],
    snapshot: AudioBands,
}

impl BandAnalyzer {
    pub fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(8_000) as f32;
        // 目标：每次分析覆盖约 40ms 的音频；hop 单独设为约 10ms，两者高度重叠。
        let desired_window = (rate * WINDOW_SECS) as usize;
        let window_len = desired_window
            .next_power_of_two()
            .clamp(MIN_WINDOW, MAX_WINDOW);
        let hop = (rate / ANALYSIS_RATE_HZ) as usize;
        let hop = hop.max(1);

        let mut window = Vec::with_capacity(window_len);
        let mut window_energy = 0.0f32;
        for i in 0..window_len {
            // 周期 Hann 窗，能量集中、旁瓣低。
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / window_len as f32).cos();
            window.push(w);
            window_energy += w * w;
        }

        let bits = window_len.trailing_zeros();
        let mut rev = vec![0u32; window_len];
        for (i, slot) in rev.iter_mut().enumerate() {
            *slot = (i as u32).reverse_bits() >> (32 - bits);
        }

        let mut tw_re = Vec::with_capacity(window_len / 2);
        let mut tw_im = Vec::with_capacity(window_len / 2);
        for k in 0..window_len / 2 {
            let angle = -std::f32::consts::TAU * k as f32 / window_len as f32;
            tw_re.push(angle.cos());
            tw_im.push(angle.sin());
        }

        Self {
            sample_rate: rate,
            window_len,
            hop,
            window,
            window_energy,
            mono: vec![0.0; window_len],
            write: 0,
            filled: 0,
            since_hop: 0,
            re: vec![0.0; window_len],
            im: vec![0.0; window_len],
            rev,
            tw_re,
            tw_im,
            envelopes: [BandEnvelope::default(); 5],
            snapshot: AudioBands::default(),
        }
    }

    /// 清空滑窗、包络与快照（seek / 暂停 / 切歌时调用）。
    pub fn reset(&mut self) {
        self.mono.fill(0.0);
        self.re.fill(0.0);
        self.im.fill(0.0);
        self.write = 0;
        self.filled = 0;
        self.since_hop = 0;
        self.envelopes = [BandEnvelope::default(); 5];
        self.snapshot = AudioBands::default();
    }

    /// 当前五路包络快照。所有值共享同一次 FFT 的分析时刻。
    pub fn snapshot(&self) -> AudioBands {
        self.snapshot
    }

    /// 当前鼓点包络（0..1），兼容旧的单频段 API。
    pub fn level(&self) -> f32 {
        self.snapshot.kick
    }

    /// 送入一帧交错 PCM。`channels` 为声道数（>0）。
    ///
    /// `ranges` 一次配置五路频段；下一跳分析即生效，无需重建分析器。返回值
    /// 是本次写块内所有分析跳的逐通道峰值，避免粗粒度写块吞掉碎片瞬态。
    pub fn push_interleaved(
        &mut self,
        interleaved: &[f32],
        channels: usize,
        ranges: AudioBandRanges,
    ) -> AudioBands {
        if channels == 0 || interleaved.is_empty() {
            return AudioBands::default();
        }
        let mut peak = AudioBands::default();
        let mut analyzed = false;
        let gain = 1.0 / channels as f32;
        let frames = interleaved.len() / channels;
        let ranges = ranges.sanitized();
        for frame in 0..frames {
            let base = frame * channels;
            let mut sum = 0.0f32;
            for sample in &interleaved[base..base + channels] {
                sum += *sample;
            }
            self.mono[self.write] = sum * gain;
            self.write += 1;
            if self.write == self.window_len {
                self.write = 0;
            }
            self.filled = (self.filled + 1).min(self.window_len);
            self.since_hop += 1;
            if self.since_hop >= self.hop && self.filled >= self.window_len {
                self.since_hop = 0;
                self.analyze(ranges);
                analyzed = true;
                peak = peak.peak(self.snapshot);
            }
        }
        if analyzed { peak } else { self.snapshot }
    }

    fn analyze(&mut self, ranges: AudioBandRanges) {
        let n = self.window_len;
        let start = self.write;
        for i in 0..n {
            let mut idx = start + i;
            if idx >= n {
                idx -= n;
            }
            self.re[i] = self.mono[idx] * self.window[i];
            self.im[i] = 0.0;
        }
        self.fft();

        let dt = self.hop as f32 / self.sample_rate;
        let with_range = |index: usize, base: BandSpec| {
            let range = ranges.bands[index];
            BandSpec {
                low_hz: range.low_hz,
                high_hz: range.high_hz,
                ..base
            }
        };
        let specs = [
            with_range(0, FIXED_SPECS[0]),
            with_range(1, KICK_SPEC),
            with_range(2, FIXED_SPECS[1]),
            with_range(3, FIXED_SPECS[2]),
            with_range(4, FIXED_SPECS[3]),
        ];

        // 同一份 FFT 上做五次区间积分；带宽归一化避免宽频段仅因为 bin 更多
        // 而天然比低频通道更强。
        let absolutes = [
            self.band_absolute(&specs[0]),
            self.band_absolute(&specs[1]),
            self.band_absolute(&specs[2]),
            self.band_absolute(&specs[3]),
            self.band_absolute(&specs[4]),
        ];
        for (index, absolute) in absolutes.into_iter().enumerate() {
            self.envelopes[index].update(absolute, &specs[index], dt);
        }

        self.snapshot = AudioBands {
            rumble: self.envelopes[0].level,
            kick: self.envelopes[1].level,
            vocals: self.envelopes[2].level,
            instruments: self.envelopes[3].level,
            air: self.envelopes[4].level,
        }
        .sanitized();
    }

    /// 返回一个频段的平均频点能量映射。先对 bin 功率取平均，再按 Parseval
    /// 换算 RMS，使 25–120 Hz 与 6–16 kHz 可以共用同一套 0..1 包络。
    fn band_absolute(&self, spec: &BandSpec) -> f32 {
        let n = self.window_len;
        let bin_hz = self.sample_rate / n as f32;
        let max_bin = n / 2 - 1;
        let nyquist = self.sample_rate * 0.5;
        let high_hz = spec.high_hz.min(nyquist);
        if !(high_hz > spec.low_hz) || spec.low_hz >= nyquist {
            return 0.0;
        }
        let k_min = ((spec.low_hz / bin_hz).ceil() as usize).max(1);
        let k_max = ((high_hz / bin_hz).floor() as usize).min(max_bin);
        if k_min > k_max {
            return 0.0;
        }
        let mut power = 0.0f32;
        for k in k_min..=k_max {
            let re = self.re[k];
            let im = self.im[k];
            power += re * re + im * im;
        }
        let mean_power = power / (k_max - k_min + 1) as f32;
        let rms = if mean_power > 0.0 {
            (2.0 * mean_power / (n as f32 * self.window_energy)).sqrt()
        } else {
            0.0
        };
        let db = 20.0 * rms.max(1e-7).log10();
        ((db - spec.db_floor) / -spec.db_floor).clamp(0.0, 1.0)
    }

    /// 原地 radix-2 Cooley-Tukey FFT：先做位反转置换，再逐级蝶形合并。
    fn fft(&mut self) {
        let n = self.window_len;
        for i in 0..n {
            let j = self.rev[i] as usize;
            if i < j {
                self.re.swap(i, j);
                self.im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for block in (0..n).step_by(len) {
                for k in 0..half {
                    let tw = k * step;
                    let wr = self.tw_re[tw];
                    let wi = self.tw_im[tw];
                    let a = block + k;
                    let b = a + half;
                    let xr = self.re[b] * wr - self.im[b] * wi;
                    let xi = self.re[b] * wi + self.im[b] * wr;
                    self.re[b] = self.re[a] - xr;
                    self.im[b] = self.im[a] - xi;
                    self.re[a] += xr;
                    self.im[a] += xi;
                }
            }
            len <<= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chordal_core::{FrequencyRange, MAX_AUDIO_HZ, MIN_AUDIO_HZ};

    const RATE: u32 = 48_000;

    fn ranges_for_kick(band: (f32, f32)) -> AudioBandRanges {
        let mut ranges = AudioBandRanges::default();
        ranges.set(
            AudioBandKind::Kick,
            FrequencyRange {
                low_hz: band.0,
                high_hz: band.1,
            },
        );
        ranges
    }

    fn peak_bands(peak: &mut AudioBands, sample: AudioBands) {
        peak.rumble = peak.rumble.max(sample.rumble);
        peak.kick = peak.kick.max(sample.kick);
        peak.vocals = peak.vocals.max(sample.vocals);
        peak.instruments = peak.instruments.max(sample.instruments);
        peak.air = peak.air.max(sample.air);
    }

    fn feed_sine(
        analyzer: &mut BandAnalyzer,
        freq: f32,
        amp: f32,
        secs: f32,
        band: (f32, f32),
    ) -> AudioBands {
        feed_sine_with_ranges(analyzer, freq, amp, secs, ranges_for_kick(band))
    }

    fn feed_sine_with_ranges(
        analyzer: &mut BandAnalyzer,
        freq: f32,
        amp: f32,
        secs: f32,
        ranges: AudioBandRanges,
    ) -> AudioBands {
        let frames = (RATE as f32 * secs) as usize;
        let mut block = Vec::with_capacity(4096 * 2);
        let mut peak = AudioBands::default();
        for i in 0..frames {
            let s = amp * (std::f32::consts::TAU * freq * i as f32 / RATE as f32).sin();
            block.push(s);
            block.push(s);
            if block.len() >= 4096 * 2 {
                analyzer.push_interleaved(&block, 2, ranges);
                peak_bands(&mut peak, analyzer.snapshot());
                block.clear();
            }
        }
        if !block.is_empty() {
            analyzer.push_interleaved(&block, 2, ranges);
            peak_bands(&mut peak, analyzer.snapshot());
        }
        peak
    }

    #[test]
    fn normalize_band_falls_back_on_invalid_range() {
        assert_eq!(normalize_band(10_000.0, 4_000.0), DEFAULT_BEAT_BAND);
        assert_eq!(normalize_band(f32::NAN, 10_000.0), DEFAULT_BEAT_BAND);
        assert_eq!(normalize_band(4_000.0, 10_000.0), (4_000.0, 10_000.0));
        assert_eq!(normalize_band(1.0, 99_000.0), (MIN_AUDIO_HZ, MAX_AUDIO_HZ));
    }

    #[test]
    fn in_band_sine_drives_kick_high() {
        let mut analyzer = BandAnalyzer::new(RATE);
        let peak = feed_sine(&mut analyzer, 90.0, 0.6, 0.3, DEFAULT_BEAT_BAND);
        assert!(peak.kick > 0.5, "in-band kick too low: {}", peak.kick);
    }

    #[test]
    fn out_of_band_sine_stays_low() {
        let mut analyzer = BandAnalyzer::new(RATE);
        let peak = feed_sine(&mut analyzer, 320.0, 0.9, 0.8, DEFAULT_BEAT_BAND);
        assert!(peak.kick < 0.2, "out-of-band leak too high: {}", peak.kick);
    }

    #[test]
    fn level_decays_after_silence() {
        let mut analyzer = BandAnalyzer::new(RATE);
        feed_sine(&mut analyzer, 90.0, 0.6, 0.3, DEFAULT_BEAT_BAND);
        let peak = analyzer.level();
        feed_sine(&mut analyzer, 90.0, 0.0, 1.0, DEFAULT_BEAT_BAND);
        assert!(peak > 0.5, "peak too low: {peak}");
        assert!(
            analyzer.level() < 0.05,
            "level did not decay: {}",
            analyzer.level()
        );
    }

    #[test]
    fn reset_clears_every_band() {
        let mut analyzer = BandAnalyzer::new(RATE);
        feed_sine(&mut analyzer, 90.0, 0.6, 0.4, DEFAULT_BEAT_BAND);
        analyzer.reset();
        assert_eq!(analyzer.snapshot(), AudioBands::default());
        let quiet = vec![0.0f32; 4800];
        analyzer.push_interleaved(&quiet, 2, AudioBandRanges::default());
        assert!(
            analyzer.snapshot().kick < 0.01,
            "{}",
            analyzer.snapshot().kick
        );
    }

    #[test]
    fn transient_hits_stand_out_from_sustained_floor() {
        let mut analyzer = BandAnalyzer::new(RATE);
        feed_sine(&mut analyzer, 90.0, 0.05, 2.0, DEFAULT_BEAT_BAND);
        let floor = analyzer.level();
        let peak = feed_sine(&mut analyzer, 90.0, 0.8, 0.05, DEFAULT_BEAT_BAND).kick;
        assert!(
            peak > floor + 0.3,
            "transient not emphasized: floor={floor:.2} peak={peak:.2}"
        );
    }

    #[test]
    fn low_tone_isolates_rumble_and_kick() {
        let mut analyzer = BandAnalyzer::new(RATE);
        let peak = feed_sine(&mut analyzer, 60.0, 0.5, 0.8, DEFAULT_BEAT_BAND);
        assert!(peak.rumble > 0.2, "rumble too low: {peak:?}");
        assert!(peak.kick > 0.2, "kick too low: {peak:?}");
        assert!(peak.vocals < 0.1, "vocals leaked: {peak:?}");
        assert!(peak.instruments < 0.1, "instruments leaked: {peak:?}");
        assert!(peak.air < 0.1, "air leaked: {peak:?}");
    }

    #[test]
    fn vocal_tone_does_not_drive_low_or_air_bands() {
        let mut analyzer = BandAnalyzer::new(RATE);
        let peak = feed_sine(&mut analyzer, 1_000.0, 0.5, 0.8, DEFAULT_BEAT_BAND);
        assert!(peak.vocals > 0.2, "vocals too low: {peak:?}");
        assert!(peak.instruments > 0.2, "instruments too low: {peak:?}");
        assert!(peak.rumble < 0.1, "rumble leaked: {peak:?}");
        assert!(peak.kick < 0.1, "kick leaked: {peak:?}");
        assert!(peak.air < 0.1, "air leaked: {peak:?}");
    }

    #[test]
    fn instrument_and_air_bands_are_separate() {
        let mut analyzer = BandAnalyzer::new(RATE);
        let instruments = feed_sine(&mut analyzer, 3_000.0, 0.5, 0.8, DEFAULT_BEAT_BAND);
        assert!(
            instruments.instruments > 0.2,
            "instruments too low: {instruments:?}"
        );
        assert!(instruments.air < 0.1, "air leaked: {instruments:?}");

        analyzer.reset();
        let air = feed_sine(&mut analyzer, 10_000.0, 0.5, 0.8, DEFAULT_BEAT_BAND);
        assert!(air.air > 0.2, "air too low: {air:?}");
        assert!(air.instruments < 0.1, "instruments leaked: {air:?}");
    }

    #[test]
    fn custom_ranges_route_energy_to_the_requested_channel() {
        let mut ranges = AudioBandRanges::default();
        ranges.set(
            AudioBandKind::Vocals,
            FrequencyRange {
                low_hz: 8_000.0,
                high_hz: 12_000.0,
            },
        );
        ranges.set(
            AudioBandKind::Air,
            FrequencyRange {
                low_hz: 100.0,
                high_hz: 200.0,
            },
        );
        let mut analyzer = BandAnalyzer::new(RATE);
        let peak = feed_sine_with_ranges(&mut analyzer, 10_000.0, 0.5, 0.8, ranges);
        assert!(peak.vocals > 0.2, "custom vocal range ignored: {peak:?}");
        assert!(peak.air < 0.1, "old air range still active: {peak:?}");
    }

    #[test]
    fn bands_above_nyquist_stay_silent() {
        let mut analyzer = BandAnalyzer::new(8_000);
        let frames = 8_000;
        let mut block = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let sample = 0.4 * (std::f32::consts::TAU * 1_000.0 * i as f32 / 8_000.0).sin();
            block.extend_from_slice(&[sample, sample]);
        }
        analyzer.push_interleaved(&block, 2, AudioBandRanges::default());
        assert_eq!(analyzer.snapshot().air, 0.0);
    }

    #[test]
    fn analysis_hop_is_independent_of_window_size() {
        let analyzer = BandAnalyzer::new(RATE);
        assert_eq!(analyzer.hop, 480, "48kHz should analyze every 10ms");
        assert_eq!(
            analyzer.window_len, 2048,
            "low bands still need a ~40ms window"
        );
    }

    #[test]
    fn write_block_peak_preserves_fragmented_high_frequency_hits() {
        let mut analyzer = BandAnalyzer::new(RATE);
        feed_sine(&mut analyzer, 10_000.0, 0.0, 0.1, DEFAULT_BEAT_BAND);
        let frames = 2048;
        let mut block = vec![0.0f32; frames * 2];
        // 只在写块开头放一个 5ms 的镲片，随后静音。最终分析帧已经回落，
        // 但写块峰值必须保留这次击打。
        for frame in 0..240 {
            let sample =
                0.8 * (std::f32::consts::TAU * 10_000.0 * frame as f32 / RATE as f32).sin();
            block[frame * 2] = sample;
            block[frame * 2 + 1] = sample;
        }
        let peak = analyzer.push_interleaved(&block, 2, AudioBandRanges::default());
        assert!(peak.air > 0.3, "fragmented hit was lost: {peak:?}");
        assert!(
            peak.air > analyzer.snapshot().air + 0.01,
            "peak should outlive the final analysis: peak={peak:?} final={:?}",
            analyzer.snapshot()
        );
    }

    #[test]
    fn snapshots_are_finite_and_clamped() {
        let mut analyzer = BandAnalyzer::new(RATE);
        feed_sine(&mut analyzer, 90.0, 4.0, 0.5, DEFAULT_BEAT_BAND);
        let bands = analyzer.snapshot();
        for value in [
            bands.rumble,
            bands.kick,
            bands.vocals,
            bands.instruments,
            bands.air,
        ] {
            assert!(value.is_finite() && (0.0..=1.0).contains(&value));
        }
    }
}
