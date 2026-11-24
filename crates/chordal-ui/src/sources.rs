//! 音源实例装配：配置 → 适配器实例 + 注册表工厂。
//!
//! 同一适配器类型可以创建多个实例；实例 ID 同时写进 `SourceInfo.id`、
//! `TrackId.source` 与 `CollectionId.source`，因此重命名/新增实例不会
//! 让既有曲目或歌单引用串到别的源上。

use std::sync::Arc;

use chordal_cache::ObjectCache;
use chordal_core::error::{CoreError, CoreResult};
use chordal_core::model::SourceId;
use chordal_core::source::{MusicSource, SourceInfo};
use chordal_local::{LocalSource, LocalSourceConfig};
use chordal_netease::{NeteaseConfig, NeteaseSource};

use crate::app::SourceBuilder;
use crate::config::{SOURCE_TYPE_LOCAL, SOURCE_TYPE_NETEASE, SourceInstanceConfig};

pub struct BuiltSource {
    pub config: SourceInstanceConfig,
    pub info: SourceInfo,
    pub source: Arc<dyn MusicSource>,
    pub local: Option<Arc<LocalSource>>,
    pub netease: Option<Arc<NeteaseSource>>,
}

impl BuiltSource {
    pub fn builder(&self) -> SourceBuilder {
        let source = self.source.clone();
        Arc::new(move |_| Ok(source.clone()))
    }
}

/// 按配置构造适配器。构造本身不做网络/磁盘扫描；NetEase 的托管进程在
/// `warm_up` 时才启动，本地源的索引在宿主显式调用扫描时建立。
pub fn build(config: &SourceInstanceConfig, cache: Arc<ObjectCache>) -> CoreResult<BuiltSource> {
    let id = SourceId::new(config.id.clone());
    match config.source_type.as_str() {
        SOURCE_TYPE_LOCAL => {
            let local_config = config.local.clone().unwrap_or_else(|| LocalSourceConfig {
                roots: vec![dirs::audio_dir().unwrap_or_else(|| std::path::PathBuf::from("."))],
            });
            let local = LocalSource::new(id, local_config, config.name.clone());
            let mut info = local.info().clone();
            info.display_name = config.name.clone();
            info.priority = config.priority;
            let source: Arc<dyn MusicSource> = local.clone();
            Ok(BuiltSource {
                config: config.clone(),
                info,
                source,
                local: Some(local),
                netease: None,
            })
        }
        SOURCE_TYPE_NETEASE => {
            let netease_config = config
                .netease
                .clone()
                .unwrap_or_else(NeteaseConfig::default);
            let netease =
                NeteaseSource::new_with_cache(id, netease_config, config.name.clone(), Some(cache));
            let mut info = netease.info().clone();
            info.display_name = config.name.clone();
            info.priority = config.priority;
            let source: Arc<dyn MusicSource> = netease.clone();
            Ok(BuiltSource {
                config: config.clone(),
                info,
                source,
                local: None,
                netease: Some(netease),
            })
        }
        other => Err(CoreError::Config(format!("未知音源类型：{other}"))),
    }
}
