mod cli;

fn main() -> std::process::ExitCode {
    cli::run(cli::Screens {
        monitor: |state, repo, cancellation| {
            Box::pin(artifactize_tui::monitor::run(state, repo, cancellation))
        },
        review: |state, repo, reviewer, request, cancellation| {
            Box::pin(artifactize_tui::review::run(
                state,
                repo,
                reviewer,
                request,
                cancellation,
            ))
        },
    })
}

/// Portable OS fixtures shared with the library's tests.
#[cfg(test)]
#[path = "../../artifactize-core/tests/support/os.rs"]
mod test_os;

#[cfg(test)]
use artifactize_core::store;

/// Saved requests shared with the library's tests.
#[cfg(test)]
#[path = "../../artifactize-core/tests/support/request.rs"]
mod test_request;
