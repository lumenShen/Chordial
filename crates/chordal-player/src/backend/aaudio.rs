//! Android AAudio 输出后端。
//!
//! 直接链接 NDK 的 `libaaudio.so`（API 26 起提供，与 minSdk 一致），
//! 使用阻塞写模式 + `AAUDIO_PERFORMANCE_MODE_LOW_LATENCY` 共享流。
//! 失败时由调用方降级 [`super::NullSink`]，与 ALSA/WASAPI 行为一致。
//!
//! 时钟语义与 ALSA 对齐：`write` 接收 f32 交错**样本**，内部换算成帧；
//! `delay_frames` 返回“已写未播”的帧数（音频时钟 = 已写帧 - delay）。

use std::ffi::c_void;
use std::ptr;
use std::time::{Duration, Instant};

use chordal_core::error::{CoreError, CoreResult};

use super::AudioSink;

// ---- AAudio ABI 常量（aaudio/AAudio.h，API 26） ----
const AAUDIO_OK: i32 = 0;
const AAUDIO_DIRECTION_OUTPUT: i32 = 0;
const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
const AAUDIO_SHARING_MODE_SHARED: i32 = 1;
const AAUDIO_PERFORMANCE_MODE_LOW_LATENCY: i32 = 12;

const AAUDIO_ERROR_DISCONNECTED: i32 = -899;
const AAUDIO_ERROR_INVALID_STATE: i32 = -896;
const AAUDIO_ERROR_TIMEOUT: i32 = -889;
const AAUDIO_ERROR_WOULD_BLOCK: i32 = -888;

// AAudio 流状态：0 UNINITIALIZED / 1 UNKNOWN / 2 OPEN / 3 STARTING / 4 STARTED /
// 5 PAUSING / 6 PAUSED / 7 FLUSHING / 8 FLUSHED / 9 STOPPING / 10 STOPPED …
const AAUDIO_STREAM_STATE_PAUSED: i32 = 6;
const AAUDIO_STREAM_STATE_FLUSHED: i32 = 8;

/// 单次阻塞写的上限；超时后继续循环，避免把临时缓冲满当成硬错误。
const WRITE_TIMEOUT_NS: i64 = 500_000_000;

#[repr(C)]
pub struct AAudioStreamBuilder {
    _private: [u8; 0],
}

#[repr(C)]
pub struct AAudioStream {
    _private: [u8; 0],
}

#[link(name = "aaudio")]
unsafe extern "C" {
    fn AAudio_createStreamBuilder(builder: *mut *mut AAudioStreamBuilder) -> i32;
    fn AAudioStreamBuilder_setDirection(builder: *mut AAudioStreamBuilder, direction: i32);
    fn AAudioStreamBuilder_setSharingMode(builder: *mut AAudioStreamBuilder, mode: i32);
    fn AAudioStreamBuilder_setPerformanceMode(builder: *mut AAudioStreamBuilder, mode: i32);
    fn AAudioStreamBuilder_setFormat(builder: *mut AAudioStreamBuilder, format: i32);
    fn AAudioStreamBuilder_setSampleRate(builder: *mut AAudioStreamBuilder, rate: i32);
    fn AAudioStreamBuilder_setChannelCount(builder: *mut AAudioStreamBuilder, channels: i32);
    fn AAudioStreamBuilder_setBufferCapacityInFrames(
        builder: *mut AAudioStreamBuilder,
        frames: i32,
    );
    fn AAudioStreamBuilder_openStream(
        builder: *mut AAudioStreamBuilder,
        stream: *mut *mut AAudioStream,
    ) -> i32;
    fn AAudioStreamBuilder_delete(builder: *mut AAudioStreamBuilder);

    fn AAudioStream_requestStart(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_requestPause(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_requestFlush(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_requestStop(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_write(
        stream: *mut AAudioStream,
        buffer: *const c_void,
        frames: i32,
        timeout_ns: i64,
    ) -> i32;
    fn AAudioStream_getSampleRate(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getChannelCount(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getFramesWritten(stream: *mut AAudioStream) -> i64;
    fn AAudioStream_getFramesRead(stream: *mut AAudioStream) -> i64;
    fn AAudioStream_getState(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_waitForStateChange(
        stream: *mut AAudioStream,
        input_state: i32,
        next_state: *mut i32,
        timeout_ns: i64,
    ) -> i32;
    fn AAudioStream_close(stream: *mut AAudioStream) -> i32;
}

fn result_name(code: i32) -> &'static str {
    match code {
        AAUDIO_ERROR_DISCONNECTED => "disconnected",
        AAUDIO_ERROR_INVALID_STATE => "invalid state",
        AAUDIO_ERROR_TIMEOUT => "timeout",
        AAUDIO_ERROR_WOULD_BLOCK => "would block",
        _ => "error",
    }
}

fn check(rc: i32, what: &str) -> CoreResult<()> {
    if rc == AAUDIO_OK {
        Ok(())
    } else {
        Err(CoreError::Other(format!(
            "{what} failed: {} ({rc})",
            result_name(rc)
        )))
    }
}

pub struct AAudioSink {
    stream: *mut AAudioStream,
    device: String,
    /// 声道数：流内数据是 f32 交错，`AAudioStream_write` 的单位是**帧**。
    channels: usize,
}

// SAFETY: AAudioStream 由播放线程独占，指针可移动（Send），与 ALSA 后端相同。
unsafe impl Send for AAudioSink {}

impl AAudioSink {
    pub fn open(device: Option<&str>, sample_rate: u32, channels: usize) -> CoreResult<Self> {
        // AAudio 的设备选择需要先枚举 deviceId；`device` 目前只用于诊断输出，
        // 实际路由交给系统（耳机/蓝牙插拔由 AudioFlinger 处理）。
        let device = device.unwrap_or("default").to_string();
        let channels = channels.max(1);
        let sample_rate = sample_rate.max(8_000);

        let mut builder: *mut AAudioStreamBuilder = ptr::null_mut();
        check(
            unsafe { AAudio_createStreamBuilder(&mut builder) },
            "AAudio_createStreamBuilder",
        )?;
        if builder.is_null() {
            return Err(CoreError::Other("AAudio builder is null".into()));
        }

        unsafe {
            AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_OUTPUT);
            AAudioStreamBuilder_setSharingMode(builder, AAUDIO_SHARING_MODE_SHARED);
            AAudioStreamBuilder_setPerformanceMode(builder, AAUDIO_PERFORMANCE_MODE_LOW_LATENCY);
            AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_FLOAT);
            AAudioStreamBuilder_setSampleRate(builder, sample_rate as i32);
            AAudioStreamBuilder_setChannelCount(builder, channels as i32);
            // 100 ms 缓冲：暂停/切歌响应与抗欠载的折中（与 ALSA 一致）。
            let capacity = (sample_rate / 10).max(1024) as i32;
            AAudioStreamBuilder_setBufferCapacityInFrames(builder, capacity);
        }

        let mut stream: *mut AAudioStream = ptr::null_mut();
        let open_result = unsafe { AAudioStreamBuilder_openStream(builder, &mut stream) };
        unsafe { AAudioStreamBuilder_delete(builder) };
        check(open_result, "AAudioStreamBuilder_openStream")?;
        if stream.is_null() {
            return Err(CoreError::Other("AAudio stream is null".into()));
        }

        let actual_rate = unsafe { AAudioStream_getSampleRate(stream) };
        let actual_channels = unsafe { AAudioStream_getChannelCount(stream) };
        if actual_channels as usize != channels {
            unsafe { AAudioStream_close(stream) };
            return Err(CoreError::Other(format!(
                "AAudio channel mismatch: requested {channels}, got {actual_channels}"
            )));
        }
        if actual_rate as u32 != sample_rate {
            // AudioFlinger 通常会把流重采样到硬件率；这里只记录，不改播放速度。
            tracing::warn!(
                requested = sample_rate,
                actual = actual_rate,
                "AAudio picked a different sample rate"
            );
        }

        check(
            unsafe { AAudioStream_requestStart(stream) },
            "AAudioStream_requestStart",
        )?;

        let state = unsafe { AAudioStream_getState(stream) };
        tracing::info!(
            device = %device,
            sample_rate = actual_rate,
            channels = actual_channels,
            state,
            "AAudio output stream opened"
        );

        Ok(Self {
            stream,
            device: format!("aaudio ({device})"),
            channels,
        })
    }

    fn wait_state(&self, target: i32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let state = unsafe { AAudioStream_getState(self.stream) };
            if state == target {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let mut next = 0i32;
            let _ = unsafe {
                AAudioStream_waitForStateChange(
                    self.stream,
                    state,
                    &mut next,
                    remaining.as_nanos().min(i64::MAX as u128) as i64,
                )
            };
        }
    }
}

impl AudioSink for AAudioSink {
    fn write(&mut self, interleaved: &[f32]) -> CoreResult<usize> {
        let channels = self.channels;
        let samples = interleaved.len();
        if samples == 0 {
            return Ok(0);
        }
        let total_frames = samples / channels;
        if total_frames == 0 {
            return Ok(0);
        }

        let mut written_frames = 0usize;
        while written_frames < total_frames {
            let offset = written_frames * channels;
            let frames = (total_frames - written_frames) as i32;
            let rc = unsafe {
                AAudioStream_write(
                    self.stream,
                    interleaved[offset..].as_ptr() as *const c_void,
                    frames,
                    WRITE_TIMEOUT_NS,
                )
            };
            if rc > 0 {
                written_frames += rc as usize;
                continue;
            }
            match rc {
                // 缓冲暂时满：继续等下一次可写。
                AAUDIO_ERROR_TIMEOUT | AAUDIO_ERROR_WOULD_BLOCK => continue,
                AAUDIO_ERROR_DISCONNECTED => {
                    return Err(CoreError::Other(
                        "AAudio stream disconnected (audio device changed)".into(),
                    ));
                }
                AAUDIO_ERROR_INVALID_STATE => {
                    // 暂停/停止期间不应写；返回 0 让播放线程重试而不是判定失败。
                    return Ok(written_frames * channels);
                }
                other => {
                    return Err(CoreError::Other(format!(
                        "AAudioStream_write failed: {} ({other})",
                        result_name(other)
                    )));
                }
            }
        }
        Ok(written_frames * channels)
    }

    fn flush(&mut self) -> CoreResult<()> {
        // 暂停 → 丢弃缓冲 → 重新启动；调用方随后会继续写。
        let _ = unsafe { AAudioStream_requestPause(self.stream) };
        let _ = self.wait_state(AAUDIO_STREAM_STATE_PAUSED, Duration::from_millis(100));
        let _ = unsafe { AAudioStream_requestFlush(self.stream) };
        let _ = self.wait_state(AAUDIO_STREAM_STATE_FLUSHED, Duration::from_millis(100));
        let _ = unsafe { AAudioStream_requestStart(self.stream) };
        Ok(())
    }

    fn drain(&mut self) -> CoreResult<()> {
        // 等硬件把已写帧读完（曲末），最多 5 秒，避免拖住播放线程。
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let written = unsafe { AAudioStream_getFramesWritten(self.stream) };
            let read = unsafe { AAudioStream_getFramesRead(self.stream) };
            if read >= written || Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn device_name(&self) -> &str {
        &self.device
    }

    fn delay_frames(&self) -> u64 {
        let written = unsafe { AAudioStream_getFramesWritten(self.stream) };
        let read = unsafe { AAudioStream_getFramesRead(self.stream) };
        written.saturating_sub(read).max(0) as u64
    }
}

impl Drop for AAudioSink {
    fn drop(&mut self) {
        if !self.stream.is_null() {
            unsafe {
                let _ = AAudioStream_requestStop(self.stream);
                let _ = AAudioStream_close(self.stream);
            }
            self.stream = ptr::null_mut();
        }
    }
}
