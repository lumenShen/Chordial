#![allow(clippy::type_complexity)]

//! Chordial 2.0 领域核心。
//!
//! 该 crate 定义"音乐源适配器"的契约（[`source::MusicSource`]）、领域模型
//! （[`model`]）、多源注册/加载/卸载的 [`registry::SourceRegistry`]，以及贯穿
//! 全仓库的性能检查点系统（[`perf`]）。
//!
//! 核心不变量：
//! - 所有跨源聚合路径（合并、去重、排序）都必须是 O(n) 单遍扫描，
//!   由 `tests/perf_scaling.rs` 的复杂度护栏测试保证。
//! - 所有热点路径埋有性能检查点（[`perf::perf_scope`]），超过预算会在
//!   tracing 中告警并记入 [`perf::CheckpointLedger`]，供测试阶段断言。

pub mod audio;
pub mod error;
pub mod fs;
pub mod mem;
pub mod model;
pub mod perf;
pub mod playlist;
pub mod registry;
pub mod runtime;
pub mod source;

pub use audio::{
    AUDIO_BAND_COUNT, AudioBandKind, AudioBandRanges, AudioBands, AudioControlMapping,
    CONTROL_GRID_SIZE, CONTROL_POINT_COUNT, ControlPointParseError, ControlPointSet,
    FrequencyRange, MAX_AUDIO_HZ, MIN_AUDIO_HZ, is_interior_point_number,
    normalize_frequency_range, parse_control_point_set,
};
pub use error::{CoreError, CoreResult};
pub use model::{
    AccountProfile, AccountState, AlbumRef, ArtistRef, AudioQuality, Capabilities, CollectionId,
    CollectionKind, CollectionPage, CollectionSummary, LoginChallenge, LoginMethod, LoginProgress,
    LyricLine, Lyrics, QualityTier, ResolvedStream, SearchQuery, SearchResults, SourceId,
    SourceKind, StreamHandle, TimedLine, TimedWord, Track, TrackId, TrackOrigin, TrackPage,
};
pub use playlist::{
    PlaylistEditCapabilities, PlaylistEditResult, PlaylistEditor, PlaylistEntry, PlaylistId,
    PlaylistKind, PlaylistMutation, PlaylistSummary, SavedPlaylist,
};
pub use registry::{SourceDescriptor, SourceRegistry, SourceStatus};
pub use source::{AccountSource, CatalogSource, MusicSource, SourceInfo};
