//! 本地/订阅歌单模型与统一编辑契约。
//!
//! 歌单条目**保存完整 `Track`**（含源实例 ID 与源内 ID），因此本地不缓存
//! 音频副本也能回到原音源继续播放；源关闭/删除时由 UI 用
//! `Track::identity_key()` 查找本地替代副本，找不到才灰显。
//!
//! 编辑动作统一为 [`PlaylistMutation`]，本地存储与远端音源各自实现
//! [`PlaylistEditor`]，UI 只写一套。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::model::{CollectionId, CollectionKind, Track};

/// 本地歌单 ID。与源内集合 ID 分离，订阅歌单同时保留源集合 ID。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct PlaylistId(pub String);

impl PlaylistId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for PlaylistId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 歌单类型：本地自建，或从某个音源订阅的远程歌单本地缓存。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaylistKind {
    Local,
    Subscribed {
        /// 远程歌单的跨源 ID；刷新时用它重新拉取。
        collection: CollectionId,
        /// 上次成功同步的 Unix 毫秒。
        last_synced_ms: u64,
    },
}

impl PlaylistKind {
    pub fn is_subscribed(&self) -> bool {
        matches!(self, Self::Subscribed { .. })
    }

    pub fn subscribed_collection(&self) -> Option<&CollectionId> {
        match self {
            Self::Subscribed { collection, .. } => Some(collection),
            Self::Local => None,
        }
    }
}

/// 歌单条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistEntry {
    /// 完整曲目：保留源实例 ID / 源内 ID / 封面等，用于回源播放。
    pub track: Track,
    /// 跨源身份键快照：歌单内去重、失效替代、歌单差异比较。
    #[serde(default)]
    pub identity_key: String,
    pub added_at_ms: u64,
}

impl PlaylistEntry {
    pub fn new(track: Track, added_at_ms: u64) -> Self {
        Self {
            identity_key: track.identity_key(),
            track,
            added_at_ms,
        }
    }
}

/// 保存到本地的歌单记录（本地自建或订阅缓存）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPlaylist {
    pub id: PlaylistId,
    pub title: String,
    pub kind: PlaylistKind,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub creator: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub entries: Vec<PlaylistEntry>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl SavedPlaylist {
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn contains_identity(&self, identity_key: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.identity_key == identity_key)
    }

    /// 追加曲目；按身份键去重。返回实际新增数量。
    pub fn add_tracks(&mut self, tracks: impl IntoIterator<Item = Track>, now_ms: u64) -> usize {
        let mut added = 0usize;
        for track in tracks {
            let identity_key = track.identity_key();
            if self.contains_identity(&identity_key) {
                continue;
            }
            self.entries.push(PlaylistEntry {
                track,
                identity_key,
                added_at_ms: now_ms,
            });
            added += 1;
        }
        if added > 0 {
            self.updated_at_ms = now_ms;
        }
        added
    }

    /// 用远端/新快照替换曲目（订阅刷新）。保留本地条目顺序？
    /// 订阅缓存是远端镜像：直接替换；本地歌单不走这里。
    pub fn replace_tracks(&mut self, tracks: impl IntoIterator<Item = Track>, now_ms: u64) {
        self.entries = tracks
            .into_iter()
            .map(|track| PlaylistEntry {
                identity_key: track.identity_key(),
                track,
                added_at_ms: now_ms,
            })
            .collect();
        self.updated_at_ms = now_ms;
    }

    pub fn remove_identity(&mut self, identity_key: &str) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.identity_key != identity_key);
        before != self.entries.len()
    }

    pub fn remove_index(&mut self, index: usize) -> Option<PlaylistEntry> {
        if index >= self.entries.len() {
            return None;
        }
        Some(self.entries.remove(index))
    }

    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.entries.len() || to >= self.entries.len() || from == to {
            return false;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        true
    }

    /// 订阅刷新时判断远端内容是否变化（按身份键序列比较）。
    pub fn tracks_differs(&self, tracks: &[Track]) -> bool {
        if self.entries.len() != tracks.len() {
            return true;
        }
        self.entries
            .iter()
            .zip(tracks)
            .any(|(entry, track)| entry.identity_key != track.identity_key())
    }

    pub fn summary(&self) -> PlaylistSummary {
        PlaylistSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            kind: self.kind.clone(),
            count: self.entries.len(),
            cover: self.cover.clone(),
            creator: self.creator.clone(),
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// 侧栏/列表用的轻量摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistSummary {
    pub id: PlaylistId,
    pub title: String,
    pub kind: PlaylistKind,
    pub count: usize,
    pub cover: Option<String>,
    pub creator: Option<String>,
    pub updated_at_ms: u64,
}

impl PlaylistSummary {
    pub fn is_subscribed(&self) -> bool {
        self.kind.is_subscribed()
    }

    pub fn subscribed_source(&self) -> Option<&crate::model::SourceId> {
        self.kind
            .subscribed_collection()
            .map(|collection| &collection.source)
    }
}

/// 统一的歌单编辑动作。UI 只构造它，具体由本地存储或远端音源执行。
///
/// 远端实现需要源内曲目 ID，因此删除/排序都携带完整 `Track` 快照；
/// 本地存储用 `identity_key()` 去重与删除。
#[derive(Debug, Clone)]
pub enum PlaylistMutation {
    Create {
        title: String,
        tracks: Vec<Track>,
    },
    Rename {
        title: String,
    },
    AddTracks {
        tracks: Vec<Track>,
    },
    RemoveTracks {
        tracks: Vec<Track>,
    },
    Reorder {
        from: usize,
        to: usize,
        /// 调整后的完整顺序；远端编辑器据此提交 ids。
        ordered: Vec<Track>,
    },
    Delete,
}

/// 某个编辑实现支持的动作，UI 据此隐藏不可用按钮。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaylistEditCapabilities {
    pub create: bool,
    pub rename: bool,
    pub add_tracks: bool,
    pub remove_tracks: bool,
    pub reorder: bool,
    pub delete: bool,
}

impl PlaylistEditCapabilities {
    pub const fn local() -> Self {
        Self {
            create: true,
            rename: true,
            add_tracks: true,
            remove_tracks: true,
            reorder: true,
            delete: true,
        }
    }

    pub const fn read_only() -> Self {
        Self {
            create: false,
            rename: false,
            add_tracks: false,
            remove_tracks: false,
            reorder: false,
            delete: false,
        }
    }
}

/// 编辑结果。远端创建会返回新歌单的源内 ID。
#[derive(Debug, Clone, Default)]
pub struct PlaylistEditResult {
    pub playlist_id: Option<CollectionId>,
    pub changed: usize,
}

/// 远端音源实现：把统一动作映射到自己的 HTTP API / SDK。
///
/// 本地歌单不需要实现它（存储层直接执行）；订阅缓存也走本地存储。
#[async_trait]
pub trait PlaylistEditor: Send + Sync {
    fn playlist_edit_capabilities(&self, kind: CollectionKind) -> PlaylistEditCapabilities;
    async fn apply_playlist_mutation(
        &self,
        playlist: Option<&CollectionId>,
        mutation: &PlaylistMutation,
    ) -> CoreResult<PlaylistEditResult>;
}

/// 新建空本地歌单的默认标题。
pub fn default_playlist_title(index: usize) -> String {
    if index <= 1 {
        "新建歌单".to_string()
    } else {
        format!("新建歌单 {index}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AlbumRef, ArtistRef, SourceId, TrackId, TrackOrigin};

    fn track(source: &str, id: &str, title: &str, artist: &str) -> Track {
        let mut track = Track::new(
            TrackId::new(SourceId::new(source), id),
            title,
            vec![ArtistRef {
                id: None,
                name: artist.into(),
            }],
            Some(AlbumRef {
                id: None,
                name: "Album".into(),
                cover: None,
            }),
            Some(180_000),
            TrackOrigin::Remote {
                source: SourceId::new(source),
            },
        );
        track.track_no = Some(2);
        track.rebuild_identity_key();
        track
    }

    fn playlist() -> SavedPlaylist {
        SavedPlaylist {
            id: PlaylistId::new("pl-1"),
            title: "测试".into(),
            kind: PlaylistKind::Local,
            cover: None,
            creator: None,
            description: None,
            entries: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    #[test]
    fn adding_dedupes_by_cross_source_identity() {
        let mut playlist = playlist();
        let a = track("netease", "1", "Hype", "Such & PSYQUI");
        let b = track("local", "2", "hype", "PSYQUI;Such");
        assert_eq!(playlist.add_tracks(vec![a, b], 10), 1);
        assert_eq!(playlist.count(), 1);
        assert_eq!(playlist.updated_at_ms, 10);
    }

    #[test]
    fn reorder_and_remove_keep_identity_keys() {
        let mut playlist = playlist();
        let a = track("netease", "1", "A", "X");
        let b = track("netease", "2", "B", "X");
        playlist.add_tracks(vec![a, b], 10);
        assert!(playlist.reorder(0, 1));
        assert_eq!(playlist.entries[0].track.title, "B");
        let key = playlist.entries[0].identity_key.clone();
        assert!(playlist.remove_identity(&key));
        assert_eq!(playlist.count(), 1);
    }

    #[test]
    fn subscription_diff_detects_track_changes() {
        let mut playlist = playlist();
        let a = track("netease", "1", "A", "X");
        let b = track("netease", "2", "B", "X");
        playlist.add_tracks(vec![a.clone()], 10);
        assert!(!playlist.tracks_differs(std::slice::from_ref(&a)));
        assert!(playlist.tracks_differs(&[a, b]));
    }
}
