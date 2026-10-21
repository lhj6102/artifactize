#[path = "support/run.rs"]
mod owned_run;
use owned_run::{RunChild, Watcher};

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use support::os::{steps, symlink_file, true_program};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

struct Fixture {
    root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let state = root.path().join("state");
        root.track(&state, &repo);
        Self { repo, state, root }
    }

    fn write(&self, path: &str, value: Value) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        support::declaration::write(path, value.to_string()).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(support::os::cli());
        // Its own process group, so Ctrl-Break reaches it alone on Windows.
        support::os::new_group(&mut command)
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .envs(support::os::home_variables(self.root.path().join("home")))
            .env("FINGERPRINT_MARKER", "from-the-terminal");
        command
    }

    fn verify(&self, selection: &[&str], code: i32) -> Value {
        let output = self
            .command()
            .arg("verify")
            .arg("--wait")
            .args(selection)
            .arg("--json")
            .record_output()
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

    fn no_records(&self) {
        // Observe the public read-only cache seam, never open a writer beside Run cleanup.
        let output = self
            .command()
            .args(["cache", "list", "--history", "--json"])
            .record_output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let records: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
        assert!(records.is_empty(), "{records:?}");
    }

    fn no_execution(&self, run: &Value) {
        assert!(!self.repo.join("executed").exists());
        assert_eq!(run["executionsStarted"], 0);
        assert_eq!(run["status"], "ERROR");
        self.no_records();
    }
}

/// A runtime eval that runs the steps program with `run`.
fn eval(id: &str, run: &[&str]) -> Value {
    json!({
        "id":id,
        "title":"Check",
        "profile":{"kind":"runtime","command":steps(),"args":run},
        "payload":{"instruction":"Check input."},
    })
}

/// A fingerprint script that runs the steps program with `run`.
fn fingerprint(run: &[&str]) -> Value {
    json!({"script":{"command":steps(),"args":run}})
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
                "fingerprint":fingerprint(&["cat", "key"]),
                "evals":[eval("check", &["touch", "executed"])],
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
        fixture.no_execution(&error);
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
    // Only the declared deadline ends this command; host scheduling cannot race a natural exit.
    let (lingering, lingering_args) = support::os::lingering_command();
    for (declared, message) in [
        (
            fingerprint(&[
                "print",
                "valid",
                "eprint",
                "private-diagnostic",
                "exit",
                "7",
            ]),
            "exited with",
        ),
        (
            json!({"script":{"command":lingering,"args":lingering_args,"timeout_ms":50}}),
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
            json!({"name":"test","fingerprint":declared,"evals":[eval("check", &["touch", "executed"])]}),
        );
        let run = fixture.verify(&["--all"], 2);
        let error = run["error"].as_str().unwrap();
        assert!(error.contains(message), "{error}");
        assert!(!error.contains("private-diagnostic"));
        fixture.no_execution(&run);
    }
}

#[test]
fn protocol_uses_owner_cwd_literal_argv_and_disposable_private_directories_in_the_authors_environment()
 {
    let fixture = Fixture::new();
    let probe = fixture.root.path().join("probe");
    let script = r#"import json, os, pathlib, stat, sys
PATH_TEXT_FUNCTION
context = json.load(sys.stdin)
assert context == {'version': 1, 'artifactId': 'test'}
assert type(context['version']) is int
assert pathlib.Path.cwd().name == 'owner'
assert os.environ['FINGERPRINT_MARKER'] == 'from-the-terminal'
assert sys.argv[2] == '$FINGERPRINT_MARKER; ../literal $(touch executed)'
assert pathlib.Path(sys.argv[3]) == pathlib.Path.cwd() / 'key'
PRIVATE_DIRECTORY_CHECK
with open(sys.argv[1], 'a') as log:
    log.write(path_text(os.environ['ARTIFACTIZE_OUTPUT_DIR']) + '\n')
print('protocol:v1')
"#;
    let path = fixture.repo.join("owner/fingerprint.py");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let (command, mut args) = support::os::python_script(
        &path,
        &support::os::portable_path_probe(&support::os::fingerprint_probe(script)),
    );
    args.extend([
        json!(support::os::path_text(&probe)),
        json!("$FINGERPRINT_MARKER; ../literal $(touch executed)"),
        json!("{test}/key"),
    ]);
    fixture.write(
        "owner/index.artf",
        json!({
            "name":"test",
            "fingerprint":{"script":{"command":command,"args":args,"files":["key"]}},
            "evals":[eval("one", &["exit", "0"]),eval("two", &["exit", "3"])],
        }),
    );
    fs::write(fixture.repo.join("owner/key"), "material").unwrap();
    let run = fixture.verify(&["--all", "--force"], 1);
    assert_eq!(run["requests"][0]["fingerprint"], "protocol:v1");
    assert_eq!(run["requests"][1]["fingerprint"], "protocol:v1");
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["requests"][1]["status"], "RED");
    let paths = fs::read_to_string(&probe).unwrap();
    // Only the observable protocol matters, not how often preparation/rechecks invoke it.
    assert!(!paths.is_empty(), "the declared probe must run");
    assert!(
        paths.ends_with('\n'),
        "the probe saved a complete path record"
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
        .record_output()
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
        (&["write", "key", "after", "exit", "0"][..], "INPUT_CHANGED"),
        (&["write", "key", "after", "exit", "1"][..], "INPUT_CHANGED"),
        (
            &["write", "key", "bad value"][..],
            "FINGERPRINT_RECHECK_FAILED",
        ),
        (&["remove", "key"][..], "FINGERPRINT_RECHECK_FAILED"),
    ] {
        fs::write(fixture.repo.join("key"), "before").unwrap();
        let mut declared = fingerprint(&["cat", "key"]);
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
        fixture.no_records();
    }
    fixture.write(
        "index.artf",
        json!({"name":"test","fingerprint":false,"evals":[eval("check", &["write", "key", "after"])]}),
    );
    let run = fixture.verify(&["--all"], 0);
    assert!(run["requests"][0]["fingerprint"].is_null());
    assert_eq!(run["requests"][0]["status"], "GREEN");
}

#[test]
fn preparation_covers_only_required_artifacts_but_includes_unselected_dependencies() {
    let fixture = Fixture::new();
    let mut selected = eval("check", &["exit", "0"]);
    selected["payload"]["instruction"] = json!("Check {dependency}.");
    fixture.write(
        "a/index.artf",
        json!({"name":"selected","fingerprint":false,"evals":[selected]}),
    );
    fixture.write(
        "b/index.artf",
        json!({"name":"dependency","basis":true,"fingerprint":fingerprint(&["print", "dependency:v1"])}),
    );
    fixture.write(
        "c/index.artf",
        json!({
            "name":"unrelated",
            "fingerprint":fingerprint(&["exit", "9"]),
            "evals":[eval("check", &["touch", "executed"])],
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
        json!({"name":"dependency","basis":true,"fingerprint":fingerprint(&["exit", "9"])}),
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
        let marker = fixture.root.path().join("started 'marker");
        // Fixture language stays static; even quoted or spaced paths are literal argv.
        let marker_text = support::os::path_text(&marker);
        let held = support::os::path_text(&fixture.root.path().join("held"));
        // Preparation prints stable at once when it only rechecks; otherwise, and on the
        // recheck after the eval ran, it saves its identity and stays up with a child.
        let mut source = if recheck {
            vec!["needs", "executed", "stable"]
        } else {
            vec![]
        };
        source.extend([
            "pid",
            &marker_text,
            "env",
            &marker_text,
            "ARTIFACTIZE_OUTPUT_DIR",
            "hold",
            &held,
            "print",
            "stable",
        ]);
        fixture.write(
            "index.artf",
            json!({
                "name":"test",
                "fingerprint":{"script":{
                    "command":steps(),
                    "args":source,
                }},
                "evals":[eval("check", &["touch", "executed"])],
            }),
        );
        let child = fixture
            .command()
            .args(["verify", "--wait", "--all", "--json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let child = RunChild::new(child, &fixture.state, &fixture.repo);
        let deadline = Instant::now() + support::os::patience(Duration::from_secs(10));
        // Creation and even a partial second line are not a saved PID/path record.
        let marker = loop {
            if let Ok(text) = fs::read_to_string(&marker)
                && text.ends_with('\n')
                && text.lines().count() == 2
                && text.lines().next().unwrap().parse::<u32>().is_ok()
                && !text.lines().nth(1).unwrap().is_empty()
            {
                break text;
            }
            assert!(
                Instant::now() < deadline,
                "fingerprint never saved its identity"
            );
            thread::sleep(Duration::from_millis(10));
        };
        let mut lines = marker.lines();
        let pid: u32 = lines.next().unwrap().parse().unwrap();
        let output = lines.next().unwrap();
        support::runs::cancel_active(&fixture.state, &fixture.repo);
        let result = child.wait_with_output().unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(!Path::new(output).exists());
        assert!(support::os::gone(pid));
        let result: Value = serde_json::from_slice(&result.stdout).unwrap();
        fixture.no_records();
        if recheck {
            assert_eq!(result["requests"][0]["status"], "ERROR");
            assert_eq!(result["requests"][0]["errorCode"], "CANCELLED");
            assert!(result["requests"][0]["result"].is_null());
        } else {
            assert!(result["error"].as_str().unwrap().contains("cancelled"));
            fixture.no_execution(&result);
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
    let api = |reference: &str| {
        json!({
            "name":"api",
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
    fixture.write("api/index.artf", api("{core}"));
    fs::write(
        fixture.repo.join("api/test_api.py"),
        "import pathlib, sys\nassert (pathlib.Path(sys.argv[1]) / 'lib.txt').is_file()\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        let output = fixture.command().args(args).record_output().unwrap();
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

    // Unknown and escaping references fail closed in config check and status alike.
    for (reference, message) in [
        ("{missing}", "Unknown Artifact reference {missing}"),
        ("{core}/../api", "safe relative logical path"),
    ] {
        fixture.write("api/index.artf", api(reference));
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

/// Own a status caller and its gated fingerprint scripts, including assertion unwinding.
/// Release the gate and collect the caller before fixture directories can disappear.
struct FingerprintCaller {
    child: Option<Watcher>,
    release: PathBuf,
}

impl FingerprintCaller {
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.as_mut().unwrap().try_wait()
    }

    fn wait_with_output(mut self) -> std::io::Result<std::process::Output> {
        self.child.take().unwrap().wait_with_output()
    }
}

impl Drop for FingerprintCaller {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            // This is fixture cleanup, not the observation that establishes parallelism.
            let _ = fs::write(&self.release, "");
            let _ = child.wait_with_output();
        }
    }
}

/// Eight Artifacts whose fingerprint scripts note how many of them run at once.
fn concurrent_scripts(fixture: &Fixture) -> PathBuf {
    let markers = fixture.root.path().join("markers");
    fs::create_dir(&markers).unwrap();
    // Each script marks itself running, counts the running markers, and stays up until released.
    let directory = support::os::path_text(&markers);
    for index in 0..8 {
        let name = format!("part{index}");
        let running = format!("{directory}/running.{name}");
        let counts = format!("{directory}/counts.{name}");
        let release = format!("{directory}/release");
        let output = format!("{name}-v1\n");
        let script = [
            "touch", &running, "count", &directory, "running.", &counts, "wait", &release,
            "remove", &running, "print", &output,
        ];
        fixture.write(
            &format!("{name}/index.artf"),
            json!({
                "name":name,
                "fingerprint":{
                    "script":{"command":steps(),"args":script},
                },
                "evals":[eval("check", &["exit", "0"])],
            }),
        );
    }
    markers
}

/// Observe the most fingerprint scripts that ran at once, preserving their notes.
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
    }
    most
}

/// Reset only completed count evidence between phases; the release gate has its own lifetime.
fn reset_counts(markers: &Path) {
    for entry in fs::read_dir(markers).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("counts.")
        {
            fs::remove_file(path).unwrap();
        }
    }
}

#[test]
fn fingerprints_run_in_bounded_parallel_with_the_same_output_at_any_bound() {
    let fixture = Fixture::new();
    let markers = concurrent_scripts(&fixture);
    let status = |jobs: &str| {
        let child = fixture
            .command()
            .args(["status", "--json", "--fingerprint-jobs", jobs])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut child = FingerprintCaller {
            child: Some(Watcher::new(child)),
            release: markers.join("release"),
        };
        let bound: usize = jobs.parse().unwrap();
        let deadline = Instant::now() + support::os::patience(Duration::from_secs(20));
        // A running marker appears before its count is written. Wait for the complete
        // observable count record, so releasing scripts cannot race that observation.
        while !fs::read_dir(&markers).unwrap().any(|entry| {
            let path = entry.unwrap().path();
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("counts.")
                && fs::read_to_string(path).is_ok_and(|text| {
                    text.ends_with('\n')
                        && text.lines().any(|line| line.parse::<usize>() == Ok(bound))
                })
        }) {
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
    reset_counts(&markers);
    fs::remove_file(markers.join("release")).unwrap();
    let (three, most) = status("3");
    assert_eq!(most, 3);
    assert_eq!(three, one);
    reset_counts(&markers);

    // verify bounds preparation and the end-of-review rechecks alike, and records the bound.
    let run = fixture.verify(&["--all", "--fingerprint-jobs", "3"], 0);
    assert_eq!(run["fingerprintJobs"], 3);
    assert!(most_at_once(&markers) <= 3);
    reset_counts(&markers);
    let default = fixture.verify(&["--all"], 0);
    assert!(default["fingerprintJobs"].as_u64().unwrap() >= 1);
    assert_eq!(default["executionsStarted"], 0);
    let output = fixture
        .command()
        .args(["status", "--fingerprint-jobs", "0"])
        .record_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn the_first_failing_fingerprint_in_order_is_reported_and_later_ones_are_cancelled() {
    let fixture = Fixture::new();
    let started = fixture.root.path().join("started 'markers");
    fs::create_dir(&started).unwrap();
    for (name, script) in [
        // Fails last, but comes first.
        (
            "alpha",
            &[
                "wait",
                "../gamma/finished",
                "eprint",
                "first failure\n",
                "exit",
                "3",
            ][..],
        ),
        // Would run long; it comes after alpha's failure, so it is cancelled.
        (
            "beta",
            &[
                "pid", "pid", "touch", "ready", "wait", "release", "print", "beta\n",
            ][..],
        ),
        (
            "gamma",
            &["wait", "../beta/ready", "touch", "finished", "exit", "4"][..],
        ),
    ] {
        let start = support::os::path_text(&started.join(name));
        let mut run = vec!["touch", &start];
        run.extend(script);
        fixture.write(
            &format!("{name}/index.artf"),
            json!({
                "name":name,
                "fingerprint":{"script":{
                    "command":steps(),
                    "args":run,
                }},
                "evals":[eval("check", &["touch", "executed"])],
            }),
        );
    }
    let output = fixture
        .command()
        .args(["status", "--fingerprint-jobs", "4"])
        .record_output()
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
    // Gamma releases alpha only after beta has published its PID/start barrier.
    for name in ["alpha", "beta", "gamma"] {
        assert!(started.join(name).exists());
    }
    // Repeat the same handshake for verify instead of inheriting status's markers.
    fs::remove_file(fixture.repo.join("beta/ready")).unwrap();
    fs::remove_file(fixture.repo.join("gamma/finished")).unwrap();
    let run = fixture
        .command()
        .args(["verify", "--wait", "--all"])
        .record_output()
        .unwrap();
    assert_eq!(run.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&run.stderr).contains("Artifact alpha"));
    assert!(!fixture.repo.join("executed").exists());
}
