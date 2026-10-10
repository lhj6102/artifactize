use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use artifactize::config::{ReviewRequirement, read_workspace_config};
use serde_json::{Value, json};

mod support;

struct Fixture(
    PathBuf,
    #[expect(
        dead_code,
        reason = "keep the fixture directory alive until the test ends"
    )]
    tempfile::TempDir,
);

impl Fixture {
    fn new() -> Self {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-fixtures");
        fs::create_dir_all(&parent).unwrap();
        // Atomic unique creation, unlike clock-derived names on coarse Windows clocks.
        let directory = support::os::tempdir_in(&parent, "config-");
        Self(support::os::canonical(directory.path()), directory)
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        support::declaration::write(path, contents).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .current_dir(&self.0)
            .env("ARTIFACTIZE_STATE_HOME", self.0.join("state-home"));
        command
    }
}

fn json_output(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn config_check_reports_bad_mounts_and_references_without_opening_runtime_inputs() {
    let fixture = Fixture::new();
    fixture.write("input/index.artf", r#"{"name":"input"}"#);
    let mut declaration = json!({
        "name":"review",
        "mounts":{"source":"input"},
        "evals":[
            {
                "id":"run",
                "title":"Run",
                "profile":{
                    "kind":"runtime",
                    "command":"missing-command",
                    "args":["{source}/missing-file"],
                },
                "payload":{"instruction":"Read {source}."},
            },
        ],
    });
    fixture.write("review/index.artf", &declaration.to_string());
    let output = fixture
        .command()
        .args(["config", "check", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        json_output(&output),
        json!({"ok":true,"artifacts":2,"evals":1})
    );
    declaration["evals"][0]["payload"]["instruction"] = json!("Unknown {missing}.");
    fixture.write("review/index.artf", &declaration.to_string());
    let output = fixture
        .command()
        .args(["config", "check", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error = json_output(&output)["error"].as_str().unwrap().to_owned();
    // Reference validation reports the authored logical declaration path (forward slashes),
    // unlike discovery errors that report a physical directory entry with native separators.
    assert!(error.contains("review/index.artf"), "{error}");
    assert!(error.contains("review/run"));
    assert!(error.contains("Unknown Artifact reference {missing}"));
    declaration["evals"][0]["payload"]["instruction"] = json!("Inspect.");
    declaration["evals"][0]["profile"]["args"] = json!(["{missing}/file"]);
    fixture.write("review/index.artf", &declaration.to_string());
    let output = fixture
        .command()
        .args(["config", "check", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        json_output(&output)["error"]
            .as_str()
            .unwrap()
            .contains("Unknown Artifact reference {missing}")
    );
    declaration["mounts"]["source"] = json!("missing");
    fixture.write("review/index.artf", &declaration.to_string());
    let output = fixture
        .command()
        .args(["config", "check"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown mount target"));
    assert!(!fixture.0.join("state-home").exists());
}

#[test]
fn config_check_rejects_renamed_fingerprint_keys_with_the_new_shape() {
    let shape = r#"was renamed to fingerprint: use fingerprint = { files = ["."], ignore = [] } or [fingerprint.script]."#;
    for (key, value) in [
        ("stale_key", json!({"content":{"inputs":["."]}})),
        (
            "stale_key",
            json!({"script":{"command":"./hash.sh","args":[]}}),
        ),
        ("stale", json!({"kind":"content"})),
    ] {
        let fixture = Fixture::new();
        let mut declaration = json!({"name":"app"});
        declaration[key] = value;
        fixture.write("app/index.artf", &declaration.to_string());
        let output = fixture
            .command()
            .args(["config", "check", "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error = json_output(&output)["error"].as_str().unwrap().to_owned();
        assert!(error.contains("app/index.artf"), "{error}");
        assert!(error.ends_with(&format!(": {key} {shape}")), "{error}");
        let output = fixture
            .command()
            .args(["config", "check"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains(&format!("{key} {shape}")));
    }
}

#[test]
fn config_check_rejects_removed_family_declarations_without_opening_instance_lists() {
    let message = "family was removed in 0.9.0; declare each instance as its own Artifact.";
    for value in [
        json!({"instances":"missing.json"}),
        json!({"instances":{"one":{}}}),
        json!(false),
    ] {
        let fixture = Fixture::new();
        fixture.write(
            "app/index.artf",
            &json!({"name":"app","family":value}).to_string(),
        );
        fixture.write("app/nested/index.artf", r#"{"name":"nested"}"#);
        let output = fixture
            .command()
            .args(["config", "check", "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error = json_output(&output)["error"].as_str().unwrap().to_owned();
        assert!(error.contains("app/index.artf"), "{error}");
        assert!(error.ends_with(message), "{error}");
        let output = fixture
            .command()
            .args(["config", "check"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(message));
        assert!(!fixture.0.join("state-home").exists());
    }
}

#[test]
fn config_check_is_static_strict_and_uses_the_supplied_workspace() {
    let fixture = Fixture::new();
    fixture.write(
        "input/index.artf",
        include_str!("fixtures/declarations/input/index.artf"),
    );
    fixture.write(
        "review/index.artf",
        include_str!("fixtures/declarations/review/index.artf"),
    );
    fixture.write(
        "review/hook.sh",
        include_str!("fixtures/declarations/review/hook.sh"),
    );
    fixture.write(
        "unreviewed/index.artf",
        include_str!("fixtures/declarations/unreviewed/index.artf"),
    );
    support::os::make_executable(&fixture.0.join("review/hook.sh"));
    fixture.write(".git/ignored/index.artf", "not JSON");
    fixture.write("node_modules/ignored/index.artf", "not JSON");
    fixture.write("other.json", "other names are not configuration");
    // Discovery never enters a linked folder.
    support::os::link_dir(&fixture.0.join("review"), &fixture.0.join("linked-review"));

    let output = fixture
        .command()
        .args(["config", "check"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Folder configuration is valid.\n");
    assert!(output.stderr.is_empty());

    let elsewhere = Fixture::new();
    let output = fixture
        .command()
        .current_dir(&elsewhere.0)
        .arg("--repo")
        .arg(&fixture.0)
        .args(["config", "check", "--json", "--state-dir", "receipts"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        json_output(&output),
        json!({"ok": true, "artifacts": 3, "evals": 3})
    );
    assert!(!fixture.0.join("review/hook-executed").exists());
    assert!(!fixture.0.join("hook-executed").exists());
    assert!(!fixture.0.join("state-home").exists());
    assert!(!fixture.0.join("receipts").exists());

    let config = read_workspace_config(&fixture.0).unwrap();
    assert_eq!(
        config.review_requirement("input"),
        Some(ReviewRequirement::Basis)
    );
    assert_eq!(
        config.review_requirement("unreviewed"),
        Some(ReviewRequirement::Unreviewed)
    );
    assert_eq!(
        config.review_requirement("review"),
        Some(ReviewRequirement::Evals)
    );
    assert_eq!(config.review_requirement("missing"), None);
    assert_eq!(
        config
            .evals
            .iter()
            .map(|eval| eval.id.as_str())
            .collect::<Vec<_>>(),
        ["review/agent", "review/human", "review/runtime"]
    );
    assert_eq!(config.artifacts["review"].path, Path::new("review"));
    assert_eq!(
        config.evals[2]
            .declaration
            .payload()
            .as_ref()
            .unwrap()
            .instruction,
        "Check {source}."
    );

    fixture.write(
        "other/index.artf",
        r#"{"name":"other","evals":[{"id":"runtime","title":"Other","profile":{"kind":"human"},"payload":{"instruction":"Inspect"}}]}"#,
    );
    let config = read_workspace_config(&fixture.0).unwrap();
    assert_eq!(config.evals[0].id, "other/runtime");
    fixture.write("other/index.artf", r#"{"name":"review"}"#);
    let output = fixture
        .command()
        .args(["--json", "config", "check"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        json_output(&output)["error"]
            .as_str()
            .unwrap()
            .contains("Duplicate Artifact name: review")
    );
    let output = fixture
        .command()
        .args(["config", "check"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Duplicate Artifact name"));

    fixture.write("other/index.artf", r#"{"name":"other","review_policy":{}}"#);
    assert!(
        read_workspace_config(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains("repository root")
    );
    fs::remove_file(fixture.0.join("other/index.artf")).unwrap();
    if support::os::symlink_file(
        fixture.0.join("input/index.artf"),
        fixture.0.join("other/index.artf"),
    )
    .is_some()
    {
        assert!(
            read_workspace_config(&fixture.0)
                .unwrap_err()
                .to_string()
                .contains("regular file")
        );
        fs::remove_file(fixture.0.join("other/index.artf")).unwrap();
    }
    fs::create_dir(fixture.0.join("other/index.artf")).unwrap();
    assert!(
        read_workspace_config(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains("regular file")
    );

    let empty = Fixture::new();
    let output = empty
        .command()
        .args(["config", "check", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        json_output(&output)["error"]
            .as_str()
            .unwrap()
            .contains("at least one .artf")
    );
    let output = empty
        .command()
        .args(["config", "check", "--unknown", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        json_output(&output)["error"]
            .as_str()
            .unwrap()
            .contains("--unknown")
    );
}

#[test]
fn config_check_names_the_replacements_for_removed_backends() {
    for backend in ["chatgpt", "claude"] {
        let agent = json!({"kind":"agent","backend":backend,"model":"m"});
        let openai = json!({"kind":"agent","backend":"openai","model":"m"});
        for (profile, variants) in [(agent.clone(), json!({})), (openai, json!({"old":agent}))] {
            let fixture = Fixture::new();
            fixture.write(
                "app/index.artf",
                &json!({"name":"app","evals":[{
                    "id":"review","title":"Review","profile":profile,"profile_variants":variants,
                    "payload":{"instruction":"Review."}
                }]})
                .to_string(),
            );
            let output = fixture
                .command()
                .args(["config", "check", "--json"])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2));
            let error = json_output(&output)["error"].as_str().unwrap().to_owned();
            assert!(error.contains("app/index.artf"), "{error}");
            assert!(
                error.contains(&format!(
                    r#"backend "{backend}" was removed in 0.5.0; use "openai" or "anthropic" with an API key, or "codex""#
                )),
                "{error}"
            );
        }
    }
}

#[test]
fn config_check_names_the_removal_of_result_check() {
    let fixture = Fixture::new();
    fixture.write(
        "app/index.artf",
        &json!({"name":"app","evals":[{
            "id":"review","title":"Review",
            "profile":{"kind":"agent","backend":"openai","model":"m"},
            "payload":{"instruction":"Review."},
            "result_check":{"command":support::os::python_program(),"args":["check.py"]}
        }]})
        .to_string(),
    );
    let output = fixture
        .command()
        .args(["config", "check", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error = json_output(&output)["error"].as_str().unwrap().to_owned();
    assert!(error.contains("app/index.artf"), "{error}");
    assert!(
        error.contains("Eval review: result_check was removed in 0.6.0"),
        "{error}"
    );
}

#[test]
fn artfignore_keeps_folders_out_of_discovery() {
    let fixture = Fixture::new();
    fixture.write("app/index.artf", r#"{"name":"app"}"#);
    // A test fixture that reuses app's name would break discovery.
    fixture.write("app/tests/fixtures/index.artf", r#"{"name":"app"}"#);
    fixture.write("examples/demo/index.artf", r#"{"name":"demo"}"#);
    let error = read_workspace_config(&fixture.0).unwrap_err().to_string();
    assert!(error.contains("Duplicate Artifact name: app"), "{error}");

    fixture.write(
        ".artfignore",
        "# fixtures and examples\napp/tests/\nexamples/\n",
    );
    let config = read_workspace_config(&fixture.0).unwrap();
    assert!(config.artifacts.contains_key("app"));
    assert!(!config.artifacts.contains_key("demo"));
    assert!(config.artifacts["app"].children.is_empty());
}
