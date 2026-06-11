//! The link model and path normalization.
//!
//! A link is identified by its normalized path (e.g. `/v/123456`). Teleporta
//! treats the path as opaque — it does not know that `v` means "vehicle" or
//! that `123456` is a vehicle number. The mobile app owns parsing and routing.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// A resolved link definition. This is the source-of-truth shape stored in
/// PostgreSQL and cached (as JSON) in Redis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub id: Uuid,
    /// Normalized request path, e.g. `/v/123456`. Unique per link.
    pub path: String,
    /// Operator-defined category such as `vehicle`, `promo`, `referral`. Opaque
    /// to Teleporta; useful only for grouping and operational reporting.
    pub route_type: String,
    /// Where desktop browsers (and platforms without a store URL) should land.
    pub web_fallback_url: Option<String>,
    /// iOS App Store URL used when the app is not installed.
    pub ios_store_url: Option<String>,
    /// Android Play Store URL used when the app is not installed.
    pub android_store_url: Option<String>,
    /// Opaque application metadata. Stored and returned verbatim; Teleporta
    /// never performs business authorization based on it.
    #[serde(default)]
    pub metadata: Value,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Link {
    /// A link is expired if it has an `expires_at` in the past (inclusive).
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|e| e <= now)
    }

    /// A link is resolvable (eligible to be served) when it is active and not
    /// expired. Inactive or expired links are treated as "not found" so they
    /// fall through to the generic fallback rather than routing.
    pub fn is_resolvable(&self, now: DateTime<Utc>) -> bool {
        self.is_active && !self.is_expired(now)
    }
}

/// Normalize a request path into the canonical form used as the link key.
///
/// Rules:
/// * leading slash is guaranteed,
/// * a trailing slash is stripped (except for the root `/`),
/// * surrounding whitespace is trimmed.
///
/// Query strings must be stripped by the caller before normalization; this
/// function operates on the path component only.
pub fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    let with_leading = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };

    if with_leading.len() > 1 {
        let stripped = with_leading.trim_end_matches('/');
        if stripped.is_empty() {
            "/".to_string()
        } else {
            stripped.to_string()
        }
    } else {
        with_leading
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn link_with_expiry(expires_at: Option<DateTime<Utc>>, active: bool) -> Link {
        Link {
            id: Uuid::nil(),
            path: "/v/1".into(),
            route_type: "vehicle".into(),
            web_fallback_url: None,
            ios_store_url: None,
            android_store_url: None,
            metadata: Value::Null,
            is_active: active,
            expires_at,
            created_by: None,
            created_at: Utc.timestamp_opt(0, 0).unwrap(),
            updated_at: Utc.timestamp_opt(0, 0).unwrap(),
        }
    }

    #[test]
    fn normalize_adds_leading_slash() {
        assert_eq!(normalize_path("v/123456"), "/v/123456");
        assert_eq!(normalize_path("/v/123456"), "/v/123456");
    }

    #[test]
    fn normalize_strips_trailing_slash() {
        assert_eq!(normalize_path("/promo/summer/"), "/promo/summer");
        assert_eq!(normalize_path("/v/123456///"), "/v/123456");
    }

    #[test]
    fn normalize_root_and_empty() {
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path(""), "/");
        assert_eq!(normalize_path("   "), "/");
        assert_eq!(normalize_path("///"), "/");
    }

    #[test]
    fn normalize_trims_whitespace() {
        assert_eq!(normalize_path("  /ref/abc  "), "/ref/abc");
    }

    #[test]
    fn expiry_and_resolvability() {
        let now = Utc.timestamp_opt(1_000, 0).unwrap();
        let past = Utc.timestamp_opt(500, 0).unwrap();
        let future = Utc.timestamp_opt(2_000, 0).unwrap();

        assert!(link_with_expiry(Some(past), true).is_expired(now));
        assert!(!link_with_expiry(Some(future), true).is_expired(now));
        assert!(!link_with_expiry(None, true).is_expired(now));

        assert!(link_with_expiry(Some(future), true).is_resolvable(now));
        assert!(!link_with_expiry(Some(past), true).is_resolvable(now));
        assert!(!link_with_expiry(Some(future), false).is_resolvable(now));
    }
}
