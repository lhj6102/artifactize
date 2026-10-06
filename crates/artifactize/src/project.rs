//! Project selection, verification orchestration, and status.

pub mod selection;

mod status;
pub use status::{ArtifactState, Counts, EvalState, StatusView, status};

mod verify;
pub use verify::{DEFAULT_HUMAN_WAIT, VerifyOptions, verify};

/// The fingerprint bound `status` and `verify` use: `--fingerprint-jobs`, else the CPUs.
fn fingerprint_parallelism(options: &VerifyOptions) -> Result<crate::cache::Parallelism, String> {
    match options.fingerprint_jobs {
        Some(0) => Err("fingerprint jobs must be at least 1.".into()),
        jobs => Ok(crate::cache::Parallelism::new(
            jobs.unwrap_or_else(crate::cache::Parallelism::available),
        )),
    }
}
