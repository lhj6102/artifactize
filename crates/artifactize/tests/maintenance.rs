use std::{fs, path::Path, process::Command};

use rusqlite::params;
use serde_json::{Value, json};
use support::os::bin;

mod support;

/// A link to a directory: a symlink on Unix, and on Windows a junction, which needs no
/// privilege and which artifactize refuses just the same.
fn link_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    support::os::symlink_dir(target, link).unwrap();
    #[cfg(windows)]
    support::os::junction(target, link);
}

fn remove_link(link: &Path) {
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
    // Windows removes a directory link as a directory.
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
}

fn command(state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
    command.arg("--state-dir").arg(state).arg("--json");
    command
}

fn result(command: &mut Command, code: i32) -> Value {
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn check<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == name)
        .unwrap()
}

fn verify(state: &Path, repo: &Path, profile: Value, code: i32) -> Value {
    fs::create_dir_all(repo).unwrap();
    support::declaration::write(repo.join("index.artf"), json!({
        "name":"a", "fingerprint":false, "evals":[{"id":"review", "title":"Review", "payload":{"instruction":"Review"}, "profile":profile}]
    }).to_string()).unwrap();
    // A Human request is recorded and left waiting: the wait times out at once.
    result(
        command(state)
            .arg("--repo")
            .arg(repo)
            .args(["verify", "--all", "--timeout-ms", "1"]),
        code,
    )
}

fn runtime_run(state: &Path, repo: &Path) -> Value {
    verify(
        state,
        repo,
        json!({"kind":"runtime", "command":bin("/bin/true"), "args":[]}),
        0,
    )
}

fn doctor(state: &Path, path: &Path) -> Command {
    let mut command = command(state);
    command
        .arg("doctor")
        .env("PATH", path)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ARTIFACTIZE_CODEX_AUTH_FILE")
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .env("HTTP_PROXY", "http://127.0.0.1:1");
    command
}

#[test]
fn doctor_is_local_and_preserves_credentials_and_all_database_rows() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let absent = result(&mut doctor(&state, &bin), 0);
    for name in ["openai", "anthropic"] {
        assert_eq!(check(&absent, name)["status"], "WARN");
        assert_eq!(check(&absent, name)["details"]["present"], false);
    }
    assert_eq!(check(&absent, "codex")["status"], "WARN");
    assert_eq!(check(&absent, "codex")["details"]["source"], "none");
    // 0.5.0 removed the chatgpt and claude backends and their checks.
    let names: Vec<_> = absent["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|check| check["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "state",
            "schema",
            "limits",
            "sessions",
            "openai",
            "anthropic",
            "codex",
            "remote"
        ]
    );
    assert_eq!(check(&absent, "limits")["details"], json!({"backends":{}}));
    let sessions = &check(&absent, "sessions")["details"];
    assert_eq!(
        (
            &sessions["enabled"],
            &sessions["sessions"],
            &sessions["bytes"],
            &sessions["stateId"]
        ),
        (&json!(true), &json!(0), &json!(0), &Value::Null)
    );
    assert_eq!(sessions["maxBytes"], 1_073_741_824);
    assert_eq!(sessions["targetBytes"], 805_306_368);
    assert_eq!(check(&absent, "state")["details"]["writable"], true);
    assert_eq!(check(&absent, "schema")["details"], json!({"schema":null}));
    assert!(!state.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);

    runtime_run(&state, &repo);
    let before = fs::read(state.join("state.sqlite")).unwrap();
    // Leftover 0.4 ChatGPT credentials are neither read nor removed.
    let auth = state.join("auth");
    support::os::create_private_dir_all(&auth);
    let leftover = json!({"access_token":"private-access", "refresh_token":"private-refresh"});
    support::os::write_private_file(&auth.join("chatgpt.json"), leftover.to_string());
    let present = result(
        doctor(&state, &bin)
            .arg("--repo")
            .arg(&repo)
            .env("OPENAI_API_KEY", "secret-openai")
            .env("ANTHROPIC_API_KEY", "secret-anthropic"),
        0,
    );
    assert_eq!(present["ok"], true);
    assert_eq!(
        check(&present, "schema")["details"],
        json!({"schema":artifactize::store::STATE_SCHEMA_VERSION})
    );
    for name in ["openai", "anthropic", "config", "schema"] {
        assert_eq!(check(&present, name)["status"], "PASS");
    }
    let output = present.to_string();
    for secret in [
        "secret-openai",
        "secret-anthropic",
        "private-access",
        "private-refresh",
    ] {
        assert!(!output.contains(secret));
    }
    assert_eq!(
        fs::read_to_string(auth.join("chatgpt.json")).unwrap(),
        leftover.to_string()
    );
    assert_eq!(fs::read_dir(&auth).unwrap().count(), 1);
    assert_eq!(fs::read(state.join("state.sqlite")).unwrap(), before);

    // A database written by a newer artifactize is a hard error and stays as it is.
    rusqlite::Connection::open(state.join("state.sqlite"))
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let before = fs::read(state.join("state.sqlite")).unwrap();
    let newer = result(&mut doctor(&state, &bin), 1);
    assert_eq!(check(&newer, "schema")["status"], "FAIL");
    let supported = artifactize::store::STATE_SCHEMA_VERSION;
    assert_eq!(
        check(&newer, "schema")["details"],
        json!({"schema":99,"supported":supported})
    );
    assert_eq!(fs::read(state.join("state.sqlite")).unwrap(), before);

    // An earlier artifactize's database is a hard error too: it is never migrated.
    let older = root.path().join("older");
    fs::create_dir(&older).unwrap();
    rusqlite::Connection::open(older.join("state.sqlite"))
        .unwrap()
        .execute_batch(
            "CREATE TABLE run_members(run_id TEXT, eval_id TEXT); PRAGMA user_version=4;",
        )
        .unwrap();
    let before = fs::read(older.join("state.sqlite")).unwrap();
    let report = result(&mut doctor(&older, &bin), 1);
    assert_eq!(check(&report, "schema")["status"], "FAIL");
    assert_eq!(
        check(&report, "schema")["details"],
        json!({"schema":4,"supported":supported})
    );
    assert_eq!(
        check(&report, "schema")["message"],
        artifactize::store::EARLIER_STATE
    );
    assert_eq!(fs::read(older.join("state.sqlite")).unwrap(), before);
    assert!(!older.join("state.sqlite-wal").exists());
}

#[test]
fn doctor_reports_hard_local_errors_and_models_cli_needs_no_run() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(repo.join("index.artf"), "invalid").unwrap();
    let report = result(doctor(&state, root.path()).arg("--repo").arg(&repo), 1);
    assert_eq!(check(&report, "config")["status"], "FAIL");
    fs::write(&state, "not a directory").unwrap();
    let report = result(&mut doctor(&state, root.path()), 1);
    assert_eq!(check(&report, "state")["status"], "FAIL");
    fs::remove_file(&state).unwrap();
    let report = result(
        doctor(&repo.join("state"), root.path())
            .arg("--repo")
            .arg(&repo),
        1,
    );
    assert_eq!(check(&report, "state")["status"], "FAIL");
    assert!(!repo.join("state").exists());
    for (backend, variable) in [
        ("openai", "OPENAI_API_KEY"),
        ("anthropic", "ANTHROPIC_API_KEY"),
    ] {
        let failure = result(
            command(&state)
                .args(["models", backend])
                .env_remove(variable),
            2,
        );
        assert!(failure["error"].as_str().unwrap().contains(variable));
    }
    for removed in ["chatgpt", "claude"] {
        result(command(&state).args(["models", removed]), 2);
    }
    assert!(!state.exists());
}

#[test]
fn prune_removes_only_finished_output_and_dry_run_preserves_everything() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    let finished = runtime_run(&state, &repo);
    let waiting = verify(&state, &repo, json!({"kind":"human"}), 3);
    let active = runtime_run(&state, &repo);
    let owned = runtime_run(&state, &repo);
    let db = rusqlite::Connection::open(state.join("state.sqlite")).unwrap();
    let id = |run: &Value| run["id"].as_str().unwrap().to_owned();
    db.execute("UPDATE runs SET status='RUNNING' WHERE id=?", [id(&active)])
        .unwrap();
    let start = support::os::start_time(std::process::id()) as i64;
    db.execute("UPDATE executions SET owner_pid=?,owner_start_time=? WHERE json_extract(data,'$.provenance.runId')=?", params![std::process::id(), start, id(&owned)]).unwrap();
    drop(db);
    let run = state.join("runs").join(id(&finished));
    for path in [
        "tool-leftover/runtime-leftover/output/content",
        "tool-output-test/content",
        "unknown/keep",
        "runtime-extra/audit.json",
    ] {
        let path = run.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "keep audit, prune scratch").unwrap();
    }
    let waiting_output = state.join("runs").join(id(&waiting)).join("output");
    fs::create_dir_all(&waiting_output).unwrap();
    let before = fs::read(state.join("state.sqlite")).unwrap();
    let repo_before = fs::read(repo.join("index.artf")).unwrap();
    let dry = result(command(&state).args(["prune", "--dry-run"]), 0);
    assert_eq!(dry["removed"], json!([]));
    assert_eq!(dry["wouldRemove"].as_array().unwrap().len(), 6);
    assert!(
        dry["wouldRemove"]
            .as_array()
            .unwrap()
            .iter()
            .all(|path| Path::new(path.as_str().unwrap()).is_dir())
    );
    for run in [&waiting, &active, &owned] {
        assert!(
            dry["skippedRuns"]
                .as_array()
                .unwrap()
                .contains(&json!(id(run)))
        );
    }
    let young = result(command(&state).args(["prune", "--older-than", "7d"]), 0);
    assert_eq!(young["removed"], json!([]));
    assert!(
        young["skippedRuns"]
            .as_array()
            .unwrap()
            .contains(&json!(id(&finished)))
    );
    let removed = result(command(&state).args(["prune", "--older-than", "0s"]), 0);
    assert_eq!(removed["removed"], dry["wouldRemove"]);
    for path in removed["removed"].as_array().unwrap() {
        assert!(!Path::new(path.as_str().unwrap()).exists());
    }
    assert!(waiting_output.is_dir());
    assert!(
        Path::new(active["requests"][0]["runDir"].as_str().unwrap())
            .join("output")
            .is_dir()
    );
    assert!(
        Path::new(owned["requests"][0]["runDir"].as_str().unwrap())
            .join("output")
            .is_dir()
    );
    assert!(run.join("unknown/keep").exists());
    assert!(run.join("runtime-extra/audit.json").exists());
    assert_eq!(fs::read(state.join("state.sqlite")).unwrap(), before);
    assert_eq!(fs::read(repo.join("index.artf")).unwrap(), repo_before);
    let shown = result(command(&state).args(["run", "show", &id(&finished)]), 0);
    assert_eq!(shown, finished);
}

#[test]
fn prune_refuses_symlinks_and_repository_targets_before_deleting() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    let finished = runtime_run(&state, &repo);
    let run = state.join("runs").join(finished["id"].as_str().unwrap());
    let output = Path::new(finished["requests"][0]["runDir"].as_str().unwrap()).join("output");
    for link in [run.join("tool-output-link"), output.join("nested-link")] {
        link_dir(&repo, &link);
        let error = result(command(&state).args(["prune"]), 2);
        assert!(error["error"].as_str().unwrap().contains("symlink"));
        assert!(output.is_dir());
        assert!(repo.join("index.artf").exists());
        remove_link(&link);
    }
    let alias = root.path().join("alias");
    link_dir(&state, &alias);
    assert!(
        result(command(&alias).arg("prune"), 2)["error"]
            .as_str()
            .unwrap()
            .contains("symlink")
    );
    let moved = state.join("saved-runs");
    fs::rename(state.join("runs"), &moved).unwrap();
    link_dir(&moved, &state.join("runs"));
    assert!(
        result(command(&state).arg("prune"), 2)["error"]
            .as_str()
            .unwrap()
            .contains("symlink")
    );
    remove_link(&state.join("runs"));
    fs::rename(moved, state.join("runs")).unwrap();
    fs::write(run.join(".git"), "gitdir: elsewhere").unwrap();
    assert!(
        result(command(&state).arg("prune"), 2)["error"]
            .as_str()
            .unwrap()
            .contains("repository")
    );
    assert!(output.is_dir());
    fs::remove_file(run.join(".git")).unwrap();
    support::declaration::write(output.join("index.artf"), "{}").unwrap();
    assert!(
        result(command(&state).arg("prune"), 2)["error"]
            .as_str()
            .unwrap()
            .contains("repository")
    );
    assert!(output.is_dir());
    for duration in ["-1d", "10", "1.5h", "999999999999999999999d"] {
        result(command(&state).args(["prune", "--older-than", duration]), 2);
    }
}

/// Windows opens a name in any case, so a state folder named in another case is still inside
/// the repository. Prune compares canonical paths and refuses it as it refuses the same case.
#[cfg(windows)]
#[test]
fn prune_refuses_a_state_inside_the_repository_named_in_another_case() {
    let root = tempfile::tempdir().unwrap();
    let repo = support::os::canonical(root.path()).join("Repo");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(repo.join("index.artf"), r#"{"name":"a","basis":true}"#).unwrap();
    for state in [
        repo.join("state"),
        support::os::other_case(&repo).join("state"),
    ] {
        let error = result(command(&state).arg("prune").arg("--repo").arg(&repo), 2);
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("outside the reviewed repository"),
            "{state:?}: {error}"
        );
    }
}
