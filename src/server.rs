//! HTTP + WebSocket API consumed by the browser SDK.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use crate::config::Config;
use crate::models::{PrintRequest, Printer};
use crate::print::PrintService;
use crate::registry::PrinterRegistry;

/// Base64 inflates documents by ~33%; this admits PDFs up to roughly 48 MB.
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
const WS_PING_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub registry: Arc<PrinterRegistry>,
    pub printing: Arc<PrintService>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/printers", get(list_printers))
        .route("/api/printers/refresh", post(refresh_printers))
        .route("/api/print", post(print))
        .route("/ws", get(websocket))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(cors_layer(&state.config))
        // Outermost: reject rebound hostnames before anything else runs.
        .layer(middleware::from_fn(require_localhost_host))
        .with_state(state)
}

fn cors_layer(config: &Config) -> CorsLayer {
    let origin = if config.allows_any_origin() {
        AllowOrigin::any()
    } else {
        let origins: Vec<HeaderValue> = config
            .allowed_origins
            .iter()
            .filter_map(|o| HeaderValue::from_str(o.trim().trim_end_matches('/')).ok())
            .collect();
        AllowOrigin::list(origins)
    };
    CorsLayer::new()
        .allow_origin(origin)
        .allow_methods(Any)
        .allow_headers(Any)
        // Chrome's Private Network Access preflight: lets public HTTPS sites reach
        // the loopback agent (still subject to the origin list above).
        .allow_private_network(true)
}

/// DNS-rebinding protection: a malicious site can point evil.example.com at 127.0.0.1
/// and bypass CORS (it becomes same-origin). Rejecting non-localhost Host headers
/// closes that hole.
async fn require_localhost_host(request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    if is_localhost(host) {
        next.run(request).await
    } else {
        (
            StatusCode::FORBIDDEN,
            "PrintBridge only accepts localhost requests.",
        )
            .into_response()
    }
}

/// True for `localhost`, `127.0.0.1`, or `[::1]`, with or without a port.
fn is_localhost(host_header: &str) -> bool {
    let host = if let Some(rest) = host_header.strip_prefix('[') {
        match rest.split_once(']') {
            Some((inner, _)) => return inner == "::1",
            None => return false,
        }
    } else {
        host_header.split(':').next().unwrap_or_default()
    };
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1"
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "name": "PrintBridge",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    }))
}

#[derive(Deserialize)]
struct PrintersQuery {
    #[serde(default)]
    refresh: bool,
}

async fn list_printers(
    State(state): State<AppState>,
    Query(q): Query<PrintersQuery>,
) -> Json<Vec<Printer>> {
    Json(state.registry.get(q.refresh).await)
}

async fn refresh_printers(State(state): State<AppState>) -> Json<Vec<Printer>> {
    Json(state.registry.refresh().await)
}

async fn print(
    State(state): State<AppState>,
    body: Result<Json<PrintRequest>, JsonRejection>,
) -> Response {
    let request = match body {
        Ok(Json(r)) => r,
        Err(rejection) => {
            let message = rejection.body_text();
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "printerId": null, "via": "none", "message": message })),
            )
                .into_response();
        }
    };

    let result = state.printing.print(request).await;
    let status = if result.ok {
        StatusCode::OK
    } else if result.rejected {
        StatusCode::BAD_REQUEST
    } else {
        // The agent is a gateway to the printer; the printer side failed.
        StatusCode::BAD_GATEWAY
    };
    (status, Json(result)).into_response()
}

/// Pushes `{type:"printers", printers:[…]}` on connect and after every discovery
/// pass; a client message containing "refresh" triggers a rescan.
async fn websocket(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    // Browsers don't apply CORS to WebSockets, so the origin list is enforced here.
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|o| o.to_str().ok())
        .unwrap_or_default();
    if !state.config.allows_any_origin() && !state.config.origin_allowed(origin) {
        return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
    }
    upgrade.on_upgrade(move |socket| serve_socket(socket, state))
}

async fn serve_socket(socket: WebSocket, state: AppState) {
    let (mut tx, mut rx) = socket.split();
    let mut changes = state.registry.subscribe();

    let snapshot = |printers: Vec<Printer>| {
        Message::Text(
            json!({ "type": "printers", "printers": printers })
                .to_string()
                .into(),
        )
    };
    if tx.send(snapshot(state.registry.current())).await.is_err() {
        return;
    }

    let mut ping = tokio::time::interval(WS_PING_INTERVAL);
    ping.tick().await; // first tick is immediate
    loop {
        tokio::select! {
            change = changes.recv() => match change {
                Ok(printers) => {
                    if tx.send(snapshot(printers)).await.is_err() { break; }
                }
                // Fell behind: just send the latest state.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    if tx.send(snapshot(state.registry.current())).await.is_err() { break; }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            incoming = rx.next() => match incoming {
                Some(Ok(Message::Text(text))) if text.to_ascii_lowercase().contains("refresh") => {
                    let registry = state.registry.clone();
                    tokio::spawn(async move { registry.refresh().await });
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            _ = ping.tick() => {
                if tx.send(Message::Ping(Default::default())).await.is_err() { break; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_loopback_hosts() {
        for ok in [
            "localhost",
            "localhost:9631",
            "LOCALHOST:1",
            "127.0.0.1",
            "127.0.0.1:9631",
            "[::1]",
            "[::1]:9631",
        ] {
            assert!(is_localhost(ok), "{ok}");
        }
        for bad in [
            "",
            "evil.example.com",
            "127.0.0.1.evil.com",
            "localhost.evil.com:9631",
            "[::2]:9631",
            "[::1",
            "10.0.0.1",
        ] {
            assert!(!is_localhost(bad), "{bad}");
        }
    }
}
