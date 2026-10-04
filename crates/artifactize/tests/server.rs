use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
};

use reqwest::StatusCode;
use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn admin(state: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--state-dir")
        .arg(state)
        .arg("server")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

struct Server {
    _root: tempfile::TempDir,
    state: PathBuf,
    child: Child,
    _stdout: BufReader<ChildStdout>,
    url: String,
    client: reqwest::Client,
}

impl Server {
    fn start() -> Self {
        Self::start_with(|_| {})
    }

    /// Start on a state directory that `prepare` may populate first.
    fn start_with(prepare: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("server");
        prepare(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--state-dir")
            .arg(&state)
            .args(["server", "run", "--listen", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let url = line
            .trim()
            .strip_prefix("Listening on ")
            .unwrap_or_else(|| panic!("{line}"))
            .to_owned();
        Self {
            _root: root,
            state,
            child,
            _stdout: stdout,
            url,
            client: reqwest::Client::new(),
        }
    }

    fn token(&self, name: &str, scopes: &str) -> String {
        let added = admin(&self.state, &["token", "add", name, "--scopes", scopes]);
        added["token"].as_str().unwrap().to_owned()
    }

    async fn whoami(&self, token: &str) -> (StatusCode, Value) {
        let response = self
            .client
            .get(format!("{}v1/whoami", self.url))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        (response.status(), response.json().await.unwrap())
    }

    async fn publish(&self, token: &str, fingerprint: &str, record: &Value) -> (StatusCode, Value) {
        let response = self
            .client
            .put(format!("{}v1/entries/{HASH}/{fingerprint}", self.url))
            .bearer_auth(token)
            .json(record)
            .send()
            .await
            .unwrap();
        (response.status(), response.json().await.unwrap())
    }

    async fn lookup(&self, token: &str, fingerprints: &[&str]) -> (StatusCode, Value) {
        self.lookup_as(token, "fingerprint", fingerprints).await
    }

    /// Look up keys named `field`; a 0.3 client names them `staleKey`.
    async fn lookup_as(
        &self,
        token: &str,
        field: &str,
        fingerprints: &[&str],
    ) -> (StatusCode, Value) {
        let keys: Vec<_> = fingerprints
            .iter()
            .map(|fingerprint| {
                let mut key = json!({"evalDefHash":HASH});
                key[field] = json!(fingerprint);
                key
            })
            .collect();
        let response = self
            .client
            .post(format!("{}v1/lookup", self.url))
            .bearer_auth(token)
            .json(&json!({ "keys": keys }))
            .send()
            .await
            .unwrap();
        (response.status(), response.json().await.unwrap())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn record(fingerprint: &str, kind: &str, verdict: &str) -> Value {
    json!({"schema":1,"fingerprint":fingerprint,"evalDefHash":HASH,"verdict":verdict,
        "evalId":"app/check","runId":"run-a","requestId":"run-a-1","executionId":"execution-run-a-1",
        "profile":{"kind":kind},"result":{"verdict":verdict},"usage":null,
        "producer":{"name":"alice@laptop","version":"0.1.0"},
        "startedAt":"2026-10-04T00:00:00Z","completedAt":"2026-10-04T00:00:01Z",
        "publisher":"mallory"})
}

#[tokio::test]
async fn first_writer_wins_scopes_are_enforced_and_purge_removes_a_publisher() {
    let server = Server::start();
    let reader = server.token("ci", "read");
    let alice = server.token("alice-laptop", "read,publish");
    let bob = server.token("bob-laptop", "publish");
    let signer = server.token("alice-signoff", "read,publish,human");

    assert_eq!(
        server.whoami(&alice).await,
        (
            StatusCode::OK,
            json!({"principal":"alice-laptop","scopes":["read","publish"]})
        )
    );
    assert_eq!(
        server.whoami("azt_unknown").await.0,
        StatusCode::UNAUTHORIZED
    );
    let anonymous = server
        .client
        .get(format!("{}v1/whoami", server.url))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(anonymous.headers()["www-authenticate"], "Bearer");

    assert_eq!(
        server
            .publish(&alice, "app-v1", &record("app-v1", "runtime", "GREEN"))
            .await,
        (StatusCode::CREATED, json!({"created":true}))
    );
    assert_eq!(
        server
            .publish(&bob, "app-v1", &record("app-v1", "runtime", "RED"))
            .await,
        (StatusCode::OK, json!({"created":false}))
    );
    assert_eq!(
        server
            .publish(&reader, "app-v2", &record("app-v2", "runtime", "GREEN"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        server.lookup(&bob, &["app-v1"]).await.0,
        StatusCode::FORBIDDEN
    );
    let (status, found) = server.lookup(&reader, &["app-v1", "missing"]).await;
    assert_eq!(status, StatusCode::OK);
    let entries = found["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["verdict"], "GREEN");
    assert_eq!(entries[0]["publisher"], "alice-laptop");
    assert!(entries[0]["publishedAt"].is_string());

    let (status, error) = server
        .publish(&alice, "sign-v1", &record("sign-v1", "human", "GREEN"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    assert_eq!(
        server
            .publish(&signer, "sign-v1", &record("sign-v1", "human", "GREEN"))
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        server
            .publish(&alice, "other", &record("app-v3", "runtime", "GREEN"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut large = record("app-v4", "agent", "GREEN");
    large["result"]["notes"] = json!("x".repeat(300 * 1024));
    assert_eq!(
        server.publish(&alice, "app-v4", &large).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );

    let tokens = admin(&server.state, &["token", "list"]);
    assert!(!tokens.to_string().contains("azt_"));
    let stored: String = Connection::open(server.state.join("review-store.sqlite"))
        .unwrap()
        .query_row("SELECT hash FROM tokens WHERE name='ci'", [], |row| {
            row.get(0)
        })
        .unwrap();
    let digest: String = Sha256::digest(reader.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(stored, digest);

    let revoked = admin(
        &server.state,
        &["token", "revoke", "alice-laptop", "--purge"],
    );
    assert_eq!(revoked["purgedEntries"], 1);
    assert_eq!(server.whoami(&alice).await.0, StatusCode::UNAUTHORIZED);
    let (_, found) = server.lookup(&reader, &["app-v1", "sign-v1"]).await;
    let entries = found["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["publisher"], "alice-signoff");
    let listed = admin(&server.state, &["token", "list"]);
    assert!(listed[0]["revokedAt"].is_string() && listed[0]["name"] == "alice-laptop");

    assert_eq!(
        admin(&server.state, &["rm", "sign-v1"]),
        json!({"removed":true})
    );
    let (_, found) = server.lookup(&reader, &["sign-v1"]).await;
    assert_eq!(found, json!({"entries":[]}));
}

/// A 0.3 client's record (summary or full) with `staleKey` in place of `fingerprint`.
fn legacy(mut record: Value, full: bool) -> Value {
    let object = record.as_object_mut().unwrap();
    let fingerprint = object.remove("fingerprint").unwrap();
    if full {
        object.insert(
            "execution".into(),
            json!({"id":"execution-run-a-1","staleKey":fingerprint,"status":"GREEN"}),
        );
    }
    object.insert("staleKey".into(), fingerprint);
    record
}

fn stored(state: &Path) -> Vec<(String, i64, String)> {
    let db = Connection::open(state.join("review-store.sqlite")).unwrap();
    let mut statement = db
        .prepare("SELECT fingerprint,bytes,data FROM entries ORDER BY fingerprint")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test]
async fn records_and_lookups_from_0_3_clients_keep_working() {
    let server = Server::start();
    let alice = server.token("alice-laptop", "read,publish");
    for (fingerprint, full) in [("app-v1", false), ("app-v2", true)] {
        let record = legacy(record(fingerprint, "runtime", "GREEN"), full);
        assert_eq!(
            server.publish(&alice, fingerprint, &record).await,
            (StatusCode::CREATED, json!({"created":true}))
        );
    }
    assert_eq!(
        server
            .publish(
                &alice,
                "other",
                &legacy(record("app-v3", "runtime", "GREEN"), false)
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // Records are stored under the current name, whichever name the client sent.
    let entries = stored(&server.state);
    assert_eq!(entries.len(), 2);
    for (fingerprint, bytes, data) in &entries {
        let record: Value = serde_json::from_str(data).unwrap();
        assert_eq!(record["fingerprint"], fingerprint.as_str());
        assert_eq!(*bytes, data.len() as i64);
        assert!(!data.contains("staleKey"), "{data}");
    }

    // A 0.4 lookup gets the current shape.
    let (status, found) = server.lookup(&alice, &["app-v1", "app-v2"]).await;
    assert_eq!(status, StatusCode::OK);
    let entries = found["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry.get("staleKey").is_none()));
    let full = entries
        .iter()
        .find(|entry| entry["fingerprint"] == "app-v2")
        .unwrap();
    assert_eq!(full["execution"]["fingerprint"], "app-v2");
    assert!(full["execution"].get("staleKey").is_none());

    // A 0.3 lookup names its keys staleKey and gets the 0.3 shape back.
    let (status, found) = server
        .lookup_as(&alice, "staleKey", &["app-v1", "app-v2", "missing"])
        .await;
    assert_eq!(status, StatusCode::OK);
    let entries = found["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert!(
        entries
            .iter()
            .all(|entry| entry.get("fingerprint").is_none())
    );
    assert_eq!(entries[0]["staleKey"], "app-v1");
    assert_eq!(entries[0]["publisher"], "alice-laptop");
    assert_eq!(entries[1]["staleKey"], "app-v2");
    assert_eq!(entries[1]["execution"]["staleKey"], "app-v2");
    assert!(entries[1]["execution"].get("fingerprint").is_none());
}

#[tokio::test]
async fn a_version_one_review_store_upgrades_in_place() {
    let token = "azt_version-one-reader";
    let digest: String = Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let summary = legacy(record("app-v1", "runtime", "GREEN"), false);
    let full = legacy(record("app-v2", "runtime", "RED"), true);
    let server = Server::start_with(|state| {
        fs::create_dir_all(state).unwrap();
        let db = Connection::open(state.join("review-store.sqlite")).unwrap();
        // The schema version 1 layout written by artifactize 0.3.
        db.execute_batch(
            "PRAGMA user_version=1;
            CREATE TABLE tokens(name TEXT PRIMARY KEY, hash TEXT NOT NULL UNIQUE, scopes TEXT NOT NULL, created_at TEXT NOT NULL, revoked_at TEXT);
            CREATE TABLE entries(eval_def_hash TEXT NOT NULL, stale_key TEXT NOT NULL, publisher TEXT NOT NULL, bytes INTEGER NOT NULL, last_used TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(eval_def_hash, stale_key));
            CREATE INDEX entries_lru ON entries(last_used, eval_def_hash, stale_key);
            CREATE INDEX entries_publisher ON entries(publisher);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO tokens VALUES('ci',?,'[\"read\"]','2026-10-04T00:00:00Z',NULL)",
            [&digest],
        )
        .unwrap();
        for record in [&summary, &full] {
            let data = record.to_string();
            db.execute(
                "INSERT INTO entries VALUES(?,?,'alice-laptop',?,'2026-10-04T00:00:02Z',?)",
                rusqlite::params![HASH, record["staleKey"].as_str(), data.len() as i64, data],
            )
            .unwrap();
        }
    });
    let db = Connection::open(server.state.join("review-store.sqlite")).unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        2
    );
    let schema: Vec<String> = db
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!schema.join("\n").contains("stale"), "{schema:?}");
    let entries = stored(&server.state);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.0.as_str())
            .collect::<Vec<_>>(),
        ["app-v1", "app-v2"]
    );
    for (fingerprint, bytes, data) in &entries {
        let record: Value = serde_json::from_str(data).unwrap();
        assert_eq!(record["fingerprint"], fingerprint.as_str());
        assert_eq!(*bytes, data.len() as i64);
        assert!(!data.contains("staleKey"), "{data}");
    }

    let (status, found) = server.lookup(token, &["app-v1", "app-v2"]).await;
    assert_eq!(status, StatusCode::OK);
    let mut expected = [summary, full];
    for record in &mut expected {
        let object = record.as_object_mut().unwrap();
        let fingerprint = object.remove("staleKey").unwrap();
        if let Some(execution) = object.get_mut("execution") {
            let execution = execution.as_object_mut().unwrap();
            execution.remove("staleKey");
            execution.insert("fingerprint".into(), fingerprint.clone());
        }
        object.insert("fingerprint".into(), fingerprint);
    }
    assert_eq!(found, json!({"entries": expected}));
    assert_eq!(
        admin(&server.state, &["rm", "app-v2"]),
        json!({"removed":true})
    );
}
