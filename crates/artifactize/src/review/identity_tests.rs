use super::*;
use crossterm::event::{KeyCode, KeyEvent};

#[tokio::test]
async fn compatibility_constructor_parses_open_id_once_and_invalid_input_never_reads_state() {
    let root = crate::test_os::tempdir();
    let state = root.path().join("absent-state");
    // Invalid IDs cannot be supplied to the core constructor.
    assert!("../escape".parse::<RequestId>().is_err());
    assert!(!state.exists());
    let valid = Review::new(
        state,
        None,
        "fixture".parse().unwrap(),
        Some("run-legacy_1.3".parse().unwrap()),
    );
    assert_eq!(valid.open.as_ref().unwrap().as_str(), "run-legacy_1.3");
}

#[test]
fn request_jobs_and_outcomes_retain_typed_ids_while_idle_and_busy_page_steps_stay_ten() {
    let mut review = tests::opened(Some("alice"), tests::demo());
    let id: RequestId = "run-1-3".parse().unwrap();
    assert_eq!(review.actionable().unwrap().0, id);
    assert_eq!(
        review.key(KeyEvent::from(KeyCode::Char('u'))),
        Action::Start(Job::Release {
            ids: vec![id.clone()],
            quit: false
        })
    );
    review.key(KeyEvent::from(KeyCode::PageDown));
    assert_eq!(review.scroll, 10);
    review.key(KeyEvent::from(KeyCode::PageUp));
    assert_eq!(review.scroll, 0);
    review.scroll = u16::MAX - 2;
    review.key(KeyEvent::from(KeyCode::PageDown));
    assert_eq!(review.scroll, u16::MAX);
    drop(review.start(Job::Run {
        id: id.clone(),
        tool: "notes_release".into(),
        claim: false,
    }));
    review.scroll = 5;
    review.key(KeyEvent::from(KeyCode::PageUp));
    assert_eq!(review.scroll, 0);
    review.key(KeyEvent::from(KeyCode::PageDown));
    assert_eq!(review.scroll, 10);
    review.taken = vec![id.clone()];
    review.finish(Outcome::Released {
        ids: vec![id.clone()],
        quit: false,
        failed: vec![(id.clone(), "fixture release error".into())],
    });
    assert_eq!(review.taken, [id]);
}
