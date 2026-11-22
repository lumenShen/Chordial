//! 频段能量分析：歌词背景「鼓点」跳动的数据源。
//!
//! 播放线程把解码后的交错样本送入 [`BandAnalyzer`]；它维护一个滑动窗口，
//! 按固定节奏做一次 radix-2 FFT，只累加 `[low, high]` Hz 内的频点功率，
//! 再经过快攻击/慢释放的包络跟随器输出 0..1 的鼓点强度。
//!
//! 设计约束：
//! - 缓冲区全部在构造时分配，热路径零分配；
//! - 每秒最多分析 [`ANALYSIS_RATE_HZ`] 次，FFT 长度按采样率自适应
//!   （1024..4096），192 kHz 素材也不会把播放线程压满；
//! - 频段由调用方每帧传入，设置面板修改后下一帧即生效，无需重建分析器。

/// 背景鼓点默认监听频段（Hz）：镲片/军鼓的击打能量集中区。
pub const DEFAULT_BEAT_BAND: (f32, f32) = (4000.0, 10000.0);

/// 分析节奏（次/秒）。40ms 的更新间隔足够让跳动跟上鼓点。
const ANALYSIS_RATE_HZ: f32 = 25.0;
const MIN_WINDOW: usize = 1024;
const MAX_WINDOW: usize = 4096;
const MIN_HZ: f32 = 20.0;
const MAX_HZ: f32 = 22_050.0;
/// 包络：6ms 攻击（鼓点几乎瞬时拉满）、160ms 释放（留出可见的回落）。
const ATTACK_SECS: f32 = 0.006;
const RELEASE_SECS: f32 = 0.16;
/// 映射下限：-60 dBFS 视为静音，0 dBFS 映射到 1。
const DB_FLOOR: f32 = -60.0;

/// 把用户输入的频段收敛到可分析范围；上限不高于下限时回退默认值。
pub fn normalize_band(low_hz: f32, high_hz: f32) -> (f32, f32) {
    let low = if low_hz.is_finite() {
        low_hz.clamp(MIN_HZ, MAX_HZ)
    } else {
        DEFAULT_BEAT_BAND.0
    };
    let high = if high_hz.is_finite() {
        high_hz.clamp(MIN_HZ, MAX_HZ)
    } else {
        DEFAULT_BEAT_BAND.1
    };
    if high > low {
        (low, high)
    } else {
        DEFAULT_BEAT_BAND
    }
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
    level: f32,
}

impl BandAnalyzer {
    pub fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(8_000) as f32;
        // 目标：每次分析覆盖约 40ms 的音频。
        let desired = (rate / ANALYSIS_RATE_HZ) as usize;
        let window_len = desired.next_power_of_two().clamp(MIN_WINDOW, MAX_WINDOW);
        let hop = desired.max(1);

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
            level: 0.0,
        }
    }

    /// 清空滑窗与包络（seek / 暂停 / 切歌时调用）。
    pub fn reset(&mut self) {
        self.mono.fill(0.0);
        self.re.fill(0.0);
        self.im.fill(0.0);
        self.write = 0;
        self.filled = 0;
        self.since_hop = 0;
        self.level = 0.0;
    }

    /// 当前鼓点包络（0..1）。
    pub fn level(&self) -> f32 {
        self.level
    }

    /// 送入一帧交错 PCM。`channels` 为声道数（>0）。
    pub fn push_interleaved(
        &mut self,
        interleaved: &[f32],
        channels: usize,
        low_hz: f32,
        high_hz: f32,
    ) {
        if channels == 0 || interleaved.is_empty() {
            return;
        }
        let gain = 1.0 / channels as f32;
        let frames = interleaved.len() / channels;
        let (low_hz, high_hz) = normalize_band(low_hz, high_hz);
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
                self.analyze(low_hz, high_hz);
            }
        }
    }

    fn analyze(&mut self, low_hz: f32, high_hz: f32) {
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

        let bin_hz = self.sample_rate / n as f32;
        let max_bin = n / 2 - 1;
        let k_min = ((low_hz / bin_hz).ceil() as usize).max(1);
        let k_max = ((high_hz / bin_hz).floor() as usize).min(max_bin);
        let mut power = 0.0f32;
        for k in k_min..=k_max {
            let re = self.re[k];
            let im = self.im[k];
            power += re * re + im * im;
        }
        // Parseval：单边带功率 → 窗内信号的频段 RMS。
        let rms = if power > 0.0 {
            (2.0 * power / (n as f32 * self.window_energy)).sqrt()
        } else {
            0.0
        };
        let db = 20.0 * rms.max(1e-7).log10();
        let target = ((db - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0);

        let dt = self.hop as f32 / self.sample_rate;
        let coeff = if target > self.level {
            1.0 - (-dt / ATTACK_SECS).exp()
        } else {
            1.0 - (-dt / RELEASE_SECS).exp()
        };
        self.level += (target - self.level) * coeff;
        if !self.level.is_finite() {
            self.level = 0.0;
        }
        self.level = self.level.clamp(0.0, 1.0);
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

    const RATE: u32 = 48_000;

    fn feed_sine(analyzer: &mut BandAnalyzer, freq: f32, amp: f32, secs: f32, band: (f32, f32)) {
        let frames = (RATE as f32 * secs) as usize;
        let mut block = Vec::with_capacity(4096 * 2);
        for i in 0..frames {
            let s = amp * (std::f32::consts::TAU * freq * i as f32 / RATE as f32).sin();
            block.push(s);
            block.push(s);
            if block.len() >= 4096 * 2 {
                analyzer.push_interleaved(&block, 2, band.0, band.1);
                block.clear();
            }
        }
        if !block.is_empty() {
            analyzer.push_interleaved(&block, 2, band.0, band.1);
        }
    }

    #[test]
    fn normalize_band_falls_back_on_invalid_range() {
        assert_eq!(normalize_band(10_000.0, 4_000.0), DEFAULT_BEAT_BAND);
        assert_eq!(normalize_band(f32::NAN, 10_000.0), DEFAULT_BEAT_BAND);
        assert_eq!(normalize_band(4_000.0, 10_000.0), (4_000.0, 10_000.0));
        assert_eq!(normalize_band(1.0, 99_000.0), (MIN_HZ, MAX_HZ));
    }

    #[test]
    fn in_band_sine_drives_level_high() {
        let mut analyzer = BandAnalyzer::new(RATE);
        feed_sine(&mut analyzer, 6_000.0, 0.6, 0.8, DEFAULT_BEAT_BAND);
        assert!(
            analyzer.level() > 0.7,
            "in-band level too low: {}",
            analyzer.level()
        );
    }

    #[test]
    fn out_of_band_sine_stays_low() {
        let mut analyzer = BandAnalyzer::new(RATE);
        feed_sine(&mut analyzer, 200.0, 0.9, 0.8, DEFAULT_BEAT_BAND);
        assert!(
            analyzer.level() < 0.2,
            "out-of-band leak too high: {}",
            analyzer.level()
        );
    }

    #[test]
    fn level_decays_after_silence() {
        let mut analyzer = BandAnalyzer::new(RATE);
        feed_sine(&mut analyzer, 6_000.0, 0.6, 0.5, DEFAULT_BEAT_BAND);
        let peak = analyzer.level();
        feed_sine(&mut analyzer, 6_000.0, 0.0, 1.0, DEFAULT_BEAT_BAND);
        assert!(peak > 0.5, "peak too low: {peak}");
        assert!(
            analyzer.level() < 0.05,
            "level did not decay: {}",
            analyzer.level()
        );
    }

    #[test]
    fn reset_clears_level() {
        let mut analyzer = BandAnalyzer::new(RATE);
        feed_sine(&mut analyzer, 6_000.0, 0.6, 0.4, DEFAULT_BEAT_BAND);
        analyzer.reset();
        assert_eq!(analyzer.level(), 0.0);
        let quiet = vec![0.0f32; 4800];
        analyzer.push_interleaved(&quiet, 2, DEFAULT_BEAT_BAND.0, DEFAULT_BEAT_BAND.1);
        assert!(analyzer.level() < 0.01, "{}", analyzer.level());
    }
}
