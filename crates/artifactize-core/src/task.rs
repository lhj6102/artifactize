//! Joining spawned tasks. A task that panicked has a bug, so its panic continues in the task
//! that joins it instead of becoming an ordinary failure; a cancelled task stays an error.

/// `result` of joining a task, with a panic resumed here.
pub fn joined<T>(result: Result<T, tokio::task::JoinError>) -> Result<T, tokio::task::JoinError> {
    match result {
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        other => other,
    }
}

pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    joined(tokio::task::spawn_blocking(work).await).map_err(|error| error.to_string())
}
