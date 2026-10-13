//! The terminal screens of artifactize: the Monitor and the Human review. Every fact and
//! operation they show comes from `artifactize-core`; this crate draws and reads keys.

pub mod monitor;
pub mod review;

/// Two blank columns separate the header breadcrumb from its right-aligned status.
pub(crate) const HEADER_GAP: usize = 2;

// The core modules the screens use, under the paths their code has always named.
pub(crate) use artifactize_core::{
    agent, changes, config, human, platform, runtime, store, task, tools, types, workspace,
};

#[cfg(test)]
pub(crate) use artifactize_core::{project, repository};

/// Operating-system fixtures shared with the core's tests.
#[cfg(test)]
#[path = "../../artifactize-core/tests/support/os.rs"]
mod test_os;

/// The screens `artifactize monitor` and `artifactize review` open, for the binary to hand to
/// [`artifactize_core::cli::run`].
pub fn screens() -> artifactize_core::cli::Screens {
    artifactize_core::cli::Screens {
        monitor: |state, repo, cancellation| Box::pin(monitor::run(state, repo, cancellation)),
        review: |state, repo, reviewer, request, cancellation| {
            Box::pin(review::run(state, repo, reviewer, request, cancellation))
        },
    }
}

/// A saved request, shared with the core's unit tests.
#[cfg(test)]
#[path = "../../artifactize-core/tests/support/request.rs"]
mod test_request;

/// Saved Agent conversation files, shared with the core's unit tests.
#[cfg(test)]
#[path = "../../artifactize-core/tests/support/session_files.rs"]
mod test_session_files;

/// Declaration files in the form the tests write them.
#[cfg(test)]
#[expect(
    dead_code,
    reason = "shared with the core's tests, which use more of it"
)]
#[path = "../../artifactize-core/tests/support/declaration.rs"]
mod test_declaration;

#[cfg(test)]
#[path = "../../artifactize-core/tests/support/runs.rs"]
#[expect(dead_code, reason = "unit fixtures only record Human requests")]
mod test_runs;
