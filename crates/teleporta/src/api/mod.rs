//! HTTP routing.

pub mod admin;
pub mod health;
pub mod links;
pub mod well_known;

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::{DefaultOnRequest, DefaultOnResponse, MakeSpan, TraceLayer},
};
use tracing::{Level, Span};

use crate::state::SharedState;

/// Builds the per-request tracing span — the structured access log. The span
/// is named `request` and carries the method, URI, and a request id; once it is
/// open, `tower_http`'s `on_response` event logs latency and status under it,
/// producing lines like:
///
/// ```text
/// INFO request{method=GET uri=/v1/links request_id=43211e5c-...}: tower_http::trace::on_response: finished processing request latency=56 ms status=200
/// ```
#[derive(Clone, Copy, Debug)]
struct RequestIdSpan;

impl<B> MakeSpan<B> for RequestIdSpan {
    fn make_span(&mut self, request: &axum::http::Request<B>) -> Span {
        // `SetRequestIdLayer` runs before us and guarantees the header is set,
        // but fall back to "-" so the span never panics if that changes.
        let request_id = request
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-");
        tracing::info_span!(
            "request",
            method = %request.method(),
            uri = %request.uri(),
            request_id = %request_id,
        )
    }
}

pub fn router(state: SharedState) -> Router {
    // Health probes are kept off the trace layer: k8s liveness/readiness hit
    // them every few seconds and per-request logs would drown the signal.
    let health = Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .with_state(state.clone());

    // Admin CRUD API for managing link definitions (the write side).
    let admin = Router::new()
        .route("/v1/links", post(admin::create_link).get(admin::list_links))
        .route(
            "/v1/links/*path",
            get(admin::get_link)
                .put(admin::update_link)
                .delete(admin::delete_link),
        )
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
        .with_state(state);

    // In axum, the LAST `.layer` applied is the OUTERMOST wrapper. So
    // SetRequestIdLayer below runs first (assigning x-request-id when the
    // client didn't supply one), then TraceLayer makes the span that captures
    // it, then PropagateRequestIdLayer copies the id onto the response.
    //
    // The admin routes are merged BEFORE the public `/*path` catch-all so that
    // `/v1/links...` is matched by the CRUD handlers rather than swallowed by
    // the link resolver. Health is merged afterwards, outside the trace layer.
    let traced = Router::new()
        .merge(admin)
        .merge(public)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(RequestIdSpan)
                .on_request(DefaultOnRequest::new().level(Level::DEBUG))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));

    Router::new().merge(health).merge(traced)
}
