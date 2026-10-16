//! Legacy saved integer semantics are independent of config timeout validation.
use crate::store::Run;
use serde_json::{Value, json};
use std::time::Duration;

fn run_wire() -> Value {
    json!({
        "id":"run-duration",
        "repoPath":"fixture/repo",
        "stateDir":"fixture/state",
        "status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z",
        "selection":{"kind":"all"},
        "validation":null,
    })
}

#[test]
fn saved_duration_matches_schema5_integer_null_missing_boundaries() {
    for (wire, expected) in [
        (None, None),
        (Some(Value::Null), None),
        (Some(json!(0)), Some(Duration::ZERO)),
        (Some(json!(1)), Some(Duration::from_millis(1))),
        (Some(json!(600000)), Some(Duration::from_millis(600000))),
        (
            Some(json!(4294967295_u64)),
            Some(Duration::from_millis(4294967295)),
        ),
    ] {
        let mut value = run_wire();
        if let Some(wire) = wire {
            value["waitTimeoutMs"] = wire;
        }
        let expected_wire = value.get("waitTimeoutMs").cloned().unwrap_or(Value::Null);
        let saved: Run = serde_json::from_value(value).unwrap();
        assert_eq!(saved.wait_timeout_ms, expected);
        assert_eq!(
            serde_json::to_value(saved).unwrap()["waitTimeoutMs"],
            expected_wire
        );
    }
    for invalid in [
        json!(-1),
        json!(1.0),
        json!(1.5),
        json!(4294967296_u64),
        json!("1"),
    ] {
        let mut value = run_wire();
        value["waitTimeoutMs"] = invalid;
        assert!(serde_json::from_value::<Run>(value).is_err());
    }
}

#[tokio::test]
async fn unrepresentable_duration_never_writes_an_unreadable_run() {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(&repo).unwrap();
    let receipts = crate::store::OwnedReceipts::open(&state, &repo)
        .await
        .unwrap();
    let mut value = run_wire();
    value["repoPath"] = json!(repo);
    value["stateDir"] = json!(state);
    value["waitTimeoutMs"] = json!(1);
    let mut run: Run = serde_json::from_value(value).unwrap();
    receipts.create_run(&run, &[]).await.unwrap();
    for duration in [
        Duration::from_millis(4294967296),
        Duration::from_nanos(1),
        Duration::from_micros(1001),
    ] {
        run.wait_timeout_ms = Some(duration);
        assert!(serde_json::to_value(&run).is_err());
        assert!(receipts.save_run(&run).await.is_err());
        assert_eq!(
            crate::store::read_run(&state, "run-duration")
                .await
                .unwrap()
                .run
                .wait_timeout_ms,
            Some(Duration::from_millis(1))
        );
        run.id = "run-invalid-duration".parse().unwrap();
        assert!(receipts.create_run(&run, &[]).await.is_err());
        assert!(
            crate::store::read_run(&state, "run-invalid-duration")
                .await
                .is_err()
        );
        run.id = "run-duration".parse().unwrap();
    }
}
