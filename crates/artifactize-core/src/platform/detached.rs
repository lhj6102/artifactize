//! Process handles for launches that outlive the requester and are not killed on drop.

use std::io;

use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use super::{ProcessOutcome, os};

pub(crate) struct DetachedChild(Child);

impl DetachedChild {
    pub(crate) fn pid(&self) -> u32 {
        self.0.id().expect("spawned child has not been reaped")
    }

    pub(crate) fn take_output(&mut self) -> (ChildStdout, ChildStderr) {
        (
            self.0.stdout.take().expect("stdout is piped"),
            self.0.stderr.take().expect("stderr is piped"),
        )
    }

    pub(crate) async fn wait(&mut self) -> io::Result<ProcessOutcome> {
        self.0.wait().await.map(ProcessOutcome::from_native)
    }

    pub(crate) async fn kill(&mut self) -> io::Result<()> {
        self.0.kill().await
    }

    /// Leave the child running and reap it in the background, with no waiter at the call site.
    pub(crate) fn reap(mut self) {
        tokio::spawn(async move {
            let _ = self.wait().await;
        });
    }
}

pub(crate) fn spawn_detached(command: Command) -> io::Result<DetachedChild> {
    os::spawn_detached(command).map(DetachedChild)
}

pub(crate) fn spawn_watched(command: Command) -> io::Result<DetachedChild> {
    os::spawn_watched(command).map(DetachedChild)
}

pub(crate) fn spawn_run(command: Command) -> io::Result<DetachedChild> {
    os::spawn_run(command).map(DetachedChild)
}
