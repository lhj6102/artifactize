use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
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
        fs::write(path, value.to_string()).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .env("IDENTITY_SECRET", "must-not-leak");
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
    json!({"id":id,"title":"Check", "profile":{"kind":"runtime","command":"/bin/sh","args":["-c",script]},"payload":{"instruction":"Check input."}})
}

fn identity(script: &str) -> Value {
    json!({"kind":"identity","script":{"command":"/bin/sh","args":["-c",script]}})
}

#[test]
fn exact_output_is_validated_before_any_review_can_start() {
    let fixture = Fixture::new();
    for bytes in [
        b"".as_slice(),
        b" value",
        b"value ",
        b"value\t",
        b"value\r\n",
        b"value\n\n",
        b"one\ntwo",
        b"value/key",
        b"\xff",
        b"ok\0",
        b"\x1b[31mok\x1b[0m",
        &[b'x'; 129],
    ] {
        fs::write(fixture.repo.join("key"), bytes).unwrap();
        fixture.write("artifactize.json", json!({"name":"test","stale":identity("cat key"),"evals":[eval("check", "touch executed")]}));
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
    for bytes in [b"Aa0._:-".as_slice(), b"Aa0._:-\n", &[b'x'; 128]] {
        fs::write(fixture.repo.join("key"), bytes).unwrap();
        let run = fixture.verify(&["--all"], 0);
        assert_eq!(
            run["requests"][0]["identity"],
            String::from_utf8(bytes.to_vec())
                .unwrap()
                .trim_end_matches('\n')
        );
    }
}

#[test]
fn identity_process_failures_missing_inputs_and_links_never_fall_back() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("key"), "valid").unwrap();
    symlink("key", fixture.repo.join("link")).unwrap();
    symlink(&fixture.repo, fixture.repo.join("dir-link")).unwrap();
    for (stale, message) in [
        (
            identity("printf valid; printf private-diagnostic >&2; exit 7"),
            "exited with",
        ),
        (identity("kill -TERM $$"), "exited with"),
        (
            json!({"timeoutMs":50,"kind":"identity","script":{"command":"/bin/sleep","args":["30"]}}),
            "timed out",
        ),
        (
            json!({"kind":"identity","script":{"command":"missing-identity-executable","args":[]}}),
            "spawned",
        ),
        (
            json!({"kind":"identity","script":{"command":"/bin/true","args":[]},"inputs":["missing"]}),
            "missing",
        ),
        (
            json!({"kind":"identity","script":{"command":"/bin/true","args":[]},"inputs":["link"]}),
            "symlinks",
        ),
        (
            json!({"kind":"identity","script":{"command":"/bin/true","args":[]},"inputs":["dir-link/key"]}),
            "symlinks",
        ),
        (
            json!({"kind":"identity","script":{"command":"./link","args":[]}}),
            "symlinks",
        ),
        (
            json!({"kind":"identity","script":{"command":"../outside","args":[]}}),
            "relative",
        ),
    ] {
        fixture.write(
            "artifactize.json",
            json!({"name":"test","stale":stale,"evals":[eval("check","touch executed")]}),
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
    let script = r#"#!/usr/bin/python3
import json, os, pathlib, stat, sys
context = json.load(sys.stdin)
assert context == {'version': 1, 'artifactId': 'test'}
assert pathlib.Path.cwd().name == 'owner'
assert 'IDENTITY_SECRET' not in os.environ
assert sys.argv[2] == '$HOME; ../literal $(touch executed)'
assert sys.argv[3] == str(pathlib.Path.cwd() / 'key')
for key in ['HOME', 'TMPDIR', 'XDG_CACHE_HOME', 'ARTIFACTIZE_OUTPUT_DIR']:
    path = pathlib.Path(os.environ[key])
    assert not path.is_relative_to(pathlib.Path(os.environ['ARTIFACTIZE_WORKSPACE_DIR']))
    assert stat.S_IMODE(path.stat().st_mode) == 0o700
    (path / 'discard').write_text('scratch')
with open(sys.argv[1], 'a') as log:
    log.write(os.environ['ARTIFACTIZE_OUTPUT_DIR'] + '\n')
print('protocol:v1')
"#;
    fixture.write("owner/artifactize.json", json!({"name":"test","stale":{"kind":"identity","inputs":["key"],"script":{"command":"./identity.py","args":[probe,"$HOME; ../literal $(touch executed)","{test}/key"]}},"evals":[eval("one","exit 0"),eval("two","exit 3")]}));
    fs::write(fixture.repo.join("owner/key"), "material").unwrap();
    let path = fixture.repo.join("owner/identity.py");
    fs::write(&path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    let run = fixture.verify(&["--all", "--force"], 1);
    assert_eq!(run["requests"][0]["identity"], "protocol:v1");
    assert_eq!(run["requests"][1]["identity"], "protocol:v1");
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["requests"][1]["status"], "RED");
    let paths = fs::read_to_string(&probe).unwrap();
    assert_eq!(
        paths.lines().count(),
        3,
        "one preparation, one recheck per eval"
    );
    assert!(paths.lines().all(|path| !Path::new(path).exists()));
    assert_eq!(run["validation"]["artifacts"][0]["identity"], "script");
    assert_eq!(run["validation"]["artifacts"][0]["value"], "protocol:v1");
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
        ("printf 'bad value' > key", "IDENTITY_RECHECK_FAILED"),
        ("rm key", "IDENTITY_RECHECK_FAILED"),
    ] {
        fs::write(fixture.repo.join("key"), "before").unwrap();
        let mut stale = identity("cat key");
        stale["inputs"] = json!(["key"]);
        fixture.write(
            "artifactize.json",
            json!({"name":"test","stale":stale,"evals":[eval("check",review)]}),
        );
        let run = fixture.verify(&["--all"], 2);
        let request = &run["requests"][0];
        assert_eq!(run["status"], "ERROR");
        assert_eq!(request["status"], "ERROR");
        assert_eq!(request["errorCode"], code);
        assert_eq!(request["identity"], "before");
        assert!(
            request["result"].is_null(),
            "must not persist a semantic verdict"
        );
        assert_eq!(run["validation"]["satisfied"], false);
    }
    fixture.write(
        "artifactize.json",
        json!({"name":"test","evals":[eval("check","printf after > key")]}),
    );
    let run = fixture.verify(&["--all"], 0);
    assert!(run["requests"][0]["identity"].is_null());
    assert_eq!(run["requests"][0]["status"], "GREEN");
}

#[test]
fn preparation_covers_only_required_artifacts_but_includes_unselected_dependencies() {
    let fixture = Fixture::new();
    let mut selected = eval("check", "exit 0");
    selected["payload"]["instruction"] = json!("Check {dependency}.");
    fixture.write(
        "a/artifactize.json",
        json!({"name":"selected","evals":[selected]}),
    );
    fixture.write(
        "b/artifactize.json",
        json!({"name":"dependency","basis":true,"stale":identity("printf dependency:v1")}),
    );
    fixture.write("c/artifactize.json", json!({"name":"unrelated","stale":identity("exit 9"),"evals":[eval("check","touch executed")]}));
    let run = fixture.verify(&["--eval", "selected/check"], 0);
    assert_eq!(run["requests"].as_array().unwrap().len(), 1);
    assert!(run["requests"][0]["identity"].is_null());
    let artifacts = run["validation"]["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 2);
    assert_eq!(
        artifacts.iter().find(|a| a["id"] == "dependency").unwrap()["value"],
        "dependency:v1"
    );
    fixture.write(
        "b/artifactize.json",
        json!({"name":"dependency","basis":true,"stale":identity("exit 9")}),
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
fn family_context_is_per_instance_and_identity_material_is_rechecked() {
    let fixture = Fixture::new();
    fixture.write("artifactize.json", json!({"name":"root","basis":true}));
    fixture.write("family/artifactize.json", json!({
        "name":"family","family":{"instances":{"first":{"material":["first.txt"]},"second":{"material":["second.txt"]}}},
        "stale":{"kind":"identity","script":{"command":"python3","args":["identity.py"]}},
        "evals":[eval("check","exit 0")]
    }));
    fs::write(fixture.repo.join("family/identity.py"), "import json, sys\nx = json.load(sys.stdin)\nassert x == {'version': 1, 'artifactId': x['artifactId'], 'family': {'name': 'family', 'material': [x['artifactId'] + '.txt']}}\nprint(open(x['family']['material'][0]).read(), end='')\n").unwrap();
    fs::write(
        fixture.repo.join("family/python3"),
        "not executable and must not shadow PATH",
    )
    .unwrap();
    fs::write(fixture.repo.join("family/first.txt"), "first:v1").unwrap();
    fs::write(fixture.repo.join("family/second.txt"), "second:v2").unwrap();
    let run = fixture.verify(&["family"], 0);
    assert_eq!(run["requests"][0]["identity"], "first:v1");
    assert_eq!(run["requests"][1]["identity"], "second:v2");
    let mut declaration: Value =
        serde_json::from_slice(&fs::read(fixture.repo.join("family/artifactize.json")).unwrap())
            .unwrap();
    declaration["evals"] = json!([eval("check", "rm first.txt")]);
    fixture.write("family/artifactize.json", declaration);
    let run = fixture.verify(&["first"], 2);
    assert_eq!(run["requests"][0]["errorCode"], "IDENTITY_RECHECK_FAILED");
}

#[test]
fn cancellation_during_preparation_or_recheck_kills_the_command_and_removes_output() {
    for recheck in [false, true] {
        let fixture = Fixture::new();
        let marker = fixture.root.path().join("started");
        let source = format!(
            "if {}; then printf '%s\\n%s\\n' \"$$\" \"$ARTIFACTIZE_OUTPUT_DIR\" > '{}'; sleep 30 & wait; fi; printf stable",
            if recheck { "test -e executed" } else { "true" },
            marker.display()
        );
        fixture.write("artifactize.json", json!({"name":"test","stale":identity(&source),"evals":[eval("check","touch executed")]}));
        let child = fixture
            .command()
            .args(["verify", "--all", "--json"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::read_to_string(&marker).is_err() {
            assert!(Instant::now() < deadline, "identity never started");
            thread::sleep(Duration::from_millis(10));
        }
        let marker = fs::read_to_string(marker).unwrap();
        let mut lines = marker.lines();
        let pid: u32 = lines.next().unwrap().parse().unwrap();
        let output = lines.next().unwrap();
        assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
        let result = child.wait_with_output().unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(!Path::new(output).exists());
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        let result: Value = serde_json::from_slice(&result.stdout).unwrap();
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
