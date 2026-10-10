//! Embedded review exercises the same lifecycle jobs without invoking an editor or provider.
use super::*;
use crate::project::{self, VerifyOptions, selection::Selection};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(&repo).unwrap();
    crate::test_declaration::write(
        repo.join("index.artf"),
        json!({
            "name":"signoff",
            "fingerprint":{},
            "views":{
                "human_tools":{
                    "inspect":{
                        "kind":"output",
                        "description":"Inspect",
                        "command":crate::test_os::bin("true"),
                        "args":[],
                    },
                },
            },
            "evals":[
                {
                    "id":"review",
                    "title":"Approve",
                    "profile":{"kind":"human"},
                    "payload":{"instruction":"Inspect."},
                    "pass_schema":{
                        "type":"object",
                        "properties":{"approved":{"const":true}},
                        "required":["approved"],
                        "additionalProperties":false,
                    },
                    "fail_schema":{
                        "type":"object",
                        "properties":{"reason":{"type":"string"}},
                        "required":["reason"],
                        "additionalProperties":false,
                    },
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
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
        reviewer.parse().unwrap(),
        Some(view.request.id.clone()),
    );
    review.load_single(view);
    review
}
/// A key in the shared Detail component that stays in the Detail.
fn press(review: &mut Review, key: KeyEvent) -> Action {
    match review.key_detail(key) {
        Handled::Action(action) => action,
        other => panic!("the key left the Detail: {other:?}"),
    }
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
    let schema = json!({
        "type":"object",
        "properties":{
            "nested":{"type":"object","properties":{"name":{"type":"string"}}},
            "list":{"type":"array","items":{"type":"string"}},
        },
    });
    let mut form = Form::new(crate::runtime::Verdict::Red, Some(&schema));
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
            press(&mut review, KeyEvent::from(KeyCode::Char(character))),
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
    human::unclaim(&receipts, &view.request.id, &"alice".parse().unwrap())
        .await
        .unwrap();
    human::claim(&receipts, &view.request.id, &"bob".parse().unwrap())
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
    press(&mut review, KeyEvent::from(KeyCode::Char('x')));
    assert_eq!(
        match review.mode() {
            Mode::Form(form) => form.draft(),
            _ => panic!("form"),
        },
        draft
    );
    assert_eq!(
        press(
            &mut review,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)
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
    crate::test_os::create_private_dir_all(&credentials);
    crate::test_os::write_private_file(
        &credentials.join("remote-token.json"),
        json!({"url":url,"token":"fixture-token-never-real"}).to_string(),
    );
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
async fn a_tool_runs_at_once_and_keeps_the_open_draft() {
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
    // No confirmation step: the run job starts at once, and the form stays open meanwhile.
    let outcome = perform(&mut review, Control::RunTool).await;
    assert!(
        matches!(&outcome, Outcome::Ran { result: Ok(result), .. } if !result.is_error),
        "{outcome:?}"
    );
    review.finish_single(outcome);
    assert_eq!(
        match review.mode() {
            Mode::Form(form) => form.draft(),
            _ => panic!("kept form"),
        },
        draft
    );
}

#[tokio::test]
async fn an_external_submission_ends_editing_and_frees_the_detail_keys() {
    let (_root, repo, state) = fixture();
    let view = waiting(&repo, &state).await;
    let mut review = opened(&state, view.clone(), "alice");
    let outcome = perform(&mut review, Control::Claim).await;
    review.finish_single(outcome);
    review.refresh().await;
    review.control(Control::Red);
    review.paste_single("draft");
    assert!(review.editing());
    assert_eq!(
        review.key_detail(KeyEvent::from(KeyCode::Char('q'))),
        Handled::Action(Action::None),
        "q is text in the form"
    );
    // The same reviewer submits from another terminal; the review reloads the request.
    human::submit_and_publish(
        &state,
        view.request.id.as_str(),
        &"alice".parse().unwrap(),
        &json!({"verdict":"GREEN","approved":true}),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    review.refresh().await;
    assert!(review.settled());
    assert!(!review.editing());
    assert_eq!(review.mode(), &Mode::Request);
    review.paste_single("ignored");
    for (code, handled) in [
        (KeyCode::Char('?'), Handled::Pass),
        (KeyCode::Char('!'), Handled::Pass),
        (KeyCode::Left, Handled::Back),
        (KeyCode::Char('q'), Handled::Quit),
        (KeyCode::Esc, Handled::Back),
    ] {
        assert_eq!(review.key_detail(KeyEvent::from(code)), handled, "{code:?}");
    }
}

#[tokio::test]
async fn a_run_after_an_outside_release_neither_claims_nor_runs() {
    let (_root, repo, state) = fixture();
    let view = waiting(&repo, &state).await;
    let id = view.request.id.clone();
    let mut review = opened(&state, view, "alice");
    let outcome = perform(&mut review, Control::Claim).await;
    review.finish_single(outcome);
    review.refresh().await;
    // Another terminal releases the claim before this review has refreshed: the job checks
    // ownership atomically, so the stale view neither claims nor runs.
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    human::unclaim(&receipts, &id, &"alice".parse().unwrap())
        .await
        .unwrap();
    let Action::Start(job) = review.control(Control::RunTool) else {
        panic!("the stale view still starts the job");
    };
    assert!(matches!(&job, Job::Run { claim: false, .. }), "{job:?}");
    let outcome = review.start(job).await;
    assert!(
        matches!(
            &outcome,
            Outcome::Ran {
                result: Err(_),
                claimed: None,
                ..
            }
        ),
        "{outcome:?}"
    );
    review.finish_single(outcome);
    // Once refreshed, Enter asks for a claim instead of starting anything.
    review.refresh().await;
    assert!(!review.owned());
    assert_eq!(review.control(Control::RunTool), Action::None);
    let saved = store::read_request(&state, &id).await.unwrap();
    assert!(saved.claim.is_none(), "running never claims");
}

#[tokio::test]
async fn a_run_after_an_outside_submission_neither_runs_nor_drops_the_draft() {
    let (_root, repo, state) = fixture();
    let view = waiting(&repo, &state).await;
    let id = view.request.id.clone();
    let mut review = opened(&state, view, "alice");
    let outcome = perform(&mut review, Control::Claim).await;
    review.finish_single(outcome);
    review.refresh().await;
    review.control(Control::Red);
    review.paste_single("draft with 한글");
    human::submit_and_publish(
        &state,
        id.as_str(),
        &"alice".parse().unwrap(),
        &json!({"verdict":"GREEN","approved":true}),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    // A run started from the stale view is refused by the settled request.
    let Action::Start(job) = review.control(Control::RunTool) else {
        panic!("the stale view still starts the job");
    };
    let outcome = review.start(job).await;
    assert!(
        matches!(&outcome, Outcome::Ran { result: Err(_), .. }),
        "{outcome:?}"
    );
    review.finish_single(outcome);
    review.refresh().await;
    assert!(review.settled());
    assert_eq!(review.control(Control::RunTool), Action::None);
    assert_eq!(review.mode(), &Mode::Request);
    assert_eq!(
        review.drafts[&crate::runtime::Verdict::Red].fields[0].display(),
        "draft with 한글"
    );
}

#[tokio::test]
async fn a_refresh_keeps_the_quit_prompt_for_claims_still_held() {
    let (_root, repo, state) = fixture();
    let view = waiting(&repo, &state).await;
    let id = view.request.id.clone();
    let mut review = Review::new(
        state.clone(),
        None,
        "alice".parse().unwrap(),
        Some(id.clone()),
    );
    review.refresh().await;
    let Action::Start(job) = review.key(KeyEvent::from(KeyCode::Char('c'))) else {
        panic!("claim job");
    };
    let outcome = review.start(job).await;
    review.finish(outcome);
    review.refresh().await;
    // Another terminal settles it; this session still holds the claim it took on a second one.
    human::submit_and_publish(
        &state,
        id.as_str(),
        &"alice".parse().unwrap(),
        &json!({"verdict":"GREEN","approved":true}),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    review.refresh().await;
    assert!(review.settled());
    review.taken.push("run-held-1".parse().unwrap());
    assert_eq!(
        review.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Action::None
    );
    assert_eq!(review.mode(), &Mode::Leave);
    review.refresh().await;
    assert_eq!(review.mode(), &Mode::Leave, "a refresh keeps the prompt");
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
    let Action::Start(job) = press(&mut review, KeyEvent::from(KeyCode::Char('c'))) else {
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
        crate::runtime::Verdict::Red,
        Some(&json!({
            "type":"object",
            "properties":{
                "a":{
                    "type":"string",
                    "description":"A long hint that wraps over several narrow terminal rows",
                },
                "b":{"type":"string"},
                "c":{"type":"string"},
            },
        })),
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
        crate::runtime::Verdict::Red,
        Some(&json!({"type":"object","properties":{"nested":{"type":"object"}}})),
    );
    form.json = Some(format!("{}한글", "x".repeat(140)));
    form.cursor = form.json.as_ref().unwrap().len();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(32, 8)).unwrap();
    terminal
        .draw(|frame| view::draw_inline_form(frame, frame.area(), &form, 0, true))
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
                review.draw_detail(frame, frame.area(), true);
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
    // Before a claim the instruction is the body: most of the criteria show at once.
    let text = draw(&mut review);
    assert!(
        text.contains("criterion-0") && text.contains("criterion-30"),
        "{text}"
    );
    review.scroll_instruction(15);
    assert!(draw(&mut review).contains("criterion-39"));
    // In REVIEW it folds to two rows above the tools and fields, and keeps its scroll.
    review.request.as_mut().unwrap().claim = Some(super::tests::claim("alice"));
    review.control(Control::Red);
    let text = draw(&mut review);
    assert!(
        text.contains("criterion-15")
            && text.contains("criterion-16")
            && !text.contains("criterion-17")
            && text.contains("RED fields")
            && text.contains("Tools (2)"),
        "{text}"
    );
}

#[test]
fn keyboard_pages_flat_fields_without_mouse_capture() {
    let mut review = super::tests::opened(Some("alice"), super::tests::demo());
    let properties: serde_json::Map<String, Value> = (0..30)
        .map(|n| (format!("field-{n:02}"), json!({"type":"string"})))
        .collect();
    review.mode = Mode::Form(Form::new(
        crate::runtime::Verdict::Red,
        Some(&json!({"type":"object","properties":properties})),
    ));
    press(&mut review, KeyEvent::from(KeyCode::PageDown));
    assert_eq!(review.field_scroll, 10);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
    terminal
        .draw(|frame| {
            review.draw_detail(frame, frame.area(), true);
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
    press(&mut review, KeyEvent::from(KeyCode::PageUp));
    assert_eq!(review.field_scroll, 0);
}

/// The owner's file Artifact sign-off: a FILE Artifact whose launch tool opens the file, and a
/// GREEN schema whose only required property is a `const` without `type`.
fn file_signoff() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    let brand = repo.join("assets/brand");
    std::fs::create_dir_all(&brand).unwrap();
    std::fs::write(brand.join("artifactize-icon.svg"), "<svg/>\n").unwrap();
    let command = serde_json::to_string(&crate::test_os::bin("true")).unwrap();
    let declaration = format!(
        r#"name = "brand-icon"

[views.human_tools]
open = {{ description = "Open the original artwork of {{artifactName}}.", kind = "launch", command = {command}, args = ["{{artifactPath}}"] }}

[evals.approved]
title = "The owner approves this artwork"
profile = {{ kind = "human" }}

[evals.approved.payload]
instruction = "Open {{brand-icon}} with the open tool."

[evals.approved.pass_schema]
type = "object"
properties.approved = {{ const = true }}
properties.comment = {{ type = "string" }}
required = ["approved"]
additionalProperties = false

[evals.approved.fail_schema]
type = "object"
properties.change = {{ type = "array", items = {{ type = "string", minLength = 1 }}, minItems = 1 }}
required = ["change"]
additionalProperties = false
"#
    );
    std::fs::write(brand.join("artifactize-icon.svg.artf"), declaration).unwrap();
    (root, repo, state)
}

fn drawn(review: &mut Review) -> String {
    // The resolved executable and Windows temp paths can exceed a narrow Tools pane.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(400, 40)).unwrap();
    terminal.draw(|frame| review.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let rows = buffer.content().chunks(buffer.area.width as usize);
    let rows = rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>());
    rows.collect::<Vec<_>>().join("\n")
}

async fn job(review: &mut Review, action: Action) -> Outcome {
    let Action::Start(job) = action else {
        panic!("expected lifecycle job, got {action:?}");
    };
    review.start(job).await
}

#[tokio::test]
async fn followers_of_later_runs_review_the_original_with_its_file_tools_and_const_fields() {
    let (_root, repo, state) = file_signoff();
    let original = waiting(&repo, &state).await;
    let mut followers = Vec::new();
    for _ in 0..3 {
        followers.push(waiting(&repo, &state).await);
    }
    for follower in &followers {
        assert_eq!(follower.request.execution_id, original.request.execution_id);
        assert!(follower.request.human_definition.is_none());
    }
    let tool = |review: &Review| {
        review
            .tools()
            .into_iter()
            .map(|tool| (tool.name.to_string(), tool.kind))
            .collect::<Vec<_>>()
    };
    let expected = vec![(
        "open_brand-icon".to_owned(),
        crate::config::HumanToolKind::Launch,
    )];

    // Monitor's Detail resolves a follower to the original before loading the review.
    let monitor = crate::monitor::test_original(&state, &followers[2])
        .await
        .unwrap();
    assert_eq!(monitor.request.id, original.request.id);
    assert_eq!(tool(&opened(&state, monitor, "alice")), expected);

    // The standalone list holds one entry per waiting sign-off, not one per joined Run.
    let mut review = Review::new(
        state.clone(),
        Some(repo.clone()),
        "alice".parse().unwrap(),
        None,
    );
    review.refresh().await;
    let listed: Vec<_> = review
        .waiting
        .iter()
        .map(|view| view.request.id.clone())
        .collect();
    assert_eq!(listed, vec![original.request.id.clone()]);
    assert!(drawn(&mut review).contains("Waiting Human reviews (1)"));

    // Opening a follower by id reviews its original: tools, schemas and actions in one place.
    let mut review = Review::new(
        state.clone(),
        Some(repo.clone()),
        "alice".parse().unwrap(),
        Some(followers[0].request.id.clone()),
    );
    review.refresh().await;
    assert_eq!(review.open.as_ref(), Some(&original.request.id));
    assert_eq!(tool(&review), expected);
    let action = review.control(Control::Claim);
    let outcome = job(&mut review, action).await;
    review.finish(outcome);
    review.refresh().await;
    assert!(review.owned());
    assert!(drawn(&mut review).contains("Tools (1)"));

    // The launch tool resolves {artifactPath} to the file and launches from its folder; the
    // focused Tools pane shows that command, and Enter runs it without a confirmation.
    let Some(Ok(command)) = review.command("open_brand-icon").cloned() else {
        panic!("resolved command");
    };
    let root = crate::test_os::canonical(&repo);
    let file = crate::test_os::canonical(&root.join("assets/brand/artifactize-icon.svg"));
    assert_eq!(command.args, vec![crate::test_os::path_text(&file)]);
    assert_eq!(command.cwd, root.join("assets/brand"));
    press(&mut review, KeyEvent::from(KeyCode::Tab));
    let screen = drawn(&mut review);
    assert!(
        screen.contains(&crate::test_os::path_text(&file)),
        "{screen}"
    );
    let action = press(&mut review, KeyEvent::from(KeyCode::Enter));
    let outcome = job(&mut review, action).await;
    assert!(
        matches!(&outcome, Outcome::Ran { result: Ok(result), .. } if !result.is_error),
        "{outcome:?}"
    );
    review.finish(outcome);

    // The const property is a fixed, prefilled field and part of the submitted result.
    review.control(Control::Green);
    let Mode::Form(form) = review.mode() else {
        panic!("GREEN form");
    };
    let fields: Vec<_> = form
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field.required, field.display()))
        .collect();
    assert_eq!(
        fields,
        vec![
            ("approved", true, "true (fixed)".to_owned()),
            ("comment", false, String::new()),
        ]
    );
    assert_eq!(
        form.result().unwrap(),
        json!({"verdict":"GREEN","approved":true})
    );
    let screen = drawn(&mut review);
    assert!(screen.contains("true (fixed)"), "{screen}");
    assert!(!screen.contains("No owner fields"), "{screen}");
    let action = review.control(Control::Submit);
    let outcome = job(&mut review, action).await;
    assert!(
        matches!(&outcome, Outcome::Submitted { result: Ok(_), .. }),
        "{outcome:?}"
    );
    review.finish(outcome);
    for view in std::iter::once(&original).chain(&followers) {
        let view = store::read_request(&state, &view.request.id).await.unwrap();
        assert_eq!(view.request.status, crate::types::RequestStatus::Green);
        assert_eq!(
            view.request.result.unwrap().owner_fields()["approved"],
            json!(true)
        );
    }
}

#[test]
fn a_request_without_a_recorded_definition_never_offers_an_empty_form() {
    let mut review = super::tests::opened(Some("alice"), super::tests::demo());
    review.request.as_mut().unwrap().request.human_definition = None;
    assert_eq!(review.control(Control::Green), Action::None);
    assert!(!review.editing());
    let screen = drawn(&mut review);
    assert!(
        screen.contains("records no Human definition")
            && screen.contains("Unknown: no Human definition is recorded."),
        "{screen}"
    );
}

#[test]
fn const_properties_are_fixed_fields_with_or_without_a_type() {
    for approved in [
        json!({"const":true}),
        json!({"type":"boolean","const":true,"description":"Approve the artwork."}),
    ] {
        let schema = json!({
            "type":"object",
            "properties":{"approved":approved,"comment":{"type":"string"}},
            "required":["approved"],
            "additionalProperties":false,
        });
        let form = Form::new(crate::runtime::Verdict::Green, Some(&schema));
        assert!(form.json.is_none(), "{schema}");
        assert_eq!(form.fields[0].input, Input::Fixed(json!(true)));
        assert_eq!(form.fields[0].display(), "true (fixed)");
        assert_eq!(
            form.result().unwrap(),
            json!({"verdict":"GREEN","approved":true})
        );
    }
}
