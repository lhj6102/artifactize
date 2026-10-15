use super::*;
use crate::store::{self, OwnedReceipts};
use serde_json::{Value, json};
use std::fs;

fn run(id: &str, repo: &Path, state: &Path, definitions: Value) -> Run {
    serde_json::from_value(json!({
        "id":id,"repoPath":repo,"stateDir":state,"status":"GREEN",
        "createdAt":"2026-01-01T00:00:00Z","completedAt":"2026-01-01T00:00:01Z",
        "selection":{"kind":"all"},"definitions":definitions,"validation":null,
        "evidence":{"dependency/check":"RED"}
    }))
    .unwrap()
}
fn request(run: &str, artifact: &str, status: &str, repo: &Path) -> store::Request {
    crate::monitor::tests::request(
        &format!("{artifact}/check"),
        status,
        json!({
            "id":format!("{run}-{artifact}"),"runId":run,"cwd":repo
        }),
    )
    .request
}
async fn fixture(
    runs: usize,
    requests: usize,
) -> (tempfile::TempDir, PathBuf, PathBuf, OwnedReceipts) {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let receipts = OwnedReceipts::open(&state, &repo).await.unwrap();
    for n in 0..runs {
        let id = format!("run-{n}");
        let run = run(
            &id,
            &repo,
            &state,
            json!({"artifacts":{"app":{"path":"app","owner":{"unknown":true}}},"evals":[{"id":"app/check","target":"app","deps":[],"declaration":{"title":"Saved title"}}]}),
        );
        let requests = (0..requests)
            .map(|q| request(&id, &format!("a{q}"), "GREEN", &repo))
            .collect::<Vec<_>>();
        receipts.create_run(&run, &requests).await.unwrap();
    }
    (root, repo, state, receipts)
}
fn read(
    reader: &mut Reader,
    state: &Path,
    id: &RunId,
    limit: u32,
) -> Option<(RunView, Vec<RequestView>)> {
    reader
        .snapshot(state, |db, cache, state| {
            cache.catalog(db, state)?;
            let page = cache.page(db, state, None, limit, 0, Some(id))?;
            let audit = page
                .iter()
                .find(|run| &run.id == id)
                .unwrap()
                .unreadable
                .clone();
            let selected = cache.selected(db, state, id, Some(audit), false)?;
            cache.identity(db)?;
            Ok(selected)
        })
        .unwrap()
}
#[tokio::test]
async fn read_run_releases_database_before_return_on_success_and_errors() {
    for id in ["run-0", "run-absent", ""] {
        let (root, _repo, state, receipts) = fixture(1, 1).await;
        drop(receipts);
        let result = store::read_run(&state, id).await;
        if id == "run-0" {
            let saved = result.unwrap();
            assert_eq!(saved.run.id.as_str(), id);
            assert_eq!(saved.requests.len(), 1);
        } else {
            assert!(result.is_err(), "{id:?} must not identify a saved Run");
        }
        // No retry or runtime yield: returning the read must release the database.
        fs::rename(state.join(DATABASE), state.join("released.sqlite")).unwrap();
        fs::rename(state.join("released.sqlite"), state.join(DATABASE)).unwrap();
        root.close().unwrap();
    }
}

#[tokio::test]
async fn snapshots_and_direct_lookups_return_only_the_selected_runs_requests() {
    for (runs, requests) in [(3, 4), (11, 13)] {
        let (_root, _repo, state, _receipts) = fixture(runs, requests).await;
        let mut reader = Reader::default();
        let id = "run-0".parse().unwrap();
        let (run, views) = read(&mut reader, &state, &id, runs as u32).unwrap();
        assert_eq!(run.run.id.as_str(), "run-0");
        let expected = (0..requests)
            .map(|q| format!("run-0-a{q}"))
            .collect::<Vec<_>>();
        assert_eq!(
            views
                .iter()
                .map(|view| view.request.id.to_string())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(read(&mut reader, &state, &id, runs as u32).is_none());
        let direct = store::read_requests(&state, Some("run-0")).await.unwrap();
        assert_eq!(
            direct
                .iter()
                .map(|view| view.request.id.to_string())
                .collect::<Vec<_>>(),
            expected
        );
        let direct = store::read_request(&state, "run-0-a0").await.unwrap();
        assert_eq!(direct.request.id.as_str(), "run-0-a0");
        assert_eq!(direct.request.run_id.as_str(), "run-0");
        assert_eq!(direct.request.status(), crate::types::RequestStatus::Green);
    }
}
#[tokio::test]
async fn missed_hints_reconcile_only_changed_views_and_resync_rebuilds_the_baseline() {
    let (_root, _repo, state, receipts) = fixture(3, 4).await;
    let mut reader = Reader::default();
    let id = "run-0".parse().unwrap();
    assert_eq!(read(&mut reader, &state, &id, 3).unwrap().1.len(), 4);
    // Direct external commits emit no changes hint: the next snapshot remains authoritative.
    let db = Connection::open(state.join(DATABASE)).unwrap();
    db.execute("UPDATE requests SET status='ERROR',data=json_set(data,'$.status','ERROR','$.result',NULL,'$.error','external failure') WHERE id='run-0-a0'", []).unwrap();
    let (run, views) = read(&mut reader, &state, &id, 3).unwrap();
    assert_eq!(
        views[0].request.status(),
        crate::types::RequestStatus::Error
    );
    assert_eq!(views[0].request.error(), Some("external failure"));
    assert_eq!(
        run.run.evidence["dependency/check"],
        crate::types::RequestStatus::Red
    );
    // Run metadata is published once, even when all request bodies stay unchanged.
    let mut saved = run.run;
    saved.jobs = Some(7);
    receipts.save_run(&saved).await.unwrap();
    let (run, views) = read(&mut reader, &state, &id, 3).unwrap();
    assert_eq!(run.run.jobs, Some(7));
    assert_eq!(views[0].request.error(), Some("external failure"));
    assert!(read(&mut reader, &state, &id, 3).is_none());
    reader.reset();
    assert_eq!(
        read(&mut reader, &state, &id, 3).unwrap().1[0]
            .request
            .error(),
        Some("external failure")
    );
    // Deletion evicts the record rather than leaving an old cached request in the tree.
    db.execute("DELETE FROM requests WHERE id='run-0-a1'", [])
        .unwrap();
    assert_eq!(read(&mut reader, &state, &id, 3).unwrap().1.len(), 3);
}
#[tokio::test]
async fn one_snapshot_preserves_known_requests_and_missing_duplicate_definitions() {
    let (_root, repo, state, receipts) = fixture(0, 0).await;
    let mut saved = run(
        "run-1",
        &repo,
        &state,
        json!({
            "artifacts":{"app":{"path":"saved","owner":{"kept":true}}},
            "evals":[{"id":"app/check","target":"app","declaration":{"title":"First","owner":1}}, {"id":"app/check","target":"app","declaration":{"title":"Second"}}]
        }),
    );
    let good = request("run-1", "app", "GREEN", &repo);
    let absent = request("run-1", "unknown", "RED", &repo);
    receipts
        .create_run(&saved, &[good.clone(), absent])
        .await
        .unwrap();
    let (snapshot, views) = store::read_run_requests(&state, "run-1").await.unwrap();
    assert_eq!(snapshot.run.id.as_str(), "run-1");
    assert_eq!(snapshot.run.status(), crate::types::RunStatus::Green);
    assert_eq!(
        views
            .iter()
            .map(|view| (view.request.id.as_str(), view.request.status()))
            .collect::<Vec<_>>(),
        [
            ("run-1-app", crate::types::RequestStatus::Green),
            ("run-1-unknown", crate::types::RequestStatus::Red)
        ]
    );
    assert_eq!(
        serde_json::to_value(views[0].definition.as_ref().unwrap()).unwrap()["eval"]["declaration"]
            ["title"],
        "First"
    );
    assert_eq!(
        serde_json::to_value(views[0].definition.as_ref().unwrap()).unwrap()["artifact"]["owner"]["kept"],
        true
    );
    assert!(views[1].definition.is_none());
    for definitions in [
        Value::Null,
        json!({}),
        json!({"artifacts":null,"evals":null}),
    ] {
        saved.definitions = serde_json::from_value(definitions).unwrap();
        receipts.save_run(&saved).await.unwrap();
        assert!(
            store::read_request(&state, &good.id)
                .await
                .unwrap()
                .definition
                .is_none()
        );
    }
}
#[tokio::test]
async fn snapshot_keeps_old_evidence_while_a_wal_writer_commits_and_then_reconciles() {
    let (_root, _repo, state, _receipts) = fixture(1, 2).await;
    let mut reader = Reader::default();
    let id: RunId = "run-0".parse().unwrap();
    reader.snapshot(&state, |db, cache, _| {
        let before: String = db.query_row("SELECT status FROM requests WHERE id='run-0-a0'", [], |row| row.get(0))?;
        let path = state.join(DATABASE);
        std::thread::spawn(move || {
            let writer = Connection::open(path).unwrap();
            writer.execute("UPDATE requests SET status='QUEUED',data=json_set(data,'$.status','QUEUED','$.result',NULL) WHERE id='run-0-a0'", []).unwrap();
        }).join().unwrap();
        let selected = cache.selected(db, &state, &id, None, false)?.unwrap();
        assert_eq!(before, "GREEN");
        assert_eq!(selected.1[0].request.status(), crate::types::RequestStatus::Green);
        Ok(())
    }).unwrap();
    assert_eq!(
        read(&mut reader, &state, &id, 1).unwrap().1[0]
            .request
            .status(),
        crate::types::RequestStatus::Queued
    );
}
#[tokio::test]
async fn reader_changes_database_identity_and_never_uses_the_previous_states_cache() {
    let (_root, _repo, state, _receipts) = fixture(1, 2).await;
    let (_replacement, _repo2, state2, _receipts2) = fixture(1, 3).await;
    let mut reader = Reader::default();
    let id = "run-0".parse().unwrap();
    let original = reader
        .snapshot(&state, |db, cache, _| cache.identity(db))
        .unwrap();
    assert_eq!(read(&mut reader, &state, &id, 1).unwrap().1.len(), 2);
    let replacement = reader
        .snapshot(&state2, |db, cache, _| cache.identity(db))
        .unwrap();
    assert_ne!(original, replacement);
    assert_eq!(read(&mut reader, &state2, &id, 1).unwrap().1.len(), 3);
}

#[tokio::test]
async fn followers_keep_the_original_claim_remote_session_and_run_specific_evidence() {
    let (_root, repo, state, receipts) = fixture(0, 0).await;
    let graph = json!({"artifacts":{"app":{"path":"saved"}},"evals":[{"id":"app/check","target":"app","declaration":{"title":"Saved"}}]});
    let mut original = request("run-1", "app", "WAITING_HUMAN", &repo);
    original.profile = serde_json::from_value(json!({"kind":"human"})).unwrap();
    original.requested_profile = original.profile.clone();
    original.execution_id = Some("execution".parse().unwrap());
    original.session_id = Some("original-session".parse().unwrap());
    let original_run = run("run-1", &repo, &state, graph.clone());
    receipts
        .create_run(&original_run, std::slice::from_ref(&original))
        .await
        .unwrap();
    let execution: store::Execution = serde_json::from_value(json!({
        "id":"execution","key":null,"fingerprint":null,"evalDefHash":original.eval_def_hash,
        "ownerPid":0,"ownerStartTime":0,"status":"WAITING_HUMAN","result":null,"error":null,"errorCode":null,
        "profile":{"kind":"human"},"usage":null,"startedAt":"2026-01-01T00:00:00Z","completedAt":null,
        "provenance":{"repoPath":repo,"runId":"run-1","requestId":original.id,"evalId":"app/check","evalDefHash":original.eval_def_hash,"completedAt":null}
    })).unwrap();
    receipts
        .complete_execution(&execution, &original)
        .await
        .unwrap();
    let mut follower = original.clone();
    follower.id = "follower".parse().unwrap();
    follower.run_id = "run-2".parse().unwrap();
    follower.joined = true;
    follower.session_id = Some("remote-session".parse().unwrap());
    follower.session = Some(serde_json::from_value(json!({"producer":"remote@host","state":"remote-state","runId":"remote-run","requestId":"remote-request","sessionId":"remote-session"})).unwrap());
    follower.producer = Some(store::Producer {
        name: "remote@host".parse().unwrap(),
        version: "saved".into(),
        session: follower.session.clone(),
    });
    let mut follower_run = run("run-2", &repo, &state, graph);
    follower_run.evidence.insert(
        "dependency/check".parse().unwrap(),
        crate::types::RequestStatus::Green,
    );
    receipts
        .create_run(&follower_run, std::slice::from_ref(&follower))
        .await
        .unwrap();
    receipts.save_request(&follower).await.unwrap();
    let db = Connection::open(state.join(DATABASE)).unwrap();
    db.execute(
        "UPDATE requests SET claimed_by='alice',claimed_at='2026-01-01T00:00:00Z' WHERE id=?",
        [&original.id],
    )
    .unwrap();
    let (saved, views) = store::read_run_requests(&state, "run-2").await.unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(
        serde_json::to_value(&views[0].request).unwrap(),
        serde_json::to_value(&follower).unwrap()
    );
    let claim = views[0].claim.as_ref().unwrap();
    assert_eq!(claim.request_id, original.id);
    assert_eq!(claim.reviewer.as_str(), "alice");
    assert_eq!(
        views[0].execution.as_ref().unwrap().id.as_str(),
        "execution"
    );
    assert_eq!(
        saved.run.evidence["dependency/check"],
        crate::types::RequestStatus::Green
    );
    assert_eq!(
        store::read_original(&state, &views[0])
            .await
            .unwrap()
            .request
            .id,
        original.id
    );
    assert_eq!(
        store::read_waiting_originals(&state, Some(&repo))
            .await
            .unwrap()
            .iter()
            .map(|view| view.request.id.as_str())
            .collect::<Vec<_>>(),
        [original.id.as_str()]
    );
    assert_eq!(
        store::read_session_request(&state, "remote-session")
            .await
            .unwrap()
            .unwrap()
            .request
            .id,
        follower.id
    );
    assert_eq!(
        store::read_request_repo(&state, &follower.id)
            .await
            .unwrap()
            .1,
        repo
    );
    let catalog = store::read_catalog(&state).await.unwrap();
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[0].waiting, catalog[1].waiting);
    assert_eq!(
        catalog[0].waiting,
        [store::Signoff::Execution(execution.id)]
    );
    // Losing the saved original must not hide a still-waiting follower.
    db.execute("DELETE FROM requests WHERE id=?", [&original.id])
        .unwrap();
    let waiting = store::read_waiting_originals(&state, Some(&repo))
        .await
        .unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].request.id, follower.id);
}
#[tokio::test]
async fn uninitialized_database_is_empty_until_the_writer_publishes_the_schema() {
    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    fs::create_dir(&state).unwrap();
    fs::create_dir(&repo).unwrap();
    Connection::open(state.join(DATABASE)).unwrap();
    let mut reader = Reader::default();
    assert!(
        reader
            .snapshot_optional(&state, |_, _, _| -> Result<(), Error> {
                panic!("No schema has been published")
            })
            .unwrap()
            .is_none()
    );
    let receipts = OwnedReceipts::open(&state, &repo).await.unwrap();
    receipts
        .create_run(&run("run-0", &repo, &state, Value::Null), &[])
        .await
        .unwrap();
    let id = "run-0".parse().unwrap();
    assert!(read(&mut reader, &state, &id, 1).unwrap().1.is_empty());
}

#[tokio::test]
async fn failed_snapshot_cannot_suppress_the_next_accepted_changed_view() {
    let (_root, _repo, state, _receipts) = fixture(1, 2).await;
    let mut reader = Reader::default();
    let id = "run-0".parse().unwrap();
    read(&mut reader, &state, &id, 1);
    let db = Connection::open(state.join(DATABASE)).unwrap();
    db.execute(
        "UPDATE requests SET data=json_set(data,'$.title','Changed') WHERE id='run-0-a0'",
        [],
    )
    .unwrap();
    let failed = reader.snapshot(&state, |db, cache, state| -> Result<(), Error> {
        assert!(cache.selected(db, state, &id, None, false)?.is_some());
        Err(Error::Invalid("snapshot rejected after reading".into()))
    });
    assert!(failed.is_err());
    let views = read(&mut reader, &state, &id, 1).unwrap().1;
    assert_eq!(views[0].request.title, "Changed");
}

#[tokio::test]
async fn unreadable_state_identity_does_not_hide_saved_runtime_runs() {
    let (_root, _repo, state, _receipts) = fixture(1, 2).await;
    let db = Connection::open(state.join(DATABASE)).unwrap();
    db.execute("UPDATE state_meta SET value='' WHERE name='id'", [])
        .unwrap();
    let mut reader = Reader::default();
    assert!(
        reader
            .snapshot(&state, |db, cache, _| cache.identity(db))
            .unwrap()
            .is_none()
    );
    let id = "run-0".parse().unwrap();
    assert_eq!(read(&mut reader, &state, &id, 1).unwrap().1.len(), 2);
    db.execute("DELETE FROM state_meta WHERE name='id'", [])
        .unwrap();
    reader.reset();
    assert!(
        reader
            .snapshot(&state, |db, cache, _| cache.identity(db))
            .unwrap()
            .is_none()
    );
    assert_eq!(read(&mut reader, &state, &id, 1).unwrap().1.len(), 2);
}

#[tokio::test]
async fn pinned_newer_run_preserves_visible_created_time_and_rowid_tie_order() {
    let (_root, _repo, state, _receipts) = fixture(5, 1).await;
    let mut reader = Reader::default();
    let pinned = "run-4".parse().unwrap();
    let ids = reader
        .snapshot(&state, |db, cache, state| {
            Ok(cache
                .page(db, state, None, 2, 2, Some(&pinned))?
                .into_iter()
                .map(|run| run.id)
                .collect::<Vec<_>>())
        })
        .unwrap();
    assert_eq!(
        ids.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
        ["run-4", "run-2", "run-1"]
    );
}
