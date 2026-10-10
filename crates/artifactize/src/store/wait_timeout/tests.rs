//! Legacy saved integer semantics are independent of config timeout validation.
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

#[derive(Serialize, Deserialize)]
struct Saved {
    #[serde(default, rename = "waitTimeoutMs", with = "super")]
    timeout: Option<Duration>,
}
#[derive(Deserialize)]
struct Legacy {
    #[serde(default, rename = "waitTimeoutMs")]
    timeout: Option<u32>,
}

#[test]
fn saved_duration_matches_schema5_integer_null_missing_boundaries() {
    for value in [
        json!({}),
        json!({"waitTimeoutMs":null}),
        json!({"waitTimeoutMs":0}),
        json!({"waitTimeoutMs":1}),
        json!({"waitTimeoutMs":600000}),
        json!({"waitTimeoutMs":4294967295_u64}),
    ] {
        let old: Legacy = serde_json::from_value(value.clone()).unwrap();
        let saved: Saved = serde_json::from_value(value).unwrap();
        assert_eq!(
            saved.timeout,
            old.timeout.map(|ms| Duration::from_millis(u64::from(ms)))
        );
        assert_eq!(
            serde_json::to_value(saved).unwrap(),
            json!({"waitTimeoutMs":old.timeout})
        );
    }
    for value in [
        json!({"waitTimeoutMs":-1}),
        json!({"waitTimeoutMs":1.0}),
        json!({"waitTimeoutMs":1.5}),
        json!({"waitTimeoutMs":4294967296_u64}),
        json!({"waitTimeoutMs":"1"}),
    ] {
        assert!(serde_json::from_value::<Legacy>(value.clone()).is_err());
        assert!(serde_json::from_value::<Saved>(value).is_err());
    }
}

#[tokio::test]
async fn unrepresentable_duration_never_writes_an_unreadable_run() {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(&repo).unwrap();
    let receipts = crate::store::Receipts::open(&state, &repo).await.unwrap();
    let mut run: crate::store::Run = serde_json::from_value(json!({
        "id":"run-duration",
        "repoPath":repo,
        "stateDir":state,
        "status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z",
        "selection":{"kind":"all"},
        "validation":null,
        "waitTimeoutMs":1,
    }))
    .unwrap();
    receipts.create_run(&run, &[]).await.unwrap();
    for duration in [
        Duration::from_millis(u64::from(u32::MAX) + 1),
        Duration::from_nanos(1),
        Duration::from_micros(1001),
    ] {
        assert!(
            serde_json::to_value(Saved {
                timeout: Some(duration)
            })
            .is_err()
        );
        run.wait_timeout_ms = Some(duration);
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
