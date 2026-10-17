#[path = "support/run.rs"]
mod owned_run;
use owned_run::launch::{LAUNCH, Launched};

use std::{fs, path::Path, process::Command, time::Duration};

use artifactize_core::{
    config::{parse_declaration, read_workspace_config},
    tools::{
        self,
        human::{Content, Registry, ToolResult},
    },
};
use serde_json::{Value, json};
use support::os::canonical;
use tokio_util::sync::CancellationToken;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

fn tool(kind: &str, command: &str, args: &[&str]) -> Value {
    json!({"description":"Inspect {artifactName}","kind":kind,"command":command,"args":args})
}

use support::os::python;

fn write_artifact(path: &Path, name: &str, tools: Value, instruction: &str) {
    fs::create_dir_all(path).unwrap();
    support::declaration::write(
        path.join("index.artf"),
        json!({
            "name":name,
            "views":{"human_tools":tools,"agent_tools":{"read":{"builtin":"read"}}},
            "evals":[
                {
                    "id":"review",
                    "title":"Review",
                    "profile":{"kind":"human"},
                    "payload":{"instruction":instruction},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
}

async fn call(repo: &Path) -> ToolResult {
    let config = read_workspace_config(repo).unwrap();
    Registry::new(&config, "a/review")
        .unwrap()
        .call(&"inspect_a".parse().unwrap(), CancellationToken::new())
        .await
}

fn text(result: &ToolResult) -> &str {
    let Content::Text { text } = &result.content()[0] else {
        panic!("{result:?}")
    };
    text
}

#[test]
fn flat_declarations_reject_free_arguments_and_unknown_placeholders_inertly() {
    let parse = |tool| {
        parse_declaration(
            &support::declaration::to_toml(
                json!({"name":"a","views":{"human_tools":{"inspect":tool}}}),
            )
            .expect("Test builder must be TOML-compatible; use raw TOML for invalid syntax."),
        )
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
        ("args", json!(["${ARTIFACTIZE_TEST_ARGUMENT}"])),
        ("args", json!(["prefix{artifactPath}"])),
        ("command", json!("{artifactPath}/tool")),
        ("timeout_ms", json!(0)),
        ("timeout_ms", json!(2_147_483_648u64)),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert!(parse(invalid.clone()).is_err(), "{invalid}");
    }
    assert!(
        parse(
            json!({"metadata":{"description":"Old"},"script":{"command":"noop-fixture","args":[]}})
        )
        .is_err()
    );
    let repo = support::os::tempdir();
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
    let output = Command::new(support::os::cli())
        .args([
            "--repo",
            repo.path().to_str().unwrap(),
            "config",
            "check",
            "--json",
        ])
        .record_output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[tokio::test]
async fn catalog_is_human_only_scoped_and_collision_checked() {
    let repo = support::os::tempdir();
    let command = tool("output", &support::os::printf_program(), &["ok"]);
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
                .call(&unknown.parse().unwrap(), CancellationToken::new())
                .await
                .is_error()
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
    let directory = support::os::tempdir();
    let repo = canonical(directory.path());
    let repo = repo.as_path();
    write_artifact(repo, "root", json!({}), "Review.");
    write_artifact(&repo.join("b"), "b", json!({}), "Review.");
    fs::write(repo.join("b/input"), "input").unwrap();
    let owner = repo.join("a");
    let command = tool(
        "output",
        &support::os::printf_program(),
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
    assert!(!result.is_error(), "{result:?}");
    let input = repo.join("b").join("input");
    assert_eq!(
        text(&result),
        format!(
            "{}\n--input={}\n{}\nliteral $(touch injected)\n",
            support::os::path_text(&owner),
            support::os::path_text(&input),
            support::os::path_text(&input)
        )
    );
    assert!(!owner.join("injected").exists());
    let config = read_workspace_config(repo).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    fs::remove_file(&input).unwrap();
    // A link to a folder outside the repository redirects the operand.
    support::os::link_dir(&support::os::temp_root(), &input);
    assert!(
        registry
            .call(&"inspect_a".parse().unwrap(), CancellationToken::new())
            .await
            .is_error()
    );
    declaration.as_object_mut().unwrap().remove("mounts");
    declaration["views"]["human_tools"]["inspect"]["args"] = json!(["{b}"]);
    support::declaration::write(marker, declaration.to_string()).unwrap();
    assert!(call(repo).await.is_error());
}

#[tokio::test]
async fn executable_resolution_matches_agent_tools_and_cwd_is_owner() {
    let directory = support::os::tempdir();
    let repo = canonical(directory.path());
    write_artifact(&repo, "root", json!({}), "Review.");
    let owner = repo.join("a");
    write_artifact(&owner, "a", json!({}), "Review.");
    let executable =
        support::os::working_directory_program(&owner, "unique-human-tool-not-on-path");
    let executable = support::os::path_text(&executable);
    for (command, success) in [
        ("unique-human-tool-not-on-path", false),
        ("./unique-human-tool-not-on-path", true),
        ("../a/unique-human-tool-not-on-path", false),
        (executable.as_str(), true),
    ] {
        write_artifact(
            &owner,
            "a",
            json!({"inspect":tool("output", command, &[])}),
            "Review.",
        );
        let result = call(&repo).await;
        assert_eq!(!result.is_error(), success, "{command}: {result:?}");
        if success {
            assert_eq!(text(&result), support::os::path_text(&owner));
        }
    }
}

#[tokio::test]
async fn output_cleans_bounds_both_streams_and_reports_nonzero_exit() {
    let repo = support::os::tempdir();
    write_artifact(
        repo.path(),
        "a",
        json!({
            "inspect":tool(
                "output",
                support::os::python_program(),
                &python(r"import os, sys; os.write(1, b'\x1b[31mhello\x1b[0m\x00\t\n'); os.write(2, b'\x1b[31mproblem\x1b[0m\x00'); sys.exit(3)"),
            ),
        }),
        "Review.",
    );
    let result = call(repo.path()).await;
    assert!(result.is_error());
    assert!(
        text(&result).contains(&support::os::exit_status(3)),
        "{result:?}"
    );
    assert!(text(&result).ends_with("hello\t\n"));
    assert_eq!(
        result.content()[1],
        Content::Text {
            text: "stderr:\nproblem".into()
        }
    );
    write_artifact(
        repo.path(),
        "a",
        json!({
            "inspect":tool(
                "output",
                support::os::python_program(),
                &python("import sys; print('界'*100000); print('界'*100000,file=sys.stderr)"),
            ),
        }),
        "Review.",
    );
    let result = call(repo.path()).await;
    assert!(!result.is_error());
    assert_eq!(result.content().len(), 2);
    for block in result.content() {
        let Content::Text { text } = block else {
            panic!()
        };
        assert!(text.len() <= 65536);
        assert!(text.ends_with("[output truncated]"));
    }
}

/// Every event a watched run has logged so far, waiting for at most a generous bound so a
/// broken stream fails the test instead of hanging it.
async fn next(
    events: &mut artifactize_core::review::log::Events,
) -> artifactize_core::review::log::Kind {
    tokio::time::timeout(Duration::from_secs(60), events.recv())
        .await
        .expect("the run logs its next event")
        .expect("the sink is alive")
        .kind
}

/// Lets a launched fixture program finish, also when an assertion unwinds first.
struct Release(std::path::PathBuf);

impl Drop for Release {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, "");
    }
}

#[tokio::test]
async fn a_watched_run_logs_both_streams_by_line_and_a_launch_logs_until_its_program_exits() {
    use artifactize_core::review::log::{End, Kind, Stream, channel};
    let repo = support::os::tempdir();
    write_artifact(
        repo.path(),
        "a",
        json!({
            "inspect":tool(
                "output",
                support::os::python_program(),
                &python("import sys; print('one'); print('oops', file=sys.stderr); print('two'); sys.exit(2)"),
            ),
        }),
        "Review.",
    );
    let config = read_workspace_config(repo.path()).unwrap();
    let (sink, mut events) = channel();
    let request = "run-1-1".parse().unwrap();
    let name = "inspect_a".parse().unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    let result = registry
        .call_logged(
            &name,
            CancellationToken::new(),
            sink.start(&request, "inspect", "a"),
        )
        .await;
    assert!(result.is_error());
    assert!(
        matches!(next(&mut events).await, Kind::Start { operation, .. } if operation == "inspect")
    );
    let mut lines = Vec::new();
    let end = loop {
        match next(&mut events).await {
            Kind::Line { stream, text } => lines.push((stream, text)),
            Kind::End(end) => break end,
            other => panic!("{other:?}"),
        }
    };
    // Each stream keeps its own order; the two pipes interleave as they arrive.
    let of = |wanted: Stream| -> Vec<&str> {
        lines
            .iter()
            .filter(|(stream, _)| *stream == wanted)
            .map(|(_, text)| text.as_str())
            .collect()
    };
    assert_eq!(of(Stream::Stdout), ["one", "two"]);
    assert_eq!(of(Stream::Stderr), ["oops"]);
    assert_eq!(end.text(), "■ exit 2");

    // A launch returns at once; what its program prints keeps arriving in the same run until
    // it exits, here once the test lets it.
    let release = Release(repo.path().join("release"));
    let waits = format!(
        "import os, sys, time\nprint('started', flush=True)\nfor _ in range(6000):\n    if os.path.exists({:?}): break\n    time.sleep(0.01)\nprint('after', file=sys.stderr, flush=True)",
        support::os::path_text(&release.0)
    );
    write_artifact(
        repo.path(),
        "a",
        json!({"inspect":tool("launch", support::os::python_program(), &python(&waits))}),
        "Review.",
    );
    let config = read_workspace_config(repo.path()).unwrap();
    let registry = Registry::new(&config, "a/review").unwrap();
    let result = registry
        .call_logged(
            &name,
            CancellationToken::new(),
            sink.start(&request, "inspect", "a"),
        )
        .await;
    assert_eq!(result.content(), [Content::Launch { launched: true }]);
    assert!(matches!(next(&mut events).await, Kind::Start { .. }));
    assert_eq!(
        next(&mut events).await,
        Kind::Line {
            stream: Stream::Stdout,
            text: "started".into()
        }
    );
    drop(release);
    assert_eq!(
        next(&mut events).await,
        Kind::Line {
            stream: Stream::Stderr,
            text: "after".into()
        }
    );
    assert!(matches!(
        next(&mut events).await,
        Kind::End(End::Exited { code: Some(0), .. })
    ));
}

#[tokio::test]
async fn output_timeout_and_pre_cancelled_launch_do_not_handoff() {
    let repo = support::os::tempdir();
    // Only the tool's declared deadline ends this process; no natural sleep lifetime races it.
    let (program, args) = support::os::lingering_command();
    let mut command = tool(
        "output",
        &program,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    command["timeout_ms"] = json!(40);
    write_artifact(repo.path(), "a", json!({"inspect":command}), "Review.");
    assert_eq!(text(&call(repo.path()).await), "Human tool timed out.");
    write_artifact(
        repo.path(),
        "a",
        json!({"inspect":tool("launch", &support::os::touch_program(), &["spawned"])}),
        "Review.",
    );
    let config = read_workspace_config(repo.path()).unwrap();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(
        Registry::new(&config, "a/review")
            .unwrap()
            .call(&"inspect_a".parse().unwrap(), cancellation)
            .await
            .is_error()
    );
    assert!(!repo.path().join("spawned").exists());
    write_artifact(
        repo.path(),
        "a",
        json!({"inspect":tool("launch", "missing-artifactize-human-program", &[])}),
        "Review.",
    );
    assert!(call(repo.path()).await.is_error());
    let mut launched = Launched::new(repo);
    write_artifact(
        launched.path(),
        "a",
        json!({"inspect":tool("launch", support::os::python_program(), &python(LAUNCH))}),
        "Review.",
    );
    assert_eq!(
        call(launched.path()).await.content(),
        [Content::Launch { launched: true }]
    );
    // Handoff acceptance is not the saved marker. Observe the full marker and adopt the
    // child while it is held by the fixture's release, including assertion-unwind paths.
    let expected = support::os::environment_variable("ARTIFACTIZE_HUMAN_MARKER")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "launched".into());
    support::os::wait_until(Duration::from_secs(5), || {
        fs::read_to_string(launched.path().join("marker")).unwrap_or_default() == expected
            && launched.own().is_some()
    });
}

#[tokio::test]
async fn detached_launch_cleanup_adopts_a_pending_handoff_on_drop_and_unwind() {
    for unwind in [false, true] {
        let repo = support::os::tempdir();
        fs::write(repo.path().join("hold-handoff"), "").unwrap();
        write_artifact(
            repo.path(),
            "a",
            json!({"inspect":tool("launch", support::os::python_program(), &python(LAUNCH))}),
            "Review.",
        );
        let launched = Launched::new(repo);
        assert_eq!(
            call(launched.path()).await.content(),
            [Content::Launch { launched: true }]
        );
        support::os::wait_until(Duration::from_secs(5), || {
            launched.path().join("handoff-ready").exists()
        });
        assert!(!launched.path().join("pid").exists(), "PID handoff is held");
        let root = launched.path().to_owned();
        // Retain the joined fixture long enough to independently read the child's PID.
        let inputs = launched.retain_inputs();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _owned = launched;
            assert!(!unwind, "pending detached launch unwind probe");
        }));
        assert_eq!(result.is_err(), unwind);
        let pid = fs::read_to_string(root.join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            !support::os::running(pid),
            "the pending child was adopted and stopped"
        );
        drop(inputs);
        assert!(!root.exists(), "child cleanup precedes fixture removal");
    }
}

#[test]
fn launch_outlives_host_and_an_agent_tool_sees_the_real_home_without_artifactize_variables() {
    for unwind in [false, true] {
        let repo = support::os::tempdir();
        // The parent owns the release: exiting the launcher probe must not end its child.
        let mut launched = Launched::new(repo);
        let result = support::os::run_human_test(
            "human_environment_and_launch_probe",
            launched.path(),
            &[
                (
                    "ARTIFACTIZE_HUMAN_PROBE",
                    launched.path().as_os_str().to_owned(),
                ),
                ("ARTIFACTIZE_HUMAN_MARKER", "reviewer-marker".into()),
            ],
        );
        let pid = launched.own();
        assert!(result.status.success(), "{result:?}");
        let pid = pid.expect("launcher saved its complete pid marker");
        // The probe that launched it has exited; the detached child outlives it, but may not
        // have finished becoming a tracked job member or process-group leader just yet.
        support::os::wait_until(Duration::from_secs(5), || support::os::running(pid));
        support::os::assert_detached(pid);
        assert!(support::os::running(pid), "detached child is still running");
        assert_eq!(
            fs::read_to_string(launched.path().join("marker")).unwrap(),
            "reviewer-marker"
        );
        let start = support::os::start_time(pid);
        let root = launched.path().to_owned();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _owned = launched;
            assert!(!unwind, "detached Human launch fixture unwind probe");
        }));
        assert_eq!(result.is_err(), unwind);
        assert!(!support::os::same_process_running(pid, start));
        assert!(
            !root.exists(),
            "detached child cleanup precedes fixture removal"
        );
    }
}

#[tokio::test]
async fn human_environment_and_launch_probe() {
    let Some(repo) = support::os::environment_variable("ARTIFACTIZE_HUMAN_PROBE") else {
        return;
    };
    let repo = Path::new(&repo);
    let output = support::os::tempdir();
    write_artifact(
        repo,
        "a",
        json!({
            "inspect":tool(
                "output",
                support::os::python_program(),
                &python(
                    &support::os::human_environment_probe(),
                ),
            ),
        }),
        "Review.",
    );
    let result = call(repo).await;
    assert!(!result.is_error(), "{result:?}");
    // Python prints native environment paths and uses CRLF on Windows. Normalize
    // those two path fields at the observation boundary through the portable helper.
    let observed = text(&result).replace("\r\n", "\n");
    let mut lines: Vec<String> = observed.split('\n').map(str::to_owned).collect();
    assert_eq!(lines.len(), 6, "{observed}");
    for index in [1, 4] {
        lines[index] = support::os::path_text(Path::new(&lines[index]));
    }
    assert_eq!(
        lines.join("\n"),
        format!(
            "reviewer-marker\n{}\n:77\nwayland-test\n{}\n",
            support::os::path_text(&repo.join("real-home")),
            support::os::path_text(&repo.join("real-config"))
        )
    );
    let mut declaration: Value =
        support::declaration::read(fs::read(repo.join("index.artf")).unwrap()).unwrap();
    declaration["evals"].as_array_mut().unwrap().push(json!({
        "id":"agent",
        "title":"Agent",
        "profile":{"kind":"agent","backend":"openai","model":"test","reasoning":"high"},
        "payload":{"instruction":"Review."},
    }));
    declaration["views"]["agent_tools"] = json!({
        "inspect":{
            "description":"Inspect",
            "protocol":"plain",
            "command":support::os::python_program(),
            "args":[
                "-c",
                support::os::inherited_home_probe(),
            ],
        },
    });
    declaration["views"]["human_tools"]["inspect"] =
        tool("launch", support::os::python_program(), &python(LAUNCH));
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
    assert!(!result.is_error(), "{result:?}");
    let tools::Content::Text { text } = &result.content()[0] else {
        panic!()
    };
    assert!(text.replace("\r\n", "\n").starts_with("absent\n"));
    // The author's own environment, home included; artifactize's variable is not in it.
    assert!(text.contains("real-home"));
    let result = call(repo).await;
    assert!(!result.is_error(), "{result:?}");
    assert_eq!(result.content(), [Content::Launch { launched: true }]);
    tokio::time::timeout(support::os::patience(Duration::from_secs(2)), async {
        while fs::read_to_string(repo.join("marker")).unwrap_or_default() != "reviewer-marker"
            || fs::read_to_string(repo.join("pid"))
                .ok()
                .and_then(|text| text.parse::<u32>().ok())
                .is_none()
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn human_results_preserve_saved_stdout_stderr_and_launch_wire_bytes() {
    for saved in [
        r#"{"content":[{"type":"text","text":""}],"isError":false}"#,
        r#"{"content":[{"type":"launch","launched":true}],"isError":false}"#,
        r#"{"content":[{"type":"text","text":"failed\nstdout"},{"type":"text","text":"stderr:\nproblem"}],"isError":true}"#,
    ] {
        let result: ToolResult = serde_json::from_str(saved).unwrap();
        assert_eq!(serde_json::to_string(&result).unwrap(), saved);
    }
    assert_eq!(
        serde_json::to_string(&ToolResult::error(" \n")).unwrap(),
        r#"{"content":[{"type":"text","text":"Tool failed."}],"isError":true}"#,
    );
    for invalid in [
        json!({"content":[],"isError":false}),
        json!({"content":[],"isError":true}),
        json!({"content":[{"type":"launch","launched":true}],"isError":true}),
        json!({"content":[{"type":"text","text":" "}],"isError":true}),
        json!({"content":[{"type":"text","text":"failed"},{"type":"text","text":""}],"isError":true}),
    ] {
        assert!(serde_json::from_value::<ToolResult>(invalid).is_err());
    }
}
