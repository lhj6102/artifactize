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

const PASS: &[&str] = &["-c", "exit 0"];

#[test]
fn a_dependency_change_misses_but_two_connections_away_only_through_the_fingerprint() {
    let fixture = Fixture::new();
    fixture.artifact("base", artifact("base", json!({}), json!({}), PASS));
    fixture.artifact(
        "mid",
        artifact("mid", json!({}), json!({"base":"base"}), PASS),
    );
    fixture.artifact(
        "top",
        artifact("top", json!({}), json!({"mid":"mid"}), PASS),
    );
    for name in ["base", "mid", "top"] {
        fixture.file(&format!("{name}/file.txt"), "v1");
    }
    assert_eq!(
        fixture.executed(0),
        ["base/check", "mid/check", "top/check"]
    );
    assert!(fixture.executed(0).is_empty());
    // mid mounts base, so its key covers base's fingerprint; top is two connections away.
    fixture.file("base/file.txt", "v2");
    assert_eq!(fixture.executed(0), ["base/check", "mid/check"]);
    fixture.file("mid/file.txt", "v2");
    assert_eq!(fixture.executed(0), ["mid/check", "top/check"]);

    // A developer whose review of top reads base says so in top's fingerprint.
    let covered = json!({"script":{"command":"/bin/sh","args":[
        "-c", "cat file.txt \"$1\" | cksum | tr ' ' -", "sh", "{base}/file.txt"
    ]}});
    fixture.artifact("top", artifact("top", covered, json!({"mid":"mid"}), PASS));
    assert_eq!(fixture.executed(0), ["top/check"]);
    fixture.file("base/file.txt", "v3");
    assert_eq!(
        fixture.executed(0),
        ["base/check", "mid/check", "top/check"]
    );
}

#[test]
fn a_dependency_without_a_fingerprint_leaves_its_dependents_unkeyed() {
    let fixture = Fixture::new();
    fixture.artifact("core", json!({"name":"core","basis":true}));
    fixture.file("core/lib.txt", "v1");
    fixture.artifact(
        "api",
        artifact("api", json!({}), json!({"core":"core"}), PASS),
    );
    for _ in 0..2 {
        let run = fixture.json(&["verify", "--all"], 0);
        assert!(run["requests"][0]["key"].is_null(), "{run}");
        assert!(run["requests"][0]["fingerprint"].is_string());
        assert_eq!(run["executionsStarted"], 1);
    }
    let status = fixture.json(&["status"], 1);
    assert_eq!(status["evals"][0]["action"], "execute");
    assert!(
        status["evals"][0]["reason"]
            .as_str()
            .unwrap()
            .starts_with("Dependency core declares no fingerprint"),
        "{status}"
    );
    // Declaring one restores reuse.
    fixture.artifact("core", json!({"name":"core","basis":true,"fingerprint":{}}));
    fixture.json(&["verify", "--all"], 0);
    assert!(fixture.executed(0).is_empty());
}

#[test]
fn equal_fingerprints_of_different_artifacts_never_share_a_result() {
    let fixture = Fixture::new();
    let same = json!({"script":{"command":"/bin/echo","args":["same-output"]}});
    for name in ["first", "second"] {
        fixture.artifact(name, artifact(name, same.clone(), json!({}), PASS));
    }
    let run = fixture.json(&["verify", "--all"], 0);
    assert_eq!(run["executionsStarted"], 2);
    let requests = run["requests"].as_array().unwrap();
    assert_eq!(requests[0]["fingerprint"], requests[1]["fingerprint"]);
    assert_eq!(requests[0]["evalDefHash"], requests[1]["evalDefHash"]);
    assert_ne!(requests[0]["key"], requests[1]["key"]);
    assert!(fixture.executed(0).is_empty());
}

#[test]
fn a_tool_declaration_change_alone_reuses() {
    let fixture = Fixture::new();
    let mut declaration = artifact("app", json!({}), json!({}), PASS);
    let tool = |description: &str| {
        json!({"agentTools":{"lint":{"description":description,
            "inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false},
            "protocol":"json","command":"/bin/true","args":[]}}})
    };
    declaration["views"] = tool("Lint a file.");
    fixture.artifact("app", declaration.clone());
    fixture.file("app/file.txt", "v1");
    assert_eq!(fixture.executed(0), ["app/check"]);
    // A tool is a way of viewing the Artifact, not part of it (#85).
    declaration["views"] = tool("Lint one file and report every finding.");
    declaration["views"]["agentTools"]["lint"]["inputSchema"]["properties"]["strict"] =
        json!({"type":"boolean"});
    fixture.artifact("app", declaration);
    assert_eq!(fixture.json(&["status"], 0)["evals"][0]["action"], "reuse");
    assert!(fixture.executed(0).is_empty());
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
fn dependency_cycles_terminate() {
    let fixture = Fixture::new();
    fixture.artifact("a", artifact("a", json!({}), json!({"peer":"b"}), PASS));
    fixture.artifact("b", artifact("b", json!({}), json!({"peer":"a"}), PASS));
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
    fixture.artifact("core", json!({"name":"core","basis":true,"fingerprint":{}}));
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
