//! 用户配置：`~/.config/chordial/config.json`。缺失时写默认值。

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use chordal_core::{AudioBandKind, AudioBandRanges, AudioControlMapping, FrequencyRange};
use chordal_local::LocalSourceConfig;
use chordal_netease::{NeteaseConfig, SongQualityLevel};

use crate::eq::{EqRecipe, SavedEqPreset};
use crate::theme::Appearance;

pub const SOURCE_TYPE_LOCAL: &str = "local";
pub const SOURCE_TYPE_NETEASE: &str = "netease";

/// 一个“音源实例”的持久化配置。
///
/// `id` 是实例级追踪 ID（写进 `TrackId.source` / `CollectionId.source`），
/// 因此同一类型可以创建多个实例而互不串数据；`source_type` 只决定用哪个
/// 适配器工厂构造。重命名实例只改 `name`，不会影响既有曲目/歌单引用。
#[derive(Clone, Serialize, Deserialize)]
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
    /// 类型描述符可解释的扩展配置；新音源无需再增加顶层字段。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Value>,
    /// 实例级账号会话。敏感值只持久化，不进入日志或摘要。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub session: BTreeMap<String, Value>,
}

impl fmt::Debug for SourceInstanceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceInstanceConfig")
            .field("id", &self.id)
            .field("source_type", &self.source_type)
            .field("name", &self.name)
            .field("enabled", &self.enabled)
            .field("priority", &self.priority)
            .field("local", &self.local)
            .field("netease", &self.netease.as_ref().map(|_| "<configured>"))
            .field("settings_keys", &self.settings.len())
            .field("session_keys", &self.session.len())
            .finish()
    }
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
            settings: BTreeMap::new(),
            session: BTreeMap::new(),
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
            settings: BTreeMap::new(),
            session: BTreeMap::new(),
        }
    }

    pub fn is_local(&self) -> bool {
        self.source_type == SOURCE_TYPE_LOCAL
    }

    pub fn is_netease(&self) -> bool {
        self.source_type == SOURCE_TYPE_NETEASE
    }
}

/// 为实例生成稳定且唯一的 ID：优先使用类型名，已占用则追加 `-2`、`-3`…
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
    #[serde(default)]
    pub local_roots: Vec<PathBuf>,
    #[serde(default)]
    pub netease: NeteaseConfig,
    /// 音源实例列表（可多个同类型实例）。旧配置缺失时由
    /// [`AppConfig::normalize_sources`] 从上面两个字段迁移生成。
    #[serde(default)]
    pub sources: Vec<SourceInstanceConfig>,
    #[serde(default)]
    pub player_device: Option<String>,
    /// 每源聚合搜索超时（ms）。
    #[serde(default = "default_search_timeout_ms")]
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
    /// 旧版单鼓点频段下限，仅用于迁移与旧版本兼容；运行时以
    /// [`AppConfig::audio_band_ranges`] 为准。
    #[serde(default = "default_beat_band_low_hz")]
    pub beat_band_low_hz: f32,
    /// 旧版单鼓点频段上限，仅用于迁移与旧版本兼容。
    #[serde(default = "default_beat_band_high_hz")]
    pub beat_band_high_hz: f32,
    /// 五路频段的独立监听范围；缺失时由旧单鼓点字段迁移。
    #[serde(default)]
    pub audio_band_ranges: AudioBandRanges,
    /// 五路频段分别控制哪些 1–25 编号的内部点。
    #[serde(default)]
    pub audio_band_controls: AudioControlMapping,
    /// 全部音频反应背景的视觉倍率：0 = 关闭，1.0 = 原始幅度，上限 1.5。
    /// 默认 0.5，保持可感知但不过分剧烈。
    #[serde(default = "default_beat_strength")]
    pub beat_strength: f32,
    /// 统一弹簧预设：决定基础角频率与阻尼比；缺省为均衡。
    #[serde(default)]
    pub motion_preset: crate::motion::MotionPreset,
    /// 动画时长倍率：> 1 慢放、< 1 加速；默认 1.0，范围 0.25..=3.0。
    #[serde(default = "default_animation_duration")]
    pub animation_duration_multiplier: f32,
    /// 动画回弹倍率：> 1 回弹更明显、< 1 更稳；默认 1.0，范围 0.25..=2.0。
    #[serde(default = "default_animation_overshoot")]
    pub animation_overshoot_multiplier: f32,
    /// 流体背景（Mesh Gradient）光栅分辨率除数：输出分辨率 = 显示区域设备像素 ÷ N。
    /// 越大越省性能、越模糊；默认 2（1/2 边长、1/4 像素量），更接近真实
    /// 场景的流体细节。渲染器严格按该除数计算光栅尺寸，不额外收缩分辨率。
    #[serde(default = "default_background_render_divisor")]
    pub background_render_divisor: u32,
    /// 一次性迁移标记：旧配置的除数 4 是旧默认值，迁到新的 2 之后
    /// 用户若主动再选 4 不会被反复改写。
    #[serde(default)]
    pub bg_divisor_v2_migrated: bool,
    /// 一次性迁移标记：旧的 4000–10000 Hz 默认频段迁到 35–160 Hz 底鼓。
    #[serde(default)]
    pub beat_band_v2_migrated: bool,
    /// 旧单鼓点配置迁移到五路独立频段的一次性标记。
    #[serde(default)]
    pub audio_bands_v3_migrated: bool,
    /// 画面性能诊断浮窗（FPS、1% Low、CPU、内存）。默认关闭。
    #[serde(default)]
    pub perf_overlay: bool,
    /// 参数均衡器（最后一次编辑的配置，合成后的扁平快照）。
    #[serde(default)]
    pub equalizer: chordal_player::EqSettings,
    /// 参数均衡器链路（耳机档案 / 内置预设 / 自定义频段）。
    /// 旧配置缺失时由 `equalizer.bands` 迁移为自定义频段。
    #[serde(default)]
    pub eq_recipe: Option<EqRecipe>,
    /// 「另存为」得到的个人均衡器配置。
    #[serde(default)]
    pub eq_saved_presets: Vec<SavedEqPreset>,
}

pub fn default_cache_dir() -> PathBuf {
    chordal_cache::default_cache_root()
}

fn default_search_timeout_ms() -> u64 {
    6_000
}

pub fn default_cache_limit() -> u64 {
    chordal_cache::DEFAULT_LIMIT_BYTES
}

pub fn default_download_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CHORDIAL_DOWNLOAD_DIR") {
        return PathBuf::from(dir);
    }
    dirs::audio_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Chordial Downloads")
}

/// 本地音乐库默认扫描根目录。
///
/// Android 上没有可用的 `dirs::audio_dir()`，若沿用 `"."` 回退会把工作目录
/// （`/`）当音乐库递归扫描。因此 Android 侧使用两个目录：
/// 1. 设备共享的 `/storage/emulated/0/Music`（需要 READ_MEDIA_AUDIO 授权，
///    与桌面端默认的“音乐”目录对应）；
/// 2. JNI 注入的 `CHORDIAL_MUSIC_DIR`（应用私有目录，adb/下载落盘用）。
///
/// 目录不可读时 walkdir 会跳过（`filter_map(|e| e.ok())`），不会阻塞启动。
pub fn default_music_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let mut roots = Vec::new();
        let shared = PathBuf::from("/storage/emulated/0/Music");
        if shared.is_dir() {
            roots.push(shared);
        }
        if let Some(dir) = std::env::var_os("CHORDIAL_MUSIC_DIR") {
            roots.push(PathBuf::from(dir));
        }
        roots
    }
    #[cfg(not(target_os = "android"))]
    {
        if let Some(dir) = std::env::var_os("CHORDIAL_MUSIC_DIR") {
            return vec![PathBuf::from(dir)];
        }
        vec![dirs::audio_dir().unwrap_or_else(|| PathBuf::from("."))]
    }
}

fn default_beat_band_low_hz() -> f32 {
    chordal_player::DEFAULT_BEAT_BAND.0
}

fn default_beat_band_high_hz() -> f32 {
    chordal_player::DEFAULT_BEAT_BAND.1
}

/// 全部音频反应背景的视觉默认倍率（0..=1.5）。
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

/// 动画时长倍率默认值与合法范围（0.25–3.0）。
pub const DEFAULT_ANIMATION_DURATION_MULTIPLIER: f32 = 1.0;
pub const ANIMATION_DURATION_MULTIPLIER_MIN: f32 = 0.25;
pub const ANIMATION_DURATION_MULTIPLIER_MAX: f32 = 3.0;

/// 动画回弹倍率默认值与合法范围（0.25–2.0）。
pub const DEFAULT_ANIMATION_OVERSHOOT_MULTIPLIER: f32 = 1.0;
pub const ANIMATION_OVERSHOOT_MULTIPLIER_MIN: f32 = 0.25;
pub const ANIMATION_OVERSHOOT_MULTIPLIER_MAX: f32 = 2.0;

fn default_animation_duration() -> f32 {
    DEFAULT_ANIMATION_DURATION_MULTIPLIER
}

fn default_animation_overshoot() -> f32 {
    DEFAULT_ANIMATION_OVERSHOOT_MULTIPLIER
}

/// 把时长倍率夹到可渲染范围；手改配置与设置输入框共用。
pub fn clamp_animation_duration_multiplier(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(
            ANIMATION_DURATION_MULTIPLIER_MIN,
            ANIMATION_DURATION_MULTIPLIER_MAX,
        )
    } else {
        DEFAULT_ANIMATION_DURATION_MULTIPLIER
    }
}

/// 把回弹倍率夹到可渲染范围；手改配置与设置输入框共用。
pub fn clamp_animation_overshoot_multiplier(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(
            ANIMATION_OVERSHOOT_MULTIPLIER_MIN,
            ANIMATION_OVERSHOOT_MULTIPLIER_MAX,
        )
    } else {
        DEFAULT_ANIMATION_OVERSHOOT_MULTIPLIER
    }
}

/// 流体背景光栅分辨率默认除数：输出 = 显示区域 ÷ 2。
pub const DEFAULT_BACKGROUND_RENDER_DIVISOR: u32 = 2;

/// 除数下限：1 = 按显示区域原始尺寸光栅化。
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
            changed |= source.normalize();
            if !seen.insert(source.id.clone()) {
                let base = source.source_type.as_str();
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
        let local_roots = default_music_roots();
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
            audio_band_ranges: AudioBandRanges::default(),
            audio_band_controls: AudioControlMapping::default(),
            beat_strength: DEFAULT_BEAT_STRENGTH,
            motion_preset: crate::motion::MotionPreset::default(),
            animation_duration_multiplier: DEFAULT_ANIMATION_DURATION_MULTIPLIER,
            animation_overshoot_multiplier: DEFAULT_ANIMATION_OVERSHOOT_MULTIPLIER,
            background_render_divisor: DEFAULT_BACKGROUND_RENDER_DIVISOR,
            // 新配置直接用新默认值，无需迁移。
            bg_divisor_v2_migrated: true,
            beat_band_v2_migrated: true,
            audio_bands_v3_migrated: true,
            perf_overlay: false,
            equalizer: chordal_player::EqSettings::default(),
            eq_recipe: None,
            eq_saved_presets: Vec::new(),
        }
    }
}

impl AppConfig {
    /// 旧版默认鼓点频段是镲片/军鼓区；升级后只在仍等于旧默认时迁移，
    /// 用户自定义频段保持不变。返回是否需要落盘。
    fn migrate_beat_band(&mut self) -> bool {
        if self.beat_band_v2_migrated {
            return false;
        }
        self.beat_band_v2_migrated = true;
        if self.beat_band_low_hz == 4_000.0 && self.beat_band_high_hz == 10_000.0 {
            self.beat_band_low_hz = chordal_player::DEFAULT_BEAT_BAND.0;
            self.beat_band_high_hz = chordal_player::DEFAULT_BEAT_BAND.1;
        }
        true
    }
}

impl AppConfig {
    /// 旧版只有单鼓点频段；迁移到五路配置时把它复制到 Kick 通道，其余
    /// 四路使用默认范围。已有非默认五路配置不会被覆盖。
    fn migrate_audio_bands(&mut self) -> bool {
        let old_ranges = self.audio_band_ranges;
        let old_controls = self.audio_band_controls;
        self.audio_band_ranges = self.audio_band_ranges.sanitized();
        self.audio_band_controls = self.audio_band_controls.sanitized();
        let mut dirty =
            old_ranges != self.audio_band_ranges || old_controls != self.audio_band_controls;
        if !self.audio_bands_v3_migrated {
            if self.audio_band_ranges == AudioBandRanges::default() {
                self.audio_band_ranges.set(
                    AudioBandKind::Kick,
                    FrequencyRange {
                        low_hz: self.beat_band_low_hz,
                        high_hz: self.beat_band_high_hz,
                    },
                );
            }
            self.audio_bands_v3_migrated = true;
            dirty = true;
        }
        dirty
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
                // 动画倍率越界或非有限时回落到默认值，并落盘修正，避免下次启动重复纠正。
                let duration =
                    clamp_animation_duration_multiplier(cfg.animation_duration_multiplier);
                if !cfg.animation_duration_multiplier.is_finite()
                    || cfg.animation_duration_multiplier != duration
                {
                    cfg.animation_duration_multiplier = duration;
                    dirty = true;
                }
                let overshoot =
                    clamp_animation_overshoot_multiplier(cfg.animation_overshoot_multiplier);
                if !cfg.animation_overshoot_multiplier.is_finite()
                    || cfg.animation_overshoot_multiplier != overshoot
                {
                    cfg.animation_overshoot_multiplier = overshoot;
                    dirty = true;
                }
                cfg.background_render_divisor =
                    clamp_background_render_divisor(cfg.background_render_divisor);
                // 旧默认除数 4 一次性迁到 2；用户之后主动设回 4 不再被改写。
                if !cfg.bg_divisor_v2_migrated && cfg.background_render_divisor == 4 {
                    cfg.background_render_divisor = 2;
                    cfg.bg_divisor_v2_migrated = true;
                    dirty = true;
                }
                if cfg.migrate_beat_band() {
                    dirty = true;
                }
                if cfg.migrate_audio_bands() {
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
    if let Ok(json) = serde_json::to_vec_pretty(cfg)
        && let Err(error) = chordal_core::fs::atomic_write(&path, &json)
    {
        tracing::warn!(path = %path.display(), %error, "config save failed");
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
    fn eq_recipe_roundtrips_and_legacy_config_defaults_none() {
        let mut cfg = AppConfig::default();
        cfg.eq_recipe = Some(crate::eq::EqRecipe {
            profile: Some(crate::eq::EqProfile {
                name: "Test Profile".into(),
                source: "AutoEQ".into(),
                settings: chordal_player::EqSettings::with_bands([chordal_player::EqBand::new(
                    1_000.0, -1.0, 1.0,
                )]),
            }),
            blocks: vec![crate::eq::PresetBlock::new("air_detail")],
            custom: vec![chordal_player::EqBand::new(120.0, 2.0, 1.0)],
            input_gain_db: -2.0,
            output_gain_db: 0.0,
        });
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        let recipe = parsed.eq_recipe.expect("recipe must round-trip");
        assert_eq!(
            recipe.profile.as_ref().map(|profile| profile.name.as_str()),
            Some("Test Profile")
        );
        assert_eq!(recipe.blocks.len(), 1);
        assert_eq!(recipe.blocks[0].preset_id, "air_detail");
        assert_eq!(recipe.custom.len(), 1);

        // 旧版本配置没有 `eq_recipe` 字段，应回退到扁平 `equalizer` 迁移。
        let legacy: AppConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(legacy.eq_recipe.is_none());
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
    fn legacy_default_beat_band_migrates_once() {
        let mut cfg: AppConfig =
            serde_json::from_str(r#"{"beat_band_low_hz":4000.0,"beat_band_high_hz":10000.0}"#)
                .unwrap();
        assert!(cfg.migrate_beat_band());
        assert_eq!(cfg.beat_band_low_hz, 35.0);
        assert_eq!(cfg.beat_band_high_hz, 160.0);
        assert!(cfg.beat_band_v2_migrated);
        assert!(!cfg.migrate_beat_band());
    }

    #[test]
    fn custom_beat_band_survives_migration() {
        let mut cfg: AppConfig =
            serde_json::from_str(r#"{"beat_band_low_hz":150.0,"beat_band_high_hz":2000.0}"#)
                .unwrap();
        assert!(cfg.migrate_beat_band());
        assert_eq!(cfg.beat_band_low_hz, 150.0);
        assert_eq!(cfg.beat_band_high_hz, 2_000.0);
    }

    #[test]
    fn source_debug_output_redacts_session_values() {
        let mut netease = NeteaseConfig::default();
        netease.cookie = Some("secret-cookie".into());
        let mut source = SourceInstanceConfig::netease("netease", "网易云音乐", netease);
        source.settings.insert(
            "token".into(),
            serde_json::Value::String("secret-setting".into()),
        );
        source.session.insert(
            "cookie".into(),
            serde_json::Value::String("secret-session".into()),
        );

        let debug = format!("{source:?}");
        assert!(!debug.contains("secret-cookie"));
        assert!(!debug.contains("secret-setting"));
        assert!(!debug.contains("secret-session"));
        assert!(debug.contains("session_keys"));
    }

    #[test]
    fn legacy_single_band_migrates_into_five_ranges() {
        let mut cfg: AppConfig =
            serde_json::from_str(r#"{"beat_band_low_hz":150.0,"beat_band_high_hz":2000.0}"#)
                .unwrap();
        cfg.migrate_beat_band();
        assert!(cfg.migrate_audio_bands());
        assert_eq!(cfg.audio_band_ranges.get(AudioBandKind::Kick).low_hz, 150.0);
        assert_eq!(
            cfg.audio_band_ranges.get(AudioBandKind::Kick).high_hz,
            2_000.0
        );
        assert_eq!(
            cfg.audio_band_ranges.get(AudioBandKind::Vocals),
            AudioBandKind::Vocals.default_range()
        );
        assert!(!cfg.migrate_audio_bands());
    }

    #[test]
    fn existing_custom_five_band_config_survives_migration() {
        let mut cfg: AppConfig =
            serde_json::from_str(r#"{"beat_band_low_hz":150.0,"beat_band_high_hz":2000.0}"#)
                .unwrap();
        cfg.audio_band_ranges.set(
            AudioBandKind::Kick,
            FrequencyRange {
                low_hz: 45.0,
                high_hz: 190.0,
            },
        );
        cfg.migrate_audio_bands();
        assert_eq!(cfg.audio_band_ranges.get(AudioBandKind::Kick).low_hz, 45.0);
        assert_eq!(
            cfg.audio_band_ranges.get(AudioBandKind::Kick).high_hz,
            190.0
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
    fn session_migration_marks_config_dirty_once() {
        let mut cfg = AppConfig::default();
        let netease = cfg
            .sources
            .iter_mut()
            .find(|source| source.is_netease())
            .unwrap();
        netease.netease.as_mut().unwrap().cookie = Some("legacy-cookie".into());
        cfg.netease.cookie = Some("legacy-cookie".into());

        assert!(cfg.normalize_sources());
        let source = cfg
            .sources
            .iter()
            .find(|source| source.is_netease())
            .unwrap();
        assert_eq!(source.session_cookie().as_deref(), Some("legacy-cookie"));
        assert!(!cfg.normalize_sources());
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

    #[test]
    fn motion_settings_defaults_to_balanced_and_unit_multipliers() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.motion_preset, crate::motion::MotionPreset::Balanced);
        assert_eq!(
            cfg.animation_duration_multiplier,
            DEFAULT_ANIMATION_DURATION_MULTIPLIER
        );
        assert_eq!(
            cfg.animation_overshoot_multiplier,
            DEFAULT_ANIMATION_OVERSHOOT_MULTIPLIER
        );
    }

    #[test]
    fn five_band_ranges_and_controls_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.audio_band_ranges.set(
            AudioBandKind::Vocals,
            FrequencyRange {
                low_hz: 800.0,
                high_hz: 1_200.0,
            },
        );
        cfg.audio_band_controls.set(
            AudioBandKind::Vocals,
            chordal_core::ControlPointSet::from_numbers(&[7, 13]),
        );
        let parsed: AppConfig =
            serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(
            parsed.audio_band_ranges.get(AudioBandKind::Vocals).low_hz,
            800.0
        );
        assert_eq!(
            parsed.audio_band_ranges.get(AudioBandKind::Vocals).high_hz,
            1_200.0
        );
        assert_eq!(
            parsed
                .audio_band_controls
                .get(AudioBandKind::Vocals)
                .display(),
            "7,13"
        );
    }

    #[test]
    fn motion_settings_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.motion_preset = crate::motion::MotionPreset::Bouncy;
        cfg.animation_duration_multiplier = 1.5;
        cfg.animation_overshoot_multiplier = 1.75;
        let json = serde_json::to_string(&cfg).unwrap();
        // 预设以小写字符串落盘，不依赖枚举顺序。
        assert!(json.contains("\"motion_preset\":\"bouncy\""));
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.motion_preset, crate::motion::MotionPreset::Bouncy);
        assert!((parsed.animation_duration_multiplier - 1.5).abs() < 1e-6);
        assert!((parsed.animation_overshoot_multiplier - 1.75).abs() < 1e-6);
    }

    #[test]
    fn legacy_config_without_motion_fields_defaults() {
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
        assert_eq!(parsed.motion_preset, crate::motion::MotionPreset::Balanced);
        assert_eq!(
            parsed.animation_duration_multiplier,
            DEFAULT_ANIMATION_DURATION_MULTIPLIER
        );
        assert_eq!(
            parsed.animation_overshoot_multiplier,
            DEFAULT_ANIMATION_OVERSHOOT_MULTIPLIER
        );
    }

    #[test]
    fn motion_multipliers_are_clamped() {
        assert_eq!(
            clamp_animation_duration_multiplier(0.0),
            ANIMATION_DURATION_MULTIPLIER_MIN
        );
        assert_eq!(
            clamp_animation_duration_multiplier(9.0),
            ANIMATION_DURATION_MULTIPLIER_MAX
        );
        assert_eq!(
            clamp_animation_duration_multiplier(f32::NAN),
            DEFAULT_ANIMATION_DURATION_MULTIPLIER
        );
        assert_eq!(
            clamp_animation_overshoot_multiplier(0.0),
            ANIMATION_OVERSHOOT_MULTIPLIER_MIN
        );
        assert_eq!(
            clamp_animation_overshoot_multiplier(9.0),
            ANIMATION_OVERSHOOT_MULTIPLIER_MAX
        );
        assert_eq!(
            clamp_animation_overshoot_multiplier(f32::NAN),
            DEFAULT_ANIMATION_OVERSHOOT_MULTIPLIER
        );
    }
}
