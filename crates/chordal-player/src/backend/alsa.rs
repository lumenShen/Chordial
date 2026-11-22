//! Linux ALSA 输出后端（运行时 `dlopen`，零构建期依赖）。
//!
//! 环境现实：本机只有 `libasound.so.2` 运行时库，没有 alsa-lib 开发包，
//! `alsa-sys` 的 pkg-config 构建走不通。因此这里用 **运行时 dlopen**
//! （`libc::dlopen`）+ 手写最小 FFI；缺库时返回 Err，由调用方降级
//! [`super::NullSink`]。

use std::ffi::CString;

use chordal_core::error::{CoreError, CoreResult};

use super::AudioSink;

// snd_pcm_format_t / snd_pcm_access_t 的稳定 ABI 常量。
const SND_PCM_STREAM_PLAYBACK: i32 = 0;
const SND_PCM_FORMAT_FLOAT_LE: i32 = 14;
const SND_PCM_ACCESS_RW_INTERLEAVED: i32 = 3;
const EPIPE: i32 = 32; // Linux 通用值；dlopen 场景不引 errno crate
const ESTRPIPE: i32 = 86;

type PcmHandle = *mut libc::c_void;

unsafe extern "C" {
    fn dlopen(filename: *const libc::c_char, flag: i32) -> *mut libc::c_void;
    fn dlsym(handle: *mut libc::c_void, symbol: *const libc::c_char) -> *mut libc::c_void;
    fn dlclose(handle: *mut libc::c_void) -> i32;
}

const RTLD_NOW: i32 = 2;
const RTLD_LOCAL: i32 = 0;

macro_rules! sym {
    ($handle:expr, $name:literal, $ty:ty) => {{
        let c = CString::new($name).expect("static symbol name");
        let ptr = unsafe { dlsym($handle, c.as_ptr()) };
        if ptr.is_null() {
            return Err(CoreError::Other(format!(
                "libasound missing symbol {}",
                $name
            )));
        }
        unsafe { std::mem::transmute::<*mut libc::c_void, $ty>(ptr) }
    }};
}

type SndPcmOpenFn = unsafe extern "C" fn(*mut PcmHandle, *const libc::c_char, i32, i32) -> i32;
type SndPcmSetParamsFn = unsafe extern "C" fn(PcmHandle, i32, i32, u32, u32, i32, u32) -> i32;
type SndPcmWriteiFn =
    unsafe extern "C" fn(PcmHandle, *const libc::c_void, libc::c_ulong) -> libc::c_long;
type SndPcmSimpleFn = unsafe extern "C" fn(PcmHandle) -> i32;
type SndPcmDrainFn = unsafe extern "C" fn(PcmHandle) -> i32;
type SndPcmDelayFn = unsafe extern "C" fn(PcmHandle, *mut libc::c_long) -> i32;

struct AlsaSymbols {
    _lib: *mut libc::c_void,
    open: SndPcmOpenFn,
    set_params: SndPcmSetParamsFn,
    writei: SndPcmWriteiFn,
    prepare: SndPcmSimpleFn,
    recover: unsafe extern "C" fn(PcmHandle, i32, i32) -> i32,
    drop_buf: SndPcmSimpleFn,
    drain: SndPcmDrainFn,
    delay: SndPcmDelayFn,
    close: SndPcmSimpleFn,
}

// SAFETY: dlopen 句柄与其符号是进程级单例；AlsaSymbols 只在 open 时构造、
// Drop 时 dlclose，且 Sink 不跨线程共享（Send 由 AudioSink 契约约定）。
unsafe impl Send for AlsaSymbols {}

impl Drop for AlsaSymbols {
    fn drop(&mut self) {
        if !self._lib.is_null() {
            unsafe { dlclose(self._lib) };
        }
    }
}

fn load_symbols() -> CoreResult<AlsaSymbols> {
    let lib_name = CString::new("libasound.so.2").expect("static");
    let lib = unsafe { dlopen(lib_name.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
    if lib.is_null() {
        return Err(CoreError::Other(
            "libasound.so.2 not found (install alsa-lib runtime)".into(),
        ));
    }
    Ok(AlsaSymbols {
        open: sym!(lib, "snd_pcm_open", SndPcmOpenFn),
        set_params: sym!(lib, "snd_pcm_set_params", SndPcmSetParamsFn),
        writei: sym!(lib, "snd_pcm_writei", SndPcmWriteiFn),
        prepare: sym!(lib, "snd_pcm_prepare", SndPcmSimpleFn),
        recover: sym!(
            lib,
            "snd_pcm_recover",
            unsafe extern "C" fn(PcmHandle, i32, i32) -> i32
        ),
        drop_buf: sym!(lib, "snd_pcm_drop", SndPcmSimpleFn),
        drain: sym!(lib, "snd_pcm_drain", SndPcmDrainFn),
        delay: sym!(lib, "snd_pcm_delay", SndPcmDelayFn),
        close: sym!(lib, "snd_pcm_close", SndPcmSimpleFn),
        _lib: lib,
    })
}

pub struct AlsaSink {
    pcm: PcmHandle,
    syms: AlsaSymbols,
    device: String,
}

// SAFETY: snd_pcm_t 由独占线程使用；指针本身可移动（Send）。
unsafe impl Send for AlsaSink {}

impl AlsaSink {
    pub fn open(device: Option<&str>, sample_rate: u32, channels: usize) -> CoreResult<Self> {
        let syms = load_symbols()?;
        let device = device.unwrap_or("default").to_string();
        let c_device = CString::new(device.clone())
            .map_err(|_| CoreError::Other("invalid device name".into()))?;

        let mut pcm: PcmHandle = std::ptr::null_mut();
        let rc = unsafe { (syms.open)(&mut pcm, c_device.as_ptr(), SND_PCM_STREAM_PLAYBACK, 0) };
        if rc != 0 || pcm.is_null() {
            return Err(CoreError::Other(format!(
                "snd_pcm_open({device}) failed: {rc}"
            )));
        }
        // latency = 200ms：暂停/切歌响应性与抗抖动的折中。
        let rc = unsafe {
            (syms.set_params)(
                pcm,
                SND_PCM_FORMAT_FLOAT_LE,
                SND_PCM_ACCESS_RW_INTERLEAVED,
                channels as u32,
                sample_rate,
                1, // 允许软重采样
                200_000,
            )
        };
        if rc != 0 {
            unsafe { (syms.close)(pcm) };
            return Err(CoreError::Other(format!(
                "snd_pcm_set_params failed: {rc} (rate={sample_rate}, ch={channels})"
            )));
        }
        Ok(Self { pcm, syms, device })
    }
}

impl AudioSink for AlsaSink {
    fn write(&mut self, interleaved: &[f32]) -> CoreResult<usize> {
        let samples = interleaved.len();
        if samples == 0 {
            return Ok(0);
        }
        let mut remaining = samples;
        let mut offset = 0usize;
        while remaining > 0 {
            let rc = unsafe {
                (self.syms.writei)(
                    self.pcm,
                    interleaved[offset..].as_ptr() as *const libc::c_void,
                    remaining as libc::c_ulong,
                )
            };
            if rc >= 0 {
                let written = rc as usize;
                offset += written;
                remaining -= written;
                continue;
            }
            let err = rc as i32;
            if err == -EPIPE || err == -ESTRPIPE {
                // 欠载/流恢复：prepare 后重试。O(1) 系统调用。
                let r = unsafe { (self.syms.recover)(self.pcm, err, 1) };
                if r != 0 {
                    let r2 = unsafe { (self.syms.prepare)(self.pcm) };
                    if r2 != 0 {
                        return Err(CoreError::Other(format!("alsa recover failed: {r2}")));
                    }
                }
                continue;
            }
            return Err(CoreError::Other(format!("snd_pcm_writei failed: {err}")));
        }
        Ok(samples)
    }

    fn flush(&mut self) -> CoreResult<()> {
        let rc = unsafe { (self.syms.drop_buf)(self.pcm) };
        if rc != 0 {
            return Err(CoreError::Other(format!("snd_pcm_drop failed: {rc}")));
        }
        let rc = unsafe { (self.syms.prepare)(self.pcm) };
        if rc != 0 {
            return Err(CoreError::Other(format!("snd_pcm_prepare failed: {rc}")));
        }
        Ok(())
    }

    fn drain(&mut self) -> CoreResult<()> {
        let rc = unsafe { (self.syms.drain)(self.pcm) };
        if rc != 0 {
            return Err(CoreError::Other(format!("snd_pcm_drain failed: {rc}")));
        }
        Ok(())
    }

    fn device_name(&self) -> &str {
        &self.device
    }

    fn delay_frames(&self) -> u64 {
        let mut delay: libc::c_long = 0;
        let rc = unsafe { (self.syms.delay)(self.pcm, &mut delay) };
        if rc == 0 && delay > 0 {
            delay as u64
        } else {
            0
        }
    }
}

impl Drop for AlsaSink {
    fn drop(&mut self) {
        if !self.pcm.is_null() {
            unsafe { (self.syms.close)(self.pcm) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alsa_load_gracefully_fails_without_lib() {
        // 不允许 install libasound 的环境下返回 Err 而非 panic。
        let result = AlsaSink::open(None, 44_100, 2);
        if let Ok(sink) = result {
            // 本机有 libasound：打开成功也算通过（不写数据）。
            drop(sink);
        }
    }
}
