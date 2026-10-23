//! Declared commands share lookup, preflight and literal argv across audiences.

#[path = "support/run.rs"]
mod owned_run;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

use owned_run::launch::{LAUNCH, Launched};
use std::{cell::RefCell, fs, path::PathBuf, process::Command, time::Duration};

use serde_json::{Value, json};

// Bounds observe broken fixture handoff, never order execution or set a child lifetime.
const LAUNCH_BOUND: Duration = Duration::from_secs(10);

struct Project {
    launched: RefCell<Option<Launched>>,
    repo: PathBuf,
    state: PathBuf,
    path: std::ffi::OsString,
    _root: owned_run::Root,
}

impl Drop for Project {
    fn drop(&mut self) {
        // Detached launches are not Runs. Release both owners before temporary files,
        // even when an assertion or CLI output parsing unwinds after the handoff.
        if let Some(launched) = self.launched.get_mut().take() {
            // The launch owner is consumed, never reused after unwinding; keep the outer
            // cwd as well if even its guarded cleanup unexpectedly panics.
            let cleanup =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || launched.close()))
                    .unwrap_or_else(|_| Err("Detached lookup cleanup panicked".into()));
            if let Err(error) = cleanup {
                self._root.retain_inputs();
                if std::thread::panicking() {
                    eprintln!("{error}");
                } else {
                    panic!("{error}");
                }
            }
        }
        // Root releases every registered Run even when detached cleanup failed above.
    }
}

// Literal argv and native lookup are separate from the shared detached-launch lifecycle.
const ARGV_PROBE: &str = r"import json, os, pathlib, sys
if sys.argv[1:] == ['fingerprint']:
    print('lookup-key')
elif sys.argv[1:] == ['launch']:
    sys.argv = [sys.argv[0], os.environ['LOOKUP_LAUNCH_DIR']]
    OWNED_PIDS = LAUNCH_PIDS
    exec(LAUNCH_SOURCE)
else:
    print(json.dumps(sys.argv[1:]))
";

impl Project {
    fn new() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        root.track(&root.path().join("state"), &repo);
        let programs = root.path().join("programs with spaces");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(&programs).unwrap();
        fs::write(
            programs.join("argv.py"),
            ARGV_PROBE
                .replace("LAUNCH_PIDS", support::os::launch_probe_pids())
                .replace("LAUNCH_SOURCE", &serde_json::to_string(LAUNCH).unwrap()),
        )
        .unwrap();
        support::os::lookup_shim(&programs);
        Self {
            launched: RefCell::new(None),
            state: root.path().join("state"),
            repo,
            path: support::os::prepend_path(&programs),
            _root: root,
        }
    }

    fn launch(&self) -> Value {
        // Install the owner before any spawn, status assertion or JSON parsing can fail.
        {
            let mut owner = self.launched.borrow_mut();
            assert!(owner.is_none(), "one launch per lookup fixture");
            *owner = Some(Launched::new(support::os::tempdir()));
        }
        self.run(
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
        )
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

    fn launch_path(&self) -> PathBuf {
        self.launched
            .borrow()
            .as_ref()
            .expect("the launch fixture owns its inputs")
            .path()
            .to_owned()
    }

    fn run(&self, args: &[&str], code: i32) -> Value {
        let mut command = Command::new(support::os::cli());
        if let Some(launched) = self.launched.borrow().as_ref() {
            command.env("LOOKUP_LAUNCH_DIR", launched.path());
        }
        let output = command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .arg("--json")
            .env("PATH", &self.path)
            .env(support::os::PATHEXT.0, support::os::PATHEXT.1)
            .env("ARTIFACTIZE_REMOTE", "off")
            .record_output()
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
    let run = project.run(&["verify", "--wait", "--all"], 0);
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
    let launched = project.launch();
    assert_eq!(launched["result"]["content"][0]["launched"], true);
    support::os::wait_until(LAUNCH_BOUND, || {
        fs::read_to_string(project.launch_path().join("marker"))
            .is_ok_and(|contents| contents == "launched")
    });
    assert_eq!(
        fs::read_to_string(project.launch_path().join("marker")).unwrap(),
        "launched"
    );
}

#[test]
fn lookup_launch_exits_before_temporary_files_on_normal_drop_and_assertion_unwind() {
    for unwind in [false, true] {
        let project = Project::new();
        project.declaration(json!([]));
        let launched = project.launch();
        assert_eq!(launched["result"]["content"][0]["launched"], true);
        support::os::wait_until(LAUNCH_BOUND, || {
            fs::read_to_string(project.launch_path().join("marker"))
                .is_ok_and(|contents| contents == "launched")
        });
        assert_eq!(
            fs::read_to_string(project.launch_path().join("marker")).unwrap(),
            "launched"
        );
        let pids: Vec<u32> =
            serde_json::from_slice(&fs::read(project.launch_path().join("pids")).unwrap()).unwrap();
        assert!(!pids.is_empty(), "the probe reports its actual processes");
        let identities: Vec<_> = pids
            .into_iter()
            .map(|pid| (pid, support::os::start_time(pid)))
            .collect();
        assert!(
            identities
                .iter()
                .all(|&(pid, birth)| support::os::same_process_running(pid, birth))
        );

        // Retain only the files to observe their ordering independently of the process
        // owner. The guard must discover/adopt the PID itself, including on unwind.
        let inputs = project.launched.borrow().as_ref().unwrap().retain_inputs();
        let root_path = project.launch_path();
        // The fixture is moved into the closure and disposed, never reused after unwind;
        // its Rc/RefCell only shares the retained input directory with this observation.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _project = project;
            assert!(!unwind, "lookup launch fixture assertion unwind probe");
        }));
        assert_eq!(result.is_err(), unwind);
        assert!(root_path.is_dir(), "temporary files are still retained");
        assert_eq!(
            fs::read_to_string(root_path.join("marker")).unwrap(),
            "launched"
        );
        assert!(
            identities
                .iter()
                .all(|&(pid, birth)| !support::os::same_process_running(pid, birth)),
            "all detached lookup probe processes must exit before their files are removed"
        );
        drop(inputs);
        assert!(!root_path.exists(), "fixture temporary files are removed");
    }
}

// Only Windows runs `.cmd` shims, whose arguments Rust refuses when cmd would misparse them.
#[cfg(windows)]
#[test]
fn batch_argument_refusal_is_clear_for_runtime_agent_human_output_and_launch() {
    let project = Project::new();
    project.declaration(json!(["unsafe\nargument"]));
    let run = project.run(&["verify", "--wait", "--all"], 2);
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
    let output = Command::new(support::os::stand_in_shell())
        .args([
            "-c",
            "printf '<%s>\\n' \"$@\"",
            "fixture",
            "a b",
            "say \"hello\"",
            "a&b",
            "",
        ])
        .record_output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"<a b>\n<say \"hello\">\n<a&b>\n<>\n");
    // The program reaches the fixed script as a variable, never as interpolated source.
    let output = Command::new(support::os::stand_in_shell())
        .args([
            "-c",
            "\"$PROGRAM\" -c 'import sys; print(len(sys.argv)-1)' \"$@\"",
            "fixture",
        ])
        .env("PROGRAM", support::os::python_program())
        .record_output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "0");
}
