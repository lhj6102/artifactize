//! Saved lifecycle corruption is isolated at the CLI/storage boundary.

use std::{fs, path::Path, process::Command};

use artifactize_core::{
    project::{VerifyOptions, selection::Selection},
    store::{self, DATABASE, Execution, Request},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

#[path = "support/run.rs"]
mod owned_run;

fn cli(repo: &Path, state: &Path, args: &[&str], json_output: bool) -> String {
    let mut command = Command::new(support::os::cli());
    command
        .arg("--repo")
        .arg(repo)
        .arg("--state-dir")
        .arg(state)
        .args(args);
    if json_output {
        command.arg("--json");
    }
    let output = command.record_output().unwrap();
    assert!(
        matches!(output.status.code(), Some(0..=2)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

async fn verify(repo: &Path, state: &Path) -> store::RunView {
    let context =
        artifactize_core::runs::Context::new(repo, Some(state), &support::os::cli()).unwrap();
    let started =
        artifactize_core::runs::start(&context, &Selection::All, &VerifyOptions::default(), None)
            .await
            .unwrap();
    // Own this submission only: other rows are deliberately corrupted fixture inputs.
    // The lease also waits for the worker's final connections before corruption begins.
    let _run = owned_run::RunLease::new(state, repo, started.run.clone());
    owned_run::terminal(state, &started.run).await
}

#[tokio::test]
async fn owned_root_releases_every_repository_before_removal_on_return_and_unwind() {
    for unwind in [false, true] {
        let root = owned_run::Root::from(support::os::tempdir());
        let directory = root.path().to_owned();
        let state = root.path().join("state");
        let mut owners = Vec::new();
        for name in ["first", "second"] {
            let repo = root.path().join(name);
            fs::create_dir(&repo).unwrap();
            fs::write(repo.join("index.artf"), "name = 'app'\nfingerprint = {}\n[evals.review]\ntitle = 'Review'\nprofile = { kind = 'human' }\n[evals.review.payload]\ninstruction = 'Review.'\n").unwrap();
            root.track(&state, &repo);
            let saved = support::runs::record(
                &repo,
                Some(&state),
                &Selection::All,
                &VerifyOptions::default(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
            let control = saved.run.control.unwrap();
            let artifactize_core::runs::Worker::Admitted { owner } = control.worker else {
                panic!("fixture worker was admitted");
            };
            owners.push(owner);
            owners.extend(control.supervisor);
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let root = root;
            assert!(!unwind, "fixture assertion failure");
            drop(root);
        }));
        if unwind {
            assert_eq!(
                result.unwrap_err().downcast_ref::<&str>(),
                Some(&"fixture assertion failure")
            );
        } else {
            result.unwrap();
        }
        assert!(
            owners
                .into_iter()
                .all(|owner| !support::os::same_process_running(owner.pid.get(), owner.start_time))
        );
        assert!(
            !directory.exists(),
            "only released fixture inputs are removed"
        );
    }
}

#[tokio::test]
async fn failed_root_cleanup_retains_inputs_preserves_panics_and_releases_other_scopes() {
    for unwind in [false, true] {
        // No auto-deleting ancestor may remove inputs retained by failed Run cleanup.
        let root = owned_run::Root::from(support::os::tempdir());
        let directory = root.path().to_owned();
        let broken = root.path().join("a-broken-state");
        let state = root.path().join("z-active-state");
        let repo = root.path().join("repo");
        fs::create_dir(&broken).unwrap();
        fs::create_dir(&repo).unwrap();
        fs::write(broken.join(DATABASE), "not SQLite").unwrap();
        fs::write(repo.join("input"), "owned input").unwrap();
        fs::write(repo.join("index.artf"), "name = 'app'\nfingerprint = {}\n[evals.review]\ntitle = 'Review'\nprofile = { kind = 'human' }\n[evals.review.payload]\ninstruction = 'Review.'\n").unwrap();
        root.track(&broken, &repo);
        root.track(&state, &repo);
        let saved = support::runs::record(
            &repo,
            Some(&state),
            &Selection::All,
            &VerifyOptions::default(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        let control = saved.run.control.unwrap();
        let artifactize_core::runs::Worker::Admitted { owner } = control.worker else {
            panic!("fixture worker was admitted");
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let root = root;
            assert!(!unwind, "original fixture panic");
            drop(root);
        }));
        let panic = result.unwrap_err();
        if unwind {
            assert_eq!(
                panic.downcast_ref::<&str>(),
                Some(&"original fixture panic")
            );
        } else {
            assert!(
                panic
                    .downcast_ref::<String>()
                    .unwrap()
                    .starts_with("Fixture Run cleanup failed; inputs retained at ")
            );
        }
        for owner in std::iter::once(owner).chain(control.supervisor) {
            assert!(!support::os::same_process_running(
                owner.pid.get(),
                owner.start_time
            ));
        }
        assert_eq!(fs::read(repo.join("input")).unwrap(), b"owned input");
        assert!(directory.exists(), "failed cleanup retains inputs");
        // Explicit removal is safe only after every saved worker/supervisor has exited.
        fs::remove_dir_all(&directory).unwrap();
    }
}

#[tokio::test]
async fn contradictory_rows_are_unreadable_without_aborting_queries_or_verify() {
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(repo.join("index.artf"), json!({
        "name":"app",
        "evals":[
            {"id":"bad","title":"Bad","payload":{"instruction":"Check."},"profile":{"kind":"runtime","command":support::os::echo_program(),"args":["bad"]}},
            {"id":"healthy","title":"Healthy","payload":{"instruction":"Check."},"profile":{"kind":"runtime","command":support::os::echo_program(),"args":["healthy"]}}
        ]
    }).to_string()).unwrap();
    let first = verify(&repo, &state).await;
    assert_eq!(first.requests.len(), 2);
    let request = &first.requests[0];
    let execution = request.execution_id.as_ref().unwrap();
    let db = Connection::open(state.join(DATABASE)).unwrap();
    db.execute(
        "UPDATE requests SET data=json_set(data,'$.error','corrupted request') WHERE id=?",
        [&request.id],
    )
    .unwrap();
    // Contradictory ERROR execution still indexed as the formerly reusable GREEN row.
    db.execute("UPDATE executions SET status='ERROR',data=json_set(data,'$.status','ERROR','$.error','corrupted execution','$.errorCode','RUNTIME_ERROR') WHERE id=?", [execution]).unwrap();
    // Keep the independent cache index GREEN to prove reuse parses the JSON, not only SQL.
    db.execute(
        "UPDATE executions SET status='GREEN' WHERE id=?",
        [execution],
    )
    .unwrap();
    let listed: Value = serde_json::from_str(&cli(&repo, &state, &["run", "list"], true)).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["unreadable"].as_array().unwrap().len(), 2);
    let text = cli(&repo, &state, &["run", "list"], false);
    for id in [request.id.as_str(), execution.as_str()] {
        assert!(text.contains(id), "{text}");
    }
    assert!(text.contains("matching result"), "{text}");
    assert!(text.contains("cannot carry a verdict result"), "{text}");
    let shown: Value = serde_json::from_str(&cli(
        &repo,
        &state,
        &["run", "show", first.run.id.as_str()],
        true,
    ))
    .unwrap();
    assert_eq!(shown["requests"].as_array().unwrap().len(), 1);
    assert_eq!(shown["requests"][0]["evalId"], "app/healthy");
    assert_eq!(shown["unreadable"].as_array().unwrap().len(), 2);
    let requests = store::read_requests(&state, Some(&first.run.id))
        .await
        .unwrap();
    assert_eq!(requests.len(), 1);
    let saved = store::read_run(&state, &first.run.id).await.unwrap();
    let (snapshot, views) = store::read_run_requests(&state, &first.run.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&snapshot).unwrap(),
        serde_json::to_value(&saved).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&views).unwrap(),
        serde_json::to_value(&requests).unwrap()
    );
    let monitor =
        artifactize_core::monitor::progress(&saved, &requests, time::OffsetDateTime::now_utc());
    for id in [request.id.as_str(), execution.as_str()] {
        assert!(
            monitor
                .errors
                .iter()
                .any(|(bad_id, reason)| bad_id == id && reason.contains("Unreadable"))
        );
    }
    let detail = artifactize_core::monitor::detail(
        &saved,
        &requests,
        &artifactize_core::monitor::Target::Run,
        time::OffsetDateTime::now_utc(),
    );
    assert!(detail.field("Errors").unwrap().contains(execution.as_str()));
    let status: Value = serde_json::from_str(&cli(&repo, &state, &["status"], true)).unwrap();
    assert_eq!(status["unreadable"].as_array().unwrap().len(), 2);
    let status_text = cli(&repo, &state, &["status"], false);
    for id in [request.id.as_str(), execution.as_str()] {
        assert!(status_text.contains(id), "{status_text}");
    }
    let second = verify(&repo, &state).await;
    assert_eq!(second.run.status().as_str(), "GREEN");
    assert_ne!(second.requests[0].execution_id.as_ref(), Some(execution));
    assert_eq!(
        second.requests[1].execution_id,
        first.requests[1].execution_id
    );
    assert_eq!(
        store::read_run(&state, &first.run.id)
            .await
            .unwrap()
            .unreadable
            .len(),
        2
    );
}

#[tokio::test]
async fn valid_lifecycle_json_is_byte_identical_and_legacy_completion_rules_survive() {
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(repo.join("index.artf"), json!({
        "name":"app", "evals":[{"id":"check","title":"Check","payload":{"instruction":"Check."},"profile":{"kind":"runtime","command":support::os::echo_program(),"args":["ok"]}}]
    }).to_string()).unwrap();
    let run = verify(&repo, &state).await;
    let db = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let request_text: String = db
        .query_row(
            "SELECT data FROM requests WHERE id=?",
            [&run.requests[0].id],
            |row| row.get(0),
        )
        .unwrap();
    let execution_text: String = db
        .query_row(
            "SELECT data FROM executions WHERE id=?",
            params![run.requests[0].execution_id],
            |row| row.get(0),
        )
        .unwrap();
    let request: Request = serde_json::from_str(&request_text).unwrap();
    let execution: Execution = serde_json::from_str(&execution_text).unwrap();
    assert_eq!(serde_json::to_string(&request).unwrap(), request_text);
    assert_eq!(serde_json::to_string(&execution).unwrap(), execution_text);
    let base = serde_json::to_value(&request).unwrap();
    for status in [
        "QUEUED",
        "BLOCKED",
        "BUDGET_EXHAUSTED",
        "STALE",
        "UNREVIEWED",
        "WAIT_DEPENDENCY",
        "RUNNING",
        "WAITING_HUMAN",
        "GREEN",
        "RED",
        "ERROR",
    ] {
        for completed in [Value::Null, json!("2026-01-01T00:00:00Z")] {
            let mut wire = base.clone();
            wire["status"] = json!(status);
            wire["completedAt"] = completed.clone();
            wire["result"] = if matches!(status, "GREEN" | "RED") {
                json!({"verdict":status})
            } else {
                Value::Null
            };
            wire["error"] = if status == "ERROR" {
                json!("failure")
            } else {
                Value::Null
            };
            wire["errorCode"] = Value::Null;
            let expected = match status {
                "GREEN" | "RED" | "ERROR" => !completed.is_null(),
                "RUNNING" | "WAITING_HUMAN" => completed.is_null(),
                _ => true,
            };
            assert_eq!(
                serde_json::from_value::<Request>(wire.clone()).is_ok(),
                expected,
                "{status}, {completed}"
            );
            if matches!(
                status,
                "RUNNING" | "WAITING_HUMAN" | "GREEN" | "RED" | "ERROR"
            ) {
                let mut execution_wire = serde_json::to_value(&execution).unwrap();
                for key in ["status", "result", "error", "errorCode", "completedAt"] {
                    execution_wire[key] = wire[key].clone();
                }
                assert_eq!(
                    serde_json::from_value::<Execution>(execution_wire).is_ok(),
                    expected,
                    "execution {status}, {completed}"
                );
            }
        }
    }
}

#[test]
fn run_lifecycle_keeps_legacy_bytes_and_rejects_every_illegal_combination() {
    // Independent schema-5 fixture: field order, nulls and omission rules are historical.
    let wire = r#"{"id":"legacy-run","repoPath":"repo","stateDir":"state","status":"RUNNING","createdAt":"2026-01-01T00:00:00.000000000Z","completedAt":null,"selection":{"kind":"all"},"profile":null,"definitions":null,"recursive":false,"force":false,"ignoreGates":false,"jobs":2,"maxExecutions":null,"executionsStarted":0,"waitTimeoutMs":null,"waitTimedOut":false,"validation":null,"error":null}"#;
    // Removed saved keys are ignored on read, without a schema migration.
    let historical = wire.replace("\"ignoreGates\":false", "\"ignoreGates\":true");
    let decoded = serde_json::from_str::<store::Run>(&historical).unwrap();
    assert_eq!(decoded.jobs, Some(2), "legacy numeric caps remain explicit");
    assert_eq!(serde_json::to_value(&decoded).unwrap()["jobs"], 2);
    let unlimited: store::Run = serde_json::from_str(&wire.replace("\"jobs\":2,", "")).unwrap();
    assert_eq!(unlimited.jobs, None);
    assert!(
        serde_json::to_value(unlimited)
            .unwrap()
            .get("jobs")
            .is_none()
    );
    assert!(
        serde_json::to_value(&decoded)
            .unwrap()
            .get("ignoreGates")
            .is_none()
    );
    for status in ["RUNNING", "GREEN", "RED", "ERROR", "INCOMPLETE"] {
        for at in ["null", "\"2026-01-01T00:00:01.000000000Z\""] {
            for error in ["null", "\"failure\""] {
                let text = wire
                    .replace(
                        "\"status\":\"RUNNING\"",
                        &format!("\"status\":\"{status}\""),
                    )
                    .replace("\"completedAt\":null", &format!("\"completedAt\":{at}"))
                    .replace("\"error\":null", &format!("\"error\":{error}"));
                let expected = match status {
                    "RUNNING" => at == "null" && error == "null",
                    "GREEN" | "RED" => at != "null" && error == "null",
                    _ => at != "null",
                };
                let run = serde_json::from_str::<store::Run>(&text);
                assert_eq!(run.is_ok(), expected, "{status}, {at}, {error}");
                if let Ok(run) = run {
                    assert_eq!(
                        serde_json::to_string(&run).unwrap(),
                        text.replace("\"ignoreGates\":false,", "")
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn contradictory_runs_are_reported_without_aborting_list_status_or_prune() {
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(repo.join("index.artf"), json!({
        "name":"app", "evals":[{"id":"check","title":"Check","payload":{"instruction":"Check."},"profile":{"kind":"runtime","command":support::os::echo_program(),"args":["ok"]}}]
    }).to_string()).unwrap();
    let healthy = verify(&repo, &state).await;
    // Corrupt the newest Run: the listings must still work.
    let bad = verify(&repo, &state).await;
    let db = Connection::open(state.join(DATABASE)).unwrap();
    db.execute(
        "UPDATE runs SET status='RUNNING',data=json_set(data,'$.status','RUNNING') WHERE id=?",
        [&bad.run.id],
    )
    .unwrap();
    let listed: Value = serde_json::from_str(&cli(&repo, &state, &["run", "list"], true)).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 2);
    let corrupted = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["id"] == bad.run.id.as_str())
        .unwrap();
    assert_eq!(corrupted["unreadable"][0]["kind"], "run");
    assert_eq!(corrupted["unreadable"][0]["id"], bad.run.id.as_str());
    assert!(
        corrupted["unreadable"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("A running Run cannot be completed")
    );
    for args in [["run", "list"].as_slice(), ["status"].as_slice()] {
        let text = cli(&repo, &state, args, false);
        assert!(text.contains(bad.run.id.as_str()), "{text}");
        assert!(text.contains("A running Run cannot be completed"), "{text}");
    }
    let status: Value = serde_json::from_str(&cli(&repo, &state, &["status"], true)).unwrap();
    assert_eq!(status["unreadable"][0]["id"], bad.run.id.as_str());
    assert_eq!(
        store::read_run(&state, &healthy.run.id)
            .await
            .unwrap()
            .run
            .status()
            .as_str(),
        "GREEN"
    );
    let shown = Command::new(support::os::cli())
        .arg("--repo")
        .arg(&repo)
        .arg("--state-dir")
        .arg(&state)
        .args(["run", "show", bad.run.id.as_str(), "--json"])
        .record_output()
        .unwrap();
    assert!(!shown.status.success());
    let error = format!(
        "{}{}",
        String::from_utf8_lossy(&shown.stdout),
        String::from_utf8_lossy(&shown.stderr)
    );
    assert!(error.contains(bad.run.id.as_str()), "{error}");
    assert!(
        error.contains("A running Run cannot be completed"),
        "{error}"
    );
    let pruned = Command::new(support::os::cli())
        .arg("--repo")
        .arg(&repo)
        .arg("--state-dir")
        .arg(&state)
        .args(["prune", "--dry-run", "--json"])
        .record_output()
        .unwrap();
    assert!(
        pruned.status.success(),
        "{}",
        String::from_utf8_lossy(&pruned.stderr)
    );
    assert!(String::from_utf8_lossy(&pruned.stderr).contains(bad.run.id.as_str()));
    let report: Value = serde_json::from_slice(&pruned.stdout).unwrap();
    assert!(
        report["skippedRuns"]
            .as_array()
            .unwrap()
            .contains(&json!(bad.run.id))
    );
    assert_eq!(store::read_catalog(&state).await.unwrap().len(), 1);
    assert_eq!(
        store::read_requests(&state, Some(&bad.run.id))
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(verify(&repo, &state).await.run.status().as_str(), "GREEN");
}
