#[path = "support/os.rs"]
mod os;
#[path = "support/process.rs"]
mod process;

fn call(root: &std::path::Path, args: &[&str]) -> Output {
    os::call(env!("CARGO_BIN_EXE_artifactize-tools"), root, args)
}
use std::{fs, process::Output};

fn text(output: Output) -> String {
    assert!(os::succeeded(&output.status), "{output:?}");
    artifactize_tools::text::output(&output.stdout).into_owned()
}

#[test]
fn subcommands_share_scoped_files_and_exit_codes() {
    let root = os::tempdir();
    fs::write(
        root.path().join("notes.md"),
        "# First\nalpha\n## Child\nchild\n# Next\nomega\n",
    )
    .unwrap();
    assert_eq!(
        text(call(root.path(), &["read", "notes.md"])),
        "# First\nalpha\n## Child\nchild\n# Next\nomega\n"
    );
    assert_eq!(
        text(call(root.path(), &["section", "notes.md", "First"])),
        "# First\nalpha\n## Child\nchild\n"
    );
    for (args, expected) in [
        (
            vec!["list", "."],
            serde_json::json!({
                "artifactId":"cwd", "path":"",
                "entries":[{"kind":"file", "name":"notes.md", "path":"notes.md"}],
                "totalEntries":1, "truncated":false, "nextOffset":null,
            }),
        ),
        (
            vec!["glob", "*.md"],
            serde_json::json!({"path":"", "files":["notes.md"], "truncated":false}),
        ),
        (
            vec!["grep", "alpha"],
            serde_json::json!({
                "path":"", "matches":[{"path":"notes.md", "line":2, "text":"alpha"}],
                "truncated":false,
            }),
        ),
    ] {
        let output = text(call(root.path(), &args));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            expected,
            "{args:?}"
        );
    }
    let missing = call(root.path(), &["section", "notes.md", "Absent"]);
    assert_eq!(os::exit_code(&missing), 1);
    let error = artifactize_tools::text::output(&missing.stderr).into_owned();
    assert!(error.contains("First\nChild\nNext"), "{error}");
    for sub in ["read", "list", "glob", "grep", "section"] {
        let mut args = match sub {
            "glob" => vec![sub, "*", "../outside"],
            "grep" => vec![sub, "alpha", "../outside"],
            "section" => vec![sub, "../outside", "First"],
            _ => vec![sub, "../outside"],
        };
        assert_eq!(os::exit_code(&call(root.path(), &args)), 1, "{sub}");
        let index = if matches!(sub, "glob" | "grep") { 2 } else { 1 };
        let absolute = root.path().join("notes.md");
        args[index] = absolute.to_str().unwrap();
        assert_eq!(os::exit_code(&call(root.path(), &args)), 1, "{sub}");
    }
    let help = text(call(root.path(), &["--help"]));
    assert!(help.contains("symlinks are refused"));
    assert_eq!(os::exit_code(&call(root.path(), &["unknown"])), 2);
}

#[test]
fn searches_validate_cli_patterns_and_keep_the_declared_flags() {
    let root = os::tempdir();
    fs::write(root.path().join("notes.md"), "ALPHA\nbeta\n").unwrap();
    for args in [["grep", "", "."], ["glob", "", "."], ["grep", "[", "."]] {
        let output = call(root.path(), &args);
        assert_eq!(os::exit_code(&output), 1, "{args:?}");
        assert!(!output.stderr.is_empty(), "{args:?}");
    }
    let output = text(call(
        root.path(),
        &["grep", "alpha", "--case-insensitive", "--glob", "*.md"],
    ));
    let result: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        result["matches"],
        serde_json::json!([{"path":"notes.md","line":1,"text":"ALPHA"}])
    );
}

#[test]
fn symlink_inputs_are_not_followed() {
    let root = os::tempdir();
    fs::write(root.path().join("real.md"), "# First\nbody\n").unwrap();
    let link = root.path().join("link.md");
    os::symlink_file(root.path().join("real.md"), &link);
    for args in [vec!["read", "link.md"], vec!["section", "link.md", "First"]] {
        let output = call(root.path(), &args);
        assert_eq!(os::exit_code(&output), 1);
    }
}

#[test]
fn help_resolves_path_and_enforces_output_bounds() {
    let root = os::tempdir();
    os::help_program(root.path());
    let run = |arg| {
        os::help(
            env!("CARGO_BIN_EXE_artifactize-tools"),
            root.path(),
            "stub",
            &[arg],
        )
    };
    assert_eq!(text(run("sub")), "stub help: sub|--help\n");
    let large = run("large");
    assert_eq!(os::exit_code(&large), 1);
    assert!(artifactize_tools::text::output(&large.stderr).contains("64 KiB"));
}

#[tokio::test]
async fn standalone_refuses_an_output_limit_without_room_for_the_probe() {
    use artifactize_tools::launch::{Launch, LaunchError, Launcher, Standalone};

    let root = os::tempdir();
    let launch = Launch {
        program: root.path().join("not-started"),
        args: Vec::new(),
        cwd: root.path().to_owned(),
        timeout: std::time::Duration::ZERO,
        output_limit: usize::MAX,
    };
    let error = Standalone
        .run(launch, &tokio_util::sync::CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, LaunchError::Failed(message) if message.contains("Output limit")));
}

#[tokio::test]
async fn help_timeout_ends_the_program_and_the_children_it_starts_at_once() {
    let root = os::tempdir();
    let result = process::timeout_tree(root.path()).await;
    assert!(
        matches!(
            result,
            Err(artifactize_tools::launch::LaunchError::TimedOut)
        ),
        "{result:?}"
    );
}

// A closing terminal sends SIGHUP: the launch stops and its program goes with it, long before
// the help timeout would end it.
#[cfg(unix)]
#[test]
fn a_terminal_hang_up_stops_the_launch_and_its_program() {
    use rustix::process::{Pid, Signal, kill_process, test_kill_process};
    use std::time::{Duration, Instant};

    let root = os::tempdir();
    let started = root.path().join("started");
    os::compile(
        root.path(),
        "stub",
        &format!(
            r#"fn main() {{
        std::fs::write({started:?}, std::process::id().to_string()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(60));
    }}"#
        ),
    );
    let mut cli = std::process::Command::new(env!("CARGO_BIN_EXE_artifactize-tools"))
        .current_dir(root.path())
        .env("PATH", root.path())
        .args(["help", "stub", "sub"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let program = loop {
        if let Some(pid) = fs::read_to_string(&started)
            .ok()
            .and_then(|text| text.parse().ok())
        {
            break Pid::from_raw(pid).unwrap();
        }
        assert!(Instant::now() < deadline, "the program did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    let hung_up = Instant::now();
    kill_process(Pid::from_child(&cli), Signal::HUP).unwrap();
    // Well under the 10 s help timeout, which would otherwise be what ends it.
    let bound = Duration::from_secs(5);
    while cli.try_wait().unwrap().is_none() {
        assert!(
            hung_up.elapsed() < bound,
            "the CLI kept running after SIGHUP"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    while test_kill_process(program).is_ok() {
        assert!(
            hung_up.elapsed() < bound,
            "the launched program outlived the hang-up"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[tokio::test]
async fn help_tree_fixture_cleans_up_when_its_caller_unwinds() {
    let root = os::tempdir();
    process::unwind_tree(root.path()).await;
}

// Only Unix has a native opener subprocess; the portable CLI recorder also runs on Windows.
#[cfg(unix)]
#[test]
fn open_passes_one_absolute_target_to_the_recording_opener() {
    let root = os::tempdir();
    os::recording_program(root.path(), os::opener_program());
    let file = os::opener_input(root.path());
    let record = root.path().join("record");
    let output = os::open_recording(
        env!("CARGO_BIN_EXE_artifactize-tools"),
        root.path(),
        file.file_name().unwrap().to_str().unwrap(),
    );
    assert!(os::succeeded(&output.status), "{output:?}");
    assert_eq!(
        fs::read_to_string(record).unwrap(),
        format!("1\n{}\n", file.display())
    );
}

#[test]
fn version_matches_the_shared_workspace_version() {
    let output = os::tools_output(env!("CARGO_BIN_EXE_artifactize-tools"), &["--version"]);
    assert!(os::succeeded(&output.status));
    assert_eq!(
        artifactize_tools::text::output(&output.stdout),
        format!("artifactize-tools {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

// Only Unix permits a backslash as one literal directory-entry character.
#[cfg(unix)]
#[test]
fn list_and_glob_omit_native_names_that_cannot_be_reopened_logically() {
    let root = os::tempdir();
    os::write_unrepresentable_input(root.path());
    fs::write(root.path().join("visible.md"), "visible").unwrap();
    let list: serde_json::Value =
        serde_json::from_str(&text(call(root.path(), &["list"]))).unwrap();
    assert_eq!(
        list["entries"],
        serde_json::json!([{"kind":"file", "name":"visible.md", "path":"visible.md"}])
    );
    let glob: serde_json::Value =
        serde_json::from_str(&text(call(root.path(), &["glob", "*.md"]))).unwrap();
    assert_eq!(glob["files"], serde_json::json!(["visible.md"]));
}
