use super::{Caps, collect_within, list};
use crate::store::{DATABASE, OwnedReceipts};
use serde_json::json;
use std::path::Path;

/// A reusable GREEN record of `eval` in `checkout`, completed and last used at second `at`.
fn record(db: &rusqlite::Connection, n: u32, checkout: &Path, eval: &str, human: bool, at: u32) {
    let key = format!("{n:064x}");
    let hash = format!("{:064x}", 1000 + n);
    let completed = format!("2026-01-01T00:00:{at:02}Z");
    let profile = if human {
        json!({"kind": "human"})
    } else {
        json!({"kind": "runtime", "command": "true", "args": []})
    };
    let data = json!({
        "id": format!("execution-{n}"), "key": key, "fingerprint": null, "evalDefHash": hash,
        "ownerPid": 0, "ownerStartTime": 0, "status": "GREEN", "result": {"verdict": "GREEN"},
        "error": null, "errorCode": null, "profile": profile, "options": {}, "usage": null,
        "provenance": {"repoPath": checkout, "runId": "run-1", "requestId": format!("run-1-{n}"),
            "evalId": eval, "evalDefHash": hash, "completedAt": completed},
        "startedAt": completed, "completedAt": completed,
    })
    .to_string();
    db.execute(
        "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,completed_at,bytes,last_used,data) VALUES (?,?,?,'GREEN',0,0,?,?,?,?)",
        rusqlite::params![format!("execution-{n}"), key, hash, completed, data.len() as i64, completed, data],
    )
    .unwrap();
}

#[tokio::test]
async fn a_large_project_evicts_neither_another_checkouts_newest_results_nor_its_own() {
    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    let (big, small, gone) = (
        root.path().join("big"),
        root.path().join("small"),
        root.path().join("gone"),
    );
    std::fs::create_dir(&big).unwrap();
    std::fs::create_dir(&small).unwrap();
    drop(OwnedReceipts::open(&state, &big).await.unwrap());
    let mut db = rusqlite::Connection::open(state.join(DATABASE)).unwrap();
    // The large project: five evals, two of them with older records a newer one replaced.
    for n in 0..5 {
        record(&db, n, &big, &format!("big{n}/check"), false, 30 + n);
    }
    record(&db, 5, &big, "big0/check", false, 10);
    record(&db, 6, &big, "big1/check", false, 11);
    // Another project, last used long ago: a sign-off and a machine result.
    record(&db, 7, &small, "small/approved", true, 1);
    record(&db, 8, &small, "small/check", false, 2);
    // A checkout that is gone: its sign-off and its machine result.
    record(&db, 9, &gone, "gone/approved", true, 3);
    record(&db, 10, &gone, "gone/check", false, 20);
    // Room for two records no checkout reuses now.
    collect_within(
        &mut db,
        Caps {
            entries: 2,
            bytes: i64::MAX,
        },
    )
    .unwrap();
    let kept: Vec<_> = list(&state, false)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.eval_id.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    // Every checkout's newest results stay, however few the caps allow; of the rest, the
    // gone checkout's sign-off stays before anything, then the most recently used record.
    assert_eq!(
        kept,
        [
            "big0/check",
            "big1/check",
            "big2/check",
            "big3/check",
            "big4/check",
            "gone/approved",
            "gone/check",
            "small/approved",
            "small/check",
        ]
    );
    let records = list(&state, true).await.unwrap();
    let ids: std::collections::BTreeSet<_> = records
        .iter()
        .map(|entry| entry.execution_id.to_string())
        .collect();
    assert!(
        !ids.contains("execution-5") && !ids.contains("execution-6"),
        "the replaced records left: {ids:?}"
    );
    assert!(ids.contains("execution-9") && ids.contains("execution-10"));
}
