use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    _root: TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        Self {
            repo,
            state: root.path().join("state"),
            _root: root,
        }
    }

    fn artifact(&self, folder: &str, value: Value) {
        self.file(&format!("{folder}/artifactize.json"), &value.to_string());
    }

    fn file(&self, path: &str, contents: &str) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.run(&[args, &["--json"]].concat());
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?} {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Evals that started an execution in this Run, rather than reusing one.
    fn executed(&self, code: i32) -> Vec<String> {
        let run = self.json(&["verify", "--all"], code);
        let prefix = format!("execution-{}-", run["id"].as_str().unwrap());
        run["requests"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|request| {
                request["executionId"]
                    .as_str()
                    .is_some_and(|id| id.starts_with(&prefix))
            })
            .map(|request| request["evalId"].as_str().unwrap().to_owned())
            .collect()
    }
}

fn artifact(name: &str, fingerprint: Value, mounts: Value, args: &[&str]) -> Value {
    json!({
        "name": name, "mounts": mounts, "fingerprint": fingerprint,
        "evals": [{"id":"check","title":"Check","profile":{"kind":"runtime","command":"/bin/sh","args":args},"payload":{"instruction":"Check."}}]
    })
}

fn content(dependencies: &str) -> Value {
    json!({ "dependencies": dependencies })
}

const PASS: &[&str] = &["-c", "exit 0"];

#[test]
fn direct_scope_stops_after_one_hop_and_transitive_reaches_the_whole_chain() {
    let fixture = Fixture::new();
    fixture.artifact("base", artifact("base", content("none"), json!({}), PASS));
    fixture.artifact(
        "mid",
        artifact("mid", content("direct"), json!({"base":"base"}), PASS),
    );
    let top = |dependencies| artifact("top", content(dependencies), json!({"mid":"mid"}), PASS);
    fixture.artifact("top", top("direct"));
    for name in ["base", "mid", "top"] {
        fixture.file(&format!("{name}/file.txt"), "v1");
    }
    assert_eq!(
        fixture.executed(0),
        ["base/check", "mid/check", "top/check"]
    );
    assert!(fixture.executed(0).is_empty());
    fixture.file("base/file.txt", "v2");
    assert_eq!(fixture.executed(0), ["base/check", "mid/check"]);

    fixture.artifact("top", top("transitive"));
    assert_eq!(fixture.executed(0), ["top/check"]);
    fixture.file("base/file.txt", "v3");
    assert_eq!(
        fixture.executed(0),
        ["base/check", "mid/check", "top/check"]
    );

    fixture.artifact("top", top("none"));
    assert_eq!(fixture.executed(0), ["top/check"]);
    fixture.file("mid/file.txt", "v2");
    assert_eq!(fixture.executed(0), ["mid/check"]);
}

#[test]
fn a_merge_rereviews_only_the_new_pairing_by_default() {
    let fixture = Fixture::new();
    let default = json!({});
    for side in ["left", "right"] {
        fixture.artifact(side, artifact(side, default.clone(), json!({}), PASS));
        fixture.file(&format!("{side}/part.txt"), "v1");
    }
    let mut pair = artifact(
        "pair",
        default.clone(),
        json!({}),
        &[
            "-c",
            "cat \"$1\" \"$2\"",
            "sh",
            "{left}/part.txt",
            "{right}/part.txt",
        ],
    );
    pair["evals"][0]["payload"]["instruction"] = json!("Check that {left} and {right} fit.");
    fixture.artifact("pair", pair);
    fixture.artifact(
        "app",
        artifact("app", default, json!({"pair":"pair"}), PASS),
    );
    assert_eq!(fixture.executed(0).len(), 4);
    fixture.file("left/part.txt", "v2");
    assert_eq!(fixture.executed(0), ["left/check", "pair/check"]);
}

#[test]
fn generated_and_ignored_review_output_never_changes_the_fingerprint() {
    let fixture = Fixture::new();
    fixture.artifact(
        "py",
        artifact(
            "py",
            json!({}),
            json!({}),
            &[
                "-c",
                "python3 check.py && mkdir -p out && echo report > out/report.txt",
            ],
        ),
    );
    fixture.file("py/helper.py", "VALUE = 1\n");
    fixture.file("py/check.py", "import helper\nassert helper.VALUE == 1\n");
    // A repository-root .gitignore applies to every Artifact below it.
    fixture.file(".gitignore", "out/\n");
    let run = fixture.json(&["verify", "--all"], 0);
    assert_eq!(run["requests"][0]["status"], "GREEN", "{run}");
    assert!(fixture.repo.join("py/__pycache__").is_dir());
    assert!(fixture.repo.join("py/out/report.txt").is_file());
    assert_eq!(
        run["validation"]["artifacts"][0]["fingerprintKind"],
        "content"
    );
    assert!(fixture.executed(0).is_empty());

    fixture.artifact(
        "py",
        artifact(
            "py",
            json!({}),
            json!({}),
            &["-c", "echo stray > stray.txt"],
        ),
    );
    let run = fixture.json(&["verify", "--all"], 2);
    assert_eq!(run["requests"][0]["errorCode"], "INPUT_CHANGED");
}

#[test]
fn dependency_cycles_terminate_in_every_scope() {
    let fixture = Fixture::new();
    fixture.artifact(
        "a",
        artifact("a", content("transitive"), json!({"peer":"b"}), PASS),
    );
    fixture.artifact(
        "b",
        artifact("b", content("direct"), json!({"peer":"a"}), PASS),
    );
    fixture.file("a/file.txt", "v1");
    fixture.file("b/file.txt", "v1");
    assert_eq!(fixture.executed(0), ["a/check", "b/check"]);
    assert!(fixture.executed(0).is_empty());
    fixture.file("a/file.txt", "v2");
    assert_eq!(fixture.executed(0), ["a/check", "b/check"]);
}

#[test]
fn status_explains_which_inputs_and_dependencies_changed() {
    let fixture = Fixture::new();
    fixture.artifact("core", json!({"name":"core","basis":true}));
    fixture.file("core/lib.txt", "v1");
    fixture.artifact(
        "api",
        artifact("api", json!({}), json!({"core":"core"}), PASS),
    );
    fixture.file("api/src/a.py", "v1");
    fixture.file("api/old.md", "v1");
    fixture.artifact(
        "legacy",
        artifact(
            "legacy",
            json!({"script":{"command":"/bin/cat","args":["key"]}}),
            json!({}),
            PASS,
        ),
    );
    fixture.file("legacy/key", "v1");
    let first = fixture.json(&["verify", "--all"], 0);
    let run = first["id"].as_str().unwrap();
    let status = fixture.json(&["status"], 0);
    assert!(status["evals"][0].get("changes").is_none());

    fixture.file("api/src/a.py", "v2");
    fixture.file("api/docs/new.md", "v1");
    fs::remove_file(fixture.repo.join("api/old.md")).unwrap();
    fixture.file("core/lib.txt", "v2");
    fixture.file("legacy/key", "v2");
    let status = fixture.json(&["status"], 1);
    let api = &status["evals"][0];
    assert_eq!(api["id"], "api/check");
    assert_eq!(api["action"], "execute");
    assert_eq!(
        api["changes"],
        json!({
            "sinceRunId": run,
            "files": ["+docs/new.md", "-old.md", "src/a.py"],
            "dependencies": ["core"],
            "summary": "changed: +docs/new.md, -old.md, src/a.py; dependency core changed",
        })
    );
    assert_eq!(
        status["evals"][1]["changes"],
        json!({"sinceRunId": run, "summary": "fingerprint changed"})
    );
    let text = String::from_utf8(fixture.run(&["status"]).stdout).unwrap();
    assert!(
        text.contains(&format!(
            "Fingerprint changed since Run {run}: changed: +docs/new.md, -old.md, src/a.py; dependency core changed"
        )),
        "{text}"
    );
    assert!(text.contains(&format!(
        "Fingerprint changed since Run {run}: fingerprint changed"
    )));
}
