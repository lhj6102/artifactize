//! Durable admission records and invocation leases for a review-owned session.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::{runtime::Handle, sync::Notify};
use tokio_util::sync::CancellationToken;

use super::journal::Record;
use crate::process;

/// Cleanup must survive the caller's executor shutting down, not just a dropped
/// review future. A single lazy cleanup thread owns all session process I/O and
/// timers; tokens and join handles work across executors. Process exit still uses
/// durable group records (Unix) or kill-on-close Jobs (Windows).
pub(crate) fn executor() -> io::Result<&'static Handle> {
    static EXECUTOR: OnceLock<Result<Handle, String>> = OnceLock::new();
    EXECUTOR
        .get_or_init(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            let handle = runtime.handle().clone();
            std::thread::Builder::new()
                .name("tool-session-cleanup".into())
                .spawn(move || runtime.block_on(std::future::pending::<()>()))
                .map_err(|error| error.to_string())?;
            Ok(handle)
        })
        .as_ref()
        .map_err(|error| io::Error::other(error.clone()))
}

pub(super) struct Ownership {
    directory: PathBuf,
    _lease: std::fs::File,
    cleanup: Mutex<Cleanup>,
    pub ended: CancellationToken,
    active: AtomicUsize,
    finished: Notify,
}

impl Ownership {
    pub fn new(directory: &Path, lease: std::fs::File) -> Arc<Self> {
        Arc::new(Self {
            directory: directory.into(),
            _lease: lease,
            cleanup: Mutex::new(Cleanup::default()),
            ended: CancellationToken::new(),
            active: AtomicUsize::new(0),
            finished: Notify::new(),
        })
    }

    pub fn call(self: &Arc<Self>, environment: &BTreeMap<OsString, OsString>) -> Option<Call> {
        self.active.fetch_add(1, Ordering::AcqRel);
        let call = Call {
            owner: self.clone(),
            environment: environment.clone(),
            record: Record::new(&self.directory),
        };
        if self.ended.is_cancelled() {
            None
        } else {
            Some(call)
        }
    }

    fn record_cleanup(&self, cleanup: Cleanup) {
        self.cleanup.lock().expect("cleanup lock").merge(cleanup);
    }

    pub async fn finish_calls(&self) -> Cleanup {
        self.ended.cancel();
        loop {
            let notified = self.finished.notified();
            // Register the waiter before checking the count to avoid lost wakeups.
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.active.load(Ordering::Acquire) == 0 {
                return self.cleanup.lock().expect("cleanup lock").clone();
            }
            notified.await;
        }
    }
}

/// The lease lasts until the invocation's independent supervisor has reaped its
/// group. Session cleanup cannot remove shared files while a bound call uses them.
pub(crate) struct Call {
    owner: Arc<Ownership>,
    pub environment: BTreeMap<OsString, OsString>,
    record: Arc<Record>,
}

impl Call {
    pub async fn run(
        self,
        command: process::Command,
        input: Vec<u8>,
        limit: usize,
        cancellation: CancellationToken,
    ) -> Result<process::Output, process::Error> {
        let cancellation = cancellation.child_token();
        let record = self.record.clone();
        let running = process::run_with_input_limit(
            command,
            Some(input),
            limit,
            cancellation.clone(),
            move |child| async move { record.register(child).await },
        );
        tokio::pin!(running);
        let result = tokio::select! {
            biased;
            _ = self.owner.ended.cancelled() => { cancellation.cancel(); running.await },
            result = &mut running => result,
        };
        self.record.after_run(&result);
        result
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        // Unexpected task failure leaves the identity intact and blocks deletion.
        self.owner.record_cleanup(self.record.finish());
        self.owner.active.fetch_sub(1, Ordering::AcqRel);
        self.owner.finished.notify_waiters();
    }
}

/// Cleanup certainty is separate from its error. A failed record retirement may
/// follow confirmed termination; an unconfirmed group must retain the directory.
#[derive(Clone)]
pub(super) enum Cleanup {
    Complete(Result<(), String>),
    Incomplete(String),
}

impl Default for Cleanup {
    fn default() -> Self {
        Self::Complete(Ok(()))
    }
}

impl Cleanup {
    pub fn error(error: impl std::fmt::Display) -> Self {
        Self::Complete(Err(error.to_string()))
    }

    pub fn merge(&mut self, next: Self) {
        let incomplete = matches!(self, Self::Incomplete(_)) || matches!(next, Self::Incomplete(_));
        let error = match (self.clone().result(), next.result()) {
            (Ok(()), result) => result,
            (Err(first), Err(next)) if first != next => Err(format!("{first}; {next}")),
            (first, _) => first,
        };
        *self = if incomplete {
            Self::Incomplete(error.expect_err("incomplete cleanup has an error"))
        } else {
            Self::Complete(error)
        };
    }

    pub fn result(self) -> Result<(), String> {
        match self {
            Self::Complete(result) => result,
            Self::Incomplete(error) => Err(error),
        }
    }

    pub fn confirmed(&self) -> bool {
        matches!(self, Self::Complete(_))
    }
}
