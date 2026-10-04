//! ChatGPT Sign in with ChatGPT, protected credentials, and serialized refresh;
//! the remote review store's configuration and token.

mod oauth;
pub mod remote;
mod storage;
#[cfg(test)]
mod tests;

use std::{
    io::Write,
    path::Path,
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::jwk::JwkSet;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, process::Command};

use oauth::{DIRECT_SCOPE, Discovery, PendingLogin, RESOURCE};
use storage::Storage;

const REGISTRATION: &str = "chatgpt-registration.json";
const CREDENTIALS: &str = "chatgpt.json";
const REFRESH_MARGIN: u64 = 60;
const LOGIN_REQUIRED: &str = "ChatGPT is not signed in; run `artifactize login chatgpt`.";

#[derive(Clone, Deserialize, Serialize)]
struct Account {
    issuer: String,
    subject: String,
    email: Option<String>,
}

impl Account {
    fn matches(&self, other: &Self) -> bool {
        self.issuer == other.issuer && self.subject == other.subject
    }
}

#[derive(Deserialize, Serialize)]
struct Registration {
    ext_agent_host_id: String,
    client_id: Option<String>,
    #[serde(alias = "identity")]
    account: Option<Account>,
}

#[derive(Deserialize, Serialize)]
struct Credentials {
    access_token: String,
    refresh_token: String,
    id_token: String,
    token_type: String,
    expires_at: u64,
    saved_at: u64,
    client_id: String,
    ext_agent_host_id: String,
    scopes: Vec<String>,
    #[serde(alias = "identity")]
    account: Account,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    id_token: Option<String>,
    token_type: String,
    expires_in: u64,
    scope: Option<String>,
}

fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| "The system clock is before the Unix epoch.".into())
}

fn registration(storage: &Storage) -> Result<Registration, String> {
    if let Some(registration) = storage.read::<Registration>(REGISTRATION)? {
        if registration
            .client_id
            .as_deref()
            .is_some_and(|id| !oauth::valid_client_id(id))
        {
            return Err("Invalid saved ChatGPT client registration.".into());
        }
        return Ok(registration);
    }
    let registration = Registration {
        ext_agent_host_id: oauth::host_id()?,
        client_id: None,
        account: None,
    };
    storage.save(REGISTRATION, &registration)?;
    Ok(registration)
}

/// Sign in interactively. The authorization URL goes to stderr, including with --json.
pub async fn login_chatgpt(state: Option<&Path>, repo: Option<&Path>) -> Result<(), String> {
    let storage = Storage::new(state, repo)?;
    let _lock = storage.lock().await?;
    let mut registration = registration(&storage)?;
    let client = oauth::client()?;
    let discovery = Discovery::load(&client).await?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|e| e.to_string())?;
    let mut pending = PendingLogin::new(&listener, &registration)?;
    let url = pending.authorize_url(&discovery.authorization_endpoint, &registration);
    writeln!(
        std::io::stderr().lock(),
        "Open this URL to sign in with ChatGPT:\n{url}"
    )
    .map_err(|e| e.to_string())?;
    open_browser(&url).await;
    let (code, client_id) = pending.receive(listener).await?;
    // Retain the issued ID even if the code exchange fails; never re-register on retry.
    registration.client_id = Some(client_id.clone());
    storage.save(REGISTRATION, &registration)?;
    let tokens = exchange_code(&client, &discovery, &pending, &code, &client_id).await?;
    let saved_at = now()?;
    let keys: JwkSet = oauth::get_json(&client, discovery.jwks_uri.as_str()).await?;
    let id_token = tokens
        .id_token
        .as_deref()
        .ok_or("ChatGPT did not return an ID token.")?;
    let account = oauth::validate_id_token(id_token, &keys, &client_id, Some(&pending.nonce))?;
    if registration
        .account
        .as_ref()
        .is_some_and(|saved| !saved.matches(&account))
    {
        return Err("ChatGPT account binding does not match this client registration; credentials were not replaced.".into());
    }
    let credentials = credentials(tokens, &registration, account.clone(), saved_at, None)?;
    registration.account = Some(account);
    storage.save(REGISTRATION, &registration)?;
    storage.save(CREDENTIALS, &credentials)
}

async fn exchange_code(
    client: &Client,
    discovery: &Discovery,
    pending: &PendingLogin,
    code: &str,
    client_id: &str,
) -> Result<TokenResponse, String> {
    token_request(
        client,
        &discovery.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", &pending.verifier),
            ("redirect_uri", &pending.redirect_uri),
            ("resource", RESOURCE),
        ],
        None,
    )
    .await
}

async fn open_browser(url: &Url) {
    for program in ["xdg-open", "wslview"] {
        let mut command = Command::new(program);
        command
            .arg(url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Ok(Ok(status)) =
            tokio::time::timeout(std::time::Duration::from_secs(3), command.status()).await
            && status.success()
        {
            break;
        }
    }
}

/// Return a usable bearer token, re-reading under a cross-process refresh lock.
/// Callers must never print or log the returned token.
pub async fn chatgpt_access_token(
    state: Option<&Path>,
    repo: Option<&Path>,
) -> Result<String, String> {
    let storage = Storage::new(state, repo)?;
    {
        let _lock = storage.lock().await?;
        let stored = load_credentials(&storage)?;
        if stored.expires_at > now()?.saturating_add(REFRESH_MARGIN) {
            return Ok(stored.access_token);
        }
    }
    let client = oauth::client()?;
    let discovery = Discovery::load(&client).await?;
    refresh(&storage, &client, &discovery).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    pub present: bool,
    pub expires_at: Option<u64>,
    pub expired: bool,
}

/// Inspect local metadata only: no lock, refresh, network, or credential writes.
pub fn chatgpt_status(state: Option<&Path>, repo: Option<&Path>) -> Result<LoginStatus, String> {
    let storage = Storage::inspect(state, repo)?;
    let stored = read_credentials(&storage)?;
    Ok(LoginStatus {
        present: stored.is_some(),
        expires_at: stored.as_ref().map(|stored| stored.expires_at),
        expired: match stored {
            Some(stored) => stored.expires_at <= now()?,
            None => false,
        },
    })
}

fn load_credentials(storage: &Storage) -> Result<Credentials, String> {
    read_credentials(storage)?.ok_or_else(|| LOGIN_REQUIRED.into())
}

fn read_credentials(storage: &Storage) -> Result<Option<Credentials>, String> {
    let Some(stored) = storage.read::<Credentials>(CREDENTIALS)? else {
        return Ok(None);
    };
    let registration = storage
        .read::<Registration>(REGISTRATION)?
        .ok_or(LOGIN_REQUIRED)?;
    if registration.client_id.as_deref() != Some(&stored.client_id)
        || registration.ext_agent_host_id != stored.ext_agent_host_id
        || !registration
            .account
            .as_ref()
            .is_some_and(|account| account.matches(&stored.account))
        || !stored.scopes.iter().any(|scope| scope == DIRECT_SCOPE)
        || !stored.token_type.eq_ignore_ascii_case("Bearer")
        || stored.access_token.is_empty()
        || stored.refresh_token.is_empty()
    {
        return Err("Invalid ChatGPT credential binding; run `artifactize login chatgpt`.".into());
    }
    Ok(Some(stored))
}

async fn refresh(
    storage: &Storage,
    client: &Client,
    discovery: &Discovery,
) -> Result<String, String> {
    let _lock = storage.lock().await?;
    let stored = load_credentials(storage)?;
    if stored.expires_at > now()?.saturating_add(REFRESH_MARGIN) {
        return Ok(stored.access_token);
    }
    let tokens = token_request(
        client,
        &discovery.token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &stored.client_id),
            ("refresh_token", &stored.refresh_token),
            ("resource", RESOURCE),
        ],
        Some(storage),
    )
    .await?;
    let saved_at = now()?;
    let account = if let Some(token) = &tokens.id_token {
        let keys: JwkSet = oauth::get_json(client, discovery.jwks_uri.as_str()).await?;
        let account = oauth::validate_id_token(token, &keys, &stored.client_id, None)?;
        if !account.matches(&stored.account) {
            return Err("Refreshed ChatGPT account binding does not match this registration; run `artifactize login chatgpt`.".into());
        }
        account
    } else {
        stored.account.clone()
    };
    let registration = Registration {
        ext_agent_host_id: stored.ext_agent_host_id.clone(),
        client_id: Some(stored.client_id.clone()),
        account: Some(account.clone()),
    };
    let credentials = credentials(tokens, &registration, account, saved_at, Some(&stored))?;
    storage.save(CREDENTIALS, &credentials)?;
    Ok(credentials.access_token)
}

fn credentials(
    tokens: TokenResponse,
    registration: &Registration,
    account: Account,
    saved_at: u64,
    previous: Option<&Credentials>,
) -> Result<Credentials, String> {
    let scopes: Vec<String> = match tokens.scope {
        Some(scope) => scope.split_ascii_whitespace().map(str::to_owned).collect(),
        None => previous
            .ok_or("ChatGPT token response is missing granted scopes.")?
            .scopes
            .clone(),
    };
    if !scopes.iter().any(|scope| scope == DIRECT_SCOPE) {
        return Err("ChatGPT plan usage was not granted (chatgpt.tokens.use.direct); credentials were not saved. Enable plan sharing and run `artifactize login chatgpt`.".into());
    }
    if !tokens.token_type.eq_ignore_ascii_case("Bearer")
        || tokens.access_token.is_empty()
        || tokens.refresh_token.is_empty()
        || tokens.expires_in <= REFRESH_MARGIN
    {
        return Err("Invalid ChatGPT token response; run `artifactize login chatgpt`.".into());
    }
    let expires_at = saved_at
        .checked_add(tokens.expires_in)
        .ok_or("Invalid ChatGPT token expiry.")?;
    let id_token = tokens
        .id_token
        .or_else(|| previous.map(|old| old.id_token.clone()))
        .filter(|token| !token.is_empty())
        .ok_or("ChatGPT token response is missing its ID token.")?;
    Ok(Credentials {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        id_token,
        token_type: tokens.token_type,
        expires_at,
        saved_at,
        scopes,
        account,
        client_id: registration
            .client_id
            .clone()
            .ok_or("Missing ChatGPT client registration.")?,
        ext_agent_host_id: registration.ext_agent_host_id.clone(),
    })
}

async fn token_request(
    client: &Client,
    endpoint: &Url,
    form: &[(&str, &str)],
    refresh_storage: Option<&Storage>,
) -> Result<TokenResponse, String> {
    let response = client
        .post(endpoint.clone())
        .form(form)
        .send()
        .await
        .map_err(|_| {
            "ChatGPT token request failed; check your connection. Credentials were kept.".to_owned()
        })?;
    let status = response.status();
    if !status.is_success() {
        let error: serde_json::Value = response.json().await.unwrap_or_default();
        let code = error["error"]
            .as_str()
            .or_else(|| error["error"]["code"].as_str())
            .unwrap_or("");
        if let Some(storage) = refresh_storage
            && oauth::terminal_refresh_error(code)
        {
            storage.remove(CREDENTIALS)?;
        }
        // Never echo response bodies: they may contain credentials or authorization codes.
        return Err(format!(
            "{} (HTTP {})",
            oauth::oauth_message(code, refresh_storage.is_some()),
            status.as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "Invalid ChatGPT token response; credentials were not replaced.".into())
}

/// Clear local tokens, retaining registration metadata. Returns whether revocation was confirmed.
pub async fn logout_chatgpt(state: Option<&Path>, repo: Option<&Path>) -> Result<bool, String> {
    let storage = Storage::new(state, repo)?;
    let _lock = storage.lock().await?;
    let Some(stored) = storage.read::<Credentials>(CREDENTIALS)? else {
        return Ok(true);
    };
    let revoked = revoke(&stored).await;
    storage.remove(CREDENTIALS)?;
    Ok(revoked)
}

async fn revoke(stored: &Credentials) -> bool {
    let Ok(client) = oauth::client() else {
        return false;
    };
    let Ok(discovery) = Discovery::load(&client).await else {
        return false;
    };
    let Some(endpoint) = discovery.revocation_endpoint else {
        return false;
    };
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(250 * attempt)).await;
        }
        match client
            .post(endpoint.clone())
            .form(&[
                ("token", stored.refresh_token.as_str()),
                ("token_type_hint", "refresh_token"),
                ("client_id", stored.client_id.as_str()),
            ])
            .send()
            .await
        {
            Ok(response) if response.status() == reqwest::StatusCode::OK => return true,
            Ok(response) if !response.status().is_server_error() => return false,
            _ => {}
        }
    }
    false
}
