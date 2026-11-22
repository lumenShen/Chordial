//! Chordial 2.0 播放引擎。
//!
//! - [`backend`]：`AudioSink` 契约 + dlopen ALSA 后端 + 无头后端；
//! - [`decode`]：symphonia → f32 交错样本；
//! - [`http_stream`]：HTTP 渐进式流适配为可 seek 的 `MediaSource`；
//! - [`analysis`]：可配置频段的能量分析（背景鼓点跳动）；
//! - [`equalizer`]：参数均衡器（RBJ 双二阶滤波器链，UI 可绘制同一条频响）；
//! - [`player`]：单线程状态机（命令 mpsc / 状态快照 / 事件轮询）。

pub mod analysis;
pub mod backend;
pub mod decode;
pub mod equalizer;
pub mod http_stream;
pub mod player;

pub use analysis::{BandAnalyzer, DEFAULT_BEAT_BAND, normalize_band};
pub use backend::{AudioSink, NullSink, RealtimeNullSink, open_sink};
pub use equalizer::{
    EQ_MAX_BANDS, EQ_MAX_GAIN_DB, EQ_MAX_HZ, EQ_MAX_Q, EQ_MIN_GAIN_DB, EQ_MIN_HZ, EQ_MIN_Q,
    EQ_REFERENCE_RATE, EqBand, EqSettings, Equalizer, FilterKind, GRAPHIC_10_BANDS, fmt_db,
    fmt_freq, freq_to_norm, norm_to_freq,
};
pub use player::{Command, Event, PlayRequest, Player, PlayerState, Status};
