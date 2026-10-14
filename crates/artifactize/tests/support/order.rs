//! Explicit timestamp fixtures for tests that assert cache or Run ordering.
#![expect(
    dead_code,
    reason = "each test binary uses a different ordering fixture"
)]

use artifactize_core::{store, types};
use std::path::Path;

/// Seed an execution's completion order after its producer has closed SQLite. Preserve the
/// actual verdict and keep the history index, provenance and byte accounting consistent.
pub fn completed_at(state: &Path, repo: &Path, run: &serde_json::Value, at: &str) {
    let run_id = run["id"].as_str().unwrap().parse().unwrap();
    crate::support::runs::finished_worker(state, repo, &run_id, crate::support::os::running);
    let id: types::ExecutionId = run["requests"][0]["executionId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let at: types::Timestamp = at.parse().unwrap();
    let db = rusqlite::Connection::open(state.join(store::DATABASE)).unwrap();
    let data: String = db
        .query_row("SELECT data FROM executions WHERE id=?", [&id], |row| {
            row.get(0)
        })
        .unwrap();
    let mut execution: store::Execution = serde_json::from_str(&data).unwrap();
    let store::ExecutionState::Completed { at: completed, .. } = &mut execution.state else {
        panic!("completion order is seeded only for a finished verdict");
    };
    *completed = at;
    execution.started_at = execution.started_at.min(at);
    execution.provenance.completed_at = Some(at);
    let data = serde_json::to_string(&execution).unwrap();
    assert_eq!(
        db.execute(
            "UPDATE executions SET completed_at=?,bytes=?,data=? WHERE id=?",
            rusqlite::params![at, data.len() as i64, data, id],
        )
        .unwrap(),
        1,
    );
}

/// Seed a saved Run's list position without changing its requests or lifecycle. Tests use
/// this only when the relative Run order is part of what their list or cursor asserts.
pub fn created_at(state: &Path, id: &str, at: &str) {
    let id: types::RunId = id.parse().unwrap();
    let at: types::Timestamp = at.parse().unwrap();
    let db = rusqlite::Connection::open(state.join(store::DATABASE)).unwrap();
    assert_eq!(
        db.execute(
            "UPDATE runs SET data=json_set(data,'$.createdAt',?1) WHERE id=?2",
            rusqlite::params![at, id],
        )
        .unwrap(),
        1,
    );
}
