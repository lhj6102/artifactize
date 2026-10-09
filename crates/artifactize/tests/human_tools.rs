use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use artifactize::{
    config::{parse_declaration, read_workspace_config},
    tools::{
        self,
        human::{Content, Registry, ToolResult},
    },
};
use serde_json::{Value, json};
use support::os::{bin, canonical};
use tokio_util::sync::CancellationToken;

mod support;

fn tool(kind: &str, command: &str, args: &[&str]) -> Value {
    // The Unix utilities the tools name; elsewhere their Windows stand-ins.
    let command = match command {
        "sh" | "printf" | "sleep" | "touch" | "false" => bin(command),
        other => other.to_owned(),
    };
    json!({"description":"Inspect {artifactName}","kind":kind,"command":command,"args":args})
}

/// `python3 -c SOURCE`, writing UTF-8 to pipes on Windows as it does on Unix.
fn python(source: &str) -> Vec<&str> {
    let mut args = if cfg!(windows) {
        vec!["-X", "utf8"]
    } else {
        vec![]
    };
    args.extend(["-c", source]);
    args
}

fn write_artifact(path: &Path, name: &str, tools: Value, instruction: &str) {
    fs::create_dir_all(path).unwrap();
    support::declaration::write(path.join("index.artf"), json!({
        "name":name,"views":{"human_tools":tools,"agent_tools":{"read":{"builtin":"read"}}},
        "evals":[{"id":"review","title":"Review","profile":{"kind":"human"},"payload":{"instruction":instruction}}]
    }).to_string()).unwrap();
}

async fn call(repo: &Path) -> ToolResult {
    let config = read_workspace_config(repo).unwrap();
    Registry::new(&config, "a/review")
        .unwrap()
        .call("inspect_a", CancellationToken::new())
        .await
}

fn text(result: &ToolResult) -> &str {
    let Content::Text { text } = &result.content[0] else {
        panic!("{result:?}")
    };
    text
}

#[test]
fn flat_declarations_reject_free_arguments_and_unknown_placeholders_inertly() {
    let parse = |tool| {
        parse_declaration(&support::declaration::to_toml(
            json!({"name":"a","views":{"human_tools":{"inspect":tool}}}),
        )?)
    };
    let valid = tool("launch", "missing-program", &["{artifactPath}"]);
    assert!(parse(valid.clone()).is_ok());
    for (key, value) in [
        ("kind", json!("shell")),
        ("description", json!(" ")),
        ("description", json!("Open {unknown}")),
        ("input_schema", json!({"type":"object"})),
        ("protocol", json!("plain")),
        ("execution_paths", json!([])),
        ("args", json!([{"path":"free"}])),
        ("args", json!(["{artifactPath}/../outside"])),
        ("args", json!(["{bad.name}"])),
        ("args", json!(["{unclosed"])),
        ("args", json!(["${HOME}"])),
        ("args", json!(["prefix{artifactPath}"])),
        ("command", json!("{artifactPath}/tool")),
        ("timeout_ms", json!(null)),
        ("timeout_ms", json!(0)),
        ("timeout_ms", json!(2_147_483_648u64)),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert!(parse(invalid.clone()).is_err(), "{invalid}");
    }
    assert!(
        parse(json!({"metadata":{"description":"Old"},"script":{"command":"true","args":[]}}))
            .is_err()
    );
    let repo = tempfile::tempdir().unwrap();
    write_artifact(
        repo.path(),
        "a",
        json!({"inspect":tool("launch", "missing-program", &["{unknown}"])}),
        "Review.",
    );
    assert!(
        read_workspace_config(repo.path())
            .unwrap_err()
            .to_string()
            .contains("Unknown Artifact reference")
    );
    write_artifact(repo.path(), "a", json!({"inspect":valid}), "Review.");
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .args([
            "--repo",
            repo.path().to_str().unwrap(),
            "config",
            "check",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[tokio::test]
async fn catalog_is_human_only_scoped_and_collision_checked() {
    let repo = tempfile::tempdir().unwrap();
    let command = tool("output", "printf", &["ok"]);
    write_artifact(repo.path(), "root", json!({}), "Review.");
    write_artifact(
        &repo.path().join("a"),
        "a",
        json!({"inspect":command}),
        "Inspect {b}.",
    );
    write_artifact(
        &repo.path().join("b"),
        "b",
        json!({"inspect":command}),
        "Inspect {c}.",
    );
    write_artifact(
        &repo.path().join("b/child"),
        "child",
        json!({"inspect":command}),
        "Review.",
    );
    write_artifact(
        &repo.path().join("c"),
        "c",
        json!({"inspect":command}),
        "Review.",
    );
    let config = read_workspace_config(repo.path()).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    assert_eq!(
        registry
            .list()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["inspect_a", "inspect_b", "inspect_child"]
    );
    assert_eq!(registry.list().next().unwrap().description, "Inspect a");
    assert_eq!(
        Registry::for_artifact(&config, "a").unwrap().list().count(),
        1
    );
    assert!(Registry::new(&config, "missing").is_err());
    assert!(tools::Registry::new(&config, "a/review").is_err());
    for unknown in ["read_a", "inspect_c"] {
        assert!(
            registry
                .call(unknown, CancellationToken::new())
                .await
                .is_error
        );
    }
    write_artifact(
        &repo.path().join("a"),
        "a",
        json!({"inspect_b":command}),
        "Review.",
    );
    write_artifact(
        &repo.path().join("b"),
        "b_a",
        json!({"inspect":command}),
        "Review.",
    );
    let config = read_workspace_config(repo.path()).unwrap();
    assert!(
        Registry::for_artifact(&config, "root")
            .err()
            .unwrap()
            .contains("collide")
    );
}

#[tokio::test]
async fn scope_operands_use_runtime_resolution_and_recheck_symlinks() {
    let directory = tempfile::tempdir().unwrap();
    let repo = canonical(directory.path());
    let repo = repo.as_path();
    write_artifact(repo, "root", json!({}), "Review.");
    write_artifact(&repo.join("b"), "b", json!({}), "Review.");
    fs::write(repo.join("b/input"), "input").unwrap();
    let owner = repo.join("a");
    let command = tool(
        "output",
        "printf",
        &[
            "%s\n",
            "{artifactPath}",
            "--input={source}/input",
            "{a}/source/input",
            "literal $(touch injected)",
        ],
    );
    write_artifact(&owner, "a", json!({"inspect":command}), "Review.");
    let marker = owner.join("index.artf");
    let mut declaration: Value = support::declaration::read(fs::read(&marker).unwrap()).unwrap();
    declaration["mounts"] = json!({"source":"b"});
    support::declaration::write(&marker, declaration.to_string()).unwrap();
    let result = call(repo).await;
    assert!(!result.is_error, "{result:?}");
    let input = repo.join("b").join("input");
    assert_eq!(
        text(&result),
        format!(
            "{}\n--input={}\n{}\nliteral $(touch injected)\n",
            owner.display(),
            input.display(),
            input.display()
        )
    );
    assert!(!owner.join("injected").exists());
    let config = read_workspace_config(repo).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    fs::remove_file(&input).unwrap();
    #[cfg(unix)]
    support::os::symlink_file("/etc/passwd", &input).unwrap();
    // A junction needs no privilege and redirects the operand just the same.
    #[cfg(windows)]
    support::os::junction(&std::env::temp_dir(), &input);
    assert!(
        registry
            .call("inspect_a", CancellationToken::new())
            .await
            .is_error
    );
    declaration.as_object_mut().unwrap().remove("mounts");
    declaration["views"]["human_tools"]["inspect"]["args"] = json!(["{b}"]);
    support::declaration::write(marker, declaration.to_string()).unwrap();
    assert!(call(repo).await.is_error);
}

#[tokio::test]
async fn executable_resolution_matches_agent_tools_and_cwd_is_owner() {
    let directory = tempfile::tempdir().unwrap();
    let repo = canonical(directory.path());
    write_artifact(&repo, "root", json!({}), "Review.");
    let owner = repo.join("a");
    write_artifact(&owner, "a", json!({}), "Review.");
    let executable = owner.join("unique-human-tool-not-on-path");
    fs::write(&executable, "#!/bin/sh\nprintf '%s' \"$PWD\"\n").unwrap();
    support::os::make_executable(&executable);
    for (command, success) in [
        ("unique-human-tool-not-on-path", false),
        ("./unique-human-tool-not-on-path", true),
        ("../a/unique-human-tool-not-on-path", false),
        (executable.to_str().unwrap(), true),
    ] {
        write_artifact(
            &owner,
            "a",
            json!({"inspect":tool("output", command, &[])}),
            "Review.",
        );
        let result = call(&repo).await;
        assert_eq!(!result.is_error, success, "{command}: {result:?}");
        if success {
            assert_eq!(text(&result), owner.to_str().unwrap());
        }
    }
}

#[tokio::test]
async fn output_cleans_bounds_both_streams_and_reports_nonzero_exit() {
    let repo = tempfile::tempdir().unwrap();
    write_artifact(
        repo.path(),
        "a",
        json!({"inspect":tool("output", "sh", &["-c", "printf '\\033[31mhello\\033[0m\\000\\t\\n'; printf '\\033[31mproblem\\033[0m\\000' >&2; exit 3"])}),
        "Review.",
    );
    let result = call(repo.path()).await;
    assert!(result.is_error);
    // Windows says "exit code" for the same status.
    let status = if cfg!(windows) {
        "exit code: 3"
    } else {
        "exit status: 3"
    };
    assert!(text(&result).contains(status), "{result:?}");
    assert!(text(&result).ends_with("hello\t\n"));
    assert_eq!(
        result.content[1],
        Content::Text {
            text: "stderr:\nproblem".into()
        }
    );
    write_artifact(
        repo.path(),
        "a",
        json!({"inspect":tool("output", "python3", &python("import sys; print('界'*100000); print('界'*100000,file=sys.stderr)"))}),
        "Review.",
    );
    let result = call(repo.path()).await;
    assert!(!result.is_error);
    assert_eq!(result.content.len(), 2);
    for block in result.content {
        let Content::Text { text } = block else {
            panic!()
        };
        assert!(text.len() <= 65536);
        assert!(text.ends_with("[output truncated]"));
    }
}

#[tokio::test]
async fn output_timeout_and_pre_cancelled_launch_do_not_handoff() {
    let repo = tempfile::tempdir().unwrap();
    let mut command = tool("output", "sleep", &["60"]);
    command["timeout_ms"] = json!(40);
    write_artifact(repo.path(), "a", json!({"inspect":command}), "Review.");
    assert_eq!(text(&call(repo.path()).await), "Human tool timed out.");
    write_artifact(
        repo.path(),
        "a",
        json!({"inspect":tool("launch", "touch", &["spawned"])}),
        "Review.",
    );
    let config = read_workspace_config(repo.path()).unwrap();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(
        Registry::new(&config, "a/review")
            .unwrap()
            .call("inspect_a", cancellation)
            .await
            .is_error
    );
    assert!(!repo.path().join("spawned").exists());
    write_artifact(
        repo.path(),
        "a",
        json!({"inspect":tool("launch", "missing-artifactize-human-program", &[])}),
        "Review.",
    );
    assert!(call(repo.path()).await.is_error);
    write_artifact(
        repo.path(),
        "a",
        json!({"inspect":tool("launch", "false", &[])}),
        "Review.",
    );
    assert_eq!(
        call(repo.path()).await.content,
        [Content::Launch { launched: true }]
    );
}

struct Launched(u32);
impl Drop for Launched {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        support::os::kill(self.0);
    }
}

#[test]
fn launch_outlives_host_and_human_environment_is_not_agent_environment() {
    let repo = tempfile::tempdir().unwrap();
    // Build the Windows stand-ins before the probe needs them.
    bin("sh");
    let started = Instant::now();
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "human_environment_and_launch_probe",
            "--nocapture",
        ])
        .env("ARTIFACTIZE_HUMAN_PROBE", repo.path())
        .env("ARTIFACTIZE_HUMAN_MARKER", "reviewer-marker")
        .env("HOME", repo.path().join("real-home"))
        .env("DISPLAY", ":77")
        .env("WAYLAND_DISPLAY", "wayland-test")
        .env("XDG_CONFIG_HOME", repo.path().join("real-config"))
        .output()
        .unwrap();
    let pid = fs::read_to_string(repo.path().join("pid"))
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let child = pid.map(Launched);
    assert!(result.status.success(), "{result:?}");
    let child = child.expect("launcher wrote its pid");
    assert!(started.elapsed() < Duration::from_secs(3));
    #[cfg(unix)]
    {
        let stat = fs::read_to_string(format!("/proc/{}/stat", child.0)).unwrap();
        let fields: Vec<_> = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .collect();
        assert_ne!(fields[0], "Z", "detached child is still running");
        assert_eq!(
            fields[2].parse::<u32>().unwrap(),
            child.0,
            "separate process group"
        );
        assert_eq!(
            fields[3].parse::<u32>().unwrap(),
            child.0,
            "separate session"
        );
    }
    // The probe that launched it has exited; the detached child keeps running.
    #[cfg(windows)]
    assert!(
        support::os::running(child.0),
        "detached child is still running"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("marker")).unwrap(),
        "reviewer-marker"
    );
}

#[tokio::test]
async fn human_environment_and_launch_probe() {
    let Some(repo) = std::env::var_os("ARTIFACTIZE_HUMAN_PROBE") else {
        return;
    };
    let repo = Path::new(&repo);
    let output = tempfile::tempdir().unwrap();
    write_artifact(
        repo,
        "a",
        json!({"inspect":tool("output", "python3", &python("import os; print(os.environ['ARTIFACTIZE_HUMAN_MARKER']); print(os.environ['HOME']); print(os.environ['DISPLAY']); print(os.environ['WAYLAND_DISPLAY']); print(os.environ['XDG_CONFIG_HOME'])"))}),
        "Review.",
    );
    let result = call(repo).await;
    assert!(!result.is_error, "{result:?}");
    // Python ends its printed lines with CRLF on Windows.
    assert_eq!(
        text(&result).replace("\r\n", "\n"),
        format!(
            "reviewer-marker\n{}\n:77\nwayland-test\n{}\n",
            repo.join("real-home").display(),
            repo.join("real-config").display()
        )
    );
    let mut declaration: Value =
        support::declaration::read(fs::read(repo.join("index.artf")).unwrap()).unwrap();
    declaration["evals"].as_array_mut().unwrap().push(json!({"id":"agent","title":"Agent","profile":{"kind":"agent","backend":"openai","model":"test","reasoning":"high"},"payload":{"instruction":"Review."}}));
    declaration["views"]["agent_tools"] = json!({"inspect":{"description":"Inspect","protocol":"plain","command":"python3","args":["-c","import os; print(os.environ.get('ARTIFACTIZE_HUMAN_MARKER','absent')); print(os.environ['HOME'])"]}});
    declaration["views"]["human_tools"]["inspect"] = tool(
        "launch",
        "sh",
        &[
            "-c",
            "printf '%s' $$ > pid; printf '%s' \"$ARTIFACTIZE_HUMAN_MARKER\" > marker; exec sleep 10",
        ],
    );
    support::declaration::write(repo.join("index.artf"), declaration.to_string()).unwrap();
    let config = read_workspace_config(repo).unwrap();
    assert!(Registry::new(&config, "a/agent").is_err());
    let result = tools::Registry::new(&config, "a/agent")
        .unwrap()
        .call(
            "inspect_a",
            json!({}),
            output.path(),
            CancellationToken::new(),
        )
        .await;
    assert!(!result.is_error, "{result:?}");
    let tools::Content::Text { text } = &result.content[0] else {
        panic!()
    };
    assert!(text.replace("\r\n", "\n").starts_with("absent\n"));
    assert!(!text.contains("real-home"));
    let started = Instant::now();
    let result = call(repo).await;
    assert!(!result.is_error, "{result:?}");
    assert_eq!(result.content, [Content::Launch { launched: true }]);
    assert!(started.elapsed() < Duration::from_secs(1));
    tokio::time::timeout(support::os::patience(Duration::from_secs(2)), async {
        while !repo.join("marker").exists()
            || fs::read_to_string(repo.join("pid"))
                .unwrap_or_default()
                .is_empty()
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
