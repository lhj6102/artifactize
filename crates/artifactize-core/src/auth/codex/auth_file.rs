//! Parsing and inspecting a user's read-only Codex credentials.
use super::*;
use std::io::Read;

/// A read-only auth file's token must outlive this many seconds.
pub(super) const FILE_MARGIN: Duration = Duration::from_secs(60);

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
/// Why a Codex auth file could not be read as a usable token: the expired case is a state
/// callers may want to report on its own, so it is a variant rather than a message prefix.
#[derive(Debug)]
pub(super) enum AuthFileError {
    /// The token in the file has expired; the message explains how to fix it.
    Expired(String),
    /// The file itself, or what it holds, could not be used; the message explains why.
    Unreadable(String),
}

impl std::fmt::Display for AuthFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expired(message) | Self::Unreadable(message) => write!(f, "{message}"),
        }
    }
}

impl From<AuthFileError> for String {
    fn from(error: AuthFileError) -> Self {
        error.to_string()
    }
}

impl From<AuthFileError> for TokenError {
    fn from(error: AuthFileError) -> Self {
        error.to_string().into()
    }
}

pub(super) struct FileToken {
    pub(super) token: Token,
    pub(super) expires_at: Option<Timestamp>,
}

pub(super) fn read_auth_file_at(path: &Path, now: Timestamp) -> Result<FileToken, AuthFileError> {
    let name = crate::platform::path_text(path);
    let unreadable = |reason: &str| {
        AuthFileError::Unreadable(format!(
            "{AUTH_FILE_VARIABLE}: cannot read {name}: {reason}."
        ))
    };
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
    let claims = claims(access_token).unwrap_or_default();
    let expires_at = claims.exp.0;
    if expires_at.is_some_and(|exp| exp <= now.saturating_add(FILE_MARGIN)) {
        return Err(AuthFileError::Expired(format!(
            "The Codex access token in {name} has expired; sign in with Codex again (for example `codex login`). artifactize never refreshes that file."
        )));
    }
    let account_id = tokens
        .as_ref()
        .and_then(|tokens| tokens.account_id.0.as_deref())
        .filter(|id| !id.is_empty())
        .and_then(|id| id.parse().ok())
        .or_else(|| claims.account_id())
        .ok_or_else(|| unreadable("its token names no ChatGPT account"))?;
    Ok(FileToken {
        token: Token {
            access_token: access_token.to_owned(),
            account_id,
        },
        expires_at,
    })
}
