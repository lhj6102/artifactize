use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::Duration,
};

use artifactize::{
    config::read_workspace_config,
    mcp,
    store::{self, Receipts},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
};

fn fixture(repo: &Path, budget: Option<u64>) {
    fs::create_dir_all(repo.join("a")).unwrap();
    fs::create_dir_all(repo.join("b")).unwrap();
    fs::write(repo.join("a/env.sh"), "#!/bin/sh\nprintf '%s|%s|%s' \"$HOME\" \"${TOOL_CHECK_SECRET-unset}\" \"$PWD\"\nprintf invoked > touched\n").unwrap();
    fs::set_permissions(repo.join("a/env.sh"), fs::Permissions::from_mode(0o700)).unwrap();
    let mut profile = json!({"kind":"agent","backend":"claude","model":"test"});
    if let Some(budget) = budget {
        profile["maxToolCalls"] = json!(budget);
    }
    fs::write(repo.join("a/artifactize.json"), json!({
        "name":"a","views":{
            "agentTools":{
                "read":{"builtin":"read"},"image":{"builtin":"view_image"},
                "env":{"description":"Environment","command":"./env.sh","args":[],"protocol":"plain","inputSchema":{"type":"object","additionalProperties":false}},
                "data":{"description":"JSON","command":"/bin/echo","args":["{\"content\":[{\"type\":\"json\",\"data\":{\"answer\":42}}]}"],"protocol":"json","inputSchema":{"type":"object"}},
                "error":{"description":"Authored error","command":"/bin/echo","args":["{\"content\":[{\"type\":\"text\",\"text\":\"Owner error\"}],\"isError\":true}"],"protocol":"json","inputSchema":{"type":"object"}}
            },
            "humanTools":{"env":{"description":"Environment","kind":"output","command":"./env.sh","args":[]}}
        },
        "evals":[
            {"id":"review","title":"Review","profile":profile,"payload":{"instruction":"Review a."}},
            {"id":"human","title":"Human","profile":{"kind":"human"},"payload":{"instruction":"Review a."}}
        ]
    }).to_string()).unwrap();
    fs::write(
        repo.join("b/artifactize.json"),
        json!({"name":"b","views":{"agentTools":{"read":{"builtin":"read"}}},"basis":true})
            .to_string(),
    )
    .unwrap();
    fs::write(repo.join("a/text.txt"), "scoped text\n").unwrap();
    let mut image = vec![0xff, 0xd8, 0xff, 0xe0];
    image.resize(70_000, 0); // Outbound image responses exceed the inbound request cap.
    fs::write(repo.join("a/image.jpg"), image).unwrap();
    fs::write(repo.join("b/secret.txt"), "outside scope").unwrap();
    symlink(repo.join("b/secret.txt"), repo.join("a/link")).unwrap();
}

fn check(repo: &Path, state: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--repo")
        .arg(repo)
        .arg("--state-dir")
        .arg(state)
        .args(["tools", "check"])
        .args(args)
        .env("TOOL_CHECK_SECRET", "real-environment")
        .env("HOME", "/reviewer-home")
        .output()
        .unwrap()
}

fn parsed(output: &Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn static_check_never_runs_processes_and_execute_uses_the_selected_audience() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fixture(&repo, None);
    let report = parsed(&check(&repo, &state, &[]), 0);
    assert_eq!(report["scopes"].as_array().unwrap().len(), 2);
    assert!(report["scopes"].as_array().unwrap().iter().all(|scope| {
        scope["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["inputSchema"]["type"] == "object")
    }));
    assert!(!repo.join("a/touched").exists());
    assert!(!state.exists());
    let declaration_path = repo.join("a/artifactize.json");
    let original = fs::read(&declaration_path).unwrap();
    let mut declaration: Value = serde_json::from_slice(&original).unwrap();
    declaration["views"]["agentTools"]["env"]["executionPaths"] = json!(["missing-input"]);
    fs::write(&declaration_path, declaration.to_string()).unwrap();
    assert_eq!(parsed(&check(&repo, &state, &["a/review"]), 1)["ok"], false);
    assert!(!repo.join("a/touched").exists());
    fs::write(&declaration_path, original).unwrap();
    let scoped = parsed(&check(&repo, &state, &["a/review"]), 0);
    assert_eq!(scoped["scopes"].as_array().unwrap().len(), 1);
    assert!(
        scoped["scopes"][0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["artifactId"] == "a")
    );
    fs::set_permissions(repo.join("a/env.sh"), fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        parsed(
            &check(
                &repo,
                &state,
                &["--artifact", "a", "--audience", "agent", "--tool", "env"]
            ),
            1
        )["ok"],
        false
    );
    assert!(!repo.join("a/touched").exists());
    fs::set_permissions(repo.join("a/env.sh"), fs::Permissions::from_mode(0o700)).unwrap();
    let agent = parsed(
        &check(
            &repo,
            &state,
            &[
                "--execute",
                "--artifact",
                "a",
                "--audience",
                "agent",
                "--tool",
                "env",
            ],
        ),
        0,
    );
    let text = agent["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("/home|unset|"), "{text}");
    assert!(!text.contains("reviewer-home"));
    let human = parsed(
        &check(
            &repo,
            &state,
            &[
                "--execute",
                "--artifact",
                "a",
                "--audience",
                "human",
                "--tool",
                "env",
            ],
        ),
        0,
    );
    assert!(
        human["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("/reviewer-home|real-environment|")
    );
    let error = parsed(
        &check(
            &repo,
            &state,
            &[
                "--execute",
                "--artifact",
                "a",
                "--audience",
                "agent",
                "--tool",
                "error",
            ],
        ),
        1,
    );
    assert_eq!(error["result"]["isError"], true);
    assert!(!state.join(store::DATABASE).exists());
    assert_eq!(fs::read_dir(&state).unwrap().count(), 0);
    for args in [
        vec!["--eval", "a/review", "--artifact", "a"],
        vec!["--eval", "a/review", "--execute"],
        vec!["--execute", "--artifact", "a"],
        vec!["--args", "{}"],
        vec![
            "--execute",
            "--artifact",
            "a",
            "--audience",
            "human",
            "--tool",
            "env",
            "--args",
            "{}",
        ],
    ] {
        assert_eq!(check(&repo, &state, &args).status.code(), Some(2));
    }
}

struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Client {
    async fn start(manifest: &Path) -> Self {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .args(["mcp", "--manifest"])
            .arg(manifest)
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
        };
        client.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).await;
        let init = client.receive().await;
        assert_eq!(init["result"]["serverInfo"]["name"], "artifactize");
        assert!(init["result"]["capabilities"].get("tools").is_some());
        client
            .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        client
    }
    async fn send(&mut self, message: Value) {
        self.input
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        self.input.flush().await.unwrap();
    }
    async fn receive(&mut self) -> Value {
        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(10), self.output.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        if read == 0 {
            use tokio::io::AsyncReadExt;
            let mut error = String::new();
            self.child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut error)
                .await
                .unwrap();
            panic!("MCP connection closed: {error}");
        }
        serde_json::from_str(&line).unwrap()
    }
    async fn call(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})).await;
        self.receive().await["result"].clone()
    }
    async fn close(mut self) {
        drop(self.input);
        let status = tokio::time::timeout(Duration::from_secs(10), self.child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success(), "{status}");
    }
}

async fn prepare(repo: &Path, state: &Path, output: &Path, execution: &str) -> PathBuf {
    let config = read_workspace_config(repo).unwrap();
    let eval = config.evals.iter().find(|e| e.id == "a/review").unwrap();
    let path = mcp::write_config(&config, eval, execution, state, output)
        .await
        .unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let config: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(config["mcpServers"]["artifactize"]["args"][0], "mcp");
    PathBuf::from(
        config["mcpServers"]["artifactize"]["args"][2]
            .as_str()
            .unwrap(),
    )
}

#[tokio::test]
async fn stdio_tools_are_scoped_multimodal_audited_and_budgeted_across_reconnects() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fixture(&repo, Some(7));
    let manifest = prepare(&repo, &state, &root.path().join("output"), "exec-test").await;
    let mut client = Client::start(&manifest).await;
    client
        .send(json!({"jsonrpc":"2.0","id":10,"method":"ping"}))
        .await;
    assert!(client.receive().await.get("result").is_some());
    client
        .send(json!({"jsonrpc":"2.0","id":11,"method":"not/a/method"}))
        .await;
    assert!(client.receive().await.get("error").is_some());
    // rmcp ignores syntactically invalid JSON, then serves the next valid request.
    client.input.write_all(b"{invalid json\n").await.unwrap();
    client
        .send(json!({"jsonrpc":"2.0","id":12,"method":"ping"}))
        .await;
    assert_eq!(client.receive().await["id"], 12);
    client
        .send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .await;
    let listed = client.receive().await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    assert!(
        tools
            .iter()
            .all(|tool| tool["name"].as_str().unwrap().ends_with("_a")
                && tool["inputSchema"]["type"] == "object")
    );
    let text = client.call(3, "read_a", json!({"path":"text.txt"})).await;
    assert_eq!(text["isError"], false);
    assert!(
        text["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("scoped text")
    );
    let image = client.call(4, "image_a", json!({"path":"image.jpg"})).await;
    assert_eq!(image["content"][0]["type"], "image");
    assert_eq!(image["content"][0]["mimeType"], "image/jpeg");
    assert!(image["content"][0]["data"].as_str().unwrap().len() > mcp::REQUEST_LIMIT);
    for path in ["../b/secret.txt", "link"] {
        assert_eq!(
            client.call(5, "read_a", json!({"path":path})).await["isError"],
            true
        );
    }
    assert_eq!(
        client.call(6, "read_b", json!({"path":"secret.txt"})).await["isError"],
        true
    );
    let data = client.call(7, "data_a", json!({})).await;
    assert_eq!(
        serde_json::from_str::<Value>(data["content"][0]["text"].as_str().unwrap()).unwrap(),
        json!({"answer":42})
    );
    assert_eq!(client.call(8, "error_a", json!({})).await["isError"], true);
    client.close().await;
    let mut client = Client::start(&manifest).await;
    let exhausted = client.call(9, "env_a", json!({})).await;
    assert_eq!(exhausted["isError"], true);
    assert!(
        exhausted["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("maxToolCalls")
    );
    assert!(!repo.join("a/touched").exists());
    client.close().await;
    let receipts = Receipts::open(&state, &repo).await.unwrap();
    let calls = receipts.tool_calls("exec-test").await.unwrap();
    assert_eq!(calls.len(), 8);
    for (i, call) in calls.iter().enumerate() {
        assert_eq!(call["order"], i + 1);
        assert!(call["arguments"].is_object());
        assert!(call["result"].is_string());
        assert_eq!(call["error"].is_null(), call["isError"] == false);
    }
    let db = rusqlite::Connection::open(state.join(store::DATABASE)).unwrap();
    assert_eq!(
        db.query_row("SELECT started FROM mcp_sessions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM tool_calls", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        8
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM runs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    let run: store::Run = serde_json::from_value(json!({"id":"saved","repoPath":repo,"stateDir":state,"status":"ERROR","createdAt":"now","selection":{},"validation":{}})).unwrap();
    let request: store::Request = serde_json::from_value(json!({
        "id":"request","runId":"saved","evalId":"a/review","target":"a","title":"Review","profile":{},"requestedProfile":{},
        "payload":{},"references":{},"deps":[],"status":"ERROR","createdAt":"now","cwd":repo
    })).unwrap();
    receipts.create_run(&run, &[request]).await.unwrap();
    db.execute(
        "UPDATE requests SET data=json_set(data,'$.executionId','exec-test')",
        [],
    )
    .unwrap();
    fs::remove_dir_all(&repo).unwrap();
    let view = store::read_run(&state, "saved").await.unwrap();
    assert_eq!(view.requests[0].tool_calls, calls);
}

#[tokio::test]
async fn oversized_unterminated_requests_are_rejected_before_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fixture(&repo, None);
    let manifest = prepare(&repo, &state, &root.path().join("output"), "exec-cap").await;
    let mut client = Client::start(&manifest).await;
    client
        .input
        .write_all(&vec![b' '; mcp::REQUEST_LIMIT + 1])
        .await
        .unwrap();
    // Keep stdin open: the cap must terminate the server without waiting for EOF.
    let result = tokio::time::timeout(Duration::from_secs(10), client.child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    drop(client.input);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("64 KiB"));
    assert!(
        Receipts::open(&state, &repo)
            .await
            .unwrap()
            .tool_calls("exec-cap")
            .await
            .unwrap()
            .is_empty()
    );
}
