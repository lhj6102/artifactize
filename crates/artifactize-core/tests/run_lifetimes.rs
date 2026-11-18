//! Terminal receipts precede worker teardown: fixture cleanup owns both process lifetimes.
use artifactize_core::{changes::Subscription, store};
use futures_util::FutureExt;
use serde_json::json;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio_util::sync::CancellationToken;

mod support;

/// This fixture state's IPC namespace, which lives outside the temporary state.
struct Namespace(std::path::PathBuf);

impl Drop for Namespace {
    fn drop(&mut self) {
        let removed = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            removed.expect("remove the fixture's IPC namespace");
        }
    }
}

/// Join the subscription's shutdown, then remove its namespace, on success and unwind alike.
async fn subscribed<T>(
    state: &std::path::Path,
    observe: impl AsyncFnOnce(&mut Subscription) -> T,
) -> T {
    let _namespace = Namespace(
        artifactize_core::changes::namespace(state).expect("the fixture state's IPC namespace"),
    );
    support::runs::with_subscription(state, observe).await
}

#[tokio::test]
async fn terminal_cleanup_waits_for_a_late_supervisor_before_releasing_its_state() {
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir(&repo).unwrap();
    store::Receipts::open(&state, &repo)
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    let database = state.join(store::DATABASE);
    let start = || {
        let mut command = std::process::Command::new(support::os::python_program());
        command.args(["-I", "-c", "import sqlite3,sys\ndb=sqlite3.connect(sys.argv[1])\nprint('holding',flush=True)\nsys.stdin.buffer.read()\ndb.close()"])
            .arg(&database).current_dir(&state);
        support::os::spawn_piped_test(command)
    };
    let mut executor = start();
    let mut supervisor = start();
    let release_executor = executor.take_stdin();
    let release_supervisor = supervisor.take_stdin();
    for child in [&mut executor, &mut supervisor] {
        let mut ready = tokio::io::BufReader::new(child.take_stdout()).lines();
        assert_eq!(ready.next_line().await.unwrap().as_deref(), Some("holding"));
    }
    let view = store::RunView {
        run: serde_json::from_value(json!({
            "id":"run-terminal", "repoPath":repo, "stateDir":state, "status":"GREEN",
            "createdAt":"2026-01-01T00:00:00Z", "completedAt":"2026-01-01T00:00:01Z",
            "selection":{"kind":"all"}, "validation":null,
            "control":{"worker":{"state":"admitted","owner":{"pid":executor.id(),"start_time":support::os::start_time(executor.id())}}, "supervisor":{"pid":supervisor.id(),"start_time":support::os::start_time(supervisor.id())}, "activity":"executing"}
        })).unwrap(),
        requests: Vec::new(),
        unreadable: Vec::new(),
    };
    subscribed(&state, async |changes| {
        let cancellation = CancellationToken::new();
        {
            let finished = support::runs::terminal_worker_exited(&view, changes, &cancellation);
            tokio::pin!(finished);
            assert!(futures_util::poll!(&mut finished).is_pending());
        }
        drop(release_executor);
        assert!(executor.wait().await.unwrap().success());
        {
            // Start a fresh observation after the executor exits, so a pending previous wait
            // cannot mask a barrier that forgot the separately held supervisor.
            let (tick, mut ticks) = tokio::sync::mpsc::unbounded_channel();
            let reconciled = std::cell::Cell::new(false);
            let finished =
                support::runs::terminal_worker_exited_on(&view, &cancellation, async || {
                    ticks
                        .recv()
                        .await
                        .expect("the test sends each reconciliation");
                    reconciled.set(true);
                });
            tokio::pin!(finished);
            assert!(futures_util::poll!(&mut finished).is_pending());
            // Reconcile while the supervisor still owns its SQLite connection and cwd.
            // A terminal receipt and the executor's exit do not authorize root deletion.
            tick.send(()).unwrap();
            assert!(futures_util::poll!(&mut finished).is_pending());
            assert!(reconciled.get(), "the barrier consumed the reconciliation");
            drop(release_supervisor);
            assert!(supervisor.wait().await.unwrap().success());
            tick.send(()).unwrap();
            finished.await.unwrap();
        }
    })
    .await;
    assert!(support::os::database_released(&database).unwrap());
    root.close().unwrap();
}

#[tokio::test]
async fn subscription_closes_sqlite_before_propagating_assertion_and_watchdog_panics() {
    for watchdog in [false, true] {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        let state = root.path().join("state");
        std::fs::create_dir(&repo).unwrap();
        store::Receipts::open(&state, &repo)
            .await
            .unwrap()
            .close()
            .await
            .unwrap();
        let database = state.join(store::DATABASE);
        let failed = std::panic::AssertUnwindSafe(subscribed(&state, async |_changes| {
            assert!(!support::os::database_released(&database).unwrap());
            if watchdog {
                tokio::time::pause();
                support::runs::worker_exit_watchdog(
                    Duration::from_millis(1),
                    std::future::pending::<()>(),
                )
                .await;
            }
            panic!("fixture observation assertion");
        }))
        .catch_unwind()
        .await;
        let panic = failed.expect_err("the scripted observation must panic");
        let message = panic
            .downcast_ref::<&'static str>()
            .copied()
            .or_else(|| panic.downcast_ref::<String>().map(String::as_str));
        if watchdog {
            tokio::time::resume();
            assert!(
                message.is_some_and(|text| text
                    .starts_with("terminal fixture worker exits before the byte baseline:")),
                "unexpected panic: {message:?}"
            );
        } else {
            assert_eq!(message, Some("fixture observation assertion"));
        }
        assert!(support::os::database_released(&database).unwrap());
        root.close().unwrap();
    }
}
