//! Generation of the app-link verification documents that the operating
//! systems fetch to associate a domain with an app:
//!
//! * iOS — `/.well-known/apple-app-site-association` (AASA).
//! * Android — `/.well-known/assetlinks.json`.
//!
//! Both are pure functions of operator configuration. The HTTP layer is
//! responsible for serving them with the correct content type
//! (`application/json`) and without a file extension on the AASA path.

use serde_json::{json, Value};

/// Build the iOS `apple-app-site-association` document.
///
/// The `appIDs` entry is `"<team_id>.<bundle_id>"`, and a single wildcard
/// component (`/*`) is published so every path on the domain is claimed by
/// the app. Path-level filtering is the app's responsibility.
pub fn apple_app_site_association(team_id: &str, bundle_id: &str) -> Value {
    json!({
        "applinks": {
            "details": [
                {
                    "appIDs": [format!("{team_id}.{bundle_id}")],
                    "components": [
                        { "/": "/*" }
                    ]
                }
            ]
        }
    })
}

/// Build the Android `assetlinks.json` document granting the app permission to
/// handle all verified URLs for the domain.
pub fn assetlinks_json(package_name: &str, sha256_cert_fingerprints: &[String]) -> Value {
    json!([
        {
            "relation": ["delegate_permission/common.handle_all_urls"],
            "target": {
                "namespace": "android_app",
                "package_name": package_name,
                "sha256_cert_fingerprints": sha256_cert_fingerprints,
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aasa_has_expected_shape() {
        let doc = apple_app_site_association("ABCDE12345", "com.example.app");
        let app_ids = &doc["applinks"]["details"][0]["appIDs"];
        assert_eq!(app_ids[0], "ABCDE12345.com.example.app");
        assert_eq!(doc["applinks"]["details"][0]["components"][0]["/"], "/*");
    }

    #[test]
    fn assetlinks_has_expected_shape() {
        let fps = vec!["AA:BB:CC".to_string(), "DD:EE:FF".to_string()];
        let doc = assetlinks_json("com.example.app", &fps);
        assert_eq!(
            doc[0]["relation"][0],
            "delegate_permission/common.handle_all_urls"
        );
        assert_eq!(doc[0]["target"]["namespace"], "android_app");
        assert_eq!(doc[0]["target"]["package_name"], "com.example.app");
        assert_eq!(doc[0]["target"]["sha256_cert_fingerprints"][1], "DD:EE:FF");
    }
}
