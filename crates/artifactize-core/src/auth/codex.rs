//! Codex (ChatGPT) sign-in: artifactize's own OAuth tokens in `$STATE/auth/codex.json`,
//! or a Codex auth file that artifactize only reads.
//!
//! The protocol follows Pi's `openai-codex` provider (pi-mono
//! `packages/ai/src/auth/oauth/openai-codex.ts`): the Codex public OAuth client, PKCE
//! with S256, the `localhost:1455` callback or a pasted redirect URL, the
//! authorization-code and refresh-token grants, and the ChatGPT account ID from the
//! access token's `https://api.openai.com/auth` claim. Revocation on logout follows the
//! Codex CLI (`codex-rs/login/src/auth/revoke.rs`). Callers must never print or log a
//! token.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{
    Engine,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
mod auth_file;
mod callback;
mod interactive;
use auth_file::{AuthFileError, FileToken, read_auth_file_at};
#[cfg(test)]
use interactive::*;
pub use interactive::{Prompt, login, login_with};
#[cfg(test)]
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::{collections::BTreeMap, ffi::OsStr};
#[cfg(test)]
use tokio::sync::oneshot;
mod timestamp;
use callback::Callback;
pub use timestamp::Timestamp;

use super::storage::{Location, MAX_CREDENTIAL_BYTES, Storage, Tokens};

/// The Codex CLI's public OAuth client, which Pi's provider uses too.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTH_ROOT: &str = "https://auth.openai.com";
/// A loopback test endpoint replacing the sign-in root, beside the backends' own.
pub const AUTH_URL_VARIABLE: &str = "ARTIFACTIZE_CODEX_AUTH_URL";
/// A Codex auth file (for example `~/.codex/auth.json`) to read instead of
/// artifactize's own tokens. It is never written, copied or refreshed.
pub const AUTH_FILE_VARIABLE: &str = "ARTIFACTIZE_CODEX_AUTH_FILE";
/// The `originator` that sign-in and every Codex request name.
pub const ORIGINATOR: &str = "artifactize";
#[cfg(test)]
const CLAIM: &str = "https://api.openai.com/auth";
const CREDENTIALS: &str = "codex.json";
const LOCK: &str = "codex";
/// Refresh this many seconds before expiry, as the Codex CLI's refresh window does.
const REFRESH_MARGIN: Duration = Duration::from_secs(300);
/// Bound individual token-exchange HTTP calls independently of the interactive login wait.
const AUTH_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Best-effort logout should finish sooner than an ordinary token exchange.
const REVOCATION_TIMEOUT: Duration = Duration::from_secs(10);
/// OAuth error codes are diagnostics, not arbitrary provider text; keep them short.
const MAX_OAUTH_ERROR_CHARS: usize = 64;
const LOGIN_REQUIRED: &str =
    "Codex is not signed in; run `artifactize login codex` or set ARTIFACTIZE_CODEX_AUTH_FILE.";

#[derive(Deserialize, Serialize)]
struct Credentials {
    access_token: String,
    refresh_token: String,
    account_id: crate::types::CodexAccountId,
    #[serde(with = "timestamp::seconds")]
    expires_at: Timestamp,
    #[serde(with = "timestamp::seconds")]
    saved_at: Timestamp,
}

/// Why no token is available. A transient failure (the token endpoint unreachable or
/// failing with 429 or 5xx) may succeed on a retry; any other means signing in again.
#[derive(Debug)]
pub struct TokenError {
    pub transient: bool,
    pub message: String,
}

impl From<String> for TokenError {
    fn from(message: String) -> Self {
        Self {
            transient: false,
            message,
        }
    }
}

impl From<&str> for TokenError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

/// A bearer token and the ChatGPT account it belongs to. Never printed or logged.
pub struct Token {
    pub access_token: String,
    pub account_id: crate::types::CodexAccountId,
}

pub(crate) fn now() -> Result<Timestamp, String> {
    Timestamp::now()
}

/// The sign-in root: its loopback test endpoint, else `https://auth.openai.com`.
pub fn auth_root() -> Result<Url, String> {
    Ok(
        crate::llm::variable_endpoint(AUTH_URL_VARIABLE)?.map_or_else(
            || Url::parse(AUTH_ROOT).expect("built-in Codex auth URL"),
            crate::llm::TestEndpoint::into_url,
        ),
    )
}

fn endpoint(root: &Url, route: &str) -> Result<Url, String> {
    let mut url = root.clone();
    url.path_segments_mut()
        .map_err(|_| "Invalid Codex sign-in URL.")?
        .pop_if_empty()
        .push("oauth")
        .push(route);
    Ok(url)
}

/// The configured read-only auth file, if any.
pub fn auth_file() -> Option<PathBuf> {
    crate::platform::environment::var(AUTH_FILE_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(AUTH_HTTP_TIMEOUT)
        .build()
        .map_err(|_| "Cannot initialize the Codex sign-in HTTP client.".into())
}

/// The JSON claims of a JWT, read without verifying its signature, as Pi does: the
/// token came over TLS from the token endpoint or from the user's own auth file.
fn claims(token: &str) -> Option<JwtClaims> {
    let mut parts = token.split('.');
    let (_, payload, _, None) = (parts.next()?, parts.next()?, parts.next()?, parts.next()) else {
        return None;
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| URL_SAFE.decode(payload))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

impl JwtClaims {
    fn account_id(self) -> Option<crate::types::CodexAccountId> {
        self.auth.0?.0.chatgpt_account_id.0
    }
}

#[derive(Default, Deserialize)]
struct JwtClaims {
    #[serde(default)]
    exp: crate::json::Optional<Timestamp>,
    #[serde(default, rename = "https://api.openai.com/auth")]
    auth: crate::json::Optional<crate::json::Object<AccountClaim>>,
}
#[derive(Default, Deserialize)]
struct AccountClaim {
    #[serde(default)]
    chatgpt_account_id: crate::json::Optional<crate::types::CodexAccountId>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    #[serde(
        default,
        deserialize_with = "timestamp::optional_duration::deserialize"
    )]
    expires_in: Option<Duration>,
}

fn credentials(tokens: TokenResponse, saved_at: Timestamp) -> Result<Credentials, String> {
    let invalid = || "Invalid Codex token response; nothing was saved.".to_owned();
    let access_token = tokens.access_token.filter(|token| !token.is_empty());
    let refresh_token = tokens.refresh_token.filter(|token| !token.is_empty());
    let (Some(access_token), Some(refresh_token), Some(expires_in)) = (
        access_token,
        refresh_token,
        tokens.expires_in.filter(|duration| !duration.is_zero()),
    ) else {
        return Err(invalid());
    };
    let account_id = claims(&access_token)
        .and_then(JwtClaims::account_id)
        .ok_or("The Codex access token names no ChatGPT account; nothing was saved.")?;
    Ok(Credentials {
        access_token,
        refresh_token,
        account_id,
        expires_at: saved_at.checked_add(expires_in).ok_or_else(invalid)?,
        saved_at,
    })
}

#[derive(Default, Deserialize)]
struct OAuthError {
    error: Option<OAuthErrorField>,
    code: Option<OAuthCode>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OAuthErrorField {
    Text(String),
    Object { code: Option<OAuthCode> },
    Other(serde::de::IgnoredAny),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OAuthCode {
    Text(String),
    Other(serde::de::IgnoredAny),
}

impl OAuthCode {
    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(code) => Some(code),
            Self::Other(_) => None,
        }
    }
}

impl OAuthError {
    fn code(&self) -> Option<&str> {
        let error = match &self.error {
            Some(OAuthErrorField::Text(code)) => Some(code.as_str()),
            Some(OAuthErrorField::Object { code }) => code.as_ref().and_then(OAuthCode::text),
            _ => None,
        };
        error.or_else(|| self.code.as_ref().and_then(OAuthCode::text))
    }
}

/// Refresh errors after which the refresh token can never work again.
fn terminal(code: &str) -> bool {
    matches!(
        code,
        "invalid_grant"
            | "refresh_token_expired"
            | "refresh_token_reused"
            | "refresh_token_invalidated"
    )
}

/// POST a form to the token endpoint. Error bodies are never echoed: they may carry
/// codes or tokens. A terminal refresh error removes the stored tokens.
async fn token_request(
    root: &Url,
    form: &[(&str, &str)],
    refreshing: Option<&Storage>,
) -> Result<TokenResponse, TokenError> {
    let response = client()?
        .post(endpoint(root, "token")?)
        .form(form)
        .send()
        .await
        .map_err(|_| TokenError {
            transient: true,
            message:
                "Codex token request failed; check your connection. The stored sign-in was kept."
                    .into(),
        })?;
    let status = response.status();
    if !status.is_success() {
        let body: OAuthError = response.json().await.unwrap_or_default();
        let code = body.code().unwrap_or_default();
        // Rate limiting and server failures pass; a client error does not.
        let transient =
            status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
        let status = status.as_u16();
        let message = match refreshing {
            Some(storage) if terminal(code) => {
                storage.remove(CREDENTIALS)?;
                format!(
                    "The Codex refresh token is no longer valid ({code}); run `artifactize login codex`."
                )
            }
            Some(_) => {
                format!("Codex token refresh failed (HTTP {status}); the stored sign-in was kept.")
            }
            None if code == "invalid_grant" => {
                "The Codex authorization code was rejected (invalid_grant); run `artifactize login codex` again."
                    .into()
            }
            None => format!("Codex sign-in failed (HTTP {status})."),
        };
        return Err(TokenError { transient, message });
    }
    response
        .json()
        .await
        .map_err(|_| "Invalid Codex token response; nothing was saved.".into())
}

/// A usable token: from the read-only auth file when one is set, else artifactize's own,
/// refreshed under a cross-process lock when it is about to expire.
pub async fn access_token(state: Option<&Path>, repo: Option<&Path>) -> Result<Token, TokenError> {
    if let Some(path) = auth_file() {
        return Ok(read_auth_file_at(&path, now()?)?.token);
    }
    stored_token(&Storage::new(state, repo, Tokens::Codex)?, &auth_root()?).await
}

async fn stored_token(storage: &Storage, root: &Url) -> Result<Token, TokenError> {
    stored_token_at(storage, root, now).await
}

async fn stored_token_at(
    storage: &Storage,
    root: &Url,
    clock: impl Fn() -> Result<Timestamp, String>,
) -> Result<Token, TokenError> {
    let _lock = storage.lock(LOCK).await?;
    let stored = storage
        .read::<Credentials>(CREDENTIALS)?
        .ok_or(LOGIN_REQUIRED)?;
    if stored.expires_at > clock()?.saturating_add(REFRESH_MARGIN) {
        return Ok(Token {
            access_token: stored.access_token,
            account_id: stored.account_id,
        });
    }
    let tokens = token_request(
        root,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &stored.refresh_token),
            ("client_id", CLIENT_ID),
        ],
        Some(storage),
    )
    .await?;
    let refreshed = credentials(tokens, clock()?)?;
    if refreshed.account_id != stored.account_id {
        return Err(
            "The refreshed Codex token is for another ChatGPT account; run `artifactize login codex`."
                .into(),
        );
    }
    storage.save(CREDENTIALS, &refreshed)?;
    Ok(Token {
        access_token: refreshed.access_token,
        account_id: refreshed.account_id,
    })
}

/// Known read-only import fields. Wrong-typed optional fields retain the old missing/null
/// behavior; extra fields are ignored and no credential data is echoed in parse errors.
/// A read-only file can carry a usable token without an expiry claim. An expired
/// file reports no timestamp because the existing reader refuses it before returning a token.
#[derive(Debug)]
pub enum FileExpiry {
    Usable { expires_at: Option<Timestamp> },
    Expired,
}

/// Owned credentials always carry the expiry saved by the token exchange.
#[derive(Debug)]
pub enum StoredExpiry {
    Usable { expires_at: Timestamp },
    Expired { expires_at: Timestamp },
}

/// Offline sign-in states: only file states have a path, only stored states have
/// a mandatory expiry, and a storage refusal can only accompany an absent sign-in.
#[derive(Debug)]
pub enum Status {
    Absent,
    Refused { reason: String },
    File { path: PathBuf, expiry: FileExpiry },
    Stored { expiry: StoredExpiry },
}

impl Serialize for Status {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        // Keep the pre-enum JSON contract exactly at the serialization boundary.
        let (source, file, expires_at, expired, refused) = match self {
            Self::Absent => ("none", None, None, false, None),
            Self::Refused { reason } => ("none", None, None, false, Some(reason)),
            Self::File {
                path,
                expiry: FileExpiry::Usable { expires_at },
            } => ("file", Some(path), *expires_at, false, None),
            Self::File {
                path,
                expiry: FileExpiry::Expired,
            } => ("file", Some(path), None, true, None),
            Self::Stored {
                expiry: StoredExpiry::Usable { expires_at },
            } => ("stored", None, Some(*expires_at), false, None),
            Self::Stored {
                expiry: StoredExpiry::Expired { expires_at },
            } => ("stored", None, Some(*expires_at), true, None),
        };
        let mut wire = serializer.serialize_struct(
            "Status",
            3 + usize::from(file.is_some()) + usize::from(refused.is_some()),
        )?;
        wire.serialize_field("source", source)?;
        if let Some(file) = file {
            wire.serialize_field("authFile", &crate::platform::path_text(file))?;
        }
        wire.serialize_field("expiresAt", &expires_at)?;
        wire.serialize_field("expired", &expired)?;
        if let Some(reason) = refused {
            wire.serialize_field("refused", reason)?;
        }
        wire.end()
    }
}

/// Inspect the sign-in offline: no lock, refresh, network or writes.
pub fn status(state: Option<&Path>, repo: Option<&Path>) -> Result<Status, String> {
    let now = now()?;
    if let Some(path) = auth_file() {
        // Reading fails on an expiring token; that is a state to report, not an error.
        let expiry = match read_auth_file_at(&path, now) {
            Ok(FileToken { expires_at, .. }) => FileExpiry::Usable { expires_at },
            Err(AuthFileError::Expired(_)) => FileExpiry::Expired,
            Err(error) => return Err(error.into()),
        };
        return Ok(Status::File { path, expiry });
    }
    let location = Location::find(state, repo)?;
    if let Some(refusal) = location.refusal(Tokens::Codex) {
        // Only a sign-in already stored there is at risk; without one this is advice.
        return match crate::platform::entry_exists(&location.directory.join(CREDENTIALS)) {
            Ok(false) => Ok(Status::Refused { reason: refusal }),
            _ => Err(refusal),
        };
    }
    let stored = Storage::inspect(state, repo, Tokens::Codex)?.read::<Credentials>(CREDENTIALS)?;
    Ok(match stored {
        None => Status::Absent,
        Some(stored) => Status::Stored {
            expiry: if stored.expires_at <= now {
                StoredExpiry::Expired {
                    expires_at: stored.expires_at,
                }
            } else {
                StoredExpiry::Usable {
                    expires_at: stored.expires_at,
                }
            },
        },
    })
}

/// Revoke and remove artifactize's own tokens; a read-only auth file is left alone.
/// Returns whether the revocation was confirmed.
pub async fn logout(state: Option<&Path>, repo: Option<&Path>) -> Result<bool, String> {
    sign_out(&Storage::new(state, repo, Tokens::Codex)?, &auth_root()?).await
}

async fn sign_out(storage: &Storage, root: &Url) -> Result<bool, String> {
    let _lock = storage.lock(LOCK).await?;
    let Some(stored) = storage.read::<Credentials>(CREDENTIALS)? else {
        return Ok(true);
    };
    let revoked = revoke(root, &stored.refresh_token).await;
    storage.remove(CREDENTIALS)?;
    Ok(revoked)
}

async fn revoke(root: &Url, refresh_token: &str) -> bool {
    let Ok(client) = client() else {
        return false;
    };
    let Ok(url) = endpoint(root, "revoke") else {
        return false;
    };
    client
        .post(url)
        .timeout(REVOCATION_TIMEOUT)
        .json(
            &json!({"token":refresh_token,"token_type_hint":"refresh_token","client_id":CLIENT_ID}),
        )
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

#[cfg(test)]
mod callback_tests;
#[cfg(test)]
mod redirect_tests;
#[cfg(test)]
mod tests;
