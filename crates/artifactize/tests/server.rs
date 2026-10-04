use std::{
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
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("server");
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

    async fn publish(&self, token: &str, stale_key: &str, record: &Value) -> (StatusCode, Value) {
        let response = self
            .client
            .put(format!("{}v1/entries/{HASH}/{stale_key}", self.url))
            .bearer_auth(token)
            .json(record)
            .send()
            .await
            .unwrap();
        (response.status(), response.json().await.unwrap())
    }

    async fn lookup(&self, token: &str, stale_keys: &[&str]) -> (StatusCode, Value) {
        let keys: Vec<_> = stale_keys
            .iter()
            .map(|stale_key| json!({"staleKey":stale_key,"evalDefHash":HASH}))
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

fn record(stale_key: &str, kind: &str, verdict: &str) -> Value {
    json!({"schema":1,"staleKey":stale_key,"evalDefHash":HASH,"verdict":verdict,
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
