#![allow(clippy::type_complexity)]

//! Chordial 2.0 桌面入口。
//!
//! 装配流程：crash_log/tracing → 配置 → registry 注册源描述符（预构建实例）→
//! gpui Application → 开窗渲染根视图。

mod actions;
mod app;
mod autoeq;
mod bubble;
mod config;
mod crash_log;
mod downloads;
mod eq;
mod eq_page;
mod icons;
mod input;
mod layout_morph;
mod lyrics;
mod main_state;
mod perf_overlay;
mod phone_gesture;
mod player_page;
mod responsive;
mod settings;
mod sheet_drag;
mod theme;
mod widgets;

use std::collections::HashMap;
use std::sync::Arc;

use chordal_cache::{CacheConfig, ObjectCache};
use chordal_core::model::SourceId;
use chordal_core::registry::SourceRegistry;
use chordal_core::source::{MusicSource, SourceInfo};
use chordal_local::LocalSource;
use chordal_netease::NeteaseSource;
use chordal_player::Player;
use gpui::AppContext as _;
use gpui::{
    App, Application, Bounds, Pixels, Size, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};

use crate::app::{ChordialApp, SourceBuilder};

/// 运行内存观测：包装系统分配器，记录堆活跃/峰值与分配次数。
///
/// 采集数据由设置「存储」页的内存诊断区展示；`CHORDIAL_MEM_TRACE=1`
/// 可把周期采样写入日志（见 [`chordal_core::mem`]）。
#[global_allocator]
static GLOBAL_ALLOCATOR: chordal_core::mem::TrackingAllocator =
    chordal_core::mem::TrackingAllocator;

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

fn main() {
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

    // ---- 源装配：预构建实例 + builder 挂回工厂 ----
    let registry = Arc::new(SourceRegistry::new());
    let mut builders: HashMap<SourceId, SourceBuilder> = HashMap::new();

    // 本地源：实例常驻（卸载只摘除注册，索引保留，重载零成本）。
    let local = LocalSource::new(
        SourceId::new("local"),
        chordal_local::LocalSourceConfig {
            roots: cfg.local_roots.clone(),
        },
        "本地音乐",
    );
    {
        let info = SourceInfo {
            id: SourceId::new("local"),
            display_name: "本地音乐".into(),
            description: local.info().description.clone(),
            kind: chordal_core::model::SourceKind::LocalLibrary,
            priority: 0,
        };
        let local_for_info = local.clone();
        let _ = local_for_info;
        registry
            .register(chordal_core::registry::SourceDescriptor {
                info: info.clone(),
                build: {
                    let local = local.clone();
                    Box::new(move |_| Ok(local.clone()))
                },
            })
            .expect("register local source");
        let local_arc: Arc<dyn chordal_core::source::MusicSource> = local.clone();
        registry
            .load(&SourceId::new("local"), |_| Ok(local_arc.clone()))
            .expect("load local source");
        builders.insert(
            SourceId::new("local"),
            Arc::new({
                let local = local.clone();
                move |_: &SourceInfo| Ok(local.clone() as Arc<_>)
            }),
        );
    }

    // 网易云源：构建失败不致命（服务未启动时保持 Registered 状态，可稍后加载）。
    let netease_ready = {
        let netease = NeteaseSource::new_with_cache(
            SourceId::new("netease"),
            cfg.netease.clone(),
            "网易云音乐",
            Some(cache.clone()),
        );
        let info = SourceInfo {
            id: SourceId::new("netease"),
            display_name: "网易云音乐".into(),
            description: netease.info().description.clone(),
            kind: chordal_core::model::SourceKind::HttpApi,
            priority: 1,
        };
        registry
            .register(chordal_core::registry::SourceDescriptor {
                info,
                build: {
                    let netease = netease.clone();
                    Box::new(move |_| Ok(netease.clone()))
                },
            })
            .expect("register netease source");
        builders.insert(
            SourceId::new("netease"),
            Arc::new({
                let netease = netease.clone();
                move |_: &SourceInfo| Ok(netease.clone() as Arc<_>)
            }),
        );
        // 预加载（后续 health 探测失败会在 UI 显示）。
        let netease_arc: Arc<dyn chordal_core::source::MusicSource> = netease.clone();
        registry
            .load(&SourceId::new("netease"), |_| Ok(netease_arc.clone()))
            .is_ok()
    };
    let _ = netease_ready;

    let player = Player::new(cfg.player_device.clone());
    let search_timeout = cfg.search_timeout_ms;
    let local_for_app = local.clone();
    // 上次退出时性能浮窗是开着的，就在主窗口建好后恢复。
    let startup_perf_overlay = cfg.perf_overlay;

    Application::new().run(move |cx: &mut App| {
        theme::Theme::for_mode(cfg.appearance).set_global(cx);
        actions::install_keybindings(cx);
        input::install_input_keybindings(cx);
        main_state::install(registry.clone(), player.clone(), builders, cx);

        let desired_size = std::env::var("CHORDIAL_SIZE")
            .ok()
            .and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some(size(px(w.parse().ok()?), px(h.parse().ok()?)))
            })
            .unwrap_or_else(|| size(px(DEFAULT_WINDOW_W), px(DEFAULT_WINDOW_H)));

        // 打开尺寸收敛到屏幕内：默认 1440×920，在 1366×768 等小屏上会超出
        // 屏幕；这里按主显示器尺寸收缩，并留出边距避免贴边或被任务栏遮挡
        // （gpui 的 display.bounds() 含任务栏区域，不扣边距会盖住任务栏）。
        let screen_size = cx
            .primary_display()
            .map(|display| display.bounds().size)
            .unwrap_or(desired_size);
        let window_size = fit_window_to_screen(desired_size, screen_size);
        let bounds = Bounds::centered(None, window_size, cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Chordial".into()),
                // 自绘 macOS 风格标题栏：系统标题栏隐藏，交通灯与拖拽区在 app.rs 里绘制。
                appears_transparent: true,
                ..Default::default()
            }),
            // 允许收缩到手机竖屏：主面板在 <720 时切换为单列手机壳。
            window_min_size: Some(size(
                px(responsive::MIN_WINDOW_W),
                px(responsive::MIN_WINDOW_H),
            )),
            ..Default::default()
        };

        let main_window = cx
            .open_window(options, |window, cx| {
                cx.new(|cx| {
                    ChordialApp::new(
                        registry.clone(),
                        player.clone(),
                        Some(local_for_app.clone()),
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
