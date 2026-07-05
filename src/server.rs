use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::{
    compression::CompressionLayer,
    services::ServeDir,
    set_header::SetResponseHeaderLayer,
};
use tracing::{debug, warn};

use crate::config::ServerConfig;

pub struct AppState {
    pub tx: broadcast::Sender<()>,
}

pub fn build_app(state: Arc<AppState>, cfg: &ServerConfig) -> Router {
    let state_ws = state.clone();

    Router::new()
        .route("/healthz", get(health_handler))
        .route(
            "/livereload",
            get(move |ws: WebSocketUpgrade| {
                let s = state_ws.clone();
                async move { ws.on_upgrade(move |socket| handle_ws(socket, s)) }
            }),
        )
        .fallback_service(ServeDir::new(&cfg.web_folder).append_index_html_on_directories(true))
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-cache, no-store, must-revalidate"),
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
