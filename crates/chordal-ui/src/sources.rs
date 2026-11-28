//! 音源类型元数据与实例装配。
//!
//! 每个类型用一份描述符集中声明展示信息、默认配置、规范化、摘要与工厂。
//! 新增类型只需实现工厂并登记描述符，UI 与实例创建流程无需增加服务分支。

use std::sync::Arc;

use chordal_cache::ObjectCache;
use chordal_core::error::{CoreError, CoreResult};
use chordal_core::model::{SourceId, SourceKind};
use chordal_core::source::{MusicSource, SourceInfo};
use chordal_local::{LocalSource, LocalSourceConfig};
use chordal_netease::{NeteaseConfig, NeteaseSource};

use crate::app::SourceBuilder;
use crate::config::{SOURCE_TYPE_LOCAL, SOURCE_TYPE_NETEASE, SourceInstanceConfig};

pub struct BuiltSource {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceConfigEditor {
    LocalRoots,
    HttpEndpoint,
    Unsupported,
}

#[derive(Debug, Clone, Copy)]
pub struct SourceTypeDescriptor {
    pub source_type: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub kind: SourceKind,
    pub editor: SourceConfigEditor,
    pub default_priority: u8,
    pub create_config: fn(String, u8) -> SourceInstanceConfig,
    pub normalize: fn(&mut SourceInstanceConfig) -> bool,
    pub summary: fn(&SourceInstanceConfig) -> String,
    pub decode_session: Option<fn(&SourceInstanceConfig) -> Option<String>>,
    pub encode_session: Option<fn(&mut SourceInstanceConfig, Option<String>) -> bool>,
    pub build: fn(&SourceInstanceConfig, Arc<ObjectCache>) -> CoreResult<BuiltSource>,
}

pub const SOURCE_TYPE_DESCRIPTORS: &[SourceTypeDescriptor] = &[
    SourceTypeDescriptor {
        source_type: SOURCE_TYPE_LOCAL,
        display_name: "本地音乐",
        description: "扫描一个或多个音乐目录；支持增量扫描、封面/歌词边车文件。",
        kind: SourceKind::LocalLibrary,
        editor: SourceConfigEditor::LocalRoots,
        default_priority: 0,
        create_config: default_local_config,
        normalize: normalize_local_config,
        summary: local_summary,
        decode_session: None,
        encode_session: None,
        build: build_local,
    },
    SourceTypeDescriptor {
        source_type: SOURCE_TYPE_NETEASE,
        display_name: "网易云音乐",
        description: "登录账号后浏览歌单、推荐与私人电台。",
        kind: SourceKind::HttpApi,
        editor: SourceConfigEditor::HttpEndpoint,
        default_priority: 1,
        create_config: default_netease_config,
        normalize: normalize_netease_config,
        summary: netease_summary,
        decode_session: Some(decode_netease_session),
        encode_session: Some(encode_netease_session),
        build: build_netease,
    },
];

impl SourceInstanceConfig {
    pub fn type_label(&self) -> &'static str {
        source_type_descriptor(&self.source_type)
            .map(|descriptor| descriptor.display_name)
            .unwrap_or("未知音源")
    }

    pub fn category_label(&self) -> &'static str {
        match source_type_descriptor(&self.source_type).map(|descriptor| descriptor.kind) {
            Some(SourceKind::LocalLibrary) => "本地库",
            Some(SourceKind::HttpApi) => "在线服务",
            None => "扩展音源",
        }
    }

    /// 管理页的一行摘要（不含 Cookie 等敏感信息）。
    pub fn summary(&self) -> String {
        source_type_descriptor(&self.source_type)
            .map(|descriptor| (descriptor.summary)(self))
            .unwrap_or_else(|| self.source_type.clone())
    }

    pub fn session_cookie(&self) -> Option<String> {
        if let Some(decode) =
            source_type_descriptor(&self.source_type).and_then(|d| d.decode_session)
        {
            return decode(self);
        }
        generic_session_cookie(&self.session)
    }

    pub fn set_session_cookie(&mut self, cookie: Option<String>) -> bool {
        if let Some(encode) =
            source_type_descriptor(&self.source_type).and_then(|d| d.encode_session)
        {
            return encode(self, cookie);
        }
        set_generic_session_cookie(&mut self.session, cookie)
    }

    /// 补齐/修复类型专属配置，避免手改 JSON 后工厂拿到 None。
    pub fn normalize(&mut self) -> bool {
        let mut changed = false;
        if self.id.trim().is_empty() {
            self.id = self.source_type.clone();
            changed = true;
        }
        if self.name.trim().is_empty() {
            self.name = self.type_label().to_string();
            changed = true;
        }
        if let Some(descriptor) = source_type_descriptor(&self.source_type) {
            changed |= (descriptor.normalize)(self);
        }
        changed
    }
}

pub fn source_type_descriptor(source_type: &str) -> Option<&'static SourceTypeDescriptor> {
    SOURCE_TYPE_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.source_type == source_type)
}

pub fn create_source_config(source_type: &str, id: String) -> Option<SourceInstanceConfig> {
    let descriptor = source_type_descriptor(source_type)?;
    Some((descriptor.create_config)(id, descriptor.default_priority))
}

/// 构造本身不做网络/磁盘扫描；托管服务在 `warm_up` 时启动。
pub fn build(config: &SourceInstanceConfig, cache: Arc<ObjectCache>) -> CoreResult<BuiltSource> {
    let descriptor = source_type_descriptor(&config.source_type)
        .ok_or_else(|| CoreError::Config(format!("未知音源类型：{}", config.source_type)))?;
    (descriptor.build)(config, cache)
}

fn default_local_config(id: String, priority: u8) -> SourceInstanceConfig {
    let mut config =
        SourceInstanceConfig::local(id, "本地音乐", crate::config::default_music_roots());
    config.priority = priority;
    config
}

fn default_netease_config(id: String, priority: u8) -> SourceInstanceConfig {
    let mut config = SourceInstanceConfig::netease(id, "网易云音乐", NeteaseConfig::default());
    config.priority = priority;
    config
}

fn normalize_local_config(config: &mut SourceInstanceConfig) -> bool {
    if config.local.is_none() {
        config.local = Some(LocalSourceConfig {
            roots: crate::config::default_music_roots(),
        });
        true
    } else {
        false
    }
}

fn normalize_netease_config(config: &mut SourceInstanceConfig) -> bool {
    let mut changed = false;
    if config.netease.is_none() {
        config.netease = Some(NeteaseConfig::default());
        changed = true;
    }
    if let Some(cookie) = generic_session_cookie(&config.session) {
        let netease = config.netease.as_mut().expect("normalized above");
        if netease.cookie.as_deref() != Some(cookie.as_str()) {
            netease.cookie = Some(cookie);
            changed = true;
        }
    } else if let Some(cookie) = config
        .netease
        .as_ref()
        .and_then(|config| config.cookie.clone())
    {
        changed |= set_generic_session_cookie(&mut config.session, Some(cookie));
    }
    changed
}

fn generic_session_cookie(
    session: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    session
        .get("cookie")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn set_generic_session_cookie(
    session: &mut std::collections::BTreeMap<String, serde_json::Value>,
    cookie: Option<String>,
) -> bool {
    match cookie {
        Some(cookie) => {
            let value = serde_json::Value::String(cookie);
            if session.get("cookie") == Some(&value) {
                false
            } else {
                session.insert("cookie".into(), value);
                true
            }
        }
        None => session.remove("cookie").is_some(),
    }
}

fn decode_netease_session(config: &SourceInstanceConfig) -> Option<String> {
    generic_session_cookie(&config.session).or_else(|| {
        config
            .netease
            .as_ref()
            .and_then(|config| config.cookie.clone())
    })
}

fn encode_netease_session(config: &mut SourceInstanceConfig, cookie: Option<String>) -> bool {
    let typed_changed = config
        .netease
        .as_mut()
        .is_some_and(|netease| netease.cookie != cookie);
    if let Some(netease) = config.netease.as_mut() {
        netease.cookie = cookie.clone();
    }
    let session_changed = set_generic_session_cookie(&mut config.session, cookie);
    typed_changed || session_changed
}

fn local_summary(config: &SourceInstanceConfig) -> String {
    let roots = config
        .local
        .as_ref()
        .map(|config| config.roots.len())
        .unwrap_or(0);
    format!("本地库 · {roots} 个目录")
}

fn netease_summary(config: &SourceInstanceConfig) -> String {
    config
        .netease
        .as_ref()
        .map(|config| format!("在线服务 · {}", config.base_url))
        .unwrap_or_else(|| "在线服务".into())
}

fn build_local(config: &SourceInstanceConfig, _cache: Arc<ObjectCache>) -> CoreResult<BuiltSource> {
    let id = SourceId::new(config.id.clone());
    let local_config = config.local.clone().unwrap_or_else(|| LocalSourceConfig {
        roots: crate::config::default_music_roots(),
    });
    let local = LocalSource::new(id, local_config, config.name.clone());
    let mut info = local.info().clone();
    info.display_name = config.name.clone();
    info.priority = config.priority;
    let source: Arc<dyn MusicSource> = local.clone();
    Ok(BuiltSource {
        info,
        source,
        local: Some(local),
        netease: None,
    })
}

fn build_netease(
    config: &SourceInstanceConfig,
    cache: Arc<ObjectCache>,
) -> CoreResult<BuiltSource> {
    let id = SourceId::new(config.id.clone());
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
        info,
        source,
        local: None,
        netease: Some(netease),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_type_ids_are_unique_and_described() {
        let mut ids = std::collections::HashSet::new();
        for descriptor in SOURCE_TYPE_DESCRIPTORS {
            assert!(descriptor.source_type.len() > 0);
            assert!(descriptor.display_name.len() > 0);
            assert!(descriptor.description.len() > 0);
            assert!(ids.insert(descriptor.source_type));
        }
    }

    #[test]
    fn descriptors_create_their_own_configuration() {
        for descriptor in SOURCE_TYPE_DESCRIPTORS {
            let config = (descriptor.create_config)(
                format!("{}-test", descriptor.source_type),
                descriptor.default_priority,
            );
            assert_eq!(config.source_type, descriptor.source_type);
            assert_eq!(config.name, descriptor.display_name);
            assert_eq!(config.priority, descriptor.default_priority);
            assert!(!(descriptor.summary)(&config).is_empty());
        }
    }

    #[test]
    fn descriptors_normalize_missing_type_configuration() {
        for descriptor in SOURCE_TYPE_DESCRIPTORS {
            let mut config = SourceInstanceConfig {
                id: format!("{}-test", descriptor.source_type),
                source_type: descriptor.source_type.into(),
                name: descriptor.display_name.into(),
                enabled: true,
                priority: descriptor.default_priority,
                local: None,
                netease: None,
                settings: std::collections::BTreeMap::new(),
                session: std::collections::BTreeMap::new(),
            };
            (descriptor.normalize)(&mut config);
            assert!(!(descriptor.summary)(&config).is_empty());
        }
    }

    #[test]
    fn extension_settings_and_session_roundtrip() {
        let mut config =
            SourceInstanceConfig::netease("future-source", "未来音源", NeteaseConfig::default());
        config.source_type = "future".into();
        config
            .settings
            .insert("region".into(), serde_json::Value::String("cn".into()));
        config.session.insert(
            "token".into(),
            serde_json::Value::String("test-token".into()),
        );

        let bytes = serde_json::to_vec(&config).unwrap();
        let decoded: SourceInstanceConfig = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.settings, config.settings);
        assert_eq!(decoded.session, config.session);
    }

    #[test]
    fn legacy_cookie_migrates_to_instance_session() {
        let mut netease = NeteaseConfig::default();
        netease.cookie = Some("test-cookie".into());
        let mut config = SourceInstanceConfig::netease("netease", "网易云音乐", netease);

        assert!(config.normalize());
        assert_eq!(config.session_cookie().as_deref(), Some("test-cookie"));
        assert_eq!(
            config.session.get("cookie"),
            Some(&serde_json::Value::String("test-cookie".into()))
        );
        assert!(!config.normalize());

        assert!(config.set_session_cookie(None));
        assert_eq!(config.session_cookie(), None);
        assert!(config.netease.as_ref().unwrap().cookie.is_none());
        assert!(!config.set_session_cookie(None));
    }

    #[test]
    fn unknown_source_type_is_not_treated_as_local() {
        let mut unknown = SourceInstanceConfig {
            id: "unknown".into(),
            source_type: "unknown".into(),
            name: "未知".into(),
            enabled: true,
            priority: 0,
            local: None,
            netease: None,
            settings: std::collections::BTreeMap::new(),
            session: std::collections::BTreeMap::new(),
        };
        unknown.normalize();
        assert!(source_type_descriptor(&unknown.source_type).is_none());
        assert!(create_source_config(&unknown.source_type, unknown.id.clone()).is_none());
        assert!(unknown.local.is_none());
        assert!(unknown.netease.is_none());
        assert_eq!(unknown.type_label(), "未知音源");
        assert_eq!(unknown.category_label(), "扩展音源");
        assert_eq!(unknown.summary(), "unknown");
        assert!(unknown.set_session_cookie(Some("future-cookie".into())));
        assert_eq!(unknown.session_cookie().as_deref(), Some("future-cookie"));
    }
}
