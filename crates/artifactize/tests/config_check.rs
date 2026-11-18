use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use artifactize_core::config::{ReviewRequirement, read_workspace_config};
use serde_json::{Value, json};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

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
        let parent = support::os::environment_variable("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .join("test-fixtures");
        fs::create_dir_all(&parent).unwrap();
        // Atomic unique creation, unlike clock-derived names on coarse Windows clocks.
        let directory = support::os::tempdir_in(&parent, "config-");
        Self(support::os::canonical(directory.path()), directory)
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        support::declaration::write(&self.0, path, contents).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(support::os::cli());
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
fn config_check_reports_bad_references_without_opening_runtime_inputs() {
    let fixture = Fixture::new();
    fixture.write("input/index.artf", r#"{"name":"input"}"#);
    let mut declaration = json!({
        "name":"review",
        "evals":[
            {
                "id":"run",
                "title":"Run",
                "profile":{
                    "kind":"runtime",
                    "command":"missing-command",
                    "args":["{input}/missing-file"],
                },
                "payload":{"instruction":"Read {input}."},
            },
        ],
    });
    fixture.write("review/index.artf", &declaration.to_string());
    let output = fixture
        .command()
        .args(["config", "check", "--json"])
        .record_output()
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
        .record_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error = json_output(&output)["error"].as_str().unwrap().to_owned();
    // Reference validation reports the authored logical declaration path.
    assert!(error.contains(".artifactize/review.artf"), "{error}");
    assert!(error.contains("review/run"));
    assert!(error.contains("Unknown Artifact reference {missing}"));
    declaration["evals"][0]["payload"]["instruction"] = json!("Inspect.");
    declaration["evals"][0]["profile"]["args"] = json!(["{missing}/file"]);
    fixture.write("review/index.artf", &declaration.to_string());
    let output = fixture
        .command()
        .args(["config", "check", "--json"])
        .record_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        json_output(&output)["error"]
            .as_str()
            .unwrap()
            .contains("Unknown Artifact reference {missing}")
    );
    let output = fixture
        .command()
        .args(["config", "check"])
        .record_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown Artifact reference"));
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
            .record_output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error = json_output(&output)["error"].as_str().unwrap().to_owned();
        assert!(error.contains(".artifactize/app.artf"), "{error}");
        assert!(error.ends_with(&format!(": {key} {shape}")), "{error}");
        let output = fixture
            .command()
            .args(["config", "check"])
            .record_output()
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
            .record_output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error = json_output(&output)["error"].as_str().unwrap().to_owned();
        assert!(error.contains(".artifactize/app.artf"), "{error}");
        assert!(error.ends_with(message), "{error}");
        let output = fixture
            .command()
            .args(["config", "check"])
            .record_output()
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
        ".artifactize/input.artf",
        include_str!("../../artifactize-core/tests/fixtures/declarations/.artifactize/input.artf"),
    );
    fixture.write(
        ".artifactize/review.artf",
        include_str!("../../artifactize-core/tests/fixtures/declarations/.artifactize/review.artf"),
    );
    fixture.write(
        "review/hook.sh",
        include_str!("../../artifactize-core/tests/fixtures/declarations/review/hook.sh"),
    );
    fixture.write(
        ".artifactize/unreviewed.artf",
        include_str!(
            "../../artifactize-core/tests/fixtures/declarations/.artifactize/unreviewed.artf"
        ),
    );
    fixture.write("input/.gitkeep", "");
    fixture.write("unreviewed/.gitkeep", "");
    support::os::make_executable(&fixture.0.join("review/hook.sh"));
    fs::create_dir_all(fixture.0.join(".git/ignored")).unwrap();
    fs::write(
        support::declaration::source_path(fixture.0.join(".git/ignored/index.artf")),
        "not JSON",
    )
    .unwrap();
    fs::create_dir_all(fixture.0.join("node_modules/ignored")).unwrap();
    fs::write(
        support::declaration::source_path(fixture.0.join("node_modules/ignored/index.artf")),
        "not JSON",
    )
    .unwrap();
    fixture.write("other.json", "other names are not configuration");
    // Discovery never enters a linked folder.
    support::os::link_dir(&fixture.0.join("review"), &fixture.0.join("linked-review"));

    let output = fixture
        .command()
        .args(["config", "check"])
        .record_output()
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
        .record_output()
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
    assert_eq!(
        config.artifacts["review"].path.local(),
        Some(Path::new("review"))
    );
    assert_eq!(
        config.evals[2]
            .declaration
            .payload()
            .as_ref()
            .unwrap()
            .instruction,
        "Check {input}."
    );

    fixture.write(
        "other/index.artf",
        r#"{"name":"other","evals":[{"id":"runtime","title":"Other","profile":{"kind":"human"},"payload":{"instruction":"Inspect"}}]}"#,
    );
    let config = read_workspace_config(&fixture.0).unwrap();
    assert_eq!(config.evals[0].id, "other/runtime");
    fs::create_dir_all(fixture.0.join(".artifactize/collision")).unwrap();
    fs::write(
        fixture.0.join(".artifactize/collision/review.artf"),
        "path = \"other/\"\n",
    )
    .unwrap();
    let output = fixture
        .command()
        .args(["--json", "config", "check"])
        .record_output()
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
        .record_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Duplicate Artifact name"));

    fixture.write("other/index.artf", r#"{"name":"other","review_policy":{}}"#);
    assert!(
        read_workspace_config(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains("unknown field `review_policy`")
    );
    support::declaration::remove_file(fixture.0.join("other/index.artf")).unwrap();
    let link = fixture.0.join(".artifactize/other.artf");
    support::os::file_link(&fixture.0.join(".artifactize/input.artf"), &link);
    assert!(
        read_workspace_config(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains("regular file")
    );
    support::os::remove_file_link(&link);
    fs::create_dir(&link).unwrap();
    let expected = format!("{}: ", support::os::path_text(&link));
    for args in [&["config", "check"][..], &["config", "check", "--json"][..]] {
        let output = fixture.command().args(args).record_output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        let message = if args.contains(&"--json") {
            json_output(&output)["error"].as_str().unwrap().to_owned()
        } else {
            String::from_utf8(output.stderr).unwrap()
        };
        assert!(message.contains(&expected), "{message}");
        assert!(message.contains("regular file"), "{message}");
    }

    let empty = Fixture::new();
    let output = empty
        .command()
        .args(["config", "check", "--json"])
        .record_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        json_output(&output)["error"]
            .as_str()
            .unwrap()
            .contains("config migrate")
    );
    let output = empty
        .command()
        .args(["config", "check", "--unknown", "--json"])
        .record_output()
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
                .record_output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2));
            let error = json_output(&output)["error"].as_str().unwrap().to_owned();
            assert!(error.contains(".artifactize/app.artf"), "{error}");
            assert!(
                error.contains(&format!(
                    r#"backend "{backend}" was removed in 0.5.0; use "openai", "anthropic" or "gemini" with an API key, "codex""#
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
        .record_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error = json_output(&output)["error"].as_str().unwrap().to_owned();
    assert!(error.contains(".artifactize/app.artf"), "{error}");
    assert!(
        error.contains("Eval review: result_check was removed in 0.6.0"),
        "{error}"
    );
}
