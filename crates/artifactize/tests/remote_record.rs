use std::{fs, path::Path, process::Command};

use artifactize::{
    cache, human,
    project::{self, VerifyOptions, selection::Selection},
    remote::Record,
    store::Receipts,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn cli(state: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--state-dir")
        .arg(state)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn write_repo(repo: &Path, profile: Value) {
    fs::create_dir_all(repo).unwrap();
    fs::write(repo.join("identity"), "remote-v1\n").unwrap();
    fs::write(
        repo.join("check.sh"),
        "echo stdout-marker\necho stderr-marker >&2\n",
    )
    .unwrap();
    fs::write(
        repo.join("artifactize.json"),
        json!({"name":"app","stale":{"kind":"identity","script":{"command":"cat","args":["identity"]}},
            "evals":[{"id":"check","title":"Check","profile":profile,"payload":{"instruction":"Review."},
                "passSchema":{"type":"object","properties":{"approved":{"const":true}},"required":["approved"],"additionalProperties":false}}]})
        .to_string(),
    )
    .unwrap();
}

async fn verify(repo: &Path, state: &Path) -> artifactize::store::RunView {
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
async fn runtime_summary_omits_local_audit_and_its_mirror_is_reusable() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    write_repo(
        &repo,
        json!({"kind":"runtime","command":"/bin/sh","args":["check.sh"]}),
    );
    let producer_state = root.path().join("producer");
    assert_eq!(verify(&repo, &producer_state).await.run.status, "GREEN");
    let execution = cache::show(&producer_state, "remote-v1", None)
        .await
        .unwrap()
        .unwrap();
    let producer = execution.producer.clone().unwrap();
    assert_eq!(producer.version, env!("CARGO_PKG_VERSION"));
    assert!(producer.name.contains('@'));
    assert_eq!(
        execution.result.as_ref().unwrap()["stdout"],
        "stdout-marker\n"
    );

    let summary = Record::new(&execution, false).unwrap();
    let text = serde_json::to_string(&summary).unwrap();
    for leaked in [
        "argv",
        "stdout",
        "stderr",
        "toolCalls",
        "repoPath",
        "marker",
    ] {
        assert!(!text.contains(leaked), "{leaked} in {text}");
    }
    assert!(!text.contains(root.path().to_str().unwrap()), "{text}");
    let mut keys: Vec<_> = summary.result.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["durationMs", "exitCode", "truncated", "verdict"]);
    let full = Record::new(&execution, true).unwrap();
    assert_eq!(
        full.execution.unwrap().result.unwrap()["stderr"],
        "stderr-marker\n"
    );

    let mut stored = summary;
    stored.publisher = Some("alice-laptop".into());
    stored.published_at = Some("2026-10-04T00:00:00Z".into());
    let consumer_state = root.path().join("consumer");
    let receipts = Receipts::open(&consumer_state, &repo).await.unwrap();
    let mirror = stored.mirror("https://reviews.example/").unwrap();
    let mirrored = receipts.mirror_execution(&mirror).await.unwrap();
    assert_eq!(mirrored.id, format!("remote-{}", execution.id));
    assert_eq!(
        receipts.mirror_execution(&mirror).await.unwrap().id,
        mirrored.id
    );
    assert!(Record::new(&mirrored, false).is_err());

    let shown = cli(&consumer_state, &["cache", "show", "remote-v1"]);
    assert_eq!(
        shown["origin"],
        json!({"store":"https://reviews.example/","publisher":"alice-laptop","publishedAt":"2026-10-04T00:00:00Z"})
    );
    assert_eq!(shown["producer"], json!(producer));
    assert_eq!(shown["status"], "GREEN");
    assert!(shown["result"].get("stdout").is_none());
    assert_eq!(
        cli(&consumer_state, &["cache", "list"])[0]["origin"],
        "https://reviews.example/"
    );

    let reused = verify(&repo, &consumer_state).await;
    assert_eq!(reused.run.status, "GREEN");
    assert_eq!(reused.run.executions_started, 0);
    assert_eq!(reused.requests[0].execution_id.as_ref(), Some(&mirrored.id));

    // Executions saved before producers were recorded stay readable.
    Connection::open(producer_state.join("state.sqlite"))
        .unwrap()
        .execute(
            "UPDATE executions SET data=json_remove(data,'$.producer')",
            [],
        )
        .unwrap();
    assert!(
        cli(&producer_state, &["cache", "show", "remote-v1"])
            .get("producer")
            .is_none()
    );
}

#[tokio::test]
async fn human_summary_keeps_owner_fields_and_the_reviewer() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    write_repo(&repo, json!({"kind":"human"}));
    let state = root.path().join("state");
    let waiting = verify(&repo, &state).await;
    let request = &waiting.requests[0];
    assert_eq!(request.status, "WAITING_HUMAN");
    let receipts = Receipts::open(&state, &repo).await.unwrap();
    human::claim(&receipts, &request.id, "alice").await.unwrap();
    human::submit(
        &receipts,
        &request.id,
        "alice",
        &json!({"verdict":"GREEN","approved":true}),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let execution = cache::show(&state, "remote-v1", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(execution.reviewer.as_deref(), Some("alice"));
    let summary = Record::new(&execution, false).unwrap();
    assert_eq!(summary.reviewer.as_deref(), Some("alice"));
    assert_eq!(summary.result, json!({"verdict":"GREEN","approved":true}));
}
