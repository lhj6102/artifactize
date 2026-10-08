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
/// Bound one lookup's SQL work and response fan-out; clients batch larger key sets.
pub(crate) const MAX_LOOKUP_KEYS: usize = 1000;
/// Keep Axum's gross allocation cap slightly above a full record, so per-record
/// validation can return the existing precise size error within that headroom.
const BODY_LIMIT_HEADROOM: usize = 64 * 1024;

/// The fields this store indexes or authorizes are typed at the HTTP edge. The
/// remaining record is an opaque, extensible payload validated by its consumers.
#[derive(serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishRecord {
    schema: u32,
    key: crate::types::ReuseKey,
    verdict: crate::types::ExecutionStatus,
    execution_id: crate::types::ExecutionId,
    completed_at: String,
    profile: PublishProfile,
    #[serde(default, skip_serializing_if = "crate::config::Field::missing")]
    execution: crate::config::Field<Value>,
    #[serde(flatten)]
    payload: serde_json::Map<String, Value>,
}

#[derive(serde::Serialize, Deserialize)]
struct PublishProfile {
    kind: crate::config::ProfileKind,
    #[serde(flatten)]
    options: serde_json::Map<String, Value>,
}

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

struct Lookup {
    keys: Vec<String>,
}

/// Legacy object keys request an upgrade before current validation, even in mixed arrays
/// or with unknown fields. Map parsing also retains the previous last-duplicate-key behavior.
enum LookupRequest {
    Current(Lookup),
    Legacy,
    Invalid,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum LookupKey {
    Current(String),
    Legacy(crate::json::Object<std::collections::BTreeMap<String, crate::json::Ignored>>),
    Invalid(crate::json::Ignored),
}
impl<'de> Deserialize<'de> for LookupRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RequestVisitor;
        impl<'de> serde::de::Visitor<'de> for RequestVisitor {
            type Value = LookupRequest;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a lookup object")
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: S,
            ) -> Result<Self::Value, S::Error> {
                // The former Lookup derive accepted one positional keys field. Objects here
                // were ordinary invalid strings, never the map-only legacy upgrade path.
                let keys = sequence.next_element::<crate::json::Optional<Vec<String>>>()?;
                let mut extra = false;
                while sequence.next_element::<crate::json::Ignored>()?.is_some() {
                    extra = true;
                }
                match keys.and_then(|keys| keys.0) {
                    Some(keys) if !extra => Ok(LookupRequest::Current(Lookup { keys })),
                    _ => Ok(LookupRequest::Invalid),
                }
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut keys = crate::json::Optional::<Vec<LookupKey>>::default();
                let mut unknown = false;
                while let Some(field) = map.next_key::<String>()? {
                    if field == "keys" {
                        keys = map.next_value()?;
                    } else {
                        unknown = true;
                        map.next_value::<crate::json::Ignored>()?;
                    }
                }
                let Some(keys) = keys.0 else {
                    return Ok(LookupRequest::Invalid);
                };
                if keys
                    .iter()
                    .any(|key| matches!(key, LookupKey::Legacy(crate::json::Object(_))))
                {
                    return Ok(LookupRequest::Legacy);
                }
                if unknown {
                    return Ok(LookupRequest::Invalid);
                }
                let mut current = Vec::with_capacity(keys.len());
                for key in keys {
                    match key {
                        LookupKey::Current(key) => current.push(key),
                        _ => return Ok(LookupRequest::Invalid),
                    }
                }
                Ok(LookupRequest::Current(Lookup { keys: current }))
            }
        }
        deserializer.deserialize_any(RequestVisitor)
    }
}

fn parse_lookup(body: &[u8]) -> Result<Lookup, ApiError> {
    let expected = || ApiError::new(StatusCode::BAD_REQUEST, "Expected {\"keys\":[KEY,...]}.");
    match serde_json::from_slice::<LookupRequest>(body).map_err(|_| expected())? {
        LookupRequest::Current(lookup) => Ok(lookup),
        LookupRequest::Legacy => Err(ApiError::new(StatusCode::GONE, UPGRADE)),
        LookupRequest::Invalid => Err(expected()),
    }
}

#[cfg(test)]
mod lookup_tests;

async fn lookup(
    State(store): State<Store>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    authenticate(&store, &headers, Some(Scope::Read)).await?;
    let lookup = parse_lookup(&body)?;
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
    let mut record: PublishRecord = serde_json::from_slice(&body)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "Expected a JSON record."))?;
    let limit = if !matches!(record.execution, crate::config::Field::Missing) {
        MAX_FULL_BYTES
    } else {
        MAX_SUMMARY_BYTES
    };
    let completed_at = crate::broker::sortable(&record.completed_at);
    let (true, Some(completed_at)) = (
        record.schema == SCHEMA
            && record.key.as_str() == key
            && matches!(
                record.verdict,
                crate::types::ExecutionStatus::Green | crate::types::ExecutionStatus::Red
            )
            && record.execution_id.valid_wire(),
        completed_at,
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
    if record.profile.kind == crate::config::ProfileKind::Human
        && !principal.scopes.contains(&Scope::Human)
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Human sign-offs require the human scope.",
        ));
    }
    // The server, not the client, says who published and when.
    record
        .payload
        .insert("publisher".into(), json!(principal.name));
    record
        .payload
        .insert("publishedAt".into(), json!(crate::broker::now()));
    let created = store
        .insert(
            &key,
            &record.execution_id,
            &principal.name,
            &completed_at,
            serde_json::to_string(&record)
                .map_err(|error| ApiError::internal(error.to_string()))?,
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
        .layer(DefaultBodyLimit::max(MAX_FULL_BYTES + BODY_LIMIT_HEADROOM))
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
