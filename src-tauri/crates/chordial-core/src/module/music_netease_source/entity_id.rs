//! 网易云 entity_id 的编码与解析。
//!
//! entity_id 是库内 `SourceId.entity_id` 使用的稳定字符串，编码规则：
//! - 歌曲：`"song:<id>"`
//! - 专辑：`"album:<id>"`
//! - 歌手：`"artist:<id>"`
//! - 歌词：`"lyric:<song_id>:<format>"`（format 为 `yrc` / `lrc`）
//! - 歌单：`"playlist:<id>"`（保留，未来可能将歌单作为库内实体追踪）

/// entity_id 中各部分的分隔符。
pub const ENTITY_ID_SEP: &str = ":";

/// entity_id 的种类前缀。
#[allow(non_camel_case_types)]
pub enum EntityKind {
    song,
    artist,
    album,
    lyric,
    /// 保留：未来可能将歌单作为库内实体追踪
    #[allow(dead_code)]
    playlist,
}

impl EntityKind {
    /// 返回前缀字符串（如 `"song"`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::song => "song",
            Self::artist => "artist",
            Self::album => "album",
            Self::lyric => "lyric",
            Self::playlist => "playlist",
        }
    }
}

/// 构建网易云 entity_id。
pub fn build_entity_id(kind: EntityKind, id: i64) -> String {
    format!("{}{}{}", kind.as_str(), ENTITY_ID_SEP, id)
}

/// 构建歌词的 entity_id（携带格式以区分 YRC/LRC）。
pub fn build_lyric_entity_id(song_id: i64, format: &str) -> String {
    format!(
        "{}{}{}{}{}",
        EntityKind::lyric.as_str(),
        ENTITY_ID_SEP,
        song_id,
        ENTITY_ID_SEP,
        format
    )
}

/// 解析 entity_id 中的网易云 ID（去掉 `kind:` 前缀后的部分）。
pub fn parse_remote_id(entity_id: &str) -> Option<i64> {
    let parts: Vec<&str> = entity_id.splitn(3, ENTITY_ID_SEP).collect();
    if parts.len() < 2 {
        return None;
    }
    parts[1].parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_entity_id() {
        assert_eq!(build_entity_id(EntityKind::song, 123456), "song:123456");
        assert_eq!(build_entity_id(EntityKind::artist, 4625), "artist:4625");
        assert_eq!(build_entity_id(EntityKind::album, 789), "album:789");
    }

    #[test]
    fn test_parse_remote_id() {
        assert_eq!(parse_remote_id("song:123456"), Some(123456));
        assert_eq!(parse_remote_id("artist:4625"), Some(4625));
        assert_eq!(parse_remote_id("album:789"), Some(789));
        // lyric:<id>:<format> 仅返回第一段
        assert_eq!(parse_remote_id("lyric:123:yrc"), Some(123));
        // 无前缀 / 非数字
        assert_eq!(parse_remote_id("no_prefix"), None);
        assert_eq!(parse_remote_id(""), None);
        assert_eq!(parse_remote_id("song:abc"), None);
    }

    #[test]
    fn test_build_lyric_entity_id() {
        assert_eq!(
            build_lyric_entity_id(123456, "yrc"),
            "lyric:123456:yrc"
        );
        assert_eq!(
            build_lyric_entity_id(123456, "lrc"),
            "lyric:123456:lrc"
        );
    }
}