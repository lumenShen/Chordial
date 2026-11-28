//! 音频输出后端。
//!
//! 平台实现：
//! - **Linux**：[`alsa`] 运行时 `dlopen("libasound.so.2")` + 手写最小 FFI，
//!   零构建期依赖，缺库时优雅降级；
//! - **Windows**：[`wasapi`] 原生 WASAPI 共享模式（f32 交错 +
//!   `AUTOCONVERTPCM` 系统重采样）；
//! - **Android**：[`aaudio`] 链接 NDK `libaaudio.so`（API 26+），低延迟共享流；
//! - 其他平台：`open_sink` 返回 Err，由调用方降级 [`NullSink`]。
//!
//! 契约：[`AudioSink::write`] 接收 f32 交错帧样本，返回写入的**样本数**；
//! 全部实现都是 `Send`（播放线程独占）。

#[cfg(target_os = "android")]
mod aaudio;
#[cfg(target_os = "linux")]
mod alsa;
#[cfg(windows)]
mod wasapi;

#[cfg(target_os = "android")]
pub use aaudio::AAudioSink;
#[cfg(target_os = "linux")]
pub use alsa::AlsaSink;
#[cfg(windows)]
pub use wasapi::WasapiSink;

use chordal_core::error::CoreResult;

/// f32 交错 PCM 输出端。
pub trait AudioSink: Send {
    /// 写入交错样本（len 为样本数 = 帧数 × 声道数）。返回写入的样本数。
    /// 阻塞语义：缓冲满时等待（背压 = 播放时钟）。
    fn write(&mut self, interleaved: &[f32]) -> CoreResult<usize>;
    /// 丢弃输出缓冲（暂停/跳歌时用）。
    fn flush(&mut self) -> CoreResult<()>;
    /// 播完缓冲后返回（曲末）。
    fn drain(&mut self) -> CoreResult<()>;
    fn device_name(&self) -> &str;
    /// 已写入但尚未从扬声器播出的帧数（音频时钟 = 已写帧 - delay）。
    /// 无精确时钟的后端返回 0。
    fn delay_frames(&self) -> u64 {
        0
    }
}

/// 打开输出设备；`device=None` 用系统默认设备。
/// 失败（库缺失/设备被占）返回 Err，调用方可降级 [`NullSink`]。
#[cfg(target_os = "linux")]
pub fn open_sink(
    device: Option<&str>,
    sample_rate: u32,
    channels: usize,
) -> CoreResult<Box<dyn AudioSink>> {
    AlsaSink::open(device, sample_rate, channels).map(|s| Box::new(s) as Box<dyn AudioSink>)
}

/// 打开输出设备；`device` 为 WASAPI 端点 ID，`None` 用系统默认设备。
/// 失败返回 Err，调用方可降级 [`NullSink`]。
#[cfg(windows)]
pub fn open_sink(
    device: Option<&str>,
    sample_rate: u32,
    channels: usize,
) -> CoreResult<Box<dyn AudioSink>> {
    WasapiSink::open(device, sample_rate, channels).map(|s| Box::new(s) as Box<dyn AudioSink>)
}

/// 打开 Android AAudio 输出；`device` 仅用于诊断，路由交给系统。
/// 失败返回 Err，调用方可降级 [`NullSink`]。
#[cfg(target_os = "android")]
pub fn open_sink(
    device: Option<&str>,
    sample_rate: u32,
    channels: usize,
) -> CoreResult<Box<dyn AudioSink>> {
    AAudioSink::open(device, sample_rate, channels).map(|s| Box::new(s) as Box<dyn AudioSink>)
}

/// 其他平台没有音频输出后端；调用方降级 [`NullSink`]。
#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
pub fn open_sink(
    device: Option<&str>,
    sample_rate: u32,
    channels: usize,
) -> CoreResult<Box<dyn AudioSink>> {
    let _ = (device, sample_rate, channels);
    Err(chordal_core::error::CoreError::Other(
        "no audio output backend on this platform (ALSA/WASAPI/AAudio only)".into(),
    ))
}

// ---------- 静音/无头后端（测试与降级） ----------

/// 立即消费写入样本的无头后端：CI/无声卡环境保持播放管线可测。
pub struct NullSink {
    consumed: u64,
}

impl NullSink {
    pub fn new() -> Self {
        Self { consumed: 0 }
    }

    pub fn consumed_samples(&self) -> u64 {
        self.consumed
    }
}

impl Default for NullSink {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSink for NullSink {
    fn write(&mut self, interleaved: &[f32]) -> CoreResult<usize> {
        self.consumed += interleaved.len() as u64;
        Ok(interleaved.len())
    }

    fn flush(&mut self) -> CoreResult<()> {
        Ok(())
    }

    fn drain(&mut self) -> CoreResult<()> {
        Ok(())
    }

    fn device_name(&self) -> &str {
        "null"
    }
}

/// 以实时时钟节流的无头后端：`write` 睡到样本对应的真实时间点，
/// 用以在没有声卡的环境里模拟音频时钟（暂停/进度/时长测试可用）。
pub struct RealtimeNullSink {
    consumed_samples: u64,
    rate: u32,
    start: std::time::Instant,
}

impl RealtimeNullSink {
    pub fn new(rate: u32) -> Self {
        Self {
            consumed_samples: 0,
            rate,
            start: std::time::Instant::now(),
        }
    }
}

impl AudioSink for RealtimeNullSink {
    fn write(&mut self, interleaved: &[f32]) -> CoreResult<usize> {
        self.consumed_samples += interleaved.len() as u64;
        // frames = samples / channels；这里以 2ch 假设节流（测试用，精度足够）。
        let frames = self.consumed_samples / 2;
        let expected = std::time::Duration::from_secs_f64(frames as f64 / self.rate as f64);
        let elapsed = self.start.elapsed();
        if expected > elapsed {
            std::thread::sleep(expected - elapsed);
        }
        Ok(interleaved.len())
    }

    fn flush(&mut self) -> CoreResult<()> {
        Ok(())
    }

    fn drain(&mut self) -> CoreResult<()> {
        Ok(())
    }

    fn device_name(&self) -> &str {
        "realtime-null"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_sink_counts_samples() {
        let mut sink = NullSink::new();
        assert_eq!(sink.write(&[0.0f32; 4096]).unwrap(), 4096);
        assert_eq!(sink.write(&[0.0f32; 100]).unwrap(), 100);
        sink.drain().unwrap();
        sink.flush().unwrap();
    }
}
