//! Machine-wide Agent backend capacity: `$STATE/limits.json` caps how many Agent reviews of a
//! backend are in flight at once across every `verify` that shares the state directory.
//!
//! ```json
//! {"backends": {"codex": 4, "openai": 16}}
//! ```

use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;

use crate::config::Backend;

pub const FILE: &str = "limits.json";
const MAX_BYTES: u64 = 64 * 1024;
const MAX_SLOTS: u32 = 100_000;

/// Backend names to their machine-wide slot counts; a backend without an entry is unlimited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Limits {
    backends: BTreeMap<String, u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declared {
    #[serde(default)]
    backends: BTreeMap<String, u32>,
}

impl Limits {
    /// Read `$STATE/limits.json`. No file means no limits; a file that is not a regular file,
    /// is not valid, names an unknown backend or a limit outside 1–100000 is an error.
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
        Ok(Self {
            backends: declared.backends,
        })
    }

    /// The slot count of a backend, by name; `None` when it is unlimited.
    pub fn limit(&self, backend: &str) -> Option<u32> {
        self.backends.get(backend).copied()
    }

    pub fn backends(&self) -> &BTreeMap<String, u32> {
        &self.backends
    }
}
