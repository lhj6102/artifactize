//! Composition by folders and `parts`, and aspects: a part's verdict is part of each
//! whole's; references decide access and gates, while reuse also covers target composition.
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
    root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let base = support::os::canonical(root.path());
        let (repo, state) = (base.join("repo"), base.join("state"));
        fs::create_dir(&repo).unwrap();
        root.track(&state, &repo);
        Self { root, repo, state }
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        support::declaration::write(&self.repo, path, contents).unwrap();
    }

    /// Declare `name` in `file`, a folder's `index.artf` or a `<label>.artf`.
    fn declare(&self, file: &str, declaration: Value) {
        let path = self.repo.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        support::declaration::write(&(self.repo), path, declaration.to_string()).unwrap();
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(support::os::cli())
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

    fn error(&self, args: &[&str]) -> String {
        let output = self.cli(args);
        assert!(!output.status.success());
        String::from_utf8_lossy(&output.stderr).into_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.root.release(&self.state, &self.repo);
    }
}

/// A runtime eval passing with `pass`; it names `inputs` in its instruction.
fn runtime(pass: bool, inputs: &str) -> Value {
    let program = if pass {
        support::os::true_program()
    } else {
        support::os::false_program()
    };
    json!({"check":{"title":"Check","profile":{"kind":"runtime","command":program,"args":[]},
        "payload":{"instruction":format!("Check {inputs}.")}}})
}

/// A runtime eval printing `file` through a reference.
fn reads(file: &str) -> Value {
    json!({"check":{"title":"Check","profile":{"kind":"runtime",
        "command":support::os::cat_program(),"args":[file]},
        "payload":{"instruction":"Print the file."}}})
}

fn status<'a>(run: &'a Value, kind: &str, id: &str) -> &'a Value {
    &run["validation"][kind]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("{id} in {run}"))["status"]
}

/// The evals this Run executed rather than reused, by the Run their results came from.
fn executed(run: &Value) -> Vec<String> {
    run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|request| request["provenance"]["runId"] == run["id"])
        .map(|request| request["evalId"].as_str().unwrap().to_owned())
        .collect()
}

/// Each part relation in the graph, as (part, whole), in the graph's order.
fn parts(graph: &Value) -> Vec<(String, String)> {
    let mut parts: Vec<_> = graph["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|relation| relation["kind"] == "part")
        .map(|relation| {
            let name = |key: &str| relation[key].as_str().unwrap().to_owned();
            (name("source"), name("target"))
        })
        .collect();
    parts.sort();
    parts
}

#[test]
fn folders_and_parts_compose_artifacts_and_a_failing_part_fails_each_whole_only() {
    let fixture = Fixture::new();
    // combat is a part of game by its folder, fireball of combat; physics, elsewhere, of game
    // because game names it, and shared of both game and tools.
    fixture.declare(
        "game/index.artf",
        json!({"name":"game","parts":["physics","shared"]}),
    );
    fixture.declare(
        "game/combat/index.artf",
        json!({"name":"combat","evals":runtime(true, "{combat}")}),
    );
    fixture.declare(
        "game/combat/skills/fireball/index.artf",
        json!({"name":"fireball","evals":runtime(false, "the skill")}),
    );
    fixture.declare(
        "physics/index.artf",
        json!({"name":"physics","evals":runtime(true, "the physics")}),
    );
    fixture.declare(
        "tools/index.artf",
        json!({"name":"tools","parts":["shared"],"evals":runtime(true, "the tools")}),
    );
    fixture.declare(
        "tools/shared/index.artf",
        json!({"name":"shared","evals":runtime(true, "the module")}),
    );
    fixture.declare(
        "consumer/index.artf",
        json!({"name":"consumer","evals":runtime(true, "{combat}")}),
    );
    let graph = fixture.json(&["config", "graph", "--json"], 0);
    let part = |part: &str, whole: &str| (part.to_owned(), whole.to_owned());
    // A part its folder makes and `parts` names again counts once.
    assert_eq!(
        parts(&graph),
        [
            part("combat", "game"),
            part("fireball", "combat"),
            part("physics", "game"),
            part("shared", "game"),
            part("shared", "tools"),
        ]
    );

    // Selecting an Artifact selects every part, its parts' parts included.
    let run = fixture.json(&["verify", "game", "--wait", "--json"], 1);
    let mut ran = executed(&run);
    ran.sort();
    assert_eq!(
        ran,
        [
            "combat/check",
            "fireball/check",
            "physics/check",
            "shared/check"
        ]
    );
    // The RED part blocks no eval of the whole, yet no whole above it passes.
    assert_eq!(status(&run, "evals", "combat/check"), "GREEN");
    assert_eq!(status(&run, "evals", "fireball/check"), "RED");
    assert_eq!(status(&run, "artifacts", "combat"), "RED");
    assert_eq!(status(&run, "artifacts", "game"), "RED");

    // A shared part's verdict belongs to each of its wholes, and only to them.
    let run = fixture.json(&["verify", "tools", "--wait", "--json"], 0);
    assert_eq!(status(&run, "artifacts", "tools"), "GREEN");

    // A consumer of a whole runs on the whole's own evals, but cannot finish GREEN.
    let run = fixture.json(&["verify", "consumer", "--wait", "--json"], 1);
    assert_eq!(status(&run, "evals", "consumer/check"), "GREEN");
    assert_eq!(run["status"], "RED");

    // Fixed, the gathering whole without evals passes on its parts alone.
    fixture.declare(
        "game/combat/skills/fireball/index.artf",
        json!({"name":"fireball","evals":runtime(true, "the skill")}),
    );
    let run = fixture.json(&["verify", "game", "--wait", "--json"], 0);
    assert_eq!(status(&run, "artifacts", "game"), "GREEN");
}

#[test]
fn parts_reject_cycles_unknown_names_and_invalid_names() {
    let fixture = Fixture::new();
    fixture.declare("a/index.artf", json!({"name":"a","parts":["b"]}));
    fixture.declare("b/index.artf", json!({"name":"b","parts":["a"]}));
    assert!(
        fixture
            .error(&["config", "check"])
            .contains("is a part of itself through its parts")
    );
    fixture.declare("b/index.artf", json!({"name":"b","parts":["missing"]}));
    assert!(
        fixture
            .error(&["config", "check"])
            .contains("Unknown Artifact `missing`")
    );
    fixture.declare("b/index.artf", json!({"name":"b","parts":["b"]}));
    assert!(
        fixture
            .error(&["config", "check"])
            .contains("b cannot be a part of itself")
    );
    fixture.declare("b/index.artf", json!({"name":"b"}));
    fixture.declare(".artf", json!({"name":"graph","parts":["b"]}));
    fixture.declare("a/index.artf", json!({"name":"a","parts":["graph"]}));
    assert!(fixture.cli(&["config", "check"]).status.success());
    fixture.declare("a/index.artf", json!({"name":"a"}));
    // A name starts with a letter; the error names the line it is on.
    fs::write(
        fixture.repo.join(".artifactize/bad.artf"),
        "artf 1b = { path = \"b/\" }\n",
    )
    .unwrap();
    let error = fixture.error(&["config", "check"]);
    assert!(
        error.contains("bad.artf") && error.contains("line 1"),
        "{error}"
    );
}

#[test]
fn a_part_change_reviews_its_wholes_evals_and_the_evals_that_name_it() {
    let fixture = Fixture::new();
    fixture.write("app/main.txt", "main\n");
    fixture.write("app/ui/view.txt", "view\n");
    fixture.declare(
        "app/index.artf",
        json!({"name":"app","evals":reads("main.txt")}),
    );
    fixture.declare(
        "app/ui/index.artf",
        json!({"name":"ui","evals":reads("view.txt")}),
    );
    fixture.declare(
        "integration/index.artf",
        json!({"name":"integration","evals":reads("{ui}/view.txt")}),
    );
    let first = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
    assert_eq!(first["executionsStarted"], 3);
    fixture.write("app/ui/view.txt", "changed\n");
    let second = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
    let mut ran = executed(&second);
    ran.sort();
    assert_eq!(ran, ["app/check", "integration/check", "ui/check"]);
}

#[test]
fn whole_reuse_tracks_each_parts_fingerprint_separately_from_its_own_files() {
    let fixture = Fixture::new();
    fixture.declare(
        "app/index.artf",
        json!({"name":"app","parts":["external"],"evals":runtime(true, "the whole")}),
    );
    fixture.write("app/own.txt", "own");
    fixture.write("app/part.txt", "file part");
    fixture.declare(
        "app/part.txt.artf",
        json!({"name":"file-part","evals":runtime(true, "the file")}),
    );
    fixture.declare(
        "app/nested/index.artf",
        json!({"name":"nested","evals":runtime(true, "the folder")}),
    );
    fixture.write("app/nested/own.txt", "nested part");
    fixture.declare(
        "app/nested/deep/index.artf",
        json!({"name":"deep","evals":runtime(true, "the deep part")}),
    );
    fixture.write("app/nested/deep/own.txt", "deep part");
    fixture.declare(
        "external/index.artf",
        json!({"name":"external","evals":runtime(true, "the external part")}),
    );
    fixture.write("external/own.txt", "external part");
    fixture.declare(
        "unrelated/index.artf",
        json!({"name":"unrelated","evals":runtime(true, "an unrelated Artifact")}),
    );
    let first = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
    assert_eq!(first["executionsStarted"], 6);
    let unchanged = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
    assert!(executed(&unchanged).is_empty());

    for (path, changed_part, expected) in [
        (
            "app/part.txt",
            "file-part",
            vec!["app/check", "file-part/check"],
        ),
        (
            "app/nested/own.txt",
            "nested",
            vec!["app/check", "nested/check"],
        ),
        (
            "app/nested/deep/own.txt",
            "deep",
            vec!["app/check", "deep/check", "nested/check"],
        ),
        (
            "external/own.txt",
            "external",
            vec!["app/check", "external/check"],
        ),
        ("app/own.txt", "", vec!["app/check"]),
    ] {
        fixture.write(path, "changed");
        let prediction = fixture.json(&["status", "--all", "--json"], 1);
        let whole = prediction["evals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|eval| eval["id"] == "app/check")
            .unwrap();
        if !changed_part.is_empty() {
            assert_eq!(whole["changes"]["dependencies"], json!([changed_part]));
            assert!(
                whole["changes"]["files"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
            );
        }
        let run = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
        let mut ran = executed(&run);
        ran.sort();
        assert_eq!(ran, expected, "{path}");
        let unchanged = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
        assert!(executed(&unchanged).is_empty(), "{path}");
    }
}

#[test]
fn an_aspect_selects_part_of_a_folder_and_reuses_while_the_rest_changes() {
    let fixture = Fixture::new();
    fixture.write("game/src/skills/fireball.rs", "fireball\n");
    fixture.write("game/src/other.rs", "other\n");
    fixture.write("game/assets/fireball.png", "sprite\n");
    fixture.declare(
        "game/index.artf",
        json!({"name":"game","evals":reads("src/other.rs")}),
    );
    fixture.declare(
        "game/fireball.artf",
        json!({
            "name":"fireball",
            "select":["src/skills/fireball.rs","assets"],
            "views":{"agent_tools":{"list":{"builtin":"list"}}},
            "evals":reads("{fireball}/src/skills/fireball.rs"),
        }),
    );
    let graph = fixture.json(&["config", "graph", "--json"], 0);
    assert_eq!(graph["artifacts"]["fireball"]["kind"], "aspect");

    let list = |path: &str| {
        let args = format!(r#"{{"path":"{path}"}}"#);
        let listed = fixture.json(
            &[
                "tools",
                "check",
                "--execute",
                "--artifact",
                "fireball",
                "--audience",
                "agent",
                "--tool",
                "fireball_list",
                "--args",
                &args,
            ],
            0,
        );
        listed["result"]["content"][0]["data"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(list("."), ["assets", "src"]);
    assert_eq!(list("src"), ["skills"]);

    fixture.json(&["verify", "--all", "--wait", "--json"], 0);
    // A change outside the selection reviews the folder again, not the aspect.
    fixture.write("game/src/other.rs", "changed\n");
    let run = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
    assert_eq!(executed(&run), ["game/check"]);
    fixture.write("game/assets/fireball.png", "new sprite\n");
    let run = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
    let mut ran = executed(&run);
    ran.sort();
    assert_eq!(ran, ["fireball/check", "game/check"]);
}
