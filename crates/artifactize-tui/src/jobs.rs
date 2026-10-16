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
        let snapshot = std::panic::AssertUnwindSafe(async {
            match self.snapshot {
                Some(job) => crate::task::joined(job.await)
                    .map(drop)
                    .map_err(|error| error.to_string()),
                None => Ok(()),
            }
        })
        .catch_unwind()
        .await;
        let session = std::panic::AssertUnwindSafe(async {
            match self.session {
                Some(job) => crate::task::joined(job.await)
                    .map(drop)
                    .map_err(|error| format!("Session reader job failed: {error}")),
                None => Ok(()),
            }
        })
        .catch_unwind()
        .await;
        let closed = std::panic::AssertUnwindSafe(changes.close())
            .catch_unwind()
            .await;
        // Every owner is released before resuming any panic. The screen's original bug
        // takes precedence over cleanup bugs, then teardown order decides between those.
        match (result, action, snapshot, session, closed) {
            (Err(panic), _, _, _, _)
            | (_, Err(panic), _, _, _)
            | (_, _, Err(panic), _, _)
            | (_, _, _, Err(panic), _)
            | (_, _, _, _, Err(panic)) => std::panic::resume_unwind(panic),
            (Ok(result), Ok(()), Ok(snapshot), Ok(session), Ok(closed)) => {
                result.and(snapshot).and(session).and(closed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum Panic {
        Screen,
        Action,
        Snapshot,
        Session,
    }

    #[tokio::test]
    async fn shutdown_joins_every_owner_before_resuming_the_first_panic() {
        for panic in [
            Panic::Screen,
            Panic::Action,
            Panic::Snapshot,
            Panic::Session,
        ] {
            let root = crate::test_os::tempdir();
            let repo = root.path().join("repo");
            let state = root.path().join("state");
            std::fs::create_dir(&repo).unwrap();
            drop(
                crate::store::OwnedReceipts::open(&state, &repo)
                    .await
                    .unwrap(),
            );
            let database = state.join(crate::store::DATABASE);
            let changes = Subscription::new(&state).await;
            let snapshot = if matches!(panic, Panic::Session) {
                None
            } else {
                let held = std::fs::File::open(&database).unwrap();
                Some(tokio::spawn(async move {
                    let _held = held;
                    panic!("snapshot fixture panic");
                }))
            };
            let held = std::fs::File::open(&database).unwrap();
            let session = tokio::spawn(async move {
                let _held = held;
                panic!("session fixture panic");
            });
            let jobs = Jobs {
                action: Some(Box::pin(async move {
                    assert!(!matches!(panic, Panic::Action), "action fixture panic");
                    Outcome::Released {
                        ids: Vec::new(),
                        quit: true,
                        failed: Vec::new(),
                    }
                })),
                snapshot,
                session: Some(session),
            };
            let result: std::thread::Result<Result<(), String>> = match panic {
                Panic::Screen => Err(Box::new("screen fixture panic")),
                _ => Ok(Ok(())),
            };
            let failed = std::panic::AssertUnwindSafe(jobs.finish(result, changes))
                .catch_unwind()
                .await
                .unwrap_err();
            let expected = match panic {
                Panic::Screen => "screen fixture panic",
                Panic::Action => "action fixture panic",
                Panic::Snapshot => "snapshot fixture panic",
                Panic::Session => "session fixture panic",
            };
            assert_eq!(failed.downcast_ref::<&str>(), Some(&expected));
            assert!(crate::test_os::database_released(&database).unwrap());
            root.close().unwrap();
        }
    }
}
