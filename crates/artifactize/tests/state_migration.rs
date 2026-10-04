//! Version 1 and version 2 state databases, written before the staleKey and fingerprint renames,
//! upgrade in place.

use std::{fs, path::Path, process::Command};

use rusqlite::Connection;
use serde_json::{Value, json};

/// Recorded by the version 1 binary: app (script key) and notes (content key) are GREEN and
/// cached; doc (script key, Human) is WAITING_HUMAN.
const VERSION_ONE: &str = include_str!("fixtures/state-v1.sql");
/// Recorded by the version 2 binary: app (script key with inputs and a timeout) and notes
/// (content key with inputs, no dependencies and an ignore glob) are GREEN and cached; doc
/// (script key, Human, mounting and referencing notes) is WAITING_HUMAN.
const VERSION_TWO: &str = include_str!("fixtures/state-v2.sql");

/// A recorded state and the same Artifacts declared in the current shape.
struct Recorded {
    sql: &'static str,
    version: u32,
    run: &'static str,
    waiting: &'static str,
    app: (&'static str, Value),
    notes: (&'static str, Value),
    doc: (&'static str, Value, Value, &'static str),
    /// The saved app and notes declarations after the upgrade.
    definitions: (Value, Value),
}

/// `doctor --json` without backends on PATH or API keys in the environment.
fn doctor(root: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .env("PATH", root)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ARTIFACTIZE_REMOTE")
        .env_remove("ARTIFACTIZE_REMOTE_TOKEN")
        .arg("--state-dir")
        .arg(root.join("state"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "schema")
        .unwrap()
        .clone()
}

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

fn upgrades_and_keeps_reuse_and_waiting_human_reviews(recorded: Recorded) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let runtime = json!({"kind":"runtime","command":"true","args":[]});
    let (app_key, app) = recorded.app;
    let (notes_key, notes) = recorded.notes;
    let (doc_key, doc, mounts, instruction) = recorded.doc;
    for (name, key, fingerprint, profile, mounts, instruction) in [
        ("app", app_key, app, runtime.clone(), json!({}), "Check."),
        ("notes", notes_key, notes, runtime, json!({}), "Check."),
        (
            "doc",
            doc_key,
            doc,
            json!({"kind":"human"}),
            mounts,
            instruction,
        ),
    ] {
        let folder = root.join("repo").join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("key"), key).unwrap();
        let declaration = json!({"name":name,"mounts":mounts,"fingerprint":fingerprint,"evals":[{
            "id":"check","title":"Check","profile":profile,"payload":{"instruction":instruction},
            "passSchema":{"type":"object","properties":{"approved":{"const":true}},"required":["approved"],"additionalProperties":false}
        }]});
        fs::write(folder.join("artifactize.json"), declaration.to_string()).unwrap();
    }
    fs::create_dir_all(root.join("state")).unwrap();
    let database = root.join("state/state.sqlite");
    Connection::open(&database)
        .unwrap()
        .execute_batch(&recorded.sql.replace("@ROOT@", root.to_str().unwrap()))
        .unwrap();

    // doctor reports the older schema without changing the file.
    let current = artifactize::store::STATE_SCHEMA_VERSION;
    let recorded_bytes = fs::read(&database).unwrap();
    let schema = doctor(&root);
    assert_eq!(schema["status"], "PASS", "{schema}");
    assert_eq!(
        schema["details"],
        json!({"schema":recorded.version,"upgradeTo":current})
    );
    assert_eq!(fs::read(&database).unwrap(), recorded_bytes);

    // A read-only command upgrades the schema before reading.
    let entries = json(&root, &["cache", "list"], 0);
    assert_eq!(doctor(&root)["details"], json!({"schema":current}));
    let keys: Vec<_> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["fingerprint"].as_str().unwrap())
        .collect();
    assert_eq!(keys.len(), 2, "{entries}");
    assert!(keys.contains(&app_key) && keys.iter().any(|key| key.starts_with("content:")));
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
        assert!(columns.contains(&"fingerprint".to_owned()), "{columns:?}");
    }
    let indexes = names(
        &db,
        "SELECT name FROM sqlite_master WHERE type='index' AND sql IS NOT NULL",
    );
    assert!(
        indexes.contains(&"active_fingerprint".to_owned()),
        "{indexes:?}"
    );
    let schema = names(&db, "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL").join("\n");
    assert!(
        !schema.contains("ident") && !schema.contains("stale"),
        "{schema}"
    );
    let saved = names(
        &db,
        "SELECT data FROM executions UNION ALL SELECT data FROM requests UNION ALL SELECT data FROM runs",
    )
    .join("\n");
    assert!(
        !saved.contains("\"identity\"") && !saved.contains("staleKey"),
        "{saved}"
    );

    assert_eq!(
        json(&root, &["cache", "show", app_key], 0)["fingerprint"],
        app_key
    );
    let saved = json(&root, &["run", "show", recorded.run], 0);
    assert_eq!(saved["requests"][0]["fingerprint"], app_key);
    let validation = &saved["validation"]["artifacts"];
    assert_eq!(validation[0]["fingerprint"], app_key);
    assert_eq!(validation[0]["fingerprintKind"], "script");
    assert_eq!(validation[2]["fingerprintKind"], "content");
    let definitions = &saved["definitions"]["artifacts"];
    assert_eq!(definitions["app"]["fingerprint"], recorded.definitions.0);
    assert_eq!(definitions["notes"]["fingerprint"], recorded.definitions.1);

    // Before the Human submission, status predicts reuse for both cached results.
    let status = json(&root, &["status", "--all"], 1);
    assert_eq!(status["counts"]["reuse"], 2, "{status}");
    assert_eq!(status["counts"]["execute"], 0, "{status}");

    // The recorded Human scope still matches the current declarations.
    json(&root, &["request", "claim", recorded.waiting], 0);
    let submitted = json(
        &root,
        &[
            "request",
            "submit",
            recorded.waiting,
            "--verdict",
            "GREEN",
            "--fields",
            r#"{"approved":true}"#,
        ],
        0,
    );
    assert_eq!(submitted["status"], "GREEN");
    // Both recorded cache entries and the submitted Human result are reused.
    let next = json(&root, &["verify", "--all"], 0);
    assert_eq!(next["executionsStarted"], 0, "{next}");
    assert!(
        next["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|request| request["provenance"]["runId"] == recorded.run),
        "{next}"
    );
}

#[test]
fn version_one_state_upgrades_and_keeps_reuse_and_waiting_human_reviews() {
    let script = json!({"script":{"command":"cat","args":["key"]}});
    upgrades_and_keeps_reuse_and_waiting_human_reviews(Recorded {
        sql: VERSION_ONE,
        version: 1,
        run: "run-3aNTur",
        waiting: "run-3aNTur-2",
        app: ("app-v1", script.clone()),
        notes: ("notes-v1", json!({})),
        doc: ("doc-v1", script, json!({}), "Check."),
        definitions: (
            json!({"script":{"command":"cat","args":["key"],"files":[],"timeoutMs":null}}),
            json!({"files":["."],"dependencies":"direct","ignore":[]}),
        ),
    });
}

#[test]
fn version_two_state_upgrades_and_keeps_reuse_and_waiting_human_reviews() {
    let app = json!({"script":{"command":"cat","args":["key"],"files":["key"],"timeoutMs":5000}});
    let notes = json!({"files":["key"],"dependencies":"none","ignore":["*.log"]});
    upgrades_and_keeps_reuse_and_waiting_human_reviews(Recorded {
        sql: VERSION_TWO,
        version: 2,
        run: "run-CTxYkO",
        waiting: "run-CTxYkO-2",
        app: ("app-v2", app.clone()),
        notes: ("notes-v2", notes.clone()),
        doc: (
            "doc-v2",
            json!({"script":{"command":"cat","args":["key"]}}),
            json!({"notes":"notes"}),
            "Check {notes}.",
        ),
        definitions: (app, notes),
    });
}
