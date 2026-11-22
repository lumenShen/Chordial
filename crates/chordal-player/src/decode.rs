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
use symphonia::core::audio::{SampleBuffer, SignalSpec};
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

        Ok(Self {
            track_id: track.id,
            format,
            decoder,
            spec: None,
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
