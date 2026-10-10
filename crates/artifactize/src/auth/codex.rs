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
    collections::BTreeMap,
    io::{IsTerminal, Read, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{
    Engine,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

mod timestamp;
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
/// Match the localhost callback convention of the Codex public-client flow that Pi
/// also uses; listener and OAuth redirect must agree for browser/pasted-URL interoperability.
const CALLBACK_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
const SCOPE: &str = "openid profile email offline_access";
/// The `originator` that sign-in and every Codex request name.
pub const ORIGINATOR: &str = "artifactize";
#[cfg(test)]
const CLAIM: &str = "https://api.openai.com/auth";
const CREDENTIALS: &str = "codex.json";
const LOCK: &str = "codex";
/// 32 random bytes give 256 bits and 43 unpadded base64url characters, within RFC 7636's
/// permitted 43–128-character code_verifier length; the S256 challenge hashes that text.
const PKCE_VERIFIER_BYTES: usize = 32;
/// Choose 128 unpredictable bits for OAuth state/CSRF correlation, encoded as 32 hex
/// characters. This is this client's chosen size, not an OAuth-mandated exact length.
const OAUTH_STATE_BYTES: usize = 16;
/// Refresh this many seconds before expiry, as the Codex CLI's refresh window does.
const REFRESH_MARGIN: Duration = Duration::from_secs(300);
/// A read-only auth file's token must outlive this many seconds.
const FILE_MARGIN: Duration = Duration::from_secs(60);
/// Give a person five minutes to finish browser or pasted-redirect sign-in,
/// without leaving an unattended callback listener alive indefinitely.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
/// Bound individual token-exchange HTTP calls independently of the interactive login wait.
const AUTH_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Release the callback listener from a client that stops sending its request headers.
const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(5);
/// A stalled browser must not hold up a completed callback while receiving its short reply.
const CALLBACK_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
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
pub fn auth_root() -> Result<String, String> {
    Ok(crate::llm::variable_endpoint(AUTH_URL_VARIABLE)?.unwrap_or_else(|| AUTH_ROOT.into()))
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

fn account_id(access_token: &str) -> Option<crate::types::CodexAccountId> {
    claims(access_token)?.auth.0?.0.chatgpt_account_id.0
}

fn expiry(access_token: &str) -> Option<Timestamp> {
    claims(access_token)?.exp.0
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

fn random(bytes: usize) -> Result<Vec<u8>, String> {
    let mut value = vec![0; bytes];
    getrandom::fill(&mut value).map_err(|_| "Cannot obtain secure randomness.".to_owned())?;
    Ok(value)
}

/// One sign-in attempt: PKCE verifier, its S256 challenge and the `state` it expects.
struct Pending {
    verifier: String,
    state: String,
}

impl Pending {
    fn new() -> Result<Self, String> {
        Ok(Self {
            verifier: URL_SAFE_NO_PAD.encode(random(PKCE_VERIFIER_BYTES)?),
            state: random(OAUTH_STATE_BYTES)?
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
    }

    fn challenge(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()))
    }

    fn authorize_url(&self, root: &str, redirect_uri: &str) -> Result<Url, String> {
        let mut url = Url::parse(&format!("{root}/oauth/authorize"))
            .map_err(|_| "Invalid Codex sign-in URL.".to_owned())?;
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", redirect_uri),
            ("scope", SCOPE),
            ("code_challenge", &self.challenge()),
            ("code_challenge_method", "S256"),
            ("state", &self.state),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", ORIGINATOR),
        ]);
        Ok(url)
    }

    /// The code from a pasted redirect URL, `code#state`, a query string or a bare code,
    /// as Pi accepts them. A pasted state must match.
    fn pasted(&self, input: &str) -> Result<String, String> {
        let input = input.trim();
        let (code, state) = if let Ok(url) = Url::parse(input) {
            let pairs: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
            (pairs.get("code").cloned(), pairs.get("state").cloned())
        } else if let Some((code, state)) = input.split_once('#') {
            (Some(code.to_owned()), Some(state.to_owned()))
        } else if input.contains("code=") {
            let pairs: BTreeMap<_, _> = url::form_urlencoded::parse(input.as_bytes())
                .into_owned()
                .collect();
            (pairs.get("code").cloned(), pairs.get("state").cloned())
        } else {
            (Some(input.to_owned()), None)
        };
        if state.is_some_and(|state| state != self.state) {
            return Err("Codex sign-in state mismatch.".into());
        }
        code.filter(|code| !code.is_empty())
            .ok_or_else(|| "The pasted input has no authorization code.".into())
    }

    /// Answer one callback request. `Some` ends the sign-in; `None` keeps waiting.
    fn callback(
        &self,
        target: &str,
    ) -> (&'static str, &'static str, Option<Result<String, String>>) {
        let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
            return ("400 Bad Request", "Invalid callback.", None);
        };
        if url.path() != CALLBACK_PATH {
            return ("404 Not Found", "Not found.", None);
        }
        let mut parameters = BTreeMap::new();
        for (key, value) in url.query_pairs().into_owned() {
            if parameters.insert(key, value).is_some() {
                return ("400 Bad Request", "Duplicate callback parameter.", None);
            }
        }
        if parameters.get("state") != Some(&self.state) {
            return ("400 Bad Request", "State mismatch.", None);
        }
        if let Some(error) = parameters.get("error") {
            let error: String = error
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .take(MAX_OAUTH_ERROR_CHARS)
                .collect();
            return (
                "400 Bad Request",
                "Codex sign-in was not completed. Return to artifactize.",
                Some(Err(format!("Codex sign-in failed ({error})."))),
            );
        }
        match parameters.get("code").filter(|code| !code.is_empty()) {
            Some(code) => (
                "200 OK",
                "Signed in to Codex. Return to artifactize; you can close this page.",
                Some(Ok(code.clone())),
            ),
            None => ("400 Bad Request", "Missing authorization code.", None),
        }
    }

    /// Serve the loopback callback until one completes the sign-in. Requests must be
    /// `GET` with a `Host` of `localhost:PORT` or `127.0.0.1:PORT`.
    async fn serve(&self, listener: TcpListener) -> Result<String, String> {
        // Allow browser callback headers and the authorization URL while bounding
        // memory consumed by an untrusted loopback client before HTTP validation.
        const MAX_CALLBACK_HEADER_BYTES: usize = 16 * 1024;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let hosts = [format!("localhost:{port}"), format!("127.0.0.1:{port}")];
        loop {
            let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let head = tokio::time::timeout(CALLBACK_READ_TIMEOUT, async {
                let mut head = Vec::new();
                let mut byte = [0];
                while !head.ends_with(b"\r\n\r\n") && head.len() < MAX_CALLBACK_HEADER_BYTES {
                    if stream.read(&mut byte).await? == 0 {
                        break;
                    }
                    head.push(byte[0]);
                }
                Ok::<_, std::io::Error>(head)
            })
            .await;
            let Ok(Ok(head)) = head else { continue };
            let head = String::from_utf8_lossy(&head);
            let line: Vec<_> = head
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let host: Vec<_> = head
                .lines()
                .skip(1)
                .filter_map(|line| line.split_once(':'))
                .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
                .map(|(_, value)| value.trim())
                .collect();
            let valid = head.ends_with("\r\n\r\n")
                && line.len() == 3
                && line[0] == "GET"
                && matches!(line[2], "HTTP/1.0" | "HTTP/1.1")
                && host.len() == 1
                && hosts.iter().any(|expected| expected == host[0])
                && line[1].starts_with('/')
                && !line[1].starts_with("//");
            let (status, body, result) = if valid {
                self.callback(line[1])
            } else {
                ("400 Bad Request", "Invalid callback.", None)
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = tokio::time::timeout(
                CALLBACK_WRITE_TIMEOUT,
                stream.write_all(response.as_bytes()),
            )
            .await;
            if let Some(result) = result {
                return result;
            }
        }
    }

    /// The authorization code from the callback or a pasted redirect, whichever is first.
    async fn wait(
        &self,
        listener: Option<TcpListener>,
        pasted: Option<oneshot::Receiver<String>>,
    ) -> Result<String, String> {
        let callback = async {
            match listener {
                Some(listener) => self.serve(listener).await,
                None => std::future::pending().await,
            }
        };
        let pasted = async {
            match pasted {
                Some(receiver) => match receiver.await {
                    Ok(input) => self.pasted(&input),
                    Err(_) => std::future::pending().await,
                },
                None => std::future::pending().await,
            }
        };
        tokio::time::timeout(LOGIN_TIMEOUT, async {
            tokio::select! {
                result = callback => result,
                result = pasted => result,
            }
        })
        .await
        .map_err(|_| "Codex sign-in timed out; run `artifactize login codex` again.".to_owned())?
    }
}

/// Sign in interactively and save artifactize's own tokens. The sign-in URL goes to
/// stderr, also with --json.
pub async fn login(state: Option<&Path>, repo: Option<&Path>) -> Result<(), String> {
    // Check where the tokens will go before anyone signs in.
    let storage = Storage::new(state, repo, Tokens::Codex)?;
    let root = auth_root()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, CALLBACK_PORT))
        .await
        .ok();
    let terminal = std::io::stdin().is_terminal();
    let pasted = terminal.then(|| {
        let (send, receive) = oneshot::channel();
        // Detached: a sign-in finished in the browser leaves this read unanswered.
        std::thread::spawn(move || {
            for line in std::io::stdin().lines() {
                match line {
                    Ok(line) if line.trim().is_empty() => continue,
                    Ok(line) => {
                        let _ = send.send(line);
                    }
                    Err(_) => {}
                }
                break;
            }
        });
        receive
    });
    let redirect_uri = format!("http://localhost:{CALLBACK_PORT}{CALLBACK_PATH}");
    sign_in(&storage, &root, listener, &redirect_uri, pasted, true, |url, callback| {
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "Open this URL to sign in with Codex:\n{url}");
        if !callback {
            let _ = writeln!(
                err,
                "Port {CALLBACK_PORT} is in use (another Codex sign-in?), so the browser cannot return here."
            );
        }
        if terminal {
            let _ = writeln!(
                err,
                "When the browser cannot return here, paste the URL it was sent to and press Enter."
            );
        }
    })
    .await
}

async fn sign_in(
    storage: &Storage,
    root: &str,
    listener: Option<TcpListener>,
    redirect_uri: &str,
    pasted: Option<oneshot::Receiver<String>>,
    browser: bool,
    show: impl FnOnce(&Url, bool),
) -> Result<(), String> {
    if listener.is_none() && pasted.is_none() {
        return Err(format!(
            "Port {CALLBACK_PORT} is in use and there is no terminal to paste the redirect URL into; finish the other sign-in and try again."
        ));
    }
    let pending = Pending::new()?;
    let url = pending.authorize_url(root, redirect_uri)?;
    show(&url, listener.is_some());
    if browser {
        open_browser(&url).await;
    }
    let code = pending.wait(listener, pasted).await?;
    let tokens = token_request(
        root,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", &code),
            ("code_verifier", &pending.verifier),
            ("redirect_uri", redirect_uri),
        ],
        None,
    )
    .await
    .map_err(|error| error.message)?;
    let credentials = credentials(tokens, now()?)?;
    let _lock = storage.lock(LOCK).await?;
    storage.save(CREDENTIALS, &credentials)
}

async fn open_browser(url: &Url) {
    // The printed authorization URL remains available if desktop handoff fails.
    let _ = crate::platform::program::open_desktop(std::ffi::OsStr::new(url.as_str())).await;
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
    let account_id = account_id(&access_token)
        .ok_or("The Codex access token names no ChatGPT account; nothing was saved.")?;
    Ok(Credentials {
        access_token,
        refresh_token,
        account_id,
        expires_at: saved_at.checked_add(expires_in).ok_or_else(invalid)?,
        saved_at,
    })
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
    root: &str,
    form: &[(&str, &str)],
    refreshing: Option<&Storage>,
) -> Result<TokenResponse, TokenError> {
    let response = client()?
        .post(format!("{root}/oauth/token"))
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
        let body: Value = response.json().await.unwrap_or_default();
        let code = body["error"]
            .as_str()
            .or_else(|| body["error"]["code"].as_str())
            .or_else(|| body["code"].as_str())
            .unwrap_or_default();
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
        return Ok(read_auth_file(&path)?);
    }
    stored_token(&Storage::new(state, repo, Tokens::Codex)?, &auth_root()?).await
}

async fn stored_token(storage: &Storage, root: &str) -> Result<Token, TokenError> {
    stored_token_at(storage, root, now).await
}

async fn stored_token_at(
    storage: &Storage,
    root: &str,
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
#[derive(Default)]
struct AuthFile {
    tokens: crate::json::Optional<crate::json::Object<AuthFileTokens>>,
}
#[derive(Default)]
struct AuthFileTokens {
    access_token: crate::json::Optional<String>,
    account_id: crate::json::Optional<String>,
}
impl<'de> Deserialize<'de> for AuthFile {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AuthVisitor;
        impl<'de> serde::de::Visitor<'de> for AuthVisitor {
            type Value = AuthFile;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an auth-file object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut auth = AuthFile::default();
                while let Some(field) = map.next_key::<String>()? {
                    if field == "tokens" {
                        auth.tokens = map.next_value()?;
                    } else {
                        map.next_value::<crate::json::Ignored>()?;
                    }
                }
                Ok(auth)
            }
        }
        deserializer.deserialize_map(AuthVisitor)
    }
}
impl<'de> Deserialize<'de> for AuthFileTokens {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TokensVisitor;
        impl<'de> serde::de::Visitor<'de> for TokensVisitor {
            type Value = AuthFileTokens;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an auth-file token object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut tokens = AuthFileTokens::default();
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "access_token" => tokens.access_token = map.next_value()?,
                        "account_id" => tokens.account_id = map.next_value()?,
                        _ => {
                            map.next_value::<crate::json::Ignored>()?;
                        }
                    }
                }
                Ok(tokens)
            }
        }
        deserializer.deserialize_map(TokensVisitor)
    }
}

/// The access token of a Codex auth file (`{"tokens":{"access_token",...}}`), read
/// once per use and never written. An expiring token is an error, never a refresh.
fn read_auth_file(path: &Path) -> Result<Token, String> {
    read_auth_file_at(path, now().unwrap_or(Timestamp::from_seconds(u64::MAX)))
}

fn read_auth_file_at(path: &Path, now: Timestamp) -> Result<Token, String> {
    let name = crate::platform::path_text(path);
    let unreadable = |reason: &str| format!("{AUTH_FILE_VARIABLE}: cannot read {name}: {reason}.");
    let file =
        crate::platform::open_regular(path).map_err(|error| unreadable(&error.to_string()))?;
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(unreadable("it is not a regular file"));
    }
    let mut data = Vec::new();
    // Match owned credential storage: one extra byte detects oversized imports.
    file.take(MAX_CREDENTIAL_BYTES as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|error| unreadable(&error.to_string()))?;
    if data.len() > MAX_CREDENTIAL_BYTES {
        return Err(unreadable("it is larger than 1 MiB"));
    }
    let auth: crate::json::Optional<crate::json::Object<AuthFile>> =
        serde_json::from_slice(&data).map_err(|_| unreadable("it is not a Codex auth file"))?;
    let tokens = auth
        .0
        .and_then(|crate::json::Object(auth)| auth.tokens.0)
        .map(|crate::json::Object(tokens)| tokens);
    let access_token = tokens
        .as_ref()
        .and_then(|tokens| tokens.access_token.0.as_deref())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| unreadable("it holds no ChatGPT sign-in tokens"))?;
    if expiry(access_token).is_some_and(|exp| exp <= now.saturating_add(FILE_MARGIN)) {
        return Err(format!(
            "The Codex access token in {name} has expired; sign in with Codex again (for example `codex login`). artifactize never refreshes that file."
        ));
    }
    let account_id = tokens
        .as_ref()
        .and_then(|tokens| tokens.account_id.0.as_deref())
        .filter(|id| !id.is_empty())
        .and_then(|id| id.parse().ok())
        .or_else(|| account_id(access_token))
        .ok_or_else(|| unreadable("its token names no ChatGPT account"))?;
    Ok(Token {
        access_token: access_token.to_owned(),
        account_id,
    })
}

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
        let expiry = match read_auth_file(&path) {
            Ok(token) => FileExpiry::Usable {
                expires_at: expiry(&token.access_token),
            },
            Err(error) if error.starts_with("The Codex access token") => FileExpiry::Expired,
            Err(error) => return Err(error),
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

async fn sign_out(storage: &Storage, root: &str) -> Result<bool, String> {
    let _lock = storage.lock(LOCK).await?;
    let Some(stored) = storage.read::<Credentials>(CREDENTIALS)? else {
        return Ok(true);
    };
    let revoked = revoke(root, &stored.refresh_token).await;
    storage.remove(CREDENTIALS)?;
    Ok(revoked)
}

async fn revoke(root: &str, refresh_token: &str) -> bool {
    let Ok(client) = client() else {
        return false;
    };
    client
        .post(format!("{root}/oauth/revoke"))
        .timeout(REVOCATION_TIMEOUT)
        .json(
            &json!({"token":refresh_token,"token_type_hint":"refresh_token","client_id":CLIENT_ID}),
        )
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

#[cfg(test)]
mod redirect_tests;
#[cfg(test)]
mod tests;
