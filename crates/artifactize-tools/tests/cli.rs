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
    String::from_utf8(output.stdout).unwrap()
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
    let error = String::from_utf8(missing.stderr).unwrap();
    assert!(error.contains("First\nChild\nNext"), "{error}");
    for sub in ["read", "list", "glob", "grep", "section", "open"] {
        let mut args = match sub {
            "glob" | "grep" => vec![sub, "*", "../outside"],
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
    if os::symlink_file(root.path().join("real.md"), &link).is_none() {
        return;
    }
    for args in [
        vec!["read", "link.md"],
        vec!["section", "link.md", "First"],
        vec!["open", "link.md"],
    ] {
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
    assert!(String::from_utf8(large.stderr).unwrap().contains("64 KiB"));
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

#[tokio::test]
async fn help_tree_fixture_cleans_up_when_its_caller_unwinds() {
    let root = os::tempdir();
    process::unwind_tree(root.path()).await;
}

// Windows opens through ShellExecute, which runs no program a test can stand in for.
#[cfg(unix)]
#[test]
fn open_passes_one_absolute_target_to_the_recording_opener() {
    let root = os::tempdir();
    os::recording_program(root.path(), os::OPENER);
    fs::write(root.path().join("space ; notes.md"), "notes").unwrap();
    let record = root.path().join("record");
    let output = os::open_recording(
        env!("CARGO_BIN_EXE_artifactize-tools"),
        root.path(),
        "space ; notes.md",
    );
    assert!(os::succeeded(&output.status), "{output:?}");
    assert_eq!(
        fs::read_to_string(record).unwrap(),
        format!("1\n{}\n", root.path().join("space ; notes.md").display())
    );
}

#[test]
fn version_matches_the_shared_workspace_version() {
    let output = os::tools_output(env!("CARGO_BIN_EXE_artifactize-tools"), &["--version"]);
    assert!(os::succeeded(&output.status));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("artifactize-tools {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}
