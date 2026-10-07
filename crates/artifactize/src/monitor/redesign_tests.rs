//! Regression tests for catalog, modal isolation, saved evidence and UTF-8 drafts.
use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Position};
use serde_json::json;
use std::{fs, path::Path};

fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}
fn render(monitor: &mut Monitor, width: u16, height: u16) {
    Terminal::new(TestBackend::new(width, height))
        .unwrap()
        .draw(|frame| monitor.draw(frame))
        .unwrap();
}
fn saved_run(
    id: &str,
    repo: &Path,
    state: &Path,
    identity: crate::repository::Identity,
) -> store::Run {
    let mut run: store::Run = serde_json::from_value(json!({"id":id,"repoPath":repo,"stateDir":state,"status":"GREEN","createdAt":"2026-01-01T00:00:00Z","completedAt":"2026-01-01T00:00:01Z","selection":{"kind":"all"},"validation":null})).unwrap();
    run.repository = identity;
    run
}
fn git(path: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn catalog_is_global_and_scope_filter_precedes_more_than_one_page() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let alpha = root.path().join("alpha");
    let beta = root.path().join("beta");
    fs::create_dir_all(&alpha).unwrap();
    fs::create_dir_all(&beta).unwrap();
    let receipts = store::Receipts::open(&state, &alpha).await.unwrap();
    receipts
        .create_run(
            &saved_run("run-old-beta", &beta, &state, Default::default()),
            &[],
        )
        .await
        .unwrap();
    for n in 0..125 {
        receipts
            .create_run(
                &saved_run(
                    &format!("run-alpha-{n}"),
                    &alpha,
                    &state,
                    Default::default(),
                ),
                &[],
            )
            .await
            .unwrap();
    }
    let catalog = store::read_catalog(&state).await.unwrap();
    assert_eq!(catalog.len(), 126);
    let page = store::read_scoped_runs(&state, Some(std::slice::from_ref(&beta)), 100, 0)
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id.as_str(), "run-old-beta");
    assert!(
        store::read_scoped_runs(&state, Some(&[]), 100, 0)
            .await
            .unwrap()
            .is_empty()
    );
    let mut monitor = Monitor::new(state.clone(), Some(beta.clone()));
    monitor.refresh().await;
    assert_eq!(monitor.selected_run().unwrap().id.as_str(), "run-old-beta");
    assert!(
        monitor
            .catalog
            .rows
            .iter()
            .any(|row| row.label.contains("alpha"))
    );
    assert_eq!(store::state_schema(&state).unwrap(), Some(5));
    fs::remove_dir_all(&beta).unwrap();
    monitor.refresh().await;
    assert_eq!(monitor.selected_run().unwrap().id.as_str(), "run-old-beta");
}

#[tokio::test]
async fn git_subdirectory_initial_selection_preserves_workspace_and_discovers_empty_worktrees() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let other = root.path().join("other space\nline");
    fs::create_dir_all(repo.join("workspace/sub")).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
    git(
        &repo,
        &["worktree", "add", "-b", "topic", other.to_str().unwrap()],
    );
    let workspace = repo.join("workspace");
    let state = root.path().join("state");
    let identity = crate::repository::identify(&workspace);
    assert_eq!(identity.worktree_path.as_deref(), Some(repo.as_path()));
    let receipts = store::Receipts::open(&state, &workspace).await.unwrap();
    receipts
        .create_run(
            &saved_run("run-modern", &workspace, &state, identity.clone()),
            &[],
        )
        .await
        .unwrap();
    receipts
        .create_run(
            &saved_run("run-legacy", &other, &state, Default::default()),
            &[],
        )
        .await
        .unwrap();
    let mut monitor = Monitor::new(state.clone(), Some(workspace.join("sub")));
    monitor.refresh().await;
    assert_eq!(
        monitor.scope,
        Scope::Worktree(
            Repository::Git(identity.common_dir.clone().unwrap()),
            repo.clone()
        )
    );
    assert_eq!(monitor.runs.len(), 1);
    assert_eq!(
        store::read_run(&state, "run-modern")
            .await
            .unwrap()
            .run
            .repo_path,
        workspace
    );
    assert!(
        monitor
            .catalog
            .rows
            .iter()
            .any(|row| matches!(&row.scope, Scope::Worktree(_, path) if path == &other))
    );
    git(&repo, &["worktree", "remove", other.to_str().unwrap()]);
    let empty = root.path().join("empty");
    git(
        &repo,
        &["worktree", "add", "-b", "empty", empty.to_str().unwrap()],
    );
    let mut fresh = Monitor::new(state.clone(), None);
    fresh.refresh().await;
    assert!(
        fresh
            .catalog
            .rows
            .iter()
            .any(|row| matches!(&row.scope, Scope::Worktree(_, path) if path == &empty))
    );
    assert!(fresh.catalog.rows.iter().any(|row| matches!(&row.scope, Scope::Worktree(Repository::Workspace(_), path) if path == &other)), "deleted legacy paths must not be guessed into a Git group");
    fs::remove_dir_all(&repo).unwrap();
    let mut deleted = Monitor::new(state.clone(), Some(repo.clone()));
    deleted.refresh().await;
    assert_eq!(deleted.runs.len(), 1);
    assert!(matches!(
        deleted.scope,
        Scope::Worktree(Repository::Git(_), _)
    ));
    fresh.refresh().await;
    assert!(
        fresh.catalog.rows.iter().any(
            |row| matches!(&row.scope, Scope::Worktree(Repository::Git(_), path) if path == &repo)
        ),
        "stored identity survives deletion"
    );
}

#[test]
fn waiting_badges_deduplicate_execution_and_original_request() {
    let mut catalog = catalog::Catalog::default();
    let mut rows = vec![store::CatalogRun {
        repo_path: "/deleted/a".into(),
        repository: Default::default(),
        status: crate::types::RunStatus::Running,
        red: 1,
        waiting: vec![store::Signoff::Execution("execution-1".parse().unwrap())],
    }];
    rows.push(rows[0].clone());
    catalog.update(&rows, None);
    assert_eq!(catalog.rows[0].badge.waiting.len(), 1);
    assert_eq!(catalog.rows[0].badge.red, 2);
    rows[0].waiting = vec![store::Signoff::Request("original".parse().unwrap())];
    rows[1].waiting = rows[0].waiting.clone();
    catalog.update(&rows, None);
    assert_eq!(catalog.rows[0].badge.waiting.len(), 1);
}

#[tokio::test]
async fn modal_mouse_intercepts_underlying_clicks_and_resize_hit_tests_follow_current_frame() {
    let (view, requests) = super::tests::live();
    let mut monitor = Monitor::new("/fixture-state".into(), None);
    monitor.set_runs(vec![super::tests::summary("run-1", 1)]);
    monitor.set_run(view, requests);
    render(&mut monitor, 200, 45);
    let area = monitor.hits.panes[2];
    let point = (area.x + 5, area.y + 1);
    assert_eq!(
        monitor.mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            point.0,
            point.1
        )),
        Action::None
    );
    assert_eq!(
        monitor.mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            point.0,
            point.1
        )),
        Action::OpenDetail
    );
    monitor.open_detail().await;
    render(&mut monitor, 200, 45);
    let focus = monitor.focus;
    monitor.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 1, 2));
    assert_eq!(monitor.focus, focus);
    let evidence = monitor.hits.modal_panes[1];
    monitor.mouse(mouse(
        MouseEventKind::ScrollDown,
        evidence.x + 1,
        evidence.y + 1,
    ));
    assert_eq!(monitor.modal.as_ref().unwrap().scroll[1], 3);
    render(&mut monitor, 100, 30);
    let close = monitor
        .hits
        .buttons
        .iter()
        .find(|(_, button)| *button == input::Button::Close)
        .unwrap()
        .0;
    monitor.mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        close.x + 1,
        close.y,
    ));
    assert!(monitor.modal.is_none());
    let before = monitor.target();
    monitor.key(KeyEvent::from(KeyCode::F(2)));
    monitor.mouse(mouse(MouseEventKind::ScrollDown, area.x + 2, area.y + 2));
    assert_eq!(monitor.target(), before);
    let tree_area = monitor.hits.panes[2];
    assert!(
        monitor
            .tree
            .rendered_at(Position::new(tree_area.x + 2, tree_area.y + 1))
            .is_some()
    );
}

#[tokio::test]
async fn evidence_distinguishes_never_saved_gc_remote_and_runtime_summary() {
    let root = tempfile::tempdir().unwrap();
    let mut view = super::tests::request(
        "app/check",
        "GREEN",
        json!({"profile":{"kind":"agent","backend":"openai","model":"fixture"},"sessionId":"session-1"}),
    );
    let text = crate::agent::session::live::resolve(root.path(), &view)
        .await
        .unwrap_err();
    assert!(text.contains("identity"));
    view.request.session = Some(crate::agent::session::SessionRef {
        producer: store::Producer::current().name,
        state: "fixture-state".into(),
        run_id: "run-1".parse().unwrap(),
        request_id: view.request.id.clone(),
        session_id: "session-1".parse().unwrap(),
    });
    assert!(
        crate::agent::session::live::resolve(root.path(), &view)
            .await
            .is_err()
    );
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir_all(&repo).unwrap();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    view.request.session.as_mut().unwrap().state = receipts.state_id().await.unwrap();
    use crate::agent::session::{
        document::Position,
        live::{Reader, Resolution},
    };
    let Resolution::Local(source) = crate::agent::session::live::resolve(&state, &view)
        .await
        .unwrap()
    else {
        panic!("local session");
    };
    let mut reader = Reader::new(source.clone());
    assert!(
        reader
            .step(80, 10, Position::Bottom)
            .status
            .unwrap()
            .contains("removed by session GC")
    );
    crate::platform::create_private_dir_all(&crate::agent::session::directory(&state)).unwrap();
    crate::agent::session::live_tests::write(
        &source,
        "{\"kind\":\"review\",\"sessionId\":\"session-1\"}\n",
    );
    crate::agent::session::live_tests::append(
        &source,
        &crate::agent::session::live_tests::answer("saved 한글 conversation"),
    );
    assert!(
        crate::agent::session::live_tests::finish(&mut reader, 80, 20, Position::Bottom)
            .rows
            .join("\n")
            .contains("saved 한글 conversation")
    );
    let runtime = super::tests::request(
        "app/runtime",
        "GREEN",
        json!({"result":{"verdict":"GREEN","exitCode":0,"stdout":"hello","stderr":"warning","truncated":true}}),
    );
    let text = modal::evidence(root.path(), &runtime).text;
    assert!(text.contains("hello") && text.contains("warning") && text.contains("truncated: true"));
    let summary = super::tests::request(
        "app/runtime",
        "GREEN",
        json!({"result":{"verdict":"GREEN","exitCode":0}}),
    );
    assert!(
        modal::evidence(root.path(), &summary)
            .text
            .contains("Logs unavailable")
    );
}

#[tokio::test]
async fn selected_last_page_run_and_artifact_modal_survive_new_runs() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir_all(&repo).unwrap();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    for n in 0..100 {
        let mut run = saved_run(&format!("run-{n}"), &repo, &state, Default::default());
        run.definitions =
            serde_json::from_value(json!({"artifacts":{"artifact":{"path":format!("path-{n}")}}}))
                .unwrap();
        receipts.create_run(&run, &[]).await.unwrap();
    }
    let mut monitor = Monitor::new(state.clone(), None);
    monitor.refresh().await;
    monitor.select_run(99);
    monitor.refresh().await;
    monitor.open_detail().await;
    assert_eq!(
        monitor.modal.as_ref().unwrap().detail.field("Path"),
        Some("path-0")
    );
    receipts
        .create_run(
            &saved_run("run-new", &repo, &state, Default::default()),
            &[],
        )
        .await
        .unwrap();
    monitor.refresh().await;
    assert_eq!(monitor.selected_run().unwrap().id.as_str(), "run-0");
    assert_eq!(
        monitor.modal.as_ref().unwrap().detail.field("Path"),
        Some("path-0")
    );
    assert_eq!(monitor.runs.len(), 101);
}

#[tokio::test]
async fn runtime_modal_refreshes_saved_logs_on_completion_without_resetting_scroll() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir_all(&repo).unwrap();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    let mut run = saved_run("run-1", &repo, &state, Default::default());
    run.status = crate::types::RunStatus::Running;
    run.completed_at = None;
    let mut request = super::tests::request(
        "app/check",
        "RUNNING",
        json!({"startedAt":"2026-01-01T00:00:00Z"}),
    )
    .request;
    request.cwd = repo.clone();
    receipts.create_run(&run, &[request.clone()]).await.unwrap();
    let mut monitor = Monitor::new(state.clone(), None);
    monitor.refresh().await;
    monitor
        .tree
        .select(vec!["a:app".into(), "e:app/check".into()]);
    monitor.open_detail().await;
    assert!(
        monitor
            .modal
            .as_ref()
            .unwrap()
            .evidence
            .text
            .contains("while running")
    );
    monitor.modal.as_mut().unwrap().scroll[1] = 7;
    request.status = crate::types::RequestStatus::Green;
    request.completed_at = Some("2026-01-01T00:00:01Z".into());
    request.result = Some(
        json!({"verdict":"GREEN","stdout":"final output","stderr":"final stderr","exitCode":0,"truncated":false}),
    );
    receipts.save_request(&request).await.unwrap();
    monitor.refresh().await;
    assert!(
        monitor
            .modal
            .as_ref()
            .unwrap()
            .evidence
            .text
            .contains("final output")
    );
    assert_eq!(monitor.modal.as_ref().unwrap().scroll[1], 7);
}

#[tokio::test]
async fn human_modal_routes_focus_paste_buttons_and_ctrl_c_without_losing_drafts() {
    let (view, requests) = super::tests::live();
    let mut monitor = Monitor::new("/fixture-state".into(), None);
    monitor.set_run(view, requests);
    monitor
        .tree
        .select(vec!["a:app".into(), "e:app/review".into()]);
    monitor.open_detail().await;
    let mut review = crate::review::tests::opened(Some("alice"), crate::review::tests::demo());
    review.control(review::Control::Red);
    monitor.modal.as_mut().unwrap().review = Some(review);
    monitor.modal.as_mut().unwrap().focus = ModalPane::Fields;
    monitor.paste("qrg한글");
    let draft = |monitor: &Monitor| match monitor
        .modal
        .as_ref()
        .unwrap()
        .review
        .as_ref()
        .unwrap()
        .mode()
    {
        review::Mode::Form(form) => form.draft(),
        _ => panic!("form"),
    };
    let before = draft(&monitor);
    monitor.key(KeyEvent::from(KeyCode::BackTab));
    assert_eq!(monitor.modal.as_ref().unwrap().focus, ModalPane::Tools);
    monitor.paste("must not modify hidden field");
    assert_eq!(draft(&monitor), before);
    monitor.key(KeyEvent::from(KeyCode::Tab));
    assert_eq!(monitor.modal.as_ref().unwrap().focus, ModalPane::Fields);
    render(&mut monitor, 160, 40);
    for control in [review::Control::Green, review::Control::Red] {
        let area = monitor
            .hits
            .buttons
            .iter()
            .find(|(_, button)| *button == input::Button::Review(control))
            .unwrap()
            .0;
        assert!(matches!(
            monitor.mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                area.x + 1,
                area.y
            )),
            Action::Review(review::Action::None)
        ));
        render(&mut monitor, 160, 40);
    }
    assert_eq!(draft(&monitor), before);
    monitor.key(KeyEvent::from(KeyCode::F(2)));
    monitor.paste(" off");
    assert!(draft(&monitor).contains(" off"));
    assert_eq!(
        monitor.key(KeyEvent::new(
            KeyCode::Char("c".chars().next().unwrap()),
            KeyModifiers::CONTROL
        )),
        Action::Quit
    );
}
