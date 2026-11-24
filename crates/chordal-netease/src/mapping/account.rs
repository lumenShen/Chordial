//! 网易云映射：account（由 mapping.rs 拆分，行为不变）。

use super::*;

// ---------- 账号 / 集合 ----------

#[derive(Deserialize)]
pub(super) struct UserAccountResp {
    #[serde(default)]
    account: Option<AccountBody>,
    #[serde(default)]
    profile: Option<ProfileBody>,
}

#[derive(Deserialize)]
pub(super) struct AccountBody {
    #[serde(rename = "vipType", default)]
    vip_type: Option<u64>,
    /// api-enhanced 未登录时会自动注册匿名账号（type=1000）。
    /// 这种账号同样返回 code 200 + account.id，但没有 profile/歌单。
    #[serde(rename = "anonimousUser", default)]
    anonymous: bool,
}

#[derive(Deserialize, Default)]
pub(super) struct ProfileBody {
    #[serde(rename = "userId", default)]
    user_id: Option<u64>,
    #[serde(default)]
    nickname: Option<String>,
    #[serde(rename = "avatarUrl", default)]
    avatar_url: Option<String>,
    #[serde(rename = "vipType", default)]
    vip_type: Option<u64>,
}

/// `/user/account` → 账号资料。未登录/匿名账号返回 None。
pub fn map_account(source: &SourceId, body: &serde_json::Value) -> Option<AccountProfile> {
    let resp: UserAccountResp = serde_json::from_value(body.clone()).ok()?;
    // 匿名账号不是真实登录：uid 是服务端自动注册的临时用户，昵称、歌单全空。
    if resp.account.as_ref().is_some_and(|a| a.anonymous) {
        return None;
    }
    // 真实登录一定有 profile；只有 account.id 的响应（匿名/过期 Cookie）不可用。
    let profile = resp.profile?;
    let uid = profile.user_id?;
    let vip_type = profile
        .vip_type
        .or_else(|| resp.account.as_ref().and_then(|a| a.vip_type))
        .unwrap_or(0);
    Some(AccountProfile {
        source: source.clone(),
        uid: uid.to_string(),
        nickname: profile
            .nickname
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("用户 {uid}")),
        avatar: profile.avatar_url.filter(|u| !u.trim().is_empty()),
        vip: vip_type > 0,
    })
}

#[derive(Deserialize, Default)]
pub(super) struct PlaylistListResp {
    #[serde(default)]
    playlist: Option<Vec<PlaylistBrief>>,
}

#[derive(Deserialize)]
pub(super) struct PlaylistBrief {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "coverImgUrl", default)]
    cover: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "trackCount", default)]
    track_count: Option<u64>,
    #[serde(default)]
    subscribed: Option<bool>,
    #[serde(default)]
    creator: Option<Creator>,
}

#[derive(Deserialize)]
pub(super) struct Creator {
    #[serde(default)]
    pub(super) nickname: Option<String>,
}

pub(super) fn playlist_summary(
    source: &SourceId,
    p: PlaylistBrief,
    kind: CollectionKind,
) -> Option<CollectionSummary> {
    let id = p.id?;
    Some(CollectionSummary {
        id: CollectionId::new(source.clone(), kind, id.to_string()),
        title: p.name.unwrap_or_else(|| format!("歌单 {id}")),
        cover: p.cover,
        creator: p.creator.and_then(|c| c.nickname),
        description: p.description,
        track_count: p.track_count,
        year: None,
        subscribed: p.subscribed.unwrap_or(false),
        kind,
    })
}

/// `/user/playlist` → 用户歌单摘要。
pub fn map_user_playlists(source: &SourceId, body: &serde_json::Value) -> Vec<CollectionSummary> {
    let resp: PlaylistListResp = match serde_json::from_value(body.clone()) {
        Ok(resp) => resp,
        Err(_) => return Vec::new(),
    };
    resp.playlist
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| playlist_summary(source, p, CollectionKind::Playlist))
        .collect()
}

/// 推荐类接口 → 歌单摘要列表。
///
/// 推荐接口的包装层历史上出现过 `recommend` / `playlist` / `playlists` /
/// `result` / `data` 多种形态（私人雷达若由自建服务实现也可能不同），
/// 因此按优先级逐个尝试，单条解析失败只跳过该条。
pub fn map_playlist_list(
    source: &SourceId,
    body: &serde_json::Value,
    kind: CollectionKind,
) -> Vec<CollectionSummary> {
    fn array_at<'a>(body: &'a serde_json::Value, key: &str) -> Option<&'a Vec<serde_json::Value>> {
        body.get(key).and_then(serde_json::Value::as_array)
    }
    let data = body.get("data");
    let items = array_at(body, "recommend")
        .or_else(|| array_at(body, "playlists"))
        .or_else(|| array_at(body, "playlist"))
        .or_else(|| array_at(body, "result"))
        .or_else(|| data.and_then(serde_json::Value::as_array))
        .or_else(|| data.and_then(|data| array_at(data, "playlists")))
        .or_else(|| data.and_then(|data| array_at(data, "playlist")))
        .or_else(|| data.and_then(|data| array_at(data, "list")))
        .or_else(|| data.and_then(|data| array_at(data, "recommend")));
    let Some(items) = items else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| serde_json::from_value::<PlaylistBrief>(item.clone()).ok())
        .filter_map(|playlist| playlist_summary(source, playlist, kind))
        .collect()
}
