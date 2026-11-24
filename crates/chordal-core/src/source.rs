//! 音乐源适配器契约。
//!
//! 任何音乐服务（本地库、自部署的网易云 api-enhanced、未来的其它源）都
//! 实现这个 trait 并交给 [`SourceRegistry`](crate::registry::SourceRegistry)
//! 加载/卸载。适配器之间零耦合：互相不知道对方存在。

use async_trait::async_trait;

use crate::error::{CoreError, CoreResult};
use crate::model::{
    AccountProfile, AccountState, Capabilities, CollectionId, CollectionPage, CollectionSummary,
    LoginChallenge, LoginMethod, LoginProgress, Lyrics, ResolvedStream, SearchQuery, SearchResults,
    SourceId, SourceKind, StreamHandle, Track, TrackId, TrackPage,
};
use crate::playlist::PlaylistEditor;

/// 源的静态元信息（注册时即确定，不随加载状态变化）。
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub id: SourceId,
    pub display_name: String,
    pub kind: SourceKind,
    /// 展示用一句话描述（如 "本地 ~/Music"、"api-enhanced @ 127.0.0.1:3000"）。
    pub description: String,
    /// 聚合搜索时的排序权重，越小越靠前。
    pub priority: u8,
}

/// 音乐源适配器。对象安全（`dyn MusicSource`），全部异步。
///
/// 实现注意：
/// - **禁止阻塞调用方线程**：所有等待（磁盘/网络/解码探测）都应在
///   `async` 内完成；CPU 密集段由实现方自行调度到后台执行器。
/// - **埋点义务**：实现方应在每个对外操作上挂 `perf::PerfScope`
///   （命名见 [`crate::perf::budget_us`] 的表），让复杂度护栏测试能观测。
#[async_trait]
pub trait MusicSource: Send + Sync {
    fn info(&self) -> &SourceInfo;

    fn capabilities(&self) -> Capabilities;

    /// 搜索。聚合层（registry）会给每个源加超时兜底，单个源卡死不影响其它源。
    async fn search(&self, query: &SearchQuery) -> CoreResult<SearchResults>;

    /// 单曲详情（标题/艺人/专辑补全）。无此能力的源返回 Err。
    async fn track(&self, id: &TrackId) -> CoreResult<Track>;

    /// 取播放句柄（本地路径或临时直链 URL）。
    async fn stream(&self, id: &TrackId) -> CoreResult<StreamHandle>;

    /// 解析播放句柄并附带实际音质。默认实现只取句柄；
    /// 在线源应覆盖它，让“本地/在线同曲”选择拿到真实码率。
    async fn resolve_stream(&self, id: &TrackId) -> CoreResult<ResolvedStream> {
        Ok(ResolvedStream {
            handle: self.stream(id).await?,
            quality: None,
        })
    }

    /// 歌词。无歌词返回 `Ok(None)`。
    async fn lyrics(&self, id: &TrackId) -> CoreResult<Option<Lyrics>> {
        let _ = id;
        Ok(None)
    }

    /// 卸载前的清理钩子。registry 在移除槽位后调用。
    async fn shutdown(&self) {}

    /// 应用退出时停止后台服务 / 子进程；不清理缓存、不注销账号。
    async fn stop_services(&self) {}

    /// 预热：启动外部服务/托管 sidecar，可在 UI 加载源后后台调用。
    async fn warm_up(&self) -> CoreResult<()> {
        Ok(())
    }

    /// 可选：目录浏览能力（专辑/歌单/歌手）。
    fn catalog(&self) -> Option<&dyn CatalogSource> {
        None
    }

    /// 可选：账号能力（登录/登出/用户歌单）。
    fn account(&self) -> Option<&dyn AccountSource> {
        None
    }

    /// 可选：歌单编辑能力。UI 通过统一 [`PlaylistMutation`] 调用，
    /// 不为每个音源单独写一套编辑界面。
    fn playlist_editor(&self) -> Option<&dyn PlaylistEditor> {
        None
    }
}

/// 可浏览目录：专辑 / 歌单 / 歌手。
#[async_trait]
pub trait CatalogSource: Send + Sync {
    async fn collection(&self, id: &CollectionId) -> CoreResult<CollectionPage>;

    async fn collection_tracks(
        &self,
        id: &CollectionId,
        offset: u32,
        limit: u32,
    ) -> CoreResult<TrackPage>;

    async fn search_collections(&self, query: &SearchQuery) -> CoreResult<Vec<CollectionSummary>>;
}

/// 账号会话。登录态由客户端持有（Cookie），服务端无状态。
#[async_trait]
pub trait AccountSource: Send + Sync {
    fn account_state(&self) -> AccountState;

    /// 用已保存的 Cookie 探测账号；无 Cookie 或失效返回 Ok(None)。
    async fn refresh_account(&self) -> CoreResult<Option<AccountProfile>>;

    async fn login_begin(&self, method: LoginMethod) -> CoreResult<LoginChallenge>;

    async fn login_poll(&self, challenge: &LoginChallenge) -> CoreResult<LoginProgress>;

    /// 发送短信验证码（手机号登录第一步）。
    async fn send_captcha(&self, phone: &str) -> CoreResult<()> {
        let _ = phone;
        Err(CoreError::Config("该源不支持短信验证码登录".into()))
    }

    /// 手机号 + 验证码直接登录（手机号登录第二步）。
    async fn login_cellphone(&self, phone: &str, captcha: &str) -> CoreResult<AccountProfile> {
        let _ = (phone, captcha);
        Err(CoreError::Config("该源不支持手机号登录".into()))
    }

    async fn logout(&self) -> CoreResult<()>;

    async fn user_playlists(&self) -> CoreResult<Vec<CollectionSummary>>;

    /// 今日推荐（每日推荐歌曲）。不支持的源返回 Err。
    async fn daily_recommendations(&self) -> CoreResult<Vec<Track>> {
        Err(CoreError::Config("该音源不支持今日推荐".into()))
    }

    /// 私人 FM。不支持的源返回 Err。
    async fn personal_fm(&self) -> CoreResult<Vec<Track>> {
        Err(CoreError::Config("该音源不支持私人 FM".into()))
    }

    /// 每日推荐歌单（账号个性化歌单列表）。
    async fn recommended_playlists(&self) -> CoreResult<Vec<CollectionSummary>> {
        Err(CoreError::Config("该音源不支持推荐歌单".into()))
    }

    /// 私人雷达：按账号每日生成的歌单集合。接口不支持时返回 Err，
    /// UI 以提示形式降级，不影响其它推荐区块。
    async fn private_radar(&self) -> CoreResult<Vec<CollectionSummary>> {
        Err(CoreError::Config("该音源不支持私人雷达".into()))
    }

    /// 当前 Cookie（用于 UI 持久化；不要写入日志）。
    fn cookie(&self) -> Option<String>;
}
