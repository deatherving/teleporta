//! Runtime configuration, loaded entirely from environment variables.
//!
//! Every value is read from a `TELEPORTA_*` variable. There is no config file
//! parser: a single, explicit env surface keeps the deploy story uniform
//! across Docker, Kubernetes, and bare-metal, and makes misconfiguration fail
//! loudly at startup rather than silently at request time.
//!
//! The database section supports three connection modes (URL, discrete
//! password, AWS RDS/Aurora IAM), plus an env-tunable connection
//! [`PoolConfig`] whose `max_lifetime` is validated against the IAM token TTL.

use std::path::PathBuf;
use std::time::Duration;

use sqlx::postgres::PgSslMode;

/// IAM auth tokens minted by RDS are valid for 15 minutes. Connections must
/// rotate, and tokens must refresh, strictly inside that window.
const IAM_TOKEN_TTL_SECS: u64 = 15 * 60;

#[derive(Debug, Clone)]
pub struct Config {
    /// Socket address the HTTP server binds, e.g. `0.0.0.0:8080`.
    pub http_addr: String,
    /// The canonical public origin (`https://go.example.com`), used to build
    /// absolute fallback URLs when a link has none of its own.
    pub public_base_url: String,
    /// iOS app association config. `None` disables the AASA endpoint.
    pub ios: Option<IosConfig>,
    /// Android app association config. `None` disables the assetlinks endpoint.
    pub android: Option<AndroidConfig>,
    pub database: DatabaseConfig,
    pub pool: PoolConfig,
    pub redis: RedisConfig,
    pub privacy: PrivacyConfig,
    pub fallback: FallbackConfig,
}

#[derive(Debug, Clone)]
pub struct IosConfig {
    pub team_id: String,
    pub bundle_id: String,
    pub app_store_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AndroidConfig {
    pub package_name: String,
    pub sha256_cert_fingerprints: Vec<String>,
    pub play_store_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RedisConfig {
    pub url: String,
    pub key_prefix: String,
    pub link_cache_ttl: Duration,
    pub negative_cache_ttl: Duration,
}

#[derive(Debug, Clone)]
pub struct PrivacyConfig {
    /// Store the raw client IP in `link_clicks.raw_ip`. Default false.
    pub store_raw_ip: bool,
    /// Store a salted hash of the client IP in `link_clicks.ip_hash`.
    pub hash_ip: bool,
    /// Salt mixed into the IP hash. Operators MUST override the default.
    pub ip_hash_salt: String,
}

#[derive(Debug, Clone)]
pub struct FallbackConfig {
    /// If true, the fallback page auto-redirects to the chosen store/web URL.
    pub auto_redirect_to_store: bool,
    /// Delay before the auto-redirect fires, in milliseconds.
    pub auto_redirect_delay: Duration,
    /// Canonical site users are sent to when a path does not resolve. When set,
    /// unknown paths 302-redirect here instead of rendering the not-found page.
    pub home_url: Option<String>,
}

/// Postgres connection-pool tunables.
///
/// `idle_timeout` and `max_lifetime` accept `0` to mean "disabled / no cap"
/// (mapped to `None`). For IAM auth, `max_lifetime` must be set and strictly
/// below the 15-minute token TTL so physical connections rotate before their
/// auth token expires.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Option<Duration>,
    pub max_lifetime: Option<Duration>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 0,
            acquire_timeout: Duration::from_secs(5),
            idle_timeout: Some(Duration::from_secs(300)),
            max_lifetime: Some(Duration::from_secs(600)),
        }
    }
}

/// How `teleporta-server` connects to Postgres.
#[derive(Debug, Clone)]
pub enum DatabaseConfig {
    /// A libpq-style URL (`postgres://...`). Convenient for local/dev.
    Url(String),
    /// Explicit host/port/user/password fields (`auth_mode = password`).
    Discrete(DiscreteDatabase),
    /// Discrete fields plus AWS RDS/Aurora IAM auth (`auth_mode = iam`). The
    /// password is a short-lived auth token refreshed in the background.
    Iam(IamDatabase),
}

#[derive(Debug, Clone)]
pub struct DiscreteDatabase {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub user: String,
    pub password: String,
    pub ssl_mode: PgSslMode,
    pub ssl_root_cert: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct IamDatabase {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub user: String,
    pub ssl_mode: PgSslMode,
    pub ssl_root_cert: Option<PathBuf>,
    pub aws_region: String,
    /// How often to regenerate the RDS auth token and apply it via
    /// `PgPool::set_connect_options`. Must be `< IAM_TOKEN_TTL_SECS`.
    pub token_refresh_interval: Duration,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        // Bind config lives under TELEPORTA_HTTP_*, not TELEPORTA_SERVER_*: a
        // Kubernetes Service named `teleporta-server` injects the Docker-links var
        // TELEPORTA_SERVER_PORT=tcp://<clusterIP>:<port>, which would clobber the
        // bind port and fail u16 parsing at startup. Do not rename these back.
        let host = env_or("TELEPORTA_HTTP_HOST", "0.0.0.0");
        let port = parse_env("TELEPORTA_HTTP_PORT", 8080u16)?;
        let http_addr = format!("{host}:{port}");

        let public_base_url = env_or("TELEPORTA_PUBLIC_BASE_URL", "http://localhost:8080");

        let ios = parse_ios_config()?;
        let android = parse_android_config()?;
        let database = parse_database_config()?;
        let pool = parse_pool_config()?;
        validate_pool_against_database(&pool, &database)?;
        let redis = parse_redis_config()?;
        let privacy = parse_privacy_config()?;
        let fallback = parse_fallback_config()?;

        Ok(Self {
            http_addr,
            public_base_url,
            ios,
            android,
            database,
            pool,
            redis,
            privacy,
            fallback,
        })
    }
}

fn parse_ios_config() -> anyhow::Result<Option<IosConfig>> {
    let team_id = opt_env("TELEPORTA_IOS_TEAM_ID");
    let bundle_id = opt_env("TELEPORTA_IOS_BUNDLE_ID");
    match (team_id, bundle_id) {
        (None, None) => Ok(None),
        (Some(team_id), Some(bundle_id)) => Ok(Some(IosConfig {
            team_id,
            bundle_id,
            app_store_url: opt_env("TELEPORTA_IOS_APP_STORE_URL"),
        })),
        _ => anyhow::bail!(
            "incomplete iOS config: set both TELEPORTA_IOS_TEAM_ID and \
             TELEPORTA_IOS_BUNDLE_ID, or neither"
        ),
    }
}

fn parse_android_config() -> anyhow::Result<Option<AndroidConfig>> {
    let package_name = opt_env("TELEPORTA_ANDROID_PACKAGE_NAME");
    let fingerprints = opt_env("TELEPORTA_ANDROID_SHA256_CERT_FINGERPRINTS");
    match (package_name, fingerprints) {
        (None, None) => Ok(None),
        (Some(package_name), Some(raw)) => {
            let fingerprints: Vec<String> = raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if fingerprints.is_empty() {
                anyhow::bail!(
                    "TELEPORTA_ANDROID_SHA256_CERT_FINGERPRINTS is set but contains no fingerprints"
                );
            }
            Ok(Some(AndroidConfig {
                package_name,
                sha256_cert_fingerprints: fingerprints,
                play_store_url: opt_env("TELEPORTA_ANDROID_PLAY_STORE_URL"),
            }))
        }
        _ => anyhow::bail!(
            "incomplete Android config: set both TELEPORTA_ANDROID_PACKAGE_NAME and \
             TELEPORTA_ANDROID_SHA256_CERT_FINGERPRINTS, or neither"
        ),
    }
}

fn parse_redis_config() -> anyhow::Result<RedisConfig> {
    let url = env_or("TELEPORTA_REDIS_URL", "redis://localhost:6379");
    let key_prefix = env_or("TELEPORTA_REDIS_KEY_PREFIX", "teleporta");
    let link_cache_ttl = Duration::from_secs(parse_env("TELEPORTA_LINK_CACHE_TTL_SECS", 300u64)?);
    let negative_cache_ttl =
        Duration::from_secs(parse_env("TELEPORTA_NEGATIVE_CACHE_TTL_SECS", 30u64)?);
    Ok(RedisConfig {
        url,
        key_prefix,
        link_cache_ttl,
        negative_cache_ttl,
    })
}

fn parse_privacy_config() -> anyhow::Result<PrivacyConfig> {
    let store_raw_ip = bool_env("TELEPORTA_PRIVACY_STORE_RAW_IP")?;
    let hash_ip = match std::env::var("TELEPORTA_PRIVACY_HASH_IP") {
        Err(_) => true, // default on
        Ok(_) => bool_env("TELEPORTA_PRIVACY_HASH_IP")?,
    };
    let ip_hash_salt = env_or("TELEPORTA_PRIVACY_IP_HASH_SALT", "change-me");
    if hash_ip && ip_hash_salt == "change-me" {
        tracing::warn!(
            "TELEPORTA_PRIVACY_IP_HASH_SALT is the default 'change-me'; set a unique \
             salt so click-log IP hashes are not trivially reversible"
        );
    }
    Ok(PrivacyConfig {
        store_raw_ip,
        hash_ip,
        ip_hash_salt,
    })
}

fn parse_fallback_config() -> anyhow::Result<FallbackConfig> {
    let auto_redirect_to_store = bool_env("TELEPORTA_FALLBACK_AUTO_REDIRECT_TO_STORE")?;
    let delay_ms = parse_env("TELEPORTA_FALLBACK_AUTO_REDIRECT_DELAY_MS", 500u64)?;
    Ok(FallbackConfig {
        auto_redirect_to_store,
        auto_redirect_delay: Duration::from_millis(delay_ms),
        home_url: opt_env("TELEPORTA_FALLBACK_HOME_URL"),
    })
}

fn validate_pool_against_database(pool: &PoolConfig, db: &DatabaseConfig) -> anyhow::Result<()> {
    if matches!(db, DatabaseConfig::Iam(_)) {
        match pool.max_lifetime {
            None => anyhow::bail!(
                "TELEPORTA_DATABASE_POOL_MAX_LIFETIME_SECS=0 (disabled) is incompatible with \
                 IAM auth: connections must rotate before their 15-minute IAM token expires",
            ),
            Some(d) if d.as_secs() >= IAM_TOKEN_TTL_SECS => anyhow::bail!(
                "TELEPORTA_DATABASE_POOL_MAX_LIFETIME_SECS ({}) must be < {IAM_TOKEN_TTL_SECS} \
                 for IAM auth (IAM tokens expire at 15 minutes)",
                d.as_secs(),
            ),
            Some(_) => {}
        }
    }
    Ok(())
}

fn parse_pool_config() -> anyhow::Result<PoolConfig> {
    let defaults = PoolConfig::default();
    let max_connections = parse_env(
        "TELEPORTA_DATABASE_POOL_MAX_CONNECTIONS",
        defaults.max_connections,
    )?;
    if max_connections == 0 {
        anyhow::bail!("TELEPORTA_DATABASE_POOL_MAX_CONNECTIONS must be > 0");
    }
    let min_connections = parse_env(
        "TELEPORTA_DATABASE_POOL_MIN_CONNECTIONS",
        defaults.min_connections,
    )?;
    if min_connections > max_connections {
        anyhow::bail!(
            "TELEPORTA_DATABASE_POOL_MIN_CONNECTIONS ({min_connections}) must be \
             <= TELEPORTA_DATABASE_POOL_MAX_CONNECTIONS ({max_connections})",
        );
    }
    let acquire_timeout_secs = parse_env(
        "TELEPORTA_DATABASE_POOL_ACQUIRE_TIMEOUT_SECS",
        defaults.acquire_timeout.as_secs(),
    )?;
    if acquire_timeout_secs == 0 {
        anyhow::bail!("TELEPORTA_DATABASE_POOL_ACQUIRE_TIMEOUT_SECS must be > 0");
    }
    let idle_default = defaults.idle_timeout.map_or(0, |d| d.as_secs());
    let idle_secs = parse_env("TELEPORTA_DATABASE_POOL_IDLE_TIMEOUT_SECS", idle_default)?;
    let max_life_default = defaults.max_lifetime.map_or(0, |d| d.as_secs());
    let max_life_secs = parse_env("TELEPORTA_DATABASE_POOL_MAX_LIFETIME_SECS", max_life_default)?;

    Ok(PoolConfig {
        max_connections,
        min_connections,
        acquire_timeout: Duration::from_secs(acquire_timeout_secs),
        idle_timeout: (idle_secs > 0).then(|| Duration::from_secs(idle_secs)),
        max_lifetime: (max_life_secs > 0).then(|| Duration::from_secs(max_life_secs)),
    })
}

fn parse_database_config() -> anyhow::Result<DatabaseConfig> {
    let url = opt_env("TELEPORTA_DATABASE_URL");
    let auth_mode = env_or("TELEPORTA_DATABASE_AUTH_MODE", "password");
    let auth_mode = auth_mode.trim().to_ascii_lowercase();

    match auth_mode.as_str() {
        "iam" => {
            if url.is_some() {
                anyhow::bail!(
                    "TELEPORTA_DATABASE_URL and TELEPORTA_DATABASE_AUTH_MODE=iam are mutually \
                     exclusive; use the discrete TELEPORTA_DATABASE_* fields with IAM auth",
                );
            }
            let host = required_env("TELEPORTA_DATABASE_HOST")?;
            let port = parse_env("TELEPORTA_DATABASE_PORT", 5432u16)?;
            let name = required_env("TELEPORTA_DATABASE_NAME")?;
            let user = required_env("TELEPORTA_DATABASE_USER")?;
            // IAM implies a real AWS RDS/Aurora endpoint over TLS. Default to
            // `require`; reject `disable`.
            let ssl_mode = match opt_env("TELEPORTA_DATABASE_SSL_MODE") {
                Some(_) => parse_ssl_mode_env()?,
                None => PgSslMode::Require,
            };
            if matches!(ssl_mode, PgSslMode::Disable) {
                anyhow::bail!(
                    "TELEPORTA_DATABASE_SSL_MODE=disable is incompatible with IAM auth \
                     (RDS requires TLS)",
                );
            }
            let ssl_root_cert = parse_ssl_root_cert_env(ssl_mode);
            let aws_region = required_env("TELEPORTA_DATABASE_AWS_REGION")?;
            let refresh_secs =
                parse_env("TELEPORTA_DATABASE_IAM_TOKEN_REFRESH_INTERVAL_SECS", 840u64)?;
            if refresh_secs == 0 {
                anyhow::bail!("TELEPORTA_DATABASE_IAM_TOKEN_REFRESH_INTERVAL_SECS must be > 0");
            }
            if refresh_secs >= IAM_TOKEN_TTL_SECS {
                anyhow::bail!(
                    "TELEPORTA_DATABASE_IAM_TOKEN_REFRESH_INTERVAL_SECS must be < \
                     {IAM_TOKEN_TTL_SECS} (IAM tokens expire at 15 minutes)",
                );
            }
            Ok(DatabaseConfig::Iam(IamDatabase {
                host,
                port,
                name,
                user,
                ssl_mode,
                ssl_root_cert,
                aws_region,
                token_refresh_interval: Duration::from_secs(refresh_secs),
            }))
        }
        "password" => {
            if let Some(url) = url {
                return Ok(DatabaseConfig::Url(url));
            }
            let host = required_env("TELEPORTA_DATABASE_HOST").map_err(|_| {
                anyhow::anyhow!(
                    "no database configuration provided: set TELEPORTA_DATABASE_URL, or the \
                     discrete TELEPORTA_DATABASE_HOST/NAME/USER/PASSWORD fields, or set \
                     TELEPORTA_DATABASE_AUTH_MODE=iam with HOST/NAME/USER/AWS_REGION",
                )
            })?;
            let port = parse_env("TELEPORTA_DATABASE_PORT", 5432u16)?;
            let name = required_env("TELEPORTA_DATABASE_NAME")?;
            let user = required_env("TELEPORTA_DATABASE_USER")?;
            let password = required_env("TELEPORTA_DATABASE_PASSWORD")?;
            let ssl_mode = parse_ssl_mode_env()?;
            let ssl_root_cert = parse_ssl_root_cert_env(ssl_mode);
            Ok(DatabaseConfig::Discrete(DiscreteDatabase {
                host,
                port,
                name,
                user,
                password,
                ssl_mode,
                ssl_root_cert,
            }))
        }
        other => anyhow::bail!(
            "invalid TELEPORTA_DATABASE_AUTH_MODE: {other:?} (expected 'password' or 'iam')",
        ),
    }
}

fn parse_ssl_root_cert_env(ssl_mode: PgSslMode) -> Option<PathBuf> {
    let path = opt_env("TELEPORTA_DATABASE_SSL_ROOT_CERT").map(PathBuf::from);
    if path.is_none() && matches!(ssl_mode, PgSslMode::VerifyCa | PgSslMode::VerifyFull) {
        tracing::warn!(
            "TELEPORTA_DATABASE_SSL_MODE is verify-ca/verify-full but \
             TELEPORTA_DATABASE_SSL_ROOT_CERT is unset; sqlx will use the system trust \
             store. For AWS RDS/Aurora this typically fails — point this at the RDS \
             global CA bundle.",
        );
    }
    path
}

fn parse_ssl_mode_env() -> anyhow::Result<PgSslMode> {
    let normalized = opt_env("TELEPORTA_DATABASE_SSL_MODE").map(|s| s.to_ascii_lowercase());
    match normalized.as_deref() {
        None => Ok(PgSslMode::Prefer),
        Some("disable") => Ok(PgSslMode::Disable),
        Some("allow") => Ok(PgSslMode::Allow),
        Some("prefer") => Ok(PgSslMode::Prefer),
        Some("require") => Ok(PgSslMode::Require),
        Some("verify-ca") | Some("verify_ca") => Ok(PgSslMode::VerifyCa),
        Some("verify-full") | Some("verify_full") => Ok(PgSslMode::VerifyFull),
        Some(other) => Err(anyhow::anyhow!(
            "invalid TELEPORTA_DATABASE_SSL_MODE: {other:?} \
             (expected disable/allow/prefer/require/verify-ca/verify-full)",
        )),
    }
}

// --- small env helpers -----------------------------------------------------

/// Read an env var, returning `None` if unset or empty after trimming.
fn opt_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn env_or(key: &str, default: &str) -> String {
    opt_env(key).unwrap_or_else(|| default.to_string())
}

fn required_env(key: &str) -> anyhow::Result<String> {
    opt_env(key).ok_or_else(|| anyhow::anyhow!("{key} is required"))
}

fn parse_env<T>(key: &str, default: T) -> anyhow::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match opt_env(key) {
        None => Ok(default),
        Some(v) => v
            .parse::<T>()
            .map_err(|e| anyhow::anyhow!("invalid {key}: {e}")),
    }
}

fn bool_env(key: &str) -> anyhow::Result<bool> {
    match opt_env(key) {
        None => Ok(false),
        Some(v) => match v.to_ascii_lowercase().as_str() {
            "0" | "false" | "no" | "off" => Ok(false),
            "1" | "true" | "yes" | "on" => Ok(true),
            other => Err(anyhow::anyhow!(
                "invalid boolean value for {key}: {other:?} (expected true/false)",
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    const VARS: &[&str] = &[
        "TELEPORTA_HTTP_HOST",
        "TELEPORTA_HTTP_PORT",
        "TELEPORTA_PUBLIC_BASE_URL",
        "TELEPORTA_IOS_TEAM_ID",
        "TELEPORTA_IOS_BUNDLE_ID",
        "TELEPORTA_IOS_APP_STORE_URL",
        "TELEPORTA_ANDROID_PACKAGE_NAME",
        "TELEPORTA_ANDROID_SHA256_CERT_FINGERPRINTS",
        "TELEPORTA_ANDROID_PLAY_STORE_URL",
        "TELEPORTA_DATABASE_URL",
        "TELEPORTA_DATABASE_AUTH_MODE",
        "TELEPORTA_DATABASE_HOST",
        "TELEPORTA_DATABASE_PORT",
        "TELEPORTA_DATABASE_NAME",
        "TELEPORTA_DATABASE_USER",
        "TELEPORTA_DATABASE_PASSWORD",
        "TELEPORTA_DATABASE_SSL_MODE",
        "TELEPORTA_DATABASE_AWS_REGION",
        "TELEPORTA_DATABASE_IAM_TOKEN_REFRESH_INTERVAL_SECS",
        "TELEPORTA_DATABASE_POOL_MAX_CONNECTIONS",
        "TELEPORTA_DATABASE_POOL_MIN_CONNECTIONS",
        "TELEPORTA_DATABASE_POOL_MAX_LIFETIME_SECS",
        "TELEPORTA_REDIS_URL",
        "TELEPORTA_LINK_CACHE_TTL_SECS",
        "TELEPORTA_PRIVACY_STORE_RAW_IP",
        "TELEPORTA_PRIVACY_HASH_IP",
        "TELEPORTA_FALLBACK_AUTO_REDIRECT_TO_STORE",
        "TELEPORTA_FALLBACK_HOME_URL",
    ];

    fn clear() {
        for k in VARS {
            // SAFETY: tests in this module are serialized via #[serial].
            unsafe { std::env::remove_var(k) };
        }
    }

    fn set(k: &str, v: &str) {
        // SAFETY: tests in this module are serialized via #[serial].
        unsafe { std::env::set_var(k, v) };
    }

    fn base_password_env() {
        set("TELEPORTA_DATABASE_HOST", "localhost");
        set("TELEPORTA_DATABASE_NAME", "teleporta");
        set("TELEPORTA_DATABASE_USER", "teleporta");
        set("TELEPORTA_DATABASE_PASSWORD", "password");
    }

    #[test]
    #[serial]
    fn defaults_apply() {
        clear();
        base_password_env();
        let cfg = Config::from_env().unwrap();
        assert_eq!(cfg.http_addr, "0.0.0.0:8080");
        assert_eq!(cfg.public_base_url, "http://localhost:8080");
        assert_eq!(cfg.redis.url, "redis://localhost:6379");
        assert_eq!(cfg.redis.key_prefix, "teleporta");
        assert_eq!(cfg.redis.link_cache_ttl, Duration::from_secs(300));
        assert!(!cfg.privacy.store_raw_ip);
        assert!(cfg.privacy.hash_ip);
        assert!(cfg.ios.is_none());
        assert!(cfg.android.is_none());
    }

    #[test]
    #[serial]
    fn discrete_password_mode() {
        clear();
        base_password_env();
        set("TELEPORTA_DATABASE_SSL_MODE", "disable");
        let cfg = Config::from_env().unwrap();
        match cfg.database {
            DatabaseConfig::Discrete(d) => {
                assert_eq!(d.host, "localhost");
                assert_eq!(d.port, 5432);
                assert!(matches!(d.ssl_mode, PgSslMode::Disable));
            }
            other => panic!("expected discrete, got {other:?}"),
        }
    }

    #[test]
    #[serial]
    fn url_mode_in_password_auth() {
        clear();
        set("TELEPORTA_DATABASE_URL", "postgres://x");
        let cfg = Config::from_env().unwrap();
        assert!(matches!(cfg.database, DatabaseConfig::Url(u) if u == "postgres://x"));
    }

    #[test]
    #[serial]
    fn errors_when_no_database_provided() {
        clear();
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("no database configuration"));
    }

    #[test]
    #[serial]
    fn iam_mode_builds_with_defaults() {
        clear();
        set("TELEPORTA_DATABASE_AUTH_MODE", "iam");
        set("TELEPORTA_DATABASE_HOST", "db.cluster.us-west-2.rds.amazonaws.com");
        set("TELEPORTA_DATABASE_NAME", "teleporta");
        set("TELEPORTA_DATABASE_USER", "teleporta_iam");
        set("TELEPORTA_DATABASE_AWS_REGION", "us-west-2");
        let cfg = Config::from_env().unwrap();
        match cfg.database {
            DatabaseConfig::Iam(i) => {
                assert_eq!(i.aws_region, "us-west-2");
                assert!(matches!(i.ssl_mode, PgSslMode::Require));
                assert_eq!(i.token_refresh_interval, Duration::from_secs(840));
            }
            other => panic!("expected iam, got {other:?}"),
        }
    }

    #[test]
    #[serial]
    fn iam_rejects_url() {
        clear();
        set("TELEPORTA_DATABASE_AUTH_MODE", "iam");
        set("TELEPORTA_DATABASE_URL", "postgres://x");
        set("TELEPORTA_DATABASE_HOST", "h");
        set("TELEPORTA_DATABASE_NAME", "n");
        set("TELEPORTA_DATABASE_USER", "u");
        set("TELEPORTA_DATABASE_AWS_REGION", "us-west-2");
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    #[serial]
    fn iam_rejects_disable_ssl() {
        clear();
        set("TELEPORTA_DATABASE_AUTH_MODE", "iam");
        set("TELEPORTA_DATABASE_HOST", "h");
        set("TELEPORTA_DATABASE_NAME", "n");
        set("TELEPORTA_DATABASE_USER", "u");
        set("TELEPORTA_DATABASE_AWS_REGION", "us-west-2");
        set("TELEPORTA_DATABASE_SSL_MODE", "disable");
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("incompatible with IAM"));
    }

    #[test]
    #[serial]
    fn iam_rejects_disabled_max_lifetime() {
        clear();
        set("TELEPORTA_DATABASE_AUTH_MODE", "iam");
        set("TELEPORTA_DATABASE_HOST", "h");
        set("TELEPORTA_DATABASE_NAME", "n");
        set("TELEPORTA_DATABASE_USER", "u");
        set("TELEPORTA_DATABASE_AWS_REGION", "us-west-2");
        set("TELEPORTA_DATABASE_POOL_MAX_LIFETIME_SECS", "0");
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("MAX_LIFETIME"));
    }

    #[test]
    #[serial]
    fn android_partial_config_errors() {
        clear();
        base_password_env();
        set("TELEPORTA_ANDROID_PACKAGE_NAME", "com.example.app");
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("incomplete Android config"));
    }

    #[test]
    #[serial]
    fn android_fingerprints_split_on_comma() {
        clear();
        base_password_env();
        set("TELEPORTA_ANDROID_PACKAGE_NAME", "com.example.app");
        set(
            "TELEPORTA_ANDROID_SHA256_CERT_FINGERPRINTS",
            "AA:BB , CC:DD",
        );
        let cfg = Config::from_env().unwrap();
        let android = cfg.android.unwrap();
        assert_eq!(android.sha256_cert_fingerprints, vec!["AA:BB", "CC:DD"]);
    }

    #[test]
    #[serial]
    fn invalid_auth_mode_rejected() {
        clear();
        base_password_env();
        set("TELEPORTA_DATABASE_AUTH_MODE", "kerberos");
        let err = Config::from_env().unwrap_err();
        assert!(err.to_string().contains("invalid TELEPORTA_DATABASE_AUTH_MODE"));
    }
}
