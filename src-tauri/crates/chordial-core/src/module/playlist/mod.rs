//! 歌单系统 — 本地保存用户从在线来源（网易云等）搜索到的歌单，并支持编辑。
//!
//! # 数据模型
//!
//! ```text
//! Playlist
//!   ├── id: 库内 UUID
//!   ├── source: 来源类型（netease / local）
//!   ├── remote_id: 在线来源中的 ID（如网易云歌单 ID）
//!   ├── name, cover_url, creator, description
//!   ├── tags: Vec<String>
//!   ├── song_ids: Vec<String>  ← 引用库内 Song.id
//!   └── created_at, updated_at
//! ```
//!
//! # 持久化
//!
//! 通过 [`PersistentStore`] 持久化到 `storage.json` 的 `playlists` key，
//! 每个 Playlist 作为 subkey 存储，支持 O(1) 查找和分页。
//!
//! # 工作流程
//!
//! 1. **搜索在线歌单**：`netease_search_playlists` 命令调用
//!    [`NeteaseCloudMusicApiSource::search_playlists`]
//! 2. **保存到本地**：`playlist_save` 命令将歌单详情 + 全部曲目保存为本地 Playlist
//!    - 曲目通过 `library.add_song` 添加到库（带 netease SourceId）
//!    - Playlist.song_ids 保存库内 Song.id 列表
//! 3. **编辑歌单**：`playlist_update` 修改元数据；`playlist_add_song` /
//!    `playlist_remove_song` 修改曲目列表
//! 4. **从右键菜单添加歌曲**：`playlist_add_song` 命令

use crate::module::storage::persistent::PersistentStore;
use serde::{Deserialize, Serialize};

/// 歌单存储 key。
pub const KEY: &str = "playlists";

/// 歌单模型。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    /// 库内统一 ID（UUID）
    pub id: String,
    /// 来源类型（"netease" / "local"）
    pub source: String,
    /// 在线来源中的歌单 ID（若来自在线；本地创建则为 None）
    #[serde(default)]
    pub remote_id: Option<i64>,
    /// 歌单名称
    pub name: String,
    /// 封面 URL
    #[serde(default)]
    pub cover_url: Option<String>,
    /// 创建者名称
    #[serde(default)]
    pub creator: Option<String>,
    /// 描述
    #[serde(default)]
    pub description: Option<String>,
    /// 标签
    #[serde(default)]
    pub tags: Vec<String>,
    /// 包含的歌曲 ID 列表（引用库内 Song.id）
    #[serde(default)]
    pub song_ids: Vec<String>,
    /// 创建时间（Unix 毫秒）
    pub created_at: i64,
    /// 最后更新时间（Unix 毫秒）
    pub updated_at: i64,
}

impl Playlist {
    /// 创建新歌单（自动生成 ID 和时间戳）。
    pub fn new(
        source: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        let now = chrono_now_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source: source.into(),
            remote_id: None,
            name: name.into(),
            cover_url: None,
            creator: None,
            description: None,
            tags: Vec::new(),
            song_ids: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// 获取当前 Unix 毫秒时间戳（不依赖 chrono，避免引入额外依赖）。
fn chrono_now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── CRUD 操作 ─────────────────────────────────────────────────────────────

/// 获取所有歌单。
pub fn get_all(store: &PersistentStore) -> Vec<Playlist> {
    store.get_all_entries::<Playlist>(KEY)
}

/// 按 ID 获取歌单。
pub fn get(store: &PersistentStore, id: &str) -> Option<Playlist> {
    store.get_entry::<Playlist>(KEY, id)
}

/// 添加歌单。
pub fn add(store: &PersistentStore, playlist: &Playlist) -> Result<(), String> {
    if store.has_entry(KEY, &playlist.id) {
        return Err(format!("歌单 id={} 已存在", playlist.id));
    }
    store.set_subkey(KEY, &playlist.id, playlist)
}

/// 更新歌单（必须已存在）。
pub fn update(store: &PersistentStore, playlist: &Playlist) -> Result<(), String> {
    if !store.has_entry(KEY, &playlist.id) {
        return Err(format!("歌单 id={} 不存在", playlist.id));
    }
    store.set_subkey(KEY, &playlist.id, playlist)
}

/// 删除歌单，返回是否删除成功。
pub fn remove(store: &PersistentStore, id: &str) -> bool {
    store.remove_entry(KEY, id)
}

/// 获取歌单总数。
pub fn count(store: &PersistentStore) -> usize {
    store.count_entries(KEY)
}

/// 向歌单追加歌曲。
///
/// 若歌曲已在歌单中则跳过（去重）。
pub fn add_song(store: &PersistentStore, playlist_id: &str, song_id: &str) -> Result<(), String> {
    let mut playlist = get(store, playlist_id)
        .ok_or_else(|| format!("歌单 {} 不存在", playlist_id))?;
    if !playlist.song_ids.iter().any(|s| s == song_id) {
        playlist.song_ids.push(song_id.to_string());
        playlist.updated_at = chrono_now_millis();
        update(store, &playlist)?;
    }
    Ok(())
}

/// 从歌单移除歌曲。
pub fn remove_song(
    store: &PersistentStore,
    playlist_id: &str,
    song_id: &str,
) -> Result<(), String> {
    let mut playlist = get(store, playlist_id)
        .ok_or_else(|| format!("歌单 {} 不存在", playlist_id))?;
    let len_before = playlist.song_ids.len();
    playlist.song_ids.retain(|s| s != song_id);
    if playlist.song_ids.len() != len_before {
        playlist.updated_at = chrono_now_millis();
        update(store, &playlist)?;
    }
    Ok(())
}

/// 调整歌单内歌曲顺序。
///
/// `new_order` 为新的 song_id 顺序，长度必须与原 song_ids 一致。
pub fn reorder_songs(
    store: &PersistentStore,
    playlist_id: &str,
    new_order: Vec<String>,
) -> Result<(), String> {
    let mut playlist = get(store, playlist_id)
        .ok_or_else(|| format!("歌单 {} 不存在", playlist_id))?;
    if new_order.len() != playlist.song_ids.len() {
        return Err(format!(
            "歌曲数量不匹配：期望 {} 实际 {}",
            playlist.song_ids.len(),
            new_order.len()
        ));
    }
    playlist.song_ids = new_order;
    playlist.updated_at = chrono_now_millis();
    update(store, &playlist)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_store() -> PersistentStore {
        let tmp = std::env::temp_dir().join(format!(
            "chordial_test_playlist_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);
        PersistentStore::new(tmp)
    }

    #[test]
    fn test_playlist_lifecycle() {
        let store = temp_store();
        let mut p = Playlist::new("netease", "我的歌单");
        p.description = Some("test".to_string());

        // 添加
        add(&store, &p).unwrap();
        assert_eq!(count(&store), 1);

        // 获取
        let got = get(&store, &p.id).unwrap();
        assert_eq!(got.name, "我的歌单");
        assert_eq!(got.description.as_deref(), Some("test"));

        // 更新
        let mut p2 = got.clone();
        p2.name = "改名".to_string();
        update(&store, &p2).unwrap();
        let got2 = get(&store, &p.id).unwrap();
        assert_eq!(got2.name, "改名");

        // 添加歌曲
        add_song(&store, &p.id, "song1").unwrap();
        add_song(&store, &p.id, "song2").unwrap();
        // 重复添加应被去重
        add_song(&store, &p.id, "song1").unwrap();
        let got3 = get(&store, &p.id).unwrap();
        assert_eq!(got3.song_ids.len(), 2);

        // 移除歌曲
        remove_song(&store, &p.id, "song1").unwrap();
        let got4 = get(&store, &p.id).unwrap();
        assert_eq!(got4.song_ids, vec!["song2".to_string()]);

        // 删除歌单
        assert!(remove(&store, &p.id));
        assert_eq!(count(&store), 0);
    }

    #[test]
    fn test_reorder_songs() {
        let store = temp_store();
        let mut p = Playlist::new("local", "test");
        p.song_ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        add(&store, &p).unwrap();

        // 重排
        reorder_songs(&store, &p.id, vec!["c".into(), "a".into(), "b".into()]).unwrap();
        let got = get(&store, &p.id).unwrap();
        assert_eq!(got.song_ids, vec!["c".to_string(), "a".to_string(), "b".to_string()]);

        // 长度不匹配应失败
        let err = reorder_songs(&store, &p.id, vec!["a".into()]);
        assert!(err.is_err());
    }
}
