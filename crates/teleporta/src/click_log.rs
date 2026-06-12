//! Lightweight operational click logging.
//!
//! This exists for operations and debugging — confirming QR codes are scanned,
//! spotting invalid paths, investigating abuse — and explicitly NOT for
//! attribution. Inserts run on a detached task so logging never adds latency
//! to (or fails) the user-facing redirect/fallback.
//!
//! Privacy is configurable: by default the raw IP is dropped and only a salted
//! hash is stored.

use std::net::IpAddr;

use ipnetwork::IpNetwork;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{DestinationType, Platform};

use crate::config::PrivacyConfig;
use crate::state::SharedState;

/// Everything we know about a single inbound click.
pub struct ClickContext {
    /// The matched link, if any. `None` for unknown paths.
    pub link_id: Option<Uuid>,
    pub request_path: String,
    pub query_params: Value,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    pub platform: Platform,
    pub destination_type: DestinationType,
    /// Best-effort client IP (from `X-Forwarded-For` or the socket).
    pub client_ip: Option<IpAddr>,
}

/// Record a click without blocking the request. Spawns a detached insert.
pub fn record(state: SharedState, ctx: ClickContext) {
    tokio::spawn(async move {
        if let Err(e) = insert(&state, ctx).await {
            // A logging failure must never be visible to the user; just trace.
            tracing::warn!(error = %e, "failed to record click event");
        }
    });
}

async fn insert(state: &SharedState, ctx: ClickContext) -> Result<(), sqlx::Error> {
    let ip_hash = ctx
        .client_ip
        .filter(|_| state.config.privacy.hash_ip)
        .map(|ip| hash_ip(&state.config.privacy, ip));

    let raw_ip: Option<IpNetwork> = ctx
        .client_ip
        .filter(|_| state.config.privacy.store_raw_ip)
        .map(IpNetwork::from);

    sqlx::query(
        "INSERT INTO link_clicks \
            (link_id, request_path, query_params, user_agent, referrer, platform, \
             destination_type, ip_hash, raw_ip) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(ctx.link_id)
    .bind(&ctx.request_path)
    .bind(&ctx.query_params)
    .bind(&ctx.user_agent)
    .bind(&ctx.referrer)
    .bind(ctx.platform.as_str())
    .bind(ctx.destination_type.as_str())
    .bind(&ip_hash)
    .bind(raw_ip)
    .execute(&state.pool)
    .await?;

    Ok(())
}

/// Salted SHA-256 of the client IP, hex-encoded. Not reversible without the
/// salt; stable for the same (salt, ip) pair so repeat visits can be grouped.
fn hash_ip(privacy: &PrivacyConfig, ip: IpAddr) -> String {
    let mut hasher = Sha256::new();
    hasher.update(privacy.ip_hash_salt.as_bytes());
    hasher.update(ip.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn privacy() -> PrivacyConfig {
        PrivacyConfig {
            store_raw_ip: false,
            hash_ip: true,
            ip_hash_salt: "salt".into(),
        }
    }

    #[test]
    fn hash_is_stable_and_salted() {
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        let h1 = hash_ip(&privacy(), ip);
        let h2 = hash_ip(&privacy(), ip);
        assert_eq!(h1, h2, "same salt+ip must hash identically");
        assert_eq!(h1.len(), 64, "sha256 hex is 64 chars");

        let mut other = privacy();
        other.ip_hash_salt = "different".into();
        assert_ne!(h1, hash_ip(&other, ip), "salt must change the hash");
    }
}
