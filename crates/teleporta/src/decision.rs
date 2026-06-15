//! The routing decision: given a resolved link and a detected platform, pick
//! the destination to use when the app is *not* installed.
//!
//! Teleporta never emits a custom-scheme redirect (`myapp://...`). The public
//! HTTPS link *is* the app link, and the OS opens the installed app directly.
//! This decision only governs the browser fallback: which store or web URL the
//! fallback page should point at, and what `destination_type` to record.

use serde::{Deserialize, Serialize};

use crate::link::Link;
use crate::platform::Platform;

/// The kind of destination chosen for the fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationType {
    AppStore,
    PlayStore,
    Web,
    /// No destination is configured for this platform.
    None,
}

impl DestinationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DestinationType::AppStore => "app_store",
            DestinationType::PlayStore => "play_store",
            DestinationType::Web => "web",
            DestinationType::None => "none",
        }
    }
}

/// The chosen fallback destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub destination_type: DestinationType,
    pub url: Option<String>,
}

impl Decision {
    fn none() -> Self {
        Decision {
            destination_type: DestinationType::None,
            url: None,
        }
    }
}

/// Decide the primary fallback destination for `platform`.
///
/// Preference per platform: the link's own store URL, then the deployment-wide
/// store URL (`ios_store_fallback` / `android_store_fallback`, taken from the
/// iOS / Android app config), then the link's web URL, then nothing:
/// * iOS — App Store (row, else global), else web.
/// * Android — Play Store (row, else global), else web.
/// * Desktop / Other — web only.
///
/// The global store fallback lets a deployment route every link to the store
/// without setting `ios_store_url` / `android_store_url` on each row.
pub fn decide(
    link: &Link,
    platform: Platform,
    ios_store_fallback: Option<&str>,
    android_store_fallback: Option<&str>,
) -> Decision {
    let web = || match &link.web_fallback_url {
        Some(url) => Decision {
            destination_type: DestinationType::Web,
            url: Some(url.clone()),
        },
        None => Decision::none(),
    };

    match platform {
        Platform::Ios => match link.ios_store_url.as_deref().or(ios_store_fallback) {
            Some(url) => Decision {
                destination_type: DestinationType::AppStore,
                url: Some(url.to_string()),
            },
            None => web(),
        },
        Platform::Android => match link.android_store_url.as_deref().or(android_store_fallback) {
            Some(url) => Decision {
                destination_type: DestinationType::PlayStore,
                url: Some(url.to_string()),
            },
            None => web(),
        },
        Platform::Desktop | Platform::Other => web(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn link() -> Link {
        Link {
            id: Uuid::nil(),
            path: "/v/1".into(),
            route_type: "vehicle".into(),
            web_fallback_url: Some("https://example.com/v/1".into()),
            ios_store_url: Some("https://apps.apple.com/app/id1".into()),
            android_store_url: Some("https://play.google.com/store/apps/details?id=x".into()),
            metadata: serde_json::Value::Null,
            is_active: true,
            expires_at: None,
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn ios_prefers_app_store() {
        let d = decide(&link(), Platform::Ios, None, None);
        assert_eq!(d.destination_type, DestinationType::AppStore);
        assert_eq!(d.url.as_deref(), Some("https://apps.apple.com/app/id1"));
    }

    #[test]
    fn android_prefers_play_store() {
        let d = decide(&link(), Platform::Android, None, None);
        assert_eq!(d.destination_type, DestinationType::PlayStore);
    }

    #[test]
    fn desktop_uses_web() {
        let d = decide(&link(), Platform::Desktop, None, None);
        assert_eq!(d.destination_type, DestinationType::Web);
        assert_eq!(d.url.as_deref(), Some("https://example.com/v/1"));
    }

    #[test]
    fn falls_back_to_web_when_store_missing() {
        let mut l = link();
        l.ios_store_url = None;
        let d = decide(&l, Platform::Ios, None, None);
        assert_eq!(d.destination_type, DestinationType::Web);
    }

    #[test]
    fn global_store_fallback_used_when_row_missing() {
        let mut l = link();
        l.ios_store_url = None;
        l.android_store_url = None;
        // The global store URL takes precedence over the link's web fallback.
        let d = decide(&l, Platform::Ios, Some("https://apps.apple.com/app/idG"), None);
        assert_eq!(d.destination_type, DestinationType::AppStore);
        assert_eq!(d.url.as_deref(), Some("https://apps.apple.com/app/idG"));
        // The row's own store URL still wins over the global one.
        let d = decide(&link(), Platform::Ios, Some("https://apps.apple.com/app/idG"), None);
        assert_eq!(d.url.as_deref(), Some("https://apps.apple.com/app/id1"));
    }

    #[test]
    fn none_when_nothing_configured() {
        let mut l = link();
        l.ios_store_url = None;
        l.android_store_url = None;
        l.web_fallback_url = None;
        assert_eq!(
            decide(&l, Platform::Ios, None, None).destination_type,
            DestinationType::None
        );
        assert_eq!(
            decide(&l, Platform::Other, None, None).destination_type,
            DestinationType::None
        );
    }
}
