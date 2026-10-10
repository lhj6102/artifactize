use std::{fs, path::PathBuf, process::Command, time::Duration};

use artifactize::{
    cache,
    config::{ProfileKind, parse_declaration, read_workspace_config},
    graph::{EvalStatus, Evidence, Graph},
    monitor::{self, Target},
    project::{self, VerifyOptions, selection::Selection},
    runtime::Verdict,
    store,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

mod support;

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

fn parse(value: &Value) -> Result<artifactize::config::ArtifactDeclaration, String> {
    parse_declaration(
        &support::declaration::to_toml(value.clone())
            .expect("Test builder must be TOML-compatible; use raw TOML for invalid syntax."),
    )
}

fn dependency(targets: &[&str]) -> Value {
    json!({
        "id":"ready",
        "title":"Inputs are ready",
        "profile":{"kind":"dependency","depends_on":targets},
    })
}

fn runtime(command: &str) -> Value {
    json!({
        "id":"check",
        "title":"Check input",
        "profile":{"kind":"runtime","command":support::os::bin(command),"args":[]},
        "payload":{"instruction":"Check the input."},
    })
}

impl Fixture {
    fn new() -> Self {
        let root = support::os::tempdir();
        Self {
            repo: root.path().join("repo"),
            state: root.path().join("state"),
            _root: root,
        }
    }

    fn declare(&self, name: &str, evals: Vec<Value>) {
        self.write(name, json!({"name":name,"fingerprint":{},"evals":evals}));
    }

    fn write(&self, name: &str, value: Value) {
        let folder = self.repo.join(name);
        fs::create_dir_all(&folder).unwrap();
        support::declaration::write(folder.join("index.artf"), value.to_string()).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .env("ARTIFACTIZE_REMOTE", "off")
            .env("ARTIFACTIZE_STATE_HOME", self.state.with_extension("home"))
            .args(["--repo"])
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state);
        command
    }

    fn config(&self) -> artifactize::config::RepoConfig {
        read_workspace_config(&self.repo).unwrap()
    }

    fn selection() -> Selection {
        Selection::Eval {
            eval_id: "player/ready".parse().unwrap(),
        }
    }

    async fn verify(&self, options: &VerifyOptions) -> store::RunView {
        project::verify(
            &self.repo,
            Some(&self.state),
            &Self::selection(),
            options,
            CancellationToken::new(),
        )
        .await
        .unwrap()
    }

    async fn status(&self, options: &VerifyOptions) -> project::StatusView {
        project::status(
            &self.repo,
            Some(&self.state),
            &Self::selection(),
            options,
            CancellationToken::new(),
        )
        .await
        .unwrap()
    }
}

#[test]
fn dependency_declarations_are_strict_and_other_profiles_still_require_payload() {
    let valid = json!({"name":"player","evals":[dependency(&["movement", "art"])]});
    let declaration = parse(&valid).unwrap();
    assert!(declaration.evals[0].payload().is_none());
    for targets in [
        json!([]),
        json!(["art", "art"]),
        json!([""]),
        json!(["bad/name"]),
        json!(vec!["art"; 65]),
    ] {
        let mut invalid = valid.clone();
        invalid["evals"][0]["profile"]["depends_on"] = targets;
        assert!(parse(&invalid).is_err(), "{invalid}");
    }
    let mut missing = valid.clone();
    missing["evals"][0]["profile"]
        .as_object_mut()
        .unwrap()
        .remove("depends_on");
    assert!(parse(&missing).is_err());
    for (field, value) in [
        ("payload", json!({"instruction":"No."})),
        ("pass_schema", json!({})),
        ("fail_schema", json!({})),
        ("profile_variants", json!({})),
    ] {
        let mut invalid = valid.clone();
        invalid["evals"][0][field] = value;
        assert!(parse(&invalid).is_err(), "{invalid}");
    }
    for profile in [
        json!({"kind":"human"}),
        json!({"kind":"agent","backend":"openai","model":"offline"}),
        json!({"kind":"runtime","command":"missing","args":[]}),
    ] {
        let invalid =
            json!({"name":"player","evals":[{"id":"check","title":"Check","profile":profile}]});
        assert!(parse(&invalid).is_err());
    }
    let mut max = valid;
    max["evals"][0]["profile"]["depends_on"] =
        json!((0..64).map(|i| format!("a{i}")).collect::<Vec<_>>());
    assert!(parse(&max).is_ok());
}

#[test]
fn dependency_targets_resolve_mount_aliases_and_reject_unknown_self_and_duplicate_targets() {
    let fixture = Fixture::new();
    fixture.declare("art", vec![runtime("/bin/true")]);
    fixture.write(
        "player",
        json!({"name":"player","mounts":{"sprite":"art"},"evals":[dependency(&["sprite"])]}),
    );
    let config = fixture.config();
    let relation = config
        .relations
        .iter()
        .find(|relation| {
            matches!(
                relation.kind,
                artifactize::scope::RelationKind::Dependency { .. }
            )
        })
        .unwrap();
    assert_eq!(
        serde_json::to_value(relation).unwrap(),
        json!({
            "source":"art",
            "target":"player",
            "kind":"dependency",
            "evalId":"player/ready",
            "name":"sprite",
        })
    );
    for (targets, error) in [
        (vec!["missing"], "missing"),
        (vec!["player"], "own Artifact"),
        (vec!["sprite", "art"], "unique Artifacts"),
    ] {
        fixture.write(
            "player",
            json!({"name":"player","mounts":{"sprite":"art"},"evals":[dependency(&targets)]}),
        );
        assert!(
            read_workspace_config(&fixture.repo)
                .unwrap_err()
                .to_string()
                .contains(error)
        );
    }
}

#[test]
fn dependency_verdicts_are_derived_even_when_gates_are_ignored() {
    let fixture = Fixture::new();
    fixture.declare("art", vec![runtime("/bin/true")]);
    fixture.declare("movement", vec![runtime("/bin/true")]);
    fixture.declare("player", vec![dependency(&["art", "movement"])]);
    let config = fixture.config();
    let graph = Graph::new(&config).unwrap();
    for ignore in [false, true] {
        for (evidence, status) in [
            (None, EvalStatus::Wait),
            (Some(Evidence::Stale), EvalStatus::Wait),
            (Some(Evidence::OperationalError), EvalStatus::Wait),
            (Some(Evidence::Current(Verdict::Red)), EvalStatus::Blocked),
            (Some(Evidence::Current(Verdict::Green)), EvalStatus::Green),
        ] {
            let mut evidence_map = std::collections::BTreeMap::from([
                (
                    "movement/check".parse().unwrap(),
                    Evidence::Current(Verdict::Green),
                ),
                (
                    "player/ready".parse().unwrap(),
                    Evidence::Current(Verdict::Green),
                ),
            ]);
            if let Some(evidence) = evidence {
                evidence_map.insert("art/check".parse().unwrap(), evidence);
            }
            let result = graph.evaluate_with_policy(&evidence_map, ignore);
            let derived = &result.evals["player/ready"];
            assert_eq!(derived.status, status);
            assert!(!derived.can_execute());
            assert_eq!(
                derived
                    .blocked_by
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
                if status == EvalStatus::Green {
                    vec![]
                } else {
                    vec!["art", "art/check"]
                }
            );
        }
    }
}

#[test]
fn basis_and_other_targets_without_evals_fulfill_dependency_verdict() {
    let fixture = Fixture::new();
    fixture.declare("empty", vec![]);
    fixture.write("basis", json!({"name":"basis","basis":true}));
    fixture.declare("player", vec![dependency(&["basis", "empty"])]);
    let config = fixture.config();
    let graph = Graph::new(&config).unwrap();
    assert_eq!(
        graph.evaluate(&Default::default()).evals["player/ready"].status,
        EvalStatus::Green
    );
}

#[test]
fn dependency_cycles_fail_config_check_but_ordinary_sccs_keep_working() {
    let fixture = Fixture::new();
    for (name, target) in [("a", "b"), ("b", "c"), ("c", "a")] {
        fixture.declare(name, vec![dependency(&[target])]);
    }
    let output = fixture
        .command()
        .args(["config", "check", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        output["error"]
            .as_str()
            .unwrap()
            .contains("Dependency Eval cycle: a/ready, b/ready, c/ready.")
    );
    for (name, target) in [("a", "b"), ("b", "c"), ("c", "a")] {
        let mut eval = runtime("/bin/true");
        eval["payload"]["instruction"] = json!(format!("Check {{{target}}}."));
        fixture.declare(name, vec![eval]);
    }
    let config = fixture.config();
    assert_eq!(Graph::new(&config).unwrap().components().len(), 1);
}

#[test]
fn selecting_dependency_eval_includes_required_evals_and_their_graph_closure() {
    let fixture = Fixture::new();
    let mut movement = runtime("/bin/true");
    movement["payload"]["instruction"] = json!("Check {source}.");
    fixture.declare("source", vec![runtime("/bin/true")]);
    fixture.declare("movement", vec![movement]);
    fixture.declare("other", vec![runtime("/bin/false")]);
    fixture.declare(
        "player",
        vec![dependency(&["movement"]), runtime("/bin/false")],
    );
    let config = fixture.config();
    let included: Vec<_> = Fixture::selection()
        .included_evals(&config, false)
        .unwrap()
        .iter()
        .map(|eval| eval.id.as_str())
        .collect();
    assert_eq!(
        included,
        vec!["player/ready", "movement/check", "source/check"]
    );
    let selection = Fixture::selection();
    let view = artifactize::query::graph(&config, &selection).unwrap();
    assert_eq!(
        view.artifacts.keys().copied().collect::<Vec<_>>(),
        vec!["movement", "player", "source"]
    );
    assert!(view.relations.iter().any(|relation| matches!(
        relation.relation.kind,
        artifactize::scope::RelationKind::Dependency { .. }
    )));
}

#[tokio::test]
async fn dependency_verify_is_a_derived_request_never_an_execution_or_cache_entry() {
    let fixture = Fixture::new();
    fixture.declare("art", vec![runtime("/bin/true")]);
    fixture.declare("player", vec![dependency(&["art"])]);
    let options = VerifyOptions::default();
    let before = fixture.status(&options).await;
    let ready = before
        .evals
        .iter()
        .find(|eval| eval.id == "player/ready")
        .unwrap();
    assert_eq!(ready.state, project::EvalCondition::WaitDependency);
    assert_eq!(ready.action, project::VerifyAction::Derive);
    assert_eq!(
        ready
            .blocked_by
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["art", "art/check"]
    );
    assert!(ready.key.is_none());
    let first = fixture.verify(&options).await;
    assert_eq!(first.run.status.as_str(), "GREEN");
    assert_eq!(first.run.executions_started, 1);
    let derived = first
        .requests
        .iter()
        .find(|request| request.eval_id == "player/ready")
        .unwrap();
    assert_eq!(derived.status.as_str(), "GREEN");
    assert!(derived.execution_id.is_none());
    assert!(derived.key.is_none());
    assert!(derived.started_at.is_none());
    assert!(derived.run_dir.is_none());
    assert!(derived.argv.is_none());
    assert!(derived.provenance.is_none());
    assert!(derived.blocked_by.is_empty());
    assert!(!artifactize::query::reused(derived));
    let output = artifactize::query::run_output(&first, OffsetDateTime::now_utc());
    assert_eq!(output["requests"][0]["source"]["kind"], "derived");
    assert_eq!(output["summary"]["executed"]["total"], 1);
    let db = Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM executions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let config = fixture.config();
    assert_eq!(
        cache::eval_key(&config, &config.evals[1], &Default::default()),
        Err(cache::Unkeyed::Derived)
    );
    let after = fixture.status(&options).await;
    assert!(after.satisfied);
    assert_eq!(
        after
            .evals
            .iter()
            .find(|eval| eval.id == "player/ready")
            .unwrap()
            .state,
        project::EvalCondition::Pass
    );
    let reuse = VerifyOptions {
        reuse_only: std::collections::BTreeSet::from([
            ProfileKind::Runtime,
            ProfileKind::Dependency,
        ]),
        force: true,
        ..options
    };
    let second = fixture.verify(&reuse).await;
    assert_eq!(second.run.status.as_str(), "GREEN");
    assert_eq!(second.run.executions_started, 0);
    assert!(!second.requests[0].force);
    assert_eq!(second.requests[0].status.as_str(), "GREEN");
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM executions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let text = fixture
        .command()
        .args(["verify", "player/ready"])
        .output()
        .unwrap();
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).contains("(derived)"));
}

#[tokio::test]
async fn dependency_red_and_missing_reuse_evidence_show_blocked_artifacts_and_evals() {
    for (command, reuse_only, expected) in [
        ("/bin/false", false, "BLOCKED"),
        ("/bin/true", true, "WAIT_DEPENDENCY"),
    ] {
        let fixture = Fixture::new();
        fixture.declare("art", vec![runtime(command)]);
        fixture.declare("player", vec![dependency(&["art"])]);
        let options = VerifyOptions {
            ignore_gates: Some(true),
            reuse_only: if reuse_only {
                std::collections::BTreeSet::from([ProfileKind::Runtime])
            } else {
                Default::default()
            },
            ..Default::default()
        };
        let run = fixture.verify(&options).await;
        let request = &run.requests[0];
        assert_eq!(request.status.as_str(), expected);
        assert_eq!(
            request
                .blocked_by
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["art", "art/check"]
        );
        assert!(request.execution_id.is_none());
        let output = artifactize::query::run_output(&run, OffsetDateTime::now_utc());
        assert_eq!(
            output["requests"][0]["blockedBy"],
            json!(["art", "art/check"])
        );
        let text = fixture
            .command()
            .args(["status", "player/ready", "--json"])
            .output()
            .unwrap();
        let status: Value = serde_json::from_slice(&text.stdout).unwrap();
        let derived = status["evals"]
            .as_array()
            .unwrap_or_else(|| panic!("{status}"))
            .iter()
            .find(|eval| eval["id"] == "player/ready")
            .unwrap();
        assert_eq!(derived["blockedBy"], json!(["art", "art/check"]));
        let requests = store::read_requests(&fixture.state, Some(run.run.id.as_str()))
            .await
            .unwrap();
        let detail = monitor::detail(
            &run,
            &requests,
            &Target::Eval("player/ready".parse().unwrap()),
            OffsetDateTime::now_utc(),
        );
        assert_eq!(detail.field("Source"), Some("derived (no execution)"));
        assert_eq!(detail.field("Blocked by"), Some("art, art/check"));
        let progress = monitor::progress(&run, &requests, OffsetDateTime::now_utc());
        assert!(progress.work.contains("derived 1"));
        let tree = monitor::tree(&run, &requests, OffsetDateTime::now_utc());
        let player = tree.iter().find(|node| node.id == "a:player").unwrap();
        assert!(
            player
                .children
                .iter()
                .any(|node| node.id == "e:player/ready" && node.marks.contains("[dep]"))
        );
    }
}

#[tokio::test]
async fn dependency_waits_for_human_and_never_offers_its_own_human_request() {
    let fixture = Fixture::new();
    fixture.declare(
        "art",
        vec![json!({
            "id":"approve",
            "title":"Approve art",
            "profile":{"kind":"human"},
            "payload":{"instruction":"Inspect art."},
        })],
    );
    fixture.declare("player", vec![dependency(&["art"])]);
    let options = VerifyOptions {
        wait_timeout: Duration::from_millis(10),
        ..Default::default()
    };
    let run = fixture.verify(&options).await;
    assert_eq!(run.run.status.as_str(), "INCOMPLETE");
    assert_eq!(run.requests[0].status.as_str(), "WAIT_DEPENDENCY");
    assert_eq!(
        run.requests[0]
            .blocked_by
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["art", "art/approve"]
    );
    assert_eq!(run.requests[1].status.as_str(), "WAITING_HUMAN");
    let waiting = store::read_waiting(&fixture.state, None).await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].request.eval_id, "art/approve");
    let review = fixture
        .command()
        .args(["review", run.requests[0].id.as_str(), "--json"])
        .output()
        .unwrap();
    assert!(!review.status.success());
    let receipts = store::Receipts::open(&fixture.state, &fixture.repo)
        .await
        .unwrap();
    assert!(
        artifactize::human::claim(
            &receipts,
            run.requests[0].id.as_str(),
            &"tester".parse().unwrap()
        )
        .await
        .is_err()
    );
    artifactize::human::claim(
        &receipts,
        run.requests[1].id.as_str(),
        &"tester".parse().unwrap(),
    )
    .await
    .unwrap();
    artifactize::human::submit(
        &receipts,
        run.requests[1].id.as_str(),
        &"tester".parse().unwrap(),
        &json!({"verdict":"GREEN"}),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let current = fixture.status(&VerifyOptions::default()).await;
    assert_eq!(
        current
            .evals
            .iter()
            .find(|eval| eval.id == "player/ready")
            .unwrap()
            .state,
        project::EvalCondition::Pass
    );
    let next = fixture.verify(&VerifyOptions::default()).await;
    assert_eq!(next.requests[0].status.as_str(), "GREEN");
    // The original Run is historical; it is not rewritten by the later derivation.
    assert_eq!(
        store::read_run(&fixture.state, &run.run.id)
            .await
            .unwrap()
            .requests[0]
            .status
            .as_str(),
        "WAIT_DEPENDENCY"
    );
}

#[tokio::test]
async fn dependency_chains_derive_without_job_or_execution_budget() {
    let fixture = Fixture::new();
    fixture.write("basis", json!({"name":"basis","basis":true}));
    fixture.declare("mid", vec![dependency(&["basis"])]);
    fixture.declare("player", vec![dependency(&["mid"])]);
    let options = VerifyOptions {
        jobs: 1,
        max_executions: Some(0),
        reuse_only: std::collections::BTreeSet::from([ProfileKind::Dependency]),
        ..Default::default()
    };
    let run = fixture.verify(&options).await;
    assert_eq!(run.run.status.as_str(), "GREEN");
    assert_eq!(run.run.executions_started, 0);
    assert_eq!(run.requests.len(), 2);
    assert!(
        run.requests
            .iter()
            .all(|request| request.execution_id.is_none() && request.status.as_str() == "GREEN")
    );
}

#[tokio::test]
async fn dependency_only_run_has_no_fingerprint_process_or_team_store_calls() {
    let fixture = Fixture::new();
    fixture.write(
        "basis",
        json!({
            "name":"basis",
            "basis":true,
            "fingerprint":{"script":{"command":"missing-fingerprint-command","args":[]}},
        }),
    );
    fixture.write(
        "player",
        json!({
            "name":"player",
            "fingerprint":{"script":{"command":"missing-fingerprint-command","args":[]}},
            "evals":[dependency(&["basis"])],
        }),
    );
    let remote =
        support::FakeProvider::start(|_| panic!("dependency-only Run contacted the team store"));
    let output = fixture
        .command()
        .env("ARTIFACTIZE_REMOTE", &remote.url)
        .env("ARTIFACTIZE_REMOTE_TOKEN", "azt_offline-fixture")
        .args([
            "verify",
            "player/ready",
            "--reuse-only",
            "dependency",
            "--force",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let run: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(run["summary"]["derived"], 1);
    assert_eq!(run["summary"]["executorStarts"], 0);
    assert_eq!(run["requests"][0]["source"]["kind"], "derived");
    let status = fixture
        .command()
        .env("ARTIFACTIZE_REMOTE", &remote.url)
        .env("ARTIFACTIZE_REMOTE_TOKEN", "azt_offline-fixture")
        .args(["status", "player/ready", "--json"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(remote.requests().is_empty());
    let db = Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM executions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn dependency_operational_error_and_cancellation_never_turn_into_a_red_verdict() {
    let fixture = Fixture::new();
    fixture.declare("art", vec![runtime("missing-runtime-command")]);
    fixture.declare("player", vec![dependency(&["art"])]);
    let run = fixture.verify(&VerifyOptions::default()).await;
    assert_eq!(run.run.status.as_str(), "ERROR");
    assert_eq!(run.requests[0].status.as_str(), "WAIT_DEPENDENCY");
    assert_eq!(
        run.requests[0]
            .blocked_by
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["art", "art/check"]
    );
    assert_eq!(run.requests[1].status.as_str(), "ERROR");
    let config = fixture.config();
    let graph = Graph::new(&config).unwrap();
    let cancelled = std::collections::BTreeMap::from([(
        "art/check".parse().unwrap(),
        Evidence::OperationalError,
    )]);
    assert_eq!(
        graph.evaluate(&cancelled).evals["player/ready"].status,
        EvalStatus::Wait
    );
    let mut slow = runtime("/bin/sh");
    slow["profile"]["args"] = json!(["-c", "touch started; sleep 30"]);
    fixture.write(
        "art",
        json!({"name":"art","fingerprint":false,"evals":[slow]}),
    );
    let token = CancellationToken::new();
    let cancel = token.clone();
    let started = fixture.repo.join("art/started");
    let cancellation = async move {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !started.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime started");
        cancel.cancel();
    };
    let selection = Fixture::selection();
    let options = VerifyOptions::default();
    let (run, ()) = tokio::join!(
        project::verify(
            &fixture.repo,
            Some(&fixture.state),
            &selection,
            &options,
            token
        ),
        cancellation
    );
    let run = run.unwrap();
    assert_eq!(run.requests[1].error_code.as_deref(), Some("CANCELLED"));
    assert_eq!(run.requests[0].status.as_str(), "WAIT_DEPENDENCY");
    assert_eq!(
        run.requests[0]
            .blocked_by
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["art", "art/check"]
    );
}

#[tokio::test]
async fn root_ignore_policy_preserves_dependency_verdict_and_final_obligations() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.repo).unwrap();
    support::declaration::write(
        fixture.repo.join("index.artf"),
        json!({"name":"root","basis":true,"review_policy":{"dependency_gates":"ignore"}})
            .to_string(),
    )
    .unwrap();
    fixture.declare("art", vec![runtime("/bin/false")]);
    fixture.declare("player", vec![dependency(&["art"])]);
    let run = fixture.verify(&VerifyOptions::default()).await;
    assert!(run.run.ignore_gates);
    assert_eq!(run.requests[0].status.as_str(), "BLOCKED");
    assert_eq!(
        serde_json::to_value(&run.run.validation).unwrap()["satisfied"],
        false
    );
    assert!(
        serde_json::to_value(&run.run.validation).unwrap()["obligations"]
            .as_array()
            .unwrap()
            .contains(&json!("player"))
    );
    let text = fixture
        .command()
        .args(["status", "player/ready"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("player/ready: BLOCKED — derive"));
    assert!(text.contains("art, art/check"));
}

#[test]
fn dependency_eval_inside_an_ordinary_scc_uses_current_peer_evidence() {
    let fixture = Fixture::new();
    let mut art = runtime("/bin/true");
    art["payload"]["instruction"] = json!("Check {player}.");
    fixture.declare("art", vec![art]);
    fixture.declare("player", vec![dependency(&["art"])]);
    let config = fixture.config();
    let graph = Graph::new(&config).unwrap();
    assert_eq!(graph.components().len(), 1);
    let evidence = std::collections::BTreeMap::from([(
        "art/check".parse().unwrap(),
        Evidence::Current(Verdict::Green),
    )]);
    assert_eq!(
        graph.evaluate(&evidence).evals["player/ready"].status,
        EvalStatus::Green
    );
}

#[test]
fn dependency_only_transitive_basis_inputs_never_run_fingerprints() {
    let fixture = Fixture::new();
    fixture.write(
        "leaf",
        json!({
            "name":"leaf",
            "basis":true,
            "fingerprint":{"script":{"command":"missing-fingerprint-command","args":[]}},
        }),
    );
    fixture.write(
        "basis",
        json!({"name":"basis","basis":true,"mounts":{"leaf":"leaf"}}),
    );
    fixture.declare("player", vec![dependency(&["basis"])]);
    for command in ["verify", "status"] {
        let output = fixture
            .command()
            .args([command, "player/ready", "--json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    // An ordinary consumer still needs the dependency Artifact fingerprint for its key.
    fixture.write(
        "player",
        json!({
            "name":"player",
            "mounts":{"basis":"basis"},
            "evals":[dependency(&["basis"]), runtime("/bin/true")],
        }),
    );
    let output = fixture
        .command()
        .args(["verify", "player", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let view: Value = serde_json::from_slice(&output.stdout).unwrap();
    let ordinary = view["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|request| request["evalId"] == "player/check")
        .unwrap();
    assert!(ordinary["key"].is_string());
    assert!(ordinary["fingerprints"]["basis"].is_string());
}

#[test]
fn named_profile_skips_derived_evals_but_still_validates_ordinary_variants() {
    let fixture = Fixture::new();
    let mut eval = runtime("/bin/false");
    eval["profile_variants"] =
        json!({"fast":{"kind":"runtime","command":support::os::bin("/bin/true"),"args":[]}});
    fixture.declare("art", vec![eval]);
    for derived in [false, true] {
        if derived {
            fixture.declare("player", vec![dependency(&["art"])]);
        }
        let output = fixture
            .command()
            .args(["verify", "--all", "--profile", "fast", "--json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "derived={derived}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let run: Value = serde_json::from_slice(&output.stdout).unwrap();
        let ordinary = run["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|request| request["evalId"] == "art/check")
            .unwrap();
        assert_eq!(ordinary["options"]["variant"], "fast");
        for command in ["verify", "status"] {
            let output = fixture
                .command()
                .args([command, "--all", "--profile", "missing", "--json"])
                .output()
                .unwrap();
            assert!(!output.status.success());
            let error: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(
                error["error"]
                    .as_str()
                    .unwrap()
                    .contains("Unknown profile variant for art/check: missing")
            );
        }
        let status = fixture
            .command()
            .args(["status", "--all", "--profile", "fast", "--json"])
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stdout)
        );
        if derived {
            let output = fixture
                .command()
                .args(["verify", "player/ready", "--profile", "fast", "--json"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            let output = fixture
                .command()
                .args(["status", "player/ready", "--profile", "fast", "--json"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
    let mapping =
        project::selection::ProfileSelection::Evals(std::collections::BTreeMap::from([(
            "player/ready".parse().unwrap(),
            "fast".parse().unwrap(),
        )]));
    let error = project::selection::select_profiles(
        fixture.config(),
        &Selection::All,
        Some(&mapping),
        false,
    )
    .unwrap_err();
    assert_eq!(
        error,
        "Dependency Eval player/ready cannot select a profile variant."
    );
}
