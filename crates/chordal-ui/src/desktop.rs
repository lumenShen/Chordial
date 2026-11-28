#![allow(clippy::type_complexity)]

//! Shared application assembly and the desktop window entry point.

use std::collections::HashMap;
use std::sync::Arc;

use chordal_cache::{CacheConfig, ObjectCache};
use chordal_core::model::SourceId;
use chordal_core::registry::SourceRegistry;
use chordal_local::LocalSource;
use chordal_netease::NeteaseSource;
use chordal_player::Player;
use gpui::AppContext as _;
use gpui::{
    App, Application, Bounds, Pixels, Size, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};

use crate::app::{ChordialApp, SourceBuilder};
use crate::motion::MotionTuning;
use crate::{actions, config, crash_log, input, main_state, responsive, sources, theme};

/// 默认打开尺寸；小屏上会按显示器尺寸收缩。
const DEFAULT_WINDOW_W: f32 = 1440.0;
const DEFAULT_WINDOW_H: f32 = 920.0;
/// 窗口与屏幕边缘之间预留的横向边距。
const WINDOW_SCREEN_MARGIN_X: f32 = 48.0;
/// 纵向额外让出标题栏与任务栏的高度。
const WINDOW_SCREEN_MARGIN_Y: f32 = 88.0;

/// 把期望的打开尺寸收缩到显示器范围内（至少保证最小窗口尺寸）。
///
/// `display.bounds()` 是含任务栏的整块屏幕，所以纵向多留一点边距；
/// 居中后上下各占一半，仍能让窗口完整落在屏幕内。
fn fit_window_to_screen(desired: Size<Pixels>, screen: Size<Pixels>) -> Size<Pixels> {
    let screen_w: f32 = screen.width.into();
    let screen_h: f32 = screen.height.into();
    let max_size = size(
        px((screen_w - WINDOW_SCREEN_MARGIN_X).max(responsive::MIN_WINDOW_W)),
        px((screen_h - WINDOW_SCREEN_MARGIN_Y).max(responsive::MIN_WINDOW_H)),
    );
    desired.min(&max_size)
}

/// Starts Chordial in a desktop window.
pub fn run_desktop() {
    run_app(false);
}

/// Shared startup used by the desktop binary and Android JNI library.
pub(crate) fn run_app(android: bool) {
    // 日志与崩溃兜底：stdout + 文件日志，panic/原生崩溃写报告。
    // RUST_LOG 优先，默认 info；perf 事件走 target=chordial::perf。
    crash_log::init();
    // 内存观测线程：默认 2s 采样一次，环形账本容量固定，长期运行不增长。
    // 采样失败/禁用（CHORDIAL_MEM_INTERVAL_MS=0）不影响主流程。
    chordal_core::mem::start_sampler();
    let baseline = chordal_core::mem::snapshot();
    tracing::info!(
        target: "chordial::mem",
        rss_mb = baseline.rss_bytes.map(|b| b / (1024 * 1024)),
        live_kb = baseline.live_bytes / 1024,
        peak_live_kb = baseline.peak_live_bytes / 1024,
        "memory baseline"
    );

    let cfg = config::load_or_default();

    // 对象缓存池：默认 5 GiB，目录/上限来自配置。打开失败时退回临时目录，
    // 保证播放器仍可用（缓存是加速层，不是正确性依赖）。
    let cache = Arc::new(
        ObjectCache::open(CacheConfig {
            root: cfg.cache_dir.clone(),
            limit_bytes: cfg.cache_limit_bytes,
        })
        .unwrap_or_else(|e| {
            tracing::warn!(
                root = %cfg.cache_dir.display(),
                error = %e,
                "object cache open failed; falling back to temp dir"
            );
            ObjectCache::open(CacheConfig {
                root: std::env::temp_dir().join("chordial-objects"),
                limit_bytes: cfg.cache_limit_bytes,
            })
            .expect("fallback object cache")
        }),
    );

    // ---- 源装配：配置里的每个实例都预构建，ENABLED 的才挂进 registry ----
    //
    // 注册但不加载的实例保留在槽位里（status = Registered），UI 的开关只是
    // 在“加载/软关闭”之间切换；软关闭不调用 shutdown，重开时复用同一实例，
    // 本地索引与网易云会话都不会丢。
    let registry = Arc::new(SourceRegistry::new());
    let mut builders: HashMap<SourceId, SourceBuilder> = HashMap::new();
    let mut local_sources: HashMap<SourceId, Arc<LocalSource>> = HashMap::new();
    let mut netease_sources: HashMap<SourceId, Arc<NeteaseSource>> = HashMap::new();
    let mut local_for_app: Option<Arc<LocalSource>> = None;

    for source_config in &cfg.sources {
        let built = match sources::build(source_config, cache.clone()) {
            Ok(built) => built,
            Err(e) => {
                tracing::warn!(id = %source_config.id, error = %e, "skip source instance");
                continue;
            }
        };
        let id = built.info.id.clone();
        let builder = built.builder();
        builders.insert(id.clone(), builder.clone());
        if let Some(local) = &built.local {
            local_sources.insert(id.clone(), local.clone());
            if local_for_app.is_none() && source_config.enabled {
                local_for_app = Some(local.clone());
            }
        }
        if let Some(netease) = &built.netease {
            netease_sources.insert(id.clone(), netease.clone());
        }
        let descriptor_factory = {
            let instance = built.source.clone();
            Box::new(move |_: &chordal_core::source::SourceInfo| Ok(instance.clone()))
                as Box<
                    dyn Fn(
                            &chordal_core::source::SourceInfo,
                        ) -> chordal_core::error::CoreResult<
                            Arc<dyn chordal_core::source::MusicSource>,
                        > + Send
                        + Sync,
                >
        };
        if let Err(e) = registry.register(chordal_core::registry::SourceDescriptor {
            info: built.info.clone(),
            build: descriptor_factory,
        }) {
            tracing::warn!(id = %id, error = %e, "register source failed");
            continue;
        }
        if source_config.enabled {
            let instance = built.source.clone();
            if let Err(e) = registry.load(&id, |_| Ok(instance.clone())) {
                tracing::warn!(id = %id, error = %e, "load source failed");
            }
        }
    }
    // 没有启用的本地实例时，仍给主面板一个可扫描的本地句柄（源页面可再开启）。
    if local_for_app.is_none() {
        local_for_app = local_sources.values().next().cloned();
    }

    let player = Player::new(cfg.player_device.clone());
    let search_timeout = cfg.search_timeout_ms;
    // 上次退出时性能浮窗是开着的，就在主窗口建好后恢复。
    let startup_perf_overlay = cfg.perf_overlay && !android;

    Application::new().run(move |cx: &mut App| {
        theme::Theme::for_mode(cfg.appearance).set_global(cx);
        actions::install_keybindings(cx);
        input::install_input_keybindings(cx);
        main_state::install(
            registry.clone(),
            player.clone(),
            builders,
            local_sources,
            netease_sources,
            MotionTuning::new(
                cfg.motion_preset,
                cfg.animation_duration_multiplier,
                cfg.animation_overshoot_multiplier,
            ),
            cx,
        );

        let screen_size = cx
            .primary_display()
            .map(|display| display.bounds().size)
            .unwrap_or_else(|| size(px(DEFAULT_WINDOW_W), px(DEFAULT_WINDOW_H)));
        let desired_size = if android {
            screen_size
        } else {
            std::env::var("CHORDIAL_SIZE")
                .ok()
                .and_then(|v| {
                    let (w, h) = v.split_once('x')?;
                    Some(size(px(w.parse().ok()?), px(h.parse().ok()?)))
                })
                .unwrap_or_else(|| size(px(DEFAULT_WINDOW_W), px(DEFAULT_WINDOW_H)))
        };
        let window_size = if android {
            desired_size
        } else {
            fit_window_to_screen(desired_size, screen_size)
        };
        let bounds = Bounds::centered(None, window_size, cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: if android {
                None
            } else {
                Some(TitlebarOptions {
                    title: Some("Chordial".into()),
                    appears_transparent: true,
                    ..Default::default()
                })
            },
            is_movable: !android,
            is_resizable: !android,
            is_minimizable: !android,
            window_min_size: if android {
                None
            } else {
                Some(size(
                    px(responsive::MIN_WINDOW_W),
                    px(responsive::MIN_WINDOW_H),
                ))
            },
            ..Default::default()
        };

        let main_window = cx
            .open_window(options, |window, cx| {
                cx.new(|cx| {
                    ChordialApp::new(
                        registry.clone(),
                        player.clone(),
                        local_for_app.clone(),
                        cache.clone(),
                        search_timeout,
                        window,
                        cx,
                    )
                })
            })
            .expect("open chordal window");

        // 主窗口完成首轮构建后再开浮窗，避免在根视图构造期间嵌套开窗。
        if startup_perf_overlay {
            cx.defer(move |cx| {
                let _ = main_window.update(cx, |app, _window, cx| app.open_perf_overlay(cx));
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size_of(w: f32, h: f32) -> Size<Pixels> {
        size(px(w), px(h))
    }

    #[test]
    fn keeps_default_size_on_large_screens() {
        let fitted = fit_window_to_screen(
            size_of(DEFAULT_WINDOW_W, DEFAULT_WINDOW_H),
            size_of(1920.0, 1080.0),
        );
        assert_eq!(fitted.width, px(DEFAULT_WINDOW_W));
        assert_eq!(fitted.height, px(DEFAULT_WINDOW_H));
    }

    #[test]
    fn shrinks_below_small_screen_resolution() {
        let fitted = fit_window_to_screen(
            size_of(DEFAULT_WINDOW_W, DEFAULT_WINDOW_H),
            size_of(1366.0, 768.0),
        );
        assert_eq!(fitted.width, px(1366.0 - WINDOW_SCREEN_MARGIN_X));
        assert_eq!(fitted.height, px(768.0 - WINDOW_SCREEN_MARGIN_Y));
        assert!(fitted.width < px(1366.0));
        assert!(fitted.height < px(768.0));
    }

    #[test]
    fn respects_min_window_size_on_tiny_screens() {
        let fitted = fit_window_to_screen(
            size_of(DEFAULT_WINDOW_W, DEFAULT_WINDOW_H),
            size_of(800.0, 540.0),
        );
        assert_eq!(fitted.width, px(800.0 - WINDOW_SCREEN_MARGIN_X));
        assert_eq!(fitted.height, px(responsive::MIN_WINDOW_H));
    }
}
