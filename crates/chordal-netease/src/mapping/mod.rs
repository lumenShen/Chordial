//! api-enhanced 响应 DTO → 领域模型的映射。
//!
//! 全部字段 `Option` + 宽松匹配：api-enhanced 是社区逆向项目，响应结构
//! 偶有漂移，映射必须"缺什么补什么"，绝不 panic。
//! 所有 map 函数均为 O(返回条目数) 单遍。

use std::collections::HashSet;

use serde::Deserialize;

use chordal_core::error::{CoreError, CoreResult};
use chordal_core::model::{

    AccountProfile, AlbumRef, ArtistRef, AudioQuality, CollectionId, CollectionKind,
    CollectionPage, CollectionSummary, LyricLine, Lyrics, QualityTier, ResolvedStream, SearchKind,
    SearchResults, SourceId, StreamHandle, Track, TrackId, TrackOrigin, TrackPage,
};

mod search;
mod detail;
mod stream;
mod lyrics;
mod account;
mod collections;
pub use search::*;
pub use detail::*;
pub use stream::*;
pub use lyrics::*;
pub use account::*;
pub use collections::*;

/// 音质档位（/song/url/v1 的 level 参数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SongQualityLevel {
    Standard,
    Higher,
    #[default]
    ExHigh,
    Lossless,
    HireS,
}

impl SongQualityLevel {
    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Higher => "higher",
            Self::ExHigh => "exhigh",
            Self::Lossless => "lossless",
            Self::HireS => "hires",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "128k",
            Self::Higher => "192k",
            Self::ExHigh => "320k",
            Self::Lossless => "FLAC",
            Self::HireS => "Hi-Res",
        }
    }
}

// ---------- /search ----------


#[cfg(test)]

#[cfg(test)]
mod tests;
