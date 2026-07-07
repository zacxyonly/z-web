use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Request, State,
    },
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use std::{net::SocketAddr, path::Path, sync::Arc};
use tokio::sync::broadcast;
use tower::ServiceExt;
use tower_http::{
    compression::CompressionLayer,
    services::ServeDir,
    set_header::SetResponseHeaderLayer,
};
use tracing::{debug, error, warn};

use crate::config::ServerConfig;
use crate::php;

pub struct AppState {
    pub tx: broadcast::Sender<()>,
    pub cfg: ServerConfig,
}

pub fn build_app(state: Arc<AppState>, cfg: &ServerConfig) -> Router {
    let state_ws = state.clone();
    let port = cfg.port;

    Router::new()
        .route("/healthz", get(health_handler))
        .route(
            "/livereload",
            get(move |ws: WebSocketUpgrade| {
                let s = state_ws.clone();
                async move { ws.on_upgrade(move |socket| handle_ws(socket, s)) }
            }),
        )
        .fallback(move |State(state): State<Arc<AppState>>, req: Request| async move {
            serve_or_php(state, port, req).await
        })
        .with_state(state)
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        ))
}

/// Decide whether a request should go to PHP-FPM or to the static file
/// server, and dispatch accordingly. Static-file behavior is unchanged when
/// `php` isn't configured (or is disabled) for this server.
async fn serve_or_php(state: Arc<AppState>, port: u16, req: Request) -> Response {
    let cfg = &state.cfg;

    if let Some(php_cfg) = cfg.php.as_ref().filter(|p| p.enabled) {
        let uri = req.uri().clone();
        if let Some(script_name) = resolve_php_script(&cfg.web_folder, php_cfg, uri.path()) {
            let method = req.method().clone();
            let headers = req.headers().clone();
            let query_string = uri.query().unwrap_or("").to_string();
            let remote_addr = req
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|c| c.0.ip().to_string())
                .unwrap_or_else(|| "127.0.0.1".to_string());

            let body: Bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
                Ok(b) => b,
                Err(e) => {
                    error!("Failed to read request body: {e}");
                    return (StatusCode::BAD_REQUEST, "Failed to read request body")
                        .into_response();
                }
            };

            let script_filename =
                format!("{}/{}", cfg.web_folder.trim_end_matches('/'), script_name);
            let request_uri = uri
                .path_and_query()
                .map(|pq| pq.as_str().to_string())
                .unwrap_or_else(|| uri.path().to_string());
            let script_name_abs = format!("/{script_name}");

            return php::proxy_to_php(
                php_cfg,
                &cfg.web_folder,
                &script_filename,
                &script_name_abs,
                &request_uri,
                &query_string,
                &method,
                &headers,
                &remote_addr,
                port,
                body,
            )
            .await;
        }
    }

    serve_static(&cfg.web_folder, req).await
}

/// If `path` maps to a `.php` script (directly, or via directory + index
/// file) that exists on disk under `web_folder`, return its path relative
/// to `web_folder`. Otherwise return None so the request falls through to
/// static file serving.
fn resolve_php_script(
    web_folder: &str,
    php_cfg: &crate::config::PhpConfig,
    request_path: &str,
) -> Option<String> {
    let trimmed = request_path.trim_start_matches('/');

    let candidate = if trimmed.ends_with(&php_cfg.extension) {
        trimmed.to_string()
    } else if request_path.ends_with('/') || trimmed.is_empty() {
        format!("{}{}", trimmed, php_cfg.index)
    } else {
        return None;
    };

    // Guard against path traversal escaping web_folder.
    if candidate.contains("..") {
        return None;
    }

    let full_path = Path::new(web_folder).join(&candidate);
    if full_path.is_file() {
        Some(candidate)
    } else {
        None
    }
}

async fn serve_static(web_folder: &str, req: Request) -> Response {
    let service = ServeDir::new(web_folder).append_index_html_on_directories(true);
    match service.oneshot(req).await {
        Ok(res) => res.into_response(),
        Err(e) => {
            error!("Static file service error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Static file error").into_response()
        }
    }
}

async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION")
        })),
    )
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(()) => {
                        debug!("Sending reload to client");
                        if socket.send(Message::Text("reload".into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("WS receiver lagged {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() { break; }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
