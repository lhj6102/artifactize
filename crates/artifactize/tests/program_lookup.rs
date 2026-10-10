//! Declared commands share lookup, preflight and literal argv across audiences.

mod support;

use std::{
    fs,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

struct Project {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
    path: std::ffi::OsString,
}

impl Project {
    fn new() -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        let programs = root.path().join("programs with spaces");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(&programs).unwrap();
        fs::write(programs.join("argv.py"), "import json,pathlib,sys\nif sys.argv[1:] == ['fingerprint']:\n print('lookup-key')\nelif sys.argv[1:] == ['launch']:\n pathlib.Path('launched').write_text('launched')\nelse:\n print(json.dumps(sys.argv[1:]))\n").unwrap();
        support::os::lookup_shim(&programs);
        Self {
            state: root.path().join("state"),
            repo,
            path: support::os::prepend_path(&programs),
            _root: root,
        }
    }

    fn declaration(&self, args: Value) {
        support::declaration::write(self.repo.join("index.artf"), json!({
            "name":"test",
            "fingerprint":{"script":{"command":"artifactize-lookup-shim","args":["fingerprint"]}},
            "views":{
                "agent_tools":{"argv":{"description":"Arguments","protocol":"plain","command":"artifactize-lookup-shim","args":args,"input_schema":{"type":"object","additionalProperties":false}}},
                "human_tools":{
                    "argv":{"description":"Arguments","kind":"output","command":"artifactize-lookup-shim","args":args},
                    "launch":{"description":"Launch","kind":"launch","command":"artifactize-lookup-shim","args":["launch"]},
                },
            },
            "evals":[{"id":"check","title":"Check","profile":{"kind":"runtime","command":"artifactize-lookup-shim","args":args},"payload":{"instruction":"Check."}}],
        }).to_string()).unwrap();
    }

    fn run(&self, args: &[&str], code: i32) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .arg("--json")
            .env("PATH", &self.path)
            .env(support::os::PATHEXT.0, support::os::PATHEXT.1)
            .env("ARTIFACTIZE_REMOTE", "off")
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
    }
}

#[test]
fn declared_eval_fingerprint_agent_and_human_share_path_lookup_and_literal_arguments() {
    let project = Project::new();
    let args = json!(["a b", "say \"hello\"", "a&b", ""]);
    project.declaration(args.clone());
    let checked = project.run(&["tools", "check"], 0);
    assert_eq!(checked["ok"], true);
    let run = project.run(&["verify", "--all"], 0);
    assert_eq!(run["status"], "GREEN");
    let stdout = run["requests"][0]["result"]["stdout"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(stdout).unwrap(), args);
    for audience in ["agent", "human"] {
        let report = project.run(
            &[
                "tools",
                "check",
                "--execute",
                "--artifact",
                "test",
                "--audience",
                audience,
                "--tool",
                "argv",
            ],
            0,
        );
        assert_ne!(report["result"]["isError"], true);
        let text = report["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), args);
    }
    let launched = project.run(
        &[
            "tools",
            "check",
            "--execute",
            "--artifact",
            "test",
            "--audience",
            "human",
            "--tool",
            "launch",
        ],
        0,
    );
    assert_eq!(launched["result"]["content"][0]["launched"], true);
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(5));
    while !project.repo.join("launched").exists() {
        assert!(Instant::now() < deadline, "detached shim did not run");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::read_to_string(project.repo.join("launched")).unwrap(),
        "launched"
    );
}

// Only Windows runs `.cmd` shims, whose arguments Rust refuses when cmd would misparse them.
#[cfg(windows)]
#[test]
fn batch_argument_refusal_is_clear_for_runtime_agent_human_output_and_launch() {
    let project = Project::new();
    project.declaration(json!(["unsafe\nargument"]));
    let run = project.run(&["verify", "--all"], 2);
    assert_eq!(run["status"], "ERROR");
    assert!(
        run.to_string().contains("batch file arguments are invalid"),
        "{run}"
    );
    for audience in ["agent", "human"] {
        let report = project.run(
            &[
                "tools",
                "check",
                "--execute",
                "--artifact",
                "test",
                "--audience",
                audience,
                "--tool",
                "argv",
            ],
            1,
        );
        assert_eq!(report["result"]["isError"], true);
        assert!(
            report
                .to_string()
                .contains("arguments could not be passed safely"),
            "{report}"
        );
    }
    let path = project.repo.join("index.artf");
    let mut declaration: Value = support::declaration::read(fs::read(&path).unwrap()).unwrap();
    declaration["views"]["human_tools"]["launch"]["args"] = json!(["unsafe\nargument"]);
    support::declaration::write(path, declaration.to_string()).unwrap();
    let report = project.run(
        &[
            "tools",
            "check",
            "--execute",
            "--artifact",
            "test",
            "--audience",
            "human",
            "--tool",
            "launch",
        ],
        1,
    );
    assert!(
        report
            .to_string()
            .contains("arguments could not be passed safely"),
        "{report}"
    );
}

#[test]
fn standalone_shell_preserves_quoted_argument_fields_including_empty_arguments() {
    let output = Command::new(support::os::stand_in_bin("sh"))
        .args([
            "-c",
            "printf '<%s>\\n' \"$@\"",
            "fixture",
            "a b",
            "say \"hello\"",
            "a&b",
            "",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"<a b>\n<say \"hello\">\n<a&b>\n<>\n");
    let output = Command::new(support::os::stand_in_bin("sh"))
        .args([
            "-c",
            "python3 -c 'import sys; print(len(sys.argv)-1)' \"$@\"",
            "fixture",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "0");
}
