//! Public verification/status options reject invalid bounds before touching project state.
use artifactize::project::{self, VerifyOptions, selection::Selection};
use tokio_util::sync::CancellationToken;

mod support;

#[tokio::test]
async fn fingerprint_bounds_return_input_errors_without_creating_state() {
    let root = support::os::tempdir();
    let repo = root.path().join("missing-repo");
    let state = root.path().join("state");
    let maximum = tokio::sync::Semaphore::MAX_PERMITS;
    for (jobs, expected) in [
        (0, "fingerprint jobs must be at least 1.".to_owned()),
        (
            maximum + 1,
            format!("fingerprint jobs must be at most {maximum}."),
        ),
        (
            usize::MAX,
            format!("fingerprint jobs must be at most {maximum}."),
        ),
    ] {
        let options = VerifyOptions {
            fingerprint_jobs: Some(jobs),
            ..VerifyOptions::default()
        };
        assert_eq!(
            project::verify(
                &repo,
                Some(&state),
                &Selection::All,
                &options,
                CancellationToken::new(),
            )
            .await
            .unwrap_err(),
            expected
        );
        assert_eq!(
            project::status(
                &repo,
                Some(&state),
                &Selection::All,
                &options,
                CancellationToken::new(),
            )
            .await
            .unwrap_err(),
            expected
        );
        assert!(!state.exists());
    }
}

#[test]
fn fingerprint_parallelism_accepts_the_permit_ceiling() {
    let maximum = tokio::sync::Semaphore::MAX_PERMITS;
    assert_eq!(
        artifactize::cache::Parallelism::new(maximum).limit(),
        maximum
    );
}
