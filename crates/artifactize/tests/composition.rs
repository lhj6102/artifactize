//! Composition by name and aspects: a constituent's verdict is part of its parent's, while
//! only references decide what an eval reads, reuses and waits for.
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
        fs::write(path, contents).unwrap();
    }

    /// Declare `name` in `file`, a folder's `index.artf` or a `<label>.artf`.
    fn declare(&self, file: &str, declaration: Value) {
        let path = self.repo.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        support::declaration::write(path, declaration.to_string()).unwrap();
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

/// The evals this Run executed rather than reused, by their executions' Run.
fn executed(run: &Value) -> Vec<String> {
    let own = format!("execution-{}-", run["id"].as_str().unwrap());
    run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|request| {
            request["executionId"]
                .as_str()
                .is_some_and(|id| id.starts_with(&own))
        })
        .map(|request| request["evalId"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn names_compose_artifacts_and_a_failing_constituent_fails_its_parent_only() {
    let fixture = Fixture::new();
    // Folders deliberately do not follow the names, and a nested folder relates to nothing.
    fixture.declare("game/index.artf", json!({"name":"game"}));
    fixture.declare(
        "combat/index.artf",
        json!({"name":"game::combat","evals":runtime(true, "{game::combat}")}),
    );
    fixture.declare(
        "game/fireball/index.artf",
        json!({"name":"game::combat::skills::fireball","evals":runtime(false, "the skill")}),
    );
    fixture.declare(
        "consumer/index.artf",
        json!({"name":"consumer","evals":runtime(true, "{game::combat}")}),
    );
    let graph = fixture.json(&["config", "graph", "--json"], 0);
    let children: Vec<_> = graph["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|relation| relation["kind"] == "child")
        .map(|relation| (relation["source"].clone(), relation["target"].clone()))
        .collect();
    assert_eq!(
        children,
        [
            (json!("game::combat"), json!("game")),
            (
                json!("game::combat::skills::fireball"),
                json!("game::combat")
            ),
        ]
    );

    // Selecting the parent selects every constituent.
    let run = fixture.json(&["verify", "game", "--wait", "--json"], 1);
    let mut ran = executed(&run);
    ran.sort();
    assert_eq!(
        ran,
        ["game::combat/check", "game::combat::skills::fireball/check"]
    );
    // The RED constituent blocks no eval of its parent, yet no level above it passes.
    assert_eq!(status(&run, "evals", "game::combat/check"), "GREEN");
    assert_eq!(
        status(&run, "evals", "game::combat::skills::fireball/check"),
        "RED"
    );
    assert_eq!(status(&run, "artifacts", "game::combat"), "RED");
    assert_eq!(status(&run, "artifacts", "game"), "RED");

    // A consumer of the parent runs on the parent's own evals, but cannot finish GREEN.
    let run = fixture.json(&["verify", "consumer", "--wait", "--json"], 1);
    assert_eq!(status(&run, "evals", "consumer/check"), "GREEN");
    assert_eq!(run["status"], "RED");

    // Fixed, the grouping parent without evals passes on its constituents alone.
    fixture.declare(
        "game/fireball/index.artf",
        json!({"name":"game::combat::skills::fireball","evals":runtime(true, "the skill")}),
    );
    let run = fixture.json(&["verify", "game", "--wait", "--json"], 0);
    assert_eq!(status(&run, "artifacts", "game"), "GREEN");
}

#[test]
fn a_constituent_change_reviews_only_the_evals_that_name_it() {
    let fixture = Fixture::new();
    fixture.write("app/main.txt", "main\n");
    fixture.write("app/ui/view.txt", "view\n");
    fixture.declare(
        "app/index.artf",
        json!({"name":"app","evals":reads("main.txt")}),
    );
    fixture.declare(
        "app/ui/index.artf",
        json!({"name":"app::ui","evals":reads("view.txt")}),
    );
    fixture.declare(
        "integration/index.artf",
        json!({"name":"integration","evals":reads("{app::ui}/view.txt")}),
    );
    let first = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
    assert_eq!(first["executionsStarted"], 3);
    fixture.write("app/ui/view.txt", "changed\n");
    let second = fixture.json(&["verify", "--all", "--wait", "--json"], 0);
    let mut ran = executed(&second);
    ran.sort();
    assert_eq!(ran, ["app::ui/check", "integration/check"]);
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
            "name":"game::fireball",
            "select":["src/skills/fireball.rs","assets"],
            "views":{"agent_tools":{"list":{"builtin":"list"}}},
            "evals":reads("{game::fireball}/src/skills/fireball.rs"),
        }),
    );
    let graph = fixture.json(&["config", "graph", "--json"], 0);
    assert_eq!(graph["artifacts"]["game::fireball"]["kind"], "aspect");

    let list = |path: &str| {
        let args = format!(r#"{{"path":"{path}"}}"#);
        let listed = fixture.json(
            &[
                "tools",
                "check",
                "--execute",
                "--artifact",
                "game::fireball",
                "--audience",
                "agent",
                "--tool",
                "list",
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
    assert_eq!(ran, ["game/check", "game::fireball/check"]);
}

#[test]
fn an_aspect_is_declared_only_by_a_label_that_names_no_file_and_reads_only_its_selection() {
    let fixture = Fixture::new();
    fixture.write("game/src/skills/fireball.rs", "fireball\n");
    fixture.write("game/src/other.rs", "other\n");
    fixture.declare("game/index.artf", json!({"name":"game"}));
    let aspect = |args: &str| {
        json!({"name":"game::fireball","select":["src/skills"],
            "evals":reads(args)})
    };
    fixture.declare(
        "game/fireball.artf",
        aspect("{game::fireball}/src/other.rs"),
    );
    assert!(
        fixture
            .error(&["config", "check"])
            .contains("exposes only its selected paths: src/skills")
    );

    fixture.declare(
        "game/fireball.artf",
        aspect("{game::fireball}/src/skills/fireball.rs"),
    );
    fixture.json(&["config", "check", "--json"], 0);
    fixture.write("game/fireball", "a file named like the label");
    assert!(
        fixture
            .error(&["config", "check"])
            .contains("declares the file fireball")
    );
    fs::remove_file(fixture.repo.join("game/fireball")).unwrap();

    fixture.declare("game/index.artf", json!({"name":"game","select":["src"]}));
    assert!(
        fixture
            .error(&["config", "check"])
            .contains("select declares an aspect")
    );
    fixture.declare("game/index.artf", json!({"name":"game"}));

    fixture.declare(
        "game/fireball.artf",
        json!({"name":"game::fireball","select":["src/missing.rs"]}),
    );
    assert!(
        fixture
            .error(&["config", "check"])
            .contains("select: src/missing.rs")
    );
}
