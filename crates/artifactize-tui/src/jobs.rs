//! Terminal-owned work must finish before the screen releases its state or session files.
use crate::{changes::Subscription, review::Outcome};
use futures_util::FutureExt;
use std::{future::Future, pin::Pin};
use tokio::task::JoinHandle;

#[derive(Default)]
pub(crate) struct Jobs {
    pub action: Option<Pin<Box<dyn Future<Output = Outcome>>>>,
    pub snapshot: Option<JoinHandle<artifactize_core::monitor::loader::Completed>>,
    pub session: Option<
        JoinHandle<(
            crate::monitor::session::Job,
            crate::agent::session::live::Window,
        )>,
    >,
}

impl Jobs {
    /// The screen cancels its active Human action first. Even errors and unwinding join
    /// accepted blocking reads and close the subscription; dropping a JoinHandle detaches it.
    pub async fn finish(
        self,
        result: std::thread::Result<Result<(), String>>,
        changes: Subscription,
    ) -> Result<(), String> {
        let action = match self.action {
            Some(action) => std::panic::AssertUnwindSafe(action)
                .catch_unwind()
                .await
                .map(drop),
            None => Ok(()),
        };
        let snapshot = match self.snapshot {
            Some(job) => crate::task::joined(job.await)
                .map(drop)
                .map_err(|error| error.to_string()),
            None => Ok(()),
        };
        let session = match self.session {
            Some(job) => crate::task::joined(job.await)
                .map(drop)
                .map_err(|error| format!("Session reader job failed: {error}")),
            None => Ok(()),
        };
        let closed = changes.close().await;
        match (result, action) {
            (Err(panic), _) | (_, Err(panic)) => std::panic::resume_unwind(panic),
            (Ok(result), Ok(())) => result.and(snapshot).and(session).and(closed),
        }
    }
}
