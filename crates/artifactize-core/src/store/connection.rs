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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cancelling_close_and_shutting_down_the_runtime_releases_sqlite_without_panicking() {
        let output = tokio::time::timeout(
            crate::test_os::patience(Duration::from_secs(20)),
            crate::test_os::self_test("store::connection::tests::close_cancellation_fixture")
                .env("ARTIFACTIZE_SQLITE_CLOSE_FIXTURE", "1")
                .output(),
        )
        .await
        .expect("the SQLite shutdown probe exits")
        .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            output.stderr.is_empty(),
            "a cancelled SQLite close must not panic on its worker: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn close_cancellation_fixture() {
        if crate::platform::environment::var("ARTIFACTIZE_SQLITE_CLOSE_FIXTURE").is_none() {
            return;
        }
        let root = crate::test_os::tempdir();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let connection = runtime
            .block_on(Connection::open(root.path().join("closing.sqlite")))
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
        // before checking its stderr in the parent and releasing the temporary files.
        assert!(matches!(
            runtime.block_on(connection.call(|_| Ok::<_, rusqlite::Error>(()))),
            Err(tokio_rusqlite::Error::ConnectionClosed)
        ));
        drop(connection);
        root.close().unwrap();
    }
}
