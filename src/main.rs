//! PrintBridge — a local agent that lets web apps discover and print to every printer
//! reachable from the user's machine, via a loopback-only HTTP + WebSocket API.

mod config;
mod discovery;
mod ipp;
mod models;
mod print;
mod registry;
mod server;
mod sys;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::print::PrintService;
use crate::registry::PrinterRegistry;
use crate::server::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Arc::new(Config::parse());

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,mdns_sd=warn")),
        )
        .init();

    if config.allows_any_origin() {
        tracing::warn!(
            "CORS allows any origin: every website this user visits can list printers and print. \
             Set --allowed-origins / PRINTBRIDGE_ALLOWED_ORIGINS for production."
        );
    }

    let registry = Arc::new(PrinterRegistry::new(config.clone()));
    let printing = Arc::new(PrintService::new(registry.clone())?);
    let state = AppState {
        config: config.clone(),
        registry: registry.clone(),
        printing,
    };

    // Bind loopback only: the bridge must never be reachable from other machines.
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("could not listen on {addr} — is PrintBridge already running?"))?;
    tracing::info!(
        "PrintBridge {} listening on http://{addr}",
        env!("CARGO_PKG_VERSION")
    );

    // Warm the cache in the background so the first SDK call is instant.
    tokio::spawn(async move { registry.refresh().await });

    axum::serve(listener, server::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}
