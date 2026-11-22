#![allow(clippy::type_complexity)]

//! Chordial 2.0 桌面入口。
//!
//! 装配流程：tracing → 配置 → registry 注册源描述符（预构建实例）→
//! gpui Application → 开窗渲染根视图。

mod actions;
mod app;
mod autoeq;
mod config;
mod downloads;
mod eq;
mod eq_page;
mod icons;
mod input;
mod layout_morph;
mod lyrics;
mod main_state;
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
use gpui::{App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

use crate::app::{ChordialApp, SourceBuilder};

fn main() {
    // 日志：RUST_LOG 优先，默认 info。perf 事件走 target=chordial::perf。
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,chordial=debug")),
        )
        .with_target(true)
        .init();

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

    Application::new().run(move |cx: &mut App| {
        theme::Theme::for_mode(cfg.appearance).set_global(cx);
        actions::install_keybindings(cx);
        input::install_input_keybindings(cx);
        main_state::install(registry.clone(), player.clone(), builders, cx);

        let display_size = std::env::var("CHORDIAL_SIZE")
            .ok()
            .and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some(size(px(w.parse().ok()?), px(h.parse().ok()?)))
            })
            .unwrap_or_else(|| size(px(1440.0), px(920.0)));
        let bounds = Bounds::centered(None, display_size, cx);
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

        cx.open_window(options, |window, cx| {
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
    });
}
