//! Validated identities and persisted states. Their wire representations stay strings.

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, fmt, ops::Deref, str::FromStr};

mod timestamp;
pub use timestamp::Timestamp;

/// Bound external identities and saved session path segments, including pre-UUID sessions.
pub(crate) const MAX_ID_BYTES: usize = 200;
/// Mirrors retain the remote wire identity verbatim under this local-only namespace.
const REMOTE_EXECUTION_PREFIX: &str = "remote-";
/// Bound script output and reuse-key components without requiring a cryptographic digest.
pub(crate) const MAX_FINGERPRINT_BYTES: usize = 128;
pub(crate) use artifactize_tools::MAX_SAFE_JSON_INTEGER;
/// A SHA-256 digest has 32 bytes, encoded as 64 lowercase hexadecimal ASCII bytes
/// in reuse keys and remote definition hashes; this wire width is not a storage limit.
pub(crate) const SHA256_HEX_BYTES: usize = 64;
/// Eight digest bytes retain compact diagnostic manifests without changing wire width.
const DIGEST_PREFIX_HEX_BYTES: usize = 16;
/// Principal labels are bounded ASCII display identities, never bearer secrets.
const MAX_TOKEN_NAME_BYTES: usize = 64;

fn segment(value: &str) -> bool {
    (1..=MAX_ID_BYTES).contains(&value.len())
        && value.bytes().any(|byte| byte != b'.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

macro_rules! identity {
    ($name:ident, $valid:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(String);
        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if ($valid)(&value) {
                    Ok(Self(value))
                } else {
                    Err(format!("Invalid {}: {value:?}.", stringify!($name)))
                }
            }
        }
        impl FromStr for $name {
            type Err = String;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::try_from(value.to_owned())
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }
        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }
        impl PartialEq<$name> for String {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }
        impl PartialEq<$name> for str {
            fn eq(&self, other: &$name) -> bool {
                self == other.0
            }
        }
        impl PartialEq<$name> for &str {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }
        impl Borrow<String> for $name {
            fn borrow(&self) -> &String {
                &self.0
            }
        }
        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }
        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                value
                    .as_str()?
                    .parse()
                    .map_err(|error: String| FromSqlError::Other(error.into()))
            }
        }
    };
}
/// An Artifact name or a local Eval id: `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`.
fn name(value: &str) -> bool {
    crate::config::identifier(value, "").is_ok()
}

identity!(ArtifactName, name);
// A workspace-qualified Eval id: the Artifact name, `/`, and the Eval's local id.
identity!(EvalId, |value: &str| value
    .split_once('/')
    .is_some_and(|(artifact, eval)| name(artifact) && name(eval)));
identity!(RunId, segment);
identity!(RequestId, segment);
// Stored mirrors predate typed identities and can be 207 bytes: the unchanged
// `remote-` prefix plus a maximum-length 200-byte wire identity. Only that
// namespace gets the larger bound; all other identities retain their wire limit.
identity!(ExecutionId, |value: &str| segment(value)
    || value
        .strip_prefix(REMOTE_EXECUTION_PREFIX)
        .is_some_and(segment));
impl ExecutionId {
    /// Wire publications never get the extra local mirror namespace allowance.
    pub(crate) fn valid_wire(&self) -> bool {
        segment(self.as_str())
    }

    pub(crate) fn remote_mirror(&self) -> Result<Self, String> {
        if !self.valid_wire() {
            return Err("A remote execution ID exceeds the wire identity bound.".into());
        }
        format!("{REMOTE_EXECUTION_PREFIX}{self}").parse()
    }
}
identity!(SessionId, segment);
identity!(Fingerprint, |value: &str| (1..=MAX_FINGERPRINT_BYTES)
    .contains(&value.len())
    && value.bytes().all(
        |byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
    ));
/// Lowercase hexadecimal SHA-256: what reuse keys and definition hashes are.
fn sha256_hex(value: &str) -> bool {
    value.len() == SHA256_HEX_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

identity!(ReuseKey, sha256_hex);
// The SHA-256 of an eval's review strategy (`cache::eval_definition_hash`).
identity!(DefinitionHash, sha256_hex);
// State UUIDs and legacy session state identifiers share the saved segment domain.
identity!(StateId, segment);
identity!(Sha256Digest, sha256_hex);
// Compact diagnostic digests are the unchanged first eight bytes of a SHA-256.
identity!(DigestPrefix, |value: &str| value.len()
    == DIGEST_PREFIX_HEX_BYTES
    && value
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
identity!(StoreUrl, |value: &str| reqwest::Url::parse(value)
    .is_ok_and(|url| matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()));
// Human audit labels permit printable Unicode and spaces, but never control characters.
identity!(ReviewerId, |value: &str| !value.trim().is_empty()
    && value.len() <= MAX_ID_BYTES
    && !value.chars().any(char::is_control));

identity!(TokenName, |value: &str| (1..=MAX_TOKEN_NAME_BYTES)
    .contains(&value.len())
    && value.bytes().all(
        |b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')
    ));

macro_rules! status {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }
        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
        impl FromStr for $name {
            type Err = String;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($text => Ok(Self::$variant)),+,
                    _ => Err(format!("Invalid {}: {value:?}.", stringify!($name))),
                }
            }
        }
        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                self.as_str().to_sql()
            }
        }
        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                value
                    .as_str()?
                    .parse()
                    .map_err(|error: String| FromSqlError::Other(error.into()))
            }
        }
    };
}
status!(FailureCode {
   Authentication => "AUTHENTICATION",
   Quota => "QUOTA",
   RateLimit => "RATE_LIMIT",
   Transient => "TRANSIENT",
   Timeout => "TIMEOUT",
   Cancelled => "CANCELLED",
   ProviderBudgetExceeded => "PROVIDER_BUDGET_EXCEEDED",
   InvalidResult => "INVALID_RESULT",
   ProviderError => "PROVIDER_ERROR",
   AgentError => "AGENT_ERROR",
   BackendStopped => "BACKEND_STOPPED",
   PreparationFailed => "PREPARATION_FAILED",
   InputChanged => "INPUT_CHANGED",
   FingerprintRecheckFailed => "FINGERPRINT_RECHECK_FAILED",
   SpawnFailed => "SPAWN_FAILED",
   AbnormalExit => "ABNORMAL_EXIT",
   RuntimeError => "RUNTIME_ERROR",
   Superseded => "SUPERSEDED",
   OwnerDied => "OWNER_DIED",
   Spawn => "SPAWN",
});
impl std::ops::Deref for FailureCode {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl From<&str> for FailureCode {
    fn from(value: &str) -> Self {
        value.parse().expect("a program-defined failure code")
    }
}
status!(BackendStopCode { Authentication => "AUTHENTICATION", Quota => "QUOTA" });
impl TryFrom<FailureCode> for BackendStopCode {
    type Error = String;
    fn try_from(code: FailureCode) -> Result<Self, String> {
        code.as_str().parse()
    }
}
status!(FingerprintKind { Artifactsum => "artifactsum", Script => "script" });
status!(ArtifactValidationStatus { Basis => "BASIS", Green => "GREEN", Incomplete => "INCOMPLETE", Error => "ERROR", Red => "RED", Blocked => "BLOCKED", Wait => "WAIT", Stale => "STALE", Unreviewed => "UNREVIEWED" });
status!(RunStatus {
    Running => "RUNNING",
    Green => "GREEN",
    Red => "RED",
    Error => "ERROR",
    Incomplete => "INCOMPLETE",
});
status!(RequestStatus {
    Queued => "QUEUED",
    Running => "RUNNING",
    WaitingHuman => "WAITING_HUMAN",
    Green => "GREEN",
    Red => "RED",
    Error => "ERROR",
    BudgetExhausted => "BUDGET_EXHAUSTED",
    Stale => "STALE",
    Unreviewed => "UNREVIEWED",
    WaitDependency => "WAIT_DEPENDENCY",
    Blocked => "BLOCKED",
});
status!(ExecutionStatus {
    Running => "RUNNING",
    WaitingHuman => "WAITING_HUMAN",
    Green => "GREEN",
    Red => "RED",
    Error => "ERROR",
});
impl From<ExecutionStatus> for RequestStatus {
    fn from(status: ExecutionStatus) -> Self {
        match status {
            ExecutionStatus::Running => Self::Running,
            ExecutionStatus::WaitingHuman => Self::WaitingHuman,
            ExecutionStatus::Green => Self::Green,
            ExecutionStatus::Red => Self::Red,
            ExecutionStatus::Error => Self::Error,
        }
    }
}
impl TryFrom<RequestStatus> for ExecutionStatus {
    type Error = String;
    fn try_from(status: RequestStatus) -> Result<Self, String> {
        match status {
            RequestStatus::Running => Ok(Self::Running),
            RequestStatus::WaitingHuman => Ok(Self::WaitingHuman),
            RequestStatus::Green => Ok(Self::Green),
            RequestStatus::Red => Ok(Self::Red),
            RequestStatus::Error => Ok(Self::Error),
            other => Err(format!("Request state {other} is not an execution state.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewers_preserve_printable_labels_and_validate_json_and_sql() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        for label in [
            "alice",
            " reviewer ",
            "홍 길동",
            "a/b@host",
            &"x".repeat(200),
        ] {
            let reviewer: ReviewerId = label.parse().unwrap();
            let wire = serde_json::to_string(label).unwrap();
            assert_eq!(serde_json::to_string(&reviewer).unwrap(), wire);
            assert_eq!(serde_json::from_str::<ReviewerId>(&wire).unwrap(), reviewer);
            assert_eq!(
                db.query_row("SELECT ?", [&reviewer], |row| row.get::<_, ReviewerId>(0))
                    .unwrap(),
                reviewer
            );
        }
        for label in ["", "  ", "\t", "a\n", "a\0b", &"x".repeat(201)] {
            assert!(label.parse::<ReviewerId>().is_err());
            assert!(
                serde_json::from_str::<ReviewerId>(&serde_json::to_string(label).unwrap()).is_err()
            );
            assert!(
                db.query_row("SELECT ?", [label], |row| row.get::<_, ReviewerId>(0))
                    .is_err()
            );
        }
    }
    #[test]
    fn identities_validate_json_and_sql_edges() {
        for value in ["", ".", "..", "a/b", "a\\b", "a b", "a\n"] {
            assert!(value.parse::<SessionId>().is_err(), "{value:?}");
            assert!(value.parse::<RunId>().is_err());
            assert!(value.parse::<RequestId>().is_err());
            assert!(value.parse::<ExecutionId>().is_err());
        }
        assert!("legacy.session-1".parse::<SessionId>().is_ok());
        assert!("v1:Mixed_CASE.42".parse::<Fingerprint>().is_ok());
        assert!("x".repeat(128).parse::<Fingerprint>().is_ok());
        assert!("x".repeat(129).parse::<Fingerprint>().is_err());
        for value in ["f".repeat(63), "F".repeat(64), "g".repeat(64)] {
            assert!(value.parse::<ReuseKey>().is_err());
        }
        let key: ReuseKey = "a".repeat(64).parse().unwrap();
        assert_eq!(
            serde_json::to_string(&key).unwrap(),
            format!("\"{}\"", "a".repeat(64))
        );
        assert!(serde_json::from_str::<SessionId>("\"../escape\"").is_err());
        let db = rusqlite::Connection::open_in_memory().unwrap();
        assert!(
            db.query_row("SELECT '../escape'", [], |row| row.get::<_, RunId>(0))
                .is_err()
        );
    }
    #[test]
    fn store_identity_boundaries_preserve_text_and_reject_invalid_inputs() {
        assert_eq!(
            serde_json::to_string(&"state".parse::<StateId>().unwrap()).unwrap(),
            "\"state\""
        );
        let store: StoreUrl = "https://reviews.example/path".parse().unwrap();
        assert_eq!(
            serde_json::to_string(&store).unwrap(),
            "\"https://reviews.example/path\""
        );
        for invalid in [
            "fixture",
            "file:///path",
            "https://user:secret@reviews.example/",
        ] {
            assert!(invalid.parse::<StoreUrl>().is_err());
        }
        for invalid in ["", "bad name", "../escape"] {
            assert!(invalid.parse::<TokenName>().is_err());
        }
        for invalid in ["F".repeat(64), "f".repeat(63)] {
            assert!(invalid.parse::<Sha256Digest>().is_err());
        }
        assert!("f".repeat(64).parse::<Sha256Digest>().is_ok());
        assert!("f".repeat(16).parse::<DigestPrefix>().is_ok());
        assert!("f".repeat(64).parse::<DigestPrefix>().is_err());
        assert_eq!(
            serde_json::to_string(&FailureCode::OwnerDied).unwrap(),
            "\"OWNER_DIED\""
        );
        assert!(BackendStopCode::try_from(FailureCode::Timeout).is_err());
        assert_eq!(
            BackendStopCode::try_from(FailureCode::Quota).unwrap(),
            BackendStopCode::Quota
        );
        assert!("UNKNOWN".parse::<FailureCode>().is_err());
        let db = rusqlite::Connection::open_in_memory().unwrap();
        assert!(
            db.query_row("SELECT 'bad name'", [], |row| row.get::<_, TokenName>(0))
                .is_err()
        );
    }
    #[test]
    fn status_domains_retain_wire_names_and_reject_cross_domain_states() {
        assert_eq!(
            serde_json::to_string(&RunStatus::Incomplete).unwrap(),
            "\"INCOMPLETE\""
        );
        assert_eq!(
            serde_json::from_str::<RequestStatus>("\"WAITING_HUMAN\"").unwrap(),
            RequestStatus::WaitingHuman
        );
        assert!("QUEUED".parse::<ExecutionStatus>().is_err());
        assert!("WAITING_HUMAN".parse::<RunStatus>().is_err());
        assert!("INCOMPLETE".parse::<RequestStatus>().is_err());
    }
}

// OAuth account claims are opaque provider strings, historically any nonempty text.
identity!(CodexAccountId, |value: &str| !value.is_empty());
