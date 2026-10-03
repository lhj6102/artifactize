//! The state database and storage maintenance.

use std::{
    env,
    path::{Path, PathBuf},
};

use crate::workspace::canonical_target;

pub(crate) mod cache_entries;
mod executions;
mod human;
pub use human::HumanClaim;
mod receipts;
mod runs;
pub use runs::{RunSummary, read_runs};
mod tool_calls;
pub use executions::{Claim, Execution, Provenance, read_identity_executions};
pub use receipts::{
    DATABASE, LastRequest, Receipts, Request, Run, RunView, read_latest_requests, read_run,
};

/// Resolve the single state directory without creating it.
pub fn state_dir(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let path = match explicit {
        Some(path) => path.to_path_buf(),
        None => state_home().map_err(|e| e.to_string())?,
    };
    canonical_target(&path).map_err(|e| e.to_string())
}

/// SQLite `user_version` for artifactize state databases.
pub const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
#[error("cannot resolve state home: set ARTIFACTIZE_STATE_HOME, XDG_STATE_HOME, or HOME")]
pub struct StateHomeError;

/// Resolve the default state home without creating directories; empty variables are ignored.
pub fn state_home() -> Result<PathBuf, StateHomeError> {
    resolve_state_home(
        env::var_os("ARTIFACTIZE_STATE_HOME").map(PathBuf::from),
        env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

fn resolve_state_home(
    artifactize: Option<PathBuf>,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, StateHomeError> {
    if let Some(path) = artifactize.filter(|path| !path.as_os_str().is_empty()) {
        return Ok(path);
    }
    if let Some(path) = xdg.filter(|path| !path.as_os_str().is_empty()) {
        return Ok(path.join("artifactize"));
    }
    home.filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.join(".local/state/artifactize"))
        .ok_or(StateHomeError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_home_precedence() {
        let artifactize = PathBuf::from("/custom/state");
        let xdg = PathBuf::from("/xdg/state");
        let home = PathBuf::from("/home/reviewer");

        assert_eq!(
            resolve_state_home(
                Some(artifactize.clone()),
                Some(xdg.clone()),
                Some(home.clone())
            )
            .unwrap(),
            artifactize
        );
        assert_eq!(
            resolve_state_home(None, Some(xdg.clone()), Some(home.clone())).unwrap(),
            xdg.join("artifactize")
        );
        assert_eq!(
            resolve_state_home(None, None, Some(home.clone())).unwrap(),
            home.join(".local/state/artifactize")
        );
    }

    #[test]
    fn empty_state_home_variables_are_ignored() {
        let empty = Some(PathBuf::new());
        let home = PathBuf::from("/home/reviewer");

        assert_eq!(
            resolve_state_home(empty.clone(), empty.clone(), Some(home.clone())).unwrap(),
            home.join(".local/state/artifactize")
        );
        assert!(resolve_state_home(empty.clone(), empty.clone(), empty).is_err());
        assert!(resolve_state_home(None, None, None).is_err());
    }
}
