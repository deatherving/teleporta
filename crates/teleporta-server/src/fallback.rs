//! Server-rendered fallback page.
//!
//! When the app is installed, the OS opens it via Universal Links / App Links
//! *before* this page is ever shown. This page is therefore the
//! not-installed path: it explains what happened and offers the store (or web)
//! destination chosen by [`teleporta_core::decide`].

use teleporta_core::{Decision, DestinationType, Link, Platform};

use crate::config::Config;

/// Render the fallback page for a resolved link.
pub fn render_found(cfg: &Config, link: &Link, decision: &Decision) -> String {
    let primary = primary_button(decision);

    // Offer every configured alternative the user might want, regardless of
    // detected platform (detection is best-effort; let the user choose).
    let mut alternatives = Vec::new();
    if !matches!(decision.destination_type, DestinationType::AppStore) {
        if let Some(url) = link
            .ios_store_url
            .clone()
            .or_else(|| cfg.ios.as_ref().and_then(|i| i.app_store_url.clone()))
        {
            alternatives.push(secondary_link("Get it on the App Store", &url));
        }
    }
    if !matches!(decision.destination_type, DestinationType::PlayStore) {
        if let Some(url) = link
            .android_store_url
            .clone()
            .or_else(|| cfg.android.as_ref().and_then(|a| a.play_store_url.clone()))
        {
            alternatives.push(secondary_link("Get it on Google Play", &url));
        }
    }
    if !matches!(decision.destination_type, DestinationType::Web) {
        if let Some(url) = &link.web_fallback_url {
            alternatives.push(secondary_link("Continue on the web", url));
        }
    }

    let auto_redirect = auto_redirect_head(cfg, decision);
    let body = format!(
        "<p class=\"lede\">If you have the app installed, it should have opened automatically.</p>\
         {primary}\
         {alts}",
        alts = alternatives_block(&alternatives),
    );
    page(&auto_redirect, "Open in app", &body)
}

/// Render the page for an unknown / inactive / expired path.
pub fn render_not_found(cfg: &Config, platform: Platform) -> String {
    // Offer the platform-appropriate store and the public site so a user who
    // hit a dead link still has somewhere useful to go.
    let mut alternatives = Vec::new();
    match platform {
        Platform::Ios => {
            if let Some(url) = cfg.ios.as_ref().and_then(|i| i.app_store_url.clone()) {
                alternatives.push(secondary_link("Get it on the App Store", &url));
            }
        }
        Platform::Android => {
            if let Some(url) = cfg.android.as_ref().and_then(|a| a.play_store_url.clone()) {
                alternatives.push(secondary_link("Get it on Google Play", &url));
            }
        }
        _ => {}
    }
    alternatives.push(secondary_link("Go to the homepage", &cfg.public_base_url));

    let body = format!(
        "<p class=\"lede\">This link doesn't point anywhere (it may be expired or mistyped).</p>\
         {alts}",
        alts = alternatives_block(&alternatives),
    );
    page("", "Link not found", &body)
}

fn primary_button(decision: &Decision) -> String {
    match (&decision.destination_type, &decision.url) {
        (DestinationType::AppStore, Some(url)) => primary_link("Get it on the App Store", url),
        (DestinationType::PlayStore, Some(url)) => primary_link("Get it on Google Play", url),
        (DestinationType::Web, Some(url)) => primary_link("Continue on the web", url),
        _ => String::new(),
    }
}

fn auto_redirect_head(cfg: &Config, decision: &Decision) -> String {
    if !cfg.fallback.auto_redirect_to_store {
        return String::new();
    }
    let Some(url) = &decision.url else {
        return String::new();
    };
    let secs = cfg.fallback.auto_redirect_delay.as_secs_f64();
    // A meta refresh is robust across browsers and needs no JS.
    format!(
        "<meta http-equiv=\"refresh\" content=\"{secs};url={url}\">",
        url = html_escape(url),
    )
}

fn alternatives_block(alternatives: &[String]) -> String {
    if alternatives.is_empty() {
        String::new()
    } else {
        format!("<div class=\"alts\">{}</div>", alternatives.concat())
    }
}

fn primary_link(label: &str, url: &str) -> String {
    format!(
        "<a class=\"btn btn-primary\" href=\"{}\">{}</a>",
        html_escape(url),
        html_escape(label)
    )
}

fn secondary_link(label: &str, url: &str) -> String {
    format!(
        "<a class=\"btn\" href=\"{}\">{}</a>",
        html_escape(url),
        html_escape(label)
    )
}

/// Wrap body content in the shared page chrome.
fn page(head_extra: &str, title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\
<html lang=\"en\">\
<head>\
<meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<meta name=\"robots\" content=\"noindex\">\
{head_extra}\
<title>{title}</title>\
<style>\
:root{{color-scheme:light dark}}\
body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;\
font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;\
background:#f5f6f8;color:#1a1a2e}}\
@media(prefers-color-scheme:dark){{body{{background:#11131a;color:#e8e8ef}}}}\
.card{{width:100%;max-width:380px;margin:24px;padding:32px;border-radius:16px;text-align:center;\
background:#fff;box-shadow:0 8px 30px rgba(0,0,0,.08)}}\
@media(prefers-color-scheme:dark){{.card{{background:#1b1e27;box-shadow:none}}}}\
h1{{font-size:20px;margin:0 0 8px}}\
.lede{{font-size:15px;line-height:1.5;opacity:.7;margin:0 0 24px}}\
.btn{{display:block;padding:13px 16px;margin:10px 0;border-radius:10px;font-size:15px;\
font-weight:600;text-decoration:none;border:1px solid rgba(0,0,0,.12);color:inherit}}\
@media(prefers-color-scheme:dark){{.btn{{border-color:rgba(255,255,255,.16)}}}}\
.btn-primary{{background:#4f46e5;color:#fff;border-color:#4f46e5}}\
.alts{{margin-top:8px}}\
</style>\
</head>\
<body>\
<main class=\"card\">\
<h1>{title}</h1>\
{body}\
</main>\
</body>\
</html>",
        title = html_escape(title),
    )
}

/// Minimal HTML escaping for attribute and text contexts.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_dangerous_chars() {
        assert_eq!(
            html_escape("\"><script>alert('x')</script>"),
            "&quot;&gt;&lt;script&gt;alert(&#x27;x&#x27;)&lt;/script&gt;"
        );
    }
}
