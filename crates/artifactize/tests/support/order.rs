//! Explicit timestamp fixtures for tests that assert cache or Run ordering.
#![expect(
    dead_code,
    reason = "each test binary uses a different ordering fixture"
)]

use artifactize_core::{store, types};
use std::{
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

/// Pin a completed session's GC position, independent of filesystem clock resolution.
pub fn session_written_at(path: &Path, seconds: u64) {
    crate::support::os::set_modified(path, UNIX_EPOCH + Duration::from_secs(seconds));
}

/// Give a terminal Run a declared duration after its real producer has exited. Preserve its
/// typed verdict/error and every request; only the two Run timestamps are fixture inputs.
pub fn run_times(state: &Path, id: &str, created: &str, completed: &str) {
    let id: types::RunId = id.parse().unwrap();
    let created: types::Timestamp = created.parse().unwrap();
    let completed: types::Timestamp = completed.parse().unwrap();
    assert!(created <= completed, "fixture Run times are ordered");
    let mut run: store::Run = {
        let db = rusqlite::Connection::open_with_flags(
            state.join(store::DATABASE),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let data: String = db
            .query_row("SELECT data FROM runs WHERE id=?", [&id], |row| row.get(0))
            .unwrap();
        serde_json::from_str(&data).unwrap()
    };
    crate::support::runs::finished_worker(state, &run.repo_path, &id, crate::support::os::running);
    let at = match &mut run.state {
        store::RunState::Running | store::RunState::WaitingHuman => {
            panic!("fixture times require a terminal Run")
        }
        store::RunState::Yellow { at }
        | store::RunState::Green { at }
        | store::RunState::Red { at }
        | store::RunState::Error { at, .. }
        | store::RunState::Incomplete { at, .. } => at,
    };
    *at = completed;
    run.created_at = created;
    let data = serde_json::to_string(&run).unwrap();
    let db = rusqlite::Connection::open(state.join(store::DATABASE)).unwrap();
    assert_eq!(
        db.execute(
            "UPDATE runs SET data=? WHERE id=?",
            rusqlite::params![data, id]
        )
        .unwrap(),
        1,
    );
}

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
