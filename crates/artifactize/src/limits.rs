//! Machine-wide limits in `$STATE/limits.json`: `backends` caps how many Agent reviews of a
//! backend are in flight at once across every `verify` that shares the state directory, and
//! `agentSessions` sets whether Agent conversations are saved and how large their store grows.
//!
//! ```json
//! {"backends": {"codex": 4, "openai": 16},
//!  "agentSessions": {"enabled": true, "maxBytes": 1073741824, "targetBytes": 805306368}}
//! ```

use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::config::Backend;
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

/// Backend names to their machine-wide slot counts; a backend without an entry is unlimited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Limits {
    backends: BTreeMap<Backend, u32>,
    agent_sessions: AgentSessions,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declared {
    #[serde(default, deserialize_with = "backend_limits")]
    backends: BTreeMap<Backend, u32>,
    #[serde(default, rename = "agentSessions")]
    agent_sessions: Option<DeclaredSessions>,
}

fn backend_limits<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<Backend, u32>, D::Error> {
    // Parse each external name once, retain its Backend, and preserve existing diagnostics
    // and lexicographic validation order at this declaration boundary.
    let names = BTreeMap::<String, u32>::deserialize(deserializer)?;
    names
        .into_iter()
        .map(|(name, limit)| {
            let backend =
                serde_json::from_value::<Backend>(serde_json::Value::String(name.clone()))
                    .map_err(|error| serde::de::Error::custom(format!("backends: {error}")))?;
            if !(1..=MAX_SLOTS).contains(&limit) {
                return Err(serde::de::Error::custom(format!(
                    "backends.{name} must be between 1 and {MAX_SLOTS}."
                )));
            }
            Ok((backend, limit))
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeclaredSessions {
    enabled: Option<bool>,
    max_bytes: Option<u64>,
    target_bytes: Option<u64>,
}

impl DeclaredSessions {
    /// The declared bounds over the defaults. Without `targetBytes`, a `maxBytes` at or below
    /// the 768 MiB default collects down to three quarters of it.
    fn resolve(self) -> Result<AgentSessions, String> {
        let max_bytes = self.max_bytes.unwrap_or(SESSIONS_MAX_BYTES);
        if max_bytes == 0 {
            return Err("agentSessions.maxBytes must be at least 1.".into());
        }
        let target_bytes = match self.target_bytes {
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
            enabled: self.enabled.unwrap_or(true),
            max_bytes,
            target_bytes,
        })
    }
}

impl Limits {
    /// Read `$STATE/limits.json`. No file means no limits; a file that is not a regular file,
    /// is not valid, names an unknown backend or a limit outside 1–100000, or declares
    /// `agentSessions` bounds out of order is an error.
    pub fn read(state: &Path) -> Result<Self, String> {
        let path = state.join(FILE);
        let invalid = |message: String| format!("{}: {message}", crate::platform::path_text(&path));
        let too_large =
            || invalid("limits.json must be a regular file no larger than 64 KiB.".into());
        let file = match crate::platform::open_regular(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
                return Err(too_large());
            }
            Err(error) => return Err(invalid(error.to_string())),
        };
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::Read::take(&file, MAX_BYTES + 1), &mut text)
            .map_err(|e| invalid(e.to_string()))?;
        if text.len() as u64 > MAX_BYTES {
            return Err(too_large());
        }
        let declared: Declared = serde_json::from_str(&text).map_err(|error| {
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
            invalid(message.to_owned())
        })?;
        let agent_sessions = declared
            .agent_sessions
            .map(DeclaredSessions::resolve)
            .transpose()
            .map_err(invalid)?
            .unwrap_or_default();
        Ok(Self {
            backends: declared.backends,
            agent_sessions,
        })
    }

    /// The slot count of a parsed backend; `None` when it is unlimited.
    pub fn limit(&self, backend: Backend) -> Option<u32> {
        self.backends.get(&backend).copied()
    }

    pub fn backends(&self) -> &BTreeMap<Backend, u32> {
        &self.backends
    }

    pub fn agent_sessions(&self) -> AgentSessions {
        self.agent_sessions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read(text: &str) -> Result<Limits, String> {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(FILE), text).unwrap();
        Limits::read(directory.path())
    }

    #[test]
    fn agent_sessions_default_and_validate() {
        assert_eq!(
            read("{}").unwrap().agent_sessions(),
            AgentSessions::default()
        );
        assert_eq!(
            read(r#"{"agentSessions":{"enabled":false}}"#)
                .unwrap()
                .agent_sessions(),
            AgentSessions {
                enabled: false,
                ..AgentSessions::default()
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
}
