//! Saved lifecycle corruption is isolated at the CLI/storage boundary.

use std::{fs, path::Path, process::Command};

use artifactize::{
    project::{self, VerifyOptions, selection::Selection},
    store::{self, DATABASE, Execution, Request},
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

mod support;

fn cli(repo: &Path, state: &Path, args: &[&str], json_output: bool) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
    command
        .arg("--repo")
        .arg(repo)
        .arg("--state-dir")
        .arg(state)
        .args(args);
    if json_output {
        command.arg("--json");
    }
    let output = command.output().unwrap();
    assert!(
        matches!(output.status.code(), Some(0..=2)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

async fn verify(repo: &Path, state: &Path) -> store::RunView {
    project::verify(
        repo,
        Some(state),
        &Selection::All,
        &VerifyOptions::default(),
        CancellationToken::new(),
    )
    .await
    .unwrap()
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
            {"id":"bad","title":"Bad","payload":{"instruction":"Check."},"profile":{"kind":"runtime","command":support::os::bin("echo"),"args":["bad"]}},
            {"id":"healthy","title":"Healthy","payload":{"instruction":"Check."},"profile":{"kind":"runtime","command":support::os::bin("echo"),"args":["healthy"]}}
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
    let monitor =
        artifactize::monitor::progress(&saved, &requests, time::OffsetDateTime::now_utc());
    for id in [request.id.as_str(), execution.as_str()] {
        assert!(
            monitor
                .errors
                .iter()
                .any(|(bad_id, reason)| bad_id == id && reason.contains("Unreadable"))
        );
    }
    let detail = artifactize::monitor::detail(
        &saved,
        &requests,
        &artifactize::monitor::Target::Run,
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
    assert_eq!(second.run.status.as_str(), "GREEN");
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
        "name":"app", "evals":[{"id":"check","title":"Check","payload":{"instruction":"Check."},"profile":{"kind":"runtime","command":support::os::bin("echo"),"args":["ok"]}}]
    }).to_string()).unwrap();
    let run = verify(&repo, &state).await;
    let db = Connection::open(state.join(DATABASE)).unwrap();
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
