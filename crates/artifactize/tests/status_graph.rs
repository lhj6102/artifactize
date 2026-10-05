use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use rusqlite::Connection;
use serde_json::{Value, json};
use support::os::bin;
use tempfile::TempDir;

mod support;

struct Fixture {
    root: TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        support::copy_fixture(name, &repo);
        Self {
            repo,
            state: root.path().join("state"),
            root,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state);
        command
    }

    fn output(&self, args: &[&str], code: i32) -> Output {
        let output = self.command().args(args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let mut args = args.to_vec();
        args.push("--json");
        let output = self.output(&args, code);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        // verify announces its Run on stderr; nothing else is printed there.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let expected = match value["id"].as_str() {
            Some(id) if args[0] == "verify" => format!("Run: {id}\n"),
            _ => String::new(),
        };
        assert_eq!(stderr, expected);
        value
    }
}

fn copy_directory(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let to = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}

fn row<'a>(view: &'a Value, kind: &str, id: &str) -> &'a Value {
    view[kind]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap()
}

#[test]
fn runtime_status_separates_current_validation_from_saved_attempts() {
    let fixture = Fixture::new("runtime");
    let fresh = fixture.json(&["status"], 1);
    assert_eq!(fresh["selection"], json!({"kind":"all"}));
    assert_eq!(row(&fresh, "artifacts", "input")["state"], "BASIS");
    assert_eq!(row(&fresh, "evals", "green/check")["state"], "UNREVIEWED");
    assert_eq!(row(&fresh, "evals", "green/check")["action"], "execute");
    assert_eq!(
        row(&fresh, "evals", "blocked/check")["state"],
        "WAIT_DEPENDENCY"
    );
    assert_eq!(row(&fresh, "evals", "blocked/check")["action"], "wait");
    assert_eq!(
        row(&fresh, "evals", "blocked/check")["blockedBy"],
        json!(["red/check"])
    );
    assert_eq!(fresh["counts"]["execute"], 5);
    assert_eq!(fresh["counts"]["wait"], 2);
    let text = String::from_utf8(fixture.output(&["status"], 1).stdout).unwrap();
    assert!(text.contains("Verify actions: will execute 5, will reuse 0, wait 2, blocked 0"));
    assert!(text.contains("wait: needs a result verify has not produced yet"));
    assert!(!fixture.state.exists());
    let basis = fixture.json(&["status", "input"], 0);
    assert_eq!(basis["satisfied"], true);
    assert_eq!(basis["obligations"], json!([]));

    let run = fixture.json(&["verify", "--all"], 2);
    let view = fixture.json(&["status"], 1);
    assert_eq!(view["satisfied"], false);
    assert_eq!(view["counts"]["reuse"], 0);
    assert!(view.get("latestRun").is_none());
    for (id, verdict, state, action) in [
        ("green/check", "GREEN", "STALE", "execute"),
        ("red/check", "RED", "STALE", "execute"),
        ("blocked/check", "BLOCKED", "WAIT_DEPENDENCY", "wait"),
        ("timeout/check", "ERROR", "STALE", "execute"),
        (
            "waiting/check",
            "WAIT_DEPENDENCY",
            "WAIT_DEPENDENCY",
            "wait",
        ),
    ] {
        let eval = row(&view, "evals", id);
        assert_eq!(eval["last"], json!({"runId":run["id"],"verdict":verdict}));
        assert_eq!(eval["state"], state);
        assert_eq!(eval["action"], action);
    }
    let rerun = fixture.json(&["verify", "green"], 0);
    assert_eq!(rerun["validation"]["satisfied"], true);
    let later = fixture.json(&["status"], 1);
    assert_eq!(
        row(&later, "evals", "green/check")["last"]["runId"],
        rerun["id"]
    );
    assert_eq!(
        row(&later, "evals", "red/check")["last"]["runId"],
        run["id"]
    );
    let text = String::from_utf8(fixture.output(&["status", "green"], 1).stdout).unwrap();
    assert!(text.contains("Current validation: NOT SATISFIED"));
    assert!(text.contains("green/check: STALE — execute"));
    assert!(text.contains("Last: GREEN"));
    assert!(text.contains("historical, not current evidence"));
}

#[test]
fn status_selection_policy_and_final_obligations_match_verify() {
    let fixture = Fixture::new("runtime");
    let selected = fixture.json(&["status", "blocked"], 1);
    assert_eq!(selected["selectedEvalIds"], json!(["blocked/check"]));
    assert_eq!(selected["includedEvalIds"], json!(["blocked/check"]));
    assert_eq!(row(&selected, "evals", "red/check")["included"], false);
    let recursive = fixture.json(&["status", "blocked", "--recursive", "--force"], 1);
    assert_eq!(
        recursive["includedEvalIds"],
        json!(["blocked/check", "red/check"])
    );
    assert_eq!(row(&recursive, "evals", "blocked/check")["force"], true);
    assert_eq!(row(&recursive, "evals", "blocked/check")["action"], "wait");
    assert_eq!(row(&recursive, "evals", "red/check")["force"], false);
    let ignored = fixture.json(&["status", "blocked", "--ignore-gates"], 1);
    assert_eq!(row(&ignored, "evals", "blocked/check")["action"], "execute");
    assert_eq!(ignored["obligations"], json!(["blocked", "red"]));

    fs::write(
        fixture.repo.join("artifactize.json"),
        r#"{"name":"project","basis":true,"reviewPolicy":{"dependencyGates":"ignore"}}"#,
    )
    .unwrap();
    let configured = fixture.json(&["status", "blocked"], 1);
    assert_eq!(configured["ignoreGates"], true);
    assert_eq!(
        row(&configured, "evals", "blocked/check")["action"],
        "execute"
    );
    let incomplete = fixture.json(&["status", "project"], 1);
    assert_eq!(
        row(&incomplete, "artifacts", "project")["state"],
        "INCOMPLETE"
    );
    assert!(
        !incomplete["obligations"]
            .as_array()
            .unwrap()
            .contains(&json!("project"))
    );

    let selector = fixture.root.path().join("selection.json");
    fs::write(&selector, r#"["red/check","green/check","red/check"]"#).unwrap();
    let file = fixture.json(&["status", "--evals-file", selector.to_str().unwrap()], 1);
    let csv = fixture.json(&["status", "--evals", "red/check,green/check"], 1);
    assert_eq!(file, csv);
    for args in [
        vec!["status", "red", "--all"],
        vec!["status", "--eval", "red/check", "--evals", "red/check"],
        vec!["status", "missing"],
        vec!["status", "--full"],
        vec!["config", "graph", "--compact"],
        vec!["config", "graph", "missing"],
        vec!["verify"],
    ] {
        fixture.json(&args, 2);
    }
    assert!(!fixture.state.exists());
}

#[test]
fn profiles_rebuild_static_gates_without_running_commands() {
    let fixture = Fixture::new("runtime");
    let path = fixture.repo.join("red/artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    declaration["evals"][0]["profileVariants"] = json!({"dependent":{
        "kind":"runtime","command":"missing-command","args":["{green}/missing.txt"]
    }});
    fs::write(&path, declaration.to_string()).unwrap();
    let view = fixture.json(&["status", "red", "--profile", "dependent"], 1);
    assert_eq!(row(&view, "evals", "red/check")["state"], "WAIT_DEPENDENCY");
    assert_eq!(
        row(&view, "evals", "red/check")["blockedBy"],
        json!(["green/check"])
    );
    assert_eq!(
        row(&view, "evals", "red/check")["profile"]["command"],
        "missing-command"
    );
    fixture.json(&["status", "red", "--profile", "unknown"], 2);
    assert!(!fixture.state.exists());
}

#[test]
fn families_keep_full_definitions_grouping_and_last_run_pointers() {
    let fixture = Fixture::new("families");
    let unreviewed = fixture.json(&["status", "scenarios"], 1);
    assert_eq!(
        unreviewed["selectedEvalIds"],
        json!(["checkout/review", "search/review"])
    );
    assert_eq!(
        row(&unreviewed, "artifacts", "checkout")["family"],
        "scenarios"
    );
    assert_eq!(
        row(&unreviewed, "artifacts", "search")["state"],
        "UNREVIEWED"
    );
    let run = fixture.json(&["verify", "scenarios"], 0);
    assert_eq!(run["validation"]["satisfied"], true);
    let view = fixture.json(&["status", "scenarios"], 0);
    assert_eq!(view["counts"]["reuse"], 2);
    for (id, fingerprint) in [
        ("checkout/review", "checkout:READY"),
        ("search/review", "search:SEARCH"),
    ] {
        assert_eq!(row(&view, "evals", id)["state"], "PASS");
        assert_eq!(row(&view, "evals", id)["action"], "reuse");
        assert_eq!(
            row(&view, "evals", id)["last"],
            json!({"runId":run["id"],"verdict":"GREEN","fingerprint":fingerprint})
        );
    }
    fs::write(
        fixture.repo.join("scenarios/fingerprint.sh"),
        "#!/bin/sh\ntouch fingerprint-ran\nexit 91\n",
    )
    .unwrap();
    let error = fixture.json(&["status", "scenarios"], 2);
    assert!(error["error"].as_str().unwrap().contains("exited with"));
    assert!(fixture.repo.join("scenarios/fingerprint-ran").exists());
    fs::remove_file(fixture.repo.join("scenarios/fingerprint-ran")).unwrap();
    fixture.json(&["config", "check"], 0);
    let graph = fixture.json(&["config", "graph", "scenarios"], 0);
    assert_eq!(
        graph["families"]["scenarios"],
        json!({"path":"scenarios","artifactIds":["checkout","search"]})
    );
    assert_eq!(graph["artifacts"].as_object().unwrap().len(), 2);
    assert_eq!(
        graph["artifacts"]["checkout"]["family"]["material"],
        json!(["checkout.txt"])
    );
    assert_eq!(
        graph["artifacts"]["search"]["views"]["agentTools"]["detail"]["inputSchema"]["properties"]
            ["id"]["enum"],
        json!(["message", "query"])
    );
    let text =
        String::from_utf8(fixture.output(&["config", "graph", "scenarios"], 0).stdout).unwrap();
    assert!(text.contains("Family scenarios: checkout, search"));
    assert!(!fixture.repo.join("scenarios/fingerprint-ran").exists());
}

#[test]
fn graph_projects_typed_edges_closure_and_dependency_first_cycles() {
    let fixture = Fixture::new("runtime");
    let path = fixture.repo.join("review/artifactize.json");
    let mut review: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    review["mounts"] = json!({"source":"input"});
    fs::write(path, review.to_string()).unwrap();
    fs::write(
        fixture.repo.join("artifactize.json"),
        r#"{"name":"project","basis":true}"#,
    )
    .unwrap();
    let graph = fixture.json(&["config", "graph"], 0);
    assert_eq!(graph["version"], 1);
    let relations = graph["relations"].as_array().unwrap();
    for (kind, field, value) in [
        ("child", "path", "review"),
        ("mount", "alias", "source"),
        ("instruction", "evalId", "cycle-a/check"),
        ("argv", "evalId", "green/check"),
    ] {
        assert!(
            relations
                .iter()
                .any(|edge| edge["kind"] == kind && edge[field] == value)
        );
    }
    let edge = relations
        .iter()
        .find(|edge| edge["kind"] == "argv")
        .unwrap();
    assert_eq!(edge["index"], 1);
    assert_eq!(edge["name"], "input");
    assert_eq!(edge["path"], "data.txt");
    assert_eq!(edge["source"], "input");
    assert_eq!(edge["target"], "green");
    assert_eq!(edge["cyclic"], false);
    let cycle = fixture.json(&["config", "graph", "cycle-a"], 0);
    assert_eq!(cycle["artifacts"].as_object().unwrap().len(), 2);
    assert_eq!(cycle["components"].as_array().unwrap().len(), 1);
    assert_eq!(
        cycle["components"][0]["artifacts"],
        json!(["cycle-a", "cycle-b"])
    );
    assert_eq!(cycle["components"][0]["cyclic"], true);
    assert!(
        cycle["relations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|edge| edge["cyclic"] == true)
    );
    for component in graph["components"].as_array().unwrap() {
        for dependency in component["dependencies"].as_array().unwrap() {
            assert!(dependency.as_u64().unwrap() < component["id"].as_u64().unwrap());
        }
    }
    let green = fixture.json(&["config", "graph", "green"], 0);
    assert_eq!(
        green["artifacts"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["green", "input"]
    );
    assert_eq!(
        green["evals"][0]["declaration"]["profile"]["command"],
        bin("/bin/sh")
    );
    let text =
        String::from_utf8(fixture.output(&["config", "graph", "cycle-a"], 0).stdout).unwrap();
    assert!(text.contains("[cycle]"));
    assert!(text.contains("cycle-b -> cycle-a [instruction"));
    assert!(!fixture.state.exists());
}

#[test]
fn static_commands_never_execute_hooks_and_status_only_runs_fingerprint() {
    let fixture = Fixture::new("declarations");
    let marker = fixture.root.path().join("hook-executed");
    let hook = fixture.repo.join("review/hook.sh");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\nprintf called > '{}'\nexit 91\n",
            marker.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fixture.json(&["config", "check"], 0);
    fixture.json(&["config", "graph"], 0);
    assert!(!marker.exists());
    assert!(!fixture.state.exists());
    let path = fixture.repo.join("review/artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    declaration["fingerprint"]["script"] =
        json!({"command":bin("/bin/echo"),"args":["current-fingerprint"]});
    fs::write(path, declaration.to_string()).unwrap();
    let view = fixture.json(&["status"], 1);
    assert_eq!(row(&view, "artifacts", "unreviewed")["state"], "UNREVIEWED");
    assert_eq!(row(&view, "evals", "review/agent")["action"], "execute");
    assert_eq!(row(&view, "evals", "review/human")["action"], "execute");
    assert_eq!(row(&view, "evals", "review/runtime")["action"], "execute");
    assert!(!marker.exists());
    assert!(!fixture.state.join("state.sqlite").exists());
    assert_eq!(fs::read_dir(&fixture.state).unwrap().count(), 0);
}

#[test]
fn status_reads_only_this_repository_and_preserves_database_and_run_output() {
    let fixture = Fixture::new("runtime");
    let first = fixture.json(&["verify", "green"], 0);
    let other = fixture.root.path().join("other");
    copy_directory(&fixture.repo, &other);
    let output = fixture
        .command()
        .arg("--json")
        .args(["verify", "red"])
        .current_dir(&other)
        .output()
        .unwrap();
    // --repo, not cwd, owns the stored audit.
    assert_eq!(output.status.code(), Some(1));
    let other_status = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--repo")
        .arg(&other)
        .arg("--state-dir")
        .arg(&fixture.state)
        .args(["status", "green", "--json"])
        .output()
        .unwrap();
    assert_eq!(other_status.status.code(), Some(1));
    let other_status: Value = serde_json::from_slice(&other_status.stdout).unwrap();
    assert!(
        row(&other_status, "evals", "green/check")
            .get("last")
            .is_none()
    );
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    let before: String = db
        .query_row("SELECT group_concat(data) FROM runs", [], |row| row.get(0))
        .unwrap();
    fixture.json(&["status"], 1);
    let after: String = db
        .query_row("SELECT group_concat(data) FROM runs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(before, after);
    assert!(
        fixture
            .state
            .join("runs")
            .join(first["id"].as_str().unwrap())
            .is_dir()
    );
    db.pragma_update(None, "user_version", 99).unwrap();
    assert!(
        fixture.json(&["status"], 2)["error"]
            .as_str()
            .unwrap()
            .contains("Unsupported state schema")
    );
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        99
    );
    let link = fixture.root.path().join("redirected-state");
    #[cfg(unix)]
    support::os::symlink_dir(&fixture.repo, &link).unwrap();
    #[cfg(windows)]
    support::os::junction(&fixture.repo, &link);
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--repo")
        .arg(&fixture.repo)
        .arg("--state-dir")
        .arg(link)
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).contains("outside the reviewed repository"));
}
