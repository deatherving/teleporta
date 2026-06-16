//! Link resolution: Redis-first, PostgreSQL as the source of truth.
//!
//! Flow (per the design's runtime request flow):
//! 1. Look up the positive cache in Redis.
//! 2. On miss, check the negative cache (short-circuits unknown paths).
//! 3. On miss, query PostgreSQL.
//! 4. Cache the result (positive or negative).
//!
//! Inactive and expired links are treated as "not found" so they fall through
//! to the generic fallback rather than routing into the app.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::Link;

use crate::cache::Cache;
use crate::error::AppResult;

/// The full `links` column list, in a fixed order shared by the resolver and
/// the admin CRUD API so every `SELECT`/`RETURNING` projection stays in lockstep
/// with [`LinkRow`].
pub(crate) const LINK_COLUMNS: &str =
    "id, path, route_type, web_fallback_url, ios_store_url, android_store_url, \
     metadata, is_active, expires_at, created_by, created_at, updated_at";

/// Database row shape for the `links` table. Kept separate from
/// [`crate::Link`] so the domain model stays free of any sqlx dependency;
/// the columns are identical and the conversion is mechanical.
#[derive(sqlx::FromRow)]
pub(crate) struct LinkRow {
    id: Uuid,
    path: String,
    route_type: String,
    web_fallback_url: Option<String>,
    ios_store_url: Option<String>,
    android_store_url: Option<String>,
    metadata: Value,
    is_active: bool,
    expires_at: Option<DateTime<Utc>>,
    created_by: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<LinkRow> for Link {
    fn from(r: LinkRow) -> Self {
        Link {
            id: r.id,
            path: r.path,
            route_type: r.route_type,
            web_fallback_url: r.web_fallback_url,
            ios_store_url: r.ios_store_url,
            android_store_url: r.android_store_url,
            metadata: r.metadata,
            is_active: r.is_active,
            expires_at: r.expires_at,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Resolve a normalized path into a link, or `None` if there is no resolvable
/// link for it (unknown, inactive, or expired).
///
/// `now` is injected so resolution (and its expiry check) is deterministic and
/// testable.
pub async fn resolve(
    pool: &PgPool,
    cache: &Cache,
    path: &str,
    now: DateTime<Utc>,
) -> AppResult<Option<Link>> {
    // 1. Positive cache.
    if let Some(link) = cache.get_link(path).await {
        if link.is_resolvable(now) {
            return Ok(Some(link));
        }
        // A cached-but-now-expired link: fall through to a fresh DB read.
    }

    // 2. Negative cache.
    if cache.is_known_miss(path).await {
        return Ok(None);
    }

    // 3. Source of truth.
    let row = sqlx::query_as::<_, LinkRow>(&format!(
        "SELECT {LINK_COLUMNS} FROM links WHERE path = $1"
    ))
    .bind(path)
    .fetch_optional(pool)
    .await?;

    match row.map(Link::from) {
        Some(link) if link.is_resolvable(now) => {
            // 4a. Cache the resolved link.
            cache.put_link(&link).await;
            Ok(Some(link))
        }
        _ => {
            // 4b. Either no row, or an inactive/expired row: negative-cache it.
            cache.put_miss(path).await;
            Ok(None)
        }
    }
}
