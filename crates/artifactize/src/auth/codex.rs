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
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    process::Command,
    sync::oneshot,
};

use super::storage::Storage;

/// The Codex CLI's public OAuth client, which Pi's provider uses too.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTH_ROOT: &str = "https://auth.openai.com";
/// A loopback test endpoint replacing the sign-in root, beside the backends' own.
pub const AUTH_URL_VARIABLE: &str = "ARTIFACTIZE_CODEX_AUTH_URL";
/// A Codex auth file (for example `~/.codex/auth.json`) to read instead of
/// artifactize's own tokens. It is never written, copied or refreshed.
pub const AUTH_FILE_VARIABLE: &str = "ARTIFACTIZE_CODEX_AUTH_FILE";
const CALLBACK_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const SCOPE: &str = "openid profile email offline_access";
/// The `originator` that sign-in and every Codex request name.
pub const ORIGINATOR: &str = "artifactize";
const CLAIM: &str = "https://api.openai.com/auth";
const CREDENTIALS: &str = "codex.json";
const LOCK: &str = "codex";
/// Refresh this many seconds before expiry, as the Codex CLI's refresh window does.
const REFRESH_MARGIN: u64 = 300;
/// A read-only auth file's token must outlive this many seconds.
const FILE_MARGIN: u64 = 60;
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const LOGIN_REQUIRED: &str =
    "Codex is not signed in; run `artifactize login codex` or set ARTIFACTIZE_CODEX_AUTH_FILE.";

#[derive(Deserialize, Serialize)]
struct Credentials {
    access_token: String,
    refresh_token: String,
    account_id: String,
    expires_at: u64,
    saved_at: u64,
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
    pub account_id: String,
}

pub(crate) fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| "The system clock is before the Unix epoch.".into())
}

/// The sign-in root: its loopback test endpoint, else `https://auth.openai.com`.
pub fn auth_root() -> Result<String, String> {
    Ok(crate::llm::variable_endpoint(AUTH_URL_VARIABLE)?.unwrap_or_else(|| AUTH_ROOT.into()))
}

/// The configured read-only auth file, if any.
pub fn auth_file() -> Option<PathBuf> {
    std::env::var_os(AUTH_FILE_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| "Cannot initialize the Codex sign-in HTTP client.".into())
}

/// The JSON claims of a JWT, read without verifying its signature, as Pi does: the
/// token came over TLS from the token endpoint or from the user's own auth file.
fn claims(token: &str) -> Option<Value> {
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

fn account_id(access_token: &str) -> Option<String> {
    claims(access_token)?
        .get(CLAIM)?
        .get("chatgpt_account_id")?
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

fn expiry(access_token: &str) -> Option<u64> {
    claims(access_token)?["exp"].as_u64()
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
            verifier: URL_SAFE_NO_PAD.encode(random(32)?),
            state: random(16)?
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
                .take(64)
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
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let hosts = [format!("localhost:{port}"), format!("127.0.0.1:{port}")];
        loop {
            let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let head = tokio::time::timeout(Duration::from_secs(5), async {
                let mut head = Vec::new();
                let mut byte = [0];
                while !head.ends_with(b"\r\n\r\n") && head.len() < 16384 {
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
                Duration::from_secs(2),
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
    let storage = Storage::new(state, repo)?;
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
    sign_in(&storage, &root, listener, REDIRECT_URI, pasted, true, |url, callback| {
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "Open this URL to sign in with Codex:\n{url}");
        if !callback {
            let _ = writeln!(
                err,
                "Port 1455 is in use (another Codex sign-in?), so the browser cannot return here."
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
        return Err("Port 1455 is in use and there is no terminal to paste the redirect URL into; finish the other sign-in and try again.".into());
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
    // Explorer hands a URL to the default browser without a shell parsing its `&`s. It exits
    // 1 even then, which only means no other program is tried.
    let programs = if cfg!(windows) {
        ["explorer"].as_slice()
    } else {
        &["xdg-open", "wslview"]
    };
    for program in programs {
        let mut command = Command::new(program);
        command
            .arg(url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Ok(Ok(status)) = tokio::time::timeout(Duration::from_secs(3), command.status()).await
            && status.success()
        {
            break;
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

fn credentials(tokens: TokenResponse, saved_at: u64) -> Result<Credentials, String> {
    let invalid = || "Invalid Codex token response; nothing was saved.".to_owned();
    let access_token = tokens.access_token.filter(|token| !token.is_empty());
    let refresh_token = tokens.refresh_token.filter(|token| !token.is_empty());
    let (Some(access_token), Some(refresh_token), Some(expires_in)) = (
        access_token,
        refresh_token,
        tokens.expires_in.filter(|n| *n > 0),
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
        let status = status.as_u16();
        let transient = status == 429 || status >= 500;
        let message = match refreshing {
            Some(storage) if terminal(code) => {
                storage.remove(CREDENTIALS)?;
                format!("The Codex refresh token is no longer valid ({code}); run `artifactize login codex`.")
            }
            Some(_) => {
                format!("Codex token refresh failed (HTTP {status}); the stored sign-in was kept.")
            }
            None if code == "invalid_grant" => {
                "The Codex authorization code was rejected (invalid_grant); run `artifactize login codex` again.".into()
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
    stored_token(&Storage::new(state, repo)?, &auth_root()?).await
}

async fn stored_token(storage: &Storage, root: &str) -> Result<Token, TokenError> {
    let _lock = storage.lock(LOCK).await?;
    let stored = storage
        .read::<Credentials>(CREDENTIALS)?
        .ok_or(LOGIN_REQUIRED)?;
    if stored.expires_at > now()?.saturating_add(REFRESH_MARGIN) {
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
    let refreshed = credentials(tokens, now()?)?;
    if refreshed.account_id != stored.account_id {
        return Err("The refreshed Codex token is for another ChatGPT account; run `artifactize login codex`.".into());
    }
    storage.save(CREDENTIALS, &refreshed)?;
    Ok(Token {
        access_token: refreshed.access_token,
        account_id: refreshed.account_id,
    })
}

/// The access token of a Codex auth file (`{"tokens":{"access_token",...}}`), read
/// once per use and never written. An expiring token is an error, never a refresh.
fn read_auth_file(path: &Path) -> Result<Token, String> {
    let name = path.display();
    let unreadable = |reason: &str| format!("{AUTH_FILE_VARIABLE}: cannot read {name}: {reason}.");
    let file = std::fs::File::open(path).map_err(|error| unreadable(&error.to_string()))?;
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(unreadable("it is not a regular file"));
    }
    let mut data = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|error| unreadable(&error.to_string()))?;
    if data.len() > 1024 * 1024 {
        return Err(unreadable("it is larger than 1 MiB"));
    }
    let auth: Value =
        serde_json::from_slice(&data).map_err(|_| unreadable("it is not a Codex auth file"))?;
    let access_token = auth
        .pointer("/tokens/access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| unreadable("it holds no ChatGPT sign-in tokens"))?;
    if expiry(access_token)
        .is_some_and(|exp| exp <= now().unwrap_or(u64::MAX).saturating_add(FILE_MARGIN))
    {
        return Err(format!(
            "The Codex access token in {name} has expired; sign in with Codex again (for example `codex login`). artifactize never refreshes that file."
        ));
    }
    let account_id = auth
        .pointer("/tokens/account_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .or_else(|| account_id(access_token))
        .ok_or_else(|| unreadable("its token names no ChatGPT account"))?;
    Ok(Token {
        access_token: access_token.to_owned(),
        account_id,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// `file`, `stored` or `none`.
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_file: Option<PathBuf>,
    pub expires_at: Option<u64>,
    pub expired: bool,
}

/// Inspect the sign-in offline: no lock, refresh, network or writes.
pub fn status(state: Option<&Path>, repo: Option<&Path>) -> Result<Status, String> {
    let now = now()?;
    if let Some(path) = auth_file() {
        // Reading fails on an expiring token; that is a state to report, not an error.
        let (expires_at, expired) = match read_auth_file(&path) {
            Ok(token) => (expiry(&token.access_token), false),
            Err(error) if error.starts_with("The Codex access token") => (None, true),
            Err(error) => return Err(error),
        };
        return Ok(Status {
            source: "file",
            auth_file: Some(path),
            expires_at,
            expired,
        });
    }
    let stored = Storage::inspect(state, repo)?.read::<Credentials>(CREDENTIALS)?;
    Ok(Status {
        source: if stored.is_some() { "stored" } else { "none" },
        auth_file: None,
        expires_at: stored.as_ref().map(|stored| stored.expires_at),
        expired: stored.is_some_and(|stored| stored.expires_at <= now),
    })
}

/// Revoke and remove artifactize's own tokens; a read-only auth file is left alone.
/// Returns whether the revocation was confirmed.
pub async fn logout(state: Option<&Path>, repo: Option<&Path>) -> Result<bool, String> {
    sign_out(&Storage::new(state, repo)?, &auth_root()?).await
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
        .timeout(Duration::from_secs(10))
        .json(
            &json!({"token":refresh_token,"token_type_hint":"refresh_token","client_id":CLIENT_ID}),
        )
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

#[cfg(test)]
mod tests;
