use std::{fs, path::Path, process::Command, time::Duration};

use artifactize::{
    cache, human,
    project::{self, VerifyOptions, selection::Selection},
    remote::Record,
    store::Receipts,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use support::os::bin;
use tokio_util::sync::CancellationToken;

mod support;

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
    support::declaration::write(
        repo.join("index.artf"),
        json!({"name":"app","fingerprint":{"script":{"command":bin("cat"),"args":["fingerprint"]}},
            "evals":[{"id":"check","title":"Check","profile":profile,"payload":{"instruction":"Review."},
                "pass_schema":{"type":"object","properties":{"approved":{"const":true}},"required":["approved"],"additionalProperties":false}}]})
        .to_string(),
    )
    .unwrap();
}

async fn verify(repo: &Path, state: &Path) -> artifactize::store::RunView {
    project::verify(
        repo,
        Some(state),
        &Selection::All,
        // A Human request is recorded, and the wait for it times out at once.
        &VerifyOptions {
            wait_timeout: Duration::from_millis(1),
            ..Default::default()
        },
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
        json!({"kind":"runtime","command":bin("/bin/sh"),"args":["check.sh"]}),
    );
    let producer_state = root.path().join("producer");
    let produced = verify(&repo, &producer_state).await;
    assert_eq!(produced.run.status.as_str(), "GREEN");
    let key = produced.requests[0].key.clone().unwrap();
    let execution = cache::show(&producer_state, &key, false)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(execution.fingerprint.as_deref(), Some("remote-v1"));
    assert_eq!(
        execution.fingerprints,
        [("app".to_owned(), "remote-v1".parse().unwrap())].into()
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
    // A full record an earlier artifactize published still holds its tool calls and result
    // check limit; readers ignore them.
    let mut earlier = serde_json::to_value(&full).unwrap();
    earlier["execution"]["toolCalls"] = json!([{"name":"read","isError":false}]);
    earlier["execution"]["options"]["resultCheckTimeoutMs"] = json!(5000);
    let earlier: Record = serde_json::from_value(earlier).unwrap();
    assert!(
        !serde_json::to_string(&earlier)
            .unwrap()
            .contains("toolCalls")
    );
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
    assert_eq!(mirrored.id.as_str(), format!("remote-{}", execution.id));
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
    assert_eq!(reused.run.status.as_str(), "GREEN");
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
async fn maximum_wire_execution_ids_mirror_and_reuse_without_renaming() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    write_repo(
        &repo,
        json!({"kind":"runtime","command":bin("/bin/true"),"args":[]}),
    );
    let producer_state = root.path().join("producer");
    let produced = verify(&repo, &producer_state).await;
    let key = produced.requests[0].key.as_ref().unwrap();
    let original = cache::show(&producer_state, key, false)
        .await
        .unwrap()
        .pop()
        .unwrap();

    // 193 is the previous passing edge; 194 through the 200-byte wire maximum
    // need the local-only seven-byte namespace allowance for both record forms.
    for bytes in [193, 194, 200] {
        for full in [false, true] {
            let mut source = original.clone();
            source.id = "x".repeat(bytes).parse().unwrap();
            let mut wire = Record::new(&source, full).unwrap();
            wire.publisher = Some("alice-laptop".into());
            wire.published_at = Some("2026-10-04T00:00:02Z".into());
            let wire: Record = serde_json::from_value(serde_json::to_value(wire).unwrap()).unwrap();
            wire.validate().unwrap();
            let mirror = wire.mirror("https://reviews.example/").unwrap();
            assert_eq!(mirror.id.as_str(), format!("remote-{}", source.id));
            assert_eq!(mirror.id.len(), bytes + "remote-".len());
            let state = root.path().join(format!("consumer-{bytes}-{full}"));
            let receipts = Receipts::open(&state, &repo).await.unwrap();
            receipts.mirror_execution(&mirror).await.unwrap().unwrap();
            assert!(receipts.mirror_execution(&mirror).await.unwrap().is_none());
            let saved = cache::show(&state, key, false)
                .await
                .unwrap()
                .pop()
                .unwrap();
            assert_eq!(saved.id, mirror.id);
            assert_eq!(
                cache::list(&state, false).await.unwrap()[0].execution_id,
                mirror.id
            );
            let reused = verify(&repo, &state).await;
            assert_eq!(reused.run.status.as_str(), "GREEN");
            assert_eq!(reused.run.executions_started, 0);
            assert_eq!(reused.requests[0].execution_id.as_ref(), Some(&mirror.id));
        }
    }
    let mut too_long = Record::new(&original, false).unwrap();
    // This is valid only as a stored local mirror, never as a remote wire identity.
    too_long.execution_id = format!("remote-{}", "x".repeat(194)).parse().unwrap();
    assert!(too_long.validate().is_err());
}

#[tokio::test]
async fn legacy_207_byte_mirrors_remain_readable_in_json_sql_and_cache() {
    // An independent literal schema-5 fixture, not produced by the typed writer.
    let text = include_str!("fixtures/legacy_remote_execution.json");
    let execution: artifactize::store::Execution = serde_json::from_str(text).unwrap();
    assert_eq!(execution.id.len(), 207);
    let state = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    Receipts::open(state.path(), repo.path()).await.unwrap();
    let db = Connection::open(state.path().join("state.sqlite")).unwrap();
    db.execute(
        "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,completed_at,bytes,last_used,data) VALUES(?1,?2,?3,'GREEN',0,0,?4,?5,?4,?6)",
        rusqlite::params![
            execution.id,
            execution.key,
            execution.eval_def_hash,
            execution.completed_at,
            text.len() as i64,
            text
        ],
    )
    .unwrap();
    assert_eq!(
        cache::list(state.path(), false).await.unwrap()[0].execution_id,
        execution.id
    );
    assert_eq!(
        cache::show(state.path(), execution.key.as_ref().unwrap(), false)
            .await
            .unwrap()[0]
            .id,
        execution.id
    );
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        artifactize::store::STATE_SCHEMA_VERSION
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
    assert_eq!(request.status.as_str(), "WAITING_HUMAN");
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
    let declared = json!({"kind":"agent","backend":"openai","model":"model-a","reasoning":"high","timeout_ms":60000});
    let fast = json!({"kind":"agent","backend":"anthropic","model":"model-b","reasoning":"low","max_tokens":500});
    write_repo(&repo, declared.clone());
    let path = repo.join("index.artf");
    let mut declaration: Value = support::declaration::read(fs::read(&path).unwrap()).unwrap();
    declaration["evals"][0]["profile_variants"] = json!({ "fast": fast });
    support::declaration::write(&path, declaration.to_string()).unwrap();

    // The record the `fast` variant produced elsewhere, keyed like this repository's eval.
    let config = read_workspace_config(&repo).unwrap();
    let state = root.path().join("state");
    let fingerprints = cache::prepare(
        &config,
        ["app"],
        &state,
        &cache::Parallelism::new(2),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let key = cache::eval_keys(&config, &fingerprints)["app/check"].clone();
    let variant: Profile = serde_json::from_value(fast.clone()).unwrap();
    let completed = "2026-10-04T00:00:01Z".to_owned();
    let produced = Execution {
        id: "execution-run-elsewhere-1".parse().unwrap(),
        key: Some(key.value.clone()),
        fingerprint: Some("remote-v1".parse().unwrap()),
        fingerprints: key.fingerprints.clone(),
        artifact_kinds: key.artifact_kinds.clone(),
        eval_def_hash: key.eval_def_hash.clone(),
        owner_pid: 1,
        owner_start_time: 1,
        status: artifactize::types::ExecutionStatus::Green,
        result: Some(json!({"verdict":"GREEN","approved":true})),
        error: None,
        error_code: None,
        profile: artifactize::config::StoredProfile::from(&variant),
        options: ExecutionOptions::new(&variant, Some("fast")),
        usage: Some(
            serde_json::from_value(json!([{"turn":1,"attempt":1,"usage":{"inputTokens":10}}]))
                .unwrap(),
        ),
        provenance: Provenance {
            repo_path: "/elsewhere".into(),
            run_id: "run-elsewhere".parse().unwrap(),
            request_id: "run-elsewhere-1".parse().unwrap(),
            eval_id: "app/check".into(),
            eval_def_hash: key.eval_def_hash.clone(),
            completed_at: Some(completed.clone()),
            execution_paths: Default::default(),
        },
        started_at: "2026-10-04T00:00:00Z".into(),
        completed_at: Some(completed),
        producer: Some(Producer {
            name: "bob@laptop".into(),
            version: "0.5.0".into(),
            session: None,
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
    assert_eq!(reused.run.status.as_str(), "GREEN");
    assert_eq!(reused.run.executions_started, 0);
    let request = &reused.requests[0];
    assert_eq!(
        serde_json::to_value(&request.profile).unwrap(),
        serde_json::to_value(artifactize::config::StoredProfile::from(&variant)).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&request.requested_profile).unwrap()["model"],
        declared["model"]
    );
    assert_eq!(
        serde_json::to_value(&request.requested_profile).unwrap()["backend"],
        declared["backend"]
    );
    assert_eq!(request.options.variant.as_deref(), Some("fast"));
    assert_eq!(request.options.backend.as_deref(), Some("anthropic"));
    assert_eq!(request.options.max_tokens, Some(500));
    let shown = cli(&state, &["cache", "show", &key.value]);
    assert_eq!(
        shown["options"],
        json!({"backend":"anthropic","model":"model-b","reasoning":"low","maxTokens":500,"variant":"fast"})
    );

    // A model, reasoning or limit change alone keeps reusing it.
    declaration["evals"][0]["profile"] = json!({"kind":"agent","backend":"openai","model":"model-c","max_tool_calls":3,"timeout_ms":1000});
    support::declaration::write(&path, declaration.to_string()).unwrap();
    let again = verify(&repo, &state).await;
    assert_eq!(again.run.executions_started, 0);
    assert_eq!(again.requests[0].key.as_deref(), Some(key.value.as_str()));
    assert_eq!(again.requests[0].execution_id, request.execution_id);
}
