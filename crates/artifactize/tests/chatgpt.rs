use std::{fs, process::Command};

use serde_json::{Value, json};

#[test]
fn models_requires_login_in_selected_state_without_creating_a_run() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state");
    let ignored = directory.path().join("ignored-state");
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .env("ARTIFACTIZE_STATE_HOME", &ignored)
        .env("OPENAI_API_KEY", "must-not-fall-back")
        .arg("--state-dir")
        .arg(&state)
        .args(["models", "chatgpt", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("artifactize login chatgpt")
    );
    assert!(state.join("auth/chatgpt.lock").exists());
    assert!(!state.join("state.sqlite").exists());
    assert!(!ignored.exists());
    let help = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .args(["models", "chatgpt", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
}

#[test]
fn verify_uses_selected_auth_state_and_records_missing_login_as_error() {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    let state = directory.path().join("state");
    let ignored = directory.path().join("ignored-state");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("artifactize.json"), json!({
        "name":"a", "evals":[{"id":"review", "title":"Review", "payload":{"instruction":"Review this artifact"}, "profile":{"kind":"agent", "backend":"chatgpt", "model":"exact-model"}}],
    }).to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .env("ARTIFACTIZE_STATE_HOME", &ignored)
        .env("OPENAI_API_KEY", "must-not-fall-back")
        .arg("--state-dir")
        .arg(&state)
        .arg("--repo")
        .arg(&repo)
        .args(["verify", "--all", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["requests"][0]["status"], "ERROR");
    assert!(
        body["requests"][0]["error"]
            .as_str()
            .unwrap()
            .contains("artifactize login chatgpt")
    );
    assert!(state.join("auth/chatgpt.lock").exists());
    assert!(!ignored.exists());
}
