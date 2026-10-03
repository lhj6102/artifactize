use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    _root: TempDir,
    repo: PathBuf,
    state: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        Self {
            repo,
            state: root.path().join("receipts"),
            home: root.path().join("home"),
            _root: root,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .env("ARTIFACTIZE_STATE_HOME", &self.home)
            .env("ARTIFACTIZE_TEST_SECRET", "must-not-leak")
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state);
        command
    }

    fn runtime_fixture() -> Self {
        let fixture = Self::new();
        copy_directory(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime"),
            &fixture.repo,
        );
        fixture
    }

    fn verify(&self, args: &[&str], code: i32) -> Value {
        let output = self
            .command()
            .arg("verify")
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        json_output(&output)
    }

    fn runtime(&self, program: &str, args: &[&str]) {
        fs::write(self.repo.join("artifactize.json"), json!({
            "name":"test", "evals":[{"id":"check","title":"Check", "profile":{"kind":"runtime","command":program,"args":args}, "payload":{"instruction":"Check runtime."}}]
        }).to_string()).unwrap();
    }
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn copy_directory(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

#[test]
fn verify_then_fresh_read_only_show_retains_audit_without_the_repository() {
    let fixture = Fixture::new();
    copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime"),
        &fixture.repo,
    );
    let output = fixture
        .command()
        .args(["verify", "--all", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let verified = json_output(&output);
    assert_eq!(verified["status"], "ERROR");
    assert!(output.stdout.windows(6).any(|bytes| bytes == b"stdout"));
    assert!(!fixture.home.exists(), "--state-dir moves the whole state");
    fs::remove_dir_all(&fixture.repo).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .current_dir(fixture._root.path())
        .env("ARTIFACTIZE_STATE_HOME", &fixture.home)
        .arg("--state-dir")
        .arg(&fixture.state)
        .args(["run", "show", verified["id"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let full = json_output(&output);
    assert_eq!(full["status"], "ERROR");
    assert_eq!(full, verified);
    let requests = full["requests"].as_array().unwrap();
    let request = |id: &str| {
        requests
            .iter()
            .find(|request| request["evalId"] == id)
            .unwrap()
    };
    let green = request("green/check");
    assert_eq!(green["result"]["verdict"], "GREEN");
    assert_eq!(green["result"]["stdout"], "GREEN scoped input\n");
    assert_eq!(green["result"]["stderr"], "runtime stderr\n");
    assert_eq!(green["result"]["exitCode"], 0);
    assert!(green["result"]["durationMs"].is_u64());
    assert!(green["child"]["pid"].as_u64().unwrap() > 0);
    assert!(green["child"]["startTime"].as_u64().unwrap() > 0);
    assert_eq!(green["payload"]["instruction"], "Check runtime.");
    assert_eq!(
        green["cwd"],
        fixture.repo.join("review").to_string_lossy().as_ref()
    );
    assert_eq!(
        green["argv"][2],
        fixture
            .repo
            .join("input/data.txt")
            .to_string_lossy()
            .as_ref()
    );
    let output_dir = Path::new(green["runDir"].as_str().unwrap());
    assert!(output_dir.starts_with(&fixture.state));
    assert_eq!(
        fs::read_to_string(output_dir.join("output/result.txt")).unwrap(),
        "runtime output\n"
    );
    assert_eq!(request("red/check")["result"]["exitCode"], 7);
    assert_eq!(request("timeout/check")["status"], "ERROR");
    assert_eq!(request("timeout/check")["errorCode"], "TIMEOUT");
    for (id, status) in [
        ("blocked/check", "BLOCKED"),
        ("waiting/check", "WAIT_DEPENDENCY"),
    ] {
        assert_eq!(request(id)["status"], status);
        assert!(request(id)["result"].is_null());
        assert!(request(id)["child"].is_null());
        assert!(request(id)["startedAt"].is_null());
    }
    assert_eq!(request("cycle-a/check")["status"], "GREEN");
    assert_eq!(request("cycle-b/check")["status"], "GREEN");
    let database = Connection::open(fixture.state.join(artifactize::store::DATABASE)).unwrap();
    assert_eq!(
        database
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        database
            .pragma_query_value::<String, _>(None, "journal_mode", |r| r.get(0))
            .unwrap(),
        "wal"
    );
    assert_eq!(
        database
            .query_row::<i64, _, _>("SELECT count(*) FROM run_members", [], |r| r.get(0))
            .unwrap(),
        7
    );
}

#[test]
fn foreground_exit_codes_selection_and_missing_evidence() {
    let fixture = Fixture::new();
    copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime"),
        &fixture.repo,
    );
    for (selection, code, status) in [
        ("green", 0, "GREEN"),
        ("red", 1, "RED"),
        ("timeout", 2, "ERROR"),
        ("blocked", 4, "INCOMPLETE"),
        ("cycle-a", 4, "INCOMPLETE"),
    ] {
        let output = fixture
            .command()
            .args(["verify", selection, "--wait", "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code));
        let run = json_output(&output);
        assert_eq!(run["status"], status);
        assert_eq!(run["requests"].as_array().unwrap().len(), 1);
    }
    assert_eq!(
        fixture
            .command()
            .args(["verify", "red"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    for args in [vec!["verify", "unknown"], vec!["verify", "green", "--all"]] {
        assert_eq!(
            fixture.command().args(args).output().unwrap().status.code(),
            Some(2)
        );
    }
    fs::write(fixture.repo.join("unreviewed.json"), "irrelevant").unwrap();
    let empty = Fixture::new();
    fs::write(
        empty.repo.join("artifactize.json"),
        r#"{"name":"unreviewed"}"#,
    )
    .unwrap();
    assert_eq!(
        empty
            .command()
            .args(["verify", "--all"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(4)
    );
}

#[test]
fn default_state_uses_one_database_and_errors_do_not_invent_results() {
    let fixture = Fixture::new();
    fixture.runtime("/bin/echo", &["$HOME", "a; echo injected", "a b"]);
    let alias = fixture._root.path().join("alias");
    symlink(&fixture.repo, &alias).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .env("ARTIFACTIZE_STATE_HOME", &fixture.home)
        .arg("--repo")
        .arg(&alias)
        .args(["verify", "--all", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let first = json_output(&output);
    assert_eq!(
        first["requests"][0]["result"]["stdout"],
        "$HOME a; echo injected a b\n"
    );
    let state = PathBuf::from(first["stateDir"].as_str().unwrap());
    assert_eq!(state, fixture.home);
    assert_eq!(first["repoPath"], fixture.repo.to_string_lossy().as_ref());
    assert!(state.join("state.sqlite").is_file());
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .env("ARTIFACTIZE_STATE_HOME", &fixture.home)
        .current_dir(fixture._root.path())
        .args(["run", "show", first["id"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(json_output(&output), first);
    for (program, args, code) in [
        ("/artifactize/missing-command", vec![], "SPAWN_FAILED"),
        ("/bin/sh", vec!["-c", "kill -TERM $$"], "ABNORMAL_EXIT"),
        ("/bin/cat", vec!["{test}/missing"], "PREPARATION_FAILED"),
    ] {
        fixture.runtime(program, &args);
        let output = fixture
            .command()
            .args(["verify", "--all", "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let run = json_output(&output);
        assert_eq!(run["requests"][0]["errorCode"], code);
        assert!(run["requests"][0]["result"].is_null());
    }
    fixture.runtime("/bin/true", &[]);
    let second = json_output(
        &fixture
            .command()
            .args(["verify", "--all", "--json"])
            .output()
            .unwrap(),
    );
    let third = json_output(
        &fixture
            .command()
            .args(["verify", "--all", "--json"])
            .output()
            .unwrap(),
    );
    assert_ne!(second["id"], third["id"]);
    assert_ne!(
        second["requests"][0]["runDir"],
        third["requests"][0]["runDir"]
    );
}

#[test]
fn human_waiting_does_not_prevent_runtime_execution() {
    let fixture = Fixture::new();
    fixture.runtime("/bin/true", &[]);
    let path = fixture.repo.join("artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    declaration["evals"].as_array_mut().unwrap().push(json!({"id":"review","title":"Human review","profile":{"kind":"human"},"payload":{"instruction":"Review."}}));
    fs::write(path, declaration.to_string()).unwrap();
    let run = fixture.verify(&["--all"], 4);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["requests"][1]["status"], "WAITING_HUMAN");
    assert_eq!(run["executionsStarted"], 1);
}

fn wait_for(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("verify did not finish after cancellation");
        }
        thread::sleep(Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn ctrl_c_cleans_the_group_persists_cancelled_and_does_not_hold_a_writer_lock() {
    let fixture = Fixture::new();
    let marker = fixture._root.path().join("started");
    fixture.runtime(
        "/bin/sh",
        &[
            "-c",
            "sleep 30 & echo $! > \"$1\"; wait",
            "sh",
            marker.to_str().unwrap(),
        ],
    );
    let child = fixture
        .command()
        .args(["verify", "--all", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "runtime did not start");
        thread::sleep(Duration::from_millis(10));
    }
    let database = Connection::open(fixture.state.join(artifactize::store::DATABASE)).unwrap();
    database.busy_timeout(Duration::from_millis(100)).unwrap();
    database.execute_batch("BEGIN IMMEDIATE; COMMIT;").unwrap();
    let saved: String = database
        .query_row("SELECT data FROM requests", [], |r| r.get(0))
        .unwrap();
    let saved: Value = serde_json::from_str(&saved).unwrap();
    assert!(saved["child"]["pid"].as_u64().unwrap() > 0);
    let status = Command::new("/bin/kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let output = wait_for(child);
    assert_eq!(output.status.code(), Some(2));
    let run = json_output(&output);
    assert_eq!(run["status"], "ERROR");
    assert_eq!(run["requests"][0]["errorCode"], "CANCELLED");
    let show = fixture
        .command()
        .args(["run", "show", run["id"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(show.status.success());
    assert!(json_output(&show)["requests"][0]["result"].is_null());
    let grandchild = fs::read_to_string(marker).unwrap();
    if let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", grandchild.trim())) {
        assert!(
            stat.split_once(") ").unwrap().1.starts_with('Z'),
            "descendant still running: {stat}"
        );
    }
}

#[test]
fn selection_errors_fail_before_discovery_or_receipts() {
    let fixture = Fixture::new();
    for args in [
        vec!["verify"],
        vec!["verify", "green", "red"],
        vec!["verify", "green", "--eval", "green/check"],
        vec!["verify", "--eval", "green/check", "--evals", "red/check"],
        vec!["verify", "--all", "--artifacts", "green"],
        vec![
            "verify",
            "--evals-file",
            "missing",
            "--artifacts-file",
            "missing",
        ],
        vec!["verify", "--artifacts-file", "missing", "green"],
    ] {
        let output = fixture
            .command()
            .args(&args)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let error = json_output(&output)["error"].as_str().unwrap().to_owned();
        assert!(
            error.contains("cannot be used")
                || error.contains("required")
                || error.contains("unexpected argument"),
            "{error}"
        );
        assert!(!fixture.state.exists());
    }
}

#[test]
fn verify_file_and_csv_selectors_preserve_order_and_profiles_execute_without_patching_sources() {
    let fixture = Fixture::new();
    copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime"),
        &fixture.repo,
    );
    let source = fixture.repo.join("review/artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&source).unwrap()).unwrap();
    declaration["evals"][0]["profileVariants"] = json!({
        "brief": {"kind":"runtime","command":"/bin/echo","args":["variant", "{input}/data.txt"],"timeoutMs":1000}
    });
    fs::write(&source, declaration.to_string()).unwrap();
    let original = fs::read(&source).unwrap();
    let selected = fixture
        .command()
        .args([
            "verify",
            "--eval",
            "green/check",
            "--profile",
            "brief",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(selected.status.success());
    let run = json_output(&selected);
    assert_eq!(
        run["selection"],
        json!({"kind":"eval","evalId":"green/check"})
    );
    assert_eq!(run["requests"][0]["profile"]["command"], "/bin/echo");
    assert_eq!(
        run["requests"][0]["result"]["stdout"],
        format!(
            "variant {}\n",
            fixture.repo.join("input/data.txt").display()
        )
    );
    assert_eq!(fs::read(&source).unwrap(), original);

    let path = fixture._root.path().join("ids");
    for (flag, json, lines) in [
        (
            "--evals-file",
            r#"["cycle-b/check","green/check","cycle-a/check","green/check"]"#,
            " cycle-b/check \r\n\r\ngreen/check\r\ncycle-a/check\ngreen/check\n",
        ),
        (
            "--artifacts-file",
            r#"["cycle-b","green","cycle-a","green"]"#,
            " cycle-b \r\n\r\ngreen\r\ncycle-a\ngreen\n",
        ),
    ] {
        for content in [json.to_owned(), format!("\u{feff}{lines}")] {
            fs::write(&path, content).unwrap();
            let output = fixture
                .command()
                .args(["verify", flag])
                .arg(&path)
                .arg("--json")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let run = json_output(&output);
            let requests = run["requests"].as_array().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .map(|request| request["evalId"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["cycle-b/check", "green/check", "cycle-a/check"]
            );
            assert!(requests.iter().all(|request| request["status"] == "GREEN"));
        }
    }
    let output = fixture
        .command()
        .args(["verify", "--artifacts", "cycle-b,cycle-a,cycle-b", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        json_output(&output)["requests"].as_array().unwrap().len(),
        2
    );
    let output = fixture
        .command()
        .args(["verify", "--evals", "cycle-a/check,cycle-a/check", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let run = json_output(&output);
    assert_eq!(run["requests"].as_array().unwrap().len(), 1);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["validation"]["artifacts"].as_array().unwrap().len(), 2);
}

#[test]
fn invalid_selections_and_profiles_never_create_a_run() {
    let fixture = Fixture::new();
    copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime"),
        &fixture.repo,
    );
    for args in [
        vec!["verify", "--eval", "missing"],
        vec!["verify", "--evals", "green/check,"],
        vec!["verify", "--artifacts", "green,unknown"],
        vec!["verify", "--artifacts", ""],
        vec!["verify", "green", "--profile", "unknown"],
        vec!["verify", "green", "--profile", "bad name"],
    ] {
        let output = fixture
            .command()
            .args(&args)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(json_output(&output)["error"].is_string());
        assert!(!fixture.state.exists());
    }
}

fn request<'a>(run: &'a Value, id: &str) -> &'a Value {
    run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|request| request["evalId"] == id)
        .unwrap()
}

#[test]
fn individual_saves_success_while_recursive_completes_cycle_obligations() {
    let fixture = Fixture::runtime_fixture();
    let individual = fixture.verify(&["--eval", "cycle-a/check"], 4);
    assert_eq!(individual["status"], "INCOMPLETE");
    assert_eq!(
        request(&individual, "cycle-a/check")["result"]["verdict"],
        "GREEN"
    );
    assert_eq!(individual["validation"]["obligations"], json!(["cycle-b"]));
    assert_eq!(
        individual["validation"]["includedEvalIds"],
        json!(["cycle-a/check"])
    );
    let saved = fixture
        .command()
        .args(["run", "show", individual["id"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(saved.status.success());
    assert_eq!(json_output(&saved), individual);

    let recursive = fixture.verify(&["--eval", "cycle-a/check", "--recursive", "--force"], 0);
    assert_eq!(recursive["status"], "GREEN");
    assert_eq!(recursive["validation"]["obligations"], json!([]));
    assert_eq!(
        recursive["validation"]["selectedEvalIds"],
        json!(["cycle-a/check"])
    );
    assert_eq!(
        recursive["validation"]["includedEvalIds"],
        json!(["cycle-a/check", "cycle-b/check"])
    );
    assert_eq!(request(&recursive, "cycle-a/check")["force"], true);
    assert_eq!(request(&recursive, "cycle-b/check")["force"], false);
    assert!(
        recursive["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "GREEN")
    );
}

#[test]
fn force_neither_expands_selection_nor_bypasses_gates() {
    let fixture = Fixture::runtime_fixture();
    let individual = fixture.verify(&["blocked", "--force"], 4);
    assert_eq!(individual["requests"].as_array().unwrap().len(), 1);
    assert_eq!(
        request(&individual, "blocked/check")["status"],
        "WAIT_DEPENDENCY"
    );
    assert_eq!(request(&individual, "blocked/check")["force"], true);
    assert!(request(&individual, "blocked/check")["startedAt"].is_null());

    let recursive = fixture.verify(&["blocked", "--recursive", "--force"], 1);
    assert_eq!(request(&recursive, "blocked/check")["status"], "BLOCKED");
    assert_eq!(request(&recursive, "red/check")["force"], false);
    assert_eq!(recursive["status"], "RED");
    let waiting = fixture.verify(&["waiting", "--recursive"], 2);
    assert_eq!(
        request(&waiting, "waiting/check")["status"],
        "WAIT_DEPENDENCY"
    );
    assert_eq!(request(&waiting, "timeout/check")["status"], "ERROR");

    let all = fixture.verify(&["--all", "--force"], 2);
    assert!(
        all["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["force"] == true)
    );
}

#[test]
fn ignore_gates_saves_actual_verdicts_but_never_waives_final_obligations() {
    let fixture = Fixture::runtime_fixture();
    let individual = fixture.verify(&["blocked", "--ignore-gates"], 4);
    assert_eq!(individual["ignoreGates"], true);
    assert_eq!(request(&individual, "blocked/check")["status"], "GREEN");
    assert_eq!(individual["validation"]["obligations"], json!(["red"]));
    assert_eq!(individual["validation"]["satisfied"], false);
    assert_eq!(individual["validation"]["evals"][0]["status"], "GREEN");
    let recursive = fixture.verify(&["blocked", "--recursive", "--ignore-gates"], 1);
    assert_eq!(request(&recursive, "blocked/check")["status"], "GREEN");
    assert_eq!(request(&recursive, "red/check")["status"], "RED");
    assert_eq!(recursive["validation"]["obligations"], json!(["red"]));

    let path = fixture.repo.join("blocked/artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    declaration["evals"][0]["profile"] =
        json!({"kind":"runtime","command":"/missing-command","args":[]});
    fs::write(path, declaration.to_string()).unwrap();
    let failed = fixture.verify(&["blocked", "--ignore-gates"], 2);
    assert_eq!(request(&failed, "blocked/check")["status"], "ERROR");
    assert_eq!(failed["status"], "ERROR");
    assert_eq!(failed["validation"]["evals"][0]["status"], "ERROR");
}

#[tokio::test]
async fn root_gate_policy_is_honored_and_explicit_sdk_false_overrides_ignore() {
    use artifactize::{
        project::{VerifyOptions, selection::Selection, verify},
        store::read_run,
    };
    use tokio_util::sync::CancellationToken;

    let fixture = Fixture::runtime_fixture();
    fs::write(
        fixture.repo.join("artifactize.json"),
        r#"{"name":"root","basis":true,"reviewPolicy":{"dependencyGates":"ignore"}}"#,
    )
    .unwrap();
    let inherited = fixture.verify(&["blocked"], 4);
    assert_eq!(inherited["ignoreGates"], true);
    assert_eq!(request(&inherited, "blocked/check")["status"], "GREEN");
    assert_eq!(inherited["validation"]["obligations"], json!(["red"]));

    let enforced = verify(
        &fixture.repo,
        Some(&fixture.state),
        &Selection::Artifact {
            artifact_id: "blocked".into(),
        },
        &VerifyOptions {
            ignore_gates: Some(false),
            ..Default::default()
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!enforced.run.ignore_gates);
    assert_eq!(enforced.run.status, "INCOMPLETE");
    assert_eq!(enforced.requests[0].status, "WAIT_DEPENDENCY");
    let saved = read_run(&fixture.state, &enforced.run.id).await.unwrap();
    assert!(!saved.run.ignore_gates);

    fs::write(
        fixture.repo.join("artifactize.json"),
        r#"{"name":"root","basis":true,"reviewPolicy":{"dependencyGates":"green"}}"#,
    )
    .unwrap();
    let override_green = fixture.verify(&["blocked", "--ignore-gates"], 4);
    assert_eq!(request(&override_green, "blocked/check")["status"], "GREEN");
}

#[test]
fn basis_and_all_keep_no_eval_dependency_obligations() {
    let fixture = Fixture::new();
    fixture.runtime("/bin/true", &[]);
    fs::create_dir(fixture.repo.join("input")).unwrap();
    fs::write(
        fixture.repo.join("input/artifactize.json"),
        r#"{"name":"input"}"#,
    )
    .unwrap();
    let individual = fixture.verify(&["test"], 4);
    assert_eq!(individual["validation"]["obligations"], json!(["input"]));
    assert_eq!(individual["requests"][0]["status"], "GREEN");
    let recursive = fixture.verify(&["test", "--recursive", "--ignore-gates"], 4);
    assert_eq!(recursive["validation"]["obligations"], json!(["input"]));
    let text = fixture
        .command()
        .args(["verify", "--all"])
        .output()
        .unwrap();
    assert_eq!(text.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&text.stdout).contains("Unmet obligation: input"));

    fs::write(
        fixture.repo.join("artifactize.json"),
        r#"{"name":"test","basis":true}"#,
    )
    .unwrap();
    let basis = fixture.verify(&["test"], 4);
    assert!(basis["requests"].as_array().unwrap().is_empty());
    assert_eq!(basis["validation"]["obligations"], json!(["input"]));
    fs::write(
        fixture.repo.join("input/artifactize.json"),
        r#"{"name":"input","basis":true}"#,
    )
    .unwrap();
    assert_eq!(fixture.verify(&["--all"], 0)["status"], "GREEN");
}

#[test]
fn recursive_profile_selection_includes_dependencies_and_rebuilds_scope() {
    let fixture = Fixture::runtime_fixture();
    for folder in ["blocked", "red"] {
        let path = fixture.repo.join(folder).join("artifactize.json");
        let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        declaration["evals"][0]["profileVariants"] =
            json!({"pass":{"kind":"runtime","command":"/bin/echo","args":["{input}"]}});
        fs::write(path, declaration.to_string()).unwrap();
    }
    let recursive = fixture.verify(&["blocked", "--recursive", "--profile", "pass"], 0);
    assert_eq!(recursive["requests"].as_array().unwrap().len(), 2);
    assert!(
        recursive["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["profile"]["command"] == "/bin/echo")
    );
    assert_eq!(
        recursive["validation"]["artifacts"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let input = recursive["validation"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "input")
        .unwrap();
    assert_eq!(input["status"], "BASIS");
    let individual = fixture.verify(&["blocked", "--profile", "pass"], 4);
    assert_eq!(individual["requests"].as_array().unwrap().len(), 1);
    assert_eq!(individual["requests"][0]["status"], "WAIT_DEPENDENCY");
}

#[test]
fn recursive_family_selection_includes_external_evals_without_forcing_them() {
    let fixture = Fixture::new();
    copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/families"),
        &fixture.repo,
    );
    fs::create_dir(fixture.repo.join("external")).unwrap();
    fs::write(fixture.repo.join("external/artifactize.json"), json!({
        "name":"external", "evals":[{"id":"check","title":"External",
            "profile":{"kind":"runtime","command":"/bin/true","args":[]},
            "profileVariants":{"brief":{"kind":"runtime","command":"/bin/echo","args":["external variant"]}},
            "payload":{"instruction":"Check."}}]
    }).to_string()).unwrap();
    let path = fixture.repo.join("scenarios/artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    declaration["mounts"] = json!({"external":"external"});
    declaration["evals"][0]["profileVariants"] =
        json!({"brief":{"kind":"runtime","command":"/bin/echo","args":["family variant"]}});
    fs::write(path, declaration.to_string()).unwrap();

    let individual = fixture.verify(&["scenarios"], 4);
    assert_eq!(individual["requests"].as_array().unwrap().len(), 2);
    assert!(
        individual["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "WAIT_DEPENDENCY")
    );
    let instance = fixture.verify(&["checkout", "--recursive", "--force"], 0);
    assert_eq!(instance["requests"].as_array().unwrap().len(), 2);
    assert_eq!(request(&instance, "checkout/review")["force"], true);
    assert_eq!(request(&instance, "external/check")["force"], false);
    assert!(
        !instance["validation"]["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == "search")
    );

    let family = fixture.verify(
        &[
            "--artifacts",
            "scenarios,checkout",
            "--recursive",
            "--force",
            "--profile",
            "brief",
        ],
        0,
    );
    assert_eq!(family["requests"].as_array().unwrap().len(), 3);
    assert_eq!(request(&family, "checkout/review")["force"], true);
    assert_eq!(request(&family, "search/review")["force"], true);
    assert_eq!(request(&family, "external/check")["force"], false);
    assert!(
        family["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "GREEN" && r["profile"]["command"] == "/bin/echo")
    );
    assert!(!fixture.repo.join("scenarios/identity-ran").exists());
}

#[test]
fn recursive_eval_selection_includes_sibling_evals_on_the_same_artifact() {
    let fixture = Fixture::new();
    fixture.runtime("/bin/true", &[]);
    let path = fixture.repo.join("artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut sibling = declaration["evals"][0].clone();
    sibling["id"] = json!("sibling");
    declaration["evals"].as_array_mut().unwrap().push(sibling);
    fs::write(path, declaration.to_string()).unwrap();
    let individual = fixture.verify(&["--eval", "test/check"], 4);
    assert_eq!(individual["validation"]["obligations"], json!(["test"]));
    let recursive = fixture.verify(&["--eval", "test/check", "--recursive", "--force"], 0);
    assert_eq!(recursive["requests"].as_array().unwrap().len(), 2);
    assert_eq!(request(&recursive, "test/check")["force"], true);
    assert_eq!(request(&recursive, "test/sibling")["force"], false);
}

#[test]
fn agent_errors_run_alongside_runtime_and_survive_fresh_audit_reads() {
    for (backend, expected) in [
        ("openai", "OPENAI_API_KEY"),
        ("anthropic", "ANTHROPIC_API_KEY"),
        ("chatgpt", "artifactize login chatgpt"),
        ("claude", "could not be spawned"),
    ] {
        let fixture = Fixture::new();
        fixture.runtime("/bin/true", &[]);
        let path = fixture.repo.join("artifactize.json");
        let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        declaration["evals"].as_array_mut().unwrap().push(json!({"id":"agent","title":"Agent","profile":{"kind":"agent","backend":backend,"model":"exact-model"},"payload":{"instruction":"Review."}}));
        fs::write(path, declaration.to_string()).unwrap();
        let output = fixture
            .command()
            .env("PATH", fixture.state.join("no-claude"))
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .args(["verify", "--all", "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let run = json_output(&output);
        assert_eq!(run["requests"][0]["status"], "GREEN");
        assert_eq!(run["requests"][1]["status"], "ERROR");
        assert!(
            run["requests"][1]["error"]
                .as_str()
                .unwrap()
                .contains(expected)
        );
        if backend == "claude" {
            assert_eq!(run["requests"][1]["usage"][0]["usage"], json!({}));
        } else {
            assert_eq!(run["requests"][1]["usage"], json!([]));
        }
        assert_eq!(run["requests"][1]["toolCalls"], json!([]));
        assert!(run["requests"][1]["result"].is_null());
        fs::remove_dir_all(&fixture.repo).unwrap();
        let shown = fixture
            .command()
            .args(["run", "show", run["id"].as_str().unwrap(), "--json"])
            .output()
            .unwrap();
        assert_eq!(json_output(&shown)["requests"], run["requests"]);
    }
}

#[test]
fn runtime_and_agent_starts_share_the_run_budget() {
    let fixture = Fixture::new();
    fixture.runtime("/bin/true", &[]);
    let path = fixture.repo.join("artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    declaration["evals"].as_array_mut().unwrap().push(json!({"id":"agent","title":"Agent","profile":{"kind":"agent","backend":"chatgpt","model":"not-called"},"payload":{"instruction":"Review."}}));
    fs::write(path, declaration.to_string()).unwrap();
    let run = fixture.verify(&["--all", "--jobs", "1", "--max-executions", "1"], 4);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["requests"][1]["status"], "BUDGET_EXHAUSTED");
    assert!(run["requests"][1]["error"].is_null());
    let run = fixture.verify(&["--all", "--jobs", "1", "--max-executions", "2"], 2);
    assert_eq!(run["executionsStarted"], 2);
    assert_eq!(run["requests"][1]["status"], "ERROR");
    assert!(
        run["requests"][1]["error"]
            .as_str()
            .unwrap()
            .contains("artifactize login chatgpt")
    );
}
