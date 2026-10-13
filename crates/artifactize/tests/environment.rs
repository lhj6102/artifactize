//! Commands an author declares run in the author's own terminal environment: their real home,
//! their variables and the toolchains found through them. Only what is artifactize's own
//! differs: its credentials and settings stay out, and each run has a private temporary
//! directory that goes with it.
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

/// What a runtime eval or a command Agent tool is expected to see. The "toolchain" is a
/// proxy that resolves through the home directory, as rustup's `cargo` does.
const PROBE: &str = r#"import json, os, pathlib, sys
home = pathlib.Path(os.environ['HOME_NAME'])
assert str(home) == sys.argv[1], home
assert os.environ['AUTHOR_VARIABLE'] == 'from-the-terminal'
assert (home / '.toolchains' / 'stable' / 'version').read_text().strip() == 'cargo 9.9'
assert 'ARTIFACTIZE_REMOTE_TOKEN' not in os.environ
assert 'ARTIFACTIZE_REMOTE' not in os.environ
tmp = pathlib.Path(os.environ['TMPDIR'])
assert tmp.is_dir() and not tmp.is_relative_to(home)
(home / ('seen-tmp-' + sys.argv[2])).write_text(str(tmp))
if sys.argv[2] == 'tool':
    sys.stdin.read()
    print(json.dumps({'content': [{'type': 'text', 'text': 'ok'}]}))
"#;

struct Project {
    root: tempfile::TempDir,
}

impl Project {
    fn new(declare: impl FnOnce(&str, &str) -> Value) -> Self {
        let root = support::os::tempdir();
        let home = root.path().join("home");
        fs::create_dir_all(home.join(".toolchains/stable")).unwrap();
        fs::write(home.join(".toolchains/stable/version"), "cargo 9.9\n").unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        support::os::write_python_script(
            &repo.join("probe.py"),
            &PROBE.replace("HOME_NAME", support::os::home_env()),
        );
        let home = support::os::path_text(&home);
        support::declaration::write(
            repo.join("index.artf"),
            declare(support::os::python_program(), &home).to_string(),
        )
        .unwrap();
        Self { root }
    }

    fn home(&self) -> std::path::PathBuf {
        self.root.path().join("home")
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(support::os::cli())
            .arg("--repo")
            .arg(self.root.path().join("repo"))
            .arg("--state-dir")
            .arg(self.root.path().join("state"))
            .args(args)
            .env("ARTIFACTIZE_REMOTE", "off")
            .env("ARTIFACTIZE_REMOTE_TOKEN", "azt_not-for-commands")
            .env("AUTHOR_VARIABLE", "from-the-terminal")
            .envs(support::os::home_variables(self.home()))
            .record_output()
            .unwrap()
    }

    fn seen_tmp(&self, kind: &str) -> std::path::PathBuf {
        fs::read_to_string(self.home().join(format!("seen-tmp-{kind}")))
            .unwrap()
            .into()
    }
}

fn check_succeeded(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_runtime_eval_runs_in_the_authors_environment_and_its_tmp_goes_with_the_run() {
    let project = Project::new(|python, home| {
        json!({
            "name": "app",
            "fingerprint": {},
            "evals": [{
                "id": "probe",
                "title": "The command sees the author's environment",
                "profile": {"kind": "runtime", "command": python, "args": ["probe.py", home, "eval"]},
                "payload": {"instruction": "Probe."},
            }],
        })
    });
    check_succeeded(&project.run(&["verify", "--wait", "--all", "--json"]));
    // The private temporary directory was removed when the run ended.
    assert!(!project.seen_tmp("eval").exists());
}

#[test]
fn a_command_agent_tool_runs_in_the_authors_environment() {
    let project = Project::new(|python, home| {
        json!({
            "name": "app",
            "fingerprint": {},
            "views": {"agent_tools": {"probe": {
                "description": "Probe the environment",
                "protocol": "json",
                "command": python,
                "args": ["probe.py", home, "tool"],
            }}},
            "evals": [{
                "id": "review",
                "title": "Review",
                "profile": {"kind": "agent", "backend": "openai", "model": "fake-model"},
                "payload": {"instruction": "Review."},
            }],
        })
    });
    let output = project.run(&[
        "tools",
        "check",
        "--execute",
        "--artifact",
        "app",
        "--audience",
        "agent",
        "--tool",
        "probe",
    ]);
    check_succeeded(&output);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["result"]["content"][0]["text"], "ok", "{report}");
    assert!(!Path::new(&project.seen_tmp("tool")).exists());
}
