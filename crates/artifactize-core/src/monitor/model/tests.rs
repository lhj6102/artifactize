use super::{Section, Target, detail};
use crate::{store::RunView, test_request::request};
use serde_json::json;

#[test]
fn eval_detail_keeps_request_instruction_and_result_outside_technical() {
    let request = request(
        "app/check",
        "GREEN",
        json!({
            "payload":{"instruction":"Check the release notes."},
            "result":{"verdict":"GREEN","note":"Release notes match."},
            "fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "key":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }),
    );
    let run: RunView = serde_json::from_value(json!({
        "id":"run-1",
        "repoPath":crate::test_os::temp_root().join("repo"),
        "stateDir":crate::test_os::temp_root().join("state"),
        "status":"GREEN",
        "createdAt":"2026-01-01T00:00:00Z",
        "completedAt":"2026-01-01T00:00:01Z",
        "selection":{"kind":"all"},
        "definitions":{"artifacts":{},"evals":[]},
        "validation":null,
        "requests":[request.request]
    }))
    .unwrap();
    let target = Target::Eval("app/check".parse().unwrap());
    let now = run.run.created_at.time();
    let detail = detail(&run, &[request], &target, now);
    let visible = detail
        .fields
        .iter()
        .filter(|(key, _)| Section::of(key) != Section::Technical)
        .map(|(key, value)| (*key, value.as_str()))
        .collect::<Vec<_>>();

    assert!(visible.contains(&("Request", "run-1-app-check")));
    assert!(visible.contains(&("Instruction", "Check the release notes.")));
    let result = visible
        .iter()
        .find_map(|(key, value)| (*key == "Result").then_some(*value))
        .expect("the result is visible without unfolding Technical");
    assert!(result.contains("GREEN"));
    assert!(result.contains("Release notes match."));
    for key in ["Fingerprint", "Key"] {
        assert!(detail.field(key).is_some_and(|value| !value.is_empty()));
        assert!(!visible.iter().any(|(name, _)| *name == key));
    }
}

#[test]
fn run_detail_shows_the_comment_the_run_was_requested_with() {
    let run = |comment: Option<&str>| -> RunView {
        let mut run = json!({
            "id":"run-1",
            "repoPath":crate::test_os::temp_root().join("repo"),
            "stateDir":crate::test_os::temp_root().join("state"),
            "status":"RUNNING",
            "createdAt":"2026-01-01T00:00:00Z",
            "completedAt":null,
            "selection":{"kind":"all"},
            "definitions":{"artifacts":{},"evals":[]},
            "validation":null,
            "requests":[]
        });
        if let Some(comment) = comment {
            run["comment"] = json!(comment);
        }
        serde_json::from_value(run).unwrap()
    };
    let commented = run(Some("Fixing the login form"));
    let now = commented.run.created_at.time();
    let shown = detail(&commented, &[], &Target::Run, now);
    assert_eq!(shown.field("Comment"), Some("Fixing the login form"));
    assert_eq!(
        Section::of("Comment"),
        Section::What,
        "shown without unfolding Technical"
    );
    assert_eq!(
        detail(&run(None), &[], &Target::Run, now).field("Comment"),
        None
    );
}

#[test]
fn artifact_detail_groups_waits_without_dropping_equal_eval_names() {
    let run: RunView = serde_json::from_value(json!({
        "id":"run-1",
        "repoPath":crate::test_os::temp_root().join("repo"),
        "stateDir":crate::test_os::temp_root().join("state"),
        "status":"INCOMPLETE",
        "createdAt":"2026-01-01T00:00:00Z",
        "completedAt":"2026-01-01T00:00:01Z",
        "selection":{"kind":"all"},
        "definitions":{
            "artifacts":{"app":{"name":"app"},"plan":{"name":"plan"},"reference":{"name":"reference"}},
            "evals":[
                {"id":"app/first","target":"app","deps":["plan","reference"]},
                {"id":"app/second","target":"app","deps":["plan"]},
                {"id":"plan/check","target":"plan","deps":[]},
                {"id":"reference/check","target":"reference","deps":[]}
            ]
        },
        "validation":null,
        "requests":[]
    }))
    .unwrap();
    let target = Target::Artifact("app".parse().unwrap());
    let shown = detail(&run, &[], &target, run.run.created_at.time());
    let waits = shown
        .field("Waits for")
        .expect("the Artifact exposes its evals' waits");
    assert_eq!(
        waits
            .lines()
            .filter(|line| line.starts_with("↑ plan "))
            .count(),
        1
    );
    assert_eq!(
        waits
            .lines()
            .filter(|line| line.starts_with("↑ reference "))
            .count(),
        1
    );
    assert_eq!(
        waits.lines().filter(|line| line.starts_with("  ")).count(),
        2,
        "equal pending eval names from different inputs both remain visible"
    );
    assert!(waits.contains("check  not in this Run"), "{waits}");
    assert_eq!(Section::of("Waits for"), Section::Waits);
}

#[test]
fn graph_keeps_its_shape_across_lights_but_not_across_artifacts_or_edges() {
    use super::{Graph, GraphEdge, GraphNode, Light, NodeId};
    use crate::types::ArtifactName;
    let name = |text: &str| text.parse::<ArtifactName>().unwrap();
    let graph = |lights: [Light; 2], names: [&str; 2], edges: &[(&str, &str)]| Graph {
        nodes: names
            .iter()
            .zip(lights)
            .map(|(item, light)| GraphNode {
                id: NodeId::artifact(name(item)),
                name: name(item),
                light,
            })
            .collect(),
        edges: edges
            .iter()
            .map(|(from, to)| GraphEdge {
                from: name(from),
                to: name(to),
                light: lights[0],
            })
            .collect(),
    };
    let base = graph([Light::Waiting; 2], ["lib", "app"], &[("lib", "app")]);
    // Other lights keep the shape.
    assert!(base.same_shape(&graph(
        [Light::Red, Light::Green],
        ["lib", "app"],
        &[("lib", "app")]
    )));
    // Another Artifact, another order, or another edge does not.
    assert!(!base.same_shape(&graph(
        [Light::Waiting; 2],
        ["lib", "web"],
        &[("lib", "web")]
    )));
    assert!(!base.same_shape(&graph(
        [Light::Waiting; 2],
        ["app", "lib"],
        &[("lib", "app")]
    )));
    assert!(!base.same_shape(&graph([Light::Waiting; 2], ["lib", "app"], &[])));
    assert!(!base.same_shape(&graph(
        [Light::Waiting; 2],
        ["lib", "app"],
        &[("app", "lib")]
    )));
}
