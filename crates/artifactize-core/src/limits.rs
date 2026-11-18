//! Machine-wide limits in `$STATE/limits.json`: `backends` caps how many Agent reviews of a
//! backend (a built-in or custom provider name) are in flight at once across every `verify`
//! that shares the state directory, `agentSessions` sets whether Agent conversations are saved
//! and how large their store grows, and `signoffIdleGraceSeconds` sets how long a Run with only
//! unclaimed sign-offs left stays open.
//!
//! ```json
//! {"backends": {"codex": 4, "openai": 16},
//!  "agentSessions": {"enabled": true, "maxBytes": 1073741824, "targetBytes": 805306368},
//!  "signoffIdleGraceSeconds": 120}
//! ```

use std::{collections::BTreeMap, io::Write, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::config::{Backend, Field};
#[cfg(test)]
mod backend_tests;

pub const FILE: &str = "limits.json";
/// Machine-wide limits are a small declaration; reject oversized files before JSON allocation.
const MAX_BYTES: u64 = 64 * 1024;
/// Reject implausible capacity declarations while allowing large hosts; absent backends
/// remain unlimited rather than being silently clamped to this declared-slot bound.
const MAX_SLOTS: u32 = 100_000;
/// The session store's default size that starts a collection: 1 GiB.
pub const SESSIONS_MAX_BYTES: u64 = 1024 * 1024 * 1024;
/// The size a collection brings the session store down to by default: 768 MiB.
pub const SESSIONS_TARGET_BYTES: u64 = 768 * 1024 * 1024;
/// Collect to three quarters of a small declared maximum to leave growth headroom.
/// Divide first, preserving quarter-block rounding and avoiding multiplication overflow.
const SESSION_TARGET_NUMERATOR: u64 = 3;
const SESSION_TARGET_DENOMINATOR: u64 = 4;
/// How long a Run with only unclaimed sign-offs left stays open by default: time for a person
/// who just asked for a sign-off to claim it before the Run ends.
pub const SIGNOFF_IDLE_GRACE: Duration = Duration::from_secs(120);
/// The longest idle grace accepted, a day: longer would keep a forgotten Run open for good.
pub const MAX_SIGNOFF_IDLE_GRACE: Duration = Duration::from_secs(24 * 60 * 60);
/// The field `signoffIdleGraceSeconds` is saved under.
const GRACE_FIELD: &str = "signoffIdleGraceSeconds";

/// Backend names to their machine-wide slot counts; a backend without an entry is unlimited.
/// It serializes as the limits in force, in the file's own field names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    backends: BTreeMap<Backend, u32>,
    agent_sessions: AgentSessions,
    #[serde(rename = "signoffIdleGraceSeconds", serialize_with = "seconds")]
    signoff_idle_grace: Duration,
}

fn seconds<S: serde::Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(duration.as_secs())
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            backends: BTreeMap::new(),
            agent_sessions: AgentSessions::default(),
            signoff_idle_grace: SIGNOFF_IDLE_GRACE,
        }
    }
}

/// Saving Agent conversations and the size bounds of their store. When the store grows past
/// `max_bytes`, the oldest sessions are deleted until it is at or below `target_bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessions {
    pub enabled: bool,
    pub max_bytes: u64,
    pub target_bytes: u64,
}

impl Default for AgentSessions {
    fn default() -> Self {
        Self {
            enabled: true,
            max_bytes: SESSIONS_MAX_BYTES,
            target_bytes: SESSIONS_TARGET_BYTES,
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Declared {
    #[serde(
        default,
        deserialize_with = "backend_limits",
        skip_serializing_if = "Option::is_none"
    )]
    backends: Option<BTreeMap<Backend, u32>>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    agent_sessions: Field<DeclaredSessions>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    signoff_idle_grace_seconds: Field<u64>,
}

impl Declared {
    /// Resolve the original declaration even when replacing its grace: invalid input must not
    /// be silently overwritten. An override has already passed the caller's duration checks.
    fn resolve(&mut self, override_grace: Option<Duration>) -> Result<Limits, String> {
        let agent_sessions = self
            .agent_sessions
            .value()
            .map(DeclaredSessions::resolve)
            .transpose()?
            .unwrap_or_default();
        let original_grace = self
            .signoff_idle_grace_seconds
            .value()
            .copied()
            .map(grace)
            .transpose()?;
        let signoff_idle_grace = match override_grace {
            Some(grace) => {
                self.signoff_idle_grace_seconds = Field::Value(grace.as_secs());
                grace
            }
            None => original_grace.unwrap_or(SIGNOFF_IDLE_GRACE),
        };
        Ok(Limits {
            backends: self.backends.clone().unwrap_or_default(),
            agent_sessions,
            signoff_idle_grace,
        })
    }
}

/// A declared idle grace, in whole seconds, within its bound.
fn grace(seconds: u64) -> Result<Duration, String> {
    let grace = Duration::from_secs(seconds);
    if grace > MAX_SIGNOFF_IDLE_GRACE {
        return Err(format!(
            "{GRACE_FIELD} must be at most {}.",
            MAX_SIGNOFF_IDLE_GRACE.as_secs()
        ));
    }
    Ok(grace)
}

fn backend_limits<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BTreeMap<Backend, u32>>, D::Error> {
    // Parse each external name once, retain its Backend, and preserve existing diagnostics
    // and lexicographic validation order at this declaration boundary.
    let names = BTreeMap::<String, u32>::deserialize(deserializer)?;
    names
        .into_iter()
        .map(|(name, limit)| {
            let backend = name
                .parse()
                .map_err(|error| serde::de::Error::custom(format!("backends: {error}")))?;
            if !(1..=MAX_SLOTS).contains(&limit) {
                return Err(serde::de::Error::custom(format!(
                    "backends.{name} must be between 1 and {MAX_SLOTS}."
                )));
            }
            Ok((backend, limit))
        })
        .collect::<Result<_, _>>()
        .map(Some)
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeclaredSessions {
    #[serde(default, skip_serializing_if = "Field::missing")]
    enabled: Field<bool>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    max_bytes: Field<u64>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    target_bytes: Field<u64>,
}

impl DeclaredSessions {
    /// The declared bounds over the defaults. Without `targetBytes`, a `maxBytes` at or below
    /// the 768 MiB default collects down to three quarters of it.
    fn resolve(&self) -> Result<AgentSessions, String> {
        let max_bytes = self
            .max_bytes
            .value()
            .copied()
            .unwrap_or(SESSIONS_MAX_BYTES);
        if max_bytes == 0 {
            return Err("agentSessions.maxBytes must be at least 1.".into());
        }
        let target_bytes = match self.target_bytes.value().copied() {
            Some(target) if target >= max_bytes => {
                return Err(format!(
                    "agentSessions.targetBytes ({target}) must be lower than maxBytes ({max_bytes})."
                ));
            }
            Some(target) => target,
            None if SESSIONS_TARGET_BYTES < max_bytes => SESSIONS_TARGET_BYTES,
            None => max_bytes / SESSION_TARGET_DENOMINATOR * SESSION_TARGET_NUMERATOR,
        };
        Ok(AgentSessions {
            enabled: self.enabled.value().copied().unwrap_or(true),
            max_bytes,
            target_bytes,
        })
    }
}

fn location(path: &Path, message: impl std::fmt::Display) -> String {
    format!("{}: {message}", crate::platform::path_text(path))
}

fn read_declared(path: &Path) -> Result<Declared, String> {
    let too_large = || {
        location(
            path,
            "limits.json must be a regular file no larger than 64 KiB.",
        )
    };
    let file = match crate::platform::open_regular(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Declared::default());
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            return Err(too_large());
        }
        Err(error) => return Err(location(path, error)),
    };
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(&file, MAX_BYTES + 1), &mut text)
        .map_err(|error| location(path, error))?;
    if text.len() as u64 > MAX_BYTES {
        return Err(too_large());
    }
    serde_json::from_str(&text).map_err(|error| {
        let message = error.to_string();
        // Backend validation previously followed decoding, so its diagnostics had no
        // JSON location suffix. Preserve that contract when parsing names at the edge.
        let message = if message.starts_with("backends:") || message.starts_with("backends.") {
            message
                .strip_suffix(&format!(
                    " at line {} column {}",
                    error.line(),
                    error.column()
                ))
                .unwrap_or(&message)
        } else {
            &message
        };
        location(path, message)
    })
}

impl Limits {
    /// Read `$STATE/limits.json`. No file means no limits; a file that is not a regular file,
    /// is not valid, names an unknown backend or a limit outside 1–100000, or declares
    /// `agentSessions` bounds out of order is an error.
    pub fn read(state: &Path) -> Result<Self, String> {
        let path = state.join(FILE);
        read_declared(&path)?
            .resolve(None)
            .map_err(|error| location(&path, error))
    }

    /// Save `grace` as the idle grace of the Runs started from now on, keeping every other
    /// declared limit as written, and return the limits that are now in force. A file that does
    /// not read is not overwritten.
    pub fn set_signoff_idle_grace(state: &Path, grace: Duration) -> Result<Self, String> {
        if grace > MAX_SIGNOFF_IDLE_GRACE || grace.subsec_nanos() != 0 {
            return Err(format!(
                "The sign-off idle grace must be whole seconds, at most {}.",
                MAX_SIGNOFF_IDLE_GRACE.as_secs()
            ));
        }
        let path = state.join(FILE);
        let mut declared = read_declared(&path)?;
        let limits = declared
            .resolve(Some(grace))
            .map_err(|error| location(&path, error))?;
        let mut text =
            serde_json::to_vec_pretty(&declared).map_err(|error| location(&path, error))?;
        text.push(b'\n');
        if text.len() as u64 > MAX_BYTES {
            return Err(location(
                &path,
                "limits.json must be no larger than 64 KiB.",
            ));
        }
        crate::platform::create_private_dir_all(state).map_err(|error| location(&path, error))?;
        let mut file =
            tempfile::NamedTempFile::new_in(state).map_err(|error| location(&path, error))?;
        file.write_all(&text)
            .map_err(|error| location(&path, error))?;
        file.persist(&path)
            .map_err(|error| location(&path, error.error))?;
        Ok(limits)
    }

    /// The slot count of a parsed backend; `None` when it is unlimited.
    pub fn limit(&self, backend: &Backend) -> Option<u32> {
        self.backends.get(backend).copied()
    }

    pub fn backends(&self) -> &BTreeMap<Backend, u32> {
        &self.backends
    }

    pub fn agent_sessions(&self) -> AgentSessions {
        self.agent_sessions
    }

    /// How long a Run with nothing but unclaimed sign-offs left stays open before it ends.
    pub fn signoff_idle_grace(&self) -> Duration {
        self.signoff_idle_grace
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read(text: &str) -> Result<Limits, String> {
        let directory = crate::test_os::tempdir();
        fs::write(directory.path().join(FILE), text).unwrap();
        Limits::read(directory.path())
    }

    #[test]
    fn agent_sessions_default_and_validate() {
        assert_eq!(
            read("{}").unwrap().agent_sessions(),
            AgentSessions {
                enabled: true,
                max_bytes: 1_073_741_824,
                target_bytes: 805_306_368,
            }
        );
        assert_eq!(
            read(r#"{"agentSessions":{"enabled":false}}"#)
                .unwrap()
                .agent_sessions(),
            AgentSessions {
                enabled: false,
                max_bytes: 1_073_741_824,
                target_bytes: 805_306_368,
            }
        );
        assert_eq!(
            read(r#"{"agentSessions":{"maxBytes":4000,"targetBytes":1000}}"#)
                .unwrap()
                .agent_sessions(),
            AgentSessions {
                enabled: true,
                max_bytes: 4000,
                target_bytes: 1000
            }
        );
        // A small maxBytes alone collects down to three quarters of it.
        assert_eq!(
            read(r#"{"agentSessions":{"maxBytes":4000}}"#)
                .unwrap()
                .agent_sessions()
                .target_bytes,
            3000
        );
        // Quarter-block rounding stays divide-then-multiply, not floor(3*n/4).
        for (max_bytes, target_bytes) in [(1, 0), (3, 0), (5, 3), (7, 3), (8, 6), (11, 6)] {
            assert_eq!(
                read(&format!(
                    r#"{{"agentSessions":{{"maxBytes":{max_bytes}}}}}"#
                ))
                .unwrap()
                .agent_sessions()
                .target_bytes,
                target_bytes
            );
        }
        assert_eq!(
            read(r#"{"agentSessions":{"maxBytes":18446744073709551615}}"#)
                .unwrap()
                .agent_sessions()
                .target_bytes,
            805306368
        );
        for (text, error) in [
            (
                r#"{"agentSessions":{"maxBytes":100,"targetBytes":100}}"#,
                "agentSessions.targetBytes (100) must be lower than maxBytes (100).",
            ),
            (
                r#"{"agentSessions":{"targetBytes":2147483648}}"#,
                "must be lower than maxBytes (1073741824)",
            ),
            (
                r#"{"agentSessions":{"maxBytes":0}}"#,
                "agentSessions.maxBytes must be at least 1.",
            ),
            (r#"{"agentSessions":{"maxbytes":10}}"#, "unknown field"),
            (r#"{"agentSessions":{"maxBytes":-1}}"#, "invalid value"),
            (r#"{"agentSessions":{"enabled":"no"}}"#, "invalid type"),
            (r#"{"agentSessions":true}"#, "invalid type"),
        ] {
            let message = read(text).unwrap_err();
            assert!(message.contains(error), "{text}: {message}");
            assert!(message.contains(FILE), "{message}");
        }
    }

    #[test]
    fn signoff_idle_grace_defaults_validates_and_saves_beside_other_limits() {
        assert_eq!(
            read("{}").unwrap().signoff_idle_grace(),
            Duration::from_secs(120)
        );
        assert_eq!(
            read(r#"{"signoffIdleGraceSeconds":0}"#)
                .unwrap()
                .signoff_idle_grace(),
            Duration::ZERO
        );
        for (text, error) in [
            (
                r#"{"signoffIdleGraceSeconds":86401}"#,
                "must be at most 86400",
            ),
            (r#"{"signoffIdleGraceSeconds":-1}"#, "invalid value"),
            (r#"{"signoffIdleGraceSeconds":"2m"}"#, "invalid type"),
        ] {
            let message = read(text).unwrap_err();
            assert!(message.contains(error), "{text}: {message}");
        }

        let directory = crate::test_os::tempdir();
        let state = directory.path().join("state");
        let saved = Limits::set_signoff_idle_grace(&state, Duration::from_secs(300)).unwrap();
        assert_eq!(saved.signoff_idle_grace(), Duration::from_secs(300));
        fs::write(
            state.join(FILE),
            r#"{"backends":{"codex":4},"agentSessions":{"maxBytes":4000,"enabled":null}}"#,
        )
        .unwrap();
        let saved = Limits::set_signoff_idle_grace(&state, Duration::ZERO).unwrap();
        assert_eq!(saved.signoff_idle_grace(), Duration::ZERO);
        assert_eq!(saved.limit(&crate::config::Provider::Codex.into()), Some(4));
        assert_eq!(saved.agent_sessions().max_bytes, 4000);
        assert_eq!(saved.agent_sessions().target_bytes, 3000);
        assert_eq!(Limits::read(&state).unwrap(), saved);
        let declared: serde_json::Value =
            serde_json::from_slice(&fs::read(state.join(FILE)).unwrap()).unwrap();
        assert_eq!(
            declared,
            serde_json::json!({
                "backends": {"codex": 4}, "agentSessions": {"maxBytes": 4000, "enabled": null},
                "signoffIdleGraceSeconds": 0,
            })
        );
        assert!(
            Limits::set_signoff_idle_grace(&state, Duration::from_secs(86401))
                .unwrap_err()
                .contains("at most 86400")
        );
        for invalid in [r#"{"unknown":1}"#, r#"{"signoffIdleGraceSeconds":86401}"#] {
            fs::write(state.join(FILE), invalid).unwrap();
            assert!(Limits::set_signoff_idle_grace(&state, Duration::ZERO).is_err());
            assert_eq!(fs::read_to_string(state.join(FILE)).unwrap(), invalid);
        }
    }
}
