use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use artifactize::store;
use serde_json::{Value, json};
use support::os::bin;

mod support;

/// The owner's tool script. Windows runs a script by path only through the `.exe` stand-in
/// beside it, which a `.sh` extension would bypass.
const TOOL: &str = if cfg!(windows) { "env-tool" } else { "env.sh" };

fn fixture(repo: &Path) {
    fs::create_dir_all(repo.join("a")).unwrap();
    fs::create_dir_all(repo.join("b")).unwrap();
    let tool = repo.join("a").join(TOOL);
    fs::write(&tool, "#!/bin/sh\nprintf '%s|%s|%s' \"$HOME\" \"${TOOL_CHECK_SECRET-unset}\" \"$PWD\"\nprintf invoked > touched\n").unwrap();
    support::os::make_executable(&tool);
    let command = format!("./{TOOL}");
    let profile = json!({"kind":"agent","backend":"openai","model":"test"});
    support::declaration::write(
        repo.join("a/index.artf"),
        json!({
            "name":"a","views":{
                "agent_tools":{
                    "read":{"builtin":"read"},"image":{"builtin":"view_image"},
                    "env":{"description":"Environment","command":command,"args":[],"protocol":"plain","input_schema":{"type":"object","additionalProperties":false}},
                    "data":{"description":"JSON","command":bin("/bin/echo"),"args":["{\"content\":[{\"type\":\"json\",\"data\":{\"answer\":42}}]}"],"protocol":"json","input_schema":{"type":"object"}},
                    "error":{"description":"Authored error","command":bin("/bin/echo"),"args":["{\"content\":[{\"type\":\"text\",\"text\":\"Owner error\"}],\"isError\":true}"],"protocol":"json","input_schema":{"type":"object"}}
                },
                "human_tools":{"env":{"description":"Environment","kind":"output","command":command,"args":[]}}
            },
            "evals":[
                {"id":"review","title":"Review","profile":profile,"payload":{"instruction":"Review a."}},
                {"id":"human","title":"Human","profile":{"kind":"human"},"payload":{"instruction":"Review a."}}
            ]
        })
        .to_string(),
    )
    .unwrap();
    support::declaration::write(
        repo.join("b/index.artf"),
        json!({"name":"b","views":{"agent_tools":{"read":{"builtin":"read"}}},"basis":true})
            .to_string(),
    )
    .unwrap();
    fs::write(repo.join("a/text.txt"), "scoped text\n").unwrap();
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
    fixture(&repo);
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
    let declaration_path = repo.join("a/index.artf");
    let original = fs::read(&declaration_path).unwrap();
    let mut declaration: Value = support::declaration::read(&original).unwrap();
    declaration["views"]["agent_tools"]["env"]["execution_paths"] = json!(["missing-input"]);
    support::declaration::write(&declaration_path, declaration.to_string()).unwrap();
    assert_eq!(parsed(&check(&repo, &state, &["a/review"]), 1)["ok"], false);
    assert!(!repo.join("a/touched").exists());
    support::declaration::write(&declaration_path, original).unwrap();
    let scoped = parsed(&check(&repo, &state, &["a/review"]), 0);
    assert_eq!(scoped["scopes"].as_array().unwrap().len(), 1);
    assert!(
        scoped["scopes"][0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["artifactId"] == "a")
    );
    // Windows has no execute bit to take away.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
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
    }
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
    let home = format!("{}home|unset|", std::path::MAIN_SEPARATOR);
    assert!(text.contains(&home), "{text}");
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
