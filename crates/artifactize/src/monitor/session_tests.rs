use super::*;
use crate::agent::session::{
    document::{Mode, Move},
    live_tests::{answer, append, fixture},
};

fn complete(live: &mut session::Live) {
    for _ in 0..10000 {
        let Some(mut job) = live.job() else {
            return;
        };
        let window = job
            .reader
            .step_expanded(job.width, job.height, job.position, &job.expanded);
        live.finish(job, window);
    }
    panic!("jobs did not quiesce");
}
fn run_job(live: &mut session::Live) -> (session::Job, crate::agent::session::live::Window) {
    let mut job = live.job().unwrap();
    let window = job
        .reader
        .step_expanded(job.width, job.height, job.position, &job.expanded);
    (job, window)
}
fn history() -> (tempfile::TempDir, session::Live) {
    let (root, source) = fixture();
    for n in 0..600 {
        append(
            &source,
            &answer(&format!(
                "event {n} 한글 e\u{301} ｶﾞ {}",
                "wrapped".repeat(30)
            )),
        );
    }
    let mut live = session::Live::new(1, source);
    live.geometry(80, 10);
    complete(&mut live);
    (root, live)
}
#[test]
fn expanded_activity_remains_open_after_late_completion_and_resize_while_paused() {
    use rig_core::message::{
        AssistantContent, Message, ToolCall, ToolFunction, ToolName, ToolResultContent,
    };
    let (root, source) = fixture();
    let call = ToolCall::from_wire(
        "activity",
        ToolFunction::new(
            ToolName::new("read").unwrap(),
            serde_json::json!({"path":"src/visible.rs"}),
        ),
    );
    let message = serde_json::json!({
        "kind":"message",
        "message":Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call.clone())],
        },
    });
    append(&source, &format!("{message}\n"));
    append(&source, &answer("following prose ".repeat(100).as_str()));
    let mut live = session::Live::new(1, source);
    live.geometry(40, 6);
    complete(&mut live);
    live.movement(Move::Top);
    complete(&mut live);
    let id = live.window.groups[0].1;
    live.toggle(id);
    complete(&mut live);
    assert!(live.expanded(id));
    let anchor = live.window.anchor;
    let result = serde_json::json!({
        "kind":"message",
        "message":Message::tool_results(vec![
            call.result(vec![ToolResultContent::text("hidden output")]),
        ]),
        "isError":[false],
    });
    append(&live.source, &format!("{result}\n"));
    live.invalidate();
    complete(&mut live);
    assert!(live.expanded(id));
    assert_eq!(live.window.anchor.event, anchor.event);
    assert!(live.window.rows.join("\n").contains("visible.rs"));
    assert!(!live.window.rows.join("\n").contains("hidden output"));
    live.geometry(60, 6);
    complete(&mut live);
    assert!(live.expanded(id));
    assert_eq!(live.scroll.mode, Mode::Paused);
    drop(root);
}

#[test]
fn paused_append_keeps_logical_anchor_bottom_resumes_and_resize_reprojects() {
    let (_root, mut live) = history();
    assert_eq!(live.scroll.mode, Mode::Following);
    live.movement(Move::Up(30));
    complete(&mut live);
    let before = live.window.anchor;
    let rows = live.window.rows.clone();
    append(&live.source, &answer("new tail"));
    live.invalidate();
    complete(&mut live);
    assert_eq!(live.window.anchor, before);
    assert_eq!(live.window.rows, rows);
    assert_eq!(live.scroll.mode, Mode::Paused);
    live.geometry(31, 10);
    complete(&mut live);
    assert_eq!(live.window.anchor.event, before.event);
    assert!(live.window.anchor.byte <= before.byte);
    live.movement(Move::Bottom);
    complete(&mut live);
    assert_eq!(live.scroll.mode, Mode::Following);
    assert!(live.window.rows.join("\n").contains("new tail"));
    append(&live.source, &answer("followed tail"));
    live.invalidate();
    complete(&mut live);
    assert!(live.window.rows.join("\n").contains("followed tail"));
}
#[test]
fn queued_scroll_then_resize_and_resize_then_scroll_latest_intent_survives_stale_jobs() {
    let (_root, mut live) = history();
    live.movement(Move::Up(50));
    complete(&mut live);
    let before = live.window.anchor;
    live.movement(Move::Up(1));
    let (job, window) = run_job(&mut live);
    let expected = window.anchor;
    live.geometry(40, 10);
    live.finish(job, window);
    complete(&mut live);
    assert_eq!(live.window.anchor.event, expected.event);
    assert!(live.window.anchor.byte <= expected.byte);
    live.geometry(80, 10);
    complete(&mut live);
    let shown = live.window.anchor;
    live.geometry(40, 10);
    let (job, window) = run_job(&mut live);
    live.movement(Move::Up(1));
    live.finish(job, window);
    live.geometry(23, 10);
    complete(&mut live);
    assert!(live.window.anchor.event <= shown.event);
    assert_eq!(live.scroll.mode, Mode::Paused);
    assert!(before.event > 0);
}
#[test]
fn completed_but_stale_resize_never_replaces_committed_layout_and_home_down_is_bounded() {
    let (_root, mut live) = history();
    live.movement(Move::Up(30));
    complete(&mut live);
    let shown = live.window.anchor;
    live.geometry(40, 10);
    // Finish all intermediate layout jobs except the final one, then make it stale.
    loop {
        let (job, window) = run_job(&mut live);
        if !window.loading {
            live.geometry(100, 10);
            live.movement(Move::Up(1));
            live.finish(job, window);
            break;
        }
        live.finish(job, window);
    }
    complete(&mut live);
    assert!(live.window.status.is_none(), "{:?}", live.window.status);
    assert!(live.window.anchor.event <= shown.event);
    live.movement(Move::Top);
    live.movement(Move::Down(3));
    let before = live.reader.as_ref().unwrap().bytes_read;
    complete(&mut live);
    assert_eq!(live.window.top, 3);
    assert!(live.reader.as_ref().unwrap().bytes_read - before < 2048);
}
#[test]
fn flip_back_partial_widths_do_not_replace_committed_generation() {
    let (_root, mut live) = history();
    live.movement(Move::Up(50));
    complete(&mut live);
    live.movement(Move::Up(1));
    let (job, expected_window) = run_job(&mut live);
    let expected = expected_window.anchor;
    live.finish(job, expected_window);
    live.movement(Move::Down(1));
    complete(&mut live);
    for width in [40, 80, 40] {
        live.geometry(width, 10);
        let (job, window) = run_job(&mut live);
        assert!(window.loading);
        live.finish(job, window);
    }
    live.movement(Move::Up(1));
    complete(&mut live);
    assert!(live.window.status.is_none(), "{:?}", live.window.status);
    assert_eq!(live.window.anchor.event, expected.event);
    assert!(live.window.anchor.byte <= expected.byte);
}

#[test]
fn scrolling_during_large_append_layout_uses_completed_visible_prefix() {
    let (_root, mut live) = history();
    let old = live.window.anchor;
    append(&live.source, &answer(&"x".repeat(2 * 1024 * 1024)));
    live.invalidate();
    loop {
        let (job, window) = run_job(&mut live);
        let decoding_done = job.reader.decoded_events > 601;
        live.finish(job, window);
        if decoding_done {
            break;
        }
    }
    live.movement(Move::Up(1));
    complete(&mut live);
    assert!(live.window.status.is_none(), "{:?}", live.window.status);
    assert!(live.window.anchor.event <= old.event);
    assert_eq!(live.scroll.mode, Mode::Paused);
}

#[test]
fn reset_and_saved_promotion_are_not_lost_when_job_is_stale() {
    let (_root, mut live) = history();
    live.movement(Move::Up(10));
    complete(&mut live);
    crate::agent::session::live_tests::write(
        &live.source,
        "{\"kind\":\"review\"}\n{\"kind\":\"answer\",\"text\":\"replacement\"}\n",
    );
    live.invalidate();
    let (job, window) = run_job(&mut live);
    assert!(window.reset);
    live.movement(Move::Up(1));
    live.finish(job, window);
    complete(&mut live);
    assert_eq!(live.scroll.mode, Mode::Following);
    live.source.saved = false;
    live.invalidate();
    let (job, window) = run_job(&mut live);
    live.source.saved = true;
    live.finish(job, window);
    assert!(live.reader.as_ref().unwrap().source.saved);
}

#[test]
fn keyboard_mouse_page_buttons_capture_and_summary_keep_same_rules() {
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::{Terminal, backend::TestBackend};
    let (root, live) = history();
    let mut monitor = Monitor::new(root.path().into(), None);
    monitor.detail = Some(DetailPane {
        run_id: "run".parse().unwrap(),
        target: Target::Eval("app/check".parse().unwrap()),
        request: None,
        evidence: evidence::Evidence::default(),
        evidence_stamp: None,
        detail: Detail {
            title: "app/check · Title".into(),
            summary: "RUNNING 3s · agent openai m · ← lib".into(),
            fields: vec![("Status", "RUNNING — executing".into())],
        },
        review: None,
        focus: DetailArea::Evidence,
        scroll: [0; 2],
        live: Some(live),
        show_details: false,
        technical: false,
        pending: false,
    });
    monitor.focus = Pane::Detail;
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
    terminal.draw(|frame| monitor.draw(frame)).unwrap();
    complete(monitor.detail.as_mut().unwrap().live.as_mut().unwrap());
    terminal.draw(|frame| monitor.draw(frame)).unwrap();
    let evidence = monitor.hits.areas[1];
    let mouse = |kind| MouseEvent {
        kind,
        column: evidence.x + 1,
        row: evidence.y + 1,
        modifiers: KeyModifiers::empty(),
    };
    let old = monitor
        .detail
        .as_ref()
        .unwrap()
        .live
        .as_ref()
        .unwrap()
        .scroll
        .top;
    monitor.mouse(mouse(MouseEventKind::ScrollUp));
    assert_eq!(
        monitor
            .detail
            .as_ref()
            .unwrap()
            .live
            .as_ref()
            .unwrap()
            .scroll
            .top,
        old - 3
    );
    monitor.key(KeyEvent::from(KeyCode::End));
    assert_eq!(
        monitor
            .detail
            .as_ref()
            .unwrap()
            .live
            .as_ref()
            .unwrap()
            .scroll
            .mode,
        Mode::Following
    );
    monitor.key(KeyEvent::from(KeyCode::Home));
    assert_eq!(
        monitor
            .detail
            .as_ref()
            .unwrap()
            .live
            .as_ref()
            .unwrap()
            .scroll
            .top,
        0
    );
    let button = monitor
        .hits
        .buttons
        .iter()
        .find(|(_, button)| *button == input::Button::SessionBottom)
        .unwrap()
        .0;
    monitor.mouse(MouseEvent {
        column: button.x + 1,
        row: button.y,
        ..mouse(MouseEventKind::Down(MouseButton::Left))
    });
    assert_eq!(
        monitor
            .detail
            .as_ref()
            .unwrap()
            .live
            .as_ref()
            .unwrap()
            .scroll
            .mode,
        Mode::Following
    );
    monitor.key(KeyEvent::from(KeyCode::F(2)));
    let top = monitor
        .detail
        .as_ref()
        .unwrap()
        .live
        .as_ref()
        .unwrap()
        .scroll
        .top;
    monitor.mouse(mouse(MouseEventKind::ScrollUp));
    assert_eq!(
        monitor
            .detail
            .as_ref()
            .unwrap()
            .live
            .as_ref()
            .unwrap()
            .scroll
            .top,
        top
    );
    monitor.key(KeyEvent::from(KeyCode::PageUp));
    let live = monitor.detail.as_ref().unwrap().live.as_ref().unwrap();
    assert_eq!(live.scroll.top, top - live.scroll.height);
    monitor.key(KeyEvent::from(KeyCode::Char('d')));
    assert!(monitor.detail.as_ref().unwrap().show_details);
    let position = monitor
        .detail
        .as_ref()
        .unwrap()
        .live
        .as_ref()
        .unwrap()
        .scroll
        .top;
    monitor.key(KeyEvent::from(KeyCode::Up));
    assert_eq!(
        monitor
            .detail
            .as_ref()
            .unwrap()
            .live
            .as_ref()
            .unwrap()
            .scroll
            .top,
        position
    );
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("following") && text.contains("bottom"));
    // The one-line outcome stays in the title; only a Human review advertises Ctrl-S.
    assert!(text.contains("app/check · Title · RUNNING 3s · agent openai m · ← lib"));
    assert!(!text.contains("Ctrl-S") && text.contains("d details · Esc back"));
    let live = monitor.detail.as_mut().unwrap().live.as_mut().unwrap();
    live.scroll.total = 100000;
    live.scroll.top = live.scroll.bottom();
    live.scroll.mode = Mode::Following;
    let visible_position = format!("{}/100000", live.scroll.top + 1);
    terminal.draw(|frame| monitor.draw(frame)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        text.contains(&visible_position) && text.contains("following") && text.contains("bottom")
    );
    assert!(
        monitor
            .hits
            .buttons
            .iter()
            .any(|(_, button)| *button == input::Button::SessionTop)
    );
    assert!(
        monitor
            .hits
            .buttons
            .iter()
            .any(|(_, button)| *button == input::Button::SessionBottom)
    );
}

#[test]
fn tree_peek_of_a_running_agent_shows_its_last_transcript_line_and_hands_its_reader_on() {
    use ratatui::{Terminal, backend::TestBackend};
    let (root, source) = fixture();
    append(&source, &answer("first thought"));
    append(&source, &answer("Reading reference/cli.md now"));
    let (view, requests) = tests::live();
    let mut monitor = Monitor::new(root.path().into(), None);
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, requests);
    monitor.focus = Pane::Artifacts;
    monitor.tree.select(vec![
        "a:app".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:app/check"
            .parse::<crate::monitor::model::NodeId>()
            .unwrap(),
    ]);
    let id = monitor.selected_request().unwrap().request.id.clone();
    monitor.peek = Some(Peek {
        request: id,
        stamp: evidence::EvidenceStamp::new(monitor.selected_request().unwrap()),
        live: Some(session::Live::new(7, source)),
        text: String::new(),
    });
    let mut terminal = Terminal::new(TestBackend::new(160, 30)).unwrap();
    let screen = |terminal: &Terminal<TestBackend>| {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    };
    terminal.draw(|frame| monitor.draw(frame)).unwrap();
    assert!(screen(&terminal).contains("last: reading the session…"));
    complete(monitor.live_mut().unwrap());
    terminal.draw(|frame| monitor.draw(frame)).unwrap();
    assert!(
        screen(&terminal).contains("last: Reading reference/cli.md now"),
        "{}",
        screen(&terminal)
    );
    // At one pane the peek is hidden and nothing is read for it.
    terminal.backend_mut().resize(90, 30);
    terminal.draw(|frame| monitor.draw(frame)).unwrap();
    assert!(!screen(&terminal).contains("last:"));
}

#[tokio::test]
async fn peek_loads_only_for_a_visible_running_agent_and_drops_when_hidden() {
    let root = crate::test_os::tempdir();
    let (view, mut requests) = tests::live();
    let agent = requests
        .iter_mut()
        .find(|view| view.request.eval_id == "app/check")
        .unwrap();
    agent.request.profile =
        serde_json::from_value(serde_json::json!({"kind":"agent","backend":"openai","model":"m"}))
            .unwrap();
    let mut monitor = Monitor::new(root.path().into(), None);
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, requests);
    monitor.tree.select(vec![
        "a:app".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:app/check"
            .parse::<crate::monitor::model::NodeId>()
            .unwrap(),
    ]);
    monitor.size = ratatui::layout::Rect::new(0, 0, 160, 30);
    monitor.focus = Pane::Runs;
    monitor.sync_peek().await;
    assert!(monitor.peek.is_none(), "no peek without tree focus");
    monitor.focus = Pane::Artifacts;
    monitor.size.width = 90;
    monitor.sync_peek().await;
    assert!(monitor.peek.is_none(), "no peek at one pane");
    monitor.size.width = 160;
    monitor.sync_peek().await;
    let peek = monitor.peek.as_ref().unwrap();
    assert!(
        peek.live.is_none() && !peek.text.is_empty(),
        "{}",
        peek.text
    );
    monitor.tree.select(vec![
        "a:p2".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:p2/check"
            .parse::<crate::monitor::model::NodeId>()
            .unwrap(),
    ]);
    monitor.sync_peek().await;
    assert!(
        monitor.peek.is_none(),
        "only a running Agent has a transcript peek"
    );
}

#[tokio::test]
async fn failed_peeks_resolve_again_after_refresh_and_session_changes_while_readers_stay() {
    use serde_json::json;
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(&repo).unwrap();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    let run: store::Run = serde_json::from_value(json!({"id":"run-1","repoPath":repo,
        "stateDir":state,"status":"RUNNING","createdAt":"2026-01-01T00:00:00Z",
        "selection":{"kind":"all"},"validation":null}))
    .unwrap();
    let mut request = tests::request(
        "app/check",
        "RUNNING",
        json!({"profile":{"kind":"agent","backend":"openai","model":"m"},
            "startedAt":"2026-01-01T00:00:00Z"}),
    )
    .request;
    request.cwd = repo.clone();
    receipts.create_run(&run, &[request.clone()]).await.unwrap();
    let mut monitor = Monitor::new(state.clone(), None);
    monitor.refresh().await;
    monitor.focus = Pane::Artifacts;
    monitor.size = ratatui::layout::Rect::new(0, 0, 160, 30);
    monitor.tree.select(vec![
        "a:app".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:app/check"
            .parse::<crate::monitor::model::NodeId>()
            .unwrap(),
    ]);
    monitor.sync_peek().await;
    let failed = monitor.peek.as_ref().unwrap();
    assert!(failed.live.is_none() && !failed.text.is_empty());
    // A refresh lets the next frame resolve the failed peek again.
    monitor.refresh().await;
    assert!(monitor.peek.is_none());
    monitor.sync_peek().await;
    assert!(monitor.peek.as_ref().unwrap().live.is_none());
    // The session appears: the peek reads it after the refresh that brings it.
    let session = |id: &str| crate::agent::session::SessionRef {
        producer: store::Producer::current().name,
        state: "placeholder".parse().unwrap(),
        run_id: "run-1".parse().unwrap(),
        request_id: request.id.clone(),
        session_id: id.parse().unwrap(),
    };
    let state_id: crate::types::StateId = receipts.state_id().await.unwrap();
    request.session = Some(crate::agent::session::SessionRef {
        state: state_id.clone(),
        ..session("session-1")
    });
    receipts.save_request(&request).await.unwrap();
    monitor.refresh().await;
    monitor.sync_peek().await;
    let serial = monitor.peek.as_ref().unwrap().live.as_ref().unwrap().serial;
    // Unchanged facts keep the reader; nothing is resolved per frame or per refresh.
    monitor.refresh().await;
    monitor.sync_peek().await;
    assert_eq!(
        monitor.peek.as_ref().unwrap().live.as_ref().unwrap().serial,
        serial
    );
    // A replaced session is resolved again and read from the start.
    request.session = Some(crate::agent::session::SessionRef {
        state: state_id.clone(),
        ..session("session-2")
    });
    receipts.save_request(&request).await.unwrap();
    monitor.refresh().await;
    monitor.sync_peek().await;
    let live = monitor.peek.as_ref().unwrap().live.as_ref().unwrap();
    assert_ne!(live.serial, serial);
    assert_eq!(live.source.reference.session_id.as_str(), "session-2");
}
