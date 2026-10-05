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
    fs::write(repo.join("fingerprint"), "remote-v1\n").unwrap();
    fs::write(
        repo.join("check.sh"),
        "echo stdout-marker\necho stderr-marker >&2\n",
    )
    .unwrap();
    fs::write(
        repo.join("artifactize.json"),
        json!({"name":"app","fingerprint":{"script":{"command":"cat","args":["fingerprint"]}},
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
    let produced = verify(&repo, &producer_state).await;
    assert_eq!(produced.run.status, "GREEN");
    let key = produced.requests[0].key.clone().unwrap();
    let execution = cache::show(&producer_state, &key, false)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(execution.fingerprint.as_deref(), Some("remote-v1"));
    assert_eq!(
        execution.fingerprints,
        [("app".to_owned(), "remote-v1".to_owned())].into()
    );
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
    let mirrored = receipts.mirror_execution(&mirror).await.unwrap().unwrap();
    assert_eq!(mirrored.id, format!("remote-{}", execution.id));
    // The same record again is not newer than the local latest, which is now itself.
    assert!(receipts.mirror_execution(&mirror).await.unwrap().is_none());
    assert_eq!(
        cli(&consumer_state, &["cache", "list", "--history"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(Record::new(&mirrored, false).is_err());

    let shown = cli(&consumer_state, &["cache", "show", &key]);
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
        cli(&producer_state, &["cache", "show", &key])
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
    let execution = cache::show(&state, request.key.as_deref().unwrap(), false)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(execution.reviewer.as_deref(), Some("alice"));
    let summary = Record::new(&execution, false).unwrap();
    assert_eq!(summary.reviewer.as_deref(), Some("alice"));
    assert_eq!(summary.result, json!({"verdict":"GREEN","approved":true}));
}

/// Agent evals share results across backends, models, reasoning levels and limits: a result
/// another profile produced (here received from a review store) is reused without any
/// Agent call, and the request and the record show which profile produced it.
#[tokio::test]
async fn an_agent_result_is_reused_across_models_and_shows_its_profile() {
    use artifactize::{
        config::{Profile, read_workspace_config},
        store::{Execution, ExecutionOptions, Producer, Provenance},
    };
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let declared = json!({"kind":"agent","backend":"openai","model":"model-a","reasoning":"high","timeoutMs":60000});
    let fast = json!({"kind":"agent","backend":"anthropic","model":"model-b","reasoning":"low","maxTokens":500});
    write_repo(&repo, declared.clone());
    let path = repo.join("artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    declaration["evals"][0]["profileVariants"] = json!({ "fast": fast });
    fs::write(&path, declaration.to_string()).unwrap();

    // The record the `fast` variant produced elsewhere, keyed like this repository's eval.
    let config = read_workspace_config(&repo).unwrap();
    let state = root.path().join("state");
    let fingerprints = cache::prepare(&config, ["app"], &state, CancellationToken::new())
        .await
        .unwrap();
    let key = cache::eval_keys(&config, &fingerprints)["app/check"].clone();
    let variant: Profile = serde_json::from_value(fast.clone()).unwrap();
    let completed = "2026-10-04T00:00:01Z".to_owned();
    let produced = Execution {
        id: "execution-run-elsewhere-1".into(),
        key: Some(key.value.clone()),
        fingerprint: Some("remote-v1".into()),
        fingerprints: key.fingerprints.clone(),
        eval_def_hash: key.eval_def_hash.clone(),
        owner_pid: 1,
        owner_start_time: 1,
        status: "GREEN".into(),
        result: Some(json!({"verdict":"GREEN","approved":true})),
        error: None,
        error_code: None,
        profile: fast.clone(),
        options: ExecutionOptions::new(&variant, Some("fast")),
        usage: Some(json!([{"turn":1,"attempt":1,"usage":{"inputTokens":10}}])),
        tool_calls: Vec::new(),
        provenance: Provenance {
            repo_path: "/elsewhere".into(),
            run_id: "run-elsewhere".into(),
            request_id: "run-elsewhere-1".into(),
            eval_id: "app/check".into(),
            eval_def_hash: key.eval_def_hash.clone(),
            completed_at: Some(completed.clone()),
        },
        started_at: "2026-10-04T00:00:00Z".into(),
        completed_at: Some(completed),
        producer: Some(Producer {
            name: "bob@laptop".into(),
            version: "0.5.0".into(),
        }),
        reviewer: None,
        origin: None,
        manifest: None,
    };
    let mut record = Record::new(&produced, false).unwrap();
    assert_eq!(record.options.model.as_deref(), Some("model-b"));
    record.publisher = Some("bob-laptop".into());
    record.published_at = Some("2026-10-04T00:00:02Z".into());
    let receipts = Receipts::open(&state, &repo).await.unwrap();
    receipts
        .mirror_execution(&record.mirror("https://reviews.example/").unwrap())
        .await
        .unwrap();

    // The declared profile (another backend, model, reasoning and timeout) reuses it.
    let reused = verify(&repo, &state).await;
    assert_eq!(reused.run.status, "GREEN");
    assert_eq!(reused.run.executions_started, 0);
    let request = &reused.requests[0];
    assert_eq!(request.profile, fast);
    assert_eq!(request.requested_profile["model"], declared["model"]);
    assert_eq!(request.requested_profile["backend"], declared["backend"]);
    assert_eq!(request.options.variant.as_deref(), Some("fast"));
    assert_eq!(request.options.backend.as_deref(), Some("anthropic"));
    assert_eq!(request.options.max_tokens, Some(500));
    let shown = cli(&state, &["cache", "show", &key.value]);
    assert_eq!(
        shown["options"],
        json!({"backend":"anthropic","model":"model-b","reasoning":"low","maxTokens":500,"variant":"fast"})
    );

    // A model, reasoning or limit change alone keeps reusing it.
    declaration["evals"][0]["profile"] = json!({"kind":"agent","backend":"openai","model":"model-c","maxToolCalls":3,"timeoutMs":1000});
    fs::write(&path, declaration.to_string()).unwrap();
    let again = verify(&repo, &state).await;
    assert_eq!(again.run.executions_started, 0);
    assert_eq!(again.requests[0].key.as_deref(), Some(key.value.as_str()));
    assert_eq!(again.requests[0].execution_id, request.execution_id);
}
