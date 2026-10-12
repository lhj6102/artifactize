//! Tool-session seams through real CLI reviews and the offline provider.
mod support;

use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use support::{FakeProvider, openai, tool_sessions as harness};

struct Fixture {
    root: tempfile::TempDir,
    provider: FakeProvider,
}
impl Fixture {
    fn new(start: &str, stop: &str, action: &str, calls: usize) -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        let log = root.path().join("log");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir(&log).unwrap();
        fs::write(repo.join("harness.py"), harness::HARNESS).unwrap();
        let mut views = harness::views(&log, start, stop, action);
        views["agent_sessions"]["app"]["start"]["execution_paths"] = json!(["harness.py"]);
        views["agent_sessions"]["app"]["stop"]["execution_paths"] = json!(["harness.py"]);
        support::declaration::write(repo.join("index.artf"), json!({
            "name":"app", "fingerprint":{}, "views":views,
            "evals":[{"id":"review", "title":"Review", "profile":{"kind":"agent", "backend":"openai", "model":"fake-model"}, "payload":{"instruction":"Review."}}]
        }).to_string()).unwrap();
        let count = AtomicUsize::new(0);
        let provider = FakeProvider::start(move |request| {
            let index = count.fetch_add(1, Ordering::SeqCst);
            let result = if index < calls {
                openai::function_call(&format!("call_{index}"), "act_app", &json!({}))
            } else {
                openai::message(&json!({"verdict":"GREEN"}).to_string())
            };
            openai::completed(request, vec![result], openai::usage(10, 5))
        });
        Self { root, provider }
    }
    fn repo(&self) -> PathBuf {
        self.root.path().join("repo")
    }
    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }
    fn log(&self) -> PathBuf {
        self.root.path().join("log")
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(self.repo())
            .arg("--state-dir")
            .arg(self.state())
            .args(args)
            .arg("--json")
            .env("ARTIFACTIZE_OPENAI_BASE_URL", self.provider.openai_base())
            .env("OPENAI_API_KEY", "fake-key")
            .env("ARTIFACTIZE_REMOTE", "off")
            .env(support::os::home_env(), self.root.path().join("home"));
        command
    }
    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.command(args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn rewrite(&self, edit: impl FnOnce(&mut Value)) {
        let mut definition =
            support::declaration::read(fs::read(self.repo().join("index.artf")).unwrap()).unwrap();
        edit(&mut definition);
        support::declaration::write(self.repo().join("index.artf"), definition.to_string())
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::write(self.log().join("release"), "");
    }
}

#[test]
fn declarations_reject_unknown_sessions_and_lifecycle_options_with_spans() {
    use artifactize::config::parse_declaration;
    let unknown = "name = 'app'\nbasis = true\n[views.agent_tools.act]\ndescription = 'act'\ncommand = 'harness'\nargs = []\nprotocol = 'json'\nsession = 'missing'\n";
    let error = parse_declaration(unknown).unwrap_err();
    assert!(error.contains("Unknown tool session: missing"), "{error}");
    assert!(error.contains("8 |") || error.contains("line 8"), "{error}");
    let extra = "name = 'app'\nbasis = true\n[views.agent_sessions.app]\nstart = { command = 'harness', args = [], timeout_ms = 1 }\nstop = { command = 'harness', args = [] }\n";
    let error = parse_declaration(extra).unwrap_err();
    assert!(error.contains("unknown field `timeout_ms`"), "{error}");
    assert!(error.contains("4 |") || error.contains("line 4"), "{error}");
}

#[test]
fn static_check_resolves_start_without_running_it() {
    let fixture = Fixture::new("start", "stop", "act", 0);
    fixture.rewrite(|definition| {
        definition["views"]["agent_sessions"]["app"]["start"]["command"] =
            json!("missing-tool-session-program")
    });
    let report = fixture.json(&["tools", "check"], 1);
    assert!(
        report.to_string().contains("missing-tool-session-program"),
        "{report}"
    );
    assert!(harness::records(&fixture.log(), "start").is_empty());
    assert!(!fixture.state().join("tool-sessions").exists());
    assert!(fixture.provider.requests().is_empty());
}

#[test]
fn start_failure_never_calls_provider_and_stops_sessions_already_started() {
    for second in [false, true] {
        let fixture = Fixture::new(if second { "start" } else { "exit" }, "stop", "act", 0);
        if second {
            fixture.rewrite(|definition| {
                let mut session = definition["views"]["agent_sessions"]["app"].clone();
                session["start"]["command"] = json!("missing-second-session-program");
                definition["views"]["agent_sessions"]["second"] = session;
                let mut tool = definition["views"]["agent_tools"]["act"].clone();
                tool["session"] = json!("second");
                definition["views"]["agent_tools"]["z_second"] = tool;
            });
        }
        let run = fixture.json(&["verify", "--all"], 2);
        assert_eq!(
            run["requests"][0]["errorCode"], "PREPARATION_FAILED",
            "{run}"
        );
        assert!(fixture.provider.requests().is_empty());
        harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
    }
}

#[test]
fn two_calls_share_one_started_session_and_cleanup_precedes_green_publication() {
    for action in ["act", "plain"] {
        let fixture = Fixture::new("start", "fail-stop", action, 2);
        let run = fixture.json(&["verify", "--all"], 0);
        assert_eq!(run["requests"][0]["status"], "GREEN", "{run}");
        harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
        let requests = fixture.provider.requests();
        assert_eq!(requests.len(), 3);
        let outputs: Vec<_> = requests[2].body["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|value| value["type"] == "function_call_output")
            .collect();
        assert_eq!(outputs.len(), 2);
        assert!(
            outputs
                .iter()
                .all(|value| value.to_string().contains("started first"))
        );
        let pins = &run["requests"][0]["provenance"]["executionPaths"];
        for key in ["app_app.start", "app_app.stop"] {
            assert_eq!(pins[key]["harness.py"].as_str().unwrap().len(), 64);
        }
        let reused = fixture.json(&["verify", "--all"], 0);
        assert_eq!(reused["executionsStarted"], 0);
        assert_eq!(
            reused["requests"][0]["provenance"],
            run["requests"][0]["provenance"]
        );
        harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
    }
}

#[test]
fn execute_diagnostic_and_follow_up_get_their_own_sessions() {
    let mut fixture = Fixture::new("start", "stop", "act", 0);
    fixture.json(
        &[
            "tools",
            "check",
            "--execute",
            "--artifact",
            "app",
            "--audience",
            "agent",
            "--tool",
            "act",
        ],
        0,
    );
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
    assert!(fixture.provider.requests().is_empty());
    let run = fixture.json(&["verify", "--all"], 0);
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 2);
    let id = run["requests"][0]["id"].as_str().unwrap();
    let calls = AtomicUsize::new(0);
    fixture.provider = FakeProvider::start(move |request| {
        let result = if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            openai::function_call("followup_call", "act_app", &json!({}))
        } else {
            openai::message("Evidence from a fresh tool session.")
        };
        openai::completed(request, vec![result], openai::usage(10, 5))
    });
    fixture.json(&["session", "send", id, "Explain."], 0);
    assert_eq!(fixture.provider.requests().len(), 2);
    assert!(
        fixture.provider.requests()[1]
            .body
            .to_string()
            .contains("started first")
    );
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 3);
}

#[test]
fn concurrent_reviews_have_distinct_sessions_and_both_cleanup() {
    let mut fixture = Fixture::new("start", "stop", "act", 0);
    fixture.rewrite(|definition| {
        let mut eval = definition["evals"][0].clone();
        eval["id"] = json!("review2");
        eval["payload"]["instruction"] = json!("Independent review.");
        definition["evals"].as_array_mut().unwrap().push(eval);
    });
    let log = fixture.log();
    fixture.provider = FakeProvider::start(move |request| {
        support::os::wait_until(Duration::from_secs(10), || {
            harness::records(&log, "start").len() == 2
        });
        openai::completed(
            request,
            vec![openai::message(&json!({"verdict":"GREEN"}).to_string())],
            openai::usage(10, 5),
        )
    });
    let run = fixture.json(&["verify", "--all", "--jobs", "2"], 0);
    assert_eq!(run["executionsStarted"], 2, "{run}");
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 2);
    let starts = harness::records(&fixture.log(), "start");
    assert_ne!(starts[0]["directory"], starts[1]["directory"]);
}

#[test]
fn leader_exit_is_not_restarted_and_later_calls_report_session_ended() {
    let fixture = Fixture::new("start", "stop", "crash", 2);
    let mut child = fixture
        .command(&["verify", "--all"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    support::os::wait_until(Duration::from_secs(10), || {
        !harness::records(&fixture.log(), "call").is_empty()
    });
    let started = harness::records(&fixture.log(), "start").pop().unwrap();
    support::os::wait_until(Duration::from_secs(10), || {
        !support::os::running(started["pid"].as_u64().unwrap() as u32)
    });
    support::os::wait_until(Duration::from_secs(10), || {
        !support::os::running(started["child"].as_u64().unwrap() as u32)
    });
    fs::write(fixture.log().join("continue"), "").unwrap();
    assert!(child.wait().unwrap().success());
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[2]
            .body
            .to_string()
            .contains("The tool session `app` ended.")
    );
}

// Unix groups can outlive an abruptly killed owner; Windows Jobs die with it.
#[cfg(unix)]
#[test]
fn next_session_start_sweeps_live_or_leaderless_orphans_but_never_reused_leaders() {
    for variant in ["live", "leaderless", "mismatch"] {
        let fixture = Fixture::new("start", "stop", "hold", 0);
        let mut owner = fixture
            .command(&[
                "tools",
                "check",
                "--execute",
                "--artifact",
                "app",
                "--audience",
                "agent",
                "--tool",
                "act",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        support::os::wait_until(Duration::from_secs(10), || {
            !harness::records(&fixture.log(), "call").is_empty()
        });
        let started = harness::records(&fixture.log(), "start").pop().unwrap();
        let leader = started["pid"].as_u64().unwrap() as u32;
        let descendant = started["child"].as_u64().unwrap() as u32;
        let directory = PathBuf::from(started["directory"].as_str().unwrap());
        support::os::kill(owner.id());
        owner.wait().unwrap();
        assert!(support::os::running(descendant));
        if variant == "leaderless" {
            support::os::kill(leader);
            support::os::wait_until(Duration::from_secs(10), || !support::os::running(leader));
        }
        if variant == "mismatch" {
            let path = directory.join("owner.json");
            let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            record["leader"]["start_time"] =
                json!(record["leader"]["start_time"].as_u64().unwrap() + 1);
            fs::write(path, record.to_string()).unwrap();
        }
        fixture.rewrite(|definition| {
            definition["views"]["agent_tools"]["act"]["args"][1] = json!("act")
        });
        fixture.json(
            &[
                "tools",
                "check",
                "--execute",
                "--artifact",
                "app",
                "--audience",
                "agent",
                "--tool",
                "act",
            ],
            0,
        );
        assert!(!directory.exists());
        assert!(harness::directories(&fixture.state()).is_empty());
        if variant == "mismatch" {
            assert!(support::os::running(leader));
            assert!(support::os::running(descendant));
            support::os::kill_tree(leader);
        }
        support::os::wait_until(Duration::from_secs(10), || {
            !support::os::running(descendant)
        });
    }
}
