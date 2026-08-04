//! 批量下载任务注册表与载荷类型。
//!
//! 全局下载任务注册表管理并发下载任务的取消标志；同时定义批量下载的
//! 请求项、去重策略与进度事件载荷类型。

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

/// 全局下载任务注册表 — task_id → 取消标志。
///
/// 用于支持前端取消进行中的批量下载任务。后台线程定期检查标志，
/// 若为 true 则提前终止剩余下载。
static DOWNLOAD_TASKS: LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 注册一个新下载任务，返回其取消标志。
pub fn register_download_task(task_id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    DOWNLOAD_TASKS
        .lock()
        .unwrap()
        .insert(task_id.to_string(), flag.clone());
    flag
}

/// 取消指定下载任务。返回 true 表示找到并取消，false 表示任务不存在或已完成。
pub fn cancel_download_task(task_id: &str) -> bool {
    if let Some(flag) = DOWNLOAD_TASKS.lock().unwrap().get(task_id) {
        flag.store(true, Ordering::SeqCst);
        return true;
    }
    false
}

/// 移除已完成的下载任务（后台线程结束时调用）。
pub fn unregister_download_task(task_id: &str) {
    DOWNLOAD_TASKS.lock().unwrap().remove(task_id);
}

/// 批量下载请求项（前端传入）。
#[derive(Deserialize, Clone)]
pub struct BatchDownloadItem {
    /// 网易云 entity_id（如 `"song:123456"`）
    pub song_entity_id: String,
    /// 歌曲标题（用于文件命名）
    pub title: String,
    /// 主艺术家名（用于文件命名）
    pub artist: String,
    /// 音质等级（"standard"/"higher"/"exhigh"/"lossless"/"hires"），None 默认 exhigh
    #[serde(default)]
    pub level: Option<String>,
}

/// 批量下载去重策略（前端传入）。
///
/// - `none`: 不去重，直接下载（默认）
/// - `skip_if_equal`: 本地与目标音质相同时跳过；本地更高也跳过
/// - `replace_if_better`: 本地音质低于在线时下载新版本并删除本地旧文件；
///   本地音质 ≥ 在线时跳过
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DedupStrategy {
    /// 不去重（默认）
    None,
    /// 本地已有相同或更高音质 → 跳过
    SkipIfEqual,
    /// 本地音质 < 在线 → 下载并替换（删除旧文件）
    /// 本地音质 ≥ 在线 → 跳过
    ReplaceIfBetter,
}

impl Default for DedupStrategy {
    fn default() -> Self {
        Self::None
    }
}

/// 批量下载进度事件 payload。
#[derive(serde::Serialize, Clone)]
pub struct DownloadProgress {
    /// 任务 ID（与启动时返回的一致）
    pub task_id: String,
    /// 当前完成的索引（0-based）
    pub index: usize,
    /// 总曲目数
    pub total: usize,
    /// 状态：completed / failed / cancelled / skipped / replaced
    pub status: &'static str,
    /// 当前曲目标题（前端展示用）
    pub title: String,
    /// 当前曲目艺术家
    pub artist: String,
    /// 成功时的文件路径
    pub path: Option<String>,
    /// 失败时的错误信息
    pub error: Option<String>,
    /// 已成功下载数（含 replaced）
    pub succeeded: usize,
    /// 已失败数
    pub failed: usize,
    /// 已跳过数（含 SkipIfEqual 与 ReplaceIfBetter 中的"本地已更好"场景）
    pub skipped: usize,
    /// 实际下载到的音质等级（如 "exhigh"），降级时可能与目标不同
    pub actual_level: Option<String>,
    /// 单曲处理动作描述（前端展示用）：downloaded / skipped / replaced
    pub action: Option<&'static str>,
}