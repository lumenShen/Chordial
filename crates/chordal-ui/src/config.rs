//! 用户配置：`~/.config/chordial/config.json`。缺失时写默认值。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use chordal_netease::{NeteaseConfig, SongQualityLevel};

use crate::eq::SavedEqPreset;
use crate::theme::Appearance;

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
    pub local_roots: Vec<PathBuf>,
    pub netease: NeteaseConfig,
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

impl Default for AppConfig {
    fn default() -> Self {
        let music = dirs::audio_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            local_roots: vec![music],
            netease: NeteaseConfig::default(),
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
                // 手改配置时也保证倍率落在可渲染范围。
                cfg.beat_strength = cfg.beat_strength.clamp(0.0, 1.5);
                cfg.lyric_sweep_fade_em = clamp_lyric_sweep_fade_em(cfg.lyric_sweep_fade_em);
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
        assert!((parsed.lyric_sweep_fade_em - 0.6).abs() < 1e-6);
    }
}
