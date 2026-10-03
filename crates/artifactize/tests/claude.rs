use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["bin", "repo", "home"] {
            fs::create_dir(root.path().join(dir)).unwrap();
        }
        fs::write(
            root.path().join("bin/claude"),
            include_str!("fixtures/claude.py"),
        )
        .unwrap();
        fs::set_permissions(
            root.path().join("bin/claude"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        fs::write(root.path().join("repo/evidence.txt"), "scoped evidence").unwrap();
        let fixture = Self { root };
        fixture.config(json!({}));
        fixture
    }

    fn config(&self, extra: Value) {
        let mut profile = json!({"kind":"agent", "backend":"claude", "model":"claude-test-exact", "reasoning":"high", "timeoutMs":15000, "maxToolCalls":1, "maxTokens":100});
        profile
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        fs::write(self.root.path().join("repo/artifactize.json"), json!({
            "name":"a", "views":{"agentTools":{"read":{"builtin":"read"}}},
            "evals":[{"id":"review", "title":"Review", "profile":profile, "payload":{"instruction":"Inspect {a} evidence."},
            "passSchema":{"type":"object","properties":{"note":{"type":"string"}},"required":["note"]}}]
        }).to_string()).unwrap();
    }

    fn command(&self, mode: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(self.root.path().join("repo"))
            .arg("--state-dir")
            .arg(self.root.path().join("state"))
            .args(["verify", "--all", "--json"])
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.path().join("bin").display()),
            )
            .env("HOME", self.root.path().join("home"))
            .env("FAKE_LOG", self.root.path().join("calls.jsonl"))
            .env("FAKE_PIDS", self.root.path().join("pids"))
            .env("FAKE_MODE", mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, mode: &str) -> Value {
        parsed(self.command(mode).output().unwrap())
    }

    fn calls(&self) -> Vec<Value> {
        fs::read_to_string(self.root.path().join("calls.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn parsed(output: Output) -> Value {
    assert!(
        matches!(output.status.code(), Some(0 | 2)),
        "status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn error(body: &Value, fragment: &str) {
    let request = &body["requests"][0];
    assert_eq!(request["status"], "ERROR", "{body}");
    assert!(
        request["error"].as_str().unwrap().contains(fragment),
        "{body}"
    );
}

#[test]
fn launch_contract_real_mcp_tools_and_deduplicated_usage() {
    let fixture = Fixture::new();
    let mut command = fixture.command("success");
    let stripped = [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "CLAUDE_CODE_EFFORT_LEVEL",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
        "CLAUDE_CODE_EXTRA_BODY",
        "CLAUDE_CODE_RETRY_WATCHDOG",
        "CLAUDE_CODE_SIMPLE",
        "CLAUDECODE",
        "CLAUDE_CODE_PLUGIN_DIRS",
        "CLAUDE_CODE_FORCE_SESSION_PERSISTENCE",
        "FALLBACK_FOR_ALL_PRIMARY_MODELS",
        "MAX_THINKING_TOKENS",
        "HTTP_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "NO_PROXY",
        "AWS_PROFILE",
        "GOOGLE_APPLICATION_CREDENTIALS",
    ];
    for name in stripped {
        command.env(name, "must-not-inherit");
    }
    command.env("CLAUDE_CODE_OAUTH_TOKEN", "fake-owner-token");
    command.env(
        "CLAUDE_CONFIG_DIR",
        fixture.root.path().join("home/.claude"),
    );
    let body = parsed(command.output().unwrap());
    let request = &body["requests"][0];
    assert_eq!(request["status"], "GREEN", "{body}");
    assert_eq!(request["toolCalls"].as_array().unwrap().len(), 1);
    assert_eq!(request["toolCalls"][0]["name"], "read_a");
    assert_eq!(request["toolCalls"][0]["isError"], false);
    assert!(
        request["toolCalls"][0]["result"]
            .as_str()
            .unwrap()
            .contains("scoped evidence")
    );
    let usage = request["usage"].as_array().unwrap();
    assert_eq!(usage.len(), 2, "{usage:?}");
    for turn in usage {
        assert_eq!(turn["usage"]["totalTokens"], 20);
    }
    assert_eq!(usage[1]["usage"]["invocationTotals"]["totalCostUsd"], 0.002);
    assert_eq!(
        usage[1]["usage"]["invocationTotals"]["usage"]["output_tokens"],
        10
    );
    let calls = fixture.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    let args: Vec<&str> = call["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    let expected = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--model",
        "claude-test-exact",
        "--effort",
        "high",
        "--tools",
        "",
        "--strict-mcp-config",
        "--mcp-config",
    ];
    assert_eq!(&args[..expected.len()], expected);
    assert!(Path::new(args[13]).is_file());
    assert_eq!(
        &args[14..25],
        [
            "--permission-mode",
            "dontAsk",
            "--setting-sources",
            "",
            "--allowedTools",
            "mcp__artifactize__*",
            "--no-session-persistence",
            "--disable-slash-commands",
            "--no-chrome",
            "--settings",
            r#"{"claudeMdExcludes":["**"],"autoMemoryEnabled":false,"disableAllHooks":true,"switchModelsOnFlag":false,"fallbackModel":[]}"#
        ]
    );
    assert_eq!(args[25], "--system-prompt");
    assert!(args[26].contains("Verdict schemas"));
    assert_eq!(args.len(), 27);
    for name in stripped {
        assert!(call["env"].get(name).is_none(), "leaked {name}");
    }
    assert_eq!(call["env"]["CLAUDE_CODE_MAX_RETRIES"], "0");
    assert_eq!(call["env"]["CLAUDE_CODE_NONSTREAMING_TIMEOUT_RETRIES"], "0");
    assert_eq!(
        call["env"]["CLAUDE_CODE_DISABLE_NONSTREAMING_FALLBACK"],
        "1"
    );
    assert_eq!(call["env"]["CLAUDE_CODE_OAUTH_TOKEN"], "fake-owner-token");
    assert_eq!(
        call["env"]["HOME"],
        fixture.root.path().join("home").to_str().unwrap()
    );
    assert!(
        call["prompt"]
            .as_str()
            .unwrap()
            .contains("mcp__artifactize__read_a")
    );
    assert!(call["cwd"].as_str().unwrap().contains("state/runs/"));
    assert!(!fixture.root.path().join("home/.claude").exists());
}

#[test]
fn repair_is_a_second_stateless_tools_disabled_invocation() {
    let fixture = Fixture::new();
    let body = fixture.run("repair");
    assert_eq!(body["requests"][0]["status"], "GREEN", "{body}");
    let calls = fixture.calls();
    assert_eq!(calls.len(), 2);
    let args = calls[1]["args"].as_array().unwrap();
    assert_eq!(args[13], r#"{"mcpServers":{}}"#);
    assert_eq!(args[10], "");
    assert_eq!(args[6], calls[0]["args"][6]);
    assert_eq!(args[8], calls[0]["args"][8]);
    let prompt = calls[1]["prompt"].as_str().unwrap();
    assert!(prompt.contains("not_json"));
    assert!(prompt.contains("scoped evidence"));
    assert!(prompt.contains("Inspect"));
    assert_eq!(
        body["requests"][0]["toolCalls"].as_array().unwrap().len(),
        1
    );
    assert_eq!(body["requests"][0]["usage"].as_array().unwrap().len(), 3);

    let fixture = Fixture::new();
    error(&fixture.run("bad-verdict"), "after one format repair");
    assert_eq!(fixture.calls().len(), 2);
    let fixture = Fixture::new();
    error(&fixture.run("repair-tools"), "init tools mismatch");
}

#[test]
fn stream_failures_and_budget_breaches_are_errors_not_verdict_repairs() {
    for (mode, message) in [
        ("bad-tools", "init tools mismatch"),
        ("missing-tools", "init tools mismatch"),
        ("bad-model", "model mismatch"),
        ("bad-model-usage", "model mismatch"),
        ("terminal-error", "account quota exhausted"),
        ("exit-error", "exit status: 9"),
        ("no-result", "terminal result"),
        ("malformed", "stream JSON"),
        ("duplicate-call", "repeated a tool-call ID"),
        ("incomplete", "incomplete"),
        ("mismatched-result", "does not match"),
        ("tool-budget", "maxToolCalls"),
        ("tool-budget-repair", "maxToolCalls"),
        ("budget-start", "maxTokens"),
        ("budget-stream", "maxTokens"),
        ("budget-result", "maxTokens"),
    ] {
        let fixture = Fixture::new();
        error(&fixture.run(mode), message);
        assert_eq!(fixture.calls().len(), 1, "unexpected repair for {mode}");
    }
    let fixture = Fixture::new();
    assert_eq!(
        fixture.run("control-tool")["requests"][0]["status"],
        "GREEN"
    );
}

#[test]
fn empty_tool_surface_and_explicit_xhigh_are_supported() {
    let fixture = Fixture::new();
    fixture.config(json!({"reasoning":"xhigh"}));
    let path = fixture.root.path().join("repo/artifactize.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["views"]["agentTools"] = json!({});
    fs::write(path, config.to_string()).unwrap();
    let body = fixture.run("success");
    assert_eq!(body["requests"][0]["status"], "GREEN", "{body}");
    assert_eq!(body["requests"][0]["toolCalls"], json!([]));
    assert_eq!(fixture.calls()[0]["args"][8], "xhigh");
}

#[test]
fn token_budget_is_shared_with_repair() {
    let fixture = Fixture::new();
    fixture.config(json!({"maxTokens":50}));
    error(&fixture.run("repair"), "maxTokens");
    assert_eq!(fixture.calls().len(), 2);
}

fn wait_file(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "missing {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_dead(pid: &str) {
    let started = Instant::now();
    loop {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"));
        if stat
            .as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            || stat.as_ref().is_ok_and(|stat| {
                stat.rsplit_once(')')
                    .unwrap()
                    .1
                    .trim_start()
                    .starts_with('Z')
            })
        {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "process {pid} still alive"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn deadline_and_ctrl_c_terminate_then_kill_the_process_group() {
    let fixture = Fixture::new();
    fixture.config(json!({"timeoutMs":400}));
    error(&fixture.run("ignore-term"), "timed out");
    for pid in fs::read_to_string(fixture.root.path().join("pids"))
        .unwrap()
        .split_whitespace()
    {
        assert_dead(pid);
    }

    let fixture = Fixture::new();
    let child = fixture.command("hang").spawn().unwrap();
    wait_file(&fixture.root.path().join("pids"));
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }
    let body = parsed(child.wait_with_output().unwrap());
    error(&body, "cancelled");
    for pid in fs::read_to_string(fixture.root.path().join("pids"))
        .unwrap()
        .split_whitespace()
    {
        assert_dead(pid);
    }
}

#[test]
fn cancellation_releases_mcp_and_its_running_tool_group() {
    let fixture = Fixture::new();
    let path = fixture.root.path().join("repo/artifactize.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["views"]["agentTools"] = json!({"slow": {"description":"Wait", "command":"/bin/sh", "args":["-c", "trap '' TERM; echo $$ > tool.pid; sleep 60"], "protocol":"plain", "inputSchema":{"type":"object"}}});
    fs::write(path, config.to_string()).unwrap();
    let child = fixture.command("slow-tool").spawn().unwrap();
    wait_file(&fixture.root.path().join("repo/tool.pid"));
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }
    error(&parsed(child.wait_with_output().unwrap()), "cancelled");
    assert_dead(
        fs::read_to_string(fixture.root.path().join("repo/tool.pid"))
            .unwrap()
            .trim(),
    );
}
