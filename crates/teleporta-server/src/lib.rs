//! Teleporta server: HTTP app-link router with PostgreSQL source-of-truth,
//! Redis cache, app-link verification endpoints, fallback rendering, and
//! operational click logging.

pub mod api;
pub mod cache;
pub mod click_log;
pub mod config;
pub mod db;
pub mod error;
pub mod fallback;
pub mod resolver;
pub mod state;

pub use config::Config;

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::info;

/// Wire up dependencies and serve until a shutdown signal arrives.
pub async fn run(cfg: Config) -> anyhow::Result<()> {
    let pool = db::connect(&cfg.database, &cfg.pool).await?;
    db::migrate(&pool).await?;
    info!("database connected and migrations applied");

    let cache = cache::Cache::connect(&cfg.redis).await?;
    info!("redis cache connected");

    let http_addr = cfg.http_addr.clone();
    let state = Arc::new(state::AppState::new(cfg, pool, cache));
    let app = api::router(state);

    let addr: SocketAddr = http_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid server address {http_addr}: {e}"))?;
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "teleporta-server listening");

    // ConnectInfo lets the link handler see the socket peer address for click
    // logging (used as a fallback when no X-Forwarded-For header is present).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    info!("shutdown signal received");
}
