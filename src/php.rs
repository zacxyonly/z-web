use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use fastcgi_client::{Client, Params, Request as FcgiRequest};
use tokio::net::{TcpStream, UnixStream};
use tracing::{error, warn};

use crate::config::{FpmTarget, PhpConfig};

/// Forward a single HTTP request to php-fpm over FastCGI and translate the
/// CGI response back into an axum `Response`.
#[allow(clippy::too_many_arguments)]
pub async fn proxy_to_php(
    php_cfg: &PhpConfig,
    doc_root: &str,
    script_filename: &str,
    script_name: &str,
    request_uri: &str,
    query_string: &str,
    method: &Method,
    headers: &HeaderMap,
    remote_addr: &str,
    server_port: u16,
    body: Bytes,
) -> Response {
    let target = match php_cfg.target() {
        Ok(t) => t,
        Err(e) => {
            error!("Invalid fpm_socket config: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "PHP-FPM misconfigured").into_response();
        }
    };

    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let params = Params::default()
        .request_method(method.as_str())
        .script_name(script_name)
        .script_filename(script_filename)
        .request_uri(request_uri)
        .document_root(doc_root)
        .document_uri(script_name)
        .query_string(query_string)
        .remote_addr(remote_addr)
        .remote_port(0)
        .server_addr("127.0.0.1")
        .server_port(server_port)
        .server_name("z-web")
        .content_type(content_type)
        .content_length(body.len());

    // tokio implements AsyncRead for `&[u8]` directly, so the body can be
    // streamed to php-fpm's stdin without any extra wrapping.
    let body_slice: &[u8] = &body;

    let result = match target {
        FpmTarget::Unix(path) => match UnixStream::connect(&path).await {
            Ok(stream) => {
                let client = Client::new(stream);
                client.execute_once(FcgiRequest::new(params, body_slice)).await
            }
            Err(e) => {
                error!("Failed to connect to php-fpm unix socket {path}: {e}");
                return (StatusCode::BAD_GATEWAY, "Cannot reach PHP-FPM").into_response();
            }
        },
        FpmTarget::Tcp(host, port) => match TcpStream::connect((host.as_str(), port)).await {
            Ok(stream) => {
                let client = Client::new(stream);
                client.execute_once(FcgiRequest::new(params, body_slice)).await
            }
            Err(e) => {
                error!("Failed to connect to php-fpm at {host}:{port}: {e}");
                return (StatusCode::BAD_GATEWAY, "Cannot reach PHP-FPM").into_response();
            }
        },
    };

    match result {
        Ok(output) => build_response(output),
        Err(e) => {
            error!("FastCGI execution failed: {e}");
            (StatusCode::BAD_GATEWAY, "PHP-FPM execution failed").into_response()
        }
    }
}

/// Turn a raw CGI response (headers + body, separated by a blank line) into
/// a proper axum `Response`, honoring an optional `Status:` header.
fn build_response(output: fastcgi_client::Response) -> Response {
    let stdout = output.stdout.unwrap_or_default();

    if let Some(stderr) = &output.stderr {
        if !stderr.is_empty() {
            warn!("php-fpm stderr: {}", String::from_utf8_lossy(stderr));
        }
    }

    const SEP: &[u8] = b"\r\n\r\n";
    let split_at = stdout.windows(SEP.len()).position(|w| w == SEP);

    let (header_block, body): (&[u8], Vec<u8>) = match split_at {
        Some(pos) => (&stdout[..pos], stdout[pos + SEP.len()..].to_vec()),
        None => (&[][..], stdout.clone()),
    };

    let mut status = StatusCode::OK;
    let mut header_map = HeaderMap::new();

    for line in String::from_utf8_lossy(header_block).lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();

        if name.eq_ignore_ascii_case("status") {
            if let Some(code_str) = value.split_whitespace().next() {
                if let Ok(code) = code_str.parse::<u16>() {
                    if let Ok(s) = StatusCode::from_u16(code) {
                        status = s;
                    }
                }
            }
            continue;
        }

        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            header_map.append(hn, hv);
        }
    }

    (status, header_map, body).into_response()
}
