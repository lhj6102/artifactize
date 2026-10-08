//! Embedded review exercises the same lifecycle jobs without invoking an editor or provider.
use super::*;
use crate::project::{self, VerifyOptions, selection::Selection};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("artifactize.json"), json!({"name":"signoff","fingerprint":{},"views":{"humanTools":{"inspect":{"kind":"output","description":"Inspect","command":crate::test_os::bin("true"),"args":[]}}},"evals":[{"id":"review","title":"Approve","profile":{"kind":"human"},"payload":{"instruction":"Inspect."},"passSchema":{"type":"object","properties":{"approved":{"const":true}},"required":["approved"],"additionalProperties":false},"failSchema":{"type":"object","properties":{"reason":{"type":"string"}},"required":["reason"],"additionalProperties":false}}]}).to_string()).unwrap();
    (root, repo, state)
}
async fn waiting(repo: &std::path::Path, state: &std::path::Path) -> RequestView {
    let run = project::verify(
        repo,
        Some(state),
        &Selection::All,
        &VerifyOptions {
            wait_timeout: Duration::from_millis(1),
            ..Default::default()
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    store::read_request(state, &run.requests[0].id)
        .await
        .unwrap()
}
fn opened(state: &std::path::Path, view: RequestView, reviewer: &str) -> Review {
    let mut review = Review::new(
        state.to_path_buf(),
        None,
        reviewer.into(),
        Some(view.request.id.to_string()),
    );
    review.load_single(view);
    review
}
async fn perform(review: &mut Review, control: Control) -> Outcome {
    let Action::Start(job) = review.control(control) else {
        panic!("expected lifecycle job");
    };
    review.start(job).await
}

#[tokio::test]
async fn explicit_claim_race_follower_dedup_and_release_use_original_request() {
    let (_root, repo, state) = fixture();
    let first = waiting(&repo, &state).await;
    let follower = waiting(&repo, &state).await;
    assert_ne!(first.request.id, follower.request.id);
    assert_eq!(first.request.execution_id, follower.request.execution_id);
    let resolved = crate::monitor::test_original(&state, &follower)
        .await
        .unwrap();
    assert_eq!(resolved.request.id, first.request.id);
    let mut alice = opened(&state, resolved.clone(), "alice");
    let mut bob = opened(&state, resolved, "bob");
    assert_eq!(alice.control(Control::Green), Action::None);
    assert!(!alice.editing(), "CLAIM must precede REVIEW");
    let Action::Start(a) = alice.control(Control::Claim) else {
        panic!("claim job");
    };
    let Action::Start(b) = bob.control(Control::Claim) else {
        panic!("claim job");
    };
    let (a, b) = tokio::join!(alice.start(a), bob.start(b));
    let wins = usize::from(matches!(&a, Outcome::Claimed { result: Ok(_) }))
        + usize::from(matches!(&b, Outcome::Claimed { result: Ok(_) }));
    assert_eq!(wins, 1);
    alice.finish_single(a);
    bob.finish_single(b);
    alice.refresh().await;
    bob.refresh().await;
    let winner = if alice.owned() { &mut alice } else { &mut bob };
    let outcome = perform(winner, Control::Release).await;
    winner.finish_single(outcome);
    winner.refresh().await;
    assert!(!winner.owned());
    let catalog = store::read_catalog(&state).await.unwrap();
    let signoffs: std::collections::BTreeSet<_> =
        catalog.into_iter().flat_map(|run| run.waiting).collect();
    assert_eq!(signoffs.len(), 1);
    let mut reviewer = opened(
        &state,
        crate::monitor::test_original(&state, &follower)
            .await
            .unwrap(),
        "final",
    );
    let outcome = perform(&mut reviewer, Control::Claim).await;
    reviewer.finish_single(outcome);
    reviewer.refresh().await;
    reviewer.control(Control::Green);
    let outcome = perform(&mut reviewer, Control::Submit).await;
    assert!(
        matches!(&outcome, Outcome::Submitted { result: Ok(_), .. }),
        "{outcome:?}"
    );
    reviewer.finish_single(outcome);
    assert!(reviewer.settled());
    assert_eq!(
        store::read_request(&state, &first.request.id)
            .await
            .unwrap()
            .request
            .status,
        crate::types::RequestStatus::Green
    );
    assert_eq!(
        store::read_request(&state, &follower.request.id)
            .await
            .unwrap()
            .request
            .status,
        crate::types::RequestStatus::Green
    );
}

#[test]
fn nested_json_is_utf8_editable_and_enter_is_not_submit() {
    let schema = json!({"type":"object","properties":{"nested":{"type":"object","properties":{"name":{"type":"string"}}},"list":{"type":"array","items":{"type":"string"}}}});
    let mut form = Form::new("RED", Some(&schema));
    form.json = Some(String::new());
    form.cursor = 0;
    form.paste("{\"nested\":{\"name\":\"한글\"},\"list\":[\"é\"]}");
    let valid = form.result().unwrap();
    assert_eq!(valid["nested"]["name"], "한글");
    assert_eq!(valid["list"][0], "é");
    form.inline_key(KeyEvent::from(KeyCode::Enter));
    assert!(form.json.as_ref().unwrap().ends_with('\n'));
    form.inline_key(KeyEvent::from(KeyCode::Backspace));
    form.json = Some("a한é".into());
    form.cursor = "a한é".len();
    form.inline_key(KeyEvent::from(KeyCode::Left));
    form.inline_key(KeyEvent::from(KeyCode::Backspace));
    assert_eq!(form.json.as_deref(), Some("aé"));
    assert_eq!(form.cursor, 1);
    form.inline_key(KeyEvent::from(KeyCode::Delete));
    assert_eq!(form.json.as_deref(), Some("a"));
    let before = form.json.clone();
    form.paste(&"x".repeat(crate::human::MAX_RESULT_BYTES));
    assert_eq!(form.json, before);
    assert!(form.error.is_some());
}

#[tokio::test]
async fn embedded_drafts_survive_refresh_verdict_switch_and_ownership_loss() {
    let (_root, repo, state) = fixture();
    let view = waiting(&repo, &state).await;
    let mut review = opened(&state, view.clone(), "alice");
    let outcome = perform(&mut review, Control::Claim).await;
    review.finish_single(outcome);
    review.refresh().await;
    review.control(Control::Red);
    for character in "qrg한글".chars() {
        assert_eq!(
            review.key_single(KeyEvent::from(KeyCode::Char(character)), false),
            Action::None
        );
    }
    assert_eq!(
        match review.mode() {
            Mode::Form(form) => form.fields[0].display(),
            _ => panic!("form"),
        },
        "qrg한글"
    );
    review.refresh().await;
    review.control(Control::Green);
    review.control(Control::Red);
    assert_eq!(
        match review.mode() {
            Mode::Form(form) => form.fields[0].display(),
            _ => panic!("form"),
        },
        "qrg한글"
    );
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    human::unclaim(&receipts, &view.request.id, "alice")
        .await
        .unwrap();
    human::claim(&receipts, &view.request.id, "bob")
        .await
        .unwrap();
    assert!(
        matches!(review.control(Control::Submit), Action::Start(_)),
        "saved view is stale; ownership is checked atomically by job"
    );
    let outcome = perform(&mut review, Control::Submit).await;
    assert!(matches!(
        &outcome,
        Outcome::Submitted { result: Err(_), .. }
    ));
    review.finish_single(outcome);
    review.refresh().await;
    let draft = match review.mode() {
        Mode::Form(form) => form.draft(),
        _ => panic!("draft retained"),
    };
    review.key_single(KeyEvent::from(KeyCode::Char('x')), false);
    assert_eq!(
        match review.mode() {
            Mode::Form(form) => form.draft(),
            _ => panic!("form"),
        },
        draft
    );
    assert_eq!(
        review.key_single(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            false
        ),
        Action::None
    );
}

#[tokio::test]
async fn remote_failure_after_local_submit_leaves_completed_modal_not_resubmission() {
    let (_root, repo, state) = fixture();
    let view = waiting(&repo, &state).await;
    // No actual credentials: a fake loopback store rejects its fake token after settlement.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let url = format!("http://{address}/");
    std::fs::write(
        state.join("remote.json"),
        json!({"url":url,"share":"summary"}).to_string(),
    )
    .unwrap();
    let credentials = state.join("auth");
    crate::platform::create_private_dir_all(&credentials).unwrap();
    let file = crate::platform::private_options()
        .write(true)
        .create_new(true)
        .open(credentials.join("remote-token.json"))
        .unwrap();
    serde_json::to_writer(file, &json!({"url":url,"token":"fixture-token-never-real"})).unwrap();
    let server = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut connection, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        let _ = connection.read(&mut buffer).await.unwrap();
        connection
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    let mut review = opened(&state, view, "alice");
    let outcome = perform(&mut review, Control::Claim).await;
    review.finish_single(outcome);
    review.refresh().await;
    review.control(Control::Green);
    let outcome = perform(&mut review, Control::Submit).await;
    assert!(
        matches!(&outcome, Outcome::SavedLocally { error, .. } if error.contains("saved locally")),
        "{outcome:?}"
    );
    review.finish_single(outcome);
    assert!(review.settled());
    assert_eq!(review.control(Control::Submit), Action::None);
    server.await.unwrap();
}

#[tokio::test]
async fn tool_confirmation_and_cancel_restore_the_existing_draft() {
    let (_root, repo, state) = fixture();
    let view = waiting(&repo, &state).await;
    let mut review = opened(&state, view, "alice");
    let outcome = perform(&mut review, Control::Claim).await;
    review.finish_single(outcome);
    review.refresh().await;
    review.control(Control::Red);
    review.paste_single("draft with 한글");
    let draft = match review.mode() {
        Mode::Form(form) => form.draft(),
        _ => panic!("form"),
    };
    let outcome = perform(&mut review, Control::RunTool).await;
    review.finish_single(outcome);
    assert!(review.confirming());
    review.key_single(KeyEvent::from(KeyCode::Char('n')), false);
    assert_eq!(
        match review.mode() {
            Mode::Form(form) => form.draft(),
            _ => panic!("restored form"),
        },
        draft
    );
    let outcome = perform(&mut review, Control::RunTool).await;
    review.finish_single(outcome);
    let outcome = perform(&mut review, Control::Confirm).await;
    review.finish_single(outcome);
    assert_eq!(
        match review.mode() {
            Mode::Form(form) => form.draft(),
            _ => panic!("restored form"),
        },
        draft
    );
}

#[tokio::test]
async fn keyboard_reclaim_restores_the_release_draft() {
    let (_root, repo, state) = fixture();
    let view = waiting(&repo, &state).await;
    let mut review = opened(&state, view, "alice");
    let outcome = perform(&mut review, Control::Claim).await;
    review.finish_single(outcome);
    review.refresh().await;
    review.control(Control::Red);
    review.paste_single("preserved reason");
    let outcome = perform(&mut review, Control::Release).await;
    review.finish_single(outcome);
    review.refresh().await;
    let Action::Start(job) = review.key_single(KeyEvent::from(KeyCode::Char('c')), false) else {
        panic!("keyboard claim");
    };
    let outcome = review.start(job).await;
    review.finish_single(outcome);
    review.refresh().await;
    review.control(Control::Red);
    assert_eq!(
        match review.mode() {
            Mode::Form(form) => form.fields[0].display(),
            _ => panic!("form restored"),
        },
        "preserved reason"
    );
}

#[test]
fn wrapped_flat_fields_share_their_actual_rendered_hit_rows() {
    let form = Form::new(
        "RED",
        Some(
            &json!({"type":"object","properties":{"a":{"type":"string","description":"A long hint that wraps over several narrow terminal rows"},"b":{"type":"string"},"c":{"type":"string"}}}),
        ),
    );
    let area = ratatui::layout::Rect::new(5, 4, 22, 20);
    let hits = view::field_hits(area, &form, 0);
    assert!(hits[0].0.height > 1);
    let first = hits[0].0;
    assert_eq!(
        hits.iter()
            .find(
                |(rect, _)| rect.contains(ratatui::layout::Position::new(first.x + 1, first.y + 1))
            )
            .unwrap()
            .1,
        0
    );
    assert_eq!(hits[1].0.y, hits[0].0.bottom());
    let scrolled = view::field_hits(area, &form, 2);
    assert_eq!(scrolled[0].1, 0);
    assert_eq!(scrolled[0].0.y, area.y + 1);
    let resized = view::field_hits(ratatui::layout::Rect::new(5, 4, 100, 20), &form, 0);
    assert_eq!(resized[0].0.height, 1);
}

#[test]
fn long_json_line_keeps_unicode_cursor_visible() {
    let mut form = Form::new(
        "RED",
        Some(&json!({"type":"object","properties":{"nested":{"type":"object"}}})),
    );
    form.json = Some(format!("{}한글", "x".repeat(140)));
    form.cursor = form.json.as_ref().unwrap().len();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(32, 8)).unwrap();
    terminal
        .draw(|frame| view::draw_inline_form(frame, frame.area(), &form, 0))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        text.contains('한') && text.contains('글') && text.contains('▏'),
        "{text}"
    );
}

#[test]
fn human_instruction_is_visible_and_scrollable_before_and_after_claim() {
    let mut review = super::tests::opened(None, super::tests::demo());
    let instruction = (0..40)
        .map(|n| format!("criterion-{n}\n"))
        .collect::<String>();
    review.request.as_mut().unwrap().request.payload =
        serde_json::from_value(json!({"instruction":instruction})).unwrap();
    let draw = |review: &mut Review| {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
        terminal
            .draw(|frame| {
                review.draw_single(frame, frame.area(), false);
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    };
    assert!(draw(&mut review).contains("criterion-0"));
    review.scroll_instruction(15);
    assert!(draw(&mut review).contains("criterion-15"));
    review.request.as_mut().unwrap().claim = Some(super::tests::claim("alice"));
    review.control(Control::Red);
    let text = draw(&mut review);
    assert!(
        text.contains("criterion-15")
            && text.contains("RED fields")
            && text.contains("Human tools")
    );
}

#[test]
fn keyboard_pages_flat_fields_without_mouse_capture() {
    let mut review = super::tests::opened(Some("alice"), super::tests::demo());
    let properties: serde_json::Map<String, Value> = (0..30)
        .map(|n| (format!("field-{n:02}"), json!({"type":"string"})))
        .collect();
    review.mode = Mode::Form(Form::new(
        "RED",
        Some(&json!({"type":"object","properties":properties})),
    ));
    review.key_single(KeyEvent::from(KeyCode::PageDown), false);
    assert_eq!(review.field_scroll, 10);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
    terminal
        .draw(|frame| {
            review.draw_single(frame, frame.area(), false);
        })
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("field-10"), "{text}");
    review.key_single(KeyEvent::from(KeyCode::PageUp), false);
    assert_eq!(review.field_scroll, 0);
}
