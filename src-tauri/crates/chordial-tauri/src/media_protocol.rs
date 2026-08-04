//! `chordial://` 自定义 URI scheme 协议适配器（Tauri 端）。
//!
//! 这是 transport 适配层：从 Tauri 的 URI scheme 回调取到请求，委托给
//! [`chordial_core::media::handle`]（与 server 端共用同一逻辑），再将返回的
//! `http::Response` 交给 Tauri 的 [`UriSchemeResponder`]。
//!
//! 由于 Tauri 的协议回调签名不提供 `State` 访问，这里用一个运行时桥接
//! [`REGISTRAR`]（在 `setup` 中由 `lib.rs` 写入）暂存对 core 的引用。

use chordial_core::module::music_source::registrar::SourceRegistrar;
use chordial_core::media;
use http::{header, HeaderValue, Request, Response};
use std::sync::{Arc, OnceLock};
use tauri::UriSchemeResponder;

/// 运行时桥接：setup 阶段写入，协议回调读取。
///
/// 持有对来源注册器的静态引用（替代原 commands::source_registrar() 全局单例）。
static REGISTRAR: OnceLock<Arc<SourceRegistrar>> = OnceLock::new();

/// 由 `lib.rs` 的 setup 调用，注入来源注册器。
pub fn init(registrar: Arc<SourceRegistrar>) {
    let _ = REGISTRAR.set(registrar);
}

/// 将 core 的 `http::Response<Vec<u8>>` 适配为 Tauri 期望的响应。
///
/// 注入 CORS 头：Tauri 自定义协议（`chordial://` / `http://chordial.localhost`）
/// 没有中间件层，需在此手动添加。dev 模式下页面跑在 `http://localhost:1420`，
/// 而 AMLL React 组件用 `fetch()` 加载封面用于 Pixi/WebGL 纹理，跨域会被拦截。
fn to_tauri_response(response: Response<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let (mut parts, body) = response.into_parts();
    parts.headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    parts.headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD, OPTIONS"),
    );
    parts.headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("*"),
    );
    tauri::http::Response::from_parts(parts, body)
}

/// 处理 chordial 协议请求。
///
/// 被 Tauri 的 `register_asynchronous_uri_scheme_protocol` 调用。
///
/// **必须在独立线程中执行实际工作**：`register_asynchronous_uri_scheme_protocol`
/// 的回调运行在 webview 的协议线程上（Windows WebView2 为 UI 线程）。
/// `media::handle` 涉及磁盘 I/O、symphonia 封面提取（117-296ms/次）等阻塞操作，
/// 若同步执行会阻塞协议线程，导致后续 `chordial://` 请求排队甚至 UI 冻结。
///
/// 通过 `std::thread::spawn` 将阻塞工作移至后台线程，协议线程立即返回。
/// 浏览器对同一 host 的并发连接数有上限（Chrome 为 6），因此线程数天然受限。
/// `UriSchemeResponder` 内部通过 channel 通信，是 `Send` 的，可安全跨线程使用。
pub fn handle_protocol(request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    std::thread::spawn(move || {
        let path = request.uri().path().to_string();

        let response = match REGISTRAR.get() {
            Some(registrar) => media::handle(registrar, &path, &request),
            None => media::error_response(
                http::StatusCode::SERVICE_UNAVAILABLE,
                "音乐来源系统未初始化",
            ),
        };

        responder.respond(to_tauri_response(response));
    });
}
