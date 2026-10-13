//! The standalone review's request search on saved state: the default query and `--repo`, `/`
//! editing with Tab completion, pages, and the read-only Detail of an Agent or settled
//! request with its recorded session.
use super::*;
use crate::agent::session::{Answer, Header, Kind, Recorder, Saving};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;
use std::path::Path;

struct Fixture {
    _root: tempfile::TempDir,
    state: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
}

/// A request of `eval` in Run `run`, saved with `status` and `profile`.
fn request(run: &str, eval: &str, status: &str, profile: serde_json::Value) -> store::Request {
    let mut view = crate::monitor::tests::request(
        eval,
        status,
        json!({"profile":profile,"requestedProfile":profile}),
    );
    view.request.run_id = run.parse().unwrap();
    view.request.id = format!("{run}-{}", eval.replace('/', "-")).parse().unwrap();
    view.request
}

async fn run(state: &Path, repo: &Path, id: &str, requests: &[store::Request]) {
    let receipts = store::Receipts::open(state, repo).await.unwrap();
    let run: store::Run = serde_json::from_value(json!({"id":id,"repoPath":repo,"stateDir":state,
        "status":"RUNNING","createdAt":"2026-01-01T00:00:00Z","selection":{"kind":"all"},
        "validation":null}))
    .unwrap();
    receipts.create_run(&run, requests).await.unwrap();
}

/// alpha waits for a Human sign-off and has a GREEN runtime eval and a running Agent review
/// with a recorded session; beta waits for its own Human sign-off.
async fn fixture() -> Fixture {
    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    let alpha = root.path().join("alpha");
    let beta = root.path().join("beta");
    for repo in [&alpha, &beta] {
        std::fs::create_dir_all(repo).unwrap();
    }
    let human = json!({"kind":"human"});
    let mut agent = request(
        "run-a",
        "app/agent",
        "RUNNING",
        json!({"kind":"agent","backend":"openai","model":"m"}),
    );
    agent.session_id = Some("session-a".parse().unwrap());
    let runtime = crate::monitor::tests::request(
        "app/check",
        "GREEN",
        json!({
            "result":{"verdict":"GREEN","exitCode":0,"stdout":"all good","stderr":""},
        }),
    )
    .request;
    let runtime = store::Request {
        run_id: "run-a".parse().unwrap(),
        id: "run-a-app-check".parse().unwrap(),
        ..runtime
    };
    run(
        &state,
        &alpha,
        "run-a",
        &[
            request("run-a", "release/signoff", "WAITING_HUMAN", human.clone()),
            runtime,
            agent.clone(),
        ],
    )
    .await;
    run(
        &state,
        &beta,
        "run-b",
        &[request("run-b", "docs/signoff", "WAITING_HUMAN", human)],
    )
    .await;
    // The Agent review's saved conversation, as the reviewer records it.
    let receipts = store::Receipts::open(&state, &alpha).await.unwrap();
    let saving = Saving {
        state: state.clone(),
        state_id: receipts.state_id().await.unwrap(),
        producer: store::Producer::current().name,
    };
    let mut recorder = Recorder::new(Some(&saving), &agent, &"session-a".parse().unwrap());
    recorder.start(Header::default());
    recorder.event(Kind::Answer(Answer::Completed(
        "The input looks complete.".into(),
    )));
    Fixture {
        state,
        alpha: crate::workspace::canonical_target(&alpha).unwrap(),
        beta: crate::workspace::canonical_target(&beta).unwrap(),
        _root: root,
    }
}

fn screen(review: &mut Review) -> String {
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| review.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .chunks(buffer.area.width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Press `key` and carry out the refreshes it asks for, as the driver does.
async fn press(review: &mut Review, key: KeyEvent) {
    let mut action = review.key(key);
    while action == Action::Refresh {
        action = review.refresh().await;
    }
}

async fn type_text(review: &mut Review, text: &str) {
    for character in text.chars() {
        press(review, KeyEvent::from(KeyCode::Char(character))).await;
    }
}

fn listed(review: &Review) -> Vec<String> {
    review
        .listed
        .iter()
        .map(|view| view.request.eval_id.to_string())
        .collect()
}

#[tokio::test]
async fn the_list_starts_on_every_repositorys_waiting_human_reviews_and_repo_narrows_it() {
    let fixture = fixture().await;
    let alice = || "alice".parse().unwrap();
    let mut review = Review::new(fixture.state.clone(), None, alice(), None);
    review.refresh().await;
    let mut evals = listed(&review);
    evals.sort();
    assert_eq!(evals, ["docs/signoff", "release/signoff"]);
    let shown = screen(&mut review);
    assert!(
        shown.contains("/ kind:human status:waiting_human"),
        "{shown}"
    );
    assert!(shown.contains("Waiting Human reviews (2)"), "{shown}");

    let mut review = Review::new(
        fixture.state.clone(),
        Some(fixture.beta.clone()),
        alice(),
        None,
    );
    review.refresh().await;
    assert_eq!(listed(&review), ["docs/signoff"]);
    let beta = crate::platform::path_text(&fixture.beta);
    assert!(
        review.query.ends_with(&format!("repo:{beta}")),
        "{}",
        review.query
    );
    let _ = &fixture.alpha;
}

#[tokio::test]
async fn slash_edits_the_query_tab_completes_and_esc_restores_it() {
    let fixture = fixture().await;
    let mut review = Review::new(fixture.state.clone(), None, "alice".parse().unwrap(), None);
    review.refresh().await;
    press(&mut review, KeyEvent::from(KeyCode::Char('/'))).await;
    press(
        &mut review,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    )
    .await;
    // Letters are query text while editing, not list keys (q would quit).
    type_text(&mut review, "kind:ag").await;
    press(&mut review, KeyEvent::from(KeyCode::Tab)).await;
    assert_eq!(review.query, "kind:agent ");
    assert_eq!(listed(&review), ["app/agent"]);
    type_text(&mut review, "st").await;
    press(&mut review, KeyEvent::from(KeyCode::Tab)).await;
    assert_eq!(review.query, "kind:agent status:");
    assert!(
        screen(&mut review).contains("Tab status:"),
        "the footer names the candidates"
    );
    // Values come from what the terms before the last one find, never the half-typed term.
    press(
        &mut review,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    )
    .await;
    type_text(&mut review, "kind:agent eval:").await;
    assert!(
        review.listed.is_empty(),
        "a key without its value lists nothing"
    );
    press(&mut review, KeyEvent::from(KeyCode::Tab)).await;
    assert_eq!(review.query, "kind:agent eval:app/agent ");
    press(
        &mut review,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    )
    .await;
    type_text(&mut review, "kind:agent status:bogus").await;
    let shown = screen(&mut review);
    assert!(shown.contains("Unknown status `bogus`"), "{shown}");
    assert!(review.listed.is_empty());
    press(&mut review, KeyEvent::from(KeyCode::Esc)).await;
    assert_eq!(review.query, search::DEFAULT);
    assert_eq!(review.listed.len(), 2);
    // Enter keeps an edited query and returns the keys to the list.
    press(&mut review, KeyEvent::from(KeyCode::Char('/'))).await;
    type_text(&mut review, " repo:beta").await;
    press(&mut review, KeyEvent::from(KeyCode::Enter)).await;
    assert_eq!(listed(&review), ["docs/signoff"]);
    assert!(review.editing_query.is_none());
}

#[tokio::test]
async fn an_agent_or_settled_request_opens_a_read_only_detail_with_its_evidence() {
    let fixture = fixture().await;
    let mut review = Review::new(fixture.state.clone(), None, "alice".parse().unwrap(), None);
    review.refresh().await;
    press(&mut review, KeyEvent::from(KeyCode::Char('/'))).await;
    press(
        &mut review,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    )
    .await;
    type_text(&mut review, "kind:agent").await;
    press(&mut review, KeyEvent::from(KeyCode::Enter)).await;
    press(&mut review, KeyEvent::from(KeyCode::Enter)).await;
    let record = review.shown.record().expect("a read-only Detail");
    assert_eq!(record.view.request.eval_id.as_str(), "app/agent");
    assert!(
        review.shown.request().is_none(),
        "no Human Detail, claim or form"
    );
    // Read the session as the driver's reader job would.
    screen(&mut review);
    let live = review.record_live().expect("the Agent session");
    let mut job = live.job().expect("a reader job");
    let window = job
        .reader
        .step_expanded(job.width, job.height, job.position, &job.expanded);
    review.record_live().unwrap().finish(job, window);
    let shown = screen(&mut review);
    assert!(shown.contains("read-only"), "{shown}");
    assert!(shown.contains("The input looks complete."), "{shown}");
    // c claims nothing here; Esc goes back to the list.
    press(&mut review, KeyEvent::from(KeyCode::Char('c'))).await;
    assert!(review.taken.is_empty());
    press(&mut review, KeyEvent::from(KeyCode::Esc)).await;
    assert_eq!(review.focus, Focus::List);

    press(&mut review, KeyEvent::from(KeyCode::Char('/'))).await;
    press(
        &mut review,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    )
    .await;
    type_text(&mut review, "status:green").await;
    press(&mut review, KeyEvent::from(KeyCode::Enter)).await;
    press(&mut review, KeyEvent::from(KeyCode::Enter)).await;
    let shown = screen(&mut review);
    assert!(shown.contains("Saved runtime stdout / stderr"), "{shown}");
    assert!(shown.contains("all good"), "{shown}");
}

#[tokio::test]
async fn page_keys_move_through_more_matches_than_one_page() {
    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let requests: Vec<_> = (0..search::PAGE + 5)
        .map(|index| {
            request(
                "run-p",
                &format!("app/check{index:03}"),
                "GREEN",
                json!({"kind":"runtime","command":"true","args":[]}),
            )
        })
        .collect();
    run(&state, &repo, "run-p", &requests).await;
    let mut review = Review::new(state, None, "alice".parse().unwrap(), None);
    review.query = "status:green".into();
    review.filter = review.query.parse();
    review.refresh().await;
    assert_eq!(review.listed.len(), search::PAGE);
    assert!(screen(&mut review).contains("page 1, PgDn more"));
    press(&mut review, KeyEvent::from(KeyCode::PageDown)).await;
    assert_eq!(review.listed.len(), 5);
    assert!(screen(&mut review).contains("page 2"));
    press(&mut review, KeyEvent::from(KeyCode::PageUp)).await;
    assert_eq!(review.listed.len(), search::PAGE);
}

/// A paste is one edit of the search: it searches again from the first page, like typing.
#[tokio::test]
async fn a_paste_into_the_search_requeries_from_the_first_page() {
    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let requests: Vec<_> = (0..search::PAGE + 5)
        .map(|index| {
            request(
                "run-p",
                &format!("app/check{index:03}"),
                "GREEN",
                json!({"kind":"runtime","command":"true","args":[]}),
            )
        })
        .collect();
    run(&state, &repo, "run-p", &requests).await;
    let mut review = Review::new(state, None, "alice".parse().unwrap(), None);
    review.refresh().await;

    // On the first page.
    press(&mut review, KeyEvent::from(KeyCode::Char('/'))).await;
    press(
        &mut review,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    )
    .await;
    let mut action = review.paste("eval:check104\n");
    while action == Action::Refresh {
        action = review.refresh().await;
    }
    assert_eq!(review.query, "eval:check104 ");
    assert_eq!(listed(&review), ["app/check104"]);
    press(&mut review, KeyEvent::from(KeyCode::Enter)).await;

    // On a later page: the pasted term searches from the first page again.
    press(&mut review, KeyEvent::from(KeyCode::Char('/'))).await;
    press(
        &mut review,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    )
    .await;
    type_text(&mut review, "status:green").await;
    press(&mut review, KeyEvent::from(KeyCode::Enter)).await;
    press(&mut review, KeyEvent::from(KeyCode::PageDown)).await;
    assert_eq!(review.page(), 2);
    press(&mut review, KeyEvent::from(KeyCode::Char('/'))).await;
    let mut action = review.paste(" eval:check000");
    while action == Action::Refresh {
        action = review.refresh().await;
    }
    assert_eq!(review.page(), 1);
    assert_eq!(listed(&review), ["app/check000"]);
}
