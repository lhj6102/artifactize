//! Main-pane arrows navigate focus; modal arrows remain owned by their editor.
use super::*;
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::json;

fn press(monitor: &mut Monitor, code: KeyCode) -> Action {
    monitor.key(KeyEvent::from(code))
}
fn populated() -> Monitor {
    let (run, requests) = tests::live();
    let mut monitor = Monitor::new("/fixture-state".into(), None);
    monitor.set_run(run, requests);
    monitor.tree.select(vec!["f:pages".into()]);
    // Tree key navigation uses paths from its most recently drawn frame.
    ratatui::Terminal::new(ratatui::backend::TestBackend::new(180, 40))
        .unwrap()
        .draw(|frame| monitor.draw(frame))
        .unwrap();
    monitor
}

#[test]
fn main_arrows_move_both_directions_without_wrapping_or_changing_tree() {
    let mut monitor = populated();
    monitor.focus = Pane::Repositories;
    let selected = monitor.tree.selected().to_vec();
    let opened = monitor.tree.opened().clone();
    for (code, pane, action) in [
        (KeyCode::Left, Pane::Repositories, Action::None),
        (KeyCode::Right, Pane::Runs, Action::None),
        (KeyCode::Right, Pane::Artifacts, Action::Refresh),
        (KeyCode::Right, Pane::Artifacts, Action::None),
        (KeyCode::Left, Pane::Runs, Action::None),
        (KeyCode::Left, Pane::Repositories, Action::None),
        (KeyCode::Left, Pane::Repositories, Action::None),
    ] {
        assert_eq!(press(&mut monitor, code), action);
        assert_eq!(monitor.focus, pane);
        assert_eq!(monitor.tree.selected(), selected);
        assert_eq!(monitor.tree.opened(), &opened);
    }
    monitor.focus = Pane::Artifacts;
    press(&mut monitor, KeyCode::Tab);
    assert_eq!(monitor.focus, Pane::Repositories);
    press(&mut monitor, KeyCode::BackTab);
    assert_eq!(monitor.focus, Pane::Artifacts);
}

#[test]
fn tree_h_l_and_space_still_expand_and_collapse() {
    let mut monitor = populated();
    monitor.focus = Pane::Artifacts;
    let family = vec!["f:pages".to_owned()];
    assert!(!monitor.tree.opened().contains(&family));
    assert_eq!(press(&mut monitor, KeyCode::Char('l')), Action::None);
    assert!(monitor.tree.opened().contains(&family));
    press(&mut monitor, KeyCode::Char('h'));
    assert!(!monitor.tree.opened().contains(&family));
    press(&mut monitor, KeyCode::Char(' '));
    assert!(monitor.tree.opened().contains(&family));
    press(&mut monitor, KeyCode::Char(' '));
    assert!(!monitor.tree.opened().contains(&family));
}

#[tokio::test]
async fn modal_arrows_never_change_main_focus_and_keep_json_cursor_editing() {
    let mut monitor = populated();
    monitor.focus = Pane::Artifacts;
    monitor.open_detail().await;
    for code in [KeyCode::Left, KeyCode::Right] {
        assert_eq!(press(&mut monitor, code), Action::None);
        assert_eq!(monitor.focus, Pane::Artifacts);
    }
    let schema = json!({"type":"object","properties":{"nested":{"type":"object"}}});
    // A nested JSON form owns Left/Right even though the main focus remains Artifacts.
    let mut view = tests::request(
        "app/review",
        "WAITING_HUMAN",
        json!({"profile":{"kind":"human"},"humanDefinition":{"eval":{"declaration":{"failSchema":schema}}}}),
    );
    view.claim = Some(store::HumanClaim {
        request_id: view.request.id.clone(),
        reviewer: "alice".into(),
        claimed_at: "2026-01-01T00:00:00Z".into(),
    });
    let mut review = review::Review::new(
        "/fixture-state".into(),
        None,
        "alice".into(),
        Some(view.request.id.to_string()),
    );
    review.load_single(view);
    review.control(review::Control::Red);
    monitor.modal.as_mut().unwrap().review = Some(review);
    monitor.modal.as_mut().unwrap().focus = ModalPane::Fields;
    let cursor = |monitor: &Monitor| match monitor
        .modal
        .as_ref()
        .unwrap()
        .review
        .as_ref()
        .unwrap()
        .mode()
    {
        review::Mode::Form(form) => form.cursor,
        _ => panic!("form"),
    };
    let initial = cursor(&monitor);
    assert_eq!(
        press(&mut monitor, KeyCode::Left),
        Action::Review(review::Action::None)
    );
    assert_eq!(cursor(&monitor), initial - 1);
    assert_eq!(monitor.focus, Pane::Artifacts);
    assert_eq!(
        press(&mut monitor, KeyCode::Right),
        Action::Review(review::Action::None)
    );
    assert_eq!(cursor(&monitor), initial);
    assert_eq!(monitor.focus, Pane::Artifacts);
}
