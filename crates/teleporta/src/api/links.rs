//! The public link router — the heart of Teleporta.
//!
//! Every non-reserved path lands here. We resolve the link, record a click,
//! and render the fallback page. Crucially, when the app is installed the OS
//! intercepts the URL and opens the app before this handler is ever reached;
//! this code only runs on the browser fallback path.

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
};
use chrono::Utc;
use serde_json::{Map, Value};

use crate::{decide, detect_platform, normalize_path, DestinationType, Platform};

use crate::click_log::{self, ClickContext};
use crate::error::AppResult;
use crate::fallback;
use crate::resolver::resolve;
use crate::state::SharedState;

/// Handler for the site root (`/`).
pub async fn handle_root(
    state: State<SharedState>,
    query: RawQuery,
    connect_info: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    serve(state, "/".to_string(), query, connect_info, headers).await
}

/// Handler for every other path (`/{*path}`).
pub async fn handle_link(
    state: State<SharedState>,
    Path(path): Path<String>,
    query: RawQuery,
    connect_info: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    serve(state, path, query, connect_info, headers).await
}

async fn serve(
    State(state): State<SharedState>,
    raw_path: String,
    RawQuery(query): RawQuery,
    ConnectInfo(socket): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    let path = normalize_path(&raw_path);

    let user_agent = header_str(&headers, axum::http::header::USER_AGENT);
    let referrer = header_str(&headers, axum::http::header::REFERER);
    let platform = user_agent
        .as_deref()
        .map(detect_platform)
        .unwrap_or(Platform::Other);
    let client_ip = client_ip(&headers, socket);
    let query_params = parse_query(query.as_deref());

    let resolution = resolve(&state.pool, &state.cache, &path, Utc::now()).await?;

    let (status, body) = match resolution {
        Some(link) => {
            let decision = decide(&link, platform);
            click_log::record(
                state.clone(),
                ClickContext {
                    link_id: Some(link.id),
                    request_path: path.clone(),
                    query_params,
                    user_agent,
                    referrer,
                    platform,
                    destination_type: decision.destination_type,
                    client_ip,
                },
            );
            (
                StatusCode::OK,
                fallback::render_found(&state.config, &link, &decision),
            )
        }
        None => {
            click_log::record(
                state.clone(),
                ClickContext {
                    link_id: None,
                    request_path: path.clone(),
                    query_params,
                    user_agent,
                    referrer,
                    platform,
                    destination_type: DestinationType::None,
                    client_ip,
                },
            );
            (
                StatusCode::NOT_FOUND,
                fallback::render_not_found(&state.config, platform),
            )
        }
    };

    Ok((status, Html(body)))
}

fn header_str(headers: &HeaderMap, name: axum::http::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// Best-effort client IP. Honors the first hop of `X-Forwarded-For` (for
/// deployments behind a load balancer / reverse proxy), otherwise the socket
/// peer address.
fn client_ip(headers: &HeaderMap, socket: SocketAddr) -> Option<IpAddr> {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = xff.split(',').next() {
            if let Ok(ip) = first.trim().parse::<IpAddr>() {
                return Some(ip);
            }
        }
    }
    Some(socket.ip())
}

/// Parse a raw query string into a flat JSON object for storage. Duplicate
/// keys keep the last value; this is operational telemetry, not a faithful
/// round-trip of the query string.
fn parse_query(query: Option<&str>) -> Value {
    let mut map = Map::new();
    if let Some(q) = query {
        for (k, v) in form_urlencoded::parse(q.as_bytes()) {
            map.insert(k.into_owned(), Value::String(v.into_owned()));
        }
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_builds_object() {
        let v = parse_query(Some("source=qr&campaign=summer"));
        assert_eq!(v["source"], "qr");
        assert_eq!(v["campaign"], "summer");
    }

    #[test]
    fn parse_query_empty() {
        assert_eq!(parse_query(None), Value::Object(Map::new()));
        assert_eq!(parse_query(Some("")), Value::Object(Map::new()));
    }

    #[test]
    fn xff_is_preferred_over_socket() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "198.51.100.9, 10.0.0.1".parse().unwrap());
        let socket: SocketAddr = "10.0.0.1:5000".parse().unwrap();
        assert_eq!(
            client_ip(&headers, socket),
            Some("198.51.100.9".parse().unwrap())
        );
    }

    #[test]
    fn socket_used_without_xff() {
        let headers = HeaderMap::new();
        let socket: SocketAddr = "203.0.113.5:443".parse().unwrap();
        assert_eq!(client_ip(&headers, socket), Some("203.0.113.5".parse().unwrap()));
    }
}
