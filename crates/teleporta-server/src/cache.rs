//! Redis-backed link cache.
//!
//! PostgreSQL remains the source of truth; Redis only accelerates resolution
//! and absorbs traffic for unknown paths via a short-lived negative cache.
//!
//! Cache operations are intentionally fault-tolerant: a Redis error is logged
//! and treated as a miss rather than failing the request. A momentary Redis
//! outage degrades to "always hit Postgres", never to "links stop working".
//!
//! Key layout (`v1` allows a future schema bump to invalidate everything):
//! ```text
//! {prefix}:link:v1:{path}        -> JSON-encoded Link  (positive cache)
//! {prefix}:link-miss:v1:{path}   -> "1"                (negative cache)
//! ```

use std::time::Duration;

use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use teleporta_core::Link;

use crate::config::RedisConfig;

#[derive(Clone)]
pub struct Cache {
    conn: ConnectionManager,
    key_prefix: String,
    link_ttl_secs: u64,
    negative_ttl_secs: u64,
}

impl Cache {
    /// Connect to Redis and build a managed (auto-reconnecting) connection.
    pub async fn connect(cfg: &RedisConfig) -> anyhow::Result<Self> {
        let client = redis::Client::open(cfg.url.as_str())
            .map_err(|e| anyhow::anyhow!("invalid TELEPORTA_REDIS_URL: {e}"))?;
        let conn = ConnectionManager::new(client)
            .await
            .map_err(|e| anyhow::anyhow!("connect to Redis at {}: {e}", cfg.url))?;
        Ok(Self {
            conn,
            key_prefix: cfg.key_prefix.clone(),
            link_ttl_secs: ttl_secs(cfg.link_cache_ttl),
            negative_ttl_secs: ttl_secs(cfg.negative_cache_ttl),
        })
    }

    fn link_key(&self, path: &str) -> String {
        format!("{}:link:v1:{path}", self.key_prefix)
    }

    fn miss_key(&self, path: &str) -> String {
        format!("{}:link-miss:v1:{path}", self.key_prefix)
    }

    /// Fetch a cached link. Returns `None` on miss, decode failure, or any
    /// Redis error (all treated as "not cached").
    pub async fn get_link(&self, path: &str) -> Option<Link> {
        let key = self.link_key(path);
        let mut conn = self.conn.clone();
        let raw: Option<String> = match conn.get(&key).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, %key, "redis GET failed; treating as cache miss");
                return None;
            }
        };
        let raw = raw?;
        match serde_json::from_str::<Link>(&raw) {
            Ok(link) => Some(link),
            Err(e) => {
                tracing::warn!(error = %e, %key, "failed to decode cached link; ignoring");
                None
            }
        }
    }

    /// Cache a resolved link with the positive TTL. Best-effort.
    pub async fn put_link(&self, link: &Link) {
        let key = self.link_key(&link.path);
        let payload = match serde_json::to_string(link) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "failed to encode link for cache; skipping");
                return;
            }
        };
        let mut conn = self.conn.clone();
        let res: redis::RedisResult<()> = conn.set_ex(&key, payload, self.link_ttl_secs).await;
        if let Err(e) = res {
            tracing::warn!(error = %e, %key, "redis SETEX (link) failed; ignoring");
        }
    }

    /// Returns true if `path` is currently in the negative cache.
    pub async fn is_known_miss(&self, path: &str) -> bool {
        let key = self.miss_key(path);
        let mut conn = self.conn.clone();
        match conn.exists(&key).await {
            Ok(exists) => exists,
            Err(e) => {
                tracing::warn!(error = %e, %key, "redis EXISTS failed; treating as not-cached");
                false
            }
        }
    }

    /// Record a negative cache entry with the negative TTL. Best-effort.
    pub async fn put_miss(&self, path: &str) {
        let key = self.miss_key(path);
        let mut conn = self.conn.clone();
        let res: redis::RedisResult<()> = conn.set_ex(&key, "1", self.negative_ttl_secs).await;
        if let Err(e) = res {
            tracing::warn!(error = %e, %key, "redis SETEX (miss) failed; ignoring");
        }
    }

    /// Invalidate both the positive and negative cache entries for a path.
    /// Called when a link is created/updated/deleted via the admin API.
    pub async fn invalidate(&self, path: &str) {
        let mut conn = self.conn.clone();
        let res: redis::RedisResult<()> =
            conn.del(&[self.link_key(path), self.miss_key(path)]).await;
        if let Err(e) = res {
            tracing::warn!(error = %e, %path, "redis DEL failed; ignoring");
        }
    }
}

/// Convert a TTL to whole seconds, with a floor of 1s so we never accidentally
/// pass 0 (which Redis would reject for SET EX).
fn ttl_secs(d: Duration) -> u64 {
    d.as_secs().max(1)
}
