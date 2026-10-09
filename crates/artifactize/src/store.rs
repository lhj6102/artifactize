//! The state database and storage maintenance.

use std::{
    env,
    path::{Path, PathBuf},
};

use crate::workspace::canonical_target;

pub mod definitions;
mod executions;
pub(crate) mod history;
mod human;
pub use human::HumanClaim;
mod catalog;
pub mod prune;
pub(crate) mod receipts;
mod runs;
mod validation;
mod wait_timeout;
pub use catalog::{CatalogRun, Signoff, read_catalog};
pub use runs::{RunSummary, read_runs, read_scoped_runs};
mod requests;
pub use executions::{
    Capacity, Claim, Execution, ExecutionOptions, Origin, Producer, Provenance,
    read_keyed_executions, read_latest_cached,
};
pub(crate) use receipts::regular_files as check_probe_files;
pub use receipts::{
    DATABASE, EARLIER_STATE, LastRequest, Receipts, Request, Run, RunView, StoppedBackend,
    read_latest_requests, read_run, read_state_id, schema_error, state_schema,
};
pub use requests::{RequestView, read_request, read_requests, read_session_request, read_waiting};

/// Let concurrent readers/writers finish short SQLite transactions without an
/// immediate busy error, while limiting how long one database operation can block.
pub(crate) const SQLITE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Resolve the single state directory without creating it.
pub fn state_dir(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let path = match explicit {
        Some(path) => path.to_path_buf(),
        None => state_home().map_err(|e| e.to_string())?,
    };
    canonical_target(&path).map_err(|e| e.to_string())
}

/// SQLite `user_version`; dependency requests and derived verdicts require schema 6.
/// Earlier state is not migrated.
pub const STATE_SCHEMA_VERSION: u32 = 6;

#[derive(Debug, thiserror::Error)]
#[cfg_attr(
    not(windows),
    error("cannot resolve state home: set ARTIFACTIZE_STATE_HOME, XDG_STATE_HOME, or HOME")
)]
#[cfg_attr(
    windows,
    error(
        "cannot resolve state home: set ARTIFACTIZE_STATE_HOME, XDG_STATE_HOME, LOCALAPPDATA, or HOME"
    )
)]
pub struct StateHomeError;

/// Resolve the default state home without creating directories; empty variables are ignored.
/// On Windows, `%LOCALAPPDATA%\artifactize` comes after `XDG_STATE_HOME` and before `HOME`.
pub fn state_home() -> Result<PathBuf, StateHomeError> {
    let xdg = env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty());
    #[cfg(windows)]
    let xdg = xdg.or_else(|| env::var_os("LOCALAPPDATA"));
    resolve_state_home(
        env::var_os("ARTIFACTIZE_STATE_HOME").map(PathBuf::from),
        xdg.map(PathBuf::from),
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
