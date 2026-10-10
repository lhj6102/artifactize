//! Validated identities and persisted states. Their wire representations stay strings.

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, fmt, ops::Deref, str::FromStr};

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
identity!(ReuseKey, |value: &str| value.len() == SHA256_HEX_BYTES
    && value.bytes().all(
        |byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
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
