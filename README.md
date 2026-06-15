# Teleporta

> Open HTTPS links in your app, or fall back to the store.

Teleporta is a lightweight, open-source, self-hosted mobile app link router
built in Rust. It routes HTTPS links to installed mobile apps using **iOS
Universal Links** and **Android App Links**. If the app is not installed,
Teleporta falls back to the App Store, Play Store, or a configured web page.

The public HTTPS link itself *is* the app link:

```
https://go.example.com/v/123456
```

- **App installed** → the OS opens the app directly and hands it the full URL;
  the app parses `/v/123456` and routes to the right screen.
- **App not installed** → the browser lands on Teleporta's fallback page, which
  offers the App Store / Play Store / web destination.

Teleporta deliberately does **not** do attribution, install tracking, deferred
attribution, SKAN, ad-network postbacks, fraud detection, campaign ROI
reporting, user-level tracking, or mobile SDKs. It is not an open-source MMP. It
handles link routing, fallback, caching, and operational logging — nothing more.

## How it works

```
User taps https://go.example.com/v/123456
        │
        ├─ App installed → OS opens the app via Universal Link / App Link
        │                  (the request never reaches the fallback page)
        │
        └─ App not installed → request hits Teleporta:
                 normalize path → resolve (Redis → Postgres) →
                 record click → render fallback (store / web)
```

Teleporta never emits a custom-scheme redirect (`myapp://...`). The OS is
responsible for opening the app; the app owns final routing and validation.

## Architecture

```
Client ──HTTPS──> Teleporta (axum)
                     ├── Redis        link cache + negative cache
                     ├── PostgreSQL   source of truth: links + click logs
                     └── /.well-known apple-app-site-association, assetlinks.json
```

A single crate, organized in two layers:

- **Domain logic** (`link`, `platform`, `decision`, `well_known`) —
  framework-independent and free of I/O: the link model, path normalization,
  platform detection, routing decision, and verification document (AASA /
  assetlinks) generation. Fully unit-tested.
- **Server** (`api`, `cache`, `config`, `db`, `resolver`, `fallback`,
  `click_log`, …) — the axum HTTP server: config, Postgres (password + AWS
  RDS/Aurora IAM auth), Redis cache, resolver, fallback rendering, and click
  logging.

## Quickstart (Docker Compose)

```bash
docker compose -f examples/docker-compose.yml up --build
```

Create a link and try it:

```bash
docker compose -f examples/docker-compose.yml exec postgres \
  psql -U teleporta -d teleporta -c \
  "INSERT INTO links (path, route_type, web_fallback_url, ios_store_url, android_store_url)
   VALUES ('/v/123456', 'vehicle', 'https://example.com/v/123456',
           'https://apps.apple.com/app/id123456789',
           'https://play.google.com/store/apps/details?id=com.example.app');"

curl -i http://localhost:8080/v/123456
curl -s http://localhost:8080/.well-known/apple-app-site-association
curl -s http://localhost:8080/.well-known/assetlinks.json
```

## Quickstart (local cargo)

Start Postgres and Redis (any way you like), then:

```bash
export TELEPORTA_DATABASE_AUTH_MODE=password
export TELEPORTA_DATABASE_HOST=127.0.0.1
export TELEPORTA_DATABASE_NAME=teleporta
export TELEPORTA_DATABASE_USER=teleporta
export TELEPORTA_DATABASE_PASSWORD=password
export TELEPORTA_DATABASE_SSL_MODE=disable
export TELEPORTA_REDIS_URL=redis://127.0.0.1:6379

cargo run --bin teleporta
```

Migrations are embedded in the binary and run automatically at startup.

## Routes

| Route | Purpose |
|-------|---------|
| `GET /*` | Resolve a link path and render the fallback page |
| `GET /.well-known/apple-app-site-association` | iOS Universal Links verification (if iOS is configured) |
| `GET /.well-known/assetlinks.json` | Android App Links verification (if Android is configured) |
| `GET /healthz` | Liveness probe |
| `GET /readyz` | Readiness probe (checks Postgres) |

## Link model

A link is keyed by its normalized path (`/v/123456`). Teleporta treats the path
and `metadata` as opaque — it does not know `v` means "vehicle". Each link has:

| Field | Notes |
|-------|-------|
| `path` | Normalized, unique. The resolution key. |
| `route_type` | Operator label (`vehicle`, `promo`, `referral`, …). Opaque. |
| `web_fallback_url` | Desktop / no-store fallback. |
| `ios_store_url` / `android_store_url` | Used when the app isn't installed. |
| `metadata` | Opaque JSON, stored and returned for app use. |
| `is_active`, `expires_at` | Inactive or expired links resolve as "not found". |

The schema lives in [`crates/teleporta/migrations/`](crates/teleporta/migrations): `links` (source of truth) and
`link_clicks` (operational log; `link_id` is nullable so clicks on unknown
paths are still recorded).

## Configuration

Teleporta is configured entirely via `TELEPORTA_*` environment variables.
Misconfiguration fails loudly at startup.

### Server

| Variable | Default | Notes |
|----------|---------|-------|
| `TELEPORTA_HTTP_HOST` | `0.0.0.0` | Bind host. |
| `TELEPORTA_HTTP_PORT` | `8080` | Bind port. |
| `TELEPORTA_PUBLIC_BASE_URL` | `http://localhost:8080` | Public origin, used for absolute fallback URLs. |

> The bind vars are `TELEPORTA_HTTP_*`, not `TELEPORTA_SERVER_*`, to avoid
> colliding with the `{SERVICE}_PORT` variable Kubernetes injects for a Service
> named `teleporta-server` (its value is `tcp://<clusterIP>:<port>`, which would
> otherwise clobber the bind port and fail startup).

### iOS app association (optional — omit to disable the AASA endpoint)

| Variable | Notes |
|----------|-------|
| `TELEPORTA_IOS_TEAM_ID` | Apple Team ID, e.g. `ABCDE12345`. |
| `TELEPORTA_IOS_BUNDLE_ID` | e.g. `com.example.app`. |
| `TELEPORTA_IOS_APP_STORE_URL` | iOS store fallback for links without their own `ios_store_url`; also offered on the fallback page. |

### Android app association (optional — omit to disable the assetlinks endpoint)

| Variable | Notes |
|----------|-------|
| `TELEPORTA_ANDROID_PACKAGE_NAME` | e.g. `com.example.app`. |
| `TELEPORTA_ANDROID_SHA256_CERT_FINGERPRINTS` | Comma-separated. Use the Play **app signing** key fingerprint(s). |
| `TELEPORTA_ANDROID_PLAY_STORE_URL` | Android store fallback for links without their own `android_store_url`; also offered on the fallback page. |

### Database

| Variable | Default | Notes |
|----------|---------|-------|
| `TELEPORTA_DATABASE_AUTH_MODE` | `password` | `password` or `iam`. |
| `TELEPORTA_DATABASE_URL` | — | Shorthand `postgres://…` (password mode only). |
| `TELEPORTA_DATABASE_HOST` / `_PORT` / `_NAME` / `_USER` | — | Discrete fields. Port default `5432`. |
| `TELEPORTA_DATABASE_PASSWORD` | — | Password mode only. |
| `TELEPORTA_DATABASE_SSL_MODE` | `prefer` | `disable`/`allow`/`prefer`/`require`/`verify-ca`/`verify-full`. |
| `TELEPORTA_DATABASE_SSL_ROOT_CERT` | — | PEM CA bundle path for `verify-*`. |
| `TELEPORTA_DATABASE_AWS_REGION` | — | Required for `iam`. |
| `TELEPORTA_DATABASE_IAM_TOKEN_REFRESH_INTERVAL_SECS` | `840` | Must be `< 900`. |
| `TELEPORTA_DATABASE_POOL_MAX_CONNECTIONS` | `10` | Must be `> 0`. |
| `TELEPORTA_DATABASE_POOL_MIN_CONNECTIONS` | `0` | Warm core for idle periods. |
| `TELEPORTA_DATABASE_POOL_ACQUIRE_TIMEOUT_SECS` | `5` | Bump for IAM cold connects. |
| `TELEPORTA_DATABASE_POOL_IDLE_TIMEOUT_SECS` | `300` | `0` disables. |
| `TELEPORTA_DATABASE_POOL_MAX_LIFETIME_SECS` | `600` | `0` disables; for IAM must be set and `< 900`. |

### Redis (required)

| Variable | Default | Notes |
|----------|---------|-------|
| `TELEPORTA_REDIS_URL` | `redis://localhost:6379` | |
| `TELEPORTA_REDIS_KEY_PREFIX` | `teleporta` | |
| `TELEPORTA_LINK_CACHE_TTL_SECS` | `300` | Positive-cache TTL. |
| `TELEPORTA_NEGATIVE_CACHE_TTL_SECS` | `30` | Unknown-path cache TTL. |

### Fallback & privacy

| Variable | Default | Notes |
|----------|---------|-------|
| `TELEPORTA_FALLBACK_AUTO_REDIRECT_TO_STORE` | `false` | Auto-redirect to the chosen destination. |
| `TELEPORTA_FALLBACK_AUTO_REDIRECT_DELAY_MS` | `500` | Delay before auto-redirect. |
| `TELEPORTA_FALLBACK_HOME_URL` | — | Canonical site for unresolved links. When set, unknown paths `302`-redirect here instead of rendering the not-found page. |
| `TELEPORTA_PRIVACY_STORE_RAW_IP` | `false` | Store the raw client IP. |
| `TELEPORTA_PRIVACY_HASH_IP` | `true` | Store a salted IP hash. |
| `TELEPORTA_PRIVACY_IP_HASH_SALT` | `change-me` | **Override this.** Warned at startup if left default. |

## iOS Universal Links

Teleporta serves the AASA at `GET /.well-known/apple-app-site-association` as
`application/json` (no file extension — the exact path iOS fetches), using a
wildcard `/*` component so every path on the domain is claimed by the app:

```json
{ "applinks": { "details": [
  { "appIDs": ["ABCDE12345.com.example.app"], "components": [{ "/": "/*" }] }
] } }
```

In the iOS app, add the Associated Domains capability with
`applinks:go.example.com`. The AASA must be served over HTTPS with a valid
certificate and no redirects. iOS caches it aggressively — changes can take time
(or a reinstall) to take effect.

## Android App Links

Teleporta serves the Digital Asset Links document at
`GET /.well-known/assetlinks.json`:

```json
[{ "relation": ["delegate_permission/common.handle_all_urls"],
   "target": { "namespace": "android_app", "package_name": "com.example.app",
               "sha256_cert_fingerprints": ["AA:BB:CC:DD:EE:FF"] } }]
```

In the app, declare an `autoVerify` intent filter:

```xml
<intent-filter android:autoVerify="true">
  <action android:name="android.intent.action.VIEW" />
  <category android:name="android.intent.category.DEFAULT" />
  <category android:name="android.intent.category.BROWSABLE" />
  <data android:scheme="https" android:host="go.example.com" />
</intent-filter>
```

With Play App Signing, publish the **app signing key** fingerprint from the Play
Console (list multiple if both an app signing and upload key can sign installed
builds). Verify with Google's
[statement list tester](https://developers.google.com/digital-asset-links/tools/generator).

## AWS RDS / Aurora IAM auth

Set `TELEPORTA_DATABASE_AUTH_MODE=iam` to use short-lived RDS auth tokens
instead of a static password (minted with the official AWS SDK):

```env
TELEPORTA_DATABASE_AUTH_MODE=iam
TELEPORTA_DATABASE_HOST=teleporta-prod.cluster-xxxx.us-west-2.rds.amazonaws.com
TELEPORTA_DATABASE_NAME=teleporta
TELEPORTA_DATABASE_USER=teleporta_iam
TELEPORTA_DATABASE_AWS_REGION=us-west-2
TELEPORTA_DATABASE_SSL_MODE=require
```

How it works:

1. On startup, load AWS SDK config for the region and mint the initial token.
2. Use the token as the Postgres password for the connection pool.
3. A background task regenerates the token every refresh interval and applies it
   to the pool, affecting only *new* connections. Existing connections keep
   working with the token they authenticated under.
4. `POOL_MAX_LIFETIME_SECS` forces connections to rotate well inside the
   15-minute RDS token TTL, even if a refresh tick is missed.

Validated at startup: `iam` and `TELEPORTA_DATABASE_URL` are mutually exclusive;
`ssl_mode=disable` is rejected; the refresh interval and pool max-lifetime must
both be `< 900`. For `verify-*`, point `TELEPORTA_DATABASE_SSL_ROOT_CERT` at the
[RDS global CA bundle](https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem)
(the AWS CA is not in the system trust store). The task/instance role needs
`rds-db:connect`, and the DB user must be granted `rds_iam`.

## Caching

PostgreSQL is the source of truth; Redis accelerates resolution and absorbs
traffic for unknown paths. Keys (the `v1` segment lets a future schema change
invalidate everything at once):

```
teleporta:link:v1:/v/123456        # positive cache: JSON-encoded link
teleporta:link-miss:v1:/unknown    # negative cache: "1"
```

Flow: Redis hit returns immediately; a negative hit returns "not found" without
touching Postgres; a miss queries Postgres and caches the result (positive or
negative). Inactive/expired links are cached as a miss, so disabling a link
takes effect within `NEGATIVE_CACHE_TTL_SECS`. Cache operations are
best-effort: a Redis error is logged and treated as a miss, never a request
failure — a momentary outage degrades to "always hit Postgres", and the managed
connection auto-reconnects.

## Privacy

Click logging exists for **operations and debugging** — confirming QR codes are
scanned, spotting invalid paths, investigating abuse — and explicitly **not**
for attribution or user tracking. Inserts run on a detached task, so logging
never adds latency to (or fails) the user-facing fallback.

A click event may record `link_id`, timestamp, path, query params, user agent,
referrer, platform, destination type, a salted IP hash, and (only when
explicitly enabled) the raw IP. The IP hash is
`hex(sha256(salt || ip))` — stable for grouping repeat visits, but the salt is
what protects it (the IPv4 space is small enough to brute-force), so keep it
secret and unique per deployment. The client IP is taken from the first hop of
`X-Forwarded-For` when present, otherwise the socket peer address — only trust
`X-Forwarded-For` if a trusted proxy sets it.

Teleporta does not auto-expire click logs; apply your own retention policy,
e.g. `DELETE FROM link_clicks WHERE clicked_at < now() - interval '90 days'`.

## Development

```bash
cargo test    # unit tests, no external services required
cargo build   # builds the teleporta binary
```

## License

MIT. See [LICENSE](LICENSE).
