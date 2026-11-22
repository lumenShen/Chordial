//! 用户配置：`~/.config/chordial/config.json`。缺失时写默认值。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use chordal_netease::{NeteaseConfig, SongQualityLevel};

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
    /// 背景鼓点监听频段下限（Hz）。
    #[serde(default = "default_beat_band_low_hz")]
    pub beat_band_low_hz: f32,
    /// 背景鼓点监听频段上限（Hz）。
    #[serde(default = "default_beat_band_high_hz")]
    pub beat_band_high_hz: f32,
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
            beat_band_low_hz: chordal_player::DEFAULT_BEAT_BAND.0,
            beat_band_high_hz: chordal_player::DEFAULT_BEAT_BAND.1,
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chordial")
        .join("config.json")
}

pub fn load_or_default() -> AppConfig {
    let path = config_path();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        match serde_json::from_str::<AppConfig>(&raw) {
            Ok(cfg) => return cfg,
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
