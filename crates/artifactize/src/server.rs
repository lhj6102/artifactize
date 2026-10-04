//! `artifactize server`: the shared remote review store over its own SQLite file.

mod store;

pub use store::{DATABASE, MAX_BYTES, MAX_ENTRIES, Revocation, Scope, Store, Token};

use std::net::SocketAddr;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::remote::{MAX_FULL_BYTES, MAX_SUMMARY_BYTES, valid_fingerprint, valid_hash};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8417";
const MAX_LOOKUP_KEYS: usize = 1000;

struct ApiError(StatusCode, String);

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self(status, message.into())
    }

    fn internal(error: String) -> Self {
        // Store errors never contain tokens; only their SHA-256 reaches SQLite.
        eprintln!("Review store error: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal review store error.",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.0, Json(json!({"error": self.1}))).into_response();
        if self.0 == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

struct Principal {
    name: String,
    scopes: Vec<Scope>,
}

async fn authenticate(
    store: &Store,
    headers: &HeaderMap,
    required: Option<Scope>,
) -> Result<Principal, ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, token)| scheme.eq_ignore_ascii_case("bearer") && !token.is_empty())
        .map(|(_, token)| token.trim())
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "A bearer token is required."))?;
    let (name, scopes) = store
        .authenticate(token)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "Invalid or revoked token."))?;
    if let Some(scope) = required
        && !scopes.contains(&scope)
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            format!("This token lacks the {} scope.", scope.name()),
        ));
    }
    Ok(Principal { name, scopes })
}

fn valid_key(eval_def_hash: &str, fingerprint: &str) -> Result<(), ApiError> {
    if valid_hash(eval_def_hash) && valid_fingerprint(fingerprint) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "Keys need a lowercase SHA-256 Eval definition hash and a valid fingerprint.",
        ))
    }
}

async fn whoami(State(store): State<Store>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let principal = authenticate(&store, &headers, None).await?;
    Ok(Json(
        json!({"principal": principal.name, "scopes": principal.scopes}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lookup {
    keys: Vec<Key>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Key {
    #[serde(alias = "staleKey")]
    fingerprint: String,
    eval_def_hash: String,
}

/// What 0.3 clients call the fingerprint. Through 0.4.x the server accepts it in published
/// records and lookup keys and answers such lookups in the 0.3 record shape.
const LEGACY_KEY: &str = "staleKey";

/// Rename a record's key field, and its full execution's, between the 0.3 and current names.
fn rename_key(record: &mut Value, from: &str, to: &str) {
    fn rename(object: Option<&mut Value>, from: &str, to: &str) {
        if let Some(object) = object.and_then(Value::as_object_mut)
            && !object.contains_key(to)
            && let Some(value) = object.remove(from)
        {
            object.insert(to.to_owned(), value);
        }
    }
    rename(record.get_mut("execution"), from, to);
    rename(Some(record), from, to);
}

async fn lookup(
    State(store): State<Store>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    authenticate(&store, &headers, Some(Scope::Read)).await?;
    let expected = || ApiError::new(StatusCode::BAD_REQUEST, "Expected {\"keys\":[...]}.");
    let lookup: Value = serde_json::from_slice(&body).map_err(|_| expected())?;
    let legacy = lookup["keys"]
        .as_array()
        .is_some_and(|keys| keys.iter().any(|key| key.get(LEGACY_KEY).is_some()));
    let lookup: Lookup = serde_json::from_value(lookup).map_err(|_| expected())?;
    if lookup.keys.len() > MAX_LOOKUP_KEYS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("At most {MAX_LOOKUP_KEYS} keys per lookup."),
        ));
    }
    let mut keys = Vec::with_capacity(lookup.keys.len());
    for key in lookup.keys {
        valid_key(&key.eval_def_hash, &key.fingerprint)?;
        keys.push((key.eval_def_hash, key.fingerprint));
    }
    let mut entries = store.lookup(keys).await.map_err(ApiError::internal)?;
    if legacy {
        for entry in &mut entries {
            let mut record: Value = serde_json::from_str(entry)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            rename_key(&mut record, "fingerprint", LEGACY_KEY);
            *entry = record.to_string();
        }
    }
    // Stored records are already JSON; splice them without reparsing.
    Ok((
        [(header::CONTENT_TYPE, "application/json")],
        format!("{{\"entries\":[{}]}}", entries.join(",")),
    )
        .into_response())
}

async fn publish(
    State(store): State<Store>,
    Path((eval_def_hash, fingerprint)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let principal = authenticate(&store, &headers, Some(Scope::Publish)).await?;
    valid_key(&eval_def_hash, &fingerprint)?;
    let mut record: Value = serde_json::from_slice(&body)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "Expected a JSON record."))?;
    // Records are stored under the current name, whichever name the client sent.
    rename_key(&mut record, LEGACY_KEY, "fingerprint");
    let limit = if record.get("execution").is_some() {
        MAX_FULL_BYTES
    } else {
        MAX_SUMMARY_BYTES
    };
    let kind = record["profile"]["kind"].as_str();
    if record["schema"] != 1
        || record["fingerprint"] != fingerprint.as_str()
        || record["evalDefHash"] != eval_def_hash.as_str()
        || !matches!(record["verdict"].as_str(), Some("GREEN" | "RED"))
        || !matches!(kind, Some("runtime" | "agent" | "human"))
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "Records need schema 1, the path's fingerprint and hash, a GREEN/RED verdict and a profile kind.",
        ));
    }
    if body.len() > limit {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("Record exceeds {limit} bytes."),
        ));
    }
    if kind == Some("human") && !principal.scopes.contains(&Scope::Human) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Human sign-offs require the human scope.",
        ));
    }
    // The server, not the client, says who published and when.
    record["publisher"] = json!(principal.name);
    record["publishedAt"] = json!(crate::broker::now());
    let created = store
        .insert(
            &eval_def_hash,
            &fingerprint,
            &principal.name,
            record.to_string(),
        )
        .await
        .map_err(ApiError::internal)?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(json!({"created": created})),
    ))
}

pub fn router(store: Store) -> Router {
    Router::new()
        .route("/v1/whoami", get(whoami))
        .route("/v1/lookup", post(lookup))
        .route("/v1/entries/{eval_def_hash}/{fingerprint}", put(publish))
        .layer(DefaultBodyLimit::max(MAX_FULL_BYTES + 64 * 1024))
        .with_state(store)
}

/// Serve until cancelled; `ready` receives the bound address once listening.
pub async fn serve(
    store: Store,
    listen: SocketAddr,
    ready: impl FnOnce(SocketAddr) -> Result<(), String>,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|e| format!("Cannot listen on {listen}: {e}"))?;
    ready(listener.local_addr().map_err(|e| e.to_string())?)?;
    axum::serve(listener, router(store))
        .with_graceful_shutdown(cancellation.cancelled_owned())
        .await
        .map_err(|e| e.to_string())
}
