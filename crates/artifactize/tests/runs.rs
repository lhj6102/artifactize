//! A1–A4: one observable seam per Run submission/lifecycle/key/migration decision.
use artifactize_core::{
    project::{VerifyOptions, selection::Selection},
    runs, store,
    types::{FailureCode, RequestStatus, RunId, RunStatus},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = support::os::tempdir();
        let repo = support::os::canonical(root.path()).join("repo");
        fs::create_dir(&repo).unwrap();
        let script = root.path().join("controlled.py");
        support::os::write_python_script(
            &script,
            "import pathlib, time\npathlib.Path('ready').write_text('ready')\nwhile not pathlib.Path('release').exists(): time.sleep(0.01)\n",
        );
        support::declaration::write(repo.join("index.artf"), json!({"name":"app", "fingerprint":false, "evals":[
            {"id":"a-fast","title":"Committed", "profile":{"kind":"runtime","command":support::os::true_program(),"args":[]},"payload":{"instruction":"Check."}},
            {"id":"z-controlled","title":"Controlled", "profile":{"kind":"runtime","command":support::os::python_program(),"args":[script]},"payload":{"instruction":"Check."}}
        ]}).to_string()).unwrap();
        let state = root.path().join("state");
        Self { root, repo, state }
    }
    fn context(&self) -> runs::Context {
        runs::Context::new(&self.repo, Some(&self.state), &support::os::cli()).unwrap()
    }
    fn command(&self) -> Command {
        let mut command = Command::new(support::os::cli());
        support::os::new_group(&mut command)
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .env("ARTIFACTIZE_REMOTE", "off");
        command
    }
    fn submit(&self, key: Option<&str>) -> RunId {
        let mut command = self.command();
        command.args(["verify", "--all", "--jobs", "1", "--json"]);
        if let Some(key) = key {
            command.args(["--request-key", key]);
        }
        let output = support::os::run(&mut command);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let accepted: Value = serde_json::from_slice(&output.stdout).unwrap();
        accepted["run"].as_str().unwrap().parse().unwrap()
    }
    async fn controlled(&self, id: &RunId) -> store::RunView {
        let mut changes = artifactize_core::changes::Subscription::new(&self.state).await;
        tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
            loop {
                let view = runs::view(&self.context(), id).await.unwrap();
                if self.repo.join("ready").exists()
                    && view.requests[0].status() == RequestStatus::Green
                {
                    return view;
                }
                assert_eq!(view.run.status(), RunStatus::Running);
                changes.next_reconciled().await;
            }
        })
        .await
        .unwrap()
    }
    async fn settled(&self, id: &RunId) -> runs::RunObservation {
        runs::wait(
            &self.context(),
            id,
            Some(support::os::patience(Duration::from_secs(30))),
            CancellationToken::new(),
        )
        .await
        .unwrap()
    }
    fn watcher(&self, args: &[&str]) -> Child {
        self.command()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }
}

#[tokio::test]
async fn a1_submit_watch_disposal_and_explicit_cancel_preserve_run_and_evidence() {
    let fixture = Fixture::new();
    let run = fixture.submit(Some("watch-attempt"));
    let view = fixture.controlled(&run).await;
    assert_eq!(
        view.run.status(),
        RunStatus::Running,
        "submission returned before release"
    );
    let mut watcher = fixture.watcher(&[
        "verify",
        "--all",
        "--wait",
        "--request-key",
        "watch-attempt",
    ]);
    use std::io::BufRead;
    let mut announced = String::new();
    std::io::BufReader::new(watcher.stderr.take().unwrap())
        .read_line(&mut announced)
        .unwrap();
    assert!(announced.contains(run.as_str()));
    support::os::interrupt(watcher.id());
    assert_eq!(watcher.wait_with_output().unwrap().status.code(), Some(130));
    let closed = fixture.watcher(&[
        "verify",
        "--all",
        "--wait",
        "--request-key",
        "watch-attempt",
    ]);
    // A killed terminal cannot run any graceful cancellation path.
    support::os::kill(closed.id());
    let _ = closed.wait_with_output().unwrap();
    assert!(
        !runs::status(&fixture.context(), &run)
            .await
            .unwrap()
            .state
            .is_terminal()
    );
    fs::write(fixture.repo.join("release"), "release").unwrap();
    assert_eq!(fixture.settled(&run).await.state, runs::State::Green);
    let output = fixture
        .command()
        .args([
            "verify",
            "--all",
            "--wait",
            "--request-key",
            "watch-attempt",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    fs::remove_file(fixture.repo.join("release")).unwrap();
    fs::remove_file(fixture.repo.join("ready")).unwrap();
    let cancelled = fixture.submit(None);
    let active = fixture.controlled(&cancelled).await;
    let child = active.requests[1].child.as_ref().unwrap().pid.get();
    let output = fixture
        .command()
        .args(["run", "cancel", cancelled.as_str()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fixture.settled(&cancelled).await.state,
        runs::State::Cancelled
    );
    let final_view = runs::view(&fixture.context(), &cancelled).await.unwrap();
    assert_eq!(final_view.requests[0].status(), RequestStatus::Green);
    assert_eq!(
        final_view.run.control.unwrap().failure_code,
        Some(FailureCode::Cancelled)
    );
    assert!(!support::os::running(child));
    assert!(
        fixture
            .command()
            .args(["run", "cancel", cancelled.as_str()])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[tokio::test]
async fn a2_request_keys_deduplicate_sequential_and_concurrent_attempts_only() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("release"), "release").unwrap();
    let first = fixture.submit(Some("attempt:one"));
    assert_eq!(fixture.settled(&first).await.state, runs::State::Green);
    // Existing keys are looked up before today's now-invalid declarations.
    let declaration = fs::read(fixture.repo.join("index.artf")).unwrap();
    fs::write(fixture.repo.join("index.artf"), "invalid declaration").unwrap();
    assert_eq!(fixture.submit(Some("attempt:one")), first);
    fs::write(fixture.repo.join("index.artf"), declaration).unwrap();
    let left = fixture.watcher(&["verify", "--all", "--request-key", "attempt:two", "--json"]);
    let right = fixture.watcher(&["verify", "--all", "--request-key", "attempt:two", "--json"]);
    let ids = [left, right].map(|child| {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["run"]
            .as_str()
            .unwrap()
            .parse::<RunId>()
            .unwrap()
    });
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[0], first);
    assert_eq!(fixture.settled(&ids[0]).await.state, runs::State::Green);
    let unkeyed = [fixture.submit(None), fixture.submit(None)];
    assert_ne!(unkeyed[0], unkeyed[1]);
    for id in &unkeyed {
        assert_eq!(fixture.settled(id).await.state, runs::State::Green);
    }
    let db = rusqlite::Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM runs", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        4
    );
}

#[tokio::test]
async fn a3_v6_runs_cache_and_human_approvals_survive_an_idempotent_upgrade() {
    let fixture = Fixture::new();
    let definition = json!({"name":"app","fingerprint":{"files":["version"]},"evals":[
        {"id":"check","title":"Check","profile":{"kind":"runtime","command":support::os::true_program(),"args":[]},"payload":{"instruction":"Check."}},
        {"id":"approve","title":"Approve","profile":{"kind":"human"},"payload":{"instruction":"Review."}}
    ]});
    support::declaration::write(fixture.repo.join("index.artf"), definition.to_string()).unwrap();
    fs::write(fixture.repo.join("version"), "v1").unwrap();
    let context = fixture.context();
    let started = runs::start(&context, &Selection::All, &VerifyOptions::default(), None)
        .await
        .unwrap();
    let mut changes = artifactize_core::changes::Subscription::new(&fixture.state).await;
    let request = tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
        loop {
            let view = runs::view(&context, &started.run).await.unwrap();
            if let Some(request) = view
                .requests
                .iter()
                .find(|request| request.status() == RequestStatus::WaitingHuman)
            {
                break request.id.clone();
            }
            changes.next_state().await;
        }
    })
    .await
    .unwrap();
    for args in [
        vec!["request", "claim", request.as_str(), "--reviewer", "tester"],
        vec![
            "request",
            "submit",
            request.as_str(),
            "--reviewer",
            "tester",
            "--verdict",
            "GREEN",
        ],
    ] {
        let output = fixture.command().args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fixture.settled(&started.run).await.state,
        runs::State::Green
    );
    let before = store::read_run(&fixture.state, &started.run).await.unwrap();
    let db = rusqlite::Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    let state_id: String = db
        .query_row("SELECT value FROM state_meta WHERE name='id'", [], |row| {
            row.get(0)
        })
        .unwrap();
    db.execute_batch("UPDATE runs SET data=json_remove(data,'$.control','$.requestKey'); DROP INDEX run_request_key; ALTER TABLE runs DROP COLUMN request_key; PRAGMA user_version=6;").unwrap();
    drop(db);
    // Existing-state read preflight upgrades too; no receipt/cache/approval is replaced.
    let migrated = store::read_run(&fixture.state, &started.run).await.unwrap();
    assert_eq!(migrated.run.id, before.run.id);
    assert!(migrated.run.control.is_none());
    assert_eq!(migrated.requests.len(), before.requests.len());
    let output = fixture
        .command()
        .args(["verify", "--all", "--wait", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reused: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reused["executionsStarted"], 0);
    assert!(
        reused["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|request| request["status"] == "GREEN")
    );
    for _ in 0..2 {
        store::Receipts::open(&fixture.state, &fixture.repo)
            .await
            .unwrap();
    }
    let db = rusqlite::Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        db.query_row("SELECT value FROM state_meta WHERE name='id'", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        state_id
    );
}

#[tokio::test]
async fn a4_dead_worker_errors_without_replay_and_cleans_owned_children() {
    let fixture = Fixture::new();
    let id = fixture.submit(Some("crash"));
    let active = fixture.controlled(&id).await;
    let runs::Worker::Admitted { owner } = active.run.control.unwrap().worker else {
        panic!("admitted worker");
    };
    let child = active.requests[1].child.as_ref().unwrap().pid.get();
    let mut changes = artifactize_core::changes::Subscription::new(&fixture.state).await;
    support::os::kill(owner.pid.get());
    // Only stored reads: no runs::status/view observer performs recovery on this path.
    // The independent supervisor must settle the dead executor itself.
    tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
        loop {
            let saved = store::read_run(&fixture.state, &id).await.unwrap();
            if saved.run.status() != RunStatus::Running {
                assert_eq!(saved.run.status(), RunStatus::Error);
                break;
            }
            changes.next_reconciled().await;
        }
    })
    .await
    .unwrap();
    let settled = runs::view(&fixture.context(), &id).await.unwrap();
    assert_eq!(
        settled.run.control.unwrap().failure_code,
        Some(FailureCode::OwnerDied)
    );
    assert_eq!(settled.requests[0].status(), RequestStatus::Green);
    assert_eq!(
        settled.requests[1].error_code(),
        Some(FailureCode::OwnerDied)
    );
    assert!(!support::os::running(child));
    assert_eq!(
        fixture.submit(Some("crash")),
        id,
        "a retry never replays interrupted work"
    );
    let db = rusqlite::Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM executions WHERE status='RUNNING'",
            [],
            |row| row.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    assert!(fixture.root.path().exists());
}
