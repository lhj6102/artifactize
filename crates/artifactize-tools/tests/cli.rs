#[path = "support/os.rs"]
mod os;

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

/// The artifactize-tools command Cargo built for these tests.
fn tools() -> Command {
    Command::new(env!("CARGO_BIN_EXE_artifactize-tools"))
}

fn call(root: &Path, args: &[&str]) -> Output {
    tools().current_dir(root).args(args).output().unwrap()
}
fn text(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
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
    for args in [
        vec!["list", "."],
        vec!["glob", "*.md"],
        vec!["grep", "alpha"],
    ] {
        let output = text(call(root.path(), &args));
        let _: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(output.contains("notes.md"));
    }
    let missing = call(root.path(), &["section", "notes.md", "Absent"]);
    assert_eq!(missing.status.code(), Some(1));
    let error = String::from_utf8(missing.stderr).unwrap();
    assert!(error.contains("First\nChild\nNext"), "{error}");
    for sub in ["read", "list", "glob", "grep", "section", "open"] {
        let mut args = match sub {
            "glob" | "grep" => vec![sub, "*", "../outside"],
            "section" => vec![sub, "../outside", "First"],
            _ => vec![sub, "../outside"],
        };
        assert_eq!(call(root.path(), &args).status.code(), Some(1), "{sub}");
        let index = if matches!(sub, "glob" | "grep") { 2 } else { 1 };
        let absolute = root.path().join("notes.md");
        args[index] = absolute.to_str().unwrap();
        assert_eq!(call(root.path(), &args).status.code(), Some(1), "{sub}");
    }
    let help = text(call(root.path(), &["--help"]));
    assert!(help.contains("symlinks are refused"));
    assert_eq!(call(root.path(), &["unknown"]).status.code(), Some(2));
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
        assert_eq!(output.status.code(), Some(1));
    }
}

#[test]
fn help_resolves_path_and_enforces_output_and_timeout_bounds() {
    let root = os::tempdir();
    os::compile(
        root.path(),
        "stub",
        r#"fn main() {
        let args: Vec<_> = std::env::args().skip(1).collect();
        assert_eq!(args.last().map(String::as_str), Some("--help"));
        match args.first().map(String::as_str) {
            Some("large") => print!("{}", "x".repeat(65537)),
            // Never finishes on its own: only the tool's timeout ends it.
            Some("wait") => loop {
                std::thread::park();
            },
            _ => println!("stub help: {}", args.join("|")),
        }
    }"#,
    );
    let run = |arg| {
        tools()
            .current_dir(root.path())
            .env("PATH", root.path())
            .args(["help", "stub", arg])
            .output()
            .unwrap()
    };
    assert_eq!(text(run("sub")), "stub help: sub|--help\n");
    let large = run("large");
    assert_eq!(large.status.code(), Some(1));
    assert!(String::from_utf8(large.stderr).unwrap().contains("64 KiB"));
    let wait = run("wait");
    assert_eq!(wait.status.code(), Some(1));
    assert!(
        String::from_utf8(wait.stderr)
            .unwrap()
            .contains("timed out")
    );
}

#[test]
fn help_timeout_ends_the_program_and_the_children_it_starts_at_once() {
    let root = os::tempdir();
    os::compile(
        root.path(),
        "spawner",
        r#"fn main() {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("child") {
            std::fs::write(&args[2], std::process::id().to_string()).unwrap();
        } else {
            // Start a child at once, before anything else, then never finish.
            let record = std::env::current_dir().unwrap().join("child.pid");
            std::process::Command::new(std::env::current_exe().unwrap())
                .arg("child")
                .arg(&record)
                .spawn()
                .unwrap();
        }
        loop {
            std::thread::park();
        }
    }"#,
    );
    let output = tools()
        .current_dir(root.path())
        .env("PATH", root.path())
        .args(["help", "spawner"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("timed out")
    );
    let pid: u32 = fs::read_to_string(root.path().join("child.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        os::ends(pid),
        "the help program's child outlived the timeout"
    );
}

// Windows opens through ShellExecute, which runs no program a test can stand in for.
#[cfg(unix)]
#[test]
fn open_passes_one_absolute_target_to_the_recording_opener() {
    let root = os::tempdir();
    os::recording_program(root.path(), os::OPENER);
    fs::write(root.path().join("space ; notes.md"), "notes").unwrap();
    let record = root.path().join("record");
    let output = tools()
        .current_dir(root.path())
        .env("PATH", root.path())
        .env(os::RECORD, &record)
        .args(["open", "space ; notes.md"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(record).unwrap(),
        format!(
            "1\n{}\n",
            fs::canonicalize(root.path().join("space ; notes.md"))
                .unwrap()
                .display()
        )
    );
}

#[test]
fn version_matches_the_shared_workspace_version() {
    let output = tools().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("artifactize-tools {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}
