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
use support::os::Launch;

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
        support::declaration::write(&(repo), repo.join("index.artf"), json!({"name":"app", "fingerprint":false, "evals":[
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
                    if support::declaration::read_text(self.repo.join("ready"))
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
                    assert!(view.run.status().is_open());
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
                .launch(),
        )
    }
}

const DETACHED_SUBMITTER: &str = "ARTIFACTIZE_TEST_DETACHED_SUBMITTER";

#[tokio::test]
async fn detached_submitter() {
    let Some(root) = support::os::environment_variable(DETACHED_SUBMITTER) else {
        return;
    };
    let root = std::path::Path::new(&root);
    let executable = support::os::environment_variable("ARTIFACTIZE_TEST_DETACHED_WORKER")
        .expect("the test owns a gated worker executable");
    let context = runs::Context::new(
        &root.join("repo"),
        Some(&root.join("state")),
        std::path::Path::new(&executable),
    )
    .unwrap();
    runs::start(&context, &Selection::All, &VerifyOptions::default(), None)
        .await
        .unwrap();
}

#[tokio::test]
async fn a1_submit_watch_disposal_and_explicit_cancel_preserve_run_and_evidence() {
    let executable = support::os::library_worker();
    let detached = Fixture::new();
    let runs = detached.state.join("runs");
    support::os::create_private_dir_all(&detached.state);
    fs::create_dir(&runs).unwrap();
    support::os::share_dir(&runs);
    assert!(!support::os::private_dir(&runs));
    let admission = detached.state.join("fixture-admission");
    fs::create_dir(&admission).unwrap();
    let gate = owned_run::FileGate::new(admission.join("release"));
    support::os::publish_file(&detached.repo.join("release"), "ready");

    // Use the durable submission API with an owned fixture worker that cannot enter
    // admission (or start any eval) until its submitter has demonstrably exited.
    let mut submitter = support::os::test_command(
        "detached_submitter",
        &[
            (
                DETACHED_SUBMITTER,
                detached._root.path().as_os_str().to_owned(),
            ),
            (
                "ARTIFACTIZE_TEST_DETACHED_WORKER",
                executable.path().as_os_str().to_owned(),
            ),
        ],
    );
    submitter.env("ARTIFACTIZE_REMOTE", "off");
    let submitted = Watcher::new(support::os::start(&mut submitter))
        .wait_with_output()
        .unwrap();
    assert!(submitted.status.success(), "{submitted:?}");
    support::os::wait_until(Duration::from_secs(10), || admission.join("held").is_file());
    let saved = store::read_runs(&detached.state, Some(&detached.repo), 2, 0)
        .await
        .unwrap();
    let [saved] = saved.as_slice() else {
        panic!("expected one submitted Run: {saved:?}")
    };
    let id = saved.id.clone();
    let held = store::read_run(&detached.state, &id).await.unwrap();
    assert!(held.run.status().is_open(), "{held:?}");
    assert_eq!(held.run.executions_started, 0);
    gate.release().unwrap();
    assert_eq!(detached.settled(&id).await.run.status(), RunStatus::Green);
    let directory = runs.join(id.as_str());
    assert!(support::os::private_dir(&directory));
    assert!(support::os::private_file(&directory.join("worker.stderr")));
    let completed = store::read_run(&detached.state, &id).await.unwrap();
    assert!(matches!(
        completed.run.control.unwrap().worker,
        runs::Worker::Admitted { .. }
    ));
    assert_eq!(completed.requests.len(), 2);
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
    let confirm_attachment = |watcher: &mut owned_run::Watcher| {
        use std::io::BufRead;
        let mut announced = String::new();
        std::io::BufReader::new(watcher.stderr.take().unwrap())
            .read_line(&mut announced)
            .unwrap();
        assert!(
            announced.starts_with(&format!("Started {run} ")),
            "{announced}"
        );
    };
    confirm_attachment(&mut watcher);
    support::os::interrupt(watcher.id());
    assert_eq!(watcher.wait_with_output().unwrap().status.code(), Some(130));
    let mut closed = fixture.watcher(&[
        "verify",
        "--all",
        "--wait",
        "--request-key",
        "watch-attempt",
    ]);
    confirm_attachment(&mut closed);
    // A killed terminal cannot run any graceful cancellation path.
    support::os::kill(closed.id());
    let _ = closed.wait_with_output().unwrap();
    assert!(
        store::read_run(&fixture.state, &run)
            .await
            .unwrap()
            .run
            .status()
            .is_open()
    );
    fs::write(
        support::declaration::source_path(fixture.repo.join("release")),
        "release",
    )
    .unwrap();
    assert_eq!(fixture.settled(&run).await.run.status(), RunStatus::Green);
    let output = support::os::run(fixture.command().args([
        "verify",
        "--all",
        "--wait",
        "--request-key",
        "watch-attempt",
    ]));
    assert_eq!(output.status.code(), Some(0));
    support::declaration::remove_file(fixture.repo.join("release")).unwrap();
    support::declaration::remove_file(fixture.repo.join("ready")).unwrap();
    let cancelled = fixture.submit(None);
    let active = fixture.controlled(&cancelled).await;
    let child = active.requests[1].child.as_ref().unwrap().pid.get();
    let output = support::os::run(
        fixture
            .command()
            .args(["run", "cancel", cancelled.as_str()]),
    );
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
        support::os::run(
            fixture
                .command()
                .args(["run", "cancel", cancelled.as_str()])
        )
        .status
        .success()
    );
}

#[tokio::test]
async fn a2_request_keys_deduplicate_sequential_and_concurrent_attempts_only() {
    let fixture = Fixture::new();
    fs::write(
        support::declaration::source_path(fixture.repo.join("release")),
        "release",
    )
    .unwrap();
    let first = fixture.submit(Some("attempt:one"));
    assert_eq!(fixture.settled(&first).await.run.status(), RunStatus::Green);
    // Existing keys are looked up before today's now-invalid declarations.
    let declaration = support::declaration::read_file(fixture.repo.join("index.artf")).unwrap();
    fs::write(
        support::declaration::source_path(fixture.repo.join("index.artf")),
        "invalid declaration",
    )
    .unwrap();
    assert_eq!(fixture.submit(Some("attempt:one")), first);
    fs::write(
        support::declaration::source_path(fixture.repo.join("index.artf")),
        declaration,
    )
    .unwrap();
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
    support::declaration::write(
        &(fixture.repo),
        fixture.repo.join("index.artf"),
        definition.to_string(),
    )
    .unwrap();
    fs::write(
        support::declaration::source_path(fixture.repo.join("version")),
        "v1",
    )
    .unwrap();
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
                    .find(|request| request.status() == RequestStatus::Deferred)
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
        let output = support::os::run(fixture.command().args(args));
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
    let database = fixture.state.join(store::DATABASE);
    let read = || {
        rusqlite::Connection::open_with_flags(&database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
    };
    let state_id: String = read()
        .query_row("SELECT value FROM state_meta WHERE name='id'", [], |row| {
            row.get(0)
        })
        .unwrap();
    // Back to schema 8, which kept a Run's definitions in its record, then back to schema 6.
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch(support::schema::TEN).unwrap();
    db.execute_batch(support::schema::EIGHT).unwrap();
    assert!(
        read()
            .query_row(
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
    // The definitions read back as they were saved.
    assert_eq!(
        serde_json::to_value(&migrated.run.definitions).unwrap(),
        serde_json::to_value(&before.run.definitions).unwrap()
    );
    assert_eq!(migrated.requests.len(), before.requests.len());
    let output = support::os::run(
        fixture
            .command()
            .args(["verify", "--all", "--wait", "--json"]),
    );
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
                if !saved.run.status().is_open() {
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
