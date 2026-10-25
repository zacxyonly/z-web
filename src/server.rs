use axum::{
    body::to_bytes,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use std::{path::Path, sync::Arc};
use tokio::sync::broadcast;
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};
use tracing::{debug, warn};

use crate::config::ServerConfig;
use crate::php::php_middleware;
use crate::proxy::handle_proxy;

pub struct AppState {
    pub tx: broadcast::Sender<()>,
}

pub fn build_app(state: Arc<AppState>, cfg: &ServerConfig) -> Router {
    let state_ws = state.clone();
    let php_cfg = Arc::new(cfg.clone());

    let folder_path = Path::new(&cfg.web_folder);
    let app = Router::new()
        .route("/healthz", get(health_handler))
        .route(
            "/livereload",
            get(move |ws: WebSocketUpgrade| {
                let s = state_ws.clone();
                async move { ws.on_upgrade(move |socket| handle_ws(socket, s)) }
            }),
        );

    let app = if let Some(ref upstream) = cfg.proxy {
        let client = Arc::new(reqwest::Client::builder().build().unwrap_or_default());
        let upstream = Arc::new(upstream.clone());
        let proxy_client = client.clone();
        let proxy_upstream = upstream.clone();

        let serve_dir = ServeDir::new(&cfg.web_folder)
            .append_index_html_on_directories(true)
            .fallback(axum::routing::any(move |req: axum::extract::Request| {
                let c = proxy_client.clone();
                let u = proxy_upstream.clone();
                async move { handle_proxy(c, u, req).await }
            }));
        app.fallback_service(serve_dir)
    } else if cfg.spa {
        let index_file = folder_path.join("index.html");
        let serve_dir = ServeDir::new(&cfg.web_folder)
            .append_index_html_on_directories(true)
            .fallback(ServeFile::new(index_file));
        app.fallback_service(serve_dir)
    } else {
        let not_found_file = folder_path.join("404.html");
        if not_found_file.is_file() {
            let serve_dir = ServeDir::new(&cfg.web_folder)
                .append_index_html_on_directories(true)
                .not_found_service(ServeFile::new(not_found_file));
            app.fallback_service(serve_dir)
        } else {
            let serve_dir = ServeDir::new(&cfg.web_folder)
                .append_index_html_on_directories(true);
            app.fallback_service(serve_dir)
        }
    };

    app.layer(middleware::from_fn(livereload_inject_middleware))
        .layer(middleware::from_fn_with_state(php_cfg, php_middleware))
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            header::HeaderValue::from_static("*"),
        ))
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

/// Automatically inject the WebSocket live-reload snippet before `</body>` for HTML responses.
async fn livereload_inject_middleware(req: axum::extract::Request, next: Next) -> Response {
    let response = next.run(req).await;

    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map_or(false, |ct| ct.starts_with("text/html"));

    if !is_html || response.status() != StatusCode::OK {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, 20 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return Response::from_parts(parts, axum::body::Body::empty()),
    };

    let html_str = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(_) => return Response::from_parts(parts, axum::body::Body::from(bytes)),
    };

    if html_str.contains("/livereload") {
        return Response::from_parts(parts, axum::body::Body::from(bytes));
    }

    const SCRIPT: &str = r#"<script>
(function(){
  var proto=location.protocol==='https:'?'wss:':'ws:';
  var ws=new WebSocket(proto+'//'+location.host+'/livereload');
  ws.onmessage=function(e){if(e.data==='reload')location.reload();};
  ws.onclose=function(){setTimeout(function(){location.reload();},2000);};
})();
</script>"#;

    let modified = if let Some(pos) = html_str.to_ascii_lowercase().rfind("</body>") {
        format!("{}{}\n{}", &html_str[..pos], SCRIPT, &html_str[pos..])
    } else {
        format!("{}\n{}", html_str, SCRIPT)
    };

    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, axum::body::Body::from(modified))
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(()) => {
                        // Debounce: wait briefly and discard rapid subsequent events
                        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                        while rx.try_recv().is_ok() {}

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
