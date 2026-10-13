//! B1: protocol/lifecycle and one table of CLI-equivalent inspect projections.
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use artifactize_core::{runs, types::RunId};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
};
use tokio_util::sync::CancellationToken;

struct Project {
    _root: tempfile::TempDir,
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
        Self {
            repo,
            state: root.path().join("state"),
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
    async fn settled(&self, run: &RunId) -> runs::RunObservation {
        runs::wait(
            &self.context(),
            run,
            Some(support::os::patience(Duration::from_secs(30))),
            CancellationToken::new(),
        )
        .await
        .unwrap()
    }
    async fn state(&self, run: &RunId, expected: artifactize_core::types::RequestStatus) {
        let mut changes = artifactize_core::changes::Subscription::new(&self.state).await;
        tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
            loop {
                let view = runs::view(&self.context(), run).await.unwrap();
                if view
                    .requests
                    .iter()
                    .any(|request| request.status() == expected)
                {
                    return;
                }
                assert!(
                    !runs::status(&self.context(), run)
                        .await
                        .unwrap()
                        .state
                        .is_terminal()
                );
                changes.next_reconciled().await;
            }
        })
        .await
        .unwrap();
    }
}
struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
    modern: bool,
}
impl Client {
    async fn new(command: std::process::Command, version: &str) -> Self {
        let mut command = tokio::process::Command::from(command);
        let mut child = command
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
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
    async fn rpc(&mut self, method: &str, mut params: Value) -> Value {
        self.id += 1;
        if self.modern {
            params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28", "io.modelcontextprotocol/clientInfo":{"name":"seam","version":"1"}, "io.modelcontextprotocol/clientCapabilities":{}});
        }
        self.send(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}))
            .await;
        let mut line = String::new();
        tokio::time::timeout(
            support::os::patience(Duration::from_secs(45)),
            self.output.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            line.len() <= 64 * 1024,
            "bounded protocol result: {} bytes",
            line.len()
        );
        let response: Value = serde_json::from_str(&line).expect("stdout carries protocol only");
        assert_eq!(response["id"], self.id);
        response
    }
    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let response = self
            .rpc("tools/call", json!({"name":name,"arguments":arguments}))
            .await;
        assert!(response.get("error").is_none(), "{name}: {response}");
        assert_ne!(response["result"]["isError"], true, "{name}: {response}");
        response["result"]["structuredContent"].clone()
    }
    async fn inspect(&mut self, name: &str, mut arguments: Value) -> Value {
        let mut text = String::new();
        loop {
            let page = self.call(name, arguments.clone()).await;
            let entries = page["entries"].as_array().unwrap();
            assert!(entries.len() <= 20);
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
        assert!(self.child.wait().await.unwrap().success());
    }
}
fn runtime(id: &str, command: impl AsRef<str>, args: Value) -> Value {
    json!({"id":id,"title":"Check","profile":{"kind":"runtime","command":command.as_ref(),"args":args},"payload":{"instruction":"Check."}})
}

#[tokio::test]
async fn b1_stdio_lifecycle_paging_legacy_and_client_exit() {
    let project = Project::new(json!([]));
    Client::new(project.command(), "2026-07-28")
        .await
        .close()
        .await;
    let malformed = project
        .command()
        .args(["--json", "mcp", "--unknown"])
        .output()
        .unwrap();
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
    let mut evals: Vec<_> = (0..21)
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
        json!([script]),
    ));
    support::declaration::write(
        project.repo.join("index.artf"),
        json!({"name":"app","fingerprint":{"files":["content.txt"]},"evals":evals}).to_string(),
    )
    .unwrap();
    let mut client = Client::new(project.command(), "2026-07-28").await;
    let tools = client.rpc("tools/list", json!({})).await;
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 17);
    let started = client
        .call(
            "verify_start",
            json!({"scope":{"kind":"all"},"request_key":"mcp-attempt"}),
        )
        .await;
    assert!(serde_json::to_vec(&started).unwrap().len() < 2048);
    let run: RunId = started["run"].as_str().unwrap().parse().unwrap();
    project
        .state(&run, artifactize_core::types::RequestStatus::Running)
        .await;
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
        !runs::status(&project.context(), &run)
            .await
            .unwrap()
            .state
            .is_terminal()
    );
    fs::write(project.repo.join("release"), "release").unwrap();
    assert_eq!(project.settled(&run).await.state, runs::State::Green);
    let mut legacy = Client::new(project.command(), "2024-11-05").await;
    assert_eq!(
        legacy.call("verify_wait", json!({"run":run})).await["state"],
        "green"
    );
    let first = legacy.call("verify_result", json!({"run":run})).await;
    assert_eq!(first["findings"].as_array().unwrap().len(), 20);
    let second = legacy
        .call(
            "verify_result",
            json!({"run":run,"cursor":first["next_cursor"]}),
        )
        .await;
    assert_eq!(second["findings"].as_array().unwrap().len(), 2);
    assert!(second["next_cursor"].is_null());
    fs::remove_file(project.repo.join("release")).unwrap();
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
        .state(&cancelled, artifactize_core::types::RequestStatus::Running)
        .await;
    assert_eq!(
        legacy.call("verify_cancel", json!({"run":cancelled})).await["cancellation_requested"],
        true
    );
    assert_eq!(
        project.settled(&cancelled).await.state,
        runs::State::Cancelled
    );
    assert!(
        runs::view(&project.context(), &cancelled)
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
        project.state(&run, expected).await;
        let observation = client
            .call("verify_wait", json!({"run":run,"seconds":1}))
            .await;
        assert!(serde_json::to_vec(&observation).unwrap().len() < 2048);
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
            json!(runs::view(&project.context(), &run).await.unwrap().requests[0].id)
        );
        if kind == "agent" {
            gate.open();
            assert_eq!(
                client
                    .call("verify_wait", json!({"run":run,"seconds":40}))
                    .await["state"],
                "green"
            );
        } else {
            client.call("verify_cancel", json!({"run":run})).await;
            project.settled(&run).await;
        }
        client.close().await;
    }
    let project = Project::new(
        json!([{ "id":"review", "title":"Person", "profile":{"kind":"human"}, "payload":{"instruction":"The person decides."} }]),
    );
    let mut client = Client::new(project.command(), "2026-07-28").await;
    let started = client
        .call(
            "verify_start",
            json!({"scope":{"kind":"all"},"defer_human":true}),
        )
        .await;
    let run: RunId = started["run"].as_str().unwrap().parse().unwrap();
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
        runs::view(&project.context(), &run)
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
    let view = runs::view(&project.context(), &run).await.unwrap();
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
        let output = command().args(args).arg("--json").output().unwrap();
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
    let output = std::process::Command::new(support::os::python_program())
        .arg("-I")
        .arg(script)
        .arg("--host")
        .arg(host)
        .arg("--artifactize")
        .arg(support::os::cli())
        .arg("--output")
        .arg(root.path().join("host"))
        .output()
        .unwrap();
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
