//! HTTP routing.

pub mod health;
pub mod links;
pub mod well_known;

use axum::{routing::get, Router};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::state::SharedState;

pub fn router(state: SharedState) -> Router {
    // Health probes are kept off the trace layer: k8s liveness/readiness hit
    // them every few seconds and per-request logs would drown the signal.
    let health = Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .with_state(state.clone());

    // Reserved verification endpoints are registered as static routes so they
    // take precedence over the `/*path` wildcard catch-all.
    let public = Router::new()
        .route(
            "/.well-known/apple-app-site-association",
            get(well_known::apple_app_site_association),
        )
        .route(
            "/.well-known/assetlinks.json",
            get(well_known::assetlinks_json),
        )
        .route("/", get(links::handle_root))
        .route("/*path", get(links::handle_link))
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    Router::new().merge(health).merge(public)
}
