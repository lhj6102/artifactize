use std::{fs, path::Path, process::Stdio, time::Duration};

use artifactize::{config::read_workspace_config, mcp, store::Receipts};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

async fn setup(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let repo = root.join("repo");
    let state = root.join("state");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("artifactize.json"), json!({
        "name":"a","views":{"agentTools":{"slow":{
            "command":"/bin/sh","args":["-c","echo $$ > child.pid; sleep 30"],
            "protocol":"plain","description":"Slow tool","inputSchema":{"type":"object"}
        }}},"evals":[{"id":"review","title":"Review","profile":{"kind":"agent","backend":"claude","model":"test","maxToolCalls":1},"payload":{"instruction":"Review."}}]
    }).to_string()).unwrap();
    let config = read_workspace_config(&repo).unwrap();
    mcp::write_config(
        &config,
        &config.evals[0],
        "lifecycle",
        &state,
        &root.join("out"),
    )
    .await
    .unwrap();
    (repo, state, root.join("out/mcp-manifest.json"))
}

#[tokio::test]
async fn disconnect_cancels_children_and_keeps_an_error_audit() {
    let root = tempfile::tempdir().unwrap();
    let (repo, state, manifest) = setup(root.path()).await;
    let mut child = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .args(["mcp", "--manifest"])
        .arg(&manifest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    input.write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).as_bytes()).await.unwrap();
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), output.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    input.write_all(format!("{}\n{}\n", json!({"jsonrpc":"2.0","method":"notifications/initialized"}), json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"slow_a","arguments":{}}})).as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !repo.join("child.pid").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pid = fs::read_to_string(repo.join("child.pid")).unwrap();
    drop(input);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(!Path::new(&format!("/proc/{}", pid.trim())).exists());
    let calls = Receipts::open(&state, &repo)
        .await
        .unwrap()
        .tool_calls("lifecycle")
        .await
        .unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["isError"], true);
    assert!(calls[0]["result"].as_str().unwrap().contains("cancelled"));
}

#[tokio::test]
async fn reconnect_refuses_changed_scope_and_budget() {
    let root = tempfile::tempdir().unwrap();
    let (_, _, manifest) = setup(root.path()).await;
    let mut data: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    data["profile"]["maxToolCalls"] = json!(100);
    fs::write(&manifest, data.to_string()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .args(["mcp", "--manifest"])
        .arg(&manifest)
        .stdin(Stdio::null())
        .output()
        .await
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("scope or budget changed"));
    assert!(result.stdout.is_empty());
}
