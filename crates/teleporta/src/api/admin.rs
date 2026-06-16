//! Admin CRUD API for link (deeplink) definitions.
//!
//! The public router (`links.rs`) only *resolves* links; this module is the
//! write side. Operators can create, read, update, list, and delete links over
//! HTTP under `/v1/links`, mirroring the resource-style API of comparable
//! Rust services (e.g. ferra's `/v1/kv`).
//!
//! A link is addressed by its normalized path. Because link paths themselves
//! contain slashes (`/v/123456`), the single-resource routes capture the rest
//! of the URL with a `*path` wildcard: `GET /v1/links/v/123456` operates on the
//! link whose path is `/v/123456`.
//!
//! NOTE: these endpoints mutate routing for the entire deployment and are NOT
//! authenticated by Teleporta itself. Expose them only on a trusted network or
//! behind an authenticating reverse proxy.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::link::{normalize_path, Link};
use crate::resolver::{LinkRow, LINK_COLUMNS};
use crate::state::SharedState;

/// Request body for `POST /v1/links`.
#[derive(Debug, Deserialize)]
pub struct CreateLink {
    /// The link path, e.g. `/v/123456`. Normalized before storage.
    pub path: String,
    pub route_type: String,
    #[serde(default)]
    pub web_fallback_url: Option<String>,
    #[serde(default)]
    pub ios_store_url: Option<String>,
    #[serde(default)]
    pub android_store_url: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    /// Defaults to `true` when omitted.
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub created_by: Option<String>,
}

/// Request body for `PUT /v1/links/*path`.
///
/// PUT replaces the mutable fields wholesale: any optional field omitted from
/// the body is cleared (set to NULL / its default). The path comes from the
/// URL and is never changed by an update.
#[derive(Debug, Deserialize)]
pub struct UpdateLink {
    pub route_type: String,
    #[serde(default)]
    pub web_fallback_url: Option<String>,
    #[serde(default)]
    pub ios_store_url: Option<String>,
    #[serde(default)]
    pub android_store_url: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    /// Defaults to `true` when omitted.
    #[serde(default)]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// Query parameters for `GET /v1/links` (list).
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub route_type: Option<String>,
    pub is_active: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Response body for `GET /v1/links` (list).
#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub items: Vec<Link>,
    pub count: usize,
}

/// `POST /v1/links` — create a link. Returns `201 Created` with the stored
/// link, or `409 Conflict` if the path already exists.
pub async fn create_link(
    State(state): State<SharedState>,
    Json(body): Json<CreateLink>,
) -> AppResult<impl IntoResponse> {
    let path = normalize_path(&body.path);
    validate_path(&path)?;
    validate_route_type(&body.route_type)?;
    let metadata = body.metadata.unwrap_or_else(|| json!({}));
    let is_active = body.is_active.unwrap_or(true);

    let row = sqlx::query_as::<_, LinkRow>(&format!(
        "INSERT INTO links \
           (path, route_type, web_fallback_url, ios_store_url, android_store_url, \
            metadata, is_active, expires_at, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         RETURNING {LINK_COLUMNS}"
    ))
    .bind(&path)
    .bind(&body.route_type)
    .bind(&body.web_fallback_url)
    .bind(&body.ios_store_url)
    .bind(&body.android_store_url)
    .bind(&metadata)
    .bind(is_active)
    .bind(body.expires_at)
    .bind(&body.created_by)
    .fetch_one(&state.pool)
    .await
    .map_err(unique_violation_to_conflict)?;

    let link: Link = row.into();
    // A path that was looked up before it existed may carry a negative-cache
    // entry; clear both caches so it starts resolving immediately.
    state.cache.invalidate(&link.path).await;

    Ok((StatusCode::CREATED, Json(link)))
}

/// `GET /v1/links/*path` — fetch a single link by path.
pub async fn get_link(
    State(state): State<SharedState>,
    Path(raw_path): Path<String>,
) -> AppResult<Json<Link>> {
    let path = normalize_path(&raw_path);
    let row = sqlx::query_as::<_, LinkRow>(&format!(
        "SELECT {LINK_COLUMNS} FROM links WHERE path = $1"
    ))
    .bind(&path)
    .fetch_optional(&state.pool)
    .await?;

    row.map(|r| Json(r.into())).ok_or(AppError::NotFound)
}

/// `PUT /v1/links/*path` — replace a link's mutable fields. `404` if absent.
pub async fn update_link(
    State(state): State<SharedState>,
    Path(raw_path): Path<String>,
    Json(body): Json<UpdateLink>,
) -> AppResult<Json<Link>> {
    let path = normalize_path(&raw_path);
    validate_route_type(&body.route_type)?;
    let metadata = body.metadata.unwrap_or_else(|| json!({}));
    let is_active = body.is_active.unwrap_or(true);

    let row = sqlx::query_as::<_, LinkRow>(&format!(
        "UPDATE links SET \
           route_type = $2, web_fallback_url = $3, ios_store_url = $4, \
           android_store_url = $5, metadata = $6, is_active = $7, \
           expires_at = $8, updated_at = now() \
         WHERE path = $1 \
         RETURNING {LINK_COLUMNS}"
    ))
    .bind(&path)
    .bind(&body.route_type)
    .bind(&body.web_fallback_url)
    .bind(&body.ios_store_url)
    .bind(&body.android_store_url)
    .bind(&metadata)
    .bind(is_active)
    .bind(body.expires_at)
    .fetch_optional(&state.pool)
    .await?;

    match row {
        Some(r) => {
            let link: Link = r.into();
            state.cache.invalidate(&link.path).await;
            Ok(Json(link))
        }
        None => Err(AppError::NotFound),
    }
}

/// `DELETE /v1/links/*path` — delete a link. `404` if it did not exist.
pub async fn delete_link(
    State(state): State<SharedState>,
    Path(raw_path): Path<String>,
) -> AppResult<impl IntoResponse> {
    let path = normalize_path(&raw_path);
    let result = sqlx::query("DELETE FROM links WHERE path = $1")
        .bind(&path)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    state.cache.invalidate(&path).await;
    Ok((StatusCode::OK, Json(json!({ "deleted": path }))))
}

/// `GET /v1/links` — list links, newest first, with optional `route_type` /
/// `is_active` filters and `limit` (default 100, max 1000) / `offset` paging.
pub async fn list_links(
    State(state): State<SharedState>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<ListResponse>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let offset = q.offset.unwrap_or(0).max(0);

    let mut builder =
        sqlx::QueryBuilder::new(format!("SELECT {LINK_COLUMNS} FROM links WHERE TRUE"));
    if let Some(route_type) = &q.route_type {
        builder.push(" AND route_type = ").push_bind(route_type.clone());
    }
    if let Some(is_active) = q.is_active {
        builder.push(" AND is_active = ").push_bind(is_active);
    }
    builder
        .push(" ORDER BY created_at DESC LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);

    let rows = builder
        .build_query_as::<LinkRow>()
        .fetch_all(&state.pool)
        .await?;

    let items: Vec<Link> = rows.into_iter().map(Into::into).collect();
    let count = items.len();
    Ok(Json(ListResponse { items, count }))
}

/// Reject an empty/root path. `normalize_path` guarantees the leading slash, so
/// `"/"` is the normalized form of an empty path.
fn validate_path(path: &str) -> AppResult<()> {
    if path == "/" {
        return Err(AppError::BadRequest("path must not be empty".into()));
    }
    if path.len() > 2048 {
        return Err(AppError::BadRequest("path too long (max 2048 chars)".into()));
    }
    Ok(())
}

fn validate_route_type(route_type: &str) -> AppResult<()> {
    if route_type.trim().is_empty() {
        return Err(AppError::BadRequest("route_type must not be empty".into()));
    }
    if route_type.len() > 128 {
        return Err(AppError::BadRequest(
            "route_type too long (max 128 chars)".into(),
        ));
    }
    Ok(())
}

/// Map a Postgres unique-violation (duplicate `path`) to a `409 Conflict`;
/// every other database error stays a `500`.
fn unique_violation_to_conflict(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &e {
        if db.is_unique_violation() {
            return AppError::Conflict("a link with this path already exists".into());
        }
    }
    AppError::Sqlx(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_path_rejects_root_and_long() {
        assert!(validate_path("/").is_err());
        assert!(validate_path("/v/123456").is_ok());
        assert!(validate_path(&format!("/{}", "a".repeat(3000))).is_err());
    }

    #[test]
    fn validate_route_type_rejects_blank_and_long() {
        assert!(validate_route_type("").is_err());
        assert!(validate_route_type("   ").is_err());
        assert!(validate_route_type("vehicle").is_ok());
        assert!(validate_route_type(&"x".repeat(200)).is_err());
    }
}
