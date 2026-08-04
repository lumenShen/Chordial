//! 酷狗 entity_id 的编码与解析。
//!
//! entity_id 是库内 `SourceId.entity_id` 使用的稳定字符串，编码规则：
//! - 歌曲：`"song:<hash>"`（FileHash 是 32 位小写十六进制）
//! - 专辑：`"album:<album_id>"`（字符串形式，部分为 0 表示无专辑）
//! - 歌手：`"artist:<singer_id>"`
//! - 歌词：`"lyric:<id>:<accesskey>"`（搜索 lyric 时获得的 id + accesskey 复合键）

/// entity_id 中各部分的分隔符。
pub const ENTITY_ID_SEP: &str = ":";

/// entity_id 的种类前缀。
#[allow(non_camel_case_types)]
pub enum EntityKind {
    song,
    artist,
    album,
    lyric,
}

impl EntityKind {
    /// 返回前缀字符串（如 `"song"`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::song => "song",
            Self::artist => "artist",
            Self::album => "album",
            Self::lyric => "lyric",
        }
    }
}

/// 构建酷狗 entity_id。
pub fn build_entity_id(kind: EntityKind, id: &str) -> String {
    format!("{}{}{}", kind.as_str(), ENTITY_ID_SEP, id)
}

/// 构建歌词的 entity_id（携带 id + accesskey 复合键）。
pub fn build_lyric_entity_id(lyric_id: &str, accesskey: &str) -> String {
    format!(
        "{}{}{}{}{}",
        EntityKind::lyric.as_str(),
        ENTITY_ID_SEP,
        lyric_id,
        ENTITY_ID_SEP,
        accesskey
    )
}

/// 解析 entity_id 中的远程 ID（去掉 `kind:` 前缀后的部分）。
/// 对于 `lyric:<id>:<accesskey>` 格式，仅返回第一段。
pub fn parse_remote_id(entity_id: &str) -> Option<String> {
    let parts: Vec<&str> = entity_id.splitn(3, ENTITY_ID_SEP).collect();
    if parts.len() < 2 {
        return None;
    }
    Some(parts[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_entity_id() {
        assert_eq!(build_entity_id(EntityKind::song, "abc123"), "song:abc123");
        assert_eq!(
            build_entity_id(EntityKind::artist, "4625"),
            "artist:4625"
        );
        assert_eq!(build_entity_id(EntityKind::album, "12345"), "album:12345");
    }

    #[test]
    fn test_parse_remote_id() {
        assert_eq!(parse_remote_id("song:abc123"), Some("abc123".to_string()));
        assert_eq!(parse_remote_id("artist:4625"), Some("4625".to_string()));
        assert_eq!(parse_remote_id("album:12345"), Some("12345".to_string()));
        // lyric:<id>:<accesskey> 仅返回第一段
        assert_eq!(
            parse_remote_id("lyric:abc:xyz"),
            Some("abc".to_string())
        );
        // 无前缀
        assert_eq!(parse_remote_id("no_prefix"), None);
        assert_eq!(parse_remote_id(""), None);
    }

    #[test]
    fn test_build_lyric_entity_id() {
        assert_eq!(
            build_lyric_entity_id("lyric123", "access456"),
            "lyric:lyric123:access456"
        );
    }
}