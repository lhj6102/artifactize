//! State databases of versions 1 to 3 (artifactize 0.1 to 0.4) upgrade in place to version 4.
//! Their reuse mappings cannot be mapped to the 0.5 reuse key, so they are dropped and the
//! first verify after the upgrade reviews again, once; Runs and waiting Human requests survive.

use std::{fs, path::Path, process::Command};

use rusqlite::Connection;
use serde_json::{Value, json};

mod support;

/// Recorded by the version 1 binary: app (script key) and notes (content key) are GREEN and
/// cached; doc (script key, Human) is WAITING_HUMAN.
const VERSION_ONE: &str = include_str!("fixtures/state-v1.sql");
/// Recorded by the version 2 binary: app (script key with inputs and a timeout) and notes
/// (content key with inputs, no dependencies and an ignore glob) are GREEN and cached; doc
/// (script key, Human, mounting and referencing notes) is WAITING_HUMAN.
const VERSION_TWO: &str = include_str!("fixtures/state-v2.sql");
/// Recorded by artifactize 0.4.0: app (script fingerprint with files and a timeout) and notes
/// (content fingerprint with files, direct dependencies and an ignore glob) are GREEN and
/// cached; doc (script fingerprint, Human, mounting and referencing notes) is WAITING_HUMAN.
const VERSION_THREE: &str = include_str!("fixtures/state-v3.sql");

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
        // The recorded declarations run `true` and `cat` from PATH.
        .env("PATH", support::os::path())
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

fn upgrades_keeps_history_and_reviews_again_once(recorded: Recorded) {
    let temp = tempfile::tempdir().unwrap();
    let root = support::os::canonical(temp.path());
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
        .execute_batch(&support::recorded_state(recorded.sql, &root))
        .unwrap();

    // doctor reports the older schema without changing the file.
    let current = artifactize::store::STATE_SCHEMA_VERSION;
    assert_eq!(current, 4);
    let recorded_bytes = fs::read(&database).unwrap();
    let schema = doctor(&root);
    assert_eq!(schema["status"], "PASS", "{schema}");
    assert_eq!(
        schema["details"],
        json!({"schema":recorded.version,"upgradeTo":current})
    );
    assert_eq!(fs::read(&database).unwrap(), recorded_bytes);

    // A read-only command upgrades the schema before reading. The earlier reuse mappings
    // cannot be mapped to the new key and are gone.
    assert_eq!(json(&root, &["cache", "list"], 0), json!([]));
    assert_eq!(doctor(&root)["details"], json!({"schema":current}));
    let db = Connection::open(&database).unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        current
    );
    let columns = |table: &str| {
        names(
            &db,
            &format!("SELECT name FROM pragma_table_info('{table}')"),
        )
    };
    assert!(columns("executions").contains(&"key".to_owned()));
    assert!(!columns("executions").contains(&"fingerprint".to_owned()));
    assert!(!columns("executions").contains(&"eval_def_hash".to_owned()));
    assert!(columns("cache_entries").contains(&"key".to_owned()));
    let indexes = names(
        &db,
        "SELECT name FROM sqlite_master WHERE type='index' AND sql IS NOT NULL",
    );
    assert!(indexes.contains(&"active_key".to_owned()), "{indexes:?}");
    let schema = names(&db, "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL").join("\n");
    assert!(
        !schema.contains("ident") && !schema.contains("stale") && !schema.contains("fingerprint"),
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
        names(&db, "SELECT CAST(count(*) AS TEXT) FROM executions"),
        ["3"]
    );

    // Runs stay readable, with their saved declarations in the current shape.
    let saved = json(&root, &["run", "show", recorded.run], 0);
    assert_eq!(saved["requests"][0]["fingerprint"], app_key);
    assert_eq!(saved["requests"][0]["status"], "GREEN");
    let validation = &saved["validation"]["artifacts"];
    assert_eq!(validation[0]["fingerprint"], app_key);
    assert_eq!(validation[0]["fingerprintKind"], "script");
    assert_eq!(validation[2]["fingerprintKind"], "content");
    let definitions = &saved["definitions"]["artifacts"];
    assert_eq!(definitions["app"]["fingerprint"], recorded.definitions.0);
    assert_eq!(definitions["notes"]["fingerprint"], recorded.definitions.1);

    // Every eval reviews again: the key is built differently.
    let status = json(&root, &["status", "--all"], 1);
    let counts = &status["counts"];
    assert_eq!(counts["reuse"], 0, "{status}");
    // doc waits for notes where it names it.
    assert_eq!(
        counts["execute"].as_u64().unwrap() + counts["wait"].as_u64().unwrap(),
        3,
        "{status}"
    );

    // The recorded Human request still matches the current declarations and settles its Run.
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
    assert_eq!(json(&root, &["cache", "list"], 0), json!([]));

    // The first verify reviews again and asks for a new sign-off; the next one reuses.
    let next = json(&root, &["verify", "--all"], 4);
    assert_eq!(next["executionsStarted"], 2, "{next}");
    let again = json(&root, &["verify", "--all"], 4);
    assert_eq!(again["executionsStarted"], 0, "{again}");
    for request in again["requests"].as_array().unwrap() {
        if request["evalId"] == "doc/check" {
            assert_eq!(request["status"], "WAITING_HUMAN");
        } else {
            assert_eq!(request["provenance"]["runId"], next["id"], "{request}");
        }
    }
}

#[test]
fn version_one_state_upgrades_and_reviews_again_once() {
    let script = json!({"script":{"command":"cat","args":["key"]}});
    upgrades_keeps_history_and_reviews_again_once(Recorded {
        sql: VERSION_ONE,
        version: 1,
        run: "run-3aNTur",
        waiting: "run-3aNTur-2",
        app: ("app-v1", script.clone()),
        notes: ("notes-v1", json!({})),
        doc: ("doc-v1", script, json!({}), "Check."),
        definitions: (
            json!({"script":{"command":"cat","args":["key"],"files":[],"timeoutMs":null}}),
            json!({"files":["."],"ignore":[]}),
        ),
    });
}

#[test]
fn version_two_state_upgrades_and_reviews_again_once() {
    let app = json!({"script":{"command":"cat","args":["key"],"files":["key"],"timeoutMs":5000}});
    let notes = json!({"files":["key"],"ignore":["*.log"]});
    upgrades_keeps_history_and_reviews_again_once(Recorded {
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

#[test]
fn version_three_state_upgrades_and_reviews_again_once() {
    let app = json!({"script":{"command":"cat","args":["key"],"files":["key"],"timeoutMs":5000}});
    let notes = json!({"files":["key"],"ignore":["*.log"]});
    upgrades_keeps_history_and_reviews_again_once(Recorded {
        sql: VERSION_THREE,
        version: 3,
        run: "run-9c3gzy",
        waiting: "run-9c3gzy-2",
        app: ("app-v3", app.clone()),
        notes: ("notes-v3", notes.clone()),
        doc: (
            "doc-v3",
            json!({"script":{"command":"cat","args":["key"]}}),
            json!({"notes":"notes"}),
            "Check {notes}.",
        ),
        definitions: (app, notes),
    });
}
