//! Global outbound HTTP proxy plumbing.
//!
//! At app launch we read the user-set proxy config out of the same
//! `app-state.json` the frontend's tauri-plugin-store writes, and
//! translate it into HTTP_PROXY / HTTPS_PROXY / NO_PROXY environment
//! variables. reqwest (used by tauri-plugin-http) reads those env
//! vars on client construction and routes every outbound request
//! through the configured proxy.
//!
//! Reading the on-disk JSON directly (rather than going through a
//! Rust binding to plugin-store) keeps this module independent of
//! plugin lifecycle: we only need a stable file path and serde.
//! Cost is one duplicated key name (`proxyConfig`) — see
//! src/lib/project-store.ts for the matching write site.

use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};

const DEFAULT_BYPASS_LIST: &str =
    "localhost,127.0.0.0/8,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,*.local";
static ACCEPT_INVALID_CERTS: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default = "default_true", rename = "bypassLocal")]
    pub bypass_local: bool,
    #[serde(default, rename = "acceptInvalidCerts")]
    pub accept_invalid_certs: bool,
}

// Hand-written Default impl so its `bypass_local` matches what
// serde produces for a missing field — `derive(Default)` would
// give `false`, which would silently disagree with the
// "missing-key means bypass on" semantics encoded by the serde
// `default = "default_true"` attribute. No caller invokes
// `ProxyConfig::default()` today, but pinning the two paths to
// the same value avoids a footgun if one ever does.
impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            bypass_local: true,
            accept_invalid_certs: false,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Read `proxyConfig` out of the project's `app-state.json`. Returns
/// None if the file doesn't exist, can't be parsed, or has no proxy
/// section — caller treats those identically to "no proxy".
pub fn read_proxy_config_from_store(store_path: &Path) -> Option<ProxyConfig> {
    let content = std::fs::read_to_string(store_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let proxy = json.get("proxyConfig")?;
    serde_json::from_value(proxy.clone()).ok()
}

/// Apply a proxy config by setting the env vars reqwest reads.
/// Returns a short human-readable summary for logging.
///
/// Validates the URL scheme — only `http://` and `https://` are
/// accepted in this version. Anything else (SOCKS5, malformed,
/// missing scheme) is treated as "disabled" so the user doesn't
/// silently trip over a half-working proxy.
///
/// Concurrency note: `std::env::set_var` mutates process-wide
/// state and is racy if any other thread reads env at the same
/// instant. Two callers exist: (1) the Tauri setup hook, which
/// runs before any HTTP client thread starts (safe), and (2)
/// the `set_proxy_env` IPC command, which can race with an
/// in-flight `reqwest::Client::new()` reading HTTP_PROXY. The
/// race window is microseconds and the worst case is one fetch
/// reading the previous value — acceptable for a user-initiated
/// toggle, and matches what every other "global proxy switch"
/// in similar apps does. Documented here so a future Rust
/// edition that hard-fails on this pattern doesn't surprise us.
pub fn apply_proxy_env(config: &ProxyConfig) -> String {
    // Every "disabled" path MUST clear all three env vars, not just
    // return — otherwise toggling the proxy off after it was on
    // leaves the previous values in place and reqwest keeps routing
    // through the now-removed proxy. The same applies to invalid
    // URLs and unsupported schemes (treat as disabled).
    let url = config.url.trim();
    let parsed_url = reqwest::Url::parse(url).ok();
    let supported_url = parsed_url.as_ref().is_some_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
    });

    let proxy_active = config.enabled && supported_url;
    ACCEPT_INVALID_CERTS.store(
        proxy_active && config.accept_invalid_certs,
        Ordering::Relaxed,
    );

    if !proxy_active {
        clear_proxy_env();
        return if !config.enabled {
            "disabled".to_string()
        } else if url.is_empty() {
            "disabled (empty url)".to_string()
        } else if !supported_url {
            // Mask before logging — an invalid URL might still
            // contain a password the user mis-typed.
            format!(
                "disabled (invalid or unsupported proxy url: {})",
                redact_url(url)
            )
        } else {
            "disabled".to_string()
        };
    }

    std::env::set_var("HTTP_PROXY", url);
    std::env::set_var("HTTPS_PROXY", url);
    if config.bypass_local {
        std::env::set_var("NO_PROXY", DEFAULT_BYPASS_LIST);
    } else {
        // Bypass off — clear NO_PROXY so a previously-set value
        // doesn't leak through.
        std::env::remove_var("NO_PROXY");
    }
    format!(
        "enabled ({}, bypass_local={}, accept_invalid_certs={})",
        redact_url(url),
        config.bypass_local,
        config.accept_invalid_certs,
    )
}

/// Apply the process-wide TLS policy to native reqwest clients. This mirrors
/// the `danger.acceptInvalidCerts` option injected into tauri-plugin-http.
pub fn configure_http_client(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    builder.danger_accept_invalid_certs(ACCEPT_INVALID_CERTS.load(Ordering::Relaxed))
}

/// Strip embedded basic-auth credentials from a URL before logging.
/// `http://user:pass@host:port` → `http://***@host:port`. URLs
/// without credentials pass through untouched. Used so stderr /
/// Console.app / journalctl output doesn't persist proxy
/// passwords.
fn redact_url(url: &str) -> String {
    // Find `scheme://` then check for `user[:pass]@` between that
    // and the next `/` (or end). If found, replace with `***@`.
    let scheme_end = match url.find("://") {
        Some(i) => i + 3,
        None => return url.to_string(),
    };
    let after_scheme = &url[scheme_end..];
    // The userinfo segment, if present, is up to the first '@'
    // that comes BEFORE the first '/'. A '@' after a '/' is part
    // of the path and must not be matched.
    let path_start = after_scheme.find('/').unwrap_or(after_scheme.len());
    let userinfo_end = match after_scheme[..path_start].find('@') {
        Some(i) => i,
        None => return url.to_string(), // no credentials embedded
    };
    let mut out = String::with_capacity(url.len());
    out.push_str(&url[..scheme_end]);
    out.push_str("***");
    out.push_str(&after_scheme[userinfo_end..]);
    out
}

/// Remove all three proxy env vars. Called whenever the user
/// disables the proxy or supplies an invalid URL — this is what
/// makes "turn off proxy" actually take effect for the next fetch
/// (without it, the previous HTTP_PROXY / HTTPS_PROXY / NO_PROXY
/// stay set in the process env and reqwest keeps using them).
fn clear_proxy_env() {
    std::env::remove_var("HTTP_PROXY");
    std::env::remove_var("HTTPS_PROXY");
    std::env::remove_var("NO_PROXY");
}
