//! User-agent based platform detection.
//!
//! This is a best-effort classification used only to choose the most relevant
//! fallback (App Store vs Play Store vs web) and to label click logs. It is
//! never used for authorization. When the app is installed, the OS opens it
//! via Universal Links / App Links *before* the request ever reaches the
//! fallback page, so this detection only matters for the not-installed path.

use serde::{Deserialize, Serialize};

/// A coarse client platform classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Ios,
    Android,
    Desktop,
    Other,
}

impl Platform {
    /// Stable lowercase label for logging and storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Ios => "ios",
            Platform::Android => "android",
            Platform::Desktop => "desktop",
            Platform::Other => "other",
        }
    }
}

/// Classify a client from its `User-Agent` header.
///
/// Order matters: Android devices sometimes include "Linux" in their UA, so
/// Android is checked before the desktop keywords.
pub fn detect_platform(user_agent: &str) -> Platform {
    let ua = user_agent.to_ascii_lowercase();

    if ua.contains("iphone") || ua.contains("ipad") || ua.contains("ipod") {
        Platform::Ios
    } else if ua.contains("android") {
        Platform::Android
    } else if ua.contains("windows")
        || ua.contains("macintosh")
        || ua.contains("mac os x")
        || ua.contains("x11")
        || ua.contains("linux")
        || ua.contains("cros")
    {
        Platform::Desktop
    } else {
        Platform::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ios() {
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15";
        assert_eq!(detect_platform(ua), Platform::Ios);
        let ipad = "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X)";
        assert_eq!(detect_platform(ipad), Platform::Ios);
    }

    #[test]
    fn detects_android_before_linux() {
        let ua = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36";
        assert_eq!(detect_platform(ua), Platform::Android);
    }

    #[test]
    fn detects_desktop() {
        assert_eq!(
            detect_platform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"),
            Platform::Desktop
        );
        assert_eq!(
            detect_platform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"),
            Platform::Desktop
        );
    }

    #[test]
    fn unknown_is_other() {
        assert_eq!(detect_platform(""), Platform::Other);
        assert_eq!(detect_platform("curl/8.0"), Platform::Other);
    }

    #[test]
    fn labels_are_stable() {
        assert_eq!(Platform::Ios.as_str(), "ios");
        assert_eq!(Platform::Android.as_str(), "android");
        assert_eq!(Platform::Desktop.as_str(), "desktop");
        assert_eq!(Platform::Other.as_str(), "other");
    }
}
