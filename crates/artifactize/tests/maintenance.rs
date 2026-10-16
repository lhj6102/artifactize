#[path = "support/run.rs"]
mod owned_run;

use std::{fs, path::Path, process::Command};

use rusqlite::params;
use serde_json::{Value, json};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

use support::os::{link_dir, remove_link_dir as remove_link, true_program};

fn command(state: &Path) -> Command {
    let mut command = Command::new(support::os::cli());
    command.arg("--state-dir").arg(state).arg("--json");
    command
}

fn result(command: &mut Command, code: i32) -> Value {
    let output = command.record_output().unwrap();
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
    support::declaration::write(
        repo.join("index.artf"),
        json!({
            "name":"a",
            "fingerprint":false,
            "evals":[
                {
                    "id":"review",
                    "title":"Review",
                    "payload":{"instruction":"Review"},
                    "profile":profile,
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    // A Human request is recorded and observed while the owned Run remains waiting.
    result(
        command(state)
            .arg("--repo")
            .arg(repo)
            .args(["verify", "--all"]),
        code,
    )
}

fn runtime_run(state: &Path, repo: &Path) -> Value {
    verify(
        state,
        repo,
        json!({"kind":"runtime", "command":true_program(), "args":[]}),
        0,
    )
}

/// Seed retention input only after the producer has released its saved Run and SQLite.
fn run_completion(state: &Path, repo: &Path, run: &Value, at: &str) {
    let id: artifactize_core::types::RunId = run["id"].as_str().unwrap().parse().unwrap();
    support::runs::finished_worker(state, repo, &id, support::os::running);
    let at: artifactize_core::types::Timestamp = at.parse().unwrap();
    let db = rusqlite::Connection::open(state.join("state.sqlite")).unwrap();
    let saved: String = db
        .query_row("SELECT data FROM runs WHERE id=?", [&id], |row| row.get(0))
        .unwrap();
    let mut saved: artifactize_core::store::Run = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved.status(), artifactize_core::types::RunStatus::Green);
    // Keep the Run's own chronology valid without depending on the producer's clock.
    saved.created_at = at;
    saved.state = artifactize_core::store::RunState::Green { at };
    assert_eq!(
        db.execute(
            "UPDATE runs SET data=? WHERE id=?",
            params![serde_json::to_string(&saved).unwrap(), id],
        )
        .unwrap(),
        1
    );
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
    let root = owned_run::Root::from(support::os::tempdir());
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

    root.track(&state, &repo);
    let run = runtime_run(&state, &repo);
    let id = run["id"].as_str().unwrap().parse().unwrap();
    support::runs::finished_worker(&state, &repo, &id, support::os::running);
    // Terminal evidence can still live in WAL while readers close. Establish the physical
    // byte baseline only after every completed page has reached the main database file.
    {
        // Bound fixture checkpoint establishment, not any execution or elapsed-time assertion.
        const CHECKPOINT_BOUND: std::time::Duration = std::time::Duration::from_secs(5);
        let db = rusqlite::Connection::open(state.join("state.sqlite")).unwrap();
        db.busy_timeout(support::os::patience(CHECKPOINT_BOUND))
            .unwrap();
        let checkpoint: (u32, u32, u32) = db
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(checkpoint, (0, 0, 0), "fixture WAL is fully checkpointed");
        db.close().unwrap();
    }
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
        json!({"schema":artifactize_core::store::STATE_SCHEMA_VERSION})
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

    // No live fixture remains when schema corruption makes lifecycle reads invalid.
    root.release(&state, &repo).unwrap();
    // A database written by a newer artifactize is a hard error and stays as it is.
    rusqlite::Connection::open(state.join("state.sqlite"))
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let before = fs::read(state.join("state.sqlite")).unwrap();
    let newer = result(&mut doctor(&state, &bin), 1);
    assert_eq!(check(&newer, "schema")["status"], "FAIL");
    let supported = artifactize_core::store::STATE_SCHEMA_VERSION;
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
        artifactize_core::store::EARLIER_STATE
    );
    assert_eq!(fs::read(older.join("state.sqlite")).unwrap(), before);
    assert!(!older.join("state.sqlite-wal").exists());
}

#[test]
fn doctor_reports_hard_local_errors_and_models_cli_needs_no_run() {
    let root = support::os::tempdir();
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
    let root = owned_run::Root::from(support::os::tempdir());
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    root.track(&state, &repo);
    let finished = runtime_run(&state, &repo);
    let waiting = verify(&state, &repo, json!({"kind":"human"}), 0);
    let _waiting = owned_run::RunLease::new(
        &state,
        &repo,
        waiting["id"].as_str().unwrap().parse().unwrap(),
    );
    let active = runtime_run(&state, &repo);
    let owned = runtime_run(&state, &repo);
    let db = rusqlite::Connection::open(state.join("state.sqlite")).unwrap();
    let id = |run: &Value| run["id"].as_str().unwrap().to_owned();
    db.execute("UPDATE runs SET status='RUNNING' WHERE id=?", [id(&active)])
        .unwrap();
    let pid = support::os::current_pid();
    let start = support::os::start_time(pid) as i64;
    db.execute(
        "UPDATE executions SET owner_pid=?,owner_start_time=? WHERE json_extract(data,'$.provenance.runId')=?",
        params![pid, start, id(&owned)],
    )
    .unwrap();
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
    // Fixed fixtures lie well on either side of present-day prune cutoffs. Neither
    // retention assertion depends on freshly executed Runs or clock resolution.
    const RETAINED_COMPLETION: &str = "2999-01-01T00:00:00Z";
    const EXPIRED_COMPLETION: &str = "2000-01-01T00:00:00Z";
    run_completion(&state, &repo, &finished, RETAINED_COMPLETION);
    let before = fs::read(state.join("state.sqlite")).unwrap();
    let repo_before = fs::read(repo.join("index.artf")).unwrap();
    let dry = result(command(&state).args(["prune", "--dry-run"]), 0);
    assert_eq!(dry["removed"], json!([]));
    // Assert the operator-visible candidates independently of prune's implementation.
    let mut candidates = dry["wouldRemove"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    candidates.sort();
    let mut expected = [
        support::os::path_text(&run.join("tool-leftover")),
        support::os::path_text(&run.join("tool-output-test")),
        support::os::path_text(
            &Path::new(finished["requests"][0]["runDir"].as_str().unwrap()).join("output"),
        ),
    ];
    expected.sort();
    assert_eq!(candidates, expected);
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
    assert_eq!(fs::read(state.join("state.sqlite")).unwrap(), before);
    // This is a second explicit fixture stimulus, not a write made by prune. Establish
    // a new preservation baseline and saved public audit before exercising deletion.
    run_completion(&state, &repo, &finished, EXPIRED_COMPLETION);
    let finished = result(command(&state).args(["run", "show", &id(&finished)]), 0);
    let before = fs::read(state.join("state.sqlite")).unwrap();
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
    for path in ["unknown/keep", "runtime-extra/audit.json"] {
        assert_eq!(
            fs::read_to_string(run.join(path)).unwrap(),
            "keep audit, prune scratch"
        );
    }
    assert_eq!(fs::read(state.join("state.sqlite")).unwrap(), before);
    assert_eq!(fs::read(repo.join("index.artf")).unwrap(), repo_before);
    let shown = result(command(&state).args(["run", "show", &id(&finished)]), 0);
    assert_eq!(shown, finished);
}

#[test]
fn prune_refuses_symlinks_and_repository_targets_before_deleting() {
    let root = owned_run::Root::from(support::os::tempdir());
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    root.track(&state, &repo);
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
    fs::remove_file(output.join("index.artf")).unwrap();
    fs::write(output.join("file.txt"), "input").unwrap();
    support::declaration::write(output.join("file.txt.artf"), r#"{"name":"file"}"#).unwrap();
    assert!(
        result(command(&state).arg("prune"), 2)["error"]
            .as_str()
            .unwrap()
            .contains("repository")
    );
    assert!(output.join("file.txt").is_file());
    for duration in ["-1d", "10", "1.5h", "999999999999999999999d"] {
        result(command(&state).args(["prune", "--older-than", duration]), 2);
    }
}

// A case-insensitive volume opens a name in any case, so a state folder named in another case
// is still inside the repository. Prune refuses it as it refuses the same case.
#[test]
fn prune_refuses_a_state_inside_the_repository_named_in_another_case() {
    let root = support::os::tempdir();
    // The behavior exists only where the volume ignores case.
    if !support::os::case_insensitive(root.path()) {
        return;
    }
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

#[test]
fn prune_preserves_gitless_workspace_copies_with_current_or_legacy_markers() {
    for marker in [
        "index.artf",
        "artifactize.json",
        ".artifactizeignore",
        "file.txt.artf",
    ] {
        let root = owned_run::Root::from(support::os::tempdir());
        let state = root.path().join("state");
        let repo = root.path().join("reviewed");
        root.track(&state, &repo);
        let finished = runtime_run(&state, &repo);
        let copy =
            Path::new(finished["requests"][0]["runDir"].as_str().unwrap()).join("output/copy");
        fs::create_dir_all(&copy).unwrap();
        fs::write(copy.join(marker), "workspace marker").unwrap();
        fs::write(copy.join("keep.txt"), "owner content").unwrap();
        for args in [vec!["prune", "--dry-run"], vec!["prune"]] {
            let error = result(command(&state).args(args), 2);
            assert!(
                error["error"]
                    .as_str()
                    .unwrap()
                    .contains("Prune refuses repository content"),
                "{marker}: {error}"
            );
            assert_eq!(
                fs::read_to_string(copy.join("keep.txt")).unwrap(),
                "owner content"
            );
            assert_eq!(
                fs::read_to_string(copy.join(marker)).unwrap(),
                "workspace marker"
            );
        }
    }
}

#[test]
fn remote_logout_refuses_credential_state_inside_current_and_legacy_workspaces() {
    for marker in [
        "index.artf",
        "artifactize.json",
        ".artifactizeignore",
        "file.txt.artf",
    ] {
        let root = support::os::tempdir();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join(marker), "workspace marker").unwrap();
        let state = workspace.join("state");
        let output = Command::new(support::os::cli())
            .arg("--state-dir")
            .arg(&state)
            .args(["remote", "logout", "--json"])
            .record_output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{marker}: {output:?}");
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("artifactize workspace"),
            "{marker}: {error}"
        );
        assert!(!state.exists(), "{marker}: credential guard created state");
    }
}

#[test]
fn doctor_never_probes_or_creates_state_inside_current_or_legacy_workspaces() {
    for marker in [
        "index.artf",
        "artifactize.json",
        ".artifactizeignore",
        "file.txt.artf",
    ] {
        let root = support::os::tempdir();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join(marker), "workspace marker").unwrap();
        let state = workspace.join("state");
        let report = result(&mut doctor(&state, root.path()), 1);
        assert_eq!(
            check(&report, "state")["status"],
            "FAIL",
            "{marker}: {report}"
        );
        assert!(
            check(&report, "state")["message"]
                .as_str()
                .unwrap()
                .contains("outside artifactize workspaces"),
            "{marker}: {report}"
        );
        assert!(!state.exists(), "{marker}: doctor created state");
    }
}

// The operator's standard macOS temp prefix is usable, but a symlink below it is not.
#[cfg(target_os = "macos")]
#[test]
fn state_home_under_system_tmp_verifies_and_prunes_without_following_user_links() {
    let root = owned_run::Root::from(support::os::tempdir_below_tmp_alias());
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(
        repo.join("index.artf"),
        json!({"name":"a","fingerprint":false,"evals":[{
            "id":"check","title":"Check","payload":{"instruction":"Check"},
            "profile":{"kind":"runtime","command":true_program(),"args":[]}
        }]})
        .to_string(),
    )
    .unwrap();
    let invoke = |args: &[&str], code| {
        let mut command = Command::new(support::os::cli());
        command
            .arg("--repo")
            .arg(&repo)
            .arg("--json")
            .env("ARTIFACTIZE_STATE_HOME", &state)
            .args(args);
        result(&mut command, code)
    };
    root.track(&state, &repo);
    let run = invoke(&["verify", "--wait", "--all"], 0);
    assert_eq!(run["status"], "GREEN");
    assert_eq!(
        run["stateDir"],
        support::os::path_text(&support::os::canonical(&state))
    );
    // Noncached evidence satisfies its Run only; status correctly reports another review.
    assert_eq!(invoke(&["status"], 1)["evals"][0]["action"], "execute");
    // The retention boundary is explicit rather than relying on a freshly completed clock.
    run_completion(&state, &repo, &run, "2000-01-01T00:00:00Z");
    let dry = invoke(&["prune", "--dry-run", "--older-than", "0s"], 0);
    assert!(!dry["wouldRemove"].as_array().unwrap().is_empty());
    let removed = invoke(&["prune", "--older-than", "0s"], 0);
    assert_eq!(removed["removed"], dry["wouldRemove"]);
    assert!(repo.join("index.artf").is_file());
    let alias = root.path().join("alias");
    link_dir(&state, &alias);
    let mut command = Command::new(support::os::cli());
    command
        .arg("--json")
        .env("ARTIFACTIZE_STATE_HOME", alias)
        .arg("prune");
    assert!(
        result(&mut command, 2)["error"]
            .as_str()
            .unwrap()
            .contains("symlink")
    );
}
