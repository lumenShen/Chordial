//! symphonia 解码封装：任意 MediaSource（本地文件 / HTTP 流）→ f32 交错立体声。
//!
//! API 对齐 symphonia 0.5.4（已对照 docs.rs 核实）：
//! - `FormatReader::seek(SeekMode, SeekTo)`（无 try_seek）、`next_packet`；
//! - 时长/时基来自 `Track::codec_params`（trait 上没有 duration()/time_base()）；
//! - `SampleBuffer::new(frames: u64, spec: SignalSpec)`，首包后惰性构建。
//!
//! 复杂度：逐 packet 解码，输出 O(帧数)；声道数 ≠2 时做一次线性
//! 上/下混，无逐帧分支热点。

use std::fs::File;

use chordal_core::error::{CoreError, CoreResult};
use symphonia::core::audio::{Channels, SampleBuffer, SignalSpec};
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

pub struct AudioDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    /// 首个解码包后确定的信号规格。
    spec: Option<SignalSpec>,
    sample_buf: Option<SampleBuffer<f32>>,
    duration_ms: Option<u64>,
}

impl AudioDecoder {
    pub fn open_file(path: &std::path::Path) -> CoreResult<Self> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        Self::open(mss, &hint)
    }

    pub fn open_media(
        source: Box<dyn symphonia::core::io::MediaSource>,
        ext: &str,
    ) -> CoreResult<Self> {
        let mss = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        hint.with_extension(ext);
        Self::open(mss, &hint)
    }

    fn open(mss: MediaSourceStream, hint: &Hint) -> CoreResult<Self> {
        let probed = symphonia::default::get_probe()
            .format(
                hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| CoreError::Other(format!("probe failed: {e}")))?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| {
                t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some()
            })
            .ok_or_else(|| CoreError::Other("no audio track".into()))?
            .clone();

        let params = track.codec_params.clone();
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| CoreError::Other(format!("decoder init failed: {e}")))?;

        // 容器时长：n_frames × time_base（均可能缺省，缺了就显示未知时长）。
        let duration_ms = match (params.time_base, params.n_frames) {
            (Some(tb), Some(frames)) => {
                let t = tb.calc_time(frames);
                Some(((t.seconds as f64 + t.frac) * 1000.0) as u64)
            }
            _ => None,
        };

        // 容器头部（MP3/FLAC/WAV/M4A…）已经给出采样率与声道数，必须在
        // 首包解码**之前**就能读到：播放线程据此打开输出设备。此前这里
        // 留 None、对外返回硬编码 44.1kHz/2ch，导致 48kHz/96kHz/单声道
        // 素材被设备按错误速率解释（变速、变调）。
        let spec = params.sample_rate.map(|rate| {
            let channels = params
                .channels
                .filter(|c| !c.is_empty())
                .unwrap_or(Channels::FRONT_LEFT | Channels::FRONT_RIGHT);
            SignalSpec::new(rate, channels)
        });

        Ok(Self {
            track_id: track.id,
            format,
            decoder,
            spec,
            sample_buf: None,
            duration_ms,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.spec.map(|s| s.rate).unwrap_or(44_100)
    }

    pub fn channels(&self) -> usize {
        self.spec.map(|s| s.channels.count()).unwrap_or(2)
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    /// 下一段交错样本追加到 `out`（声道顺序保持源的规格，经统一为 f32）。
    /// 返回 `false` = 流结束。O(packet 样本数)。
    pub fn next_block(&mut self, out: &mut Vec<f32>) -> CoreResult<bool> {
        let packet = loop {
            match self.format.next_packet() {
                Ok(p) => break p,
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(false);
                }
                Err(SymphoniaError::ResetRequired) => return Ok(false),
                Err(e) => {
                    // 单个损坏 packet 跳过，不让整个流死掉。
                    tracing::debug!(target: "chordial::player", error = %e, "packet skipped");
                    continue;
                }
            }
        };
        if packet.track_id() != self.track_id {
            return Ok(true); // 非音频轨，跳过（流未结束）
        }
        let decoded = match self.decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_)) => return Ok(true),
            Err(e) => return Err(CoreError::Other(format!("decode failed: {e}"))),
        };

        // SampleBuffer 需要首包的 SignalSpec 与容量，惰性构建一次。
        if self.sample_buf.is_none() {
            let spec = *decoded.spec();
            if let Some(declared) = self.spec
                && (declared.rate != spec.rate || declared.channels != spec.channels)
            {
                // 容器头与实际码流不一致（罕见）：输出端已按头部规格打开，
                // 这里只提示，不改写已建立的播放管线。
                tracing::warn!(
                    target: "chordial::player",
                    declared_rate = declared.rate,
                    declared_channels = declared.channels.count(),
                    decoded_rate = spec.rate,
                    decoded_channels = spec.channels.count(),
                    "decoded spec differs from container headers"
                );
            }
            self.spec = Some(spec);
            self.sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        let sample_buf = self.sample_buf.as_mut().expect("sample buffer ready");
        sample_buf.copy_interleaved_ref(decoded);
        out.extend_from_slice(sample_buf.samples());
        Ok(true)
    }

    /// 尽力跳转（容器级 seek）；容器不支持时由调用方退化为
    /// "从头解码 + 跳帧"。成功后解码器状态复位。
    pub fn try_seek_ms(&mut self, ms: u64) -> CoreResult<bool> {
        let time = Time::from(ms as f64 / 1000.0);
        match self.format.seek(
            SeekMode::Coarse,
            SeekTo::Time {
                time,
                track_id: Some(self.track_id),
            },
        ) {
            Ok(_) => {
                self.decoder.reset();
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 最小 16bit PCM WAV：足够让 symphonia 从头部读出采样率/声道。
    fn write_wav(path: &std::path::Path, rate: u32, channels: u16, frames: u32) {
        let data_len = frames as usize * channels as usize * 2;
        let mut header = [0u8; 44];
        header[0..4].copy_from_slice(b"RIFF");
        header[4..8].copy_from_slice(&(36 + data_len as u32).to_le_bytes());
        header[8..12].copy_from_slice(b"WAVE");
        header[12..16].copy_from_slice(b"fmt ");
        header[16..20].copy_from_slice(&16u32.to_le_bytes());
        header[20..22].copy_from_slice(&1u16.to_le_bytes());
        header[22..24].copy_from_slice(&channels.to_le_bytes());
        header[24..28].copy_from_slice(&rate.to_le_bytes());
        header[28..32].copy_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        header[32..34].copy_from_slice(&(channels * 2).to_le_bytes());
        header[34..36].copy_from_slice(&16u16.to_le_bytes());
        header[36..40].copy_from_slice(b"data");
        header[40..44].copy_from_slice(&(data_len as u32).to_le_bytes());

        let mut f = File::create(path).unwrap();
        f.write_all(&header).unwrap();
        f.write_all(&vec![0u8; data_len]).unwrap();
        f.flush().unwrap();
    }

    #[test]
    fn spec_is_available_before_first_decode() {
        // 回归：播放线程在解首包之前就要用采样率/声道打开输出设备。
        // 若这里返回兜底 44.1kHz/2ch，48kHz/单声道素材会被设备按错误
        // 速率解释，表现为变调慢放。
        for (rate, channels) in [(48_000u32, 1u16), (44_100, 2), (96_000, 2)] {
            let path = std::env::temp_dir().join(format!(
                "chordial-decode-{}-{}-{}.wav",
                std::process::id(),
                rate,
                channels
            ));
            write_wav(&path, rate, channels, 480);
            let decoder = AudioDecoder::open_file(&path).unwrap();
            assert_eq!(decoder.sample_rate(), rate, "rate before decode");
            assert_eq!(
                decoder.channels(),
                channels as usize,
                "channels before decode"
            );
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn decoded_blocks_keep_declared_spec() {
        let path = std::env::temp_dir().join(format!(
            "chordial-decode-block-{}-{}.wav",
            std::process::id(),
            48_000
        ));
        write_wav(&path, 48_000, 1, 4_800);
        let mut decoder = AudioDecoder::open_file(&path).unwrap();
        let mut out = Vec::new();
        assert!(decoder.next_block(&mut out).unwrap());
        assert!(!out.is_empty());
        assert_eq!(out.len() % decoder.channels(), 0);
        assert_eq!(decoder.sample_rate(), 48_000);
        assert_eq!(decoder.channels(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
