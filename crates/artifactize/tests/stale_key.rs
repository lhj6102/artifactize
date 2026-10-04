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
            .env("STALE_KEY_SECRET", "must-not-leak");
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

fn stale_key(script: &str) -> Value {
    json!({"script":{"command":"/bin/sh","args":["-c",script]}})
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
        fixture.write("artifactize.json", json!({"name":"test","staleKey":stale_key("cat key"),"evals":[eval("check", "touch executed")]}));
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
            run["requests"][0]["staleKey"],
            String::from_utf8(bytes.to_vec())
                .unwrap()
                .trim_end_matches('\n')
        );
    }
}

#[test]
fn stale_key_process_failures_missing_inputs_and_links_never_fall_back() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("key"), "valid").unwrap();
    symlink("key", fixture.repo.join("link")).unwrap();
    symlink(&fixture.repo, fixture.repo.join("dir-link")).unwrap();
    for (stale, message) in [
        (
            stale_key("printf valid; printf private-diagnostic >&2; exit 7"),
            "exited with",
        ),
        (stale_key("kill -TERM $$"), "exited with"),
        (
            json!({"script":{"command":"/bin/sleep","args":["30"],"timeoutMs":50}}),
            "timed out",
        ),
        (
            json!({"script":{"command":"missing-stale_key-executable","args":[]}}),
            "spawned",
        ),
        (
            json!({"script":{"command":"/bin/true","args":[],"inputs":["missing"]}}),
            "missing",
        ),
        (
            json!({"script":{"command":"/bin/true","args":[],"inputs":["link"]}}),
            "symlinks",
        ),
        (
            json!({"script":{"command":"/bin/true","args":[],"inputs":["dir-link/key"]}}),
            "symlinks",
        ),
        (json!({"script":{"command":"./link","args":[]}}), "symlinks"),
        (
            json!({"script":{"command":"../outside","args":[]}}),
            "relative",
        ),
    ] {
        fixture.write(
            "artifactize.json",
            json!({"name":"test","staleKey":stale,"evals":[eval("check","touch executed")]}),
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
assert 'STALE_KEY_SECRET' not in os.environ
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
    fixture.write("owner/artifactize.json", json!({"name":"test","staleKey":{"script":{"command":"./stale_key.py","args":[probe,"$HOME; ../literal $(touch executed)","{test}/key"],"inputs":["key"]}},"evals":[eval("one","exit 0"),eval("two","exit 3")]}));
    fs::write(fixture.repo.join("owner/key"), "material").unwrap();
    let path = fixture.repo.join("owner/stale_key.py");
    fs::write(&path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    let run = fixture.verify(&["--all", "--force"], 1);
    assert_eq!(run["requests"][0]["staleKey"], "protocol:v1");
    assert_eq!(run["requests"][1]["staleKey"], "protocol:v1");
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["requests"][1]["status"], "RED");
    let paths = fs::read_to_string(&probe).unwrap();
    assert_eq!(
        paths.lines().count(),
        3,
        "one preparation, one recheck per eval"
    );
    assert!(paths.lines().all(|path| !Path::new(path).exists()));
    assert_eq!(run["validation"]["artifacts"][0]["staleKeyKind"], "script");
    assert_eq!(run["validation"]["artifacts"][0]["staleKey"], "protocol:v1");
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
        ("printf 'bad value' > key", "STALE_KEY_RECHECK_FAILED"),
        ("rm key", "STALE_KEY_RECHECK_FAILED"),
    ] {
        fs::write(fixture.repo.join("key"), "before").unwrap();
        let mut stale = stale_key("cat key");
        stale["script"]["inputs"] = json!(["key"]);
        fixture.write(
            "artifactize.json",
            json!({"name":"test","staleKey":stale,"evals":[eval("check",review)]}),
        );
        let run = fixture.verify(&["--all"], 2);
        let request = &run["requests"][0];
        assert_eq!(run["status"], "ERROR");
        assert_eq!(request["status"], "ERROR");
        assert_eq!(request["errorCode"], code);
        assert_eq!(request["staleKey"], "before");
        assert!(
            request["result"].is_null(),
            "must not persist a semantic verdict"
        );
        assert_eq!(run["validation"]["satisfied"], false);
        let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
        assert_eq!(
            db.query_row::<u32, _, _>("SELECT count(*) FROM cache_entries", [], |row| row.get(0))
                .unwrap(),
            0
        );
    }
    fixture.write(
        "artifactize.json",
        json!({"name":"test","evals":[eval("check","printf after > key")]}),
    );
    let run = fixture.verify(&["--all"], 0);
    assert!(run["requests"][0]["staleKey"].is_null());
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
        json!({"name":"dependency","basis":true,"staleKey":stale_key("printf dependency:v1")}),
    );
    fixture.write("c/artifactize.json", json!({"name":"unrelated","staleKey":stale_key("exit 9"),"evals":[eval("check","touch executed")]}));
    let run = fixture.verify(&["--eval", "selected/check"], 0);
    assert_eq!(run["requests"].as_array().unwrap().len(), 1);
    assert!(run["requests"][0]["staleKey"].is_null());
    let artifacts = run["validation"]["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 2);
    assert_eq!(
        artifacts.iter().find(|a| a["id"] == "dependency").unwrap()["staleKey"],
        "dependency:v1"
    );
    fixture.write(
        "b/artifactize.json",
        json!({"name":"dependency","basis":true,"staleKey":stale_key("exit 9")}),
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
fn family_context_is_per_instance_and_stale_key_material_is_rechecked() {
    let fixture = Fixture::new();
    fixture.write("artifactize.json", json!({"name":"root","basis":true}));
    fixture.write("family/artifactize.json", json!({
        "name":"family","family":{"instances":{"first":{"material":["first.txt"]},"second":{"material":["second.txt"]}}},
        "staleKey":{"script":{"command":"python3","args":["stale_key.py"]}},
        "evals":[eval("check","exit 0")]
    }));
    fs::write(fixture.repo.join("family/stale_key.py"), "import json, sys\nx = json.load(sys.stdin)\nassert x == {'version': 1, 'artifactId': x['artifactId'], 'family': {'name': 'family', 'material': [x['artifactId'] + '.txt']}}\nprint(open(x['family']['material'][0]).read(), end='')\n").unwrap();
    fs::write(
        fixture.repo.join("family/python3"),
        "not executable and must not shadow PATH",
    )
    .unwrap();
    fs::write(fixture.repo.join("family/first.txt"), "first:v1").unwrap();
    fs::write(fixture.repo.join("family/second.txt"), "second:v2").unwrap();
    let run = fixture.verify(&["family"], 0);
    assert_eq!(run["requests"][0]["staleKey"], "first:v1");
    assert_eq!(run["requests"][1]["staleKey"], "second:v2");
    let mut declaration: Value =
        serde_json::from_slice(&fs::read(fixture.repo.join("family/artifactize.json")).unwrap())
            .unwrap();
    declaration["evals"] = json!([eval("check", "rm first.txt")]);
    fixture.write("family/artifactize.json", declaration);
    let run = fixture.verify(&["first", "--force"], 2);
    assert_eq!(run["requests"][0]["errorCode"], "STALE_KEY_RECHECK_FAILED");
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
        fixture.write("artifactize.json", json!({"name":"test","staleKey":stale_key(&source),"evals":[eval("check","touch executed")]}));
        let child = fixture
            .command()
            .args(["verify", "--all", "--json"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::read_to_string(&marker).is_err() {
            assert!(Instant::now() < deadline, "stale_key never started");
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
        let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
        assert_eq!(
            db.query_row::<u32, _, _>("SELECT count(*) FROM cache_entries", [], |row| row.get(0))
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
fn stale_key_arguments_resolve_global_names_like_runtime_argv() {
    let fixture = Fixture::new();
    fs::write(
        fixture.repo.join("stale_key.py"),
        "import hashlib, json, pathlib, sys\njson.load(sys.stdin)\ndigest = hashlib.sha256()\nfor root in sys.argv[1:]:\n    for path in sorted(pathlib.Path(root).rglob('*')):\n        if path.is_file():\n            digest.update(path.read_bytes())\nprint(digest.hexdigest())\n",
    )
    .unwrap();
    fixture.write("core/artifactize.json", json!({"name":"core","basis":true}));
    fs::write(fixture.repo.join("core/lib.txt"), "v1").unwrap();
    // The JSON from issue #48: a global Artifact name in both stale_key and runtime argv.
    let api = |reference: &str, mounts: Value| {
        json!({
            "name":"api","mounts":mounts,
            "staleKey":{"script":{"command":"python3","args":["../stale_key.py",".",reference]}},
            "evals":[{"id":"tests","title":"Tests","profile":{"kind":"runtime","command":"python3","args":["-B","test_api.py",reference]},"payload":{"instruction":"Run the API tests."}}]
        })
    };
    fixture.write("api/artifactize.json", api("{core}", json!({})));
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
    let stale_key = first["requests"][0]["staleKey"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(run(&["status"]).0, Some(0));
    fs::write(fixture.repo.join("core/lib.txt"), "v2").unwrap();
    let second = fixture.verify(&["--all"], 0);
    assert_ne!(second["requests"][0]["staleKey"], stale_key.as_str());
    assert_eq!(second["executionsStarted"], 1);

    // Mount aliases keep working.
    fixture.write("api/artifactize.json", api("{lib}", json!({"lib":"core"})));
    assert_eq!(run(&["config", "check"]).0, Some(0));
    fixture.verify(&["--all"], 0);

    // Unknown, family and escaping references fail closed in config check and status alike.
    fixture.write(
        "family/artifactize.json",
        json!({"name":"posts","family":{"instances":{"one":{}}},"basis":true}),
    );
    for (reference, message) in [
        ("{missing}", "Unknown Artifact reference {missing}"),
        ("{posts}", "names an Artifact family"),
        ("{core}/../api", "safe relative logical path"),
    ] {
        fixture.write("api/artifactize.json", api(reference, json!({})));
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
