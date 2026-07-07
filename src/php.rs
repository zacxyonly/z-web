//! PHP support via `php-cgi` (classic CGI, RFC 3875-ish).
//!
//! This is intentionally *not* FastCGI/php-fpm — it spawns one `php-cgi`
//! process per request. That's slower than a persistent FastCGI pool, but it
//! needs zero extra services running and zero extra dependencies, which
//! matches how the rest of z-web is built. For a personal/self-hosted static
//! site with occasional PHP pages, this is plenty fast; for high-traffic PHP
//! apps, put z-web behind something with a real FastCGI pool instead.

use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header::CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};
use tokio::{io::AsyncWriteExt, process::Command};
use tracing::{error, warn};

use crate::config::ServerConfig;

/// Cap on request body size piped into php-cgi's stdin (20 MiB).
const MAX_BODY_BYTES: usize = 20 * 1024 * 1024;

/// Axum middleware: if this server has `php: true` and the request targets a
/// `.php` script (or a directory whose `index.php` should be served), execute
/// it via `php-cgi` and return its output. Everything else passes through to
/// the normal router/static-file fallback untouched.
pub async fn php_middleware(
    State(cfg): State<Arc<ServerConfig>>,
    req: Request,
    next: Next,
) -> Response {
    if !cfg.php {
        return next.run(req).await;
    }

    let path = req.uri().path().to_string();

    if is_php_path(&path) {
        return match resolve_within_root(&cfg.web_folder, &path) {
            Some(script) if script.is_file() => run_php_cgi(&cfg, req, script).await,
            _ => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
        };
    }

    // Directory index fallback: serve index.php when there's no index.html.
    if let Some(dir) = resolve_within_root(&cfg.web_folder, &path) {
        if dir.is_dir() {
            let index_php = dir.join("index.php");
            let has_html_index = dir.join("index.html").is_file();
            if !has_html_index && index_php.is_file() {
                return run_php_cgi(&cfg, req, index_php).await;
            }
        }
    }

    next.run(req).await
}

fn is_php_path(path: &str) -> bool {
    path.ends_with(".php")
}

/// Safely resolve a URL path against `web_folder`, rejecting anything that
/// would escape it (`..`, symlink traversal, etc). Returns `None` for any
/// path that doesn't exist or resolves outside the root.
fn resolve_within_root(web_folder: &str, url_path: &str) -> Option<PathBuf> {
    let root = Path::new(web_folder).canonicalize().ok()?;

    let mut candidate = root.clone();
    for segment in url_path.trim_start_matches('/').split('/') {
        match segment {
            "" | "." => continue,
            ".." => return None,
            s => candidate.push(s),
        }
    }

    let canonical = candidate.canonicalize().ok()?;
    canonical.starts_with(&root).then_some(canonical)
}

async fn run_php_cgi(cfg: &ServerConfig, req: Request, script_path: PathBuf) -> Response {
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| path.clone());
    let headers = req.headers().clone();

    let body_bytes = match to_bytes(req.into_body(), MAX_BODY_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to read request body for PHP: {e}");
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
    };

    let mut cmd = Command::new(&cfg.php_cgi_path);
    cmd.env_clear()
        .env("REDIRECT_STATUS", "200")
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .env(
            "SERVER_SOFTWARE",
            concat!("z-web/", env!("CARGO_PKG_VERSION")),
        )
        .env("REQUEST_METHOD", &method)
        .env("SCRIPT_FILENAME", &script_path)
        .env("SCRIPT_NAME", &path)
        .env("REQUEST_URI", &path_and_query)
        .env("QUERY_STRING", &query)
        .env("SERVER_PORT", cfg.port.to_string())
        .env("SERVER_NAME", &cfg.ip)
        .env("DOCUMENT_ROOT", &cfg.web_folder)
        .env("CONTENT_LENGTH", body_bytes.len().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(path_env) = std::env::var_os("PATH") {
        cmd.env("PATH", path_env);
    }

    if let Some(ct) = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        cmd.env("CONTENT_TYPE", ct);
    }

    // Forward client headers as HTTP_* CGI vars (standard CGI convention).
    for (name, value) in headers.iter() {
        if let Ok(v) = value.to_str() {
            let env_name = format!("HTTP_{}", name.as_str().to_uppercase().replace('-', "_"));
            cmd.env(env_name, v);
        }
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            error!(cgi = %cfg.php_cgi_path, "Failed to spawn php-cgi: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "php-cgi failed to start ({e}). Is PHP-CGI installed and on PATH? \
                     (Debian/Ubuntu: apt install php-cgi)"
                ),
            )
                .into_response();
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if !body_bytes.is_empty() {
            if let Err(e) = stdin.write_all(&body_bytes).await {
                warn!("Failed writing request body to php-cgi stdin: {e}");
            }
        }
        drop(stdin); // EOF so php-cgi knows the input is complete
    }

    let output = match child.wait_with_output().await {
        Ok(o) => o,
        Err(e) => {
            error!("php-cgi process error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "php-cgi execution failed",
            )
                .into_response();
        }
    };

    if !output.status.success() {
        warn!(status = ?output.status, "php-cgi exited non-zero");
    }
    if !output.stderr.is_empty() {
        warn!(
            "php-cgi stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    parse_cgi_response(&output.stdout)
}

/// Parse a raw CGI response (`Header: value` lines, blank line, then body)
/// into an axum `Response`. A `Status:` header sets the HTTP status code;
/// anything else is passed through as a response header.
fn parse_cgi_response(raw: &[u8]) -> Response {
    let (header_bytes, body) = match find_header_body_split(raw) {
        Some((head_end, body_start)) => (&raw[..head_end], &raw[body_start..]),
        None => (raw, &raw[raw.len()..]),
    };

    let mut status = StatusCode::OK;
    let mut header_map = HeaderMap::new();

    for line in String::from_utf8_lossy(header_bytes).lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        if key.eq_ignore_ascii_case("Status") {
            if let Some(code) = value
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u16>().ok())
                .and_then(|c| StatusCode::from_u16(c).ok())
            {
                status = code;
            }
            continue;
        }

        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            header_map.append(name, val);
        }
    }

    let mut builder = Response::builder().status(status);
    for (name, value) in header_map.iter() {
        builder = builder.header(name, value);
    }

    builder.body(Body::from(body.to_vec())).unwrap_or_else(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to build PHP response",
        )
            .into_response()
    })
}

/// Find where CGI headers end and the body begins (first blank line).
/// Returns `(header_end_index, body_start_index)`.
fn find_header_body_split(raw: &[u8]) -> Option<(usize, usize)> {
    for i in 0..raw.len() {
        if raw[i..].starts_with(b"\r\n\r\n") {
            return Some((i, i + 4));
        }
        if raw[i..].starts_with(b"\n\n") {
            return Some((i, i + 2));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_php_paths() {
        assert!(is_php_path("/index.php"));
        assert!(is_php_path("/blog/post.php"));
        assert!(!is_php_path("/style.css"));
        assert!(!is_php_path("/"));
    }

    #[test]
    fn parses_status_header_and_body() {
        let raw = b"Status: 404 Not Found\r\nContent-Type: text/plain\r\n\r\nOops";
        let resp = parse_cgi_response(raw);
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn defaults_to_200_without_status_header() {
        let raw = b"Content-Type: text/html\n\n<h1>Hi</h1>";
        let resp = parse_cgi_response(raw);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn handles_missing_header_body_separator() {
        // Malformed/no-header output shouldn't panic — treat it all as body.
        let raw = b"just raw output, no headers";
        let resp = parse_cgi_response(raw);
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
