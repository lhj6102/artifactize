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

    async fn publish(&self, token: &str, key: &str, record: &Value) -> (StatusCode, Value) {
        let response = self
            .client
            .put(format!("{}v1/entries/{key}", self.url))
            .bearer_auth(token)
            .json(record)
            .send()
            .await
            .unwrap();
        (response.status(), response.json().await.unwrap())
    }

    async fn lookup(&self, token: &str, keys: &[&str]) -> (StatusCode, Value) {
        self.lookup_body(token, json!({ "keys": keys })).await
    }

    async fn lookup_body(&self, token: &str, body: Value) -> (StatusCode, Value) {
        let response = self
            .client
            .post(format!("{}v1/lookup", self.url))
            .bearer_auth(token)
            .json(&body)
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

/// A reuse key: 64 lowercase hex digits.
fn key(seed: u8) -> String {
    format!("{seed:02x}").repeat(32)
}

fn record(key: &str, kind: &str, verdict: &str) -> Value {
    json!({
        "schema":2,
        "key":key,
        "evalDefHash":HASH,
        "fingerprints":{"app":"app-v1"},
        "verdict":verdict,
        "evalId":"app/check",
        "runId":"run-a",
        "requestId":"run-a-1",
        "executionId":"execution-run-a-1",
        "profile":{"kind":kind},
        "options":{},
        "result":{"verdict":verdict},
        "usage":null,
        "producer":{"name":"alice@laptop","version":"0.5.0"},
        "startedAt":"2026-10-04T00:00:00Z",
        "completedAt":"2026-10-04T00:00:01Z",
        "publisher":"mallory",
    })
}

/// Another execution of the same key, completed at `completed_at`.
fn another(mut record: Value, execution: &str, verdict: &str, completed_at: &str) -> Value {
    record["executionId"] = json!(execution);
    record["verdict"] = json!(verdict);
    record["result"]["verdict"] = json!(verdict);
    record["completedAt"] = json!(completed_at);
    record
}

#[tokio::test]
async fn records_append_per_key_the_latest_wins_and_scopes_are_enforced() {
    let server = Server::start();
    let reader = server.token("ci", "read");
    let alice = server.token("alice-laptop", "read,publish");
    let bob = server.token("bob-laptop", "publish");
    let signer = server.token("alice-signoff", "read,publish,human");
    let (app, sign) = (key(1), key(2));

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

    let green = record(&app, "runtime", "GREEN");
    assert_eq!(
        server.publish(&alice, &app, &green).await,
        (StatusCode::CREATED, json!({"created":true}))
    );
    // Another execution of the key is appended; the same execution is stored once.
    let red = another(
        green.clone(),
        "execution-run-b-1",
        "RED",
        "2026-10-04T00:00:02Z",
    );
    assert_eq!(
        server.publish(&bob, &app, &red).await,
        (StatusCode::CREATED, json!({"created":true}))
    );
    assert_eq!(
        server.publish(&alice, &app, &green).await,
        (StatusCode::OK, json!({"created":false}))
    );
    // A record that completed earlier joins the history without becoming the latest.
    let older = another(
        green.clone(),
        "execution-run-c-1",
        "GREEN",
        "2026-10-03T23:59:59+00:00",
    );
    assert_eq!(
        server.publish(&alice, &app, &older).await.0,
        StatusCode::CREATED
    );
    assert_eq!(stored(&server.state).len(), 3);
    assert_eq!(
        server
            .publish(&reader, &key(3), &record(&key(3), "runtime", "GREEN"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(server.lookup(&bob, &[&app]).await.0, StatusCode::FORBIDDEN);
    let (status, found) = server.lookup(&reader, &[&app, &key(9)]).await;
    assert_eq!(status, StatusCode::OK);
    let entries = found["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["verdict"], "RED");
    assert_eq!(entries[0]["executionId"], "execution-run-b-1");
    assert_eq!(entries[0]["publisher"], "bob-laptop");
    assert!(entries[0]["publishedAt"].is_string());

    let (status, error) = server
        .publish(&alice, &sign, &record(&sign, "human", "GREEN"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    assert_eq!(
        server
            .publish(&signer, &sign, &record(&sign, "human", "GREEN"))
            .await
            .0,
        StatusCode::CREATED
    );
    for (path, record) in [
        (key(4), record(&key(5), "runtime", "GREEN")),
        (
            key(4),
            another(
                record(&key(4), "runtime", "GREEN"),
                "x",
                "GREEN",
                "yesterday",
            ),
        ),
        (
            "not-a-key".to_owned(),
            record("not-a-key", "runtime", "GREEN"),
        ),
    ] {
        assert_eq!(
            server.publish(&alice, &path, &record).await.0,
            StatusCode::BAD_REQUEST,
            "{record}"
        );
    }
    let mut legacy = record(&key(4), "runtime", "GREEN");
    legacy["schema"] = json!(1);
    assert_eq!(
        server.publish(&alice, &key(4), &legacy).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut large = record(&key(6), "agent", "GREEN");
    large["result"]["notes"] = json!("x".repeat(300 * 1024));
    assert_eq!(
        server.publish(&alice, &key(6), &large).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );

    let tokens = admin(&server.state, &["token", "list"]);
    assert!(!tokens.to_string().contains("azt_"));
    let stored_hash: String = Connection::open(server.state.join("review-store.sqlite"))
        .unwrap()
        .query_row("SELECT hash FROM tokens WHERE name='ci'", [], |row| {
            row.get(0)
        })
        .unwrap();
    let digest: String = Sha256::digest(reader.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(stored_hash, digest);

    // Purging bob removes his latest record; alice's newest record is the latest again.
    let revoked = admin(&server.state, &["token", "revoke", "bob-laptop", "--purge"]);
    assert_eq!(revoked["purgedEntries"], 1);
    assert_eq!(server.whoami(&bob).await.0, StatusCode::UNAUTHORIZED);
    let (_, found) = server.lookup(&reader, &[&app, &sign]).await;
    let entries = found["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["executionId"], "execution-run-a-1");
    assert_eq!(entries[1]["publisher"], "alice-signoff");
    let listed = admin(&server.state, &["token", "list"]);
    assert!(listed[2]["revokedAt"].is_string() && listed[2]["name"] == "bob-laptop");

    // `server rm` removes every record of a key.
    assert_eq!(
        admin(&server.state, &["rm", app.as_str()]),
        json!({"removed":true})
    );
    assert_eq!(
        admin(&server.state, &["rm", app.as_str()]),
        json!({"removed":false})
    );
    let (_, found) = server.lookup(&reader, &[&app]).await;
    assert_eq!(found, json!({"entries":[]}));
    assert_eq!(stored(&server.state).len(), 1);
}

#[tokio::test]
async fn typed_publish_fields_preserve_extensible_payload_and_wire_id_limits() {
    let server = Server::start();
    let token = server.token("typed-boundary", "read,publish");
    let app = key(7);
    let mut valid = record(&app, "runtime", "GREEN");
    valid["executionId"] = json!("x".repeat(200));
    valid["profile"]["futureOption"] = json!({"items":[1,null,true]});
    valid["ownerExtension"] = json!({"nested":["a",null,{"ok":true}]});
    // Server-owned metadata is overwritten even when supplied with another JSON type.
    valid["publisher"] = json!({"untrusted":true});
    valid["publishedAt"] = json!(42);
    valid["execution"] = Value::Null;
    assert_eq!(
        server.publish(&token, &app, &valid).await.0,
        StatusCode::CREATED
    );
    let (_, found) = server.lookup(&token, &[&app]).await;
    let saved = &found["entries"][0];
    assert_eq!(saved["executionId"], valid["executionId"]);
    assert_eq!(saved["profile"], valid["profile"]);
    assert_eq!(saved["ownerExtension"], valid["ownerExtension"]);
    assert!(saved.as_object().unwrap().contains_key("execution"));
    assert_eq!(saved["publisher"], "typed-boundary");
    assert!(saved["publishedAt"].is_string());
    for (field, value) in [
        ("schema", json!("2")),
        ("key", json!(7)),
        ("verdict", json!("WAITING_HUMAN")),
        ("executionId", json!("x".repeat(201))),
        ("executionId", json!(format!("remote-{}", "x".repeat(194)))),
        ("completedAt", Value::Null),
        ("profile", json!({"kind":"unknown"})),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert_eq!(
            server.publish(&token, &app, &invalid).await.0,
            StatusCode::BAD_REQUEST,
            "{field}"
        );
    }
}

fn stored(state: &Path) -> Vec<(String, i64, String)> {
    let db = Connection::open(state.join("review-store.sqlite")).unwrap();
    let mut statement = db
        .prepare("SELECT key,bytes,data FROM entries ORDER BY key,completed_at")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test]
async fn clients_before_0_5_are_told_to_upgrade() {
    let server = Server::start();
    let alice = server.token("alice-laptop", "read,publish");
    for field in ["fingerprint", "staleKey"] {
        let mut key = json!({"evalDefHash":HASH});
        key[field] = json!("app-v1");
        let (status, error) = server.lookup_body(&alice, json!({"keys":[key]})).await;
        assert_eq!(status, StatusCode::GONE);
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("upgrade this client to artifactize 0.5"),
            "{error}"
        );
    }
    let response = server
        .client
        .put(format!("{}v1/entries/{HASH}/app-v1", server.url))
        .bearer_auth(&alice)
        .json(&json!({"schema":1}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::GONE);
    assert!(stored(&server.state).is_empty());
}

#[tokio::test]
async fn older_review_stores_upgrade_in_place_and_keep_their_tokens() {
    let token = "azt_older-reader";
    let digest: String = Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    // The version 1 layout written by artifactize 0.3 and the version 2 layout of 0.4.
    for (version, column) in [(1, "stale_key"), (2, "fingerprint")] {
        let server = Server::start_with(|state| {
            fs::create_dir_all(state).unwrap();
            let db = Connection::open(state.join("review-store.sqlite")).unwrap();
            db.execute_batch(&format!(
                "PRAGMA user_version={version};
                CREATE TABLE tokens(name TEXT PRIMARY KEY, hash TEXT NOT NULL UNIQUE, scopes TEXT NOT NULL, created_at TEXT NOT NULL, revoked_at TEXT);
                CREATE TABLE entries(eval_def_hash TEXT NOT NULL, {column} TEXT NOT NULL, publisher TEXT NOT NULL, bytes INTEGER NOT NULL, last_used TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(eval_def_hash, {column}));
                CREATE INDEX entries_lru ON entries(last_used, eval_def_hash, {column});
                CREATE INDEX entries_publisher ON entries(publisher);"
            ))
            .unwrap();
            db.execute(
                "INSERT INTO tokens VALUES('ci',?,'[\"read\",\"publish\"]','2026-10-04T00:00:00Z',NULL)",
                [&digest],
            )
            .unwrap();
            db.execute(
                "INSERT INTO entries VALUES(?,'app-v1','alice-laptop',2,'2026-10-04T00:00:02Z','{}')",
                [HASH],
            )
            .unwrap();
        });
        let db = Connection::open(server.state.join("review-store.sqlite")).unwrap();
        assert_eq!(
            db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            3
        );
        let schema: Vec<String> = db
            .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let schema = schema.join("\n");
        assert!(
            !schema.contains("stale") && !schema.contains("fingerprint") && schema.contains("key"),
            "{schema}"
        );
        // Records keyed the 0.4 way cannot match a 0.5 key; the tokens keep working.
        assert!(stored(&server.state).is_empty());
        assert_eq!(server.whoami(token).await.0, StatusCode::OK);
        let app = key(1);
        assert_eq!(
            server
                .publish(token, &app, &record(&app, "runtime", "GREEN"))
                .await
                .0,
            StatusCode::CREATED
        );
        let (_, found) = server.lookup(token, &[&app]).await;
        assert_eq!(found["entries"][0]["key"], app.as_str());
    }
}
