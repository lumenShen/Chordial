//! Windows WASAPI 输出后端（共享模式）。
//!
//! 通过系统默认渲染端点（或指定端点 ID）以 **共享模式** 打开音频流：
//! - 流格式为 f32 交错（`WAVE_FORMAT_EXTENSIBLE` + `IEEE_FLOAT`），
//!   由 `AUTOCONVERTPCM | SRC_DEFAULT_QUALITY` 让系统负责重采样/混音；
//! - 200ms 缓冲（与 ALSA 后端一致），`write` 以缓冲余量做背压；
//! - `delay_frames` 返回 `GetCurrentPadding`，供播放时钟使用。
//!
//! 打开失败（无音频设备/被独占）返回 Err，由调用方降级 [`super::NullSink`]。

use std::time::Duration;

use chordal_core::error::{CoreError, CoreResult};
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0, eConsole,
    eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::{GUID, HSTRING};

use super::AudioSink;

/// WAVE_FORMAT_EXTENSIBLE
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {00000003-0000-0010-8000-00aa00389b71}
const SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x0000_0003_0000_0010_8000_00aa00389b71);
/// 200ms 缓冲，单位 100ns。
const BUFFER_DURATION_HNS: i64 = 2_000_000;

pub struct WasapiSink {
    client: IAudioClient,
    render: IAudioRenderClient,
    buffer_frames: u32,
    channels: usize,
    device: String,
    owns_com: bool,
}

// SAFETY: WASAPI 接口在线程内创建、使用、销毁（播放线程独占），
// 与 AudioSink 的 Send 契约一致；指针移动不改变线程归属。
unsafe impl Send for WasapiSink {}

fn werr(context: &str, err: windows::core::Error) -> CoreError {
    CoreError::Other(format!("WASAPI {context}: {err}"))
}

impl WasapiSink {
    pub fn open(device: Option<&str>, sample_rate: u32, channels: usize) -> CoreResult<Self> {
        if channels == 0 {
            return Err(CoreError::Other("WASAPI: channels must be > 0".into()));
        }
        // SAFETY: 播放线程独占调用；RPC_E_CHANGED_MODE（已按其它套间初始化）可继续使用。
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let owns_com = hr.is_ok() && hr.0 == 0;
        if !hr.is_ok() {
            return Err(CoreError::Other(format!(
                "WASAPI CoInitializeEx failed: 0x{:08X}",
                hr.0 as u32
            )));
        }

        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|e| werr("CoCreateInstance(MMDeviceEnumerator)", e))?;

        let endpoint = match device {
            Some(id) if !id.is_empty() => {
                let wide = HSTRING::from(id);
                unsafe { enumerator.GetDevice(&wide) }.map_err(|e| werr("GetDevice", e))?
            }
            _ => unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
                .map_err(|e| werr("GetDefaultAudioEndpoint", e))?,
        };

        let client: IAudioClient = unsafe { endpoint.Activate(CLSCTX_ALL, None) }
            .map_err(|e| werr("Activate(IAudioClient)", e))?;

        // f32 交错流格式；系统负责重采样到端点混音格式。
        let wf = WAVEFORMATEXTENSIBLE {
            Format: WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_EXTENSIBLE,
                nChannels: channels as u16,
                nSamplesPerSec: sample_rate,
                nAvgBytesPerSec: sample_rate * channels as u32 * 4,
                nBlockAlign: (channels * 4) as u16,
                wBitsPerSample: 32,
                cbSize: std::mem::size_of::<WAVEFORMATEXTENSIBLE>() as u16
                    - std::mem::size_of::<WAVEFORMATEX>() as u16,
            },
            Samples: WAVEFORMATEXTENSIBLE_0 {
                wValidBitsPerSample: 32,
            },
            dwChannelMask: match channels {
                1 => 0x4, // SPEAKER_FRONT_CENTER
                2 => 0x3, // SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT
                _ => 0,
            },
            SubFormat: SUBTYPE_IEEE_FLOAT,
        };

        let init = unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                BUFFER_DURATION_HNS,
                0,
                &wf as *const WAVEFORMATEXTENSIBLE as *const WAVEFORMATEX,
                None,
            )
        };
        if let Err(e) = init {
            if owns_com {
                unsafe { CoUninitialize() };
            }
            return Err(werr(
                &format!("Initialize(rate={sample_rate}, ch={channels})"),
                e,
            ));
        }

        let buffer_frames =
            unsafe { client.GetBufferSize() }.map_err(|e| werr("GetBufferSize", e))?;
        let render: IAudioRenderClient = unsafe { client.GetService() }
            .map_err(|e| werr("GetService(IAudioRenderClient)", e))?;
        unsafe { client.Start() }.map_err(|e| werr("Start", e))?;

        Ok(Self {
            client,
            render,
            buffer_frames,
            channels,
            device: device
                .filter(|d| !d.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "wasapi-default".into()),
            owns_com,
        })
    }
}

impl AudioSink for WasapiSink {
    fn write(&mut self, interleaved: &[f32]) -> CoreResult<usize> {
        let samples = interleaved.len();
        if samples == 0 {
            return Ok(0);
        }
        let channels = self.channels;
        let total_frames = samples / channels;
        let mut done = 0usize;
        while done < total_frames {
            let padding = unsafe { self.client.GetCurrentPadding() }
                .map_err(|e| werr("GetCurrentPadding", e))?;
            let free = self.buffer_frames.saturating_sub(padding);
            if free == 0 {
                // 缓冲满：等待音频线程消费（背压 = 播放时钟）。
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            let n = free.min((total_frames - done) as u32);
            let ptr = unsafe { self.render.GetBuffer(n) }.map_err(|e| werr("GetBuffer", e))?;
            // SAFETY: GetBuffer 保证 n 帧（n × channels 个 f32）可写，且在上次
            // ReleaseBuffer 之后独占。
            unsafe {
                std::ptr::copy_nonoverlapping(
                    interleaved[done * channels..].as_ptr(),
                    ptr as *mut f32,
                    n as usize * channels,
                );
            }
            unsafe { self.render.ReleaseBuffer(n, 0) }.map_err(|e| werr("ReleaseBuffer", e))?;
            done += n as usize;
        }
        Ok(samples)
    }

    fn flush(&mut self) -> CoreResult<()> {
        // Stop + Start 丢弃已入队音频（暂停/切歌响应）。
        unsafe { self.client.Stop() }.map_err(|e| werr("Stop", e))?;
        unsafe { self.client.Start() }.map_err(|e| werr("Start", e))?;
        Ok(())
    }

    fn drain(&mut self) -> CoreResult<()> {
        // 等到端点缓冲排空（曲末自然结束）。
        loop {
            let padding = unsafe { self.client.GetCurrentPadding() }
                .map_err(|e| werr("GetCurrentPadding", e))?;
            if padding == 0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn device_name(&self) -> &str {
        &self.device
    }

    fn delay_frames(&self) -> u64 {
        unsafe { self.client.GetCurrentPadding() }
            .map(u64::from)
            .unwrap_or(0)
    }
}

impl Drop for WasapiSink {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
        if self.owns_com {
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasapi_open_or_fail_gracefully() {
        // 无音频设备的环境返回 Err 而非 panic；有声卡时打开成功即可。
        if let Ok(mut sink) = WasapiSink::open(None, 44_100, 2) {
            assert_eq!(sink.write(&[]).unwrap(), 0);
            drop(sink);
        }
    }
}
