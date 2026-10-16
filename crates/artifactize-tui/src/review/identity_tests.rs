use super::*;
use crossterm::event::{KeyCode, KeyEvent};

#[test]
fn constructor_keeps_the_typed_open_id_without_creating_state() {
    let root = crate::test_os::tempdir();
    let state = root.path().join("absent-state");
    let review = Review::new(
        state.clone(),
        None,
        "fixture".parse().unwrap(),
        Some("run-legacy_1.3".parse().unwrap()),
    );
    assert_eq!(review.open.as_ref().unwrap().as_str(), "run-legacy_1.3");
    assert_eq!(review.focus(), Focus::Detail);
    assert!(
        !state.exists(),
        "constructing a screen does not create saved state"
    );
}

#[test]
fn a_failed_release_keeps_the_original_request_in_the_quit_action() {
    let mut review = tests::opened(None, tests::demo());
    let id: RequestId = "run-1-3".parse().unwrap();
    assert_eq!(
        review.key(KeyEvent::from(KeyCode::Char('c'))),
        Action::Start(Job::Claim { id: id.clone() })
    );
    review.finish(Outcome::Claimed {
        result: Ok(tests::claim("alice")),
    });
    assert_eq!(
        review.key(KeyEvent::from(KeyCode::Char('u'))),
        Action::Start(Job::Release {
            ids: vec![id.clone()],
            quit: false
        })
    );
    review.finish(Outcome::Released {
        ids: vec![id.clone()],
        quit: false,
        failed: vec![(id.clone(), "fixture release error".into())],
    });
    let shown = tests::sized(&mut review, 140, 40);
    assert!(shown.contains("run-1-3: fixture release error"), "{shown}");
    assert_eq!(review.key(KeyEvent::from(KeyCode::Char('q'))), Action::None);
    assert_eq!(
        review.key(KeyEvent::from(KeyCode::Char('u'))),
        Action::Start(Job::Release {
            ids: vec![id],
            quit: true
        })
    );
}
