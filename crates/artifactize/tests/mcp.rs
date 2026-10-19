//! B1: protocol/lifecycle and one table of CLI-equivalent inspect projections.
#[path = "support/run.rs"]
mod owned_run;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use artifactize_core::{
    runs, store,
    types::{RunId, RunStatus},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
};

// Independent public transport limits, not imports from the implementation under test.
const RESPONSE_BYTES: usize = 64 * 1024;
const PAGE_ENTRIES: usize = 20;
const COMPACT_BYTES: usize = 2048;
// Failure watchdogs only: readiness is always established by state or marker contents.
const FIXTURE_BOUND: Duration = Duration::from_secs(30);
const PROTOCOL_BOUND: Duration = Duration::from_secs(45);

struct Project {
    _root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
}
impl Project {
    fn new(evals: Value) -> Self {
        let root = support::os::tempdir();
        let repo = support::os::canonical(root.path()).join("repo");
        fs::create_dir(&repo).unwrap();
        fs::write(repo.join("content.txt"), "review input").unwrap();
        support::declaration::write(
            repo.join("index.artf"),
            json!({"name":"app","fingerprint":{"files":["content.txt"]},"evals":evals}).to_string(),
        )
        .unwrap();
        let state = root.path().join("state");
        let root = owned_run::Root::from(root);
        root.track(&state, &repo);
        Self {
            repo,
            state,
            _root: root,
        }
    }
    fn command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(support::os::cli());
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .env("ARTIFACTIZE_REMOTE", "off");
        command
    }
    fn context(&self) -> runs::Context {
        runs::Context::new(&self.repo, Some(&self.state), &support::os::cli()).unwrap()
    }
    async fn settled(&self, run: &RunId) -> store::RunView {
        owned_run::terminal(&self.state, run).await
    }
    async fn state(
        &self,
        run: &RunId,
        eval: &str,
        expected: artifactize_core::types::RequestStatus,
    ) {
        owned_run::subscribed(&self.state, async |changes| {
            tokio::time::timeout(support::os::patience(FIXTURE_BOUND), async {
                loop {
                    let view = store::read_run(&self.state, run).await.unwrap();
                    if view
                        .requests
                        .iter()
                        .any(|request| request.eval_id == eval && request.status() == expected)
                    {
                        return;
                    }
                    assert!(view.run.status() == RunStatus::Running);
                    changes.next_reconciled().await;
                }
            })
            .await
            .unwrap();
        })
        .await;
    }
}
async fn marker(path: &std::path::Path, expected: &str) {
    tokio::time::timeout(support::os::patience(FIXTURE_BOUND), async {
        while fs::read_to_string(path).ok().as_deref() != Some(expected) {
            // Poll contents rather than file existence: creation can precede the write.
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("controlled runtime saved its ready marker");
}

struct Client {
    child: support::os::PipedTestChild,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
    modern: bool,
}
impl Client {
    async fn new(mut command: std::process::Command, version: &str) -> Self {
        command.arg("mcp");
        let mut child = support::os::spawn_piped_test(command);
        let input = child.take_stdin();
        let output = BufReader::new(child.take_stdout());
        let mut client = Self {
            child,
            input,
            output,
            id: 0,
            modern: version == "2026-07-28",
        };
        if client.modern {
            let discovered = client.rpc("server/discover", json!({})).await;
            assert!(
                discovered["result"]["supportedVersions"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(version))
            );
            assert_eq!(discovered["result"]["capabilities"], json!({"tools":{}}));
        } else {
            let initialized = client.rpc("initialize", json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"seam","version":"1"}})).await;
            assert_eq!(initialized["result"]["protocolVersion"], version);
            assert_eq!(initialized["result"]["capabilities"], json!({"tools":{}}));
            client
                .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                .await;
        }
        client
    }
    async fn send(&mut self, value: Value) {
        self.input
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
        self.input.flush().await.unwrap();
    }
    async fn request(&mut self, method: &str, mut params: Value) -> u64 {
        self.id += 1;
        if self.modern {
            let progress_token = params["_meta"]["progressToken"].clone();
            params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28", "io.modelcontextprotocol/clientInfo":{"name":"seam","version":"1"}, "io.modelcontextprotocol/clientCapabilities":{}});
            if !progress_token.is_null() {
                params["_meta"]["progressToken"] = progress_token;
            }
        }
        self.send(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}))
            .await;
        self.id
    }
    async fn response(&mut self, id: u64) -> Value {
        let mut line = String::new();
        tokio::time::timeout(
            support::os::patience(PROTOCOL_BOUND),
            self.output.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            line.len() <= RESPONSE_BYTES,
            "bounded protocol result: {} bytes",
            line.len()
        );
        let response: Value = serde_json::from_str(&line).expect("stdout carries protocol only");
        assert_eq!(
            response["id"], id,
            "unexpected response order (verify_wait must remain pending before fixture settlement): {response}"
        );
        response
    }
    async fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.request(method, params).await;
        self.response(id).await
    }
    fn tool_result(name: &str, response: Value) -> Value {
        assert!(response.get("error").is_none(), "{name}: {response}");
        assert_ne!(response["result"]["isError"], true, "{name}: {response}");
        response["result"]["structuredContent"].clone()
    }
    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let response = self
            .rpc("tools/call", json!({"name":name,"arguments":arguments}))
            .await;
        Self::tool_result(name, response)
    }
    async fn wait_for_settlement(
        &mut self,
        run: &RunId,
        settle: impl std::future::Future<Output = ()>,
    ) -> Value {
        let wait = self
            .request(
                "tools/call",
                json!({"name":"verify_wait","arguments":{"run":run,"seconds":40}}),
            )
            .await;
        // A same-connection status reply is the barrier, not a sleep: the wait was
        // submitted first and the fixture must still be active. An early wait reply
        // fails the response-order assertion. A delayed stale reply fails the terminal
        // result assertion after the controlled release/cancellation below.
        let active = self.call("verify_status", json!({"run":run})).await;
        assert!(
            ["preparing", "running", "waiting"].contains(&active["state"].as_str().unwrap()),
            "fixture settled before the wait barrier: {active}"
        );
        settle.await;
        let result = Self::tool_result("verify_wait", self.response(wait).await);
        assert!(
            ["green", "cancelled"].contains(&result["state"].as_str().unwrap()),
            "verify_wait returned an active snapshot instead of controlled settlement: {result}"
        );
        result
    }
    async fn inspect(&mut self, name: &str, mut arguments: Value) -> Value {
        let mut text = String::new();
        loop {
            let page = self.call(name, arguments.clone()).await;
            let entries = page["entries"].as_array().unwrap();
            assert!(entries.len() <= PAGE_ENTRIES);
            for entry in entries {
                text.push_str(entry["text"].as_str().unwrap());
            }
            match page["next_cursor"].as_str() {
                Some(cursor) => arguments["cursor"] = json!(cursor),
                None => return serde_json::from_str(&text).unwrap(),
            }
        }
    }
    async fn close(mut self) {
        self.input.shutdown().await.unwrap();
        drop(self.input);
        let status = tokio::time::timeout(support::os::patience(FIXTURE_BOUND), self.child.wait())
            .await
            .expect("server exits after stdin closes")
            .unwrap();
        assert!(status.success());
    }
}
fn runtime(id: &str, command: impl AsRef<str>, args: Value) -> Value {
    json!({"id":id,"title":"Check","profile":{"kind":"runtime","command":command.as_ref(),"args":args},"payload":{"instruction":"Check."}})
}

#[tokio::test]
async fn client_fixture_reaps_its_server_on_normal_drop_and_unwind() {
    let project = Project::new(json!([]));
    for unwind in [false, true] {
        let client = Client::new(project.command(), "2026-07-28").await;
        let pid = client.child.id();
        let birth = support::os::start_time(pid);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _client = client;
            if unwind {
                panic!("fixture assertion unwind");
            }
        }));
        assert_eq!(result.is_err(), unwind);
        assert!(
            !support::os::same_process_running(pid, birth),
            "fixture-owned server remains alive"
        );
    }
}

#[tokio::test]
async fn b1_stdio_lifecycle_paging_legacy_and_client_exit() {
    let project = Project::new(json!([]));
    Client::new(project.command(), "2026-07-28")
        .await
        .close()
        .await;
    let malformed = support::os::run(project.command().args(["--json", "mcp", "--unknown"]));
    assert!(!malformed.status.success());
    assert!(
        malformed.stdout.is_empty(),
        "startup diagnostics are not protocol messages"
    );
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("unexpected argument"));
    let script = project._root.path().join("controlled.py");
    support::os::write_python_script(
        &script,
        "import pathlib,time\npathlib.Path('ready').write_text('ready')\nwhile not pathlib.Path('release').exists(): time.sleep(0.01)\n",
    );
    let mut evals: Vec<_> = (0..PAGE_ENTRIES + 1)
        .map(|n| {
            runtime(
                &format!("check-{n:02}"),
                support::os::true_program(),
                json!([]),
            )
        })
        .collect();
    evals.push(runtime(
        "controlled",
        support::os::python_program(),
        json!(["-I", support::os::path_text(&script)]),
    ));
    support::declaration::write(
        project.repo.join("index.artf"),
        json!({"name":"app","fingerprint":{"files":["content.txt"]},"evals":evals}).to_string(),
    )
    .unwrap();
    let mut client = Client::new(project.command(), "2026-07-28").await;
    let tools = client.rpc("tools/list", json!({})).await;
    let mut names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "cache_show",
            "config_check",
            "config_graph",
            "doctor",
            "models",
            "remote_status",
            "request_list",
            "request_logs",
            "request_show",
            "run_list",
            "run_show",
            "session_show",
            "status",
            "tools_check",
            "verify_cancel",
            "verify_result",
            "verify_start",
            "verify_status",
            "verify_wait",
            "verify_watch",
        ],
        "the public surface exposes no credentials or Human sign-off actions"
    );
    let start_schema = &tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "verify_start")
        .unwrap()["inputSchema"]["properties"]["jobs"];
    assert_eq!(start_schema["minimum"], 1);
    assert!(start_schema.get("default").is_none());
    let invalid = client
        .rpc(
            "tools/call",
            json!({"name":"verify_start","arguments":{"scope":{"kind":"all"},"jobs":0}}),
        )
        .await;
    assert_eq!(invalid["result"]["isError"], true);
    assert!(
        !project.state.join("state.sqlite").exists(),
        "zero cap was rejected before Run reservation"
    );
    let started = client
        .call(
            "verify_start",
            json!({"scope":{"kind":"all"},"request_key":"mcp-attempt"}),
        )
        .await;
    assert!(serde_json::to_vec(&started).unwrap().len() < COMPACT_BYTES);
    let run: RunId = started["run"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        store::read_run(&project.state, &run)
            .await
            .unwrap()
            .run
            .jobs,
        None
    );
    project
        .state(
            &run,
            "app/controlled",
            artifactize_core::types::RequestStatus::Running,
        )
        .await;
    marker(&project.repo.join("ready"), "ready").await;
    let bounded = client
        .call("verify_wait", json!({"run":run,"seconds":1}))
        .await;
    assert!(!["green", "red", "error", "cancelled"].contains(&bounded["state"].as_str().unwrap()));
    assert_eq!(
        client.call("verify_result", json!({"run":run})).await["ready"],
        false
    );
    assert_eq!(
        client.call("verify_status", json!({"run":run})).await["run"],
        json!(run)
    );
    client.close().await;
    // The detached worker outlives the server; only the controlled input completes it.
    assert!(
        store::read_run(&project.state, &run)
            .await
            .unwrap()
            .run
            .status()
            == RunStatus::Running
    );
    let mut legacy = Client::new(project.command(), "2024-11-05").await;
    assert_eq!(
        legacy
            .wait_for_settlement(&run, async {
                fs::write(project.repo.join("release"), "release").unwrap();
            })
            .await["state"],
        "green"
    );
    assert_eq!(project.settled(&run).await.run.status(), RunStatus::Green);
    let first = legacy.call("verify_result", json!({"run":run})).await;
    assert_eq!(first["findings"].as_array().unwrap().len(), PAGE_ENTRIES);
    let second = legacy
        .call(
            "verify_result",
            json!({"run":run,"cursor":first["next_cursor"]}),
        )
        .await;
    assert_eq!(second["findings"].as_array().unwrap().len(), 2);
    assert!(second["next_cursor"].is_null());
    fs::remove_file(project.repo.join("release")).unwrap();
    fs::remove_file(project.repo.join("ready")).unwrap();
    evals.last_mut().unwrap()["payload"]["instruction"] = json!("A new controlled judgment.");
    support::declaration::write(
        project.repo.join("index.artf"),
        json!({"name":"app","fingerprint":{"files":["content.txt"]},"evals":evals}).to_string(),
    )
    .unwrap();
    let cancelled = legacy
        .call("verify_start", json!({"scope":{"kind":"all"}}))
        .await;
    let cancelled: RunId = cancelled["run"].as_str().unwrap().parse().unwrap();
    project
        .state(
            &cancelled,
            "app/controlled",
            artifactize_core::types::RequestStatus::Running,
        )
        .await;
    marker(&project.repo.join("ready"), "ready").await;
    assert_eq!(
        legacy.call("verify_cancel", json!({"run":cancelled})).await["cancellation_requested"],
        true
    );
    assert_eq!(
        project.settled(&cancelled).await.run.status(),
        RunStatus::Error
    );
    assert!(
        store::read_run(&project.state, &cancelled)
            .await
            .unwrap()
            .requests
            .iter()
            .any(|r| r.status() == artifactize_core::types::RequestStatus::Green)
    );
    legacy.close().await;
}

#[tokio::test]
async fn b1_wait_bounds_are_eval_kind_independent() {
    for kind in ["human", "agent"] {
        let gate = support::Gate::default();
        let provider = support::FakeProvider::start({
            let gate = gate.clone();
            move |request| {
                if request.path.ends_with("/models") {
                    return support::openai::models(&["seam-model"]);
                }
                gate.wait();
                support::openai::completed(
                    request,
                    vec![support::openai::message("{\"verdict\":\"GREEN\"}")],
                    support::openai::usage(1, 1),
                )
            }
        });
        let profile = if kind == "agent" {
            json!({"kind":"agent","backend":"openai","model":"seam-model"})
        } else {
            json!({"kind":"human"})
        };
        let project = Project::new(
            json!([{ "id":"review","title":"Review","profile":profile,"payload":{"instruction":"Review this long active audit.\n".repeat(1000)} }]),
        );
        let mut command = project.command();
        command
            .env("OPENAI_API_KEY", "fake-key")
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base());
        let mut client = Client::new(command, "2026-07-28").await;
        let started = client
            .call("verify_start", json!({"scope":{"kind":"all"}}))
            .await;
        let run: RunId = started["run"].as_str().unwrap().parse().unwrap();
        let expected = if kind == "human" {
            artifactize_core::types::RequestStatus::WaitingHuman
        } else {
            artifactize_core::types::RequestStatus::Running
        };
        project.state(&run, "app/review", expected).await;
        let observation = client
            .call("verify_wait", json!({"run":run,"seconds":1}))
            .await;
        assert!(serde_json::to_vec(&observation).unwrap().len() < COMPACT_BYTES);
        assert_eq!(
            observation["counts"][if kind == "human" {
                "waiting"
            } else {
                "running"
            }],
            1
        );
        let requests = client.inspect("request_list", json!({"run": run})).await;
        assert_eq!(requests.as_array().unwrap().len(), 1);
        assert_eq!(
            requests[0]["id"],
            json!(
                store::read_run(&project.state, &run)
                    .await
                    .unwrap()
                    .requests[0]
                    .id
            )
        );
        let ended = client
            .wait_for_settlement(&run, async {
                if kind == "agent" {
                    gate.open();
                } else {
                    // Fixture cleanup, not a Human claim or judgment. The pending
                    // MCP observer must follow this explicit core cancellation to end.
                    runs::cancel(&project.context(), &run).await.unwrap();
                }
            })
            .await;
        assert_eq!(
            ended["state"],
            if kind == "agent" {
                "green"
            } else {
                "cancelled"
            }
        );
        client.close().await;
    }
    let project = Project::new(
        json!([{ "id":"review", "title":"Person", "profile":{"kind":"human"}, "payload":{"instruction":"The person decides."} }]),
    );
    let mut client = Client::new(project.command(), "2026-07-28").await;
    let started = client
        .call(
            "verify_start",
            json!({"scope":{"kind":"all"},"defer_human":true,"jobs":1}),
        )
        .await;
    let run: RunId = started["run"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        store::read_run(&project.state, &run)
            .await
            .unwrap()
            .run
            .jobs,
        Some(1)
    );
    assert_eq!(
        client
            .call("verify_wait", json!({"run":run,"seconds":40}))
            .await["state"],
        "signoffs_pending"
    );
    assert_eq!(
        client.call("verify_status", json!({"run":run})).await["state"],
        "signoffs_pending"
    );
    assert_eq!(
        client.call("verify_result", json!({"run":run})).await["verdict"],
        "SIGNOFFS_PENDING"
    );
    assert!(
        store::read_run(&project.state, &run)
            .await
            .unwrap()
            .run
            .modes
            .defer_human
    );
    client.close().await;
}

#[tokio::test]
async fn b1_read_tools_match_cli_json_table() {
    let provider = support::FakeProvider::start(|request| {
        if request.path.ends_with("/models") {
            support::openai::models(&["seam-model"])
        } else {
            support::openai::completed(
                request,
                vec![support::openai::message("{\"verdict\":\"GREEN\"}")],
                support::openai::usage(1, 1),
            )
        }
    });
    let project = Project::new(
        json!([{ "id":"review","title":"Review","profile":{"kind":"agent","backend":"openai","model":"seam-model"},"payload":{"instruction":"Review the long saved audit.\n".repeat(2000)} }]),
    );
    let command = || {
        let mut command = project.command();
        command
            .env("OPENAI_API_KEY", "fake-key")
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base());
        command
    };
    let mut client = Client::new(command(), "2026-07-28").await;
    let started = client
        .call("verify_start", json!({"scope":{"kind":"all"}}))
        .await;
    let run: RunId = started["run"].as_str().unwrap().parse().unwrap();
    project.settled(&run).await;
    let view = store::read_run(&project.state, &run).await.unwrap();
    let request = &view.requests[0];
    let key = request.key.as_ref().unwrap();
    let table = vec![
        (
            "status",
            json!({"scope":{"kind":"all"}}),
            vec!["status", "--all"],
        ),
        ("config_check", json!({}), vec!["config", "check"]),
        (
            "run_list",
            json!({"limit":20}),
            vec!["run", "list", "--limit", "20"],
        ),
        (
            "run_show",
            json!({"run":run}),
            vec!["run", "show", run.as_str()],
        ),
        (
            "request_list",
            json!({"run":run}),
            vec!["request", "list", "--run", run.as_str()],
        ),
        (
            "request_show",
            json!({"id":request.id}),
            vec!["request", "show", request.id.as_str()],
        ),
        (
            "session_show",
            json!({"reference":request.id}),
            vec!["session", "show", request.id.as_str()],
        ),
        ("tools_check", json!({}), vec!["tools", "check"]),
        ("doctor", json!({}), vec!["doctor"]),
        (
            "models",
            json!({"provider":"openai"}),
            vec!["models", "openai"],
        ),
        ("remote_status", json!({}), vec!["remote", "status"]),
        (
            "cache_show",
            json!({"key":key}),
            vec!["cache", "show", key.as_str()],
        ),
    ];
    for (name, arguments, args) in table {
        let actual = client.inspect(name, arguments).await;
        // Parity alone could let the shared serializer make both surfaces wrong.
        // Each row also checks a fixture-authored semantic witness independently.
        match name {
            "status" => {
                assert_eq!(actual["satisfied"], true);
                assert_eq!(actual["selectedEvalIds"], json!(["app/review"]));
            }
            "config_check" => assert_eq!(actual, json!({"ok":true,"artifacts":1,"evals":1})),
            "run_list" => {
                assert_eq!(actual[0]["id"], json!(run));
                assert_eq!(actual[0]["status"], "GREEN");
            }
            "run_show" => {
                assert_eq!(actual["id"], json!(run));
                assert_eq!(actual["status"], "GREEN");
                assert_eq!(actual["requests"][0]["evalId"], "app/review");
            }
            "request_list" => {
                assert_eq!(actual[0]["evalId"], "app/review");
                assert_eq!(actual[0]["result"]["verdict"], "GREEN");
            }
            "request_show" => {
                assert_eq!(actual["evalId"], "app/review");
                assert_eq!(actual["result"]["verdict"], "GREEN");
            }
            "session_show" => {
                assert!(
                    actual["events"]
                        .as_array()
                        .is_some_and(|events| !events.is_empty())
                );
                assert!(actual["events"].to_string().contains("GREEN"));
            }
            "tools_check" => {
                assert_eq!(actual["ok"], true);
                assert_eq!(actual["scopes"][0]["evalId"], "app/review");
            }
            "doctor" => {
                assert_eq!(
                    actual["stateDir"],
                    support::os::path_text(&support::os::canonical(&project.state))
                );
                assert!(
                    actual["checks"]
                        .as_array()
                        .is_some_and(|checks| !checks.is_empty())
                );
            }
            "models" => {
                assert_eq!(actual["backend"], "openai");
                assert_eq!(actual["models"][0]["slug"], "seam-model");
            }
            "remote_status" => assert_eq!(actual, json!({"configured":false})),
            "cache_show" => {
                assert_eq!(actual["status"], "GREEN");
                assert_eq!(actual["provenance"]["evalId"], "app/review");
                assert_eq!(actual["result"]["verdict"], "GREEN");
            }
            _ => unreachable!("every parity row has an independent witness"),
        }
        let output = support::os::run(command().args(args).arg("--json"));
        // Disabled remote inspection is valid JSON but exits 1 on the CLI.
        assert_eq!(
            output.status.code(),
            Some(if name == "remote_status" { 1 } else { 0 }),
            "{name}: {output:?}"
        );
        let expected: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            actual,
            expected,
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    client.close().await;
}

fn host_smoke(host: &str) {
    let root = support::os::tempdir();
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../harness/hey-arti/host_smoke.py");
    let output = support::os::run(
        std::process::Command::new(support::os::python_program())
            .arg("-I")
            .arg(script)
            .arg("--host")
            .arg(host)
            .arg("--artifactize")
            .arg(support::os::cli())
            .arg("--output")
            .arg(root.path().join("host")),
    );
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "manual: requires an installed Claude Code host and normal inference authentication"]
fn b2_claude_code_registration_lists_tools() {
    host_smoke("claude");
}
#[test]
#[ignore = "manual: requires an installed Codex CLI host"]
fn b2_codex_registration_lists_tools() {
    host_smoke("codex");
}

#[tokio::test]
async fn runtime_log_queries_retain_tail_and_report_pruned_evidence() {
    let project = Project::new(json!([]));
    let script = project._root.path().join("logs.py");
    support::os::write_python_script(
        &script,
        "import sys\nfor n in range(6000): print(f'line-{n:04d} ' + 'x' * 40)\nprint('final diagnostic', file=sys.stderr)\nsys.exit(7)\n",
    );
    support::declaration::write(
        project.repo.join("index.artf"),
        json!({
            "name":"app", "evals":[runtime("logs", support::os::python_program(), json!(["-I",support::os::path_text(&script)]))]
        })
        .to_string(),
    )
    .unwrap();
    let mut client = Client::new(project.command(), "2026-07-28").await;
    let started = client
        .call("verify_start", json!({"scope":{"kind":"all"}}))
        .await;
    let run = started["run"].as_str().unwrap().parse().unwrap();
    let view = project.settled(&run).await;
    assert_eq!(view.run.status(), RunStatus::Red);
    let request = &view.requests[0];
    let stdout = client
        .inspect(
            "request_logs",
            json!({"id":request.id,"stream":"stdout","tail":2}),
        )
        .await;
    assert_eq!(stdout["streams"][0]["evidence"], "complete");
    assert_eq!(stdout["streams"][0]["lines"][1]["number"], 6000);
    assert!(
        stdout["streams"][0]["lines"][1]["text"]
            .as_str()
            .unwrap()
            .starts_with("line-5999")
    );
    let search = client
        .inspect(
            "request_logs",
            json!({"id":request.id,"stream":"stdout","start":5900,"search":"line-5998"}),
        )
        .await;
    assert_eq!(search["streams"][0]["lines"][0]["number"], 5999);
    let cli = support::os::run(project.command().args([
        "request",
        "logs",
        request.id.as_str(),
        "--stream",
        "stdout",
        "--start",
        "2",
        "--end",
        "3",
    ]));
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let cli: Value = serde_json::from_slice(&cli.stdout).unwrap();
    let mcp = client
        .inspect(
            "request_logs",
            json!({"id":request.id,"stream":"stdout","start":2,"end":3}),
        )
        .await;
    assert_eq!(cli, mcp);
    let stderr = client
        .inspect("request_logs", json!({"id":request.id,"stream":"stderr"}))
        .await;
    assert_eq!(stderr["streams"][0]["lines"][0]["text"], "final diagnostic");
    let forward = client
        .inspect(
            "request_logs",
            json!({"id":request.id,"stream":"stdout","start":1}),
        )
        .await;
    assert!(forward["streams"][0]["next_start"].as_u64().unwrap() > 1);
    let reused = client
        .call("verify_start", json!({"scope":{"kind":"all"}}))
        .await;
    let reused_run = reused["run"].as_str().unwrap().parse().unwrap();
    let reused_view = project.settled(&reused_run).await;
    let reused_log = client
        .inspect(
            "request_logs",
            json!({"id":reused_view.requests[0].id,"stream":"stdout","tail":2}),
        )
        .await;
    assert_eq!(reused_log["producer"], json!(request.id));
    assert_eq!(reused_log["streams"], stdout["streams"]);
    fs::remove_dir_all(request.run_dir.as_ref().unwrap()).unwrap();
    let pruned = client
        .inspect(
            "request_logs",
            json!({"id":request.id,"stream":"stdout","tail":1}),
        )
        .await;
    assert_eq!(pruned["streams"][0]["evidence"], "saved_audit");
    assert!(
        pruned["streams"][0]["note"]
            .as_str()
            .unwrap()
            .contains("Missing tail")
    );
    client.close().await;
}

#[tokio::test]
async fn watch_stream_separates_live_eval_and_final_outcome_without_polling() {
    let project = Project::new(json!([]));
    let script = project._root.path().join("watch.py");
    support::os::write_python_script(
        &script,
        "import pathlib,time\npathlib.Path('ready').write_text('ready')\nwhile not pathlib.Path('release').exists(): time.sleep(0.01)\n",
    );
    support::declaration::write(
        project.repo.join("index.artf"),
        json!({
            "name":"app", "fingerprint":{"files":["content.txt"]},
            "evals":[runtime("controlled",support::os::python_program(),json!(["-I",support::os::path_text(&script)])),
                runtime("fast",support::os::true_program(),json!([]))]
        })
        .to_string(),
    )
    .unwrap();
    let mut client = Client::new(project.command(), "2026-07-28").await;
    let started = client
        .call("verify_start", json!({"scope":{"kind":"all"}}))
        .await;
    let run: RunId = started["run"].as_str().unwrap().parse().unwrap();
    marker(&project.repo.join("ready"), "ready").await;
    project
        .state(
            &run,
            "app/fast",
            artifactize_core::types::RequestStatus::Green,
        )
        .await;
    let call = client.request("tools/call",json!({"name":"verify_watch","arguments":{"run":run},"_meta":{"progressToken":"watch-seam"}})).await;
    let mut events = Vec::new();
    loop {
        let mut line = String::new();
        tokio::time::timeout(
            support::os::patience(PROTOCOL_BOUND),
            client.output.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(line.len() <= RESPONSE_BYTES);
        let message: Value = serde_json::from_str(&line).unwrap();
        if message["id"] == call {
            assert_eq!(
                Client::tool_result("verify_watch", message)["state"],
                "green"
            );
            break;
        }
        assert_eq!(message["method"], "notifications/progress");
        assert_eq!(message["params"]["progressToken"], "watch-seam");
        let event: Value =
            serde_json::from_str(message["params"]["message"].as_str().unwrap()).unwrap();
        if events.is_empty() {
            assert_eq!(event["kind"], "update");
            assert!(!["green", "red", "error"].contains(&event["data"]["state"].as_str().unwrap()));
        }
        if event["kind"] == "eval_result" && event["data"]["eval"] == "app/fast" {
            assert!(!events.iter().any(|e: &Value| e["kind"] == "outcome"));
            fs::write(project.repo.join("release"), "release").unwrap();
        }
        events.push(event);
    }
    assert_eq!(
        events.iter().filter(|e| e["kind"] == "eval_result").count(),
        2
    );
    let final_event = events.last().unwrap();
    assert_eq!(final_event["kind"], "outcome");
    assert_eq!(final_event["data"]["satisfied"], true);
    assert_eq!(project.settled(&run).await.run.status(), RunStatus::Green);
    let replay = support::os::run(project.command().args(["run", "watch", run.as_str()]));
    assert!(replay.status.success());
    let replay: Vec<Value> = String::from_utf8(replay.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(
        replay
            .iter()
            .map(|e| e["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["update", "eval_result", "eval_result", "outcome"]
    );
    client.close().await;
}

#[tokio::test]
async fn graph_discovery_and_conservative_impact_match_cli() {
    let project = Project::new(json!([]));
    fs::remove_file(project.repo.join("index.artf")).unwrap();
    for (name, declaration) in [
        ("input", json!({"name":"input","basis":true})),
        (
            "consumer",
            json!({"name":"consumer","evals":[{"id":"check","title":"Check",
                "profile":{"kind":"human"},"payload":{"instruction":"Check {input}."}}]}),
        ),
        ("independent", json!({"name":"independent","basis":true})),
    ] {
        fs::create_dir(project.repo.join(name)).unwrap();
        support::declaration::write(
            project.repo.join(name).join("index.artf"),
            declaration.to_string(),
        )
        .unwrap();
    }
    let mut client = Client::new(project.command(), "2026-07-28").await;
    let arguments = json!({"scope":{"kind":"artifact","artifactId":"input"},"impact":true});
    let mcp = client.inspect("config_graph", arguments).await;
    let cli = support::os::run(
        project
            .command()
            .args(["config", "graph", "input", "--impact", "--json"]),
    );
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert_eq!(mcp, serde_json::from_slice::<Value>(&cli.stdout).unwrap());
    assert!(mcp["artifacts"].get("consumer").is_some());
    assert!(mcp["artifacts"].get("input").is_some());
    assert!(mcp["artifacts"].get("independent").is_none());
    client.close().await;
}
