use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        let runtime = |command: &str| json!({"kind": "runtime", "command": command, "args": []});
        for (name, profile) in [
            ("green", runtime(&support::os::true_program())),
            ("red", runtime(&support::os::false_program())),
            ("broken", runtime("artifactize-missing-program")),
            ("human", json!({"kind": "human"})),
        ] {
            let mut declaration = json!({
                "name": name,
                "fingerprint":false,
                "evals": [
                    {
                        "id": "check",
                        "title": name,
                        "profile": profile,
                        "payload": {"instruction": "Check."},
                    },
                ],
            });
            if name == "green" {
                declaration["fingerprint"] =
                    json!({"script":{"command": support::os::printf_program(), "args": ["v1"]}});
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
        let mut command = Command::new(support::os::cli());
        command
            // The declarations name `true`, `false` and `printf` for PATH to find.
            .env("PATH", support::os::path())
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state);
        support::os::sign_in_as(&mut command, "alice");
        command
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self
            .command()
            .args(args)
            .arg("--json")
            .record_output()
            .unwrap();
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
        let output = fixture.command().args(&args).record_output().unwrap();
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
        .args(["verify", "human"])
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
        (vec!["verify", "--wait", "green"], 0, "GREEN"),
        (vec!["verify", "--wait", "red"], 1, "RED"),
        (vec!["verify", "--wait", "broken"], 2, "ERROR"),
        (vec!["verify", "human"], 0, "RUNNING"),
        (
            vec!["verify", "--wait", "human", "--reuse-only", "human"],
            4,
            "INCOMPLETE",
        ),
    ] {
        let saved = fixture.json(&args, code);
        assert_eq!(saved["status"], status);
        let id = saved["id"].as_str().unwrap();
        let shown = fixture.json(&["run", "show", id], 0);
        assert_eq!(shown["status"], saved["status"]);
        assert_eq!(shown["requests"], saved["requests"]);
        if status != "RUNNING" {
            fixture.json(&["run", "show", id, "--wait"], code);
        }
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
        vec!["verify", "green", "--reuse-only", "model"],
        vec!["run"],
        vec!["run", "show", run, "--timeout-ms", "5"],
        vec!["run", "show", run, "--wait", "--timeout-ms", "0"],
        vec!["run", "show", "missing"],
        vec!["monitor"],
        // Removed in 0.5.0 with the chatgpt and claude backends.
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
        .args(["verify", "--wait", "green", "--all"])
        .record_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    for args in [["help", "verify"], ["run", "--help"], ["models", "--help"]] {
        let output = fixture.command().args(args).record_output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: artifactize"));
    }
}

#[test]
fn version_matches_package() {
    let output = Command::new(support::os::cli())
        .arg("--version")
        .record_output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("artifactize {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

// Only Windows creates or suppresses a console for desktop launches.
#[cfg(windows)]
#[test]
fn redirected_or_no_console_help_never_waits_for_input() {
    for with_console in [true, false] {
        let mut command = Command::new(support::os::cli());
        support::os::console_launch(&mut command, with_console);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Keep the empty input pipe OPEN until the child has exited: an accidental read
        // cannot succeed on EOF, so completion proves it did not pause. No clock involved.
        let input = child.stdin.take().unwrap();
        let output = child.wait_with_output().unwrap();
        drop(input);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("Usage: artifactize"));
        assert!(!text.contains("Press Enter"));
    }
}

#[test]
fn tools_facade_matches_the_standalone_cli() {
    let root = support::os::tempdir();
    fs::write(
        root.path().join("notes.md"),
        "# First\nalpha\n# Next\nomega\n",
    )
    .unwrap();
    let standalone = support::os::tools_binary(&support::os::cli());
    // Use the actual trusted program for the help tool, not clap help navigation.
    let program = standalone.to_str().unwrap();
    for (args, code) in [
        (vec!["--version"], 0),
        (vec!["read", "notes.md"], 0),
        (vec!["list"], 0),
        (vec!["glob", "*.md"], 0),
        (vec!["grep", "--case-insensitive", "ALPHA"], 0),
        (vec!["section", "notes.md", "First"], 0),
        (vec!["help", program, "read"], 0),
        // These repeated flags are target-program arguments, not facade globals.
        (
            vec![
                "help", program, "--json", "--json", "--repo", "one", "--repo", "two",
            ],
            1,
        ),
        (vec!["read", "../escape"], 1),
        (vec!["section", "notes.md", "Absent"], 1),
        (vec!["read"], 2),
        (vec!["unknown"], 2),
    ] {
        let direct = Command::new(standalone)
            .current_dir(root.path())
            .args(&args)
            .record_output()
            .unwrap();
        let facade = Command::new(support::os::cli())
            .current_dir(root.path())
            .arg("tools")
            .args(&args)
            .record_output()
            .unwrap();
        assert_eq!(direct.status.code(), Some(code), "{args:?}: {direct:?}");
        assert_eq!(
            facade.status.code(),
            direct.status.code(),
            "{args:?}: {facade:?}"
        );
        assert_eq!(facade.stdout, direct.stdout, "{args:?}");
        if code != 2 {
            assert_eq!(facade.stderr, direct.stderr, "{args:?}");
        }
    }
    let help = Command::new(support::os::cli())
        .args(["tools", "--help"])
        .record_output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for name in [
        "check", "read", "list", "glob", "grep", "section", "help", "open",
    ] {
        assert!(
            text.lines().any(|line| line.trim_start().starts_with(name)),
            "{text}"
        );
        let output = Command::new(support::os::cli())
            .args(["tools", name, "--help"])
            .record_output()
            .unwrap();
        assert!(output.status.success(), "{name}: {output:?}");
    }
}

#[test]
fn help_is_displayed_without_arguments() {
    let output = Command::new(support::os::cli()).record_output().unwrap();
    let help = Command::new(support::os::cli())
        .arg("--help")
        .record_output()
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
    assert!(
        text.lines()
            .any(|line| line.trim_start().starts_with("mcp"))
    );
    assert!(
        !text
            .lines()
            .any(|line| line.trim_start().starts_with("graph")),
        "graph is hidden or removed: {text}"
    );
}

#[test]
fn graph_moved_under_config() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["config", "--help"])
        .record_output()
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
        let output = fixture.command().args(&args).record_output().unwrap();
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
        let output = Command::new(support::os::cli())
            .args(&args)
            .record_output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("unrecognized subcommand") || error.contains("unexpected argument"),
            "{error}"
        );
    }
}
