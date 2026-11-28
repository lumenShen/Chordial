//! 播放引擎状态机。
//!
//! 线程模型：**单播放线程**独占解码器与 AudioSink；命令走 std mpsc，
//! 状态走 `RwLock<Status>` 快照；事件（曲目结束/错误）走 mpsc 供 UI 轮询。
//! UI 线程只做 `try_recv` 与读快照，永不阻塞。
//!
//! 播放循环逐 packet 解码（O(音频帧数)），命令延迟 ≈ 一个 packet（~20ms）。
//! 容器 seek 失败时退化为"重开流 + 跳帧"，跳帧丢弃 O(目标时长) 解码但无
//! 二次乘性开销。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use chordal_core::AudioBands;
use chordal_core::error::CoreResult;
use chordal_core::model::StreamHandle;
use chordal_core::perf::PerfScope;
use chordal_core::{AudioBandKind, AudioBandRanges, FrequencyRange};

use crate::analysis::BandAnalyzer;
use crate::backend::{AudioSink, open_sink};
use crate::decode::AudioDecoder;
use crate::equalizer::{EqSettings, Equalizer};
use crate::http_stream::HttpMediaSource;

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerState {
    Stopped,
    Loading,
    Playing,
    Paused,
    Failed(String),
}

/// UI 可见的播放器快照。
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub generation: u64,
    pub state: PlayerState,
    pub title: Option<String>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            generation: 0,
            state: PlayerState::Stopped,
            title: None,
            position_ms: 0,
            duration_ms: None,
            volume: 0.9,
        }
    }
}

pub struct PlayRequest {
    pub handle: StreamHandle,
    pub title: String,
    pub start_ms: u64,
}

pub enum Command {
    Play(PlayRequest),
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    SeekMs(u64),
}

#[derive(Debug, Clone)]
pub enum Event {
    /// 当前曲目自然播完（generation 供 UI 过滤旧事件）。
    TrackEnded(u64),
    StreamError(String),
    SinkError(String),
}

type SinkFactory = Box<dyn Fn(u32, usize) -> CoreResult<Box<dyn AudioSink>> + Send>;

pub struct Player {
    cmd_tx: Sender<Command>,
    status: Arc<RwLock<Status>>,
    events: Mutex<Receiver<Event>>,
    backend_name: &'static str,
    /// 五频段反应快照。播放线程整份写入，UI 线程整份读取。
    audio_bands: Arc<RwLock<AudioBands>>,
    /// 五路可编辑监听频段；播放线程按写块读取一次。
    audio_band_ranges: Arc<RwLock<AudioBandRanges>>,
    /// 参数均衡器配置（UI 写入，播放线程按版本号热加载）。
    eq: Arc<RwLock<EqSettings>>,
    eq_version: Arc<AtomicU64>,
}

impl Player {
    /// 默认构造：平台原生后端（Windows WASAPI / Linux ALSA）；打开失败 →
    /// Failed 状态 + SinkError 事件（诚实报错，不静默无声）。
    pub fn new(device: Option<String>) -> Arc<Self> {
        let backend = if cfg!(windows) {
            "wasapi"
        } else if cfg!(target_os = "linux") {
            "alsa"
        } else if cfg!(target_os = "android") {
            "aaudio"
        } else {
            "null"
        };
        Self::with_sink_factory(
            Box::new(move |rate, channels| open_sink(device.as_deref(), rate, channels)),
            backend,
        )
    }

    /// 注入输出后端工厂（测试注入 NullSink / 未来接入其它输出）。
    pub fn with_sink_factory(factory: SinkFactory, backend_name: &'static str) -> Arc<Self> {
        let (cmd_tx, cmd_rx) = channel();
        let (event_tx, event_rx) = channel();
        let status = Arc::new(RwLock::new(Status::default()));
        let audio_bands = Arc::new(RwLock::new(AudioBands::default()));
        let audio_band_ranges = Arc::new(RwLock::new(AudioBandRanges::default()));
        let eq = Arc::new(RwLock::new(EqSettings::default()));
        let eq_version = Arc::new(AtomicU64::new(0));
        let thread_status = status.clone();
        let thread_audio_bands = audio_bands.clone();
        let thread_audio_band_ranges = audio_band_ranges.clone();
        let thread_eq = eq.clone();
        let thread_eq_version = eq_version.clone();
        std::thread::Builder::new()
            .name("chordial-playback".into())
            .spawn(move || {
                PlaybackThread {
                    rx: cmd_rx,
                    status: thread_status,
                    event_tx,
                    sink_factory: factory,
                    volume: 0.9,
                    generation: 0,
                    audio_bands: thread_audio_bands,
                    audio_band_ranges: thread_audio_band_ranges,
                    eq: thread_eq,
                    eq_version: thread_eq_version,
                }
                .run();
            })
            .expect("spawn playback thread");
        Arc::new(Self {
            cmd_tx,
            status,
            events: Mutex::new(event_rx),
            backend_name,
            audio_bands,
            audio_band_ranges,
            eq,
            eq_version,
        })
    }

    pub fn play(&self, handle: StreamHandle, title: impl Into<String>, start_ms: u64) {
        self.send(Command::Play(PlayRequest {
            handle,
            title: title.into(),
            start_ms,
        }));
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    pub fn resume(&self) {
        self.send(Command::Resume);
    }

    pub fn toggle(&self) {
        match self.status().state {
            PlayerState::Playing => self.pause(),
            PlayerState::Paused => self.resume(),
            _ => {}
        }
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        if let Ok(mut status) = self.status.write() {
            status.volume = volume;
        }
        self.send(Command::SetVolume(volume));
    }

    pub fn seek_ms(&self, ms: u64) {
        if let Ok(mut status) = self.status.write() {
            status.position_ms = ms;
        }
        self.send(Command::SeekMs(ms));
    }

    /// 一次更新五路监听频段；非法区间按各通道默认值收敛。下一写块即生效。
    pub fn set_audio_band_ranges(&self, ranges: AudioBandRanges) {
        if let Ok(mut slot) = self.audio_band_ranges.write() {
            *slot = ranges.sanitized();
        }
    }

    /// 当前五路监听频段。
    pub fn audio_band_ranges(&self) -> AudioBandRanges {
        self.audio_band_ranges
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .sanitized()
    }

    /// 兼容旧 API：只更新低频鼓点通道，其余四路保持当前值。
    pub fn set_beat_band(&self, low_hz: f32, high_hz: f32) {
        if let Ok(mut ranges) = self.audio_band_ranges.write() {
            ranges.set(AudioBandKind::Kick, FrequencyRange { low_hz, high_hz });
        }
    }

    /// 兼容旧 API：返回低频鼓点通道。
    pub fn beat_band(&self) -> (f32, f32) {
        let range = self.audio_band_ranges().get(AudioBandKind::Kick);
        (range.low_hz, range.high_hz)
    }

    /// 当前五频段反应快照。一次读锁读取整份数据，五路共享同一分析时刻。
    pub fn audio_bands(&self) -> AudioBands {
        self.audio_bands
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .sanitized()
    }

    /// 当前鼓点包络（0..1）。兼容旧 API，等价于 [`Self::audio_bands`] 的 `kick`。
    pub fn beat_level(&self) -> f32 {
        self.audio_bands().kick
    }

    /// 热更新参数均衡器。播放线程在下一次写块前按版本号载入，
    /// 参数变化不会中断当前曲目或重建音频设备。
    pub fn set_eq(&self, settings: EqSettings) -> u64 {
        if let Ok(mut eq) = self.eq.write() {
            *eq = settings.sanitized();
        }
        self.eq_version.fetch_add(1, Ordering::Release) + 1
    }

    pub fn eq_settings(&self) -> EqSettings {
        self.eq
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn status(&self) -> Status {
        self.status
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 轻量时钟快照：歌词渲染每帧读取，避免 clone `Status` 里的标题字符串。
    pub fn clock(&self) -> (u64, bool) {
        let status = self
            .status
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            status.position_ms,
            matches!(status.state, PlayerState::Playing),
        )
    }

    /// UI 轮询：弹出事件（曲目结束 → 自动下一首）。
    pub fn try_pop_event(&self) -> Option<Event> {
        self.events
            .lock()
            .expect("event lock poisoned")
            .try_recv()
            .ok()
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend_name
    }

    fn send(&self, cmd: Command) {
        let _ = self.cmd_tx.send(cmd);
    }
}

struct PlaybackThread {
    rx: Receiver<Command>,
    status: Arc<RwLock<Status>>,
    event_tx: Sender<Event>,
    sink_factory: SinkFactory,
    volume: f32,
    generation: u64,
    audio_bands: Arc<RwLock<AudioBands>>,
    audio_band_ranges: Arc<RwLock<AudioBandRanges>>,
    eq: Arc<RwLock<EqSettings>>,
    eq_version: Arc<AtomicU64>,
}

impl PlaybackThread {
    fn publish_audio_bands(&self, bands: AudioBands) {
        if let Ok(mut slot) = self.audio_bands.write() {
            *slot = bands.sanitized();
        }
    }

    fn run(mut self) {
        loop {
            let Ok(cmd) = self.rx.recv() else { return };
            match cmd {
                Command::Play(mut req) => {
                    // 用迭代而不是递归续播：`play_item` 在遇到新的 Play 命令时
                    // 返回 `Some(next)`，当前帧随之结束，decoder 与它持有的
                    // HttpMediaSource（整首已下载字节）立刻释放。旧实现递归
                    // 调用会把每个历史曲目的完整缓冲都压在栈帧里，快速切歌
                    // 时内存按“曲目数 × 文件大小”增长，直到最新一首结束。
                    loop {
                        let generation = self.generation + 1;
                        self.generation = generation;
                        match self.play_item(req, generation) {
                            Some(next) => req = next,
                            None => break,
                        }
                    }
                }
                Command::SetVolume(v) => {
                    self.volume = v;
                    self.patch_status(|s| s.volume = v);
                }
                Command::Stop => self.patch_status(|s| {
                    s.state = PlayerState::Stopped;
                    s.position_ms = 0;
                }),
                _ => {}
            }
        }
    }

    fn patch_status(&self, f: impl FnOnce(&mut Status)) {
        let mut status = self.status.write().expect("status lock poisoned");
        f(&mut status);
    }

    fn open_decoder(handle: &StreamHandle) -> CoreResult<AudioDecoder> {
        match handle {
            StreamHandle::File(path) => AudioDecoder::open_file(path),
            StreamHandle::Url(url) => {
                // 从 URL 猜扩展名（.mp3/.flac/...，query 前截断）。
                let path = url.split(['?', ';']).next().unwrap_or(url);
                let name = path.rsplit('/').next().unwrap_or(path);
                let ext = name
                    .rsplit_once('.')
                    .map(|(_, ext)| ext.to_ascii_lowercase())
                    .unwrap_or_default();
                let source = HttpMediaSource::open(url)?;
                AudioDecoder::open_media(Box::new(source), &ext)
            }
        }
    }

    /// 播放一首曲目。正常结束/停止/失败返回 `None`；播放中收到新的 Play
    /// 命令时返回 `Some(next)`，由 [`Self::run`] 迭代续播（不递归，避免旧
    /// 曲目的 decoder/HTTP 缓冲滞留在栈帧里）。
    fn play_item(&mut self, req: PlayRequest, generation: u64) -> Option<PlayRequest> {
        self.patch_status(|s| {
            s.generation = generation;
            s.state = PlayerState::Loading;
            s.title = Some(req.title.clone());
            s.position_ms = req.start_ms;
        });

        let warmup = PerfScope::enter("player.decode.warmup");
        let mut decoder = match Self::open_decoder(&req.handle) {
            Ok(d) => d,
            Err(e) => {
                self.patch_status(|s| s.state = PlayerState::Failed(e.to_string()));
                let _ = self.event_tx.send(Event::StreamError(e.to_string()));
                return None;
            }
        };
        let duration_ms = decoder.duration_ms();

        // 先解出第一个音频包：容器头可能缺声道数（如 MP4/AAC），或实际
        // 输出采样率与头部声明不一致（如 HE-AAC）。输出设备必须按码流的
        // **真实**规格打开，否则设备会按错误速率解释样本——听感就是变调
        // 慢放（44.1k 兜底值遇 48k/96k/单声道素材都会触发）。
        let mut pending: Vec<f32> = Vec::with_capacity(16 * 1024);
        loop {
            match decoder.next_block(&mut pending) {
                Ok(true) if pending.is_empty() => continue,
                Ok(true) => break,
                Ok(false) => break, // 空流：照常打开设备后立即收尾
                Err(e) => {
                    self.patch_status(|s| s.state = PlayerState::Failed(e.to_string()));
                    let _ = self.event_tx.send(Event::StreamError(e.to_string()));
                    return None;
                }
            }
        }
        let rate = decoder.sample_rate();
        let channels = decoder.channels();

        let mut sink = match (self.sink_factory)(rate, channels) {
            Ok(sink) => sink,
            Err(e) => {
                self.patch_status(|s| s.state = PlayerState::Failed(e.to_string()));
                let _ = self.event_tx.send(Event::SinkError(e.to_string()));
                return None;
            }
        };
        warmup.finish(0);
        tracing::info!(
            target: "chordial::player",
            device = sink.device_name(),
            rate,
            channels,
            "playback sink opened"
        );

        self.patch_status(|s| {
            s.state = PlayerState::Playing;
            s.duration_ms = duration_ms;
        });

        let mut position_frames: u64 = req.start_ms * rate as u64 / 1000;
        let mut scratch: Vec<f32> = Vec::new();
        if req.start_ms > 0 {
            // 探测规格用的首包不属于目标起点；容器 seek 失败退化为跳帧时，
            // 这段已消费的帧数要从跳过量里扣掉。
            let consumed_frames = (pending.len() / channels) as u64;
            pending.clear();
            seek_to(
                &mut decoder,
                req.start_ms,
                rate,
                consumed_frames,
                &mut scratch,
            );
        }

        // 鼓点分析器随曲目采样率重建；缓冲区一次性分配，热路径零分配。
        let mut analyzer = BandAnalyzer::new(rate);
        // 均衡器同样是每曲一个实例（滤波器状态与采样率绑定）。
        let mut eq = Equalizer::new(rate, channels);
        let mut eq_version_seen = 0u64;
        // 输出设备有缓冲（ALSA 默认 200ms）：分析发生在“写入”时，而画面
        // 应该跟“可听”的鼓点同步。按绝对帧位置排队，等播放头追到中点
        // 才发布包络。
        let mut beat_queue: VecDeque<(u64, AudioBands)> = VecDeque::new();
        self.publish_audio_bands(AudioBands::default());

        let mut last_status_update = Instant::now();
        let mut seek_req: Option<u64> = None;
        let mut paused = false;

        'play: loop {
            // ---- 命令泵（非阻塞）----
            loop {
                match self.rx.try_recv() {
                    Ok(Command::Pause) => {
                        paused = true;
                        let _ = sink.flush();
                        analyzer.reset();
                        beat_queue.clear();
                        self.publish_audio_bands(AudioBands::default());
                        self.patch_status(|s| s.state = PlayerState::Paused);
                    }
                    Ok(Command::Resume) => {
                        paused = false;
                        self.patch_status(|s| s.state = PlayerState::Playing);
                    }
                    Ok(Command::Stop) => {
                        let _ = sink.flush();
                        beat_queue.clear();
                        self.publish_audio_bands(AudioBands::default());
                        self.patch_status(|s| {
                            s.state = PlayerState::Stopped;
                            s.position_ms = 0;
                        });
                        return None; // 不发 TrackEnded：用户主动停止
                    }
                    Ok(Command::Play(new_req)) => {
                        let _ = sink.flush();
                        beat_queue.clear();
                        self.publish_audio_bands(AudioBands::default());
                        drop(sink); // 旧输出句柄立即释放
                        // 交回外层迭代续播：本帧返回后 decoder/流缓冲即释放。
                        return Some(new_req);
                    }
                    Ok(Command::SeekMs(ms)) => seek_req = Some(ms),
                    Ok(Command::SetVolume(v)) => {
                        self.volume = v;
                        self.patch_status(|s| s.volume = v);
                    }
                    Err(_) => break,
                }
            }

            // ---- 暂停：阻塞等命令，零 CPU 空转 ----
            if paused {
                'wait: loop {
                    match self.rx.recv() {
                        Ok(Command::Resume) => {
                            paused = false;
                            self.patch_status(|s| s.state = PlayerState::Playing);
                            break 'wait;
                        }
                        Ok(Command::Pause) => continue 'wait,
                        Ok(Command::SeekMs(ms)) => {
                            // 暂停时也必须立即跳转并刷新状态，否则点歌词/进度条无反馈。
                            let _ = sink.flush();
                            if seek_to(&mut decoder, ms, rate, 0, &mut scratch) {
                                position_frames = ms * rate as u64 / 1000;
                                analyzer.reset();
                                eq.reset();
                                beat_queue.clear();
                                self.publish_audio_bands(AudioBands::default());
                                self.patch_status(|s| s.position_ms = ms);
                            }
                        }
                        Ok(Command::SetVolume(v)) => {
                            self.volume = v;
                            self.patch_status(|s| s.volume = v);
                        }
                        Ok(Command::Stop) => {
                            beat_queue.clear();
                            self.publish_audio_bands(AudioBands::default());
                            self.patch_status(|s| {
                                s.state = PlayerState::Stopped;
                                s.position_ms = 0;
                            });
                            return None;
                        }
                        Ok(Command::Play(new_req)) => {
                            let _ = sink.flush();
                            beat_queue.clear();
                            self.publish_audio_bands(AudioBands::default());
                            drop(sink);
                            return Some(new_req);
                        }
                        Err(_) => return None,
                    }
                }
                continue 'play;
            }

            // ---- seek ----
            if let Some(ms) = seek_req.take() {
                let seek_scope = PerfScope::enter("player.seek");
                if seek_to(&mut decoder, ms, rate, 0, &mut scratch) {
                    position_frames = ms * rate as u64 / 1000;
                    analyzer.reset();
                    eq.reset();
                    beat_queue.clear();
                    self.publish_audio_bands(AudioBands::default());
                    self.patch_status(|s| s.position_ms = ms);
                }
                seek_scope.finish(0);
            }

            // ---- 解码填缓冲（保持待写样本量有界）----
            if pending.len() < channels * 2048 {
                match decoder.next_block(&mut pending) {
                    Ok(true) => {}
                    Ok(false) => break 'play, // EOF
                    Err(e) => {
                        self.patch_status(|s| s.state = PlayerState::Failed(e.to_string()));
                        let _ = self.event_tx.send(Event::StreamError(e.to_string()));
                        return None;
                    }
                }
            }
            if pending.is_empty() {
                continue 'play;
            }

            // ---- 按整帧切块写入；EQ → 音量，均为 O(n) 内联 ----
            // 约 21ms（48kHz）的写块足以覆盖多次 10ms 分析，又不会把碎鼓点
            // 和镲片合并到同一个视觉快照里。
            let take_frames = (pending.len() / channels).min(1024);
            let take_samples = take_frames * channels;
            let chunk = &mut pending[..take_samples];
            let version = self.eq_version.load(Ordering::Acquire);
            if version != eq_version_seen {
                eq_version_seen = version;
                let settings = self
                    .eq
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone();
                eq.load(settings, version);
            }
            eq.process(chunk);
            if self.volume < 0.999 {
                for s in chunk.iter_mut() {
                    *s *= self.volume;
                }
            }
            if let Err(e) = sink.write(chunk) {
                self.patch_status(|s| s.state = PlayerState::Failed(e.to_string()));
                let _ = self.event_tx.send(Event::SinkError(e.to_string()));
                return None;
            }
            // 与输出同一批样本（已乘音量）做频段分析；保留写块内峰值，避免
            // 最后一次分析覆盖掉更早出现的碎片鼓点/镲片。
            let ranges = self
                .audio_band_ranges
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .sanitized();
            let bands = analyzer.push_interleaved(chunk, channels, ranges);
            let written_end = position_frames + take_frames as u64;
            // 播放头到达这批样本中点时，视觉峰值对应“此刻听到”的声音。
            beat_queue.push_back((written_end - take_frames as u64 / 2, bands));
            if beat_queue.len() > 256 {
                beat_queue.pop_front();
            }
            position_frames = written_end;
            pending.drain(..take_samples);

            // 音频反应包络按每个写块发布，不受歌词状态 33ms 节流影响；屏幕
            // 刷新率只决定能看到其中多少快照，不改变播放线程的检测节奏。
            let audible = position_frames.saturating_sub(sink.delay_frames());
            while let Some(&(due, bands)) = beat_queue.front() {
                if due > audible {
                    break;
                }
                beat_queue.pop_front();
                self.publish_audio_bands(bands);
            }

            // ---- 状态节流刷新（~33ms：歌词逐词高亮的刷新下限）----
            if last_status_update.elapsed() >= Duration::from_millis(33) {
                let pos_ms = audible * 1000 / rate as u64;
                self.patch_status(|s| s.position_ms = pos_ms);
                last_status_update = Instant::now();
            }
        }

        if let Err(e) = sink.drain() {
            self.patch_status(|s| s.state = PlayerState::Failed(e.to_string()));
            let _ = self.event_tx.send(Event::SinkError(e.to_string()));
            return None;
        }
        beat_queue.clear();
        self.publish_audio_bands(AudioBands::default());
        let audible = position_frames.saturating_sub(sink.delay_frames());
        let pos_ms = audible * 1000 / rate as u64;
        self.patch_status(|s| {
            s.position_ms = pos_ms;
            s.state = PlayerState::Stopped;
        });
        let _ = self.event_tx.send(Event::TrackEnded(generation));
        None
    }
}

/// 容器 seek；失败则重开流并跳帧解码到目标位置。
/// `already_consumed_frames`：调用方此前已从当前位置消费的帧数（如规格探测
/// 首包），跳帧时从目标里扣除；持续播放中的 seek 传 0。
/// `scratch` 复用调用方缓冲，跳帧阶段零额外分配。
fn seek_to(
    decoder: &mut AudioDecoder,
    target_ms: u64,
    rate: u32,
    already_consumed_frames: u64,
    scratch: &mut Vec<f32>,
) -> bool {
    if decoder.try_seek_ms(target_ms).unwrap_or(false) {
        return true;
    }
    tracing::debug!(target: "chordial::player", target_ms, "container seek failed, skipping frames");
    let target_frames =
        ((target_ms as f64 * rate as f64 / 1000.0) as u64).saturating_sub(already_consumed_frames);
    let channels = decoder.channels();
    let mut skipped = 0u64;
    while skipped < target_frames {
        scratch.clear();
        match decoder.next_block(scratch) {
            Ok(true) => skipped += (scratch.len() / channels) as u64,
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_failure_does_not_advance_the_queue() {
        let player = Player::with_sink_factory(
            Box::new(|_, _| Err(chordal_core::error::CoreError::Config("unused".into()))),
            "test",
        );
        player.play(
            StreamHandle::File(std::path::PathBuf::from("chordial-missing-test-file.wav")),
            "broken",
            0,
        );

        let deadline = Instant::now() + Duration::from_secs(1);
        while !matches!(player.status().state, PlayerState::Failed(_)) && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(player.status().state, PlayerState::Failed(_)));
        let event = loop {
            if let Some(event) = player.try_pop_event() {
                break event;
            }
            assert!(Instant::now() < deadline, "播放器未发送失败事件");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(matches!(event, Event::StreamError(_)));
        assert!(player.try_pop_event().is_none());
    }
}
