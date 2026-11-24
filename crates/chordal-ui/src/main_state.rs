//! 进程级共享状态（gpui Global）。

use std::collections::HashMap;
use std::sync::Arc;

use chordal_core::error::CoreResult;
use chordal_core::model::SourceId;
use chordal_core::registry::SourceRegistry;
use chordal_core::source::{MusicSource, SourceInfo};
use chordal_local::LocalSource;
use chordal_netease::NeteaseSource;
use chordal_player::Player;
use gpui::{App, Global};

use crate::app::SourceBuilder;

#[allow(dead_code)]
pub struct Globals {
    pub registry: Arc<SourceRegistry>,
    pub player: Arc<Player>,
    /// 预构建实例的工厂：加载 = 把同一实例挂回 registry（索引零丢失）。
    /// 动态新增/删除音源实例时同步增删。
    pub builders: HashMap<SourceId, SourceBuilder>,
    /// 具体类型句柄：本地源需要触发扫描/读统计，NetEase 需要账号与推荐接口。
    pub local_sources: HashMap<SourceId, Arc<LocalSource>>,
    pub netease_sources: HashMap<SourceId, Arc<NeteaseSource>>,
}

impl Global for Globals {}

pub fn install(
    registry: Arc<SourceRegistry>,
    player: Arc<Player>,
    builders: HashMap<SourceId, SourceBuilder>,
    local_sources: HashMap<SourceId, Arc<LocalSource>>,
    netease_sources: HashMap<SourceId, Arc<NeteaseSource>>,
    cx: &mut App,
) {
    cx.set_global(Globals {
        registry,
        player,
        builders,
        local_sources,
        netease_sources,
    });
}

#[allow(dead_code)]
pub type DynSource = Arc<dyn MusicSource>;
#[allow(dead_code)]
pub type BuildResult = CoreResult<DynSource>;
#[allow(dead_code)]
pub type BuilderFn = dyn Fn(&SourceInfo) -> BuildResult + Send + Sync;
