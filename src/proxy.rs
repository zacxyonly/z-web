use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use reqwest::Client;
use std::sync::Arc;
use tracing::{debug, error};

pub async fn handle_proxy(
    client: Arc<Client>,
    upstream: Arc<String>,
    req: Request,
) -> Response {
    let method = req.method().clone();
    let path_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let target_url = format!("{}{}", upstream.trim_end_matches('/'), path_query);
    debug!(%method, %target_url, "Proxying request to upstream");

    let (parts, body) = req.into_parts();

    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(m) => m,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid HTTP method").into_response(),
    };

    let mut forward_req = client.request(reqwest_method, &target_url);

    // Forward client headers (skip hop-by-hop)
    for (name, value) in parts.headers.iter() {
        let n = name.as_str();
        if n.eq_ignore_ascii_case("host")
            || n.eq_ignore_ascii_case("connection")
            || n.eq_ignore_ascii_case("keep-alive")
            || n.eq_ignore_ascii_case("proxy-authenticate")
            || n.eq_ignore_ascii_case("proxy-authorization")
            || n.eq_ignore_ascii_case("te")
            || n.eq_ignore_ascii_case("trailers")
            || n.eq_ignore_ascii_case("transfer-encoding")
            || n.eq_ignore_ascii_case("upgrade")
        {
            continue;
        }
        if let Ok(v) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
            if let Ok(k) = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()) {
                forward_req = forward_req.header(k, v);
            }
        }
    }

    if let Some(host) = parts.headers.get(header::HOST) {
        if let Ok(h) = host.to_str() {
            forward_req = forward_req.header("X-Forwarded-Host", h);
        }
    }
    forward_req = forward_req.header("X-Forwarded-Proto", "http");

    if parts.method != axum::http::Method::GET && parts.method != axum::http::Method::HEAD {
        match to_bytes(body, 50 * 1024 * 1024).await {
            Ok(bytes) => {
                forward_req = forward_req.body(bytes);
            }
            Err(e) => {
                error!("Failed reading request body for proxy: {e}");
                return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
            }
        }
    }

    match forward_req.send().await {
        Ok(upstream_resp) => {
            let status = StatusCode::from_u16(upstream_resp.status().as_u16())
                .unwrap_or(StatusCode::BAD_GATEWAY);

            let mut resp_builder = Response::builder().status(status);

            for (name, value) in upstream_resp.headers().iter() {
                let n = name.as_str();
                if n.eq_ignore_ascii_case("connection")
                    || n.eq_ignore_ascii_case("transfer-encoding")
                {
                    continue;
                }
                resp_builder = resp_builder.header(name.as_str(), value.as_bytes());
            }

            let stream = upstream_resp.bytes_stream();
            let body = Body::from_stream(stream);

            match resp_builder.body(body) {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed building proxy response: {e}");
                    (StatusCode::INTERNAL_SERVER_ERROR, "Proxy Response Error").into_response()
                }
            }
        }
        Err(err) => {
            error!(%target_url, "Upstream connection error: {err}");
            (
                StatusCode::BAD_GATEWAY,
                format!("502 Bad Gateway: Could not connect to upstream {target_url} ({err})"),
            )
                .into_response()
        }
    }
}
