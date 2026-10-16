//! Terminal receipts precede worker teardown: fixture cleanup owns both process lifetimes.
use artifactize_core::{changes::Subscription, store};
use serde_json::json;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio_util::sync::CancellationToken;

mod support;

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
    let mut changes = Subscription::new(&state).await;
    let cancellation = CancellationToken::new();
    {
        let finished = support::runs::terminal_worker_exited(&view, &mut changes, &cancellation);
        tokio::pin!(finished);
        assert!(futures_util::poll!(&mut finished).is_pending());
    }
    drop(release_executor);
    assert!(executor.wait().await.unwrap().success());
    {
        // Start a fresh observation after the executor exits, so a pending previous wait
        // cannot mask a barrier that forgot the separately held supervisor.
        let finished = support::runs::terminal_worker_exited(&view, &mut changes, &cancellation);
        tokio::pin!(finished);
        // The executor no longer exists. Its supervisor still owns an idle SQLite handle
        // and its cwd, so a saved terminal receipt cannot yet authorize root deletion.
        assert!(futures_util::poll!(&mut finished).is_pending());
        drop(release_supervisor);
        assert!(supervisor.wait().await.unwrap().success());
        tokio::time::timeout(
            support::os::patience(Duration::from_secs(20)),
            &mut finished,
        )
        .await
        .expect("observe supervisor exit")
        .unwrap();
    }
    changes.close().await.unwrap();
    assert!(support::os::database_released(&database).unwrap());
    root.close().unwrap();
}
