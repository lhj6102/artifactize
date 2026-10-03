use serde_json::json;

use super::*;
use crate::{
    config::{Artifact, Eval, parse_declaration, read_workspace_config},
    scope::{Relation, RelationKind},
};

fn config(definitions: &[(&str, bool, &[&str])], edges: &[(&str, &str)]) -> RepoConfig {
    let mut config = RepoConfig {
        root: "/unused".into(),
        artifacts: BTreeMap::new(),
        families: BTreeMap::new(),
        evals: Vec::new(),
        relations: edges
            .iter()
            .map(|&(source, target)| Relation {
                source: source.into(),
                target: target.into(),
                kind: RelationKind::Mount {
                    alias: source.into(),
                },
            })
            .collect(),
    };
    for &(name, basis, evals) in definitions {
        let declaration = parse_declaration(
            &json!({"name": name, "basis": basis, "evals": evals.iter().map(|id| {
                json!({"id": id, "title": "Check", "profile": {"kind": "human"},
                    "payload": {"instruction": "Inspect."}})
            }).collect::<Vec<_>>()})
            .to_string(),
        )
        .unwrap();
        for eval in declaration.evals {
            config.evals.push(Eval {
                id: format!("{name}/{}", eval.id),
                target: name.into(),
                references: BTreeMap::new(),
                deps: Vec::new(),
                declaration: eval,
            });
        }
        config.artifacts.insert(
            name.into(),
            Artifact {
                family: None,
                name: name.into(),
                basis: declaration.basis,
                path: name.into(),
                children: BTreeMap::new(),
                mounts: declaration.mounts,
                views: declaration.views,
                stale: declaration.stale,
                review_policy: declaration.review_policy,
            },
        );
    }
    config
}

fn reviewed(ids: &[&str], edges: &[(&str, &str)]) -> RepoConfig {
    let definitions: Vec<_> = ids
        .iter()
        .map(|&id| (id, false, ["check"].as_slice()))
        .collect();
    config(&definitions, edges)
}

fn evidence(entries: &[(&str, Evidence)]) -> BTreeMap<String, Evidence> {
    entries
        .iter()
        .map(|&(id, outcome)| (id.into(), outcome))
        .collect()
}

const GREEN: Evidence = Evidence::Current(Verdict::Green);
const RED: Evidence = Evidence::Current(Verdict::Red);

#[test]
fn cycle_peers_share_external_gates_but_need_individual_final_evidence() {
    let config = reviewed(
        &["a", "b", "external", "consumer", "independent"],
        &[("a", "b"), ("b", "a"), ("external", "a"), ("b", "consumer")],
    );
    let graph = Graph::new(&config).unwrap();
    let component = &graph.components()[graph.artifacts()["a"].component];
    assert_eq!(component.artifacts, ["a", "b"]);
    assert_eq!(component.gates, ["external/check"]);
    assert_eq!(
        graph.dependency_closure(&["a", "a"]).unwrap(),
        ["a", "b", "external"]
    );
    assert_eq!(
        graph.dependency_closure(&["consumer"]).unwrap(),
        ["a", "b", "consumer", "external"]
    );
    assert!(graph.dependency_closure(&["absent"]).is_err());

    let mut outcomes = BTreeMap::new();
    let waiting = graph.evaluate(&outcomes);
    assert_eq!(waiting.evals["a/check"].readiness, Readiness::Wait);
    assert_eq!(waiting.evals["b/check"].readiness, Readiness::Wait);
    assert_eq!(waiting.evals["consumer/check"].readiness, Readiness::Wait);
    assert!(waiting.evals["external/check"].can_execute());
    assert!(waiting.evals["independent/check"].can_execute());

    outcomes.insert("external/check".into(), GREEN);
    let ready = graph.evaluate(&outcomes);
    assert!(ready.evals["a/check"].can_execute());
    assert!(ready.evals["b/check"].can_execute());
    assert!(!ready.evals["external/check"].can_execute());
    assert_eq!(ready.evals["consumer/check"].readiness, Readiness::Wait);

    outcomes.insert("a/check".into(), GREEN);
    let partial = graph.evaluate(&outcomes);
    assert_eq!(partial.evals["a/check"].status, EvalStatus::Green);
    assert!(partial.evals["b/check"].can_execute());
    assert_eq!(partial.artifacts["a"].status, ArtifactStatus::Incomplete);
    assert!(partial.artifacts["a"].own_satisfied);
    assert!(!partial.artifacts["a"].satisfied);
    assert_eq!(partial.obligations, ["b", "consumer", "independent"]);
    assert_eq!(partial.status, FinalStatus::Incomplete);

    outcomes.insert("b/check".into(), GREEN);
    let released = graph.evaluate(&outcomes);
    assert!(released.evals["consumer/check"].can_execute());
    assert!(released.artifacts["a"].satisfied);
    outcomes.insert("consumer/check".into(), GREEN);
    outcomes.insert("independent/check".into(), GREEN);
    let complete = graph.evaluate(&outcomes);
    assert_eq!(complete.status, FinalStatus::Green);
    assert!(complete.obligations.is_empty());
}

#[test]
fn red_blocks_transitively_and_retry_releases_retained_evidence() {
    let config = reviewed(&["a", "b", "c"], &[("a", "b"), ("b", "c")]);
    let graph = Graph::new(&config).unwrap();
    let mut outcomes = evidence(&[("a/check", RED), ("b/check", GREEN), ("c/check", GREEN)]);
    let blocked = graph.evaluate(&outcomes);
    assert_eq!(blocked.status, FinalStatus::Red);
    for id in ["b/check", "c/check"] {
        assert_eq!(blocked.evals[id].readiness, Readiness::Blocked);
        assert_eq!(blocked.evals[id].status, EvalStatus::Blocked);
        assert_eq!(blocked.evals[id].evidence, Some(GREEN));
        assert!(!blocked.evals[id].can_execute());
    }
    assert_eq!(blocked.evals["c/check"].unmet_gates, ["b/check"]);
    assert_eq!(blocked.artifacts["c"].passed, 0);
    outcomes.insert("a/check".into(), GREEN);
    let released = graph.evaluate(&outcomes);
    assert_eq!(released.status, FinalStatus::Green);
    assert_eq!(released.evals["c/check"].status, EvalStatus::Green);
    assert!(!released.evals["c/check"].can_execute());
}

#[test]
fn operational_error_waits_without_fabricating_descendant_verdicts() {
    let config = reviewed(&["a", "b", "c"], &[("a", "b"), ("b", "c")]);
    let graph = Graph::new(&config).unwrap();
    let outcomes = evidence(&[("a/check", Evidence::OperationalError), ("b/check", GREEN)]);
    let evaluation = graph.evaluate(&outcomes);
    assert_eq!(evaluation.status, FinalStatus::Error);
    assert_eq!(evaluation.evals["a/check"].status, EvalStatus::Error);
    assert!(!evaluation.evals["a/check"].can_execute());
    for id in ["b/check", "c/check"] {
        assert_eq!(evaluation.evals[id].readiness, Readiness::Wait);
        assert_eq!(evaluation.evals[id].status, EvalStatus::Wait);
    }
    assert_eq!(evaluation.evals["b/check"].evidence, Some(GREEN));
    assert_eq!(evaluation.evals["c/check"].evidence, None);
    assert_eq!(evaluation.obligations, ["a", "b", "c"]);
}

#[test]
fn stale_and_missing_evidence_never_satisfy_a_gate() {
    let config = reviewed(&["a", "b"], &[("a", "b")]);
    let graph = Graph::new(&config).unwrap();
    let mut outcomes = evidence(&[("b/check", GREEN), ("unrelated/check", RED)]);
    let missing = graph.evaluate(&outcomes);
    assert_eq!(missing.status, FinalStatus::Incomplete);
    assert_eq!(missing.evals["a/check"].status, EvalStatus::Unreviewed);
    assert!(missing.evals["a/check"].can_execute());
    assert_eq!(missing.evals["b/check"].readiness, Readiness::Wait);
    outcomes.insert("a/check".into(), Evidence::Stale);
    let stale = graph.evaluate(&outcomes);
    assert_eq!(stale.evals["a/check"].status, EvalStatus::Stale);
    assert!(stale.evals["a/check"].can_execute());
    assert_eq!(stale.evals["b/check"].readiness, Readiness::Wait);
    assert_eq!(stale.status, FinalStatus::Incomplete);
}

#[test]
fn every_eval_is_an_external_obligation_and_red_wins_over_wait() {
    let config = config(
        &[("a", false, &["one", "two"]), ("b", false, &["check"])],
        &[("a", "b"), ("a", "b")],
    );
    let graph = Graph::new(&config).unwrap();
    let component = &graph.components()[graph.artifacts()["b"].component];
    assert_eq!(component.gates, ["a/one", "a/two"]);
    assert_eq!(component.dependencies.len(), 1);
    let mut outcomes = evidence(&[("a/one", GREEN)]);
    let partial = graph.evaluate(&outcomes);
    assert_eq!(partial.evals["b/check"].readiness, Readiness::Wait);
    assert_eq!(partial.evals["b/check"].unmet_gates, ["a/two"]);
    assert_eq!(partial.artifacts["a"].passed, 1);
    assert_eq!(partial.artifacts["a"].total, 2);
    outcomes.insert("a/one".into(), RED);
    outcomes.insert("a/two".into(), Evidence::OperationalError);
    let blocked = graph.evaluate(&outcomes);
    assert_eq!(blocked.evals["b/check"].readiness, Readiness::Blocked);
    assert_eq!(blocked.status, FinalStatus::Error);
    outcomes.insert("a/one".into(), GREEN);
    outcomes.insert("a/two".into(), GREEN);
    assert!(graph.evaluate(&outcomes).evals["b/check"].can_execute());
}

#[test]
fn cycle_peer_red_does_not_gate_its_peers() {
    let config = reviewed(&["a", "b", "c"], &[("a", "b"), ("b", "a"), ("b", "c")]);
    let graph = Graph::new(&config).unwrap();
    let evaluation = graph.evaluate(&evidence(&[("a/check", RED)]));
    assert!(evaluation.evals["b/check"].can_execute());
    assert_eq!(evaluation.evals["c/check"].readiness, Readiness::Blocked);
    assert_eq!(evaluation.artifacts["b"].status, ArtifactStatus::Unreviewed);
    assert!(!evaluation.artifacts["b"].satisfied);
}

#[test]
fn no_eval_inputs_do_not_gate_but_basis_and_unreviewed_differ() {
    let config = config(
        &[
            ("basis", true, &[]),
            ("unreviewed", false, &[]),
            ("consumer", false, &["check"]),
            ("accepted", false, &["check"]),
        ],
        &[
            ("basis", "consumer"),
            ("unreviewed", "consumer"),
            ("basis", "accepted"),
        ],
    );
    let graph = Graph::new(&config).unwrap();
    let initial = graph.evaluate(&BTreeMap::new());
    assert!(initial.evals["consumer/check"].can_execute());
    assert_eq!(initial.artifacts["basis"].status, ArtifactStatus::Basis);
    assert_eq!(
        initial.artifacts["unreviewed"].status,
        ArtifactStatus::Unreviewed
    );
    let complete = graph.evaluate(&evidence(&[
        ("consumer/check", GREEN),
        ("accepted/check", GREEN),
    ]));
    assert_eq!(complete.evals["consumer/check"].status, EvalStatus::Green);
    assert_eq!(
        complete.artifacts["consumer"].status,
        ArtifactStatus::Incomplete
    );
    assert_eq!(complete.artifacts["accepted"].status, ArtifactStatus::Green);
    assert_eq!(complete.obligations, ["unreviewed"]);
    assert_eq!(complete.status, FinalStatus::Incomplete);
}

#[test]
fn basis_does_not_erase_its_own_dependencies_or_create_transitive_gates() {
    let config = config(
        &[
            ("input", false, &["check"]),
            ("basis", true, &[]),
            ("consumer", false, &["check"]),
        ],
        &[("input", "basis"), ("basis", "consumer")],
    );
    let graph = Graph::new(&config).unwrap();
    let component = &graph.components()[graph.artifacts()["consumer"].component];
    assert!(component.gates.is_empty());
    let evaluation = graph.evaluate(&evidence(&[("consumer/check", GREEN)]));
    assert_eq!(
        evaluation.artifacts["basis"].status,
        ArtifactStatus::Incomplete
    );
    assert!(evaluation.artifacts["basis"].own_satisfied);
    assert_eq!(
        evaluation.artifacts["consumer"].status,
        ArtifactStatus::Incomplete
    );
    assert_eq!(evaluation.obligations, ["input"]);
    assert_eq!(
        graph.dependency_closure(&["consumer"]).unwrap(),
        ["basis", "consumer", "input"]
    );
}

#[test]
fn components_and_gates_are_deterministic_and_only_direct() {
    let ids = ["a", "b", "c", "isolated"];
    let edges = [("a", "b"), ("b", "c"), ("a", "b"), ("isolated", "isolated")];
    let config = reviewed(&ids, &edges);
    let graph = Graph::new(&config).unwrap();
    let result = graph.evaluate(&BTreeMap::new());
    let component = &graph.components()[graph.artifacts()["c"].component];
    assert_eq!(component.gates, ["b/check"]);
    assert_eq!(component.dependencies, [graph.artifacts()["b"].component]);
    assert!(result.evals["isolated/check"].can_execute());
    assert_eq!(graph.dependency_closure(&[]).unwrap(), Vec::<&str>::new());
    let mut reordered = reviewed(&ids, &edges);
    reordered.relations.reverse();
    reordered.evals.reverse();
    let reordered = Graph::new(&reordered).unwrap();
    assert_eq!(graph.components(), reordered.components());
    assert_eq!(result, reordered.evaluate(&BTreeMap::new()));
}

#[test]
fn all_scope_relation_kinds_feed_the_same_graph() {
    let fixture = tempfile::tempdir().unwrap();
    for (path, declaration) in [
        (
            "owner",
            json!({"name":"owner", "mounts":{"input":"mounted"}, "evals":[
                {"id":"check", "title":"Check", "profile":{"kind":"runtime", "command":"not-run", "args":["{argument}/file"]},
                 "payload":{"instruction":"Inspect {instruction}."}}
            ]}),
        ),
        ("owner/child", json!({"name":"child", "basis":true})),
        ("mounted", json!({"name":"mounted", "basis":true})),
        ("instruction", json!({"name":"instruction", "basis":true})),
        ("argument", json!({"name":"argument", "basis":true})),
    ] {
        let path = fixture.path().join(path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("artifactize.json"), declaration.to_string()).unwrap();
    }
    let config = read_workspace_config(fixture.path()).unwrap();
    assert_eq!(config.relations.len(), 4);
    let graph = Graph::new(&config).unwrap();
    assert_eq!(
        graph.dependency_closure(&["owner"]).unwrap(),
        ["argument", "child", "instruction", "mounted", "owner"]
    );
    assert_eq!(graph.eval_target("owner/check"), Some("owner"));
    assert!(graph.evaluate(&BTreeMap::new()).evals["owner/check"].can_execute());
}

#[test]
fn invalid_graph_references_fail_closed() {
    let mut config = reviewed(&["a", "b"], &[("missing", "b")]);
    assert!(
        Graph::new(&config)
            .unwrap_err()
            .0
            .contains("Unknown relation")
    );
    config.relations.clear();
    config.evals[0].target = "missing".into();
    assert!(
        Graph::new(&config)
            .unwrap_err()
            .0
            .contains("Unknown Eval target")
    );
    config.evals[0].target = "a".into();
    config.evals[1].id = config.evals[0].id.clone();
    assert!(
        Graph::new(&config)
            .unwrap_err()
            .0
            .contains("Duplicate Eval")
    );
    config.evals.pop();
    config.artifacts.get_mut("a").unwrap().basis = Some(true);
    assert!(
        Graph::new(&config)
            .unwrap_err()
            .0
            .contains("cannot own Evals")
    );
}

#[test]
fn deep_graphs_keep_sparse_edges_and_iterative_traversal() {
    let names: Vec<_> = (0..4096).map(|index| format!("a{index:04}")).collect();
    let ids: Vec<_> = names.iter().map(String::as_str).collect();
    let edges: Vec<_> = ids.windows(2).map(|pair| (pair[0], pair[1])).collect();
    let config = reviewed(&ids, &edges);
    let graph = Graph::new(&config).unwrap();
    assert_eq!(graph.components().len(), names.len());
    assert_eq!(
        graph
            .components()
            .iter()
            .map(|component| component.gates.len())
            .sum::<usize>(),
        names.len() - 1
    );
    assert_eq!(
        graph
            .dependency_closure(&[ids[ids.len() - 1]])
            .unwrap()
            .len(),
        names.len()
    );
    let evaluation = graph.evaluate(&evidence(&[("a0000/check", RED)]));
    assert_eq!(
        evaluation.evals["a4095/check"].readiness,
        Readiness::Blocked
    );
}

#[test]
fn ignored_gates_leave_actual_evidence_and_final_obligations_intact() {
    let config = reviewed(&["a", "b", "c"], &[("a", "b"), ("b", "c")]);
    let graph = Graph::new(&config).unwrap();
    let outcomes = evidence(&[("b/check", GREEN)]);
    let ignored = graph.evaluate_with_policy(&outcomes, true);
    assert!(ignored.evals["c/check"].can_execute());
    assert_eq!(ignored.evals["b/check"].status, EvalStatus::Green);
    assert!(!ignored.artifacts["b"].satisfied);
    assert_eq!(ignored.obligations, ["a", "c"]);
    assert_eq!(ignored.status, FinalStatus::Incomplete);
    assert_eq!(
        graph.evaluate(&outcomes).evals["b/check"].status,
        EvalStatus::Wait
    );
    let failed = graph.evaluate_with_policy(
        &evidence(&[
            ("a/check", RED),
            ("b/check", Evidence::OperationalError),
            ("c/check", GREEN),
        ]),
        true,
    );
    assert_eq!(failed.status, FinalStatus::Error);
    assert_eq!(failed.evals["a/check"].status, EvalStatus::Red);
    assert_eq!(failed.evals["b/check"].status, EvalStatus::Error);
    assert_eq!(failed.evals["c/check"].status, EvalStatus::Green);
    assert_eq!(failed.obligations, ["a", "b"]);
}
