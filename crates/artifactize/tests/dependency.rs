//! Runtime references are Eval dependencies, never Artifact-wide execution gates.
use std::{collections::BTreeMap, fs, path::PathBuf};

use artifactize_core::{
    config::{ProfileKind, read_workspace_config},
    graph::{EvalStatus, Evidence, Graph, Readiness},
    monitor::{self, Target},
    project::{VerifyOptions, selection::Selection},
    runtime::Verdict,
    store,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

struct Fixture {
    _root: support::runs::RepoLease,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        let state = root.path().join("state");
        fs::create_dir(&repo).unwrap();
        Self {
            _root: support::runs::RepoLease::new(root, &state, &repo),
            repo,
            state,
        }
    }

    fn declare(&self, name: &str, evals: Vec<Value>) {
        let folder = self.repo.join(name);
        fs::create_dir_all(&folder).unwrap();
        support::declaration::write(
            folder.join("index.artf"),
            json!({"name":name,"evals":evals}).to_string(),
        )
        .unwrap();
    }

    async fn verify(&self, speculate: bool) -> store::RunView {
        support::runs::record(
            &self.repo,
            Some(&self.state),
            &Selection::All,
            &VerifyOptions {
                modes: artifactize_core::runs::RunModes {
                    speculate,
                    defer_human: false,
                },
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await
        .unwrap()
    }
}

fn runtime(id: &str, instruction: &str, green: bool) -> Value {
    json!({
        "id":id,"title":"Check behavior",
        "profile":{"kind":"runtime","command":if green {support::os::true_program()} else {support::os::false_program()},"args":[]},
        "payload":{"instruction":instruction},
    })
}

fn statuses(run: &store::RunView) -> BTreeMap<&str, &str> {
    run.requests
        .iter()
        .map(|request| (request.eval_id.as_str(), request.status().as_str()))
        .collect()
}

#[test]
fn instructions_and_argv_gate_only_the_eval_that_references_the_input() {
    let fixture = Fixture::new();
    fixture.declare("input", vec![runtime("check", "Inspect input.", true)]);
    let mut argv = runtime("argv", "Inspect the operand.", true);
    argv["profile"]["args"] = json!(["{input}/data"]);
    fixture.declare(
        "app",
        vec![
            runtime("instruction", "Check {input}.", true),
            argv,
            runtime("independent", "Check only this target.", true),
        ],
    );
    let config = read_workspace_config(&fixture.repo).unwrap();
    let graph = Graph::new(&config).unwrap();
    for evidence in [None, Some(Evidence::Current(Verdict::Red))] {
        let evidence = evidence
            .into_iter()
            .map(|value| ("input/check".parse().unwrap(), value))
            .collect();
        let evaluation = graph.evaluate(&evidence);
        for eval in ["app/instruction", "app/argv"] {
            assert_eq!(
                evaluation.evals[eval].readiness,
                if evidence.is_empty() {
                    Readiness::Wait
                } else {
                    Readiness::Blocked
                }
            );
            assert_eq!(evaluation.evals[eval].unmet_gates, ["input/check"]);
        }
        assert!(evaluation.evals["app/independent"].can_execute());
    }
}

#[test]
fn visible_mounts_are_not_implicit_eval_gates() {
    let fixture = Fixture::new();
    fixture.declare("input", vec![runtime("check", "Inspect input.", true)]);
    fixture.declare("app", vec![runtime("check", "Inspect only the app.", true)]);
    let declaration = fixture.repo.join("app/index.artf");
    let source = fs::read_to_string(&declaration).unwrap();
    fs::write(
        declaration,
        format!("mounts = {{ input = 'input' }}\n{source}"),
    )
    .unwrap();
    let config = read_workspace_config(&fixture.repo).unwrap();
    let graph = Graph::new(&config).unwrap();
    let evaluation = graph.evaluate(&BTreeMap::from([(
        "input/check".parse().unwrap(),
        Evidence::Current(Verdict::Red),
    )]));
    assert!(evaluation.evals["app/check"].can_execute());
}

#[tokio::test]
async fn default_verify_executes_independent_siblings_after_an_input_fails() {
    let fixture = Fixture::new();
    fixture.declare("input", vec![runtime("check", "Check input.", false)]);
    fixture.declare(
        "app",
        vec![
            runtime("dependent", "Check {input}.", true),
            runtime("independent", "Check only the app.", true),
        ],
    );
    let run = fixture.verify(false).await;
    assert_eq!(
        statuses(&run),
        BTreeMap::from([
            ("app/dependent", "BLOCKED"),
            ("app/independent", "GREEN"),
            ("input/check", "RED")
        ])
    );
    let blocked = run
        .requests
        .iter()
        .find(|request| request.eval_id == "app/dependent")
        .unwrap();
    assert!(blocked.execution_id.is_none());
    let tree = monitor::tree(
        &run,
        &store::read_requests(&fixture.state, Some(run.run.id.as_str()))
            .await
            .unwrap(),
        OffsetDateTime::now_utc(),
    );
    let independent = tree
        .iter()
        .flat_map(|node| &node.children)
        .find(|node| Target::from(&node.id) == Target::Eval("app/independent".parse().unwrap()))
        .unwrap();
    assert!(matches!(
        independent.kind,
        monitor::Kind::Eval(monitor::EvalView::Done(_))
    ));
}

#[tokio::test]
async fn cycle_peers_keep_their_real_verdicts_and_do_not_release_downstream_on_red() {
    for (speculate, green) in [(false, false), (true, false), (false, true), (true, true)] {
        let fixture = Fixture::new();
        fixture.declare("a", vec![runtime("check", "Check {b}.", green)]);
        fixture.declare("b", vec![runtime("check", "Check {a}.", true)]);
        fixture.declare("consumer", vec![runtime("check", "Check {b}.", true)]);
        let run = fixture.verify(speculate).await;
        assert_eq!(
            statuses(&run),
            BTreeMap::from([
                ("a/check", if green { "GREEN" } else { "RED" }),
                ("b/check", "GREEN"),
                ("consumer/check", if green { "GREEN" } else { "BLOCKED" })
            ]),
            "speculate={speculate}"
        );
        assert_eq!(
            run.run.status().as_str(),
            if green { "GREEN" } else { "RED" }
        );
        let requests = store::read_requests(&fixture.state, Some(run.run.id.as_str()))
            .await
            .unwrap();
        let tree = monitor::tree(&run, &requests, OffsetDateTime::now_utc());
        let peer = tree
            .iter()
            .flat_map(|node| &node.children)
            .find(|node| Target::from(&node.id) == Target::Eval("b/check".parse().unwrap()))
            .unwrap();
        assert!(matches!(
            peer.kind,
            monitor::Kind::Eval(monitor::EvalView::Done(_))
        ));
        assert!(
            run.requests
                .iter()
                .all(|request| request.profile.kind() == ProfileKind::Runtime)
        );
    }
}

#[test]
fn external_gates_still_hold_cycles_without_gating_unrelated_sibling_evals() {
    let fixture = Fixture::new();
    fixture.declare("external", vec![runtime("check", "Inspect.", true)]);
    fixture.declare(
        "a",
        vec![
            runtime("check", "Check {b} and {external}.", true),
            runtime("independent", "Inspect.", true),
        ],
    );
    fixture.declare("b", vec![runtime("check", "Check {a}.", true)]);
    let config = read_workspace_config(&fixture.repo).unwrap();
    let graph = Graph::new(&config).unwrap();
    let evaluation = graph.evaluate(&BTreeMap::from([
        (
            "external/check".parse().unwrap(),
            Evidence::Current(Verdict::Red),
        ),
        (
            "a/independent".parse().unwrap(),
            Evidence::Current(Verdict::Green),
        ),
    ]));
    assert_eq!(evaluation.evals["a/check"].status, EvalStatus::Blocked);
    assert_eq!(evaluation.evals["b/check"].status, EvalStatus::Blocked);
    assert_eq!(evaluation.evals["a/independent"].status, EvalStatus::Green);
}
