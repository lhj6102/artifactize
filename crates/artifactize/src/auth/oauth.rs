use std::{collections::BTreeMap, time::Duration};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use reqwest::{Client, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::{Identity, Registration, now};

pub(super) const ISSUER: &str = "https://auth.openai.com";
pub(super) const DISCOVERY: &str = "https://auth.openai.com/.well-known/openid-configuration";
pub(super) const RESOURCE: &str = "https://api.openai.com/v1";
pub(super) const DIRECT_SCOPE: &str = "chatgpt.tokens.use.direct";
pub(super) const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const CALLBACK_PATH: &str = "/auth/callback";

#[derive(Deserialize)]
pub(super) struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub jwks_uri: Url,
    pub revocation_endpoint: Option<Url>,
}

impl Discovery {
    pub async fn load(client: &Client) -> Result<Self, String> {
        let metadata: Self = get_json(client, DISCOVERY).await?;
        if metadata.issuer != ISSUER {
            return Err("Unexpected ChatGPT discovery issuer.".into());
        }
        for endpoint in [
            &metadata.authorization_endpoint,
            &metadata.token_endpoint,
            &metadata.jwks_uri,
        ]
        .into_iter()
        .chain(metadata.revocation_endpoint.as_ref())
        {
            if endpoint.scheme() != "https"
                || endpoint.host_str() != Some("auth.openai.com")
                || !endpoint.username().is_empty()
                || endpoint.password().is_some()
                || endpoint.port_or_known_default() != Some(443)
            {
                return Err("Unexpected ChatGPT discovery endpoint.".into());
            }
        }
        Ok(metadata)
    }
}

pub(super) fn client() -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| "Cannot initialize the ChatGPT HTTP client.".into())
}

pub(super) async fn get_json<T: serde::de::DeserializeOwned>(
    client: &Client,
    url: &str,
) -> Result<T, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "ChatGPT metadata request failed; check your connection.".to_owned())?;
    if !response.status().is_success() {
        return Err(format!(
            "ChatGPT metadata request failed (HTTP {}).",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "Invalid ChatGPT metadata response.".into())
}

pub(super) fn random_value() -> Result<String, String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| "Cannot obtain secure randomness.".to_owned())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(super) fn host_id() -> Result<String, String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Cannot obtain secure randomness.".to_owned())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

pub(super) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub(super) struct PendingLogin {
    pub state: String,
    pub nonce: String,
    pub verifier: String,
    pub redirect_uri: String,
    client_id: Option<String>,
    consumed: bool,
}

impl PendingLogin {
    pub fn new(listener: &TcpListener, registration: &Registration) -> Result<Self, String> {
        Ok(Self {
            state: random_value()?,
            nonce: random_value()?,
            verifier: random_value()?,
            redirect_uri: format!(
                "http://127.0.0.1:{}{CALLBACK_PATH}",
                listener.local_addr().map_err(|e| e.to_string())?.port()
            ),
            client_id: registration.client_id.clone(),
            consumed: false,
        })
    }

    pub fn authorize_url(&self, endpoint: &Url, registration: &Registration) -> Url {
        let mut url = endpoint.clone();
        let mut query = url.query_pairs_mut();
        query.extend_pairs([
            (
                "client_id",
                self.client_id.as_deref().unwrap_or("dynamic_agent_client"),
            ),
            ("ext_agent_host_id", registration.ext_agent_host_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", &self.redirect_uri),
            ("scope", SCOPES),
            ("resource", RESOURCE),
            ("state", &self.state),
            ("nonce", &self.nonce),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge(&self.verifier)),
        ]);
        if self.client_id.is_none() {
            query.append_pair("agent_name_hint", "artifactize");
        }
        // No id_token_hint: the URL is printed, and must never expose stored tokens.
        drop(query);
        url
    }

    pub fn callback(&mut self, target: &str) -> Result<(String, String), String> {
        if std::mem::replace(&mut self.consumed, true) {
            return Err("ChatGPT callback was already consumed.".into());
        }
        if !target.starts_with('/') || target.starts_with("//") || target.contains('#') {
            return Err("Invalid ChatGPT callback path.".into());
        }
        let url = Url::parse(&format!("http://127.0.0.1{target}"))
            .map_err(|_| "Invalid ChatGPT callback URL.".to_owned())?;
        if url.path() != CALLBACK_PATH {
            return Err("Invalid ChatGPT callback path.".into());
        }
        let mut parameters = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            if parameters
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err("Duplicate ChatGPT callback parameter.".into());
            }
        }
        if parameters.get("state") != Some(&self.state) {
            return Err("ChatGPT callback state mismatch.".into());
        }
        if let Some(error) = parameters.get("error") {
            return Err(oauth_message(error, false));
        }
        let code = parameters
            .get("code")
            .filter(|code| !code.is_empty())
            .ok_or("ChatGPT callback is missing its authorization code.")?;
        let client_id = match (&self.client_id, parameters.get("client_id")) {
            (Some(expected), Some(actual)) if expected != actual => {
                return Err("ChatGPT callback client ID mismatch.".into());
            }
            (Some(expected), _) => expected,
            (None, Some(issued)) if valid_client_id(issued) => issued,
            _ => return Err("ChatGPT registration did not return an issued client ID.".into()),
        };
        Ok((code.clone(), client_id.clone()))
    }

    pub async fn receive(&mut self, listener: TcpListener) -> Result<(String, String), String> {
        tokio::time::timeout(Duration::from_secs(300), async {
            loop {
                let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
                let request = tokio::time::timeout(Duration::from_secs(5), async {
                    let mut header = Vec::new();
                    let mut byte = [0];
                    while !header.ends_with(b"\r\n\r\n") && header.len() < 16384 {
                        if stream.read(&mut byte).await? == 0 { break; }
                        header.push(byte[0]);
                    }
                    Ok::<_, std::io::Error>(header)
                }).await;
                let Ok(Ok(header)) = request else { continue; };
                let header = std::str::from_utf8(&header).unwrap_or("");
                let parts: Vec<_> = header.lines().next().unwrap_or("").split_whitespace().collect();
                let expected_host = self.redirect_uri.strip_prefix("http://").unwrap().split('/').next().unwrap();
                let hosts: Vec<_> = header.lines().skip(1).filter_map(|line| line.split_once(':'))
                    .filter(|(name, _)| name.eq_ignore_ascii_case("host")).collect();
                let valid = header.ends_with("\r\n\r\n") && parts.len() == 3 && parts[0] == "GET"
                    && matches!(parts[2], "HTTP/1.0" | "HTTP/1.1") && hosts.len() == 1 && hosts[0].1.trim() == expected_host;
                let callback = valid && parts[1].split('?').next() == Some(CALLBACK_PATH);
                let result = if callback { Some(self.callback(parts[1])) } else { None };
                let (status, body) = match &result {
                    Some(Ok(_)) => ("200 OK", "Callback received. Return to artifactize to finish sign-in."),
                    Some(Err(_)) => ("400 Bad Request", "Sign-in rejected. Return to artifactize."),
                    None => ("404 Not Found", "Not found."),
                };
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = tokio::time::timeout(Duration::from_secs(2), stream.write_all(response.as_bytes())).await;
                if let Some(result) = result { return result; }
            }
        }).await.map_err(|_| "ChatGPT sign-in timed out; run `artifactize login chatgpt` again.".to_owned())?
    }
}

pub(super) fn valid_client_id(value: &str) -> bool {
    value.starts_with("oaiapp_")
        && value.len() > 7
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: serde_json::Value,
    exp: u64,
    iat: u64,
    nonce: Option<String>,
    azp: Option<String>,
    email: Option<String>,
}

pub(super) fn validate_id_token(
    token: &str,
    keys: &JwkSet,
    client_id: &str,
    nonce: Option<&str>,
) -> Result<Identity, String> {
    let invalid = || "ChatGPT ID token failed signature or claims validation.".to_owned();
    let header = decode_header(token).map_err(|_| invalid())?;
    if header.alg != Algorithm::RS256 {
        return Err(invalid());
    }
    let key = header
        .kid
        .as_deref()
        .and_then(|kid| keys.find(kid))
        .ok_or_else(invalid)?;
    let key = DecodingKey::from_jwk(key).map_err(|_| invalid())?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.leeway = 0;
    validation.validate_nbf = true;
    let claims = decode::<Claims>(token, &key, &validation)
        .map_err(|_| invalid())?
        .claims;
    if claims.sub.is_empty()
        || claims.exp <= now()?
        || claims.iat > now()?.saturating_add(60)
        || nonce.is_some_and(|nonce| claims.nonce.as_deref() != Some(nonce))
        || claims.azp.as_deref().is_some_and(|azp| azp != client_id)
        || (claims.aud.as_array().is_some_and(|aud| aud.len() > 1)
            && claims.azp.as_deref() != Some(client_id))
    {
        return Err(invalid());
    }
    Ok(Identity {
        issuer: claims.iss,
        subject: claims.sub,
        email: claims.email,
    })
}

pub(super) fn terminal_refresh_error(code: &str) -> bool {
    matches!(
        code,
        "invalid_grant"
            | "invalid_refresh_token"
            | "token_expired"
            | "refresh_token_expired"
            | "refresh_token_invalidated"
            | "refresh_token_reused"
    )
}

pub(super) fn oauth_message(code: &str, refresh: bool) -> String {
    if refresh && terminal_refresh_error(code) {
        return "ChatGPT refresh token is invalid or expired; run `artifactize login chatgpt`."
            .into();
    }
    match code {
        "access_denied" => "ChatGPT sign-in was declined (access_denied).".into(),
        "subscription_sharing_user_not_eligible" => "This account is not eligible for ChatGPT plan sharing (subscription_sharing_user_not_eligible). Check ChatGPT eligibility; repeated sign-ins will not help.".into(),
        "invalid_grant" => "ChatGPT authorization code was rejected (invalid_grant); run `artifactize login chatgpt` again.".into(),
        "invalid_client" => "ChatGPT rejected the registered client (invalid_client); check the client registration.".into(),
        _ => "ChatGPT authorization failed; check your account permissions and try again.".into(),
    }
}
