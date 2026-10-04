//! A version 1 state database, written before the staleKey rename, upgrades in place.

use std::{fs, path::Path, process::Command};

use rusqlite::Connection;
use serde_json::{Value, json};

/// Recorded by the version 1 binary: app (script key) and notes (content key) are GREEN and
/// cached; doc (script key, Human) is WAITING_HUMAN.
const VERSION_ONE: &str = include_str!("fixtures/state-v1.sql");
const RUN: &str = "run-3aNTur";
const WAITING: &str = "run-3aNTur-2";

fn json(root: &Path, args: &[&str], code: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .env("USER", "alice")
        .arg("--repo")
        .arg(root.join("repo"))
        .arg("--state-dir")
        .arg(root.join("state"))
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
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
fn version_one_state_upgrades_and_keeps_reuse_and_waiting_human_reviews() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    // The recorded Artifacts, now declared with staleKey.
    let script = json!({"script":{"command":"cat","args":["key"]}});
    let runtime = json!({"kind":"runtime","command":"true","args":[]});
    for (name, key, stale_key, profile) in [
        ("app", "app-v1", script.clone(), runtime.clone()),
        ("notes", "notes-v1", json!({"content":{}}), runtime),
        ("doc", "doc-v1", script, json!({"kind":"human"})),
    ] {
        let folder = root.join("repo").join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("key"), key).unwrap();
        let declaration = json!({"name":name,"staleKey":stale_key,"evals":[{
            "id":"check","title":"Check","profile":profile,"payload":{"instruction":"Check."},
            "passSchema":{"type":"object","properties":{"approved":{"const":true}},"required":["approved"],"additionalProperties":false}
        }]});
        fs::write(folder.join("artifactize.json"), declaration.to_string()).unwrap();
    }
    fs::create_dir_all(root.join("state")).unwrap();
    let database = root.join("state/state.sqlite");
    Connection::open(&database)
        .unwrap()
        .execute_batch(&VERSION_ONE.replace("@ROOT@", root.to_str().unwrap()))
        .unwrap();

    // A read-only command upgrades the schema before reading.
    let entries = json(&root, &["cache", "list"], 0);
    let keys: Vec<_> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["staleKey"].as_str().unwrap())
        .collect();
    assert_eq!(keys.len(), 2, "{entries}");
    assert!(keys.contains(&"app-v1") && keys.iter().any(|key| key.starts_with("content:")));
    let db = Connection::open(&database).unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        artifactize::store::STATE_SCHEMA_VERSION
    );
    for table in ["executions", "cache_entries"] {
        let columns = names(
            &db,
            &format!("SELECT name FROM pragma_table_info('{table}')"),
        );
        assert!(columns.contains(&"stale_key".to_owned()), "{columns:?}");
    }
    let indexes = names(
        &db,
        "SELECT name FROM sqlite_master WHERE type='index' AND sql IS NOT NULL",
    );
    assert!(
        indexes.contains(&"active_stale_key".to_owned()),
        "{indexes:?}"
    );
    let schema = names(&db, "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL").join("\n");
    assert!(!schema.contains("ident"), "{schema}");

    assert_eq!(
        json(&root, &["cache", "show", "app-v1"], 0)["staleKey"],
        "app-v1"
    );
    let saved = json(&root, &["run", "show", RUN], 0);
    assert_eq!(saved["requests"][0]["staleKey"], "app-v1");
    let validation = &saved["validation"]["artifacts"];
    assert_eq!(validation[0]["staleKey"], "app-v1");
    assert_eq!(validation[0]["staleKeyKind"], "script");
    assert_eq!(validation[2]["staleKeyKind"], "content");
    let definitions = &saved["definitions"]["artifacts"];
    assert_eq!(
        definitions["app"]["staleKey"],
        json!({"script":{"command":"cat","args":["key"],"inputs":[],"timeoutMs":null}})
    );
    assert_eq!(
        definitions["notes"]["staleKey"],
        json!({"content":{"inputs":["."],"dependencies":"direct","ignore":[]}})
    );

    // The recorded Human scope still matches the current declarations.
    json(&root, &["request", "claim", WAITING], 0);
    let submitted = json(
        &root,
        &[
            "request",
            "submit",
            WAITING,
            "--verdict",
            "GREEN",
            "--fields",
            r#"{"approved":true}"#,
        ],
        0,
    );
    assert_eq!(submitted["status"], "GREEN");
    // Both version 1 cache entries and the submitted Human result are reused.
    let next = json(&root, &["verify", "--all"], 0);
    assert_eq!(next["executionsStarted"], 0, "{next}");
    assert!(
        next["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|request| request["provenance"]["runId"] == RUN),
        "{next}"
    );
}
