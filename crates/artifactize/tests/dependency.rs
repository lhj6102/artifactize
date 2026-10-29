//! Runtime references are Eval dependencies, never Artifact-wide execution gates, and
//! constituents are no dependencies at all.
use std::{collections::BTreeMap, fs, path::PathBuf};

use artifactize_core::{
    config::{ProfileKind, read_workspace_config},
    graph::{ArtifactStatus, EvalStatus, Evidence, Graph, Readiness},
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
        self.declare_at(name, name, evals);
    }

    fn declare_at(&self, path: &str, name: &str, evals: Vec<Value>) {
        let folder = self.repo.join(path);
        fs::create_dir_all(&folder).unwrap();
        support::declaration::write(
            folder.join("index.artf"),
            json!({"name":name,"evals":evals}).to_string(),
        )
        .unwrap();
    }

    async fn verify(&self, speculate: bool) -> store::RunView {
        self.verify_with(artifactize_core::runs::RunModes { speculate })
            .await
    }

    async fn verify_with(&self, modes: artifactize_core::runs::RunModes) -> store::RunView {
        support::runs::record(
            &self.repo,
            Some(&self.state),
            &Selection::All,
            &VerifyOptions {
                modes,
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

fn human(id: &str, instruction: &str) -> Value {
    json!({
        "id":id,"title":"Sign off",
        "profile":{"kind":"human"},
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

/// `app::lib`, nested in `app`'s folder, is a constituent of `app`; `consumer` references
/// `app`. With `cycle`, the constituent references its parent back.
fn parent_with_child(fixture: &Fixture, cycle: bool, child_green: bool) {
    fixture.declare("app", vec![runtime("check", "Inspect the app.", true)]);
    let instruction = if cycle {
        "Check with {app}."
    } else {
        "Inspect the library."
    };
    fixture.declare_at(
        "app/lib",
        "app::lib",
        vec![runtime("check", instruction, child_green)],
    );
    fixture.declare("consumer", vec![runtime("check", "Check {app}.", true)]);
}

#[test]
fn a_red_constituent_blocks_no_eval_but_keeps_its_parent_from_passing() {
    let fixture = Fixture::new();
    parent_with_child(&fixture, false, true);
    let config = read_workspace_config(&fixture.repo).unwrap();
    let graph = Graph::new(&config).unwrap();
    // A constituent is no input: the parent's eval does not wait for it.
    let waiting = graph.evaluate(&BTreeMap::new());
    assert!(waiting.evals["app/check"].can_execute());
    assert!(waiting.evals["app::lib/check"].can_execute());
    assert_eq!(waiting.evals["consumer/check"].unmet_gates, ["app/check"]);
    let evaluation = graph.evaluate(&BTreeMap::from([
        (
            "app/check".parse().unwrap(),
            Evidence::Current(Verdict::Green),
        ),
        (
            "app::lib/check".parse().unwrap(),
            Evidence::Current(Verdict::Red),
        ),
    ]));
    assert!(evaluation.evals["consumer/check"].can_execute());
    assert_eq!(evaluation.evals["app/check"].status, EvalStatus::Green);
    assert_eq!(evaluation.artifacts["app"].status, ArtifactStatus::Red);
    assert!(!evaluation.artifacts["consumer"].satisfied);
}

#[test]
fn a_constituent_that_references_its_parent_waits_for_its_parent_evals() {
    let fixture = Fixture::new();
    parent_with_child(&fixture, true, true);
    let config = read_workspace_config(&fixture.repo).unwrap();
    let graph = Graph::new(&config).unwrap();
    let waiting = graph.evaluate(&BTreeMap::new());
    assert!(waiting.evals["app/check"].can_execute());
    assert_eq!(waiting.evals["app::lib/check"].unmet_gates, ["app/check"]);
    let evaluation = graph.evaluate(&BTreeMap::from([(
        "app::lib/check".parse().unwrap(),
        Evidence::Current(Verdict::Red),
    )]));
    assert!(evaluation.evals["app/check"].can_execute());
}

#[tokio::test]
async fn verify_and_the_saved_run_view_run_a_parent_beside_its_red_constituent() {
    for speculate in [false, true] {
        for cycle in [false, true] {
            let fixture = Fixture::new();
            parent_with_child(&fixture, cycle, false);
            let run = fixture.verify(speculate).await;
            let context = format!("speculate={speculate}, cycle={cycle}");
            assert_eq!(
                statuses(&run),
                BTreeMap::from([
                    ("app/check", "GREEN"),
                    ("app::lib/check", "RED"),
                    ("consumer/check", "GREEN"),
                ]),
                "{context}"
            );
            let requests = store::read_requests(&fixture.state, Some(run.run.id.as_str()))
                .await
                .unwrap();
            let tree = monitor::tree(&run, &requests, OffsetDateTime::now_utc());
            let parent = tree
                .iter()
                .flat_map(|node| &node.children)
                .find(|node| Target::from(&node.id) == Target::Eval("app/check".parse().unwrap()))
                .map(|node| node.kind.clone())
                .unwrap();
            assert!(
                matches!(parent, monitor::Kind::Eval(monitor::EvalView::Done(_))),
                "{context}: {parent:?}"
            );
            // The parent's row fails with its constituent and names it.
            let app = tree
                .iter()
                .find(|node| Target::from(&node.id) == Target::Artifact("app".parse().unwrap()))
                .unwrap();
            assert_eq!(app.glyph, "✗", "{context}");
            assert!(
                app.line().contains("constituents: app::lib"),
                "{context}: {}",
                app.line()
            );
        }
    }
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

/// The cycle of #261: `cli/clippy` references tui, `tui/tests` references cli, and only
/// `cli/clippy` reaches outside the cycle, to core. `cli/follows` is a sibling without
/// references; docs consumes the cycle.
fn cycle_with_siblings(fixture: &Fixture, failing: &str) {
    let green = |id: &str| id != failing;
    fixture.declare(
        "core",
        vec![runtime("check", "Check core.", green("core/check"))],
    );
    fixture.declare(
        "cli",
        vec![
            runtime("clippy", "Lint {tui} against {core}.", green("cli/clippy")),
            runtime("follows", "Check the rules.", green("cli/follows")),
        ],
    );
    fixture.declare(
        "tui",
        vec![runtime("tests", "Test with {cli}.", green("tui/tests"))],
    );
    fixture.declare(
        "docs",
        vec![runtime("check", "Describe {cli}.", green("docs/check"))],
    );
}

#[test]
fn only_a_red_outside_the_evals_own_cycle_blocks_it() {
    let fixture = Fixture::new();
    cycle_with_siblings(&fixture, "");
    let config = read_workspace_config(&fixture.repo).unwrap();
    let graph = Graph::new(&config).unwrap();
    let evaluate = |entries: &[(&str, Verdict)]| {
        graph.evaluate(
            &entries
                .iter()
                .map(|&(id, verdict)| (id.parse().unwrap(), Evidence::Current(verdict)))
                .collect(),
        )
    };

    // Waiting follows the same set: the cycle waits only for what it references outside.
    let waiting = evaluate(&[]);
    assert_eq!(waiting.evals["cli/clippy"].unmet_gates, ["core/check"]);
    assert_eq!(waiting.evals["tui/tests"].unmet_gates, ["core/check"]);
    assert!(waiting.evals["cli/follows"].can_execute());
    assert_eq!(
        waiting.evals["docs/check"].unmet_gates,
        ["cli/clippy", "cli/follows", "tui/tests"]
    );

    for failed in ["cli/follows", "tui/tests"] {
        let evaluation = evaluate(&[("core/check", Verdict::Green), (failed, Verdict::Red)]);
        for eval in ["cli/clippy", "cli/follows", "tui/tests"] {
            if eval != failed {
                assert!(evaluation.evals[eval].can_execute(), "{failed} RED: {eval}");
            }
        }
        let docs = &evaluation.evals["docs/check"];
        assert_eq!(docs.status, EvalStatus::Blocked);
        assert_eq!(
            docs.blocked_reason().unwrap(),
            format!("Dependency verdict RED: {failed}")
        );
    }

    let external = evaluate(&[("core/check", Verdict::Red)]);
    assert_eq!(external.evals["cli/clippy"].status, EvalStatus::Blocked);
    assert_eq!(external.evals["tui/tests"].status, EvalStatus::Blocked);
    assert!(external.evals["cli/follows"].can_execute());
    // A BLOCKED dependency blocks in turn, and the reason names the RED at the root.
    assert_eq!(
        external.evals["docs/check"].blocked_reason().unwrap(),
        "Dependency verdict RED: core/check"
    );
}

#[tokio::test]
async fn verify_runs_every_valid_eval_and_names_the_red_that_blocks_the_rest() {
    for speculate in [false, true] {
        for (failing, expected, reason) in [
            (
                "cli/follows",
                [
                    ("cli/clippy", "GREEN"),
                    ("cli/follows", "RED"),
                    ("core/check", "GREEN"),
                    ("docs/check", "BLOCKED"),
                    ("tui/tests", "GREEN"),
                ],
                "Dependency verdict RED: cli/follows",
            ),
            (
                "core/check",
                [
                    ("cli/clippy", "BLOCKED"),
                    ("cli/follows", "GREEN"),
                    ("core/check", "RED"),
                    ("docs/check", "BLOCKED"),
                    ("tui/tests", "BLOCKED"),
                ],
                "Dependency verdict RED: core/check",
            ),
        ] {
            let fixture = Fixture::new();
            cycle_with_siblings(&fixture, failing);
            let run = fixture.verify(speculate).await;
            let context = format!("speculate={speculate}, {failing} RED");
            assert_eq!(statuses(&run), BTreeMap::from(expected), "{context}");
            for request in &run.requests {
                if request.status().as_str() == "BLOCKED" {
                    assert_eq!(
                        request.blocked_reason.as_deref(),
                        Some(reason),
                        "{context}: {}",
                        request.eval_id
                    );
                }
            }
            // The saved Run view derives the same blocks from the same gates.
            let requests = store::read_requests(&fixture.state, Some(run.run.id.as_str()))
                .await
                .unwrap();
            let tree = monitor::tree(&run, &requests, OffsetDateTime::now_utc());
            let view = |eval: &str| {
                tree.iter()
                    .flat_map(|node| &node.children)
                    .find(|node| Target::from(&node.id) == Target::Eval(eval.parse().unwrap()))
                    .map(|node| node.kind.clone())
                    .unwrap()
            };
            for (eval, status) in expected {
                let kind = view(eval);
                match status {
                    "GREEN" => assert!(
                        matches!(kind, monitor::Kind::Eval(monitor::EvalView::Done(_))),
                        "{context}: {eval} {kind:?}"
                    ),
                    "BLOCKED" => assert!(
                        matches!(kind, monitor::Kind::Eval(monitor::EvalView::BlockedBy(_))),
                        "{context}: {eval} {kind:?}"
                    ),
                    _ => {}
                }
            }
        }
    }
}

/// `app` has a machine eval and two sign-offs; `consumer` references `app`.
fn signoffs(fixture: &Fixture, check_green: bool) {
    fixture.declare(
        "app",
        vec![
            runtime("check", "Check the app.", check_green),
            human("approve", "Approve the app."),
            human("sign", "Sign off the app."),
        ],
    );
    fixture.declare("consumer", vec![runtime("check", "Check {app}.", true)]);
}

#[test]
fn a_human_eval_waits_for_its_artifacts_machine_evals_and_is_blocked_by_their_failure() {
    let fixture = Fixture::new();
    signoffs(&fixture, true);
    let config = read_workspace_config(&fixture.repo).unwrap();
    let graph = Graph::new(&config).unwrap();
    // Only the machine sibling gates a sign-off; the other sign-off never does.
    let waiting = graph.evaluate(&BTreeMap::new());
    for eval in ["app/approve", "app/sign"] {
        assert_eq!(waiting.evals[eval].unmet_gates, ["app/check"], "{eval}");
    }
    assert!(waiting.evals["app/check"].can_execute());
    let green = graph.evaluate(&BTreeMap::from([(
        "app/check".parse().unwrap(),
        Evidence::Current(Verdict::Green),
    )]));
    for eval in ["app/approve", "app/sign"] {
        assert!(green.evals[eval].can_execute(), "{eval}");
    }
    // A RED or ERROR sibling blocks it, even where an ERROR dependency only waits.
    for (evidence, reason) in [
        (
            Evidence::Current(Verdict::Red),
            "Sibling machine eval RED: app/check",
        ),
        (
            Evidence::OperationalError,
            "Sibling machine eval ERROR: app/check",
        ),
    ] {
        let failed = graph.evaluate(&BTreeMap::from([("app/check".parse().unwrap(), evidence)]));
        for eval in ["app/approve", "app/sign"] {
            assert_eq!(failed.evals[eval].status, EvalStatus::Blocked, "{eval}");
            assert_eq!(failed.evals[eval].blocked_reason().unwrap(), reason);
        }
    }
}

#[tokio::test]
async fn every_verify_mode_blocks_a_signoff_whose_machine_sibling_is_red() {
    for speculate in [false, true] {
        let fixture = Fixture::new();
        signoffs(&fixture, false);
        let run = fixture
            .verify_with(artifactize_core::runs::RunModes { speculate })
            .await;
        let context = format!("speculate={speculate}");
        assert_eq!(
            statuses(&run),
            BTreeMap::from([
                ("app/approve", "BLOCKED"),
                ("app/check", "RED"),
                ("app/sign", "BLOCKED"),
                ("consumer/check", "BLOCKED"),
            ]),
            "{context}"
        );
        for (eval, reason) in [
            ("app/approve", "Sibling machine eval RED: app/check"),
            ("app/sign", "Sibling machine eval RED: app/check"),
            ("consumer/check", "Dependency verdict RED: app/check"),
        ] {
            let request = run
                .requests
                .iter()
                .find(|request| request.eval_id == eval)
                .unwrap();
            assert_eq!(request.blocked_reason.as_deref(), Some(reason), "{context}");
        }
        assert!(
            store::read_waiting(&fixture.state, None)
                .await
                .unwrap()
                .is_empty(),
            "{context}: no person was asked"
        );
    }
}
