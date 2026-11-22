//! 进程级共享状态（gpui Global）。

use std::collections::HashMap;
use std::sync::Arc;

use chordal_core::error::CoreResult;
use chordal_core::model::SourceId;
use chordal_core::registry::SourceRegistry;
use chordal_core::source::{MusicSource, SourceInfo};
use chordal_player::Player;
use gpui::{App, Global};

use crate::app::SourceBuilder;

#[allow(dead_code)]
pub struct Globals {
    pub registry: Arc<SourceRegistry>,
    pub player: Arc<Player>,
    /// 预构建实例的工厂：加载 = 把同一实例挂回 registry（索引零丢失）。
    pub builders: HashMap<SourceId, SourceBuilder>,
}

impl Global for Globals {}

pub fn install(
    registry: Arc<SourceRegistry>,
    player: Arc<Player>,
    builders: HashMap<SourceId, SourceBuilder>,
    cx: &mut App,
) {
    cx.set_global(Globals {
        registry,
        player,
        builders,
    });
}

#[allow(dead_code)]
pub type DynSource = Arc<dyn MusicSource>;
#[allow(dead_code)]
pub type BuildResult = CoreResult<DynSource>;
#[allow(dead_code)]
pub type BuilderFn = dyn Fn(&SourceInfo) -> BuildResult + Send + Sync;
