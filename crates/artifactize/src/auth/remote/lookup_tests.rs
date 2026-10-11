//! HTTP lookup shape errors remain per-entry, never poison a valid neighbor.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn malformed_lookup_entries_are_typed_errors_next_to_valid_records() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = parse_url(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let good = json!({
        "schema":2,
        "key":"a".repeat(64),
        "evalDefHash":"b".repeat(64),
        "fingerprints":{},
        "verdict":"GREEN",
        "evalId":"app/check",
        "runId":"run-1",
        "requestId":"run-1-1",
        "executionId":"execution-1",
        "profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]},
        "result":{"verdict":"GREEN"},
        "usage":null,
        "startedAt":"2026-01-01T00:00:00Z",
        "completedAt":"2026-01-01T00:00:01Z",
    });
    let body = json!({"entries":[{"unexpected":"shape"},good,17]}).to_string();
    let server = tokio::spawn(async move {
        let (mut connection, _) = listener.accept().await.unwrap();
        let mut request = [0; 8192];
        let _ = connection.read(&mut request).await.unwrap();
        connection
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    let remote = Remote {
        url,
        share: Share::Summary,
        token_source: TokenSource::Env,
        token: Some("fixture-only-token".into()),
        client: OnceLock::new(),
    };
    let entries = remote
        .lookup(&["a".repeat(64).parse().unwrap()])
        .await
        .unwrap();
    assert_eq!(entries.len(), 3);
    assert!(entries[0].is_err());
    assert_eq!(entries[1].as_ref().unwrap().request_id.as_str(), "run-1-1");
    assert!(entries[2].is_err());
    server.await.unwrap();
}
