//! Joining SQLite worker shutdown independently of a cancelled observer.
use tokio_rusqlite::Connection;

pub(crate) async fn close(connection: Connection) -> Result<(), String> {
    // tokio-rusqlite 0.8 panics on its worker if close's response receiver is dropped.
    // A blocking task keeps that receiver alive even when its caller is cancelled;
    // runtime shutdown joins it instead of cancelling it with ordinary async tasks.
    let runtime = tokio::runtime::Handle::current();
    crate::task::joined(
        tokio::task::spawn_blocking(move || {
            runtime
                .block_on(connection.close())
                .map_err(|error| error.to_string())
        })
        .await,
    )
    .map_err(|error| error.to_string())?
}

/// A one-shot operation owns its worker until shutdown, even when the operation failed.
/// Preserve its original error; report a close failure only after a successful operation.
pub(super) async fn finish<T>(
    connection: Connection,
    result: Result<T, String>,
) -> Result<T, String> {
    let closed = close(connection).await;
    result.and_then(|value| closed.map(|()| value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Stdio, time::Duration};
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn cancelling_close_and_shutting_down_the_runtime_releases_sqlite_without_panicking() {
        // The test owns the probe's files and the probe: they go only after it has exited.
        let root = crate::test_os::tempdir();
        let mut probe =
            crate::test_os::self_test("store::connection::tests::close_cancellation_fixture")
                .env("ARTIFACTIZE_SQLITE_CLOSE_FIXTURE", root.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("the SQLite shutdown probe starts");
        let mut stderr = probe.stderr.take().unwrap();
        let ran = tokio::time::timeout(crate::test_os::patience(Duration::from_secs(20)), async {
            let mut text = Vec::new();
            let (status, read) = tokio::join!(probe.wait(), stderr.read_to_end(&mut text));
            read.unwrap();
            (status.unwrap(), text)
        })
        .await;
        let Ok((status, stderr)) = ran else {
            // The watchdog expired: end the probe and reap it before its files go.
            probe.kill().await.unwrap();
            panic!("the SQLite shutdown probe did not exit");
        };
        assert!(
            status.success(),
            "{status:?}: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert!(
            stderr.is_empty(),
            "a cancelled SQLite close must not panic on its worker: {}",
            String::from_utf8_lossy(&stderr)
        );
        root.close().unwrap();
    }

    /// The probe: its files are in the folder the test gives it, which owns them.
    #[test]
    fn close_cancellation_fixture() {
        let Some(root) = crate::platform::environment::var("ARTIFACTIZE_SQLITE_CLOSE_FIXTURE")
        else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let connection = runtime
            .block_on(Connection::open(root.join("closing.sqlite")))
            .unwrap();
        let (release, held) = std::sync::mpsc::channel();
        runtime.block_on(async {
            let (entered, started) = tokio::sync::oneshot::channel();
            let operation = connection.call(move |_| {
                entered.send(()).unwrap();
                // A channel, not a sleep, keeps close pending until its observer is gone.
                let _ = held.recv();
                Ok::<_, rusqlite::Error>(())
            });
            tokio::pin!(operation);
            assert!(futures_util::poll!(&mut operation).is_pending());
            started.await.unwrap();
            let closing = close(connection.clone());
            tokio::pin!(closing);
            assert!(futures_util::poll!(&mut closing).is_pending());
            // Both awaiting futures are cancelled while the SQLite worker is held.
        });
        release.send(()).unwrap();
        drop(runtime);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        // Wait for the actual worker's closed channel, including any worker panic hook,
        // before the parent checks its stderr and releases the temporary files.
        assert!(matches!(
            runtime.block_on(connection.call(|_| Ok::<_, rusqlite::Error>(()))),
            Err(tokio_rusqlite::Error::ConnectionClosed)
        ));
        drop(connection);
    }
}
