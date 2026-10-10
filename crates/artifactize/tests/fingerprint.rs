use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::{Value, json};
use support::os::{shell, sleep_program, symlink_file, true_program};
use tempfile::TempDir;

mod support;

struct Fixture {
    root: TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        Self {
            repo,
            state: root.path().join("state"),
            root,
        }
    }

    fn write(&self, path: &str, value: Value) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        support::declaration::write(path, value.to_string()).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        // Its own process group, so Ctrl-Break reaches it alone on Windows.
        support::os::new_group(&mut command)
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .env("FINGERPRINT_SECRET", "must-not-leak");
        command
    }

    fn verify(&self, selection: &[&str], code: i32) -> Value {
        let output = self
            .command()
            .arg("verify")
            .args(selection)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn no_execution(&self) {
        assert!(!self.repo.join("executed").exists());
        let db = Connection::open(self.state.join("state.sqlite")).unwrap();
        for table in ["runs", "requests"] {
            let count: u32 = db
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0);
        }
        assert_eq!(fs::read_dir(self.state.join("runs")).unwrap().count(), 0);
    }
}

fn eval(id: &str, script: &str) -> Value {
    json!({
        "id":id,
        "title":"Check",
        "profile":{"kind":"runtime","command":shell(),"args":["-c",script]},
        "payload":{"instruction":"Check input."},
    })
}

fn fingerprint(script: &str) -> Value {
    json!({"script":{"command":shell(),"args":["-c",script]}})
}

#[test]
fn exact_output_is_validated_before_any_review_can_start() {
    let fixture = Fixture::new();
    for bytes in support::os::invalid_fingerprints() {
        fs::write(fixture.repo.join("key"), &bytes).unwrap();
        fixture.write(
            "index.artf",
            json!({
                "name":"test",
                "fingerprint":fingerprint("cat key"),
                "evals":[eval("check", "touch executed")],
            }),
        );
        let error = fixture.verify(&["--all"], 2);
        assert!(
            error["error"].as_str().unwrap().contains("Artifact test"),
            "{error}"
        );
        assert!(
            error["error"].as_str().unwrap().contains("1–128"),
            "{error}"
        );
        fixture.no_execution();
    }
    for bytes in support::os::valid_fingerprints() {
        fs::write(fixture.repo.join("key"), &bytes).unwrap();
        let run = fixture.verify(&["--all"], 0);
        assert_eq!(
            run["requests"][0]["fingerprint"],
            String::from_utf8(bytes.to_vec())
                .unwrap()
                .trim_end_matches('\n')
                .trim_end_matches('\r')
        );
    }
}

#[test]
fn fingerprint_process_failures_missing_files_and_links_never_fall_back() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("key"), "valid").unwrap();
    let mut links = Vec::new();
    if symlink_file("key", fixture.repo.join("link")).is_some() {
        links.extend([
            (
                json!({"script":{"command":true_program(),"args":[],"files":["link"]}}),
                "symlinks",
            ),
            (json!({"script":{"command":"./link","args":[]}}), "symlinks"),
        ]);
    }
    support::os::link_dir(&fixture.repo, &fixture.repo.join("dir-link"));
    for (declared, message) in [
        (
            fingerprint("printf valid; printf private-diagnostic >&2; exit 7"),
            "exited with",
        ),
        (
            json!({"script":{"command":sleep_program(),"args":["30"],"timeout_ms":50}}),
            "timed out",
        ),
        (
            json!({"script":{"command":"missing-fingerprint-executable","args":[]}}),
            "spawned",
        ),
        (
            json!({"script":{"command":true_program(),"args":[],"files":["missing"]}}),
            "missing",
        ),
        (
            json!({"script":{"command":true_program(),"args":[],"files":["dir-link/key"]}}),
            "symlinks",
        ),
        (
            json!({"script":{"command":"../outside","args":[]}}),
            "relative",
        ),
    ]
    .into_iter()
    .chain(support::os::signal_fingerprint().map(|value| (value, "exited with")))
    .chain(links)
    {
        fixture.write(
            "index.artf",
            json!({"name":"test","fingerprint":declared,"evals":[eval("check", "touch executed")]}),
        );
        let error = fixture.verify(&["--all"], 2);
        let error = error["error"].as_str().unwrap();
        assert!(error.contains(message), "{error}");
        assert!(!error.contains("private-diagnostic"));
        fixture.no_execution();
    }
}

#[test]
fn protocol_uses_owner_cwd_literal_argv_and_disposable_private_environment() {
    let fixture = Fixture::new();
    let probe = fixture.root.path().join("probe");
    let script = r#"import json, os, pathlib, stat, sys
context = json.load(sys.stdin)
assert context == {'version': 1, 'artifactId': 'test'}
assert type(context['version']) is int
assert pathlib.Path.cwd().name == 'owner'
assert 'FINGERPRINT_SECRET' not in os.environ
assert sys.argv[2] == '$HOME; ../literal $(touch executed)'
assert pathlib.Path(sys.argv[3]) == pathlib.Path.cwd() / 'key'
PRIVATE_DIRECTORY_CHECK
with open(sys.argv[1], 'a') as log:
    log.write(os.environ['ARTIFACTIZE_OUTPUT_DIR'] + '\n')
print('protocol:v1')
"#;
    let path = fixture.repo.join("owner/fingerprint.py");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let (command, mut args) =
        support::os::python_script(&path, &support::os::fingerprint_probe(script));
    args.extend([
        json!(probe),
        json!("$HOME; ../literal $(touch executed)"),
        json!("{test}/key"),
    ]);
    fixture.write(
        "owner/index.artf",
        json!({
            "name":"test",
            "fingerprint":{"script":{"command":command,"args":args,"files":["key"]}},
            "evals":[eval("one", "exit 0"),eval("two", "exit 3")],
        }),
    );
    fs::write(fixture.repo.join("owner/key"), "material").unwrap();
    let run = fixture.verify(&["--all", "--force"], 1);
    assert_eq!(run["requests"][0]["fingerprint"], "protocol:v1");
    assert_eq!(run["requests"][1]["fingerprint"], "protocol:v1");
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["requests"][1]["status"], "RED");
    let paths = fs::read_to_string(&probe).unwrap();
    assert_eq!(
        paths.lines().count(),
        3,
        "one preparation, one recheck per eval"
    );
    assert!(paths.lines().all(|path| !Path::new(path).exists()));
    assert_eq!(
        run["validation"]["artifacts"][0]["fingerprintKind"],
        "script"
    );
    assert_eq!(
        run["validation"]["artifacts"][0]["fingerprint"],
        "protocol:v1"
    );
    fs::remove_dir_all(&fixture.repo).unwrap();
    let output = fixture
        .command()
        .args(["run", "show", run["id"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        run
    );
    assert_eq!(fs::read_to_string(probe).unwrap(), paths);
}

#[test]
fn changed_input_or_failed_recheck_cannot_become_a_semantic_verdict() {
    let fixture = Fixture::new();
    for (review, code) in [
        ("printf after > key; exit 0", "INPUT_CHANGED"),
        ("printf after > key; exit 1", "INPUT_CHANGED"),
        ("printf 'bad value' > key", "FINGERPRINT_RECHECK_FAILED"),
        ("rm key", "FINGERPRINT_RECHECK_FAILED"),
    ] {
        fs::write(fixture.repo.join("key"), "before").unwrap();
        let mut declared = fingerprint("cat key");
        declared["script"]["files"] = json!(["key"]);
        fixture.write(
            "index.artf",
            json!({"name":"test","fingerprint":declared,"evals":[eval("check", review)]}),
        );
        let run = fixture.verify(&["--all"], 2);
        let request = &run["requests"][0];
        assert_eq!(run["status"], "ERROR");
        assert_eq!(request["status"], "ERROR");
        assert_eq!(request["errorCode"], code);
        assert_eq!(request["fingerprint"], "before");
        assert!(
            request["result"].is_null(),
            "must not persist a semantic verdict"
        );
        assert_eq!(run["validation"]["satisfied"], false);
        let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
        assert_eq!(
            db.query_row::<u32, _, _>(
                "SELECT count(*) FROM executions WHERE completed_at IS NOT NULL",
                [],
                |row| row.get(0)
            )
            .unwrap(),
            0
        );
    }
    fixture.write(
        "index.artf",
        json!({"name":"test","fingerprint":false,"evals":[eval("check", "printf after > key")]}),
    );
    let run = fixture.verify(&["--all"], 0);
    assert!(run["requests"][0]["fingerprint"].is_null());
    assert_eq!(run["requests"][0]["status"], "GREEN");
}

#[test]
fn preparation_covers_only_required_artifacts_but_includes_unselected_dependencies() {
    let fixture = Fixture::new();
    let mut selected = eval("check", "exit 0");
    selected["payload"]["instruction"] = json!("Check {dependency}.");
    fixture.write(
        "a/index.artf",
        json!({"name":"selected","fingerprint":false,"evals":[selected]}),
    );
    fixture.write(
        "b/index.artf",
        json!({"name":"dependency","basis":true,"fingerprint":fingerprint("printf dependency:v1")}),
    );
    fixture.write(
        "c/index.artf",
        json!({
            "name":"unrelated",
            "fingerprint":fingerprint("exit 9"),
            "evals":[eval("check", "touch executed")],
        }),
    );
    let run = fixture.verify(&["--eval", "selected/check"], 0);
    assert_eq!(run["requests"].as_array().unwrap().len(), 1);
    assert!(run["requests"][0]["fingerprint"].is_null());
    let artifacts = run["validation"]["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 2);
    assert_eq!(
        artifacts.iter().find(|a| a["id"] == "dependency").unwrap()["fingerprint"],
        "dependency:v1"
    );
    fixture.write(
        "b/index.artf",
        json!({"name":"dependency","basis":true,"fingerprint":fingerprint("exit 9")}),
    );
    let error = fixture.verify(&["--eval", "selected/check"], 2);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("Artifact dependency")
    );
}

#[test]
fn cancellation_during_preparation_or_recheck_kills_the_command_and_removes_output() {
    for recheck in [false, true] {
        let fixture = Fixture::new();
        let marker = fixture.root.path().join("started");
        let source = format!(
            "if {}; then printf '%s\\n%s\\n' \"$$\" \"$ARTIFACTIZE_OUTPUT_DIR\" > '{}'; {} & wait; fi; printf stable",
            if recheck { "test -e executed" } else { "true" },
            marker.display(),
            support::os::LINGERING
        );
        fixture.write(
            "index.artf",
            json!({
                "name":"test",
                "fingerprint":fingerprint(&source),
                "evals":[eval("check", "touch executed")],
            }),
        );
        let child = fixture
            .command()
            .args(["verify", "--all", "--json"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + support::os::patience(Duration::from_secs(10));
        // The redirect creates the marker before printf writes its two lines.
        while fs::read_to_string(&marker).map_or(true, |text| text.lines().count() < 2) {
            assert!(Instant::now() < deadline, "fingerprint never started");
            thread::sleep(Duration::from_millis(10));
        }
        let marker = fs::read_to_string(marker).unwrap();
        let mut lines = marker.lines();
        let pid: u32 = lines.next().unwrap().parse().unwrap();
        let output = lines.next().unwrap();
        support::os::terminate(child.id());
        let result = child.wait_with_output().unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(!Path::new(output).exists());
        assert!(support::os::gone(pid));
        let result: Value = serde_json::from_slice(&result.stdout).unwrap();
        let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
        assert_eq!(
            db.query_row::<u32, _, _>(
                "SELECT count(*) FROM executions WHERE completed_at IS NOT NULL",
                [],
                |row| row.get(0)
            )
            .unwrap(),
            0
        );
        if recheck {
            assert_eq!(result["requests"][0]["status"], "ERROR");
            assert_eq!(result["requests"][0]["errorCode"], "CANCELLED");
            assert!(result["requests"][0]["result"].is_null());
        } else {
            assert!(result["error"].as_str().unwrap().contains("cancelled"));
            fixture.no_execution();
        }
    }
}

#[test]
fn fingerprint_arguments_resolve_global_names_like_runtime_argv() {
    let fixture = Fixture::new();
    fs::write(
        fixture.repo.join("fingerprint.py"),
        "import hashlib, json, pathlib, sys\njson.load(sys.stdin)\ndigest = hashlib.sha256()\nfor root in sys.argv[1:]:\n    for path in sorted(pathlib.Path(root).rglob('*')):\n        if path.is_file():\n            digest.update(path.read_bytes())\nprint(digest.hexdigest())\n",
    )
    .unwrap();
    // Every Artifact an eval names needs a fingerprint for the eval to have a reuse key.
    fixture.write(
        "core/index.artf",
        json!({"name":"core","basis":true,"fingerprint":{}}),
    );
    fs::write(fixture.repo.join("core/lib.txt"), "v1").unwrap();
    // The JSON from issue #48: a global Artifact name in both fingerprint and runtime argv.
    let api = |reference: &str, mounts: Value| {
        json!({
            "name":"api",
            "mounts":mounts,
            "fingerprint":{
                "script":{"command":support::os::python_program(),"args":["../fingerprint.py",".",reference]},
            },
            "evals":[
                {
                    "id":"tests",
                    "title":"Tests",
                    "profile":{
                        "kind":"runtime",
                        "command":support::os::python_program(),
                        "args":["-B","test_api.py",reference],
                    },
                    "payload":{"instruction":"Run the API tests."},
                },
            ],
        })
    };
    fixture.write("api/index.artf", api("{core}", json!({})));
    fs::write(
        fixture.repo.join("api/test_api.py"),
        "import pathlib, sys\nassert (pathlib.Path(sys.argv[1]) / 'lib.txt').is_file()\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        let output = fixture.command().args(args).output().unwrap();
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    assert_eq!(run(&["config", "check"]).0, Some(0));
    let (code, status, _) = run(&["status", "--json"]);
    assert_eq!(code, Some(1), "{status}");
    let status: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["evals"][0]["action"], "execute");
    let first = fixture.verify(&["--all"], 0);
    let fingerprint = first["requests"][0]["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(run(&["status"]).0, Some(0));
    fs::write(fixture.repo.join("core/lib.txt"), "v2").unwrap();
    let second = fixture.verify(&["--all"], 0);
    assert_ne!(second["requests"][0]["fingerprint"], fingerprint.as_str());
    assert_eq!(second["executionsStarted"], 1);

    // Mount aliases keep working.
    fixture.write("api/index.artf", api("{lib}", json!({"lib":"core"})));
    assert_eq!(run(&["config", "check"]).0, Some(0));
    fixture.verify(&["--all"], 0);

    // Unknown and escaping references fail closed in config check and status alike.
    for (reference, message) in [
        ("{missing}", "Unknown Artifact reference {missing}"),
        ("{core}/../api", "safe relative logical path"),
    ] {
        fixture.write("api/index.artf", api(reference, json!({})));
        for command in [&["config", "check"][..], &["status"]] {
            let (code, _, stderr) = run(command);
            assert_eq!(code, Some(2), "{command:?} {reference}: {stderr}");
            assert!(
                stderr.contains(message),
                "{command:?} {reference}: {stderr}"
            );
        }
    }
}

/// Eight Artifacts whose fingerprint scripts note how many of them run at once.
fn concurrent_scripts(fixture: &Fixture) -> PathBuf {
    let markers = fixture.root.path().join("markers");
    fs::create_dir(&markers).unwrap();
    // Each script marks itself running, counts the running markers, and stays up for a moment.
    let script = r#"touch "$2/running.$1"; ls "$2" | grep -c '^running\.' >> "$2/counts.$1"; while [ ! -e "$2/release" ]; do sleep 0.01; done; rm "$2/running.$1"; echo "$1-v1""#;
    for index in 0..8 {
        let name = format!("part{index}");
        fixture.write(
            &format!("{name}/index.artf"),
            json!({
                "name":name,
                "fingerprint":{
                    "script":{"command":shell(),"args":["-c",script,"sh",name,markers]},
                },
                "evals":[eval("check", "exit 0")],
            }),
        );
    }
    markers
}

/// The most fingerprint scripts that ran at once, then reset the notes.
fn most_at_once(markers: &Path) -> usize {
    let mut most = 0;
    for entry in fs::read_dir(markers).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() == "release" {
            continue;
        }
        let counts = fs::read_to_string(&path).unwrap();
        most = counts
            .lines()
            .map(|count| count.trim().parse::<usize>().unwrap())
            .chain([most])
            .max()
            .unwrap();
        fs::remove_file(path).unwrap();
    }
    most
}

#[test]
fn fingerprints_run_in_bounded_parallel_with_the_same_output_at_any_bound() {
    let fixture = Fixture::new();
    let markers = concurrent_scripts(&fixture);
    let status = |jobs: &str| {
        let mut child = fixture
            .command()
            .args(["status", "--json", "--fingerprint-jobs", jobs])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let bound: usize = jobs.parse().unwrap();
        let deadline = Instant::now() + support::os::patience(Duration::from_secs(20));
        while fs::read_dir(&markers)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_str()
                    .unwrap()
                    .starts_with("running.")
            })
            .count()
            < bound
        {
            if child.try_wait().unwrap().is_some() {
                let output = child.wait_with_output().unwrap();
                panic!(
                    "status ended before its fingerprint workers met: {}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            assert!(
                Instant::now() < deadline,
                "fingerprint workers did not reach the barrier"
            );
            thread::yield_now();
        }
        fs::write(markers.join("release"), "").unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        let status: Value = serde_json::from_slice(&output.stdout).unwrap();
        (status, most_at_once(&markers))
    };
    let (one, most) = status("1");
    assert_eq!(most, 1);
    fs::remove_file(markers.join("release")).unwrap();
    let (three, most) = status("3");
    assert!((2..=3).contains(&most), "{most}");
    assert_eq!(three, one);

    // verify bounds preparation and the end-of-review rechecks alike, and records the bound.
    let run = fixture.verify(&["--all", "--fingerprint-jobs", "3"], 0);
    assert_eq!(run["fingerprintJobs"], 3);
    assert!(most_at_once(&markers) <= 3);
    let default = fixture.verify(&["--all"], 0);
    assert!(default["fingerprintJobs"].as_u64().unwrap() >= 1);
    assert_eq!(default["executionsStarted"], 0);
    let output = fixture
        .command()
        .args(["status", "--fingerprint-jobs", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn the_first_failing_fingerprint_in_order_is_reported_and_later_ones_are_cancelled() {
    let fixture = Fixture::new();
    let started = fixture.root.path().join("started");
    for (name, script) in [
        // Fails last, but comes first.
        (
            "alpha",
            "while [ ! -e ../gamma/finished ]; do sleep 0.01; done; echo first failure >&2; exit 3",
        ),
        // Would run long; it comes after alpha's failure, so it is cancelled.
        (
            "beta",
            "echo $$ > pid; while [ ! -e release ]; do sleep 0.01; done; echo beta",
        ),
        ("gamma", "touch finished; exit 4"),
    ] {
        fixture.write(
            &format!("{name}/index.artf"),
            json!({
                "name":name,
                "fingerprint":fingerprint(&format!(
                    "touch '{}.{name}'; {script}",
                    started.display()
                )),
                "evals":[eval("check", "touch executed")],
            }),
        );
    }
    let output = fixture
        .command()
        .args(["status", "--fingerprint-jobs", "4"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Fingerprint script for Artifact alpha failed"),
        "{stderr}"
    );
    let pid: u32 = fs::read_to_string(fixture.repo.join("beta/pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        support::os::gone(pid),
        "later fingerprint was not cancelled"
    );
    // All three started at once.
    for name in ["alpha", "beta", "gamma"] {
        assert!(Path::new(&format!("{}.{name}", started.display())).exists());
    }
    let run = fixture
        .command()
        .args(["verify", "--all"])
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&run.stderr).contains("Artifact alpha"));
    assert!(!fixture.repo.join("executed").exists());
}
