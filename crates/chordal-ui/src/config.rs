//! 用户配置：`~/.config/chordial/config.json`。缺失时写默认值。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use chordal_local::LocalSourceConfig;
use chordal_netease::{NeteaseConfig, SongQualityLevel};

use crate::eq::SavedEqPreset;
use crate::theme::Appearance;

pub const SOURCE_TYPE_LOCAL: &str = "local";
pub const SOURCE_TYPE_NETEASE: &str = "netease";

/// 一个“音源实例”的持久化配置。
///
/// `id` 是实例级追踪 ID（写进 `TrackId.source` / `CollectionId.source`），
/// 因此同一类型可以创建多个实例而互不串数据；`source_type` 只决定用哪个
/// 适配器工厂构造。重命名实例只改 `name`，不会影响既有曲目/歌单引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInstanceConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub source_type: String,
    pub name: String,
    /// 关闭 = 保留配置但卸载实例；播放与歌单引用会进入“不可用/本地替代”路径。
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 聚合搜索/展示顺序；越小越靠前。
    #[serde(default)]
    pub priority: u8,
    /// 本地音乐适配器配置（`source_type == "local"`）。
    #[serde(default)]
    pub local: Option<LocalSourceConfig>,
    /// 网易云适配器配置（`source_type == "netease"`）。
    #[serde(default)]
    pub netease: Option<NeteaseConfig>,
}

fn default_enabled() -> bool {
    true
}

impl SourceInstanceConfig {
    pub fn local(id: impl Into<String>, name: impl Into<String>, roots: Vec<PathBuf>) -> Self {
        Self {
            id: id.into(),
            source_type: SOURCE_TYPE_LOCAL.into(),
            name: name.into(),
            enabled: true,
            priority: 0,
            local: Some(LocalSourceConfig { roots }),
            netease: None,
        }
    }

    pub fn netease(id: impl Into<String>, name: impl Into<String>, netease: NeteaseConfig) -> Self {
        Self {
            id: id.into(),
            source_type: SOURCE_TYPE_NETEASE.into(),
            name: name.into(),
            enabled: true,
            priority: 1,
            local: None,
            netease: Some(netease),
        }
    }

    pub fn is_local(&self) -> bool {
        self.source_type == SOURCE_TYPE_LOCAL
    }

    pub fn is_netease(&self) -> bool {
        self.source_type == SOURCE_TYPE_NETEASE
    }

    pub fn type_label(&self) -> &'static str {
        match self.source_type.as_str() {
            SOURCE_TYPE_LOCAL => "本地音乐",
            SOURCE_TYPE_NETEASE => "网易云音乐",
            _ => "未知音源",
        }
    }

    /// 管理页的一行摘要（不含 Cookie 等敏感信息）。
    pub fn summary(&self) -> String {
        if self.is_local() {
            let roots = self.local.as_ref().map(|c| c.roots.len()).unwrap_or(0);
            format!("本地库 · {roots} 个目录")
        } else if self.is_netease() {
            self.netease
                .as_ref()
                .map(|c| format!("api-enhanced · {}", c.base_url))
                .unwrap_or_else(|| "api-enhanced".into())
        } else {
            self.source_type.clone()
        }
    }

    /// 补齐/修复类型专属配置，避免手改 JSON 后工厂拿到 None。
    pub fn normalize(&mut self) {
        if self.id.trim().is_empty() {
            self.id = self.source_type.clone();
        }
        if self.name.trim().is_empty() {
            self.name = self.type_label().to_string();
        }
        if self.is_local() && self.local.is_none() {
            self.local = Some(LocalSourceConfig {
                roots: vec![default_music_dir()],
            });
        }
        if self.is_netease() && self.netease.is_none() {
            self.netease = Some(NeteaseConfig::default());
        }
    }
}

fn default_music_dir() -> PathBuf {
    dirs::audio_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// 为实例生成稳定且唯一的 ID：优先使用 `local` / `netease`，
/// 已占用则 `local-2` / `netease-2`…
pub fn next_source_id<'a>(base: &str, existing: impl IntoIterator<Item = &'a str>) -> String {
    let existing: std::collections::HashSet<&str> = existing.into_iter().collect();
    if !existing.contains(base) {
        return base.to_string();
    }
    let mut index = 2u32;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing.contains(candidate.as_str()) {
            return candidate;
        }
        index += 1;
    }
}

/// 旧配置只有一份 `local_roots` / `netease`；迁移成两个默认实例。
pub fn default_source_instances(
    local_roots: &[PathBuf],
    netease: &NeteaseConfig,
) -> Vec<SourceInstanceConfig> {
    vec![
        SourceInstanceConfig::local("local", "本地音乐", local_roots.to_vec()),
        SourceInstanceConfig::netease("netease", "网易云音乐", netease.clone()),
    ]
}

/// 本地/在线同曲的播放来源偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SourcePreference {
    /// 比较音质：本地更高才播本地。
    #[default]
    Auto,
    /// 只要本地有同曲就播本地。
    PreferLocal,
    /// 始终播在线（已下载文件除外）。
    PreferOnline,
}

impl SourcePreference {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "音质优先",
            Self::PreferLocal => "本地优先",
            Self::PreferOnline => "在线优先",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::PreferLocal,
            Self::PreferLocal => Self::PreferOnline,
            Self::PreferOnline => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 旧版单实例字段：仅作迁移来源/兼容保留；运行时以 [`AppConfig::sources`]
    /// 为准。编辑默认本地/网易云实例时同步写回，避免旧版本回退丢配置。
    pub local_roots: Vec<PathBuf>,
    pub netease: NeteaseConfig,
    /// 音源实例列表（可多个同类型实例）。旧配置缺失时由
    /// [`AppConfig::normalize_sources`] 从上面两个字段迁移生成。
    #[serde(default)]
    pub sources: Vec<SourceInstanceConfig>,
    pub player_device: Option<String>,
    /// 每源聚合搜索超时（ms）。
    pub search_timeout_ms: u64,
    /// 界面明暗；缺省浅色。
    #[serde(default)]
    pub appearance: Appearance,
    /// 对象缓存目录。
    #[serde(default = "default_cache_dir")]
    pub cache_dir: PathBuf,
    /// 对象缓存上限（字节），默认 5 GiB。
    #[serde(default = "default_cache_limit")]
    pub cache_limit_bytes: u64,
    /// 显式下载目录（不占对象缓存配额）。
    #[serde(default = "default_download_dir")]
    pub download_dir: PathBuf,
    /// 本地/在线同曲偏好。
    #[serde(default)]
    pub source_preference: SourcePreference,
    /// 平滑滚动（滚轮/触控板）。默认关闭，保留系统原生手感。
    #[serde(default)]
    pub smooth_scroll: bool,
    /// 动态歌词整行扫光的渐变带宽度（em，相对主字号）。
    /// 越大软边越宽、跨字/词越明显；默认 0.6。
    #[serde(default = "default_lyric_sweep_fade_em")]
    pub lyric_sweep_fade_em: f32,
    /// 背景鼓点监听频段下限（Hz）。
    #[serde(default = "default_beat_band_low_hz")]
    pub beat_band_low_hz: f32,
    /// 背景鼓点监听频段上限（Hz）。
    #[serde(default = "default_beat_band_high_hz")]
    pub beat_band_high_hz: f32,
    /// 背景鼓点视觉效果倍率：0 = 关闭，1.0 = 原始幅度，上限 1.5。
    /// 默认 0.5，保持可感知但不过分剧烈。
    #[serde(default = "default_beat_strength")]
    pub beat_strength: f32,
    /// 流体背景（Mesh Gradient）光栅分辨率除数：输出分辨率 = 显示区域 ÷ N。
    /// 越大越省性能、越模糊；默认 2（1/2 边长、1/4 像素量），更接近真实
    /// 场景的流体细节。光栅仍有 20 万像素硬上限，4K 下成本有界。
    #[serde(default = "default_background_render_divisor")]
    pub background_render_divisor: u32,
    /// 一次性迁移标记：旧配置的除数 4 是旧默认值，迁到新的 2 之后
    /// 用户若主动再选 4 不会被反复改写。
    #[serde(default)]
    pub bg_divisor_v2_migrated: bool,
    /// 画面性能诊断浮窗（FPS、1% Low、CPU、内存）。默认关闭。
    #[serde(default)]
    pub perf_overlay: bool,
    /// 参数均衡器（最后一次编辑的配置）。
    #[serde(default)]
    pub equalizer: chordal_player::EqSettings,
    /// 「另存为」得到的个人均衡器配置。
    #[serde(default)]
    pub eq_saved_presets: Vec<SavedEqPreset>,
}

pub fn default_cache_dir() -> PathBuf {
    chordal_cache::default_cache_root()
}

pub fn default_cache_limit() -> u64 {
    chordal_cache::DEFAULT_LIMIT_BYTES
}

pub fn default_download_dir() -> PathBuf {
    dirs::audio_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Chordial Downloads")
}

fn default_beat_band_low_hz() -> f32 {
    chordal_player::DEFAULT_BEAT_BAND.0
}

fn default_beat_band_high_hz() -> f32 {
    chordal_player::DEFAULT_BEAT_BAND.1
}

/// 背景鼓点视觉效果默认倍率（0..=1.5）。
pub const DEFAULT_BEAT_STRENGTH: f32 = 0.5;

/// 整行扫光渐变带默认宽度（em）：与 AMLL 的 `wordFadeWidth` 一致。
pub const DEFAULT_LYRIC_SWEEP_FADE_EM: f32 = 0.6;

/// 扫光宽度的合法范围（em）。过小会退化成硬边，过大则整行长期处于半亮。
pub const LYRIC_SWEEP_FADE_EM_MIN: f32 = 0.1;
pub const LYRIC_SWEEP_FADE_EM_MAX: f32 = 4.0;

fn default_lyric_sweep_fade_em() -> f32 {
    DEFAULT_LYRIC_SWEEP_FADE_EM
}

/// 把扫光宽度夹到可渲染范围；手改配置与设置输入框共用。
pub fn clamp_lyric_sweep_fade_em(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(LYRIC_SWEEP_FADE_EM_MIN, LYRIC_SWEEP_FADE_EM_MAX)
    } else {
        DEFAULT_LYRIC_SWEEP_FADE_EM
    }
}

fn default_beat_strength() -> f32 {
    DEFAULT_BEAT_STRENGTH
}

/// 流体背景光栅分辨率默认除数：输出 = 显示区域 ÷ 2（硬上限仍生效）。
pub const DEFAULT_BACKGROUND_RENDER_DIVISOR: u32 = 2;

/// 除数下限：1 = 按显示区域原始尺寸请求（渲染器还会按实测光栅耗时
/// 自适应收缩，并受 200k 像素硬上限约束，因此 ÷1 不会再拖垮帧率）。
pub const BACKGROUND_RENDER_DIVISOR_MIN: u32 = 1;

/// 除数上限：对应渲染器 `render_scale` 的下限 0.05。
pub const BACKGROUND_RENDER_DIVISOR_MAX: u32 = 20;

fn default_background_render_divisor() -> u32 {
    DEFAULT_BACKGROUND_RENDER_DIVISOR
}

/// 把分辨率除数夹到可渲染范围；手改配置与设置输入框共用。
pub fn clamp_background_render_divisor(value: u32) -> u32 {
    value.clamp(BACKGROUND_RENDER_DIVISOR_MIN, BACKGROUND_RENDER_DIVISOR_MAX)
}

impl AppConfig {
    /// 补齐音源实例：旧配置迁移、修复空 ID/重复 ID、补齐类型配置。
    /// 返回是否发生了修改（调用方据此决定是否落盘）。
    pub fn normalize_sources(&mut self) -> bool {
        let mut changed = false;
        if self.sources.is_empty() {
            self.sources = default_source_instances(&self.local_roots, &self.netease);
            changed = true;
        }
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for source in &mut self.sources {
            source.normalize();
            if !seen.insert(source.id.clone()) {
                let base = if source.is_local() {
                    SOURCE_TYPE_LOCAL
                } else if source.is_netease() {
                    SOURCE_TYPE_NETEASE
                } else {
                    "source"
                };
                source.id = next_source_id(base, seen.iter().map(String::as_str));
                seen.insert(source.id.clone());
                changed = true;
            }
        }
        // 默认实例同步回旧字段，保证旧设置页/旧版本仍读到同一份值。
        if let Some(local) = self
            .sources
            .iter()
            .find(|s| s.is_local())
            .and_then(|s| s.local.as_ref())
        {
            if self.local_roots != local.roots {
                self.local_roots = local.roots.clone();
                changed = true;
            }
        }
        if let Some(netease) = self
            .sources
            .iter()
            .find(|s| s.is_netease())
            .and_then(|s| s.netease.as_ref())
            && self.netease != *netease
        {
            self.netease = netease.clone();
            changed = true;
        }
        changed
    }

    pub fn source(&self, id: &str) -> Option<&SourceInstanceConfig> {
        self.sources.iter().find(|s| s.id == id)
    }

    pub fn source_mut(&mut self, id: &str) -> Option<&mut SourceInstanceConfig> {
        self.sources.iter_mut().find(|s| s.id == id)
    }

    /// 管理页新增实例时用的唯一 ID。
    pub fn new_source_id(&self, source_type: &str) -> String {
        next_source_id(source_type, self.sources.iter().map(|s| s.id.as_str()))
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        let music = dirs::audio_dir().unwrap_or_else(|| PathBuf::from("."));
        let local_roots = vec![music];
        let netease = NeteaseConfig::default();
        Self {
            sources: default_source_instances(&local_roots, &netease),
            local_roots,
            netease,
            player_device: None,
            search_timeout_ms: 6_000,
            appearance: Appearance::Light,
            cache_dir: default_cache_dir(),
            cache_limit_bytes: default_cache_limit(),
            download_dir: default_download_dir(),
            source_preference: SourcePreference::Auto,
            smooth_scroll: false,
            lyric_sweep_fade_em: DEFAULT_LYRIC_SWEEP_FADE_EM,
            beat_band_low_hz: chordal_player::DEFAULT_BEAT_BAND.0,
            beat_band_high_hz: chordal_player::DEFAULT_BEAT_BAND.1,
            beat_strength: DEFAULT_BEAT_STRENGTH,
            background_render_divisor: DEFAULT_BACKGROUND_RENDER_DIVISOR,
            // 新配置直接用新默认值，无需迁移。
            bg_divisor_v2_migrated: true,
            perf_overlay: false,
            equalizer: chordal_player::EqSettings::default(),
            eq_saved_presets: Vec::new(),
        }
    }
}

pub fn config_path() -> PathBuf {
    // 便携/测试覆盖：`CHORDIAL_CONFIG` 指向完整文件，`CHORDIAL_CONFIG_DIR`
    // 指向目录（其下 config.json）。不设置时使用系统配置目录。
    if let Some(path) = std::env::var_os("CHORDIAL_CONFIG") {
        return PathBuf::from(path);
    }
    if let Some(dir) = std::env::var_os("CHORDIAL_CONFIG_DIR") {
        return PathBuf::from(dir).join("config.json");
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chordial")
        .join("config.json")
}

pub fn load_or_default() -> AppConfig {
    let path = config_path();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        match serde_json::from_str::<AppConfig>(&raw) {
            Ok(mut cfg) => {
                let mut dirty = false;
                // 手改配置时也保证倍率落在可渲染范围。
                cfg.beat_strength = cfg.beat_strength.clamp(0.0, 1.5);
                cfg.lyric_sweep_fade_em = clamp_lyric_sweep_fade_em(cfg.lyric_sweep_fade_em);
                cfg.background_render_divisor =
                    clamp_background_render_divisor(cfg.background_render_divisor);
                // 旧默认除数 4 一次性迁到 2；用户之后主动设回 4 不再被改写。
                if !cfg.bg_divisor_v2_migrated && cfg.background_render_divisor == 4 {
                    cfg.background_render_divisor = 2;
                    cfg.bg_divisor_v2_migrated = true;
                    dirty = true;
                }
                // 旧配置没有 sources：迁移成 local/netease 两个实例并落盘。
                if cfg.normalize_sources() {
                    dirty = true;
                }
                if dirty {
                    save(&cfg);
                }
                return cfg;
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "config parse failed; using defaults");
            }
        }
    }
    let cfg = AppConfig::default();
    save(&cfg);
    cfg
}

pub fn save(cfg: &AppConfig) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(&path, json);
    }
}

// 保持 level 枚举在配置里的字符串稳定性（serde 默认支持）。
#[allow(dead_code)]
fn _level_roundtrip() {
    let _ = serde_json::to_string(&SongQualityLevel::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_scroll_defaults_off() {
        assert!(!AppConfig::default().smooth_scroll);
    }

    #[test]
    fn lyric_sweep_fade_defaults_to_amll_width() {
        assert!((AppConfig::default().lyric_sweep_fade_em - 0.6).abs() < 1e-6);
    }

    #[test]
    fn lyric_sweep_fade_is_clamped() {
        assert_eq!(clamp_lyric_sweep_fade_em(0.0), LYRIC_SWEEP_FADE_EM_MIN);
        assert_eq!(clamp_lyric_sweep_fade_em(9.0), LYRIC_SWEEP_FADE_EM_MAX);
        assert_eq!(
            clamp_lyric_sweep_fade_em(f32::NAN),
            DEFAULT_LYRIC_SWEEP_FADE_EM
        );
    }

    #[test]
    fn background_render_divisor_defaults_to_half_resolution() {
        assert_eq!(
            AppConfig::default().background_render_divisor,
            DEFAULT_BACKGROUND_RENDER_DIVISOR
        );
    }

    #[test]
    fn background_render_divisor_is_clamped() {
        assert_eq!(clamp_background_render_divisor(0), 1);
        assert_eq!(clamp_background_render_divisor(3), 3);
        assert_eq!(clamp_background_render_divisor(99), 20);
    }

    #[test]
    fn perf_overlay_defaults_off() {
        assert!(!AppConfig::default().perf_overlay);
    }

    #[test]
    fn perf_overlay_roundtrips() {
        let mut cfg = AppConfig::default();
        cfg.perf_overlay = true;
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.perf_overlay);
    }

    #[test]
    fn smooth_scroll_roundtrips() {
        let mut cfg = AppConfig::default();
        cfg.smooth_scroll = true;
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.smooth_scroll);
    }

    #[test]
    fn old_config_without_smooth_scroll_defaults_off() {
        let json = r#"{
            "local_roots": [],
            "netease": {
                "base_url": "http://127.0.0.1:3000",
                "cookie": null,
                "level": null
            },
            "player_device": null,
            "search_timeout_ms": 6000
        }"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert!(!parsed.smooth_scroll);
        assert!(!parsed.perf_overlay);
        assert!((parsed.lyric_sweep_fade_em - 0.6).abs() < 1e-6);
        assert_eq!(
            parsed.background_render_divisor,
            DEFAULT_BACKGROUND_RENDER_DIVISOR
        );
    }

    #[test]
    fn legacy_config_migrates_to_source_instances() {
        let json = r#"{
            "local_roots": ["/music"],
            "netease": {
                "base_url": "http://127.0.0.1:3001",
                "cookie": null,
                "level": null
            },
            "player_device": null,
            "search_timeout_ms": 6000
        }"#;
        let mut cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.sources.is_empty());
        assert!(cfg.normalize_sources());
        assert_eq!(cfg.sources.len(), 2);
        let local = cfg.sources.iter().find(|s| s.is_local()).unwrap();
        assert_eq!(local.id, "local");
        assert_eq!(
            local.local.as_ref().unwrap().roots,
            vec![PathBuf::from("/music")]
        );
        let netease = cfg.sources.iter().find(|s| s.is_netease()).unwrap();
        assert_eq!(netease.id, "netease");
        assert_eq!(
            netease.netease.as_ref().unwrap().base_url,
            "http://127.0.0.1:3001"
        );
    }

    #[test]
    fn source_ids_stay_unique_for_multiple_instances() {
        assert_eq!(next_source_id("local", ["netease"]), "local");
        assert_eq!(next_source_id("local", ["local"]), "local-2");
        assert_eq!(next_source_id("local", ["local", "local-2"]), "local-3");

        let mut cfg = AppConfig::default();
        let id = cfg.new_source_id(SOURCE_TYPE_LOCAL);
        cfg.sources
            .push(SourceInstanceConfig::local(id, "本地音乐 2", vec![]));
        let json = serde_json::to_string(&cfg).unwrap();
        let mut parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(!parsed.normalize_sources());
        assert_eq!(parsed.sources.iter().filter(|s| s.is_local()).count(), 2);
        let ids: std::collections::HashSet<_> =
            parsed.sources.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids.len(), parsed.sources.len());
    }
}
