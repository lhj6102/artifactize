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

use crate::remote::{MAX_FULL_BYTES, MAX_SUMMARY_BYTES, SCHEMA, valid_hash};

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

fn valid_key(key: &str) -> Result<(), ApiError> {
    if valid_hash(key) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "Keys are lowercase SHA-256 reuse keys.",
        ))
    }
}

/// What a client from before the 0.5 reuse key gets for its requests.
const UPGRADE: &str = "This review store holds artifactize 0.5 reuse keys (store schema 3); upgrade this client to artifactize 0.5 or later.";

async fn whoami(State(store): State<Store>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let principal = authenticate(&store, &headers, None).await?;
    Ok(Json(
        json!({"principal": principal.name, "scopes": principal.scopes}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lookup {
    keys: Vec<String>,
}

async fn lookup(
    State(store): State<Store>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    authenticate(&store, &headers, Some(Scope::Read)).await?;
    let expected = || ApiError::new(StatusCode::BAD_REQUEST, "Expected {\"keys\":[KEY,...]}.");
    let lookup: Value = serde_json::from_slice(&body).map_err(|_| expected())?;
    // 0.4 and earlier clients send {fingerprint, evalDefHash} objects.
    if lookup["keys"]
        .as_array()
        .is_some_and(|keys| keys.iter().any(Value::is_object))
    {
        return Err(ApiError::new(StatusCode::GONE, UPGRADE));
    }
    let lookup: Lookup = serde_json::from_value(lookup).map_err(|_| expected())?;
    if lookup.keys.len() > MAX_LOOKUP_KEYS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("At most {MAX_LOOKUP_KEYS} keys per lookup."),
        ));
    }
    for key in &lookup.keys {
        valid_key(key)?;
    }
    let entries = store
        .lookup(lookup.keys)
        .await
        .map_err(ApiError::internal)?;
    // Stored records are already JSON; splice them without reparsing.
    Ok((
        [(header::CONTENT_TYPE, "application/json")],
        format!("{{\"entries\":[{}]}}", entries.join(",")),
    )
        .into_response())
}

async fn publish(
    State(store): State<Store>,
    Path(key): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let principal = authenticate(&store, &headers, Some(Scope::Publish)).await?;
    valid_key(&key)?;
    let mut record: Value = serde_json::from_slice(&body)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "Expected a JSON record."))?;
    let limit = if record.get("execution").is_some() {
        MAX_FULL_BYTES
    } else {
        MAX_SUMMARY_BYTES
    };
    let kind = record["profile"]["kind"].as_str();
    let completed_at = record["completedAt"]
        .as_str()
        .and_then(crate::broker::sortable);
    let execution_id = record["executionId"].as_str().map(str::to_owned);
    let (true, Some(completed_at), Some(execution_id)) = (
        record["schema"] == SCHEMA
            && record["key"] == key.as_str()
            && matches!(record["verdict"].as_str(), Some("GREEN" | "RED"))
            && matches!(kind, Some("runtime" | "agent" | "human")),
        completed_at,
        execution_id.filter(|id| (1..=200).contains(&id.len())),
    ) else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "Records need schema {SCHEMA}, the path's key, a GREEN/RED verdict, a profile kind, an execution ID and an RFC 3339 completion time."
            ),
        ));
    };
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
            &key,
            &execution_id,
            &principal.name,
            &completed_at,
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

/// The 0.4 publish route, keyed by Eval definition hash and fingerprint.
async fn legacy_publish() -> ApiError {
    ApiError::new(StatusCode::GONE, UPGRADE)
}

pub fn router(store: Store) -> Router {
    Router::new()
        .route("/v1/whoami", get(whoami))
        .route("/v1/lookup", post(lookup))
        .route("/v1/entries/{key}", put(publish))
        .route(
            "/v1/entries/{eval_def_hash}/{fingerprint}",
            put(legacy_publish),
        )
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
