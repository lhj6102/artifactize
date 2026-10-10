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
fn render_text(monitor: &mut Monitor, width: u16, height: u16) -> String {
    super::tests::sized(monitor, width, height)
}
fn saved_run(
    id: &str,
    repo: &Path,
    state: &Path,
    identity: crate::repository::Identity,
) -> store::Run {
    let mut run: store::Run = serde_json::from_value(json!({
        "id":id,
        "repoPath":repo,
        "stateDir":state,
        "status":"GREEN",
        "createdAt":"2026-01-01T00:00:00Z",
        "completedAt":"2026-01-01T00:00:01Z",
        "selection":{"kind":"all"},
        "validation":null,
    }))
    .unwrap();
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
    let base = crate::test_os::canonical(root.path());
    let alpha = base.join("alpha");
    let beta = base.join("beta");
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
    assert_eq!(
        store::state_schema(&state).unwrap(),
        Some(store::STATE_SCHEMA_VERSION)
    );
    fs::remove_dir_all(&beta).unwrap();
    monitor.refresh().await;
    assert_eq!(monitor.selected_run().unwrap().id.as_str(), "run-old-beta");
}

#[tokio::test]
async fn git_subdirectory_initial_selection_preserves_workspace_and_discovers_empty_worktrees() {
    let root = tempfile::tempdir().unwrap();
    let base = crate::test_os::canonical(root.path());
    let repo = base.join("repo");
    // Windows forbids newline in file names; keep whitespace coverage with a space.
    let other = base.join(if cfg!(windows) {
        "other space"
    } else {
        "other space\nline"
    });
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
    let empty = base.join("empty");
    git(
        &repo,
        &["worktree", "add", "-b", "empty", empty.to_str().unwrap()],
    );
    let mut fresh = Monitor::new(state.clone(), None);
    fresh.refresh().await;
    // Worktrees without Runs fold into one row until Space shows them.
    let listed = |monitor: &Monitor| {
        monitor
            .catalog
            .rows
            .iter()
            .any(|row| matches!(&row.scope, Scope::Worktree(_, path) if path == &empty))
    };
    assert!(!listed(&fresh));
    let fold = fresh
        .catalog
        .rows
        .iter()
        .position(|row| row.fold.is_some())
        .unwrap();
    assert_eq!(fresh.catalog.rows[fold].label, "+1 without Runs (Space)");
    fresh.focus = Pane::Repositories;
    assert_eq!(
        fresh.select_scope(fold),
        Action::None,
        "a fold row is no scope"
    );
    assert_eq!(
        fresh.key(KeyEvent::from(KeyCode::Char(' '))),
        Action::Refresh
    );
    fresh.refresh().await;
    assert!(listed(&fresh));
    assert_eq!(
        fresh.catalog.rows[fold + 1].label,
        "− hide 1 without Runs (Space)"
    );
    assert_eq!(
        fresh.repositories.selected(),
        Some(fold + 1),
        "the fold row stays selected"
    );
    // The repository row is the whole repository; there is no separate `ALL` row below it.
    assert_eq!(
        fresh
            .catalog
            .rows
            .iter()
            .filter(|row| row.label == "ALL")
            .count(),
        1
    );
    assert!(
        fresh.catalog.rows.iter().any(|row| matches!(
            &row.scope,
            Scope::Worktree(Repository::Workspace(_), path) if path == &other
        )),
        "deleted legacy paths must not be guessed into a Git group"
    );
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
        error: 0,
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
    monitor.focus = Pane::Artifacts;
    render(&mut monitor, 200, 45);
    let area = monitor.hits.tree;
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
    assert_eq!(monitor.focus, Pane::Detail);
    let sections = monitor.hits.areas[0];
    monitor.mouse(mouse(
        MouseEventKind::ScrollDown,
        sections.x + 1,
        sections.y + 1,
    ));
    assert_eq!(monitor.detail.as_ref().unwrap().scroll[0], 3);
    // The wheel over the tree scrolls it without taking focus from Detail.
    let tree = monitor.hits.tree;
    monitor.mouse(mouse(MouseEventKind::ScrollDown, tree.x + 2, tree.y + 1));
    assert_eq!(monitor.focus, Pane::Detail);
    assert!(monitor.detail.is_some());
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
    assert!(monitor.detail.is_none());
    assert_eq!(monitor.focus, Pane::Artifacts);
    let before = monitor.target();
    monitor.key(KeyEvent::from(KeyCode::F(2)));
    monitor.mouse(mouse(MouseEventKind::ScrollDown, area.x + 2, area.y + 2));
    assert_eq!(monitor.target(), before);
    monitor.key(KeyEvent::from(KeyCode::F(2)));
    render(&mut monitor, 100, 30);
    let tree_area = monitor.hits.tree;
    assert!(
        monitor
            .tree
            .rendered_at(Position::new(tree_area.x + 2, tree_area.y + 1))
            .is_some()
    );
    // A click on the Runs pane moves focus there; the wheel elsewhere never does.
    monitor.focus = Pane::Runs;
    render(&mut monitor, 160, 30);
    let tree = monitor.hits.tree;
    monitor.mouse(mouse(MouseEventKind::ScrollDown, tree.x + 1, tree.y + 1));
    assert_eq!(monitor.focus, Pane::Runs);
    monitor.mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        tree.x + 2,
        tree.y + 1,
    ));
    assert_eq!(monitor.focus, Pane::Artifacts);
}

#[tokio::test]
async fn evidence_distinguishes_never_saved_gc_remote_and_runtime_summary() {
    let root = tempfile::tempdir().unwrap();
    let mut view = super::tests::request(
        "app/check",
        "GREEN",
        json!({
            "profile":{"kind":"agent","backend":"openai","model":"fixture"},
            "sessionId":"session-1",
        }),
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
    view.request.session.as_mut().unwrap().state = receipts.state_id().await.unwrap().to_string();
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
        json!({
            "result":{
                "verdict":"GREEN",
                "exitCode":0,
                "stdout":"hello",
                "stderr":"warning",
                "truncated":true,
            },
        }),
    );
    let text = evidence::evidence(root.path(), &runtime).text;
    assert!(text.contains("hello") && text.contains("warning") && text.contains("truncated: true"));
    let summary = super::tests::request(
        "app/runtime",
        "GREEN",
        json!({"result":{"verdict":"GREEN","exitCode":0}}),
    );
    assert!(
        evidence::evidence(root.path(), &summary)
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
        monitor.detail.as_ref().unwrap().detail.field("Path"),
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
        monitor.detail.as_ref().unwrap().detail.field("Path"),
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
            .detail
            .as_ref()
            .unwrap()
            .evidence
            .text
            .contains("while running")
    );
    monitor.detail.as_mut().unwrap().scroll[1] = 7;
    request.status = crate::types::RequestStatus::Green;
    request.completed_at = Some("2026-01-01T00:00:01Z".parse().unwrap());
    request.result = Some(json!({
        "verdict":"GREEN",
        "stdout":"final output",
        "stderr":"final stderr",
        "exitCode":0,
        "truncated":false,
    }));
    receipts.save_request(&request).await.unwrap();
    monitor.refresh().await;
    assert!(
        monitor
            .detail
            .as_ref()
            .unwrap()
            .evidence
            .text
            .contains("final output")
    );
    assert_eq!(monitor.detail.as_ref().unwrap().scroll[1], 7);
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
    monitor.detail.as_mut().unwrap().review = Some(review);
    monitor.paste("qrg한글");
    let draft = |monitor: &Monitor| match monitor
        .detail
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
    assert_eq!(
        monitor
            .detail
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .area(),
        review::Area::Tools
    );
    monitor.paste("must not modify hidden field");
    assert_eq!(draft(&monitor), before);
    monitor.key(KeyEvent::from(KeyCode::Tab));
    assert_eq!(
        monitor
            .detail
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .area(),
        review::Area::Fields
    );
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
    // While a form is edited, a click on the tree cannot move focus out of Detail.
    let tree = monitor.hits.tree;
    monitor.mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        tree.x + 2,
        tree.y + 1,
    ));
    assert_eq!(monitor.focus, Pane::Detail);
    // Human keys are text here: q, ? and ! never quit, open help or move the tree.
    for key in ['q', '?', '!'] {
        monitor.key(KeyEvent::from(KeyCode::Char(key)));
        assert_eq!(monitor.focus, Pane::Detail);
        assert!(!monitor.help);
    }
    let before = draft(&monitor);
    // Ctrl-S submits only inside a Human review, and the keys line says so only there.
    assert!(render_text(&mut monitor, 160, 40).contains("Ctrl-S submit"));
    // Esc first stops editing and stays in Detail; the next Esc steps back to the tree.
    monitor.key(KeyEvent::from(KeyCode::Esc));
    assert_eq!(monitor.focus, Pane::Detail);
    assert!(
        !monitor.locked(),
        "focus may move once the form is set aside"
    );
    monitor.key(KeyEvent::from(KeyCode::Esc));
    assert_eq!(monitor.focus, Pane::Artifacts);
    assert!(monitor.detail.is_none());
    assert!(!render_text(&mut monitor, 160, 40).contains("Ctrl-S"));
    // The draft survives for the next open: the same verdict brings it back.
    let id = monitor.reviews.keys().next().unwrap().clone();
    let mut kept = monitor.reviews.remove(&id).unwrap();
    assert_eq!(kept.mode(), &review::Mode::Request);
    kept.control(review::Control::Red);
    match kept.mode() {
        review::Mode::Form(form) => assert_eq!(form.draft(), before),
        _ => panic!("form"),
    }
    monitor.detail = None;
    monitor.reviews.clear();
    monitor
        .tree
        .select(vec!["a:app".into(), "e:app/review".into()]);
    monitor.open_detail().await;
    let mut review = crate::review::tests::opened(Some("alice"), crate::review::tests::demo());
    review.control(review::Control::Red);
    monitor.detail.as_mut().unwrap().review = Some(review);
    assert_eq!(monitor.focus, Pane::Detail);
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

/// Open the tree's waiting Human eval in Detail with this review in place of the saved one.
async fn human_detail(review: review::Review) -> Monitor {
    let (view, requests) = super::tests::live();
    let mut monitor = Monitor::new("/fixture-state".into(), None);
    monitor.set_run(view, requests);
    monitor
        .tree
        .select(vec!["a:app".into(), "e:app/review".into()]);
    monitor.open_detail().await;
    monitor.detail.as_mut().unwrap().review = Some(review);
    monitor.notice = None;
    monitor
}

/// Columns `from..` of every row but the header and the key line.
fn region(text: &str, from: usize) -> Vec<String> {
    let rows: Vec<&str> = text.lines().collect();
    rows[1..rows.len() - 1]
        .iter()
        .map(|row| row.chars().skip(from).collect())
        .collect()
}

#[tokio::test]
async fn monitor_and_review_draw_the_same_human_review_component() {
    use crate::review::tests::{demo, embedded, opened, sized};
    for owner in [None, Some("alice")] {
        let mut standalone = opened(owner, demo());
        let mut inside = embedded(owner, demo());
        if owner.is_some() {
            for review in [&mut standalone, &mut inside] {
                review.control(review::Control::Red);
                review.paste_single("needs work");
            }
        }
        let mut monitor = human_detail(inside).await;
        let monitor_text = render_text(&mut monitor, 160, 40);
        let review_text = sized(&mut standalone, 160, 40);
        // Both put a 30-column list or tree beside the same 130-column Detail.
        let detail = region(&monitor_text, 30);
        assert_eq!(
            detail,
            region(&review_text, 30),
            "{monitor_text}\n{review_text}"
        );
        let stage = if owner.is_some() {
            "REVIEW (yours)"
        } else {
            "CLAIM"
        };
        assert!(detail[0].contains(stage), "{monitor_text}");
        assert!(
            monitor_text.contains("Approve the notes."),
            "{monitor_text}"
        );
    }
}

#[tokio::test]
async fn human_detail_keys_follow_the_shared_protocol() {
    use crate::review::tests::{demo, embedded};
    let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    let mut monitor = human_detail(embedded(None, demo())).await;
    let review = |monitor: &Monitor| {
        monitor
            .detail
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .mode()
            .clone()
    };
    // Before a claim, Ctrl-S, Ctrl-G and Ctrl-R start nothing and open no form.
    for c in ['s', 'g', 'r'] {
        assert_eq!(monitor.key(ctrl(c)), Action::Review(review::Action::None));
        assert_eq!(review(&monitor), review::Mode::Request);
    }
    assert!(render_text(&mut monitor, 160, 40).contains("Claim this request before reviewing it."));
    // c claims here, in the focused Detail.
    assert!(matches!(
        monitor.key(KeyEvent::from(KeyCode::Char('c'))),
        Action::Review(review::Action::Start(review::Job::Claim { .. }))
    ));
    // Tab focuses the tools, and a click on a tool's row selects it.
    monitor.key(KeyEvent::from(KeyCode::Tab));
    render(&mut monitor, 160, 40);
    let (row, index) = *monitor.hits.review.tool_rows.last().unwrap();
    assert_eq!(index, 1);
    monitor.mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        row.x + 1,
        row.y,
    ));
    assert!(render_text(&mut monitor, 160, 40).contains("$ xdg-open {artifactPath}"));
    // The wheel over the instruction scrolls it without moving focus.
    let instruction = monitor.hits.review.instruction;
    monitor.mouse(mouse(
        MouseEventKind::ScrollDown,
        instruction.x + 2,
        instruction.y + 1,
    ));
    assert_eq!(monitor.focus, Pane::Detail);
    // Outside a form, q quits and Left steps back to the tree.
    assert_eq!(
        monitor.key(KeyEvent::from(KeyCode::Char('q'))),
        Action::Quit
    );
    assert_eq!(monitor.key(KeyEvent::from(KeyCode::Left)), Action::None);
    assert_eq!(monitor.focus, Pane::Artifacts);
    assert!(monitor.detail.is_none());
}

#[tokio::test]
async fn human_detail_names_builtin_actions_at_80_and_120_columns() {
    use crate::review::tests::{builtin_rows, builtins, drawn_row, embedded};
    for width in [80, 120] {
        let mut monitor = human_detail(embedded(Some("alice"), builtins())).await;
        monitor.key(KeyEvent::from(KeyCode::Tab));
        for rows in builtin_rows(width) {
            let text = render_text(&mut monitor, width, 30);
            for row in rows {
                assert!(drawn_row(&text, row), "{width}: {row}\n{text}");
            }
            monitor.key(KeyEvent::from(KeyCode::Down));
        }
    }
}

#[tokio::test]
async fn human_detail_names_the_tools_and_a_click_on_the_selected_tool_runs_it() {
    use crate::review::tests::{demo, embedded};
    let mut monitor = human_detail(embedded(Some("alice"), demo())).await;
    let text = render_text(&mut monitor, 160, 40);
    assert!(text.contains("Tools (2) · Tab "), "{text}");
    assert!(text.contains("Tab tools, Enter run"), "{text}");
    // The first click focuses the tools and selects the row; the next one runs it at once.
    let (row, index) = *monitor.hits.review.tool_rows.last().unwrap();
    assert_eq!(index, 1);
    let click = |monitor: &mut Monitor, row: Rect| {
        monitor.mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            row.x + 1,
            row.y,
        ))
    };
    assert_eq!(
        click(&mut monitor, row),
        Action::Review(review::Action::None)
    );
    let text = render_text(&mut monitor, 160, 40);
    assert!(text.contains("↑↓ tool · Enter run · Tab fields"), "{text}");
    let (row, _) = *monitor
        .hits
        .review
        .tool_rows
        .iter()
        .find(|(_, index)| *index == 1)
        .unwrap();
    assert!(matches!(
        click(&mut monitor, row),
        Action::Review(review::Action::Start(review::Job::Run { ref tool, claim: false, .. }))
            if tool == "open_release"
    ));
    // Enter runs the selected tool too, with no confirmation step.
    assert!(matches!(
        monitor.key(KeyEvent::from(KeyCode::Enter)),
        Action::Review(review::Action::Start(review::Job::Run { .. }))
    ));
}

#[tokio::test]
async fn a_review_settled_while_editing_gives_its_keys_back_to_monitor() {
    use crate::review::tests::settled_while_editing;
    let mut monitor = human_detail(settled_while_editing()).await;
    assert!(!monitor.locked());
    let text = render_text(&mut monitor, 160, 40);
    assert!(text.contains("Completed result"), "{text}");
    assert!(!text.contains("Ctrl-S submit"), "{text}");
    assert_eq!(
        monitor.key(KeyEvent::from(KeyCode::Char('?'))),
        Action::None
    );
    assert!(monitor.help);
    monitor.key(KeyEvent::from(KeyCode::Char('x')));
    assert_eq!(
        monitor.key(KeyEvent::from(KeyCode::Char('!'))),
        Action::OpenDetail
    );
    assert!(monitor.detail.is_none());
    let mut monitor = human_detail(settled_while_editing()).await;
    assert_eq!(monitor.key(KeyEvent::from(KeyCode::Left)), Action::None);
    assert_eq!(monitor.focus, Pane::Artifacts);
    let mut monitor = human_detail(settled_while_editing()).await;
    assert_eq!(
        monitor.key(KeyEvent::from(KeyCode::Char('q'))),
        Action::Quit
    );
}

/// The coordinator's end-to-end repro: a real Human request is claimed, edited in the monitor's
/// form, then submitted by another command under the same claimant.
#[tokio::test]
async fn an_external_submission_frees_the_keys_and_focus_of_an_edited_human_detail() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    fs::write(
        repo.join("index.artf"),
        "name = \"app\"\n[evals.review]\ntitle = \"Review\"\nprofile = { kind = \"human\" }\n\
         [evals.review.payload]\ninstruction = \"Review\"\n\
         [evals.review.pass_schema]\nproperties.note = { type = \"string\" }\n",
    )
    .unwrap();
    let options = crate::project::VerifyOptions {
        wait_timeout: std::time::Duration::from_millis(1),
        ..Default::default()
    };
    let run = crate::project::verify(
        &repo,
        Some(&state),
        &crate::project::selection::Selection::All,
        &options,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let id = run.requests[0].id.clone();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    let reviewer = crate::human::default_reviewer().unwrap_or_else(|_| "fixture".into());
    crate::human::claim(&receipts, id.as_str(), &reviewer)
        .await
        .unwrap();
    let mut monitor = Monitor::new(state.clone(), Some(repo.clone()));
    monitor.refresh().await;
    monitor.focus = Pane::Artifacts;
    render(&mut monitor, 160, 30);
    let open = |monitor: &mut Monitor| {
        // Without USER in the environment, open the same request under the fixture reviewer.
        if monitor.detail.as_ref().unwrap().review.is_none() {
            let (_, requests) = monitor.run.as_ref().unwrap();
            let view = requests[0].clone();
            let mut review =
                review::Review::new(state.clone(), None, reviewer.clone(), Some(id.to_string()));
            review.load_single(view);
            monitor.detail.as_mut().unwrap().review = Some(review);
        }
    };
    monitor.open_detail().await;
    open(&mut monitor);
    monitor.key(KeyEvent::from(KeyCode::Char('g')));
    monitor.paste("real-draft");
    assert!(render_text(&mut monitor, 160, 30).contains("real-draft"));
    assert!(monitor.locked());
    let click = |monitor: &mut Monitor| {
        let tree = monitor.hits.tree;
        monitor.mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            tree.x + 2,
            tree.y + 1,
        ));
    };
    click(&mut monitor);
    assert_eq!(monitor.focus, Pane::Detail, "an active form keeps focus");

    crate::human::submit(
        &receipts,
        id.as_str(),
        &reviewer,
        &json!({"verdict":"GREEN","note":"outside"}),
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    monitor.refresh().await;
    let text = render_text(&mut monitor, 160, 30);
    assert!(text.contains("Completed result"), "{text}");
    assert!(
        store::read_request(&state, id.as_str())
            .await
            .unwrap()
            .claim
            .is_none()
    );
    // Neither the form nor the focus lock outlives the review.
    assert!(!monitor.locked());
    monitor.key(KeyEvent::from(KeyCode::Char('?')));
    assert!(monitor.help);
    monitor.key(KeyEvent::from(KeyCode::Char('x')));
    monitor.key(KeyEvent::from(KeyCode::Char('!')));
    assert!(monitor.detail.is_none(), "! leaves the Detail");
    assert_eq!(monitor.focus, Pane::Artifacts);
    monitor.open_detail().await;
    open(&mut monitor);
    assert_eq!(monitor.key(KeyEvent::from(KeyCode::Left)), Action::None);
    assert_eq!(monitor.focus, Pane::Artifacts);
    monitor.open_detail().await;
    open(&mut monitor);
    assert_eq!(
        monitor.key(KeyEvent::from(KeyCode::Char('q'))),
        Action::Quit
    );
    // A click outside leaves the settled Detail.
    render(&mut monitor, 160, 30);
    click(&mut monitor);
    assert_eq!(monitor.focus, Pane::Artifacts);
    assert!(monitor.detail.is_none());
}

#[tokio::test]
async fn help_and_next_attention_reach_monitor_from_idle_human_details() {
    use crate::review::tests::{demo, embedded, settled};
    // CLAIM, read-only and completed: no field is edited, so ? and ! are monitor keys.
    for review in [
        embedded(None, demo()),
        embedded(Some("bob"), demo()),
        settled("GREEN"),
    ] {
        let mut monitor = human_detail(review).await;
        assert_eq!(
            monitor.key(KeyEvent::from(KeyCode::Char('?'))),
            Action::None
        );
        assert!(monitor.help);
        assert!(render_text(&mut monitor, 160, 40).contains("Keys · any key closes"));
        monitor.key(KeyEvent::from(KeyCode::Char('x')));
        assert!(!monitor.help);
        assert_eq!(monitor.focus, Pane::Detail);
        // ! leaves through the draft-keeping path and opens the next attention eval.
        assert_eq!(
            monitor.key(KeyEvent::from(KeyCode::Char('!'))),
            Action::OpenDetail
        );
        assert!(monitor.detail.is_none());
        assert_eq!(
            monitor.reviews.len(),
            1,
            "the review is kept for the next open"
        );
        assert_eq!(monitor.target(), Some(Target::Eval("p2/check".into())));
    }
}
