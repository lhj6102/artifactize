use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

use serde_json::{Value, json};
use support::os::{cat_program, echo_program, shell, true_program};
use tempfile::TempDir;

mod support;

struct Fixture {
    _root: TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        Self {
            repo,
            state: root.path().join("state"),
            _root: root,
        }
    }

    fn artifact(&self, folder: &str, value: Value) {
        self.file(&format!("{folder}/index.artf"), &value.to_string());
    }

    fn file(&self, path: &str, contents: &str) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        support::declaration::write(path, contents).unwrap();
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
        "name": name,
        "mounts": mounts,
        "fingerprint": fingerprint,
        "evals": [
            {
                "id":"check",
                "title":"Check",
                "profile":{"kind":"runtime","command":shell(),"args":args},
                "payload":{"instruction":"Check."},
            },
        ],
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
    let covered = json!({"script":{"command":shell(),"args":[
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
fn a_dependency_disabling_fingerprints_leaves_its_dependents_unkeyed() {
    let fixture = Fixture::new();
    fixture.artifact(
        "core",
        json!({"name":"core","basis":true,"fingerprint":false}),
    );
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
            .starts_with("Dependency core declares fingerprint: false"),
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
    let same = json!({"script":{"command":echo_program(),"args":["same-output"]}});
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
        json!({
            "agent_tools":{
                "lint":{
                    "description":description,
                    "input_schema":{
                        "type":"object",
                        "properties":{"path":{"type":"string"}},
                        "additionalProperties":false,
                    },
                    "protocol":"json",
                    "command":true_program(),
                    "args":[],
                },
            },
        })
    };
    declaration["views"] = tool("Lint a file.");
    fixture.artifact("app", declaration.clone());
    fixture.file("app/file.txt", "v1");
    assert_eq!(fixture.executed(0), ["app/check"]);
    // A tool is a way of viewing the Artifact, not part of it (#85).
    declaration["views"] = tool("Lint one file and report every finding.");
    declaration["views"]["agent_tools"]["lint"]["input_schema"]["properties"]["strict"] =
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
        "artifactsum"
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
            json!({"script":{"command":cat_program(),"args":["key"]}}),
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

#[test]
fn status_json_shows_each_evals_fingerprints_definition_hash_and_key() {
    let fixture = Fixture::new();
    fixture.artifact("core", json!({"name":"core","basis":true,"fingerprint":{}}));
    fixture.file("core/lib.txt", "v1");
    fixture.artifact(
        "loose",
        json!({"name":"loose","basis":true,"fingerprint":false}),
    );
    fixture.artifact(
        "api",
        artifact("api", json!({}), json!({"core":"core"}), PASS),
    );
    fixture.artifact(
        "web",
        artifact("web", json!({}), json!({"loose":"loose"}), PASS),
    );
    let status = fixture.json(&["status"], 1);
    let eval = |id: &str| {
        status["evals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|eval| eval["id"] == id)
            .unwrap()
            .clone()
    };
    let api = eval("api/check");
    let hash = api["evalDefHash"].as_str().unwrap();
    assert_eq!(hash.len(), 64);
    let fingerprint = api["fingerprint"].as_str().unwrap();
    assert!(fingerprint.starts_with("artifactsum:"));
    assert_eq!(api["fingerprints"]["api"], fingerprint);
    assert!(
        api["fingerprints"]["core"]
            .as_str()
            .unwrap()
            .starts_with("artifactsum:")
    );
    assert_eq!(api["key"].as_str().unwrap().len(), 64);
    // An Artifact declaring fingerprint: false shows as null and leaves no key.
    let web = eval("web/check");
    assert_eq!(web["fingerprints"]["loose"], Value::Null);
    assert!(web["fingerprints"]["web"].is_string());
    assert_eq!(web["key"], Value::Null);
    assert_eq!(
        web["evalDefHash"], hash,
        "the same strategy, another Artifact"
    );

    // They are the values verify keys the Run with.
    let run = fixture.json(&["verify", "--all"], 0);
    let request = run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|request| request["evalId"] == "api/check")
        .unwrap();
    assert_eq!(request["key"], api["key"]);
    assert_eq!(request["evalDefHash"], api["evalDefHash"]);
    assert_eq!(request["fingerprints"], api["fingerprints"]);
    // web has no key, so its result satisfies only its own Run.
    let after = fixture.json(&["status"], 1);
    assert_eq!(after["evals"][0]["id"], "api/check");
    assert_eq!(after["evals"][0]["action"], "reuse");
    assert_eq!(after["evals"][0]["key"], api["key"]);
}

#[test]
fn default_artifactsum_reuses_and_tags_appear_in_graph_status_and_saved_monitor_data() {
    let fixture = Fixture::new();
    fixture.artifact("basis", json!({"name":"basis","basis":true}));
    fixture.file("basis/rules.txt", "rules");
    let mut declaration = artifact("app", json!({}), json!({"rules":"basis"}), PASS);
    declaration.as_object_mut().unwrap().remove("fingerprint");
    declaration["tags"] = json!(["type:image", "scope:combat"]);
    fixture.artifact("app", declaration.clone());
    fixture.file("app/input.txt", "input");
    let graph = fixture.json(&["config", "graph"], 0);
    assert_eq!(graph["artifacts"]["app"]["tags"], declaration["tags"]);
    assert_eq!(graph["artifacts"]["basis"]["tags"], json!([]));
    let text = String::from_utf8(fixture.run(&["config", "graph"]).stdout).unwrap();
    assert!(
        text.contains("Artifact app [type:image, scope:combat] (app)"),
        "{text}"
    );
    let fresh = fixture.json(&["status"], 1);
    assert_eq!(fresh["artifacts"][0]["tags"], declaration["tags"]);
    assert_eq!(fresh["artifacts"][1]["tags"], json!([]));
    assert!(fresh["evals"][0]["key"].is_string());
    assert!(
        fresh["evals"][0]["fingerprints"]["basis"]
            .as_str()
            .unwrap()
            .starts_with("artifactsum:")
    );
    let text = String::from_utf8(fixture.run(&["status"]).stdout).unwrap();
    assert!(
        text.contains("Artifact app [type:image, scope:combat]: UNREVIEWED"),
        "{text}"
    );
    let first = fixture.json(&["verify", "--all"], 0);
    assert_eq!(
        first["definitions"]["artifacts"]["app"]["tags"],
        declaration["tags"]
    );
    assert_eq!(
        first["validation"]["artifacts"][0]["fingerprintKind"],
        "artifactsum"
    );
    assert_eq!(first["requests"][0]["key"], fresh["evals"][0]["key"]);
    declaration["tags"] = json!(["scope:changed"]);
    fixture.artifact("app", declaration);
    let reused = fixture.json(&["verify", "--all"], 0);
    assert_eq!(reused["executionsStarted"], 0);
    assert_eq!(first["requests"][0]["key"], reused["requests"][0]["key"]);
    fixture.file("basis/rules.txt", "changed");
    assert_eq!(fixture.executed(0), ["app/check"]);
}

#[test]
fn fingerprint_false_disables_end_of_review_checks_and_reports_the_explicit_choice() {
    let fixture = Fixture::new();
    fixture.artifact(
        "app",
        artifact(
            "app",
            json!(false),
            json!({}),
            &["-c", "echo run >> output.txt"],
        ),
    );
    for _ in 0..2 {
        let run = fixture.json(&["verify", "--all"], 0);
        assert_eq!(run["executionsStarted"], 1);
        assert!(run["requests"][0]["key"].is_null());
        assert!(run["requests"][0]["fingerprint"].is_null());
    }
    let status = fixture.json(&["status"], 1);
    assert!(
        status["evals"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("declares fingerprint: false")
    );
    let text = String::from_utf8(fixture.run(&["status"]).stdout).unwrap();
    assert!(text.contains("Artifact app:"));
    assert!(text.contains("declares fingerprint: false"));
    assert_eq!(
        fs::read_to_string(fixture.repo.join("app/output.txt")).unwrap(),
        "run\nrun\n"
    );
}

#[test]
fn config_check_rejects_true_and_nonobject_fingerprints_with_a_clear_message() {
    let fixture = Fixture::new();
    for value in [json!(true), json!(42), json!("default"), json!([])] {
        fixture.artifact("app", json!({"name":"app","fingerprint":value}));
        let output = fixture.json(&["config", "check"], 2);
        assert!(
            output["error"]
                .as_str()
                .unwrap()
                .contains("fingerprint must be false or an object"),
            "{output}"
        );
    }
}

/// Case-insensitive volumes open `Secret` and `secret` as one folder, but ignore rules
/// match names as written.
/// A rule for `secret/` therefore leaves `Secret/` in the fingerprint: a change there still
/// reviews again, rather than a differently cased name hiding it.
#[test]
fn an_ignore_rule_in_another_case_never_hides_a_change() {
    for declared in [true, false] {
        let fixture = Fixture::new();
        let fingerprint = if declared {
            json!({"ignore":["secret/"]})
        } else {
            json!({})
        };
        fixture.artifact("app", artifact("app", fingerprint, json!({}), PASS));
        if !declared {
            fixture.file(".gitignore", "secret/\n");
        }
        fixture.file("app/Secret/key.txt", "v1");
        assert_eq!(fixture.executed(0), ["app/check"]);
        assert!(fixture.executed(0).is_empty());
        // On a case-insensitive volume this write reaches the original entry through an
        // alias. The enumerated name, not the caller's spelling, still controls the matcher.
        let path = if fixture.repo.join("app/secret").exists() {
            "app/secret/key.txt"
        } else {
            "app/Secret/key.txt"
        };
        fixture.file(path, "v2");
        assert_eq!(fixture.executed(0), ["app/check"]);
    }
}
