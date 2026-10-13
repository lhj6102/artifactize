//! The graph Artifact goes through ordinary verification, reuse-only, status and Runs.
use serde_json::{Value, json};
use std::{fs, process::Command};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

#[test]
fn ordinary_verify_reuses_graph_results_until_structure_changes() {
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let graph = json!({
        "name": "project-graph",
        "evals": [{"id":"check", "title":"Check project structure", "profile":{"kind":"runtime","command":support::os::true_program(),"args":[]}, "payload":{"instruction":"Check the graph."}}],
    });
    fs::write(
        repo.join(".artf"),
        support::declaration::to_toml(graph).unwrap(),
    )
    .unwrap();
    fs::write(repo.join("index.artf"), "name = 'app'\nbasis = true\n").unwrap();
    fs::write(repo.join("data.txt"), "first contents").unwrap();
    let invoke = |args: &[&str], code: i32| -> Value {
        let output = Command::new(support::os::cli())
            .arg("--repo")
            .arg(&repo)
            .arg("--state-dir")
            .arg(&state)
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let first = invoke(&["verify", "--wait", "project-graph"], 0);
    assert_eq!(first["executionsStarted"], 1);
    let fingerprint = first["requests"][0]["key"].clone();
    assert!(fingerprint.as_str().is_some());
    let unchanged = invoke(&["verify", "--wait", "project-graph"], 0);
    assert_eq!(unchanged["executionsStarted"], 0);
    assert_eq!(
        first["requests"][0]["executionId"],
        unchanged["requests"][0]["executionId"]
    );
    fs::write(repo.join("data.txt"), "unrelated changed contents").unwrap();
    fs::write(
        repo.join("index.artf"),
        "name = 'app'\nbasis = true\ntags = ['new-tag']\n",
    )
    .unwrap();
    let content_only = invoke(
        &[
            "verify",
            "--wait",
            "project-graph",
            "--reuse-only",
            "runtime",
        ],
        0,
    );
    assert_eq!(content_only["executionsStarted"], 0);
    fs::write(
        repo.join("index.artf"),
        "name = 'renamed-app'\nbasis = true\n",
    )
    .unwrap();
    let stale = invoke(
        &[
            "verify",
            "--wait",
            "project-graph",
            "--reuse-only",
            "runtime",
        ],
        4,
    );
    assert_eq!(stale["executionsStarted"], 0);
    let changed = invoke(&["verify", "--wait", "--all"], 0);
    assert_eq!(changed["executionsStarted"], 1);
    assert_ne!(changed["requests"][0]["key"], fingerprint);
    let status = invoke(&["status", "--all"], 0);
    assert!(status.to_string().contains("project-graph"));
}
