//! 本地歌单存储：`config.json` 同目录的 `playlists.json`。
//!
//! - 本地自建歌单与订阅歌单共用一份文件；
//! - 条目保存完整 `Track`（源实例 ID + 源内 ID），不保存音频副本；
//! - 订阅歌单是远端歌单的本地镜像，打开时先渲染本地，后台再刷新；
//! - 所有去重使用 `Track::identity_key()`（歌名 + 分列艺人 + 专辑 + 曲序）。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chordal_core::model::{CollectionId, CollectionSummary, Track};
use chordal_core::playlist::{
    PlaylistId, PlaylistKind, PlaylistSummary, SavedPlaylist, default_playlist_title,
};
use serde::{Deserialize, Serialize};

use crate::storage::JsonWriter;

#[derive(Debug, Default, Serialize, Deserialize)]
struct PlaylistFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    playlists: Arc<Vec<SavedPlaylist>>,
}

/// 本地歌单存储。UI 持不可变快照，写盘由单一 writer 去抖并原子替换。
pub struct PlaylistStore {
    path: PathBuf,
    playlists: Arc<Vec<SavedPlaylist>>,
    revision: u64,
    writer: Option<Arc<JsonWriter>>,
}

impl PlaylistStore {
    /// 默认路径：与 `config.json` 同目录。
    pub fn load_default_with_writer(writer: Option<Arc<JsonWriter>>) -> Self {
        let path = crate::config::config_path()
            .parent()
            .map(|dir| dir.join("playlists.json"))
            .unwrap_or_else(|| PathBuf::from("playlists.json"));
        Self::load_with_writer(path, writer)
    }

    pub fn load_from(path: impl Into<PathBuf>) -> Self {
        Self::load_with_writer(path.into(), None)
    }

    fn load_with_writer(path: PathBuf, writer: Option<Arc<JsonWriter>>) -> Self {
        let mut playlists: Arc<Vec<SavedPlaylist>> = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<PlaylistFile>(&bytes) {
                Ok(file) => file.playlists,
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "playlists.json parse failed; starting empty"
                    );
                    Arc::new(Vec::new())
                }
            },
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!(path = %path.display(), error = %e, "playlists.json read failed");
                Arc::new(Vec::new())
            }
            Err(_) => Arc::new(Vec::new()),
        };
        // 防御：手改文件可能留下重复 ID；保留首个，后续重新编号。
        let mut seen = std::collections::HashSet::new();
        for playlist in Arc::make_mut(&mut playlists) {
            if playlist.id.0.trim().is_empty() || !seen.insert(playlist.id.0.clone()) {
                playlist.id = PlaylistId::new(new_playlist_id());
                seen.insert(playlist.id.0.clone());
            }
        }
        Self {
            path,
            playlists,
            revision: 0,
            writer,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn playlists(&self) -> &[SavedPlaylist] {
        self.playlists.as_slice()
    }

    fn playlists_mut(&mut self) -> &mut Vec<SavedPlaylist> {
        Arc::make_mut(&mut self.playlists)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn summaries(&self) -> Vec<PlaylistSummary> {
        self.playlists.iter().map(SavedPlaylist::summary).collect()
    }

    pub fn get(&self, id: &PlaylistId) -> Option<&SavedPlaylist> {
        self.playlists.iter().find(|p| &p.id == id)
    }

    pub fn get_mut(&mut self, id: &PlaylistId) -> Option<&mut SavedPlaylist> {
        self.playlists_mut().iter_mut().find(|p| &p.id == id)
    }

    pub fn find_subscription(&self, collection: &CollectionId) -> Option<&SavedPlaylist> {
        self.playlists.iter().find(|playlist| {
            playlist
                .kind
                .subscribed_collection()
                .is_some_and(|id| id == collection)
        })
    }

    pub fn is_subscribed(&self, collection: &CollectionId) -> bool {
        self.find_subscription(collection).is_some()
    }

    /// 新建本地歌单（追加到列表末尾），返回 ID。
    pub fn create_local(&mut self, title: impl Into<String>, tracks: Vec<Track>) -> PlaylistId {
        let now = now_ms();
        let title = title.into().trim().to_string();
        let title = if title.is_empty() {
            default_playlist_title(
                self.playlists
                    .iter()
                    .filter(|p| matches!(p.kind, PlaylistKind::Local))
                    .count()
                    + 1,
            )
        } else {
            title
        };
        let mut playlist = SavedPlaylist {
            id: PlaylistId::new(new_playlist_id()),
            title,
            kind: PlaylistKind::Local,
            cover: None,
            creator: None,
            description: None,
            entries: Vec::new(),
            created_at_ms: now,
            updated_at_ms: now,
        };
        playlist.add_tracks(tracks, now);
        let id = playlist.id.clone();
        self.playlists_mut().push(playlist);
        self.save();
        id
    }

    /// 订阅/更新一个远程歌单。已订阅则更新摘要与曲目快照，保留本地 ID。
    pub fn upsert_subscription(
        &mut self,
        collection: &CollectionSummary,
        tracks: Vec<Track>,
    ) -> PlaylistId {
        let now = now_ms();
        if let Some(index) = self.playlists.iter().position(|playlist| {
            playlist
                .kind
                .subscribed_collection()
                .is_some_and(|id| id == &collection.id)
        }) {
            let playlist = &mut self.playlists_mut()[index];
            let changed = playlist.title != collection.title
                || playlist.cover != collection.cover
                || playlist.creator != collection.creator
                || playlist.description != collection.description
                || playlist.tracks_differs(&tracks);
            playlist.title = collection.title.clone();
            playlist.cover = collection.cover.clone();
            playlist.creator = collection.creator.clone();
            playlist.description = collection.description.clone();
            playlist.kind = PlaylistKind::Subscribed {
                collection: collection.id.clone(),
                last_synced_ms: now,
            };
            if changed {
                playlist.replace_tracks(tracks, now);
            } else {
                playlist.updated_at_ms = now;
            }
            let id = playlist.id.clone();
            self.save();
            return id;
        }

        let mut playlist = SavedPlaylist {
            id: PlaylistId::new(new_playlist_id()),
            title: collection.title.clone(),
            kind: PlaylistKind::Subscribed {
                collection: collection.id.clone(),
                last_synced_ms: now,
            },
            cover: collection.cover.clone(),
            creator: collection.creator.clone(),
            description: collection.description.clone(),
            entries: Vec::new(),
            created_at_ms: now,
            updated_at_ms: now,
        };
        playlist.replace_tracks(tracks, now);
        let id = playlist.id.clone();
        self.playlists_mut().push(playlist);
        self.save();
        id
    }

    /// 后台刷新成功：若内容有变化则替换曲目并落盘。
    pub fn refresh_subscription(
        &mut self,
        id: &PlaylistId,
        collection: &CollectionSummary,
        tracks: Vec<Track>,
    ) -> bool {
        let Some(playlist) = self.get_mut(id) else {
            return false;
        };
        let now = now_ms();
        let changed = playlist.tracks_differs(&tracks)
            || playlist.title != collection.title
            || playlist.cover != collection.cover;
        playlist.title = collection.title.clone();
        playlist.cover = collection.cover.clone();
        playlist.creator = collection.creator.clone();
        playlist.description = collection.description.clone();
        if let PlaylistKind::Subscribed { last_synced_ms, .. } = &mut playlist.kind {
            *last_synced_ms = now;
        }
        if changed {
            playlist.replace_tracks(tracks, now);
        } else {
            playlist.updated_at_ms = now;
        }
        self.save();
        changed
    }

    /// 订阅是否需要后台刷新（默认 5 分钟；打开时总是先显示本地）。
    pub fn subscription_is_stale(&self, id: &PlaylistId, max_age_ms: u64) -> bool {
        let Some(playlist) = self.get(id) else {
            return false;
        };
        let PlaylistKind::Subscribed { last_synced_ms, .. } = &playlist.kind else {
            return false;
        };
        now_ms().saturating_sub(*last_synced_ms) > max_age_ms
    }

    pub fn rename(&mut self, id: &PlaylistId, title: impl Into<String>) -> bool {
        let title = title.into().trim().to_string();
        if title.is_empty() {
            return false;
        }
        let Some(playlist) = self.get_mut(id) else {
            return false;
        };
        playlist.title = title;
        playlist.updated_at_ms = now_ms();
        self.save();
        true
    }

    pub fn delete(&mut self, id: &PlaylistId) -> bool {
        let before = self.playlists.len();
        self.playlists_mut().retain(|playlist| &playlist.id != id);
        let removed = self.playlists.len() != before;
        if removed {
            self.save();
        }
        removed
    }

    pub fn add_tracks(&mut self, id: &PlaylistId, tracks: Vec<Track>) -> usize {
        let Some(playlist) = self.get_mut(id) else {
            return 0;
        };
        let added = playlist.add_tracks(tracks, now_ms());
        if added > 0 {
            self.save();
        }
        added
    }

    pub fn remove_identity(&mut self, id: &PlaylistId, identity_key: &str) -> bool {
        let Some(playlist) = self.get_mut(id) else {
            return false;
        };
        let removed = playlist.remove_identity(identity_key);
        if removed {
            playlist.updated_at_ms = now_ms();
            self.save();
        }
        removed
    }

    pub fn reorder(&mut self, id: &PlaylistId, from: usize, to: usize) -> bool {
        let Some(playlist) = self.get_mut(id) else {
            return false;
        };
        let moved = playlist.reorder(from, to);
        if moved {
            playlist.updated_at_ms = now_ms();
            self.save();
        }
        moved
    }

    fn save(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        let file = PlaylistFile {
            version: 1,
            playlists: self.playlists.clone(),
        };
        if let Some(writer) = &self.writer {
            writer.save_json(self.path.clone(), file);
        } else {
            match serde_json::to_vec(&file) {
                Ok(bytes) => {
                    if let Err(error) = chordal_core::fs::atomic_write(&self.path, &bytes) {
                        tracing::warn!(
                            path = %self.path.display(),
                            %error,
                            "playlists.json replace failed"
                        );
                    }
                }
                Err(error) => tracing::warn!(
                    path = %self.path.display(),
                    %error,
                    "playlists.json serialization failed"
                ),
            }
        }
    }

    pub fn flush(&self) {
        if let Some(writer) = &self.writer
            && let Err(error) = writer.flush()
        {
            tracing::warn!(path = %self.path.display(), %error, "playlists flush failed");
        }
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn new_playlist_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "pl-{}-{}-{}",
        now_ms(),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chordal_core::model::{
        AlbumRef, ArtistRef, CollectionId, CollectionKind, SourceId, TrackId, TrackOrigin,
    };

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

    fn temp_store() -> PlaylistStore {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "chordial-playlist-test-{}-{}-{}.json",
            std::process::id(),
            now_ms(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        PlaylistStore::load_from(path)
    }

    #[test]
    fn creates_and_persists_local_playlist() {
        let mut store = temp_store();
        let id = store.create_local("通勤", vec![track("netease", "1", "A", "X")]);
        assert_eq!(store.get(&id).unwrap().count(), 1);
        let path = store.path().to_path_buf();
        let reloaded = PlaylistStore::load_from(&path);
        assert_eq!(reloaded.get(&id).unwrap().title, "通勤");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn revision_changes_only_when_playlists_change() {
        let mut store = temp_store();
        assert_eq!(store.revision(), 0);
        let id = store.create_local(" revision ", vec![]);
        assert_eq!(store.revision(), 1);
        assert!(!store.rename(&id, "   "));
        assert_eq!(store.revision(), 1);
        assert!(store.rename(&id, "renamed"));
        assert_eq!(store.revision(), 2);
        assert!(store.delete(&id));
        assert_eq!(store.revision(), 3);
        assert!(!store.delete(&id));
        assert_eq!(store.revision(), 3);
        let _ = std::fs::remove_file(store.path().to_path_buf());
    }

    #[test]
    fn subscription_upsert_is_idempotent_by_collection() {
        let mut store = temp_store();
        let collection = CollectionSummary {
            id: CollectionId::new(SourceId::new("netease"), CollectionKind::Playlist, "42"),
            title: "订阅歌单".into(),
            creator: Some("someone".into()),
            track_count: Some(2),
            kind: CollectionKind::Playlist,
            ..Default::default()
        };
        let a = track("netease", "1", "A", "X");
        let b = track("netease", "2", "B", "X");
        let first = store.upsert_subscription(&collection, vec![a.clone(), b.clone()]);
        let second = store.upsert_subscription(&collection, vec![a, b]);
        assert_eq!(first, second);
        assert_eq!(
            store
                .playlists()
                .iter()
                .filter(|p| p.kind.is_subscribed())
                .count(),
            1
        );
        let _ = std::fs::remove_file(store.path().to_path_buf());
    }

    #[test]
    fn local_playlist_accepts_tracks_from_any_source() {
        let mut store = temp_store();
        let id = store.create_local("混合", vec![]);
        let added = store.add_tracks(
            &id,
            vec![
                track("local", "/a.flac", "A", "X"),
                track("netease", "2", "B", "X"),
            ],
        );
        assert_eq!(added, 2);
        let sources: Vec<_> = store
            .get(&id)
            .unwrap()
            .entries
            .iter()
            .map(|e| e.track.id.source.as_str())
            .collect();
        assert_eq!(sources, vec!["local", "netease"]);
        let _ = std::fs::remove_file(store.path().to_path_buf());
    }
}
