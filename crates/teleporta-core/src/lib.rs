//! Framework-independent core logic for Teleporta.
//!
//! This crate deliberately knows nothing about axum, sqlx, Redis, or AWS. It
//! contains only the pieces that can be reasoned about and unit-tested in
//! isolation:
//!
//! * [`link`] — the link model and path normalization.
//! * [`platform`] — user-agent based platform detection.
//! * [`decision`] — the routing decision (which destination a given platform
//!   should fall back to when the app is not installed).
//! * [`well_known`] — generation of the iOS `apple-app-site-association` and
//!   Android `assetlinks.json` verification documents.

pub mod decision;
pub mod link;
pub mod platform;
pub mod well_known;

pub use decision::{decide, Decision, DestinationType};
pub use link::{normalize_path, Link};
pub use platform::{detect_platform, Platform};
