//! A state of another schema version is refused, never migrated: 0.6.0 starts a new state.

use std::{fs, path::Path, process::Command};

use artifactize::store::{EARLIER_STATE, STATE_SCHEMA_VERSION};
use rusqlite::Connection;
use serde_json::{Value, json};

/// Some of the tables of a 0.5 state, before the four of 0.6.
const EARLIER: &str = "CREATE TABLE runs(id TEXT PRIMARY KEY, repo TEXT NOT NULL, status TEXT NOT NULL, data TEXT NOT NULL);
    CREATE TABLE run_members(run_id TEXT NOT NULL, eval_id TEXT NOT NULL, ordinal INTEGER NOT NULL, request_id TEXT NOT NULL);
    CREATE TABLE cache_entries(execution_id TEXT PRIMARY KEY, key TEXT NOT NULL);";

fn artifactize(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .env("ARTIFACTIZE_REMOTE", "off")
        .env("HOME", root.join("home"))
        .arg("--repo")
        .arg(root.join("repo"))
        .arg("--state-dir")
        .arg(root.join("state"))
        .args(args)
        .output()
        .unwrap()
}

fn names(db: &Connection, query: &str) -> Vec<String> {
    db.prepare(query)
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn an_earlier_state_is_refused_and_left_as_it_is() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(root.join("home")).unwrap();
    fs::write(
        repo.join("artifactize.json"),
        json!({"name":"app","evals":[{"id":"check","title":"Check",
            "profile":{"kind":"runtime","command":"true","args":[]},"payload":{"instruction":"Check."}}]})
        .to_string(),
    )
    .unwrap();
    let state = root.join("state");
    let database = state.join("state.sqlite");
    for version in 1..STATE_SCHEMA_VERSION {
        fs::create_dir_all(&state).unwrap();
        Connection::open(&database)
            .unwrap()
            .execute_batch(&format!("{EARLIER} PRAGMA user_version={version};"))
            .unwrap();
        let before = fs::read(&database).unwrap();
        for args in [
            &["verify", "--all"][..],
            &["status", "--all"],
            &["run", "list"],
            &["request", "list"],
            &["cache", "list"],
            &["session", "show", "missing"],
            &["prune"],
        ] {
            let output = artifactize(root, args);
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(stderr.trim_end(), EARLIER_STATE, "{args:?}");
        }
        let output = artifactize(root, &["verify", "--all", "--json"]);
        assert_eq!(output.status.code(), Some(2));
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error, json!({"error":EARLIER_STATE}));
        // doctor reports it.
        let output = artifactize(root, &["doctor", "--json"]);
        assert_eq!(output.status.code(), Some(1));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        let schema = report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["name"] == "schema")
            .unwrap();
        assert_eq!(schema["status"], "FAIL");
        assert_eq!(schema["message"], EARLIER_STATE);
        assert_eq!(
            schema["details"],
            json!({"schema":version,"supported":STATE_SCHEMA_VERSION})
        );
        assert_eq!(fs::read(&database).unwrap(), before, "version {version}");
        assert!(!state.join("state.sqlite-wal").exists());
        fs::remove_dir_all(&state).unwrap();
    }

    // A new state has the four tables, and works.
    let output = artifactize(root, &["verify", "--all", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let db = Connection::open(&database).unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        STATE_SCHEMA_VERSION
    );
    assert_eq!(
        names(
            &db,
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
        ),
        ["executions", "requests", "runs", "state_meta"]
    );
    let columns = |table: &str| {
        names(
            &db,
            &format!("SELECT name FROM pragma_table_info('{table}')"),
        )
    };
    assert_eq!(
        columns("requests"),
        [
            "id",
            "run_id",
            "eval_id",
            "ordinal",
            "execution_id",
            "status",
            "claimed_by",
            "claimed_at",
            "data"
        ]
    );
    assert_eq!(
        columns("executions"),
        [
            "id",
            "key",
            "eval_def_hash",
            "status",
            "owner_pid",
            "owner_start_time",
            "backend",
            "completed_at",
            "bytes",
            "last_used",
            "data"
        ]
    );
    let output = artifactize(root, &["doctor", "--json"]);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "schema" && check["status"] == "PASS")
    );
}
