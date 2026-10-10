//! The state database and storage maintenance.

use std::path::{Path, PathBuf};

use crate::{platform, workspace::canonical_target};

pub mod definitions;
mod executions;
pub(crate) mod history;
mod human;
pub use human::HumanClaim;
mod catalog;
pub mod prune;
pub(crate) mod receipts;
mod result;
mod runs;
mod snapshots;
mod unreadable;
mod validation;
pub use unreadable::{Unreadable, read_unreadable};
mod lifecycle;
pub use lifecycle::{ExecutionState, PendingRequest, RequestState, RunState};
pub use result::{ExecutionResult, Field as ResultField, RuntimeResult};
pub use snapshots::{ArtifactValidation, Blocker, ChildIdentity, HumanDefinition, Validation};
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
    DATABASE, EARLIER_STATE, LastRequest, QueueReason, Receipts, Request, Run, RunView,
    StoppedBackend, read_latest_requests, read_run, read_state_id, schema_error, state_schema,
};
pub use requests::{
    RequestView, read_original, read_request, read_requests, read_session_request, read_waiting,
    read_waiting_originals,
};

/// Let concurrent readers/writers finish short SQLite transactions without an
/// immediate busy error, while limiting how long one database operation can block.
pub(crate) const SQLITE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How many times [`retry_transient_access`] retries past a transient Windows file-access
/// race, and how long it waits between attempts.
const TRANSIENT_ACCESS_RETRIES: u32 = 5;
const TRANSIENT_ACCESS_BACKOFF: std::time::Duration = std::time::Duration::from_millis(20);

/// Retry `attempt` past [`platform::transient_file_access`] (Windows transiently denying a
/// metadata or open probe while another handle finishes closing a just-deleted WAL sidecar)
/// before giving up with its last error. Every read-only state open reaches this through
/// [`receipts::check_files`]/[`check_probe_files`], which every reader calls first, so this is
/// the one place the retry needs to live.
pub(crate) fn retry_transient_access<T>(
    mut attempt: impl FnMut() -> std::io::Result<T>,
) -> std::io::Result<T> {
    for remaining in (0..TRANSIENT_ACCESS_RETRIES).rev() {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error) if remaining > 0 && platform::transient_file_access(&error) => {
                std::thread::sleep(TRANSIENT_ACCESS_BACKOFF);
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the loop above always returns by its last attempt")
}

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
#[error("cannot resolve state home: set {}", state_home_variables())]
pub struct StateHomeError;

/// The variables `state_home` reads, in order, for the error that names them.
fn state_home_variables() -> String {
    let names: Vec<&str> = std::iter::once("ARTIFACTIZE_STATE_HOME")
        .chain(platform::STATE_VARIABLES.iter().copied())
        .chain([platform::HOME_VARIABLE])
        .collect();
    let (last, rest) = names.split_last().expect("constant names");
    format!("{}, or {last}", rest.join(", "))
}

/// Resolve the default state home without creating directories; empty variables are ignored.
/// The platform's state variables come after `ARTIFACTIZE_STATE_HOME` and before the home.
pub fn state_home() -> Result<PathBuf, StateHomeError> {
    platform::state_directory().ok_or(StateHomeError)
}

#[cfg(test)]
#[path = "store/file_tests.rs"]
mod file_tests;
