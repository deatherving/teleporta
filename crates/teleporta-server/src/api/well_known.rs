//! App-link verification endpoints fetched by iOS and Android.
//!
//! Both must be served as `application/json`. The AASA path deliberately has
//! no file extension — that is the path iOS fetches.

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};

use teleporta_core::well_known;

use crate::state::SharedState;

/// `GET /.well-known/apple-app-site-association`
pub async fn apple_app_site_association(State(state): State<SharedState>) -> impl IntoResponse {
    match &state.config.ios {
        Some(ios) => {
            let doc = well_known::apple_app_site_association(&ios.team_id, &ios.bundle_id);
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                Json(doc),
            )
                .into_response()
        }
        None => not_configured("iOS"),
    }
}

/// `GET /.well-known/assetlinks.json`
pub async fn assetlinks_json(State(state): State<SharedState>) -> impl IntoResponse {
    match &state.config.android {
        Some(android) => {
            let doc = well_known::assetlinks_json(
                &android.package_name,
                &android.sha256_cert_fingerprints,
            );
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                Json(doc),
            )
                .into_response()
        }
        None => not_configured("Android"),
    }
}

fn not_configured(platform: &str) -> axum::response::Response {
    tracing::debug!(%platform, "verification document requested but platform is not configured");
    StatusCode::NOT_FOUND.into_response()
}
