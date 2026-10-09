use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

mod support;

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let runtime = |command: &str| json!({"kind": "runtime", "command": command, "args": []});
        for (name, profile) in [
            ("green", runtime("true")),
            ("red", runtime("false")),
            ("broken", runtime("/artifactize/missing")),
            ("human", json!({"kind": "human"})),
        ] {
            let mut declaration = json!({"name": name, "fingerprint":false, "evals": [{"id": "check", "title": name,
                "profile": profile, "payload": {"instruction": "Check."}}]});
            if name == "green" {
                declaration["fingerprint"] =
                    json!({"script":{"command": "printf", "args": ["v1"]}});
            }
            fs::create_dir_all(repo.join(name)).unwrap();
            support::declaration::write(
                repo.join(name).join("index.artf"),
                declaration.to_string(),
            )
            .unwrap();
        }
        Self {
            state: root.path().join("state"),
            repo,
            _root: root,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            // The declarations name `true`, `false` and `printf` for PATH to find.
            .env("PATH", support::os::path())
            .env("USER", "alice")
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state);
        command
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.command().args(args).arg("--json").output().unwrap();
        assert_eq!(output.status.code(), Some(code), "{args:?}: {output:?}");
        parse(&output)
    }
}

#[test]
fn malformed_identity_usage_errors_precede_state_access_and_missing_keys_keep_their_codes() {
    let fixture = Fixture::new();
    for args in [
        vec!["run", "show", "../escape"],
        vec!["request", "list", "--run", ".."],
        vec!["request", "show", "a/b"],
        vec!["request", "claim", "a/b"],
        vec!["request", "unclaim", "a/b"],
        vec!["request", "tool", "a/b", "inspect"],
        vec!["request", "submit", "a/b", "--verdict", "GREEN"],
        vec!["review", "a/b"],
        vec!["cache", "show", "missing"],
        vec!["cache", "rm", "missing"],
        vec!["server", "rm", "missing"],
    ] {
        let json = fixture.json(&args, 2);
        assert!(
            json["error"].as_str().unwrap().contains("Invalid"),
            "{args:?}: {json}"
        );
        let output = fixture.command().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("invalid value")
                && stderr.contains("Invalid")
                && stderr.contains("--help"),
            "{args:?}: {stderr}"
        );
        assert!(
            !fixture.state.exists(),
            "malformed identity created state: {args:?}"
        );
    }
    let key = "f".repeat(64);
    assert_eq!(fixture.json(&["cache", "show", &key], 4), Value::Null);
    assert_eq!(
        fixture.json(&["cache", "show", &key, "--history"], 4),
        json!([])
    );
    assert_eq!(
        fixture.json(&["cache", "rm", &key], 0),
        json!({"removed":false})
    );
    assert!(!fixture.state.exists());
}

fn parse(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("{e}: {output:?}"))
}

fn waiting_request(fixture: &Fixture) -> Value {
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(10));
    loop {
        let list = fixture.json(&["request", "list"], 0);
        let mut requests = list.as_array().unwrap().iter();
        if let Some(request) = requests.find(|r| r["status"] == "WAITING_HUMAN") {
            return request.clone();
        }
        assert!(Instant::now() < deadline, "Human request did not appear");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn exit_codes_cover_outcomes_waits_and_usage_errors() {
    let fixture = Fixture::new();
    let child = fixture
        .command()
        .args(["verify", "human", "--timeout-ms", "20000"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let waiting = waiting_request(&fixture);
    let (id, run) = (
        waiting["id"].as_str().unwrap(),
        waiting["runId"].as_str().unwrap(),
    );
    let running = fixture.json(&["run", "show", run, "--wait", "--timeout-ms", "50"], 3);
    assert_eq!(running["status"], "RUNNING");
    fixture.json(&["request", "claim", id], 0);
    fixture.json(&["request", "submit", id, "--verdict", "GREEN"], 0);
    assert_eq!(
        fixture.json(&["run", "show", run, "--wait"], 0)["status"],
        "GREEN"
    );
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(0));

    for (args, code, status) in [
        (vec!["verify", "green"], 0, "GREEN"),
        (vec!["verify", "red"], 1, "RED"),
        (vec!["verify", "broken"], 2, "ERROR"),
        (
            vec!["verify", "human", "--timeout-ms", "1"],
            3,
            "INCOMPLETE",
        ),
        (
            vec!["verify", "human", "--reuse-only", "human"],
            4,
            "INCOMPLETE",
        ),
    ] {
        let saved = fixture.json(&args, code);
        assert_eq!(saved["status"], status);
        let id = saved["id"].as_str().unwrap();
        assert_eq!(fixture.json(&["run", "show", id], 0), saved);
        fixture.json(&["run", "show", id, "--wait"], code);
    }
    fixture.json(&["status", "green"], 0);
    fixture.json(&["status", "red"], 1);
    fixture.json(&["cache", "show", &"f".repeat(64)], 4);

    for args in [
        vec!["--json", "status"],
        vec!["status", "--repo", "."],
        vec!["verify"],
        vec!["verify", "green", "--all"],
        vec!["verify", "green", "--profile"],
        vec!["verify", "green", "--bogus"],
        vec!["verify", "green", "--jobs", "0"],
        // Removed: verify waits for Human results by default.
        vec!["verify", "green", "--wait"],
        vec!["verify", "green", "--reuse-only", "model"],
        vec!["run"],
        vec!["run", "show", run, "--timeout-ms", "5"],
        vec!["run", "show", run, "--wait", "--timeout-ms", "0"],
        vec!["run", "show", "missing"],
        vec!["monitor"],
        // Removed in 0.5.0 with the chatgpt and claude backends.
        vec!["mcp", "--manifest", "manifest.json"],
        vec!["login", "chatgpt"],
        vec!["logout", "chatgpt"],
        vec!["models", "chatgpt"],
        vec!["models", "claude"],
    ] {
        let error = fixture.json(&args, 2)["error"].as_str().unwrap().to_owned();
        assert!(
            !error.is_empty() && !error.contains("Usage:"),
            "{args:?}: {error}"
        );
    }
    let output = fixture
        .command()
        .args(["verify", "green", "--all"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    for args in [["help", "verify"], ["run", "--help"], ["models", "--help"]] {
        let output = fixture.command().args(args).output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: artifactize"));
    }
}

#[test]
fn version_matches_package() {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--version")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("artifactize {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_is_displayed_without_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .output()
        .unwrap();
    let help = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(help.status.success());
    assert!(output.stderr.is_empty());
    assert!(help.stderr.is_empty());
    assert_eq!(output.stdout, help.stdout);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Usage: artifactize"));
    assert!(text.contains("--repo <PATH>"));
    assert!(text.contains("--state-dir <PATH>"));
    assert!(text.contains("--json"));
    for hidden in ["graph", "mcp"] {
        assert!(
            !text
                .lines()
                .any(|line| line.trim_start().starts_with(hidden)),
            "{hidden} is hidden or removed: {text}"
        );
    }
}

#[test]
fn graph_moved_under_config() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["config", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    for command in ["check", "graph"] {
        assert!(
            text.lines()
                .any(|line| line.trim_start().starts_with(command)),
            "{text}"
        );
    }
    assert_eq!(
        fixture.json(&["config", "graph", "green"], 0)["artifacts"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    let hint = r#"graph moved to "artifactize config graph""#;
    for args in [
        vec!["graph"],
        vec!["graph", "green"],
        vec!["graph", "--compact"],
        vec!["graph", "--help"],
    ] {
        let output = fixture.command().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("{hint}\n")
        );
    }
    assert_eq!(fixture.json(&["graph", "green"], 2), json!({"error": hint}));
}

#[test]
fn dropped_commands_and_flags_are_rejected() {
    for args in [
        vec!["plan"],
        vec!["history"],
        vec!["run", "summary", "missing"],
        vec!["request", "summary", "missing"],
        vec!["request", "show", "missing", "--full"],
        vec!["request", "tool", "missing", "inspect", "--args", "{}"],
        vec!["run", "history"],
        vec!["verify", "--all", "--full"],
        vec!["run", "show", "missing", "--full"],
        vec!["verify", "--critic", "artifact/check"],
        vec!["verify", "--critics", "artifact/check"],
        vec!["verify", "--critics-file", "selection.json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("unrecognized subcommand") || error.contains("unexpected argument"),
            "{error}"
        );
    }
}
