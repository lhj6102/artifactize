use std::{fs, path::Path};

use ratatui::{Terminal, backend::TestBackend};
use rusqlite::Connection;
use serde_json::json;
use time::format_description::well_known::Rfc3339;

use super::*;
use crate::store::{DATABASE, Receipts, Request, Run};

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-01T12:02:00Z", &Rfc3339).unwrap()
}

fn run(state: &Path, repo: &Path, id: &str, finished: bool) -> Run {
    serde_json::from_value(json!({
        "id":id, "repoPath":repo, "stateDir":state,
        "status":if finished { "COMPLETED" } else { "RUNNING" },
        "createdAt":if finished { "2026-01-01T12:00:00Z" } else { "2026-01-01T12:01:00Z" },
        "completedAt":if finished { Some("2026-01-01T12:00:30Z") } else { None },
        "selection":{"kind":"all"}, "validation":if finished { json!({"satisfied":true}) } else { json!({}) },
        "error":null
    })).unwrap()
}

fn request(run: &Run, name: &str, status: &str) -> Request {
    serde_json::from_value(json!({
        "id":format!("{}-{name}", run.id), "runId":run.id,
        "evalId":format!("artifact/{name}"), "target":"artifact", "title":name,
        "profile":{"kind":if status == "WAITING_HUMAN" { "human" } else { "runtime" }},
        "requestedProfile":{}, "payload":{}, "references":{}, "deps":[],
        "status":status, "createdAt":run.created_at, "startedAt":run.created_at,
        "completedAt":if ["GREEN", "RED", "ERROR"].contains(&status) { Some("2026-01-01T12:01:15Z") } else { None },
        "cwd":run.repo_path
    })).unwrap()
}

async fn seed(state: &Path, repo: &Path) -> Receipts {
    let receipts = Receipts::open(state, repo).await.unwrap();
    let finished = run(state, &repo.with_file_name("second"), "finished", true);
    let mut green = request(&finished, "pass", "GREEN");
    green.completed_at = finished.completed_at.clone();
    receipts.create_run(&finished, &[green]).await.unwrap();
    let running = run(state, repo, "running", false);
    let mut error = request(&running, "broken", "ERROR");
    error.error_code = Some("RUNTIME_ERROR".into());
    error.error = Some("command failed".into());
    let mut waiting = request(&running, "dependent", "WAIT_DEPENDENCY");
    waiting.started_at = None;
    waiting.blocked_reason = Some("Waiting for artifact/broken.".into());
    receipts
        .create_run(
            &running,
            &[
                request(&running, "slow", "RUNNING"),
                request(&running, "review", "WAITING_HUMAN"),
                error,
                waiting,
            ],
        )
        .await
        .unwrap();
    receipts
}

#[tokio::test]
async fn saved_models_refresh_and_errors_never_modify_state_or_take_a_write_lock() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let repo = root.path().join("first");
    fs::create_dir(&repo).unwrap();
    let receipts = seed(&state, &repo).await;
    // Saved reads must work after the repository and all its declarations are gone.
    fs::remove_dir(&repo).unwrap();
    let database = Connection::open(state.join(DATABASE)).unwrap();
    database
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); BEGIN IMMEDIATE;")
        .unwrap();
    let before = fs::read(state.join(DATABASE)).unwrap();
    let wal_before = fs::read(state.join(format!("{DATABASE}-wal"))).unwrap();
    let mut app = App::new(state.clone(), None);
    tokio::time::timeout(Duration::from_secs(1), app.refresh())
        .await
        .unwrap();
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(
        app.runs
            .iter()
            .map(|run| run.id.as_str())
            .collect::<Vec<_>>(),
        ["running", "finished"]
    );
    let row = view::RunRow::new(&app.runs[0], now());
    assert_eq!(row.age, "1m 0s");
    assert_eq!(row.counts["RUNNING"], 1);
    tokio::time::timeout(Duration::from_secs(1), app.open_selected())
        .await
        .unwrap();
    assert!(app.error.is_none(), "{:?}", app.error);
    let progress = view::Progress::new(app.progress.as_ref().unwrap(), now());
    assert_eq!(progress.duration, "1m 0s");
    assert_eq!(progress.counts["GREEN"], 0);
    assert_eq!(progress.counts["ERROR"], 1);
    assert_eq!(progress.counts["WAIT_DEPENDENCY"], 1);
    assert_eq!(progress.running[0].duration, "1m 0s");
    assert_eq!(progress.waiting[0].request, "running-review");
    assert_eq!(progress.other[0].duration, "15s");
    assert_eq!(
        progress.other[0].error.as_deref(),
        Some("RUNTIME_ERROR: command failed")
    );
    assert_eq!(progress.other[1].duration, "-");
    assert_eq!(
        progress.other[1].reason.as_deref(),
        Some("Waiting for artifact/broken.")
    );
    assert_eq!(fs::read(state.join(DATABASE)).unwrap(), before);
    assert_eq!(
        fs::read(state.join(format!("{DATABASE}-wal"))).unwrap(),
        wal_before
    );
    database.execute_batch("ROLLBACK;").unwrap();

    let mut settled = app.progress.as_ref().unwrap().requests[0].clone();
    settled.status = "GREEN".into();
    settled.completed_at = Some("2026-01-01T12:01:45Z".into());
    receipts.save_request(&settled).await.unwrap();
    app.key(KeyCode::Char('r').into()).await;
    let updated = view::Progress::new(app.progress.as_ref().unwrap(), now());
    assert!(updated.running.is_empty());
    assert_eq!(updated.counts["GREEN"], 1);
    assert_eq!(updated.other[0].duration, "45s");

    let last_refresh = app.last_refresh.clone();
    database.pragma_update(None, "user_version", 99).unwrap();
    app.refresh().await;
    assert!(
        app.error
            .as_deref()
            .unwrap()
            .contains("Unsupported state schema version")
    );
    assert_eq!(app.last_refresh, last_refresh);
    assert_eq!(
        view::Progress::new(app.progress.as_ref().unwrap(), now()),
        updated
    );
    database
        .pragma_update(None, "user_version", store::STATE_SCHEMA_VERSION)
        .unwrap();
    app.refresh().await;
    assert!(app.error.is_none());

    let mut filtered = App::new(state, Some(repo.with_file_name("second")));
    filtered.refresh().await;
    assert_eq!(filtered.runs.len(), 1);
    filtered.open_selected().await;
    let finished = view::Progress::new(filtered.progress.as_ref().unwrap(), now());
    assert_eq!(finished.duration, "30s");
    assert_eq!(finished.satisfied, Some(true));
}

#[tokio::test]
async fn paging_selection_and_navigation_preserve_last_known_data() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    let receipts = Receipts::open(&state, &repo).await.unwrap();
    for index in 0..=PAGE_SIZE {
        receipts
            .create_run(&run(&state, &repo, &format!("run-{index:02}"), false), &[])
            .await
            .unwrap();
    }
    let mut app = App::new(state.clone(), None);
    app.refresh().await;
    assert_eq!(app.runs.len(), PAGE_SIZE as usize);
    assert!(app.more);
    app.key(KeyCode::Char('j').into()).await;
    assert_eq!(app.selection.selected(), Some(1));
    let selected = app.runs[1].id.clone();
    receipts
        .create_run(&run(&state, &repo, "newer", false), &[])
        .await
        .unwrap();
    app.refresh().await;
    assert_eq!(app.runs[app.selection.selected().unwrap()].id, selected);
    app.key(KeyCode::Enter.into()).await;
    assert_eq!(app.screen, Screen::Progress(selected));
    app.key(KeyCode::Char('b').into()).await;
    assert_eq!(app.screen, Screen::Runs);
    app.key(KeyCode::PageDown.into()).await;
    assert_eq!(app.offset, PAGE_SIZE);
    assert_eq!(app.runs.len(), 2);
    assert!(!app.more);
    app.key(KeyCode::PageUp.into()).await;
    assert_eq!(app.offset, 0);
    let last_refresh = app.last_refresh.clone();
    let first_id = app.runs[0].id.clone();
    let db = Connection::open(state.join(DATABASE)).unwrap();
    db.pragma_update(None, "user_version", 99).unwrap();
    app.key(KeyCode::Char('n').into()).await;
    assert!(app.error.is_some());
    assert_eq!(app.offset, 0);
    assert_eq!(app.runs[0].id, first_id);
    assert_eq!(app.last_refresh, last_refresh);
    assert!(app.key(KeyCode::Char('q').into()).await);
    assert!(app.key(KeyCode::Esc.into()).await);
}

#[tokio::test]
async fn missing_state_is_not_created() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("missing");
    let mut app = App::new(state.clone(), Some(root.path().join("missing-repo")));
    app.refresh().await;
    app.key(KeyCode::Down.into()).await;
    app.key(KeyCode::Enter.into()).await;
    assert!(app.error.is_none());
    assert!(app.runs.is_empty());
    assert_eq!(app.selection.selected(), None);
    assert!(!state.exists());
}

fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::draw(frame, app, now())).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn run_list_and_progress_render_snapshots() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let receipts = seed(&state, Path::new("/saved/first")).await;
    let mut app = App::new(state, None);
    app.refresh().await;
    app.last_refresh = Some("2026-01-01T12:02:00Z".into());
    let list = render(&mut app, 110, 18);
    assert_eq!(list, include_str!("snapshots/runs.txt").trim_end());
    app.open_selected().await;
    app.last_refresh = Some("2026-01-01T12:02:00Z".into());
    let progress = render(&mut app, 110, 30);
    assert_eq!(progress, include_str!("snapshots/progress.txt").trim_end());
    app.error = Some("database unavailable".into());
    assert!(
        render(&mut app, 110, 30)
            .contains("Read error (showing last-known data): database unavailable")
    );
    // The entire wrapped progress remains reachable even on small terminals.
    app.scroll = u16::MAX;
    let narrow = render(&mut app, 35, 12);
    assert!(narrow.contains("artifact/broken."));
    render(&mut app, 1, 1);
    drop(receipts);
}

#[test]
fn malformed_timestamps_and_control_characters_are_safe() {
    let mut run = run(Path::new("/state"), Path::new("/repo"), "run", false);
    run.error = Some("bad\u{1b}[2J\nmessage".into());
    run.created_at = "invalid".into();
    let model = view::Progress::new(
        &RunView {
            run,
            requests: vec![],
        },
        now(),
    );
    assert_eq!(model.duration, "-");
    assert_eq!(model.error.as_deref(), Some("bad [2J message"));
}
