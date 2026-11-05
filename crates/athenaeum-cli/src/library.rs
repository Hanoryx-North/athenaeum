//! The Library client: an HTTP implementation of [`Remote`], and where its credentials live.
//!
//! This is the only place in the workspace that makes a network call on behalf of the knowledge
//! base, and it sits in the CLI rather than in `athenaeum-core` so that the core stays what it
//! promises to be: a library that never touches a network.
//!
//! The token is sent to the one origin it was issued by and nowhere else. Redirects are not
//! followed, so a server cannot bounce an authenticated request to another host, and the token is
//! never put in a URL or a log line.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use athenaeum_core::portable::portable_error;
use athenaeum_core::portable::sync::{
    CreateRepository, PushRequest, Remote, RemoteRepository, RemoteRevision, ServerInfo,
};
use athenaeum_core::{Error, Result};
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::{Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

/// Which server a command talks to when none is named.
pub const SERVER_ENV: &str = "ATHENAEUM_LIBRARY_URL";
/// A token supplied by the environment, which wins over a stored one. For CI and containers.
pub const TOKEN_ENV: &str = "ATHENAEUM_LIBRARY_TOKEN";
/// Overrides where credentials are kept.
pub const CONFIG_DIR_ENV: &str = "ATHENAEUM_CONFIG_DIR";

pub struct LibraryClient {
    base: Url,
    token: Option<String>,
    http: Client,
}

impl LibraryClient {
    pub fn new(server: &str, token: Option<String>) -> Result<Self> {
        let base = normalize_server(server)?;
        let http = Client::builder()
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("athenaeum-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| portable_error("io", error.to_string()))?;
        Ok(Self { base, token, http })
    }

    /// A client for `server` using the stored (or environment) token for it.
    pub fn for_server(server: &str) -> Result<Self> {
        let server = normalize_server(server)?
            .to_string()
            .trim_end_matches('/')
            .to_string();
        let token = credentials::token_for(&server).map(|c| c.token);
        Self::new(&server, token)
    }

    pub fn server(&self) -> String {
        self.base.as_str().trim_end_matches('/').to_string()
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        {
            let mut path = url.path_segments_mut().expect("an http URL has a path");
            path.pop_if_empty();
            path.extend(["api", "v1"]);
            path.extend(segments);
        }
        url
    }

    fn request(&self, method: Method, segments: &[&str]) -> RequestBuilder {
        let builder = self.http.request(method, self.url(segments));
        match &self.token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    fn send(&self, builder: RequestBuilder) -> Result<Response> {
        let response = builder.send().map_err(transport_error)?;
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let body = read_limited(response, 1024 * 1024).unwrap_or_default();
        Err(http_error(status, &body))
    }

    fn json<T: DeserializeOwned>(&self, builder: RequestBuilder) -> Result<T> {
        let response = self.send(builder)?;
        let bytes = read_limited(response, MAX_RESPONSE_BYTES)?;
        serde_json::from_slice(&bytes).map_err(|error| {
            portable_error(
                "invalid_response",
                format!("the server's answer was not what this client expects: {error}"),
            )
        })
    }

    /// The signed-in user, or an `unauthorized` error.
    pub fn whoami(&self) -> Result<Whoami> {
        self.json(self.request(Method::GET, &["user"]))
    }

    /// Exchange an account sign-in for a scoped desktop token. The password and
    /// temporary session cookie are never persisted or returned to the window.
    pub fn sign_in(&self, username: &str, password: &str) -> Result<(String, String)> {
        let response = self.send(
            self.http
                .post(self.url(&["auth", "login"]))
                .json(&serde_json::json!({ "username": username, "password": password })),
        )?;
        let cookie = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .find(|value| value.starts_with("athenaeum_session="))
            .ok_or_else(|| {
                portable_error(
                    "invalid_response",
                    "The Library did not return a sign-in session.",
                )
            })?
            .to_string();
        #[derive(Deserialize)]
        struct SignedIn {
            user: Whoami,
        }
        let signed_in: SignedIn = serde_json::from_slice(&read_limited(response, 1024 * 1024)?)?;
        #[derive(Deserialize)]
        struct AccessToken {
            token: String,
        }
        let token: Result<AccessToken> = self.json(
            self.http.post(self.url(&["user", "tokens"]))
                .header(reqwest::header::COOKIE, &cookie)
                .json(&serde_json::json!({ "name": "Athenaeum desktop", "scopes": ["repo:read", "repo:write"] })),
        );
        // End the temporary account session even if token creation failed.
        let _ = self.send(
            self.http
                .post(self.url(&["auth", "logout"]))
                .header(reqwest::header::COOKIE, cookie),
        );
        let token = token?;
        if token.token.is_empty() {
            return Err(portable_error(
                "invalid_response",
                "The Library did not return an access token.",
            ));
        }
        Ok((token.token, signed_in.user.username))
    }

    /// The server's authoritative operation catalogue. Public, and never follows redirects.
    pub fn openapi(&self) -> Result<serde_json::Value> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("an http URL has a path")
            .pop_if_empty()
            .push("openapi.json");
        self.json(self.http.get(url))
    }

    /// Explicit access to all API v1 routes. The same origin and token rules as sync apply.
    pub fn api_request(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let path = path.trim_start_matches('/');
        let path = path.strip_prefix("api/v1/").unwrap_or(path);
        let segments = if path.is_empty() {
            vec![]
        } else {
            path.split('/').collect::<Vec<_>>()
        };
        if segments.iter().any(|s| {
            s.is_empty() || *s == "." || *s == ".." || s.contains(['%', '\\', '?', '#', ':'])
        }) {
            return Err(portable_error("invalid_request", "Use an API-relative path with literal segments; pass query parameters with --query"));
        }
        let mut request = self.request(method, &segments).query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = self.send(request)?;
        let status = response.status().as_u16();
        let bytes = read_limited(response, MAX_RESPONSE_BYTES)?;
        let body = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
            })
        };
        Ok(serde_json::json!({"status": status, "body": body}))
    }

    /// Resolve `owner/name` to the repository it names.
    pub fn resolve(&self, owner_and_name: &str) -> Result<RemoteRepository> {
        match owner_and_name.split_once('/') {
            Some((owner, name)) if !owner.is_empty() && !name.is_empty() => {
                self.json(self.request(Method::GET, &["users", owner, "repositories", name]))
            }
            _ => self.repository(owner_and_name),
        }
    }

    pub fn start_device_flow(&self) -> Result<DeviceCode> {
        self.json(
            self.request(Method::POST, &["auth", "device"])
                .json(&serde_json::json!({ "client": "athenaeum-cli" })),
        )
    }

    /// One poll of the device flow. `Ok(None)` means "not approved yet".
    pub fn poll_device_flow(&self, device_code: &str) -> Result<Option<DeviceToken>> {
        let response = self
            .request(Method::POST, &["auth", "device", "token"])
            .json(&serde_json::json!({ "deviceCode": device_code }))
            .send()
            .map_err(transport_error)?;
        if response.status().is_success() {
            let bytes = read_limited(response, 1024 * 1024)?;
            return Ok(Some(serde_json::from_slice(&bytes)?));
        }
        let status = response.status();
        let body = read_limited(response, 1024 * 1024).unwrap_or_default();
        match http_error(status, &body) {
            Error::Portable { code, .. }
                if code == "authorization_pending" || code == "slow_down" =>
            {
                Ok(None)
            }
            other => Err(other),
        }
    }
}

impl Remote for LibraryClient {
    fn info(&self) -> Result<ServerInfo> {
        self.json(self.http.get(self.url(&[])))
    }

    fn create_repository(&self, request: &CreateRepository) -> Result<RemoteRepository> {
        self.json(self.request(Method::POST, &["repositories"]).json(request))
    }

    fn repository(&self, id: &str) -> Result<RemoteRepository> {
        self.json(self.request(Method::GET, &["repositories", id]))
    }

    fn revision(&self, id: &str, revision: &str) -> Result<RemoteRevision> {
        self.json(self.request(Method::GET, &["repositories", id, "revisions", revision]))
    }

    fn blob(&self, id: &str, sha256: &str) -> Result<Vec<u8>> {
        let response =
            self.send(self.request(Method::GET, &["repositories", id, "blobs", sha256]))?;
        read_limited(response, MAX_RESPONSE_BYTES)
    }

    fn push(&self, id: &str, request: &PushRequest) -> Result<RemoteRevision> {
        self.json(
            self.request(Method::POST, &["repositories", id, "push"])
                .json(request),
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Whoami {
    pub username: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default = "default_expiry")]
    pub expires_in: u64,
}

fn default_interval() -> u64 {
    5
}
fn default_expiry() -> u64 {
    600
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceToken {
    pub access_token: String,
    #[serde(default)]
    pub username: Option<String>,
}

/// `https://` for anything that is not this machine; plain `http://` only for loopback, where
/// there is no network to eavesdrop on.
pub fn normalize_server(server: &str) -> Result<Url> {
    let trimmed = server.trim().trim_end_matches('/');
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let url = Url::parse(&with_scheme).map_err(|error| {
        portable_error(
            "invalid_request",
            format!("{server:?} is not a URL: {error}"),
        )
    })?;
    let loopback = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        "http" => {
            return Err(portable_error(
                "insecure_server",
                format!(
                    "{server} is plain http. A token sent over it can be read by anyone on the \
                     path; use https (http is allowed only for localhost)"
                ),
            ))
        }
        other => {
            return Err(portable_error(
                "invalid_request",
                format!("unsupported scheme {other:?}"),
            ))
        }
    }
    if url.cannot_be_a_base() || url.host_str().is_none() {
        return Err(portable_error(
            "invalid_request",
            format!("{server:?} has no host"),
        ));
    }
    Ok(url)
}

fn read_limited(response: Response, limit: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(portable_error(
            "limit_exceeded",
            "the server's answer is larger than this client will read",
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| portable_error("io", error.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(portable_error(
            "limit_exceeded",
            "the server's answer is larger than this client will read",
        ));
    }
    Ok(bytes)
}

fn transport_error(error: reqwest::Error) -> Error {
    if error.is_connect() || error.is_timeout() || error.is_request() {
        portable_error(
            "offline",
            format!(
                "the Library cannot be reached ({}). Nothing was changed; local knowledge works without it",
                short(&error)
            ),
        )
    } else {
        portable_error("io", short(&error))
    }
}

fn short(error: &reqwest::Error) -> String {
    // The URL can carry nothing secret here (tokens go in a header), but it is still noise.
    let mut text = error.to_string();
    if let Some(url) = error.url() {
        text = text.replace(
            url.as_str(),
            &format!("{}://{}", url.scheme(), url.host_str().unwrap_or("?")),
        );
    }
    text
}

fn http_error(status: StatusCode, body: &[u8]) -> Error {
    #[derive(Deserialize)]
    struct Envelope {
        error: Inner,
    }
    #[derive(Deserialize)]
    struct Inner {
        code: String,
        message: String,
    }
    if let Ok(envelope) = serde_json::from_slice::<Envelope>(body) {
        return portable_error(envelope.error.code, envelope.error.message);
    }
    let code = match status.as_u16() {
        401 => "unauthorized",
        403 => "forbidden",
        404 => "not_found",
        409 => "conflict",
        413 => "limit_exceeded",
        422 => "invalid_content",
        429 => "rate_limited",
        500..=599 => "server_error",
        _ => "http_error",
    };
    portable_error(code, format!("the server answered {status}"))
}

/// Where a token is kept between runs.
pub mod credentials {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct Credential {
        pub token: String,
    }

    #[derive(Debug, Default, Serialize, Deserialize)]
    struct File {
        #[serde(default)]
        servers: BTreeMap<String, Stored>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct Stored {
        token: String,
        #[serde(default)]
        username: Option<String>,
    }

    pub fn config_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV) {
            return PathBuf::from(dir);
        }
        if cfg!(windows) {
            if let Some(appdata) = std::env::var_os("APPDATA") {
                return PathBuf::from(appdata).join("athenaeum");
            }
        }
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("athenaeum");
        }
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("athenaeum")
    }

    fn path() -> PathBuf {
        config_dir().join("credentials.json")
    }

    fn read() -> File {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn token_for(server: &str) -> Option<Credential> {
        if let Ok(token) = std::env::var(TOKEN_ENV) {
            if !token.trim().is_empty() {
                return Some(Credential {
                    token: token.trim().to_string(),
                });
            }
        }
        read().servers.get(server).map(|stored| Credential {
            token: stored.token.clone(),
        })
    }

    /// Servers that have a stored credential.
    pub fn servers() -> Vec<(String, Option<String>)> {
        read()
            .servers
            .into_iter()
            .map(|(server, stored)| (server, stored.username))
            .collect()
    }

    /// Store a token in a file only its owner can read.
    pub fn store(server: &str, token: &str, username: Option<&str>) -> Result<PathBuf> {
        let mut file = read();
        file.servers.insert(
            server.to_string(),
            Stored {
                token: token.to_string(),
                username: username.map(str::to_string),
            },
        );
        write(&file)
    }

    pub fn forget(server: &str) -> Result<bool> {
        let mut file = read();
        let removed = file.servers.remove(server).is_some();
        if removed {
            write(&file)?;
        }
        Ok(removed)
    }

    fn write(file: &File) -> Result<PathBuf> {
        use std::io::Write;
        let target = path();
        std::fs::create_dir_all(target.parent().expect("a file has a parent"))?;
        let temporary = target.with_extension("json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut handle = options.open(&temporary)?;
        handle.write_all(serde_json::to_string_pretty(file)?.as_bytes())?;
        handle.sync_all()?;
        drop(handle);
        std::fs::rename(&temporary, &target)?;
        Ok(target)
    }
}

#[cfg(test)]
mod credential_tests {
    use super::credentials::{forget, store, token_for};
    use super::*;

    #[test]
    fn tokens_are_stored_per_server_in_an_owner_only_file() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(CONFIG_DIR_ENV, dir.path());
        std::env::remove_var(TOKEN_ENV);
        let stored = store("https://library.example", "tok_secret", Some("ada")).unwrap();
        assert_eq!(
            token_for("https://library.example").unwrap().token,
            "tok_secret"
        );
        assert!(token_for("https://elsewhere.example").is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&stored).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o077,
                0,
                "the credentials file must not be readable by others"
            );
        }
        assert!(forget("https://library.example").unwrap());
        assert!(token_for("https://library.example").is_none());
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_is_refused_except_for_this_machine() {
        assert!(normalize_server("http://library.example").is_err());
        assert!(normalize_server("http://localhost:3000").is_ok());
        assert!(normalize_server("http://127.0.0.1:3000").is_ok());
        assert!(normalize_server("library.example").unwrap().scheme() == "https");
        assert!(normalize_server("ftp://x").is_err());
    }

    #[test]
    fn server_errors_keep_their_own_code() {
        let error = http_error(
            StatusCode::CONFLICT,
            br#"{"error":{"code":"stale_base","message":"pull first"}}"#,
        );
        assert!(matches!(&error, Error::Portable { code, .. } if code == "stale_base"));
        let error = http_error(StatusCode::NOT_FOUND, b"<html>");
        assert!(matches!(&error, Error::Portable { code, .. } if code == "not_found"));
    }
}
