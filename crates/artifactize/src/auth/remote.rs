//! Remote review store configuration and its bearer token.
//!
//! The remote is configured only by `$STATE/remote.json` and the environment, never by
//! `artifactize.json`. Callers must never print or log the token.

use std::{
    env, fs,
    io::{Read, Write},
    net::IpAddr,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::Duration,
};

use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};

use super::storage::Storage;

pub const CONFIG: &str = "remote.json";
const TOKEN: &str = "remote-token.json";
const LOGIN: &str = "run `artifactize remote login URL` or set ARTIFACTIZE_REMOTE_TOKEN";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Share {
    /// Reduced records without argv, captured output, tool calls or paths.
    #[default]
    Summary,
    /// Also the saved execution as is.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenSource {
    Env,
    File,
    None,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    url: String,
    #[serde(default)]
    share: Share,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredToken {
    url: String,
    token: String,
}

/// A resolved remote; its token is private and never serialized.
pub struct Remote {
    pub url: Url,
    pub share: Share,
    pub token_source: TokenSource,
    token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub principal: String,
    pub scopes: Vec<String>,
}

#[derive(Debug)]
pub struct Failure {
    /// Network errors, timeouts and 5xx, on which callers may fail open.
    pub unavailable: bool,
    /// The server sent an HTTP response.
    pub responded: bool,
    pub message: String,
}

fn variable(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

/// Accept HTTPS, or plain HTTP only on loopback; never credentials, queries or fragments.
pub fn parse_url(value: &str) -> Result<Url, String> {
    let mut url = Url::parse(value).map_err(|_| "Invalid remote URL.".to_owned())?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("The remote URL must not contain credentials, a query or a fragment.".into());
    }
    let loopback = match url.host() {
        Some(url::Host::Domain(host)) => host == "localhost",
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        None => false,
    };
    if !((url.scheme() == "https" && url.host().is_some()) || (url.scheme() == "http" && loopback))
    {
        return Err("The remote URL must use https:// (http:// only on loopback).".into());
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

fn valid_token(token: &str) -> Result<(), String> {
    if (1..=4096).contains(&token.len()) && token.bytes().all(|byte| byte.is_ascii_graphic()) {
        Ok(())
    } else {
        Err("A remote token is 1–4096 printable ASCII characters without spaces.".into())
    }
}

fn read_config(state: &Path) -> Result<Option<Config>, String> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(state.join(CONFIG))
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot read {CONFIG}: {error}")),
    };
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(format!("{CONFIG} must be a regular file."));
    }
    let mut data = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&data).map(Some).map_err(|_| {
        format!("Invalid {CONFIG}; expected {{\"url\":\"https://...\",\"share\":\"summary\"}}.")
    })
}

/// Resolve the remote from the environment and `$STATE/remote.json` without network access.
/// `None` means no remote is configured or `ARTIFACTIZE_REMOTE=off`.
pub fn remote(state: Option<&Path>, repo: Option<&Path>) -> Result<Option<Remote>, String> {
    let state_dir = crate::store::state_dir(state)?;
    let config = read_config(&state_dir)?;
    let url = match (variable("ARTIFACTIZE_REMOTE"), &config) {
        (Some(url), _) if url == "off" => return Ok(None),
        (Some(url), _) => url,
        (None, Some(config)) => config.url.clone(),
        (None, None) => return Ok(None),
    };
    let url = parse_url(&url)?;
    let share = match variable("ARTIFACTIZE_REMOTE_SHARE") {
        Some(share) => serde_json::from_value(serde_json::Value::String(share))
            .map_err(|_| "ARTIFACTIZE_REMOTE_SHARE must be summary or full.".to_owned())?,
        None => config.map(|config| config.share).unwrap_or_default(),
    };
    let (token, token_source) = if let Some(token) = variable("ARTIFACTIZE_REMOTE_TOKEN") {
        valid_token(&token).map_err(|e| format!("ARTIFACTIZE_REMOTE_TOKEN: {e}"))?;
        (Some(token), TokenSource::Env)
    } else {
        match Storage::inspect(Some(&state_dir), repo)?.read::<StoredToken>(TOKEN)? {
            Some(stored) if parse_url(&stored.url)?.origin() == url.origin() => {
                valid_token(&stored.token)?;
                (Some(stored.token), TokenSource::File)
            }
            // A token is bound to the origin it was issued for.
            Some(_) => {
                return Err(format!(
                    "The stored remote token was issued for another store; {LOGIN}."
                ));
            }
            None => (None, TokenSource::None),
        }
    };
    Ok(Some(Remote {
        url,
        share,
        token_source,
        token,
    }))
}

fn client() -> Result<reqwest::Client, Failure> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| Failure {
            unavailable: false,
            responded: false,
            message: error.to_string(),
        })
}

/// rustls reports handshake and certificate failures as InvalidData I/O errors, possibly
/// nested inside other I/O errors whose `source` skips them.
fn tls(error: &(dyn std::error::Error + 'static)) -> bool {
    match error.downcast_ref::<std::io::Error>() {
        Some(io) => {
            io.kind() == std::io::ErrorKind::InvalidData
                || io.get_ref().is_some_and(|inner| tls(inner))
        }
        None => error.source().is_some_and(tls),
    }
}

fn transport(error: reqwest::Error) -> Failure {
    let tls = tls(&error);
    let cause = std::iter::successors(Some(&error as &dyn std::error::Error), |error| {
        error.source()
    })
    .last()
    .map(ToString::to_string)
    .unwrap_or_default();
    Failure {
        unavailable: !tls && (error.is_connect() || error.is_timeout()),
        responded: false,
        message: if tls {
            format!("TLS with the remote store failed: {cause}")
        } else {
            format!("Remote store is unreachable: {cause}")
        },
    }
}

impl Remote {
    /// Ask the server who this token is; works without a token to test reachability.
    pub async fn whoami(&self) -> Result<Principal, Failure> {
        let url = self.url.join("v1/whoami").expect("relative route");
        let mut request = client()?.get(url);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(transport)?;
        let status = response.status();
        let failure = |unavailable, message: String| Failure {
            unavailable,
            responded: true,
            message,
        };
        match status {
            _ if status.is_success() => response
                .json()
                .await
                .map_err(|_| failure(false, "Invalid remote whoami response.".into())),
            StatusCode::UNAUTHORIZED if self.token.is_none() => {
                Err(failure(false, format!("No remote token; {LOGIN}.")))
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(failure(
                false,
                format!(
                    "The remote store rejected the token (HTTP {}).",
                    status.as_u16()
                ),
            )),
            _ => Err(failure(
                status.is_server_error(),
                format!("The remote store answered HTTP {}.", status.as_u16()),
            )),
        }
    }
}

/// Verify a token from stdin against the store, then save it and `remote.json`.
pub async fn login(
    state: Option<&Path>,
    repo: Option<&Path>,
    url: &str,
    share: Share,
    token: String,
) -> Result<(Remote, Principal), String> {
    let url = parse_url(url)?;
    valid_token(&token)?;
    let remote = Remote {
        url,
        share,
        token_source: TokenSource::File,
        token: Some(token),
    };
    let principal = remote.whoami().await.map_err(|failure| failure.message)?;
    let storage = Storage::new(state, repo)?;
    storage.save(
        TOKEN,
        &StoredToken {
            url: remote.url.to_string(),
            token: remote.token.clone().expect("login token"),
        },
    )?;
    let state = crate::store::state_dir(state)?;
    let mut file = tempfile::NamedTempFile::new_in(&state).map_err(|e| e.to_string())?;
    serde_json::to_writer(
        &mut file,
        &Config {
            url: remote.url.to_string(),
            share,
        },
    )
    .map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.persist(state.join(CONFIG))
        .map_err(|e| e.error.to_string())?;
    Ok((remote, principal))
}

/// Forget the stored token and `remote.json`; the server keeps the token until revoked.
pub fn logout(state: Option<&Path>, repo: Option<&Path>) -> Result<(), String> {
    Storage::new(state, repo)?.remove(TOKEN)?;
    match fs::remove_file(crate::store::state_dir(state)?.join(CONFIG)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_invalid_data_is_a_tls_failure() {
        use std::io::{Error, ErrorKind};
        let handshake = Error::other(Error::new(ErrorKind::InvalidData, "bad certificate"));
        assert!(tls(&handshake));
        assert!(!tls(&Error::from(ErrorKind::ConnectionRefused)));
    }

    #[test]
    fn urls_need_https_except_on_loopback() {
        for url in [
            "https://reviews.example",
            "https://reviews.example/team/",
            "http://127.0.0.1:8417",
            "http://[::1]:8417/",
            "http://localhost:8417/",
        ] {
            assert!(parse_url(url).unwrap().path().ends_with('/'), "{url}");
        }
        for url in [
            "http://reviews.example/",
            "http://10.0.0.1/",
            "https://user:secret@reviews.example/",
            "https://reviews.example/?token=x",
            "ftp://reviews.example/",
            "reviews.example",
        ] {
            assert!(parse_url(url).is_err(), "{url}");
        }
    }
}
