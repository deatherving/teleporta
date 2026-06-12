# Teleporta

> Open HTTPS links in your app, or fall back to the store.

Teleporta is a lightweight, self-hosted mobile app link router built in Rust.
It routes HTTPS links to installed mobile apps using **iOS Universal Links** and
**Android App Links**. If the app is not installed, Teleporta falls back to the
App Store, Play Store, or a configured web page.

The public HTTPS link itself *is* the app link (`https://go.example.com/v/123456`):

- **App installed** → the OS opens the app directly and hands it the full URL.
- **App not installed** → the browser lands on Teleporta's fallback page, which
  offers the App Store / Play Store / web destination.

Teleporta deliberately does **not** do attribution, install tracking, SKAN,
ad-network postbacks, fraud detection, campaign ROI, user-level tracking, or
mobile SDKs. It handles link routing, fallback, caching, and operational
logging — nothing more.

## Install

```bash
cargo install teleporta
```

It needs PostgreSQL (source of truth) and Redis (cache). Configuration is via
`TELEPORTA_*` environment variables; migrations run automatically at startup.

```bash
export TELEPORTA_DATABASE_AUTH_MODE=password
export TELEPORTA_DATABASE_HOST=127.0.0.1
export TELEPORTA_DATABASE_NAME=teleporta
export TELEPORTA_DATABASE_USER=teleporta
export TELEPORTA_DATABASE_PASSWORD=password
export TELEPORTA_REDIS_URL=redis://127.0.0.1:6379

teleporta
```

## Documentation

Full setup, configuration reference, iOS/Android association, AWS RDS/Aurora IAM
auth, caching, and privacy details are in the repository:

<https://github.com/deatherving/teleporta>

## License

MIT.
