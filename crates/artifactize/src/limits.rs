//! Machine-wide limits in `$STATE/limits.json`: `backends` caps how many Agent reviews of a
//! backend are in flight at once across every `verify` that shares the state directory, and
//! `agentSessions` sets whether Agent conversations are saved and how large their store grows.
//!
//! ```json
//! {"backends": {"codex": 4, "openai": 16},
//!  "agentSessions": {"enabled": true, "maxBytes": 1073741824, "targetBytes": 805306368}}
//! ```

use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::config::Backend;

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

/// Backend names to their machine-wide slot counts; a backend without an entry is unlimited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Limits {
    backends: BTreeMap<String, u32>,
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
    #[serde(default)]
    backends: BTreeMap<String, u32>,
    #[serde(default, rename = "agentSessions")]
    agent_sessions: Option<DeclaredSessions>,
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
            None => max_bytes / 4 * 3,
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
        let invalid = |message: String| format!("{}: {message}", path.display());
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(invalid(error.to_string())),
        };
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            return Err(invalid(
                "limits.json must be a regular file no larger than 64 KiB.".into(),
            ));
        }
        let text = fs::read_to_string(&path).map_err(|e| invalid(e.to_string()))?;
        let declared: Declared = serde_json::from_str(&text).map_err(|e| invalid(e.to_string()))?;
        for (name, limit) in &declared.backends {
            // Any backend artifactize knows, by the name a profile declares it with.
            serde_json::from_value::<Backend>(serde_json::Value::String(name.clone()))
                .map_err(|e| invalid(format!("backends: {e}")))?;
            if !(1..=MAX_SLOTS).contains(limit) {
                return Err(invalid(format!(
                    "backends.{name} must be between 1 and {MAX_SLOTS}."
                )));
            }
        }
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

    /// The slot count of a backend, by name; `None` when it is unlimited.
    pub fn limit(&self, backend: &str) -> Option<u32> {
        self.backends.get(backend).copied()
    }

    pub fn backends(&self) -> &BTreeMap<String, u32> {
        &self.backends
    }

    pub fn agent_sessions(&self) -> AgentSessions {
        self.agent_sessions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
