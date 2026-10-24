//! Project selection, verification orchestration, and status.

pub mod selection;

mod status;
pub use status::{
    ArtifactCondition, ArtifactState, Counts, EvalCondition, EvalState, StatusView, VerifyAction,
    status,
};
pub(crate) use status::{Progress, scan};

pub(crate) mod verify;
pub use verify::{VerifyOptions, verify};

/// The fingerprint bound `status` and `verify` use: `--fingerprint-jobs`, else the CPUs.
fn fingerprint_parallelism(options: &VerifyOptions) -> Result<crate::cache::Parallelism, String> {
    let jobs = options
        .fingerprint_jobs
        .unwrap_or_else(crate::cache::Parallelism::available);
    if jobs == 0 {
        return Err("fingerprint jobs must be at least 1.".into());
    }
    // Tokio's semaphore has a platform-sized permit ceiling; external input must not panic.
    if jobs > tokio::sync::Semaphore::MAX_PERMITS {
        return Err(format!(
            "fingerprint jobs must be at most {}.",
            tokio::sync::Semaphore::MAX_PERMITS
        ));
    }
    Ok(crate::cache::Parallelism::new(jobs))
}
