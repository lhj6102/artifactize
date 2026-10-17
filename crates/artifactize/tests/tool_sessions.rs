//! Tool-session seams through real CLI reviews and the offline provider.
#[path = "support/run.rs"]
mod owned_run;
use owned_run::{RunChild, Watcher};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use support::{FakeProvider, openai, tool_sessions as harness};

struct Fixture {
    provider: FakeProvider,
    processes: RefCell<owned_run::Processes>,
    root: owned_run::Root,
}
impl Fixture {
    fn new(start: &str, stop: &str, action: &str, calls: usize) -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        let log = root.path().join("log");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir(&log).unwrap();
        fs::write(repo.join("harness.py"), harness::script()).unwrap();
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
        root.track(&root.path().join("state"), &repo);
        Self {
            root,
            provider,
            processes: RefCell::default(),
        }
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
        let mut command = Command::new(support::os::cli());
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
            .envs(support::os::home_variables(self.root.path().join("home")));
        command
    }
    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.command(args).record_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn records(&self, kind: &str) -> Vec<Value> {
        let records = harness::records(&self.log(), kind);
        for record in &records {
            for field in ["pid", "child"] {
                if let Some(pid) = record[field].as_u64() {
                    self.processes.borrow_mut().own(u32::try_from(pid).unwrap());
                }
            }
        }
        records
    }
    fn rewrite(&self, edit: impl FnOnce(&mut Value)) {
        let mut definition =
            support::declaration::read(fs::read(self.repo().join("index.artf")).unwrap()).unwrap();
        edit(&mut definition);
        support::declaration::write(self.repo().join("index.artf"), definition.to_string())
            .unwrap();
    }

    fn own_processes(&mut self) {
        // Every invocation writes its child marker before the structured record,
        // including failures during startup.
        if let Ok(entries) = fs::read_dir(self.log()) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with("child-")
                    && let Ok(pid) = fs::read_to_string(entry.path()).unwrap_or_default().parse()
                {
                    self.processes.get_mut().own(pid);
                }
            }
        }
        for kind in ["start", "call", "stop"] {
            self.records(kind);
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.own_processes();
        // Wake provider callbacks and blocked tools even when an assertion unwinds.
        for marker in ["release", "continue", "admit", "verdict"] {
            let _ = fs::write(self.log().join(marker), "ready");
        }
        let released = self.root.release(&self.state(), &self.repo());
        // Cancellation may start stop or finish a pending invocation. Adopt those
        // identities too before the process guard and temporary root can drop.
        self.own_processes();
        if !std::thread::panicking() {
            released.expect("release tool-session Run owners before fixture removal");
        }
    }
}

#[test]
fn declarations_reject_unknown_sessions_and_lifecycle_options_with_spans() {
    use artifactize_core::config::parse_declaration;
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
    assert!(fixture.records("start").is_empty());
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
        let run = fixture.json(&["verify", "--wait", "--all"], 2);
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
        let run = fixture.json(&["verify", "--wait", "--all"], 0);
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
        let expected: String = Sha256::digest(fs::read(fixture.repo().join("harness.py")).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        for key in ["app_app.start", "app_app.stop"] {
            assert_eq!(pins[key]["harness.py"], expected);
        }
        let reused = fixture.json(&["verify", "--wait", "--all"], 0);
        assert_eq!(reused["executionsStarted"], 0);
        assert_eq!(
            reused["requests"][0]["provenance"],
            run["requests"][0]["provenance"]
        );
        harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
    }
}

#[test]
fn session_commands_share_the_session_directory_and_run_in_the_authors_home() {
    let fixture = Fixture::new("start", "stop", "act", 1);
    let run = fixture.json(&["verify", "--wait", "--all"], 0);
    assert_eq!(run["requests"][0]["status"], "GREEN", "{run}");
    let home = fixture.root.path().join("home");
    let start = fixture.records("start").pop().unwrap();
    let call = fixture.records("call").pop().unwrap();
    let stop = fixture.records("stop").pop().unwrap();
    for record in [&start, &call, &stop] {
        assert_eq!(record["home"], support::os::path_text(&home), "{record}");
        assert_eq!(record["directory"], start["directory"], "{record}");
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
    let run = fixture.json(&["verify", "--wait", "--all"], 0);
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
    let run = fixture.json(&["verify", "--wait", "--all", "--jobs", "2"], 0);
    assert_eq!(run["executionsStarted"], 2, "{run}");
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 2);
    let starts = fixture.records("start");
    assert_ne!(starts[0]["directory"], starts[1]["directory"]);
}

#[test]
fn diagnostic_fixture_releases_detached_processes_before_removal_on_drop_and_unwind() {
    for unwind in [false, true] {
        let fixture = Fixture::new("start", "stop", "hold-tree", 0);
        let owner = fixture
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
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let owner = Watcher::new(owner);
        support::os::wait_until(Duration::from_secs(10), || {
            !fixture.records("call").is_empty()
        });
        let start = fixture.records("start").pop().unwrap();
        let call = fixture.records("call").pop().unwrap();
        let identities =
            [&start["pid"], &start["child"], &call["pid"], &call["child"]].map(|pid| {
                let pid = u32::try_from(pid.as_u64().unwrap()).unwrap();
                (pid, support::os::start_time(pid))
            });
        let root = fixture.root.path().to_owned();
        // No provider state escapes this closure: the entire fixture is consumed and its
        // thread/connection guards join before catch_unwind can expose an outcome.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            // Reap the interrupted caller first. Its detached tools still belong to the
            // fixture, even on systems where they survive their caller's abrupt exit.
            let _owned = (owner, fixture);
            assert!(!unwind, "detached diagnostic fixture unwind probe");
        }));
        assert_eq!(result.is_err(), unwind);
        assert!(
            identities
                .iter()
                .all(|&(pid, start)| !support::os::same_process_running(pid, start))
        );
        assert!(
            !root.exists(),
            "all tool children exit before fixture removal"
        );
    }
}

#[test]
fn leader_exit_is_not_restarted_and_later_calls_report_session_ended() {
    let fixture = Fixture::new("start", "stop", "crash", 2);
    let child = fixture
        .command(&["verify", "--wait", "--all"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = RunChild::new(child, &fixture.state(), &fixture.repo());
    support::os::wait_until(Duration::from_secs(10), || {
        !fixture.records("call").is_empty()
    });
    let started = fixture.records("start").pop().unwrap();
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
    for variant in ["live", "leaderless", "mismatch", "zombie"] {
        let fixture = Fixture::new("start", "stop", "hold", 0);
        let owner = fixture
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
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut owner = Watcher::new(owner);
        support::os::wait_until(Duration::from_secs(10), || {
            !fixture.records("call").is_empty()
        });
        let started = fixture.records("start").pop().unwrap();
        let leader = started["pid"].as_u64().unwrap() as u32;
        let descendant = started["child"].as_u64().unwrap() as u32;
        let directory = PathBuf::from(started["directory"].as_str().unwrap());
        support::os::kill(owner.id());
        support::os::wait_for_zombie(owner.id());
        if variant == "zombie" {
            assert!(support::os::exists(owner.id()));
            assert!(!support::os::running(owner.id()));
            // Also exercise pre-lease journal compatibility. Zombie identity must
            // not keep this abandoned directory alive merely because it exists.
            fs::remove_file(directory.join("lease")).unwrap();
        } else {
            owner.wait().unwrap();
        }
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
        if variant == "zombie" {
            assert!(
                support::os::exists(owner.id()),
                "sweep happened before reaping the owner"
            );
        }
        // Always reap here; non-zombie variants already have a cached exit status.
        owner.wait().unwrap();
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

// Unix groups survive abrupt owner death; Windows Job handles kill them instead.
#[cfg(unix)]
#[test]
fn crash_sweep_recovers_stop_and_bound_invocation_groups_with_their_descendants() {
    for stop in [false, true] {
        let fixture = Fixture::new(
            "start",
            if stop { "hang-stop" } else { "stop" },
            if stop { "act" } else { "hold-tree" },
            0,
        );
        let owner = fixture
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
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut owner = Watcher::new(owner);
        let kind = if stop { "stop" } else { "call" };
        support::os::wait_until(Duration::from_secs(10), || {
            !fixture.records(kind).is_empty()
        });
        let start = fixture.records("start").pop().unwrap();
        let invocation = fixture.records(kind).pop().unwrap();
        let directory = PathBuf::from(start["directory"].as_str().unwrap());
        let pids = [
            &start["pid"],
            &start["child"],
            &invocation["pid"],
            &invocation["child"],
        ]
        .map(|pid| pid.as_u64().unwrap() as u32);
        support::os::kill(owner.id());
        owner.wait().unwrap();
        assert!(pids.iter().all(|pid| support::os::running(*pid)));
        fixture.rewrite(|definition| {
            definition["views"]["agent_tools"]["act"]["args"][1] = json!("act");
            definition["views"]["agent_sessions"]["app"]["stop"]["args"][1] = json!("stop");
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
        support::os::wait_until(Duration::from_secs(10), || {
            pids.iter().all(|pid| !support::os::running(*pid))
        });
    }
}

#[test]
fn red_reviews_stop_and_cleanup_before_the_result_is_published() {
    let mut fixture = Fixture::new("start", "stop", "act", 0);
    fixture.provider = FakeProvider::start(|request| {
        openai::completed(
            request,
            vec![openai::message(&json!({"verdict":"RED"}).to_string())],
            openai::usage(10, 5),
        )
    });
    let run = fixture.json(&["verify", "--wait", "--all"], 1);
    assert_eq!(run["requests"][0]["status"], "RED", "{run}");
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
}

// Only Windows prevents directory deletion while a descendant denies delete sharing.
#[cfg(windows)]
#[test]
fn descendant_held_session_files_are_closed_before_green_cleanup() {
    let mut fixture = Fixture::new("start", "stop", "act", 0);
    let log = fixture.log();
    fixture.provider = FakeProvider::start(move |request| {
        support::os::wait_until(Duration::from_secs(10), || {
            fs::read_to_string(log.join("verdict")).unwrap_or_default() == "ready"
        });
        openai::completed(
            request,
            vec![openai::message(&json!({"verdict":"GREEN"}).to_string())],
            openai::usage(10, 5),
        )
    });
    let owner = fixture
        .command(&["verify", "--wait", "--all"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let owner = RunChild::new(owner, &fixture.state(), &fixture.repo());
    support::os::wait_until(Duration::from_secs(10), || {
        !fixture.records("start").is_empty()
    });
    let start = fixture.records("start").pop().unwrap();
    let directory = PathBuf::from(start["directory"].as_str().unwrap());
    let name = directory.file_name().unwrap().to_str().unwrap();
    support::os::assert_delete_blocked(&directory.join(format!("held-{name}")));
    fs::write(fixture.log().join("verdict"), "ready").unwrap();
    let output = owner.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(run["requests"][0]["status"], "GREEN");
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
}

#[test]
fn an_ended_session_retires_group_ownership_before_delayed_review_cleanup() {
    let mut fixture = Fixture::new("start", "stop", "act", 0);
    let log = fixture.log();
    let requests = AtomicUsize::new(0);
    fixture.provider = FakeProvider::start(move |request| {
        let content = if requests.fetch_add(1, Ordering::SeqCst) == 0 {
            openai::function_call("call", "act_app", &json!({}))
        } else {
            fs::write(log.join("awaiting-verdict"), "ready").unwrap();
            support::os::wait_until(Duration::from_secs(15), || {
                fs::read_to_string(log.join("verdict")).unwrap_or_default() == "ready"
            });
            openai::message(&json!({"verdict":"GREEN"}).to_string())
        };
        openai::completed(request, vec![content], openai::usage(10, 5))
    });
    let owner = fixture
        .command(&["verify", "--wait", "--all"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let owner = RunChild::new(owner, &fixture.state(), &fixture.repo());
    support::os::wait_until(Duration::from_secs(10), || {
        fs::read_to_string(fixture.log().join("awaiting-verdict")).unwrap_or_default() == "ready"
    });
    let start = fixture.records("start").pop().unwrap();
    let directory = PathBuf::from(start["directory"].as_str().unwrap());
    fs::write(directory.join("exit"), "").unwrap();
    support::os::wait_until(Duration::from_secs(10), || {
        let record: Value =
            serde_json::from_slice(&fs::read(directory.join("owner.json")).unwrap()).unwrap();
        record["leader"].is_null()
    });
    support::os::wait_until(Duration::from_secs(10), || {
        !support::os::running(start["pid"].as_u64().unwrap() as u32)
            && !support::os::running(start["child"].as_u64().unwrap() as u32)
    });
    // The verdict is still pending: retirement is not deferred to review cleanup.
    assert!(fixture.records("stop").is_empty());
    fs::write(fixture.log().join("verdict"), "ready").unwrap();
    let output = owner.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
}

#[test]
fn leader_retirement_io_error_still_cancels_bound_calls_and_runs_stop() {
    let fixture = Fixture::new("start", "stop", "ignore-term", 0);
    let owner = fixture
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let owner = Watcher::new(owner);
    support::os::wait_until(Duration::from_secs(10), || {
        !fixture.records("call").is_empty()
    });
    let start = fixture.records("start").pop().unwrap();
    let call = fixture.records("call").pop().unwrap();
    let directory = PathBuf::from(start["directory"].as_str().unwrap());
    // A directory at this filename fails the atomic retirement write on every OS,
    // including privileged CI, without denying admission of stop or bound calls.
    fs::create_dir(directory.join("owner.next")).unwrap();
    fs::write(directory.join("exit"), "").unwrap();
    let output = owner.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert!(!report["checks"].as_array().unwrap().last().unwrap()["message"].is_null());
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
    support::os::wait_until(Duration::from_secs(10), || {
        !support::os::running(call["pid"].as_u64().unwrap() as u32)
            && !support::os::running(call["child"].as_u64().unwrap() as u32)
    });
}

// Only Unix SIGSTOP can freeze the owner from the interrupted bound tool before
// its grace timer fires; Windows Job owner-death behavior is covered separately.
#[cfg(unix)]
#[test]
fn crash_during_failed_leader_retirement_keeps_bound_call_journals_recoverable() {
    let fixture = Fixture::new("start", "stop", "freeze-cleanup", 0);
    let owner = fixture
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
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut owner = Watcher::new(owner);
    fs::write(fixture.log().join("owner"), owner.id().to_string()).unwrap();
    support::os::wait_until(Duration::from_secs(10), || {
        !fixture.records("call").is_empty()
    });
    let start = fixture.records("start").pop().unwrap();
    let call = fixture.records("call").pop().unwrap();
    let directory = PathBuf::from(start["directory"].as_str().unwrap());
    fs::create_dir(directory.join("owner.next")).unwrap();
    fs::write(directory.join("exit"), "").unwrap();
    support::os::wait_until(Duration::from_secs(10), || {
        fs::read_to_string(fixture.log().join("interrupted")).unwrap_or_default()
            == call["pid"].as_u64().unwrap().to_string()
    });
    let bound = call["pid"].as_u64().unwrap() as u32;
    let descendant = call["child"].as_u64().unwrap() as u32;
    assert!(support::os::running(bound));
    assert!(directory.join(format!("process-{bound}.json")).is_file());
    assert!(directory.join("owner.json").is_file());
    support::os::kill(owner.id());
    support::os::wait_for_zombie(owner.id());
    assert!(directory.exists());
    assert!(support::os::running(bound));
    fixture
        .rewrite(|definition| definition["views"]["agent_tools"]["act"]["args"][1] = json!("act"));
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
    assert!(
        support::os::exists(owner.id()),
        "recovery happened before owner reaping"
    );
    owner.wait().unwrap();
    assert!(!directory.exists());
    support::os::wait_until(Duration::from_secs(10), || {
        !support::os::running(bound) && !support::os::running(descendant)
    });
    assert!(harness::directories(&fixture.state()).is_empty());
}

#[test]
fn bound_call_admission_io_failure_never_executes_the_tool_or_skips_session_cleanup() {
    let mut fixture = Fixture::new("start", "stop", "act", 0);
    let log = fixture.log();
    let calls = AtomicUsize::new(0);
    fixture.provider = FakeProvider::start(move |request| {
        let content = if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            support::os::wait_until(Duration::from_secs(10), || {
                fs::read_to_string(log.join("admit")).unwrap_or_default() == "ready"
            });
            openai::function_call("failed_call", "act_app", &json!({}))
        } else {
            fs::write(log.join("rejected"), "rejected").unwrap();
            support::os::wait_until(Duration::from_secs(10), || {
                fs::read_to_string(log.join("verdict")).unwrap_or_default() == "ready"
            });
            openai::message(&json!({"verdict":"GREEN"}).to_string())
        };
        openai::completed(request, vec![content], openai::usage(10, 5))
    });
    let owner = fixture
        .command(&["verify", "--wait", "--all"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let owner = RunChild::new(owner, &fixture.state(), &fixture.repo());
    support::os::wait_until(Duration::from_secs(10), || {
        !fixture.records("start").is_empty()
    });
    let start = fixture.records("start").pop().unwrap();
    let directory = PathBuf::from(start["directory"].as_str().unwrap());
    let denied = support::os::deny_creates(&directory);
    fs::write(fixture.log().join("admit"), "ready").unwrap();
    support::os::wait_until(Duration::from_secs(10), || {
        fs::read_to_string(fixture.log().join("rejected")).unwrap_or_default() == "rejected"
    });
    assert!(fixture.records("call").is_empty());
    drop(denied);
    fs::write(fixture.log().join("verdict"), "ready").unwrap();
    let output = owner.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fixture.provider.requests()[1]
            .body
            .to_string()
            .contains("Agent tool execution failed.")
    );
    harness::assert_cleaned(&fixture.state(), &fixture.log(), 1);
}
