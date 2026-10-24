//! `space::name` Artifact names: a space groups names for people and changes no identity,
//! reference, tool or verify semantics.
#[path = "support/run.rs"]
mod owned_run;

use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

use serde_json::{Value, json};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

struct Fixture {
    _root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
    home: PathBuf,
}

impl Fixture {
    /// `combat::Spec` (a basis with notes), `combat::Code` whose evals use it, and an
    /// unprefixed `Code` that shares only the local part of a name.
    fn new() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let base = support::os::canonical(root.path());
        let (repo, state, home) = (base.join("repo"), base.join("state"), base.join("home"));
        fs::create_dir(&repo).unwrap();
        root.track(&state, &repo);
        root.track(&home, &repo);
        let fixture = Self {
            _root: root,
            repo,
            state,
            home,
        };
        fixture.declare(
            "spec",
            json!({"name":"combat::Spec","basis":true,
                "views":{"agent_tools":{"read":{"builtin":"read"}}}}),
        );
        fs::write(fixture.repo.join("spec/notes.md"), "spec notes\n").unwrap();
        fixture.declare(
            "code",
            json!({"name":"combat::Code","fingerprint":false,"evals":[
                {"id":"check","title":"Check","payload":{"instruction":"Check {combat::Spec}."},
                    "profile":{"kind":"runtime","command":support::os::true_program(),"args":[]}},
                {"id":"review","title":"Review","payload":{"instruction":"Review with {combat::Spec}."},
                    "profile":{"kind":"agent","backend":"openai","model":"test"}},
            ]}),
        );
        fixture.declare("plain", json!({"name":"Code","basis":true}));
        fixture
    }

    fn declare(&self, folder: &str, declaration: Value) {
        fs::create_dir_all(self.repo.join(folder)).unwrap();
        support::declaration::write(
            self.repo.join(folder).join("index.artf"),
            declaration.to_string(),
        )
        .unwrap();
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(support::os::cli())
            .env("ARTIFACTIZE_STATE_HOME", &self.home)
            .env_remove("OPENAI_API_KEY")
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .record_output()
            .unwrap()
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.cli(args);
        assert_eq!(
            output.status.code(),
            Some(code),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self._root.release(&self.state, &self.repo);
        let _ = self._root.release(&self.home, &self.repo);
    }
}

fn relates(graph: &Value, source: &str, target: &str) -> bool {
    graph["relations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|relation| {
            let relation = relation.get("relation").unwrap_or(relation);
            relation["source"] == source && relation["target"] == target
        })
}

#[test]
fn spaced_names_are_whole_identities_that_reference_across_the_graph() {
    let fixture = Fixture::new();
    let graph = fixture.json(&["config", "graph", "--json"], 0);
    let names: Vec<&str> = graph["artifacts"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(names, ["Code", "combat::Code", "combat::Spec"]);
    assert!(relates(&graph, "combat::Spec", "combat::Code"), "{graph}");
    assert!(!relates(&graph, "Code", "combat::Code"), "{graph}");

    // A model's tool name spells the space with `__`; the tool still reads its own Artifact.
    let read = fixture.json(
        &[
            "tools",
            "check",
            "--json",
            "--execute",
            "--eval",
            "combat::Code/review",
            "--tool",
            "read_combat__Spec",
            "--args",
            r#"{"path":"notes.md"}"#,
        ],
        0,
    );
    let data = &read["result"]["content"][0]["data"];
    assert_eq!(data["resolvedArtifactId"], "combat::Spec");
    assert_eq!(data["lines"][0]["text"], "spec notes\n");

    // The selected eval runs GREEN; the Artifact's unrun Agent eval leaves it INCOMPLETE.
    let verified = fixture.json(&["verify", "combat::Code/check", "--wait", "--json"], 4);
    let check = verified["validation"]["evals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|eval| eval["id"] == "combat::Code/check");
    assert_eq!(check.unwrap()["status"], "GREEN", "{verified}");
}

#[test]
fn a_name_has_at_most_one_space_and_no_empty_part() {
    let fixture = Fixture::new();
    let longest = format!("{}::{}", "s".repeat(30), "n".repeat(32));
    fixture.declare("long", json!({"name":longest,"basis":true}));
    fixture.json(&["config", "check", "--json"], 0);
    for name in [
        "combat::",
        "::Code",
        "a::b::c",
        "combat:Code",
        "combat:::Code",
        "_combat::Code",
        &format!("{longest}x"),
    ] {
        fixture.declare("long", json!({"name":name,"basis":true}));
        let output = fixture.cli(&["config", "check"]);
        let text = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success() && text.contains("Artifact name must match"),
            "{name}: {text}"
        );
    }
}

#[test]
fn names_that_spell_one_tool_name_collide() {
    let fixture = Fixture::new();
    fixture.declare(
        "twin",
        json!({"name":"combat__Spec","basis":true,
            "views":{"agent_tools":{"read":{"builtin":"read"}}}}),
    );
    fixture.declare(
        "code",
        json!({"name":"combat::Code","evals":[{"id":"review","title":"Review",
            "payload":{"instruction":"Review {combat::Spec} and {combat__Spec}."},
            "profile":{"kind":"agent","backend":"openai","model":"test"}}]}),
    );
    let report = fixture.json(&["tools", "check", "--json", "combat::Code/review"], 1);
    assert!(
        report
            .to_string()
            .contains("tool names collide: read_combat__Spec"),
        "{report}"
    );
}
