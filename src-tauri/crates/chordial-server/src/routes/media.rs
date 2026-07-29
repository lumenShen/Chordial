//! 媒体流路由 — audio / image / lyric（复用 core::media::handle）。
//!
//! | 方法 | 路径 | 对应功能 |
//! |------|------|---------|
//! | GET/HEAD | `/audio/:sn_b64/:eid_b64` | 音频流（支持 Range/206） |
//! | GET | `/image/:sn_b64/:eid_b64` | 封面图片 |
//! | GET | `/lyric/:sn_b64/:eid_b64` | 歌词文本 |

use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/audio/{sn}/{eid}", get(audio).head(audio))
        .route("/image/{sn}/{eid}", get(image))
        .route("/lyric/{sn}/{eid}", get(lyric))
}

/// 将 core 的 `http::Response<Vec<u8>>` 转换为 axum `Response`。
fn convert_response(core_resp: http::Response<Vec<u8>>) -> Response {
    let (parts, body) = core_resp.into_parts();
    let mut builder = Response::builder().status(parts.status);

    for (name, value) in parts.headers.iter() {
        builder = builder.header(name.as_str(), value.to_str().unwrap_or(""));
    }

    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("构建响应失败"))
                .unwrap()
        })
}

/// 构建一个 http::Request<Vec<u8>>（core::media 使用的类型）。
fn to_core_request(method: &Method, headers: &HeaderMap) -> http::Request<Vec<u8>> {
    let mut req = http::Request::builder().method(method.as_str());
    for (name, value) in headers.iter() {
        req = req.header(name.as_str(), value.as_bytes());
    }
    req.body(Vec::new()).unwrap()
}

/// `GET/HEAD /audio/{sn}/{eid}` — 音频流（支持 Range）。
///
/// `media::handle` 内部调用同步阻塞的 `reqwest::blocking`（在线源拉取音频/封面/歌词），
/// 必须用 `spawn_blocking` 包裹，否则阻塞 tokio runtime 导致 server 冻结。
async fn audio(
    State(state): State<AppState>,
    Path((sn, eid)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let path = format!("/audio/{}/{}", sn, eid);
    let req = to_core_request(&method, &headers);
    let registrar = state.ctx.registrar.clone();
    let resp = match tokio::task::spawn_blocking(move || chordial_core::media::handle(&registrar, &path, &req))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("阻塞任务失败: {}", e)))
                .unwrap();
        }
    };
    convert_response(resp)
}

/// `GET /image/{sn}/{eid}` — 封面图片。
async fn image(
    State(state): State<AppState>,
    Path((sn, eid)): Path<(String, String)>,
) -> Response {
    let path = format!("/image/{}/{}", sn, eid);
    let req = http::Request::builder()
        .method("GET")
        .body(Vec::new())
        .unwrap();
    let registrar = state.ctx.registrar.clone();
    let resp = match tokio::task::spawn_blocking(move || chordial_core::media::handle(&registrar, &path, &req))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("阻塞任务失败: {}", e)))
                .unwrap();
        }
    };
    convert_response(resp)
}

/// `GET /lyric/{sn}/{eid}` — 歌词文本。
async fn lyric(
    State(state): State<AppState>,
    Path((sn, eid)): Path<(String, String)>,
) -> Response {
    let path = format!("/lyric/{}/{}", sn, eid);
    let req = http::Request::builder()
        .method("GET")
        .body(Vec::new())
        .unwrap();
    let registrar = state.ctx.registrar.clone();
    let resp = match tokio::task::spawn_blocking(move || chordial_core::media::handle(&registrar, &path, &req))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("阻塞任务失败: {}", e)))
                .unwrap();
        }
    };
    convert_response(resp)
}
