//! A1–A4: one observable seam per Run submission/lifecycle/key/migration decision.
use artifactize_core::{
    project::{VerifyOptions, selection::Selection},
    runs, store,
    types::{FailureCode, RequestStatus, RunId, RunStatus},
};
#[path = "support/run.rs"]
mod owned_run;
use owned_run::Watcher;

use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

struct Fixture {
    _root: owned_run::Root,
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
            {"id":"z-controlled","title":"Controlled", "profile":{"kind":"runtime","command":support::os::python_program(),"args":[support::os::path_text(&script)]},"payload":{"instruction":"Check."}}
        ]}).to_string()).unwrap();
        let state = root.path().join("state");
        let root = owned_run::Root::from(root);
        root.track(&state, &repo);
        Self {
            _root: root,
            repo,
            state,
        }
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
        owned_run::subscribed(&self.state, async |changes| {
            tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
                loop {
                    let view = store::read_run(&self.state, id).await.unwrap();
                    if fs::read_to_string(self.repo.join("ready"))
                        .is_ok_and(|contents| contents == "ready")
                        && view.requests[0].status() == RequestStatus::Green
                        && view.requests[1].status() == RequestStatus::Running
                        && view.requests[1].child.is_some()
                        && matches!(
                            view.run.control.as_ref().map(|control| control.worker),
                            Some(runs::Worker::Admitted { .. })
                        )
                    {
                        return view;
                    }
                    assert_eq!(view.run.status(), RunStatus::Running);
                    changes.next_reconciled().await;
                }
            })
            .await
            .unwrap()
        })
        .await
    }
    async fn settled(&self, id: &RunId) -> store::RunView {
        owned_run::terminal(&self.state, id).await
    }
    fn watcher(&self, args: &[&str]) -> Watcher {
        Watcher::new(
            self.command()
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    }
}

#[tokio::test]
async fn a1_submit_watch_disposal_and_explicit_cancel_preserve_run_and_evidence() {
    // The real project had 35 requests when a submit-only caller exited before admission.
    // Retain a substantial saved graph so reading its snapshot overlaps the handoff naturally;
    // no elapsed-time assertion or watcher keeps the submitter alive.
    const DETACHED_REQUESTS: usize = 35;
    const INSTRUCTION_BYTES: usize = 64 * 1024;
    let detached = Fixture::new();
    let runs = detached.state.join("runs");
    support::os::create_private_dir_all(&detached.state);
    fs::create_dir(&runs).unwrap();
    support::os::share_dir(&runs);
    assert!(!support::os::private_dir(&runs));
    let instruction = "Check. ".repeat(INSTRUCTION_BYTES / "Check. ".len());
    let evals: Vec<_> = (0..DETACHED_REQUESTS)
        .map(|index| {
            json!({
                "id":format!("check-{index}"),"title":"Check",
                "profile":{"kind":"runtime","command":support::os::true_program(),"args":[]},
                "payload":{"instruction":instruction},
            })
        })
        .collect();
    support::declaration::write(
        detached.repo.join("index.artf"),
        json!({
            "name":"app","fingerprint":false,"evals":evals,
        })
        .to_string(),
    )
    .unwrap();
    let submitted = support::os::run(detached.command().args(["verify", "--all", "--json"]));
    assert!(submitted.status.success(), "{submitted:?}");
    let accepted: Value = serde_json::from_slice(&submitted.stdout).unwrap();
    let id: RunId = accepted["run"].as_str().unwrap().parse().unwrap();
    assert_eq!(detached.settled(&id).await.run.status(), RunStatus::Green);
    let directory = runs.join(id.as_str());
    assert!(support::os::private_dir(&directory));
    assert!(support::os::private_file(&directory.join("worker.stderr")));
    let completed = store::read_run(&detached.state, &id).await.unwrap();
    assert!(matches!(
        completed.run.control.unwrap().worker,
        runs::Worker::Admitted { .. }
    ));
    assert_eq!(completed.requests.len(), DETACHED_REQUESTS);
    assert!(
        completed
            .requests
            .iter()
            .all(|request| request.status() == RequestStatus::Green)
    );

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
        store::read_run(&fixture.state, &run)
            .await
            .unwrap()
            .run
            .status()
            == RunStatus::Running
    );
    fs::write(fixture.repo.join("release"), "release").unwrap();
    assert_eq!(fixture.settled(&run).await.run.status(), RunStatus::Green);
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
        fixture.settled(&cancelled).await.run.status(),
        RunStatus::Error
    );
    let final_view = store::read_run(&fixture.state, &cancelled).await.unwrap();
    assert_eq!(final_view.requests[0].status(), RequestStatus::Green);
    assert_eq!(
        final_view.run.control.unwrap().failure_code,
        Some(FailureCode::Cancelled)
    );
    support::os::wait_until(Duration::from_secs(30), || !support::os::running(child));
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
    assert_eq!(fixture.settled(&first).await.run.status(), RunStatus::Green);
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
    assert_eq!(
        fixture.settled(&ids[0]).await.run.status(),
        RunStatus::Green
    );
    let unkeyed = [fixture.submit(None), fixture.submit(None)];
    assert_ne!(unkeyed[0], unkeyed[1]);
    for id in &unkeyed {
        assert_eq!(fixture.settled(id).await.run.status(), RunStatus::Green);
    }
    // Read every saved attempt so an unexpected duplicate cannot fall outside the page.
    let saved = store::read_runs(&fixture.state, Some(&fixture.repo), u32::MAX, 0)
        .await
        .unwrap();
    let ids = std::collections::BTreeSet::from([
        first,
        ids[0].clone(),
        unkeyed[0].clone(),
        unkeyed[1].clone(),
    ]);
    assert_eq!(
        saved
            .into_iter()
            .map(|run| run.id)
            .collect::<std::collections::BTreeSet<_>>(),
        ids,
        "history exposes exactly the distinct submission attempts"
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
    let request = owned_run::subscribed(&fixture.state, async |changes| {
        tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
            loop {
                let view = store::read_run(&fixture.state, &started.run).await.unwrap();
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
        .unwrap()
    })
    .await;
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
        fixture.settled(&started.run).await.run.status(),
        RunStatus::Green
    );
    let before = store::read_run(&fixture.state, &started.run).await.unwrap();
    // Saved terminal state precedes closing the worker's final SQLite connection.
    support::runs::finished_worker(
        &fixture.state,
        &fixture.repo,
        &started.run,
        support::os::running,
    );
    let db = rusqlite::Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    let state_id: String = db
        .query_row("SELECT value FROM state_meta WHERE name='id'", [], |row| {
            row.get(0)
        })
        .unwrap();
    // Back to schema 8, which kept a Run's definitions in its record, then back to schema 6.
    db.execute_batch(support::schema::EIGHT).unwrap();
    assert!(
        db.query_row(
            "SELECT json_type(data,'$.definitions') FROM runs WHERE id=?",
            [&started.run],
            |row| row.get::<_, String>(0)
        )
        .unwrap()
            == "object"
    );
    db.execute_batch("UPDATE runs SET data=json_remove(data,'$.control','$.requestKey'); DROP INDEX run_request_key; ALTER TABLE runs DROP COLUMN request_key; DROP INDEX active_key; ALTER TABLE executions DROP COLUMN profile_identity; CREATE UNIQUE INDEX active_key ON executions(key) WHERE key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN'); PRAGMA user_version=6;").unwrap();
    drop(db);
    // Existing-state read preflight upgrades too; no receipt/cache/approval is replaced.
    let migrated = store::read_run(&fixture.state, &started.run).await.unwrap();
    assert_eq!(migrated.run.id, before.run.id);
    assert!(migrated.run.control.is_none());
    // The definitions moved to their own row and read back as they were saved.
    assert_eq!(
        serde_json::to_value(&migrated.run.definitions).unwrap(),
        serde_json::to_value(&before.run.definitions).unwrap()
    );
    let moved = rusqlite::Connection::open(fixture.state.join(store::DATABASE))
        .unwrap()
        .query_row(
            "SELECT json_type(r.data,'$.definitions'),d.run_id FROM runs r JOIN run_definitions d ON d.run_id=r.id WHERE r.id=?",
            [&started.run],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap();
    assert_eq!(moved.0, None);
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
        store::OwnedReceipts::open(&fixture.state, &fixture.repo)
            .await
            .unwrap();
    }
    let db = rusqlite::Connection::open_with_flags(
        fixture.state.join(store::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        store::STATE_SCHEMA_VERSION
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
    owned_run::subscribed(&fixture.state, async |changes| {
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
    })
    .await;
    let settled = store::read_run(&fixture.state, &id).await.unwrap();
    assert_eq!(
        settled.run.control.unwrap().failure_code,
        Some(FailureCode::OwnerDied)
    );
    assert_eq!(settled.requests[0].status(), RequestStatus::Green);
    assert_eq!(
        settled.requests[1].error_code(),
        Some(FailureCode::OwnerDied)
    );
    support::os::wait_until(Duration::from_secs(30), || !support::os::running(child));
    assert!(!support::os::running(child));
    assert_eq!(
        fixture.submit(Some("crash")),
        id,
        "a retry never replays interrupted work"
    );
}
