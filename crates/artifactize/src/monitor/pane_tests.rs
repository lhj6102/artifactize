//! Arrows and Esc drill down and back through the levels; a Human form owns its arrows.
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
    monitor.tree.select(vec!["a:p2".into()]);
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
        // The driver opens Detail, which then takes focus.
        (KeyCode::Right, Pane::Artifacts, Action::OpenDetail),
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
    let artifact = vec!["a:p2".to_owned()];
    press(&mut monitor, KeyCode::Char('h'));
    assert!(!monitor.tree.opened().contains(&artifact));
    assert_eq!(press(&mut monitor, KeyCode::Char('l')), Action::None);
    assert!(monitor.tree.opened().contains(&artifact));
    press(&mut monitor, KeyCode::Char('h'));
    assert!(!monitor.tree.opened().contains(&artifact));
    press(&mut monitor, KeyCode::Char(' '));
    assert!(monitor.tree.opened().contains(&artifact));
    press(&mut monitor, KeyCode::Char(' '));
    assert!(!monitor.tree.opened().contains(&artifact));
}

#[tokio::test]
async fn detail_steps_back_and_human_form_keeps_json_cursor_editing() {
    let mut monitor = populated();
    monitor.focus = Pane::Artifacts;
    monitor.open_detail().await;
    assert_eq!(monitor.focus, Pane::Detail);
    // Outside a Human review, Left and Esc step back to the tree.
    for code in [KeyCode::Left, KeyCode::Esc] {
        assert_eq!(press(&mut monitor, code), Action::None);
        assert_eq!(monitor.focus, Pane::Artifacts);
        assert!(monitor.detail.is_none());
        monitor.open_detail().await;
    }
    assert_eq!(press(&mut monitor, KeyCode::Right), Action::None);
    assert_eq!(monitor.focus, Pane::Detail);
    let schema = json!({"type":"object","properties":{"nested":{"type":"object"}}});
    // A nested JSON form owns Left/Right; focus stays in Detail.
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
    monitor.detail.as_mut().unwrap().review = Some(review);
    monitor.detail.as_mut().unwrap().focus = DetailArea::Fields;
    let cursor = |monitor: &Monitor| match monitor
        .detail
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
    assert_eq!(monitor.focus, Pane::Detail);
    assert_eq!(
        press(&mut monitor, KeyCode::Right),
        Action::Review(review::Action::None)
    );
    assert_eq!(cursor(&monitor), initial);
    assert_eq!(monitor.focus, Pane::Detail);
}

#[test]
fn claim_key_is_inert_outside_detail_and_esc_never_quits() {
    let mut monitor = populated();
    for pane in [Pane::Repositories, Pane::Runs, Pane::Artifacts] {
        monitor.focus = pane;
        assert_eq!(press(&mut monitor, KeyCode::Char('c')), Action::None);
        assert_eq!(monitor.focus, pane);
    }
    for expected in [Pane::Runs, Pane::Repositories, Pane::Repositories] {
        assert_eq!(press(&mut monitor, KeyCode::Esc), Action::None);
        assert_eq!(monitor.focus, expected);
    }
    assert_eq!(press(&mut monitor, KeyCode::Char('q')), Action::Quit);
}

#[test]
fn help_overlay_opens_with_question_mark_and_any_key_closes_it() {
    let mut monitor = populated();
    monitor.focus = Pane::Runs;
    press(&mut monitor, KeyCode::Char('?'));
    assert!(monitor.help);
    let text = tests::sized(&mut monitor, 120, 40);
    assert!(text.contains("Keys · any key closes"), "{text}");
    assert!(text.contains("never quits"), "{text}");
    // The closing key does nothing else, even q.
    assert_eq!(press(&mut monitor, KeyCode::Char('q')), Action::None);
    assert!(!monitor.help);
    assert_eq!(monitor.focus, Pane::Runs);
}

#[test]
fn bang_cycles_attention_evals_in_tree_order_and_focuses_the_tree() {
    let mut monitor = populated();
    monitor.focus = Pane::Repositories;
    let mut seen = Vec::new();
    for _ in 0..3 {
        assert_eq!(press(&mut monitor, KeyCode::Char('!')), Action::None);
        assert_eq!(monitor.focus, Pane::Artifacts);
        seen.push(monitor.target());
    }
    // From the selected Artifact p2: its ERROR eval, the waiting Human review, then wrap.
    assert_eq!(
        seen,
        [
            Some(Target::Eval("p2/check".into())),
            Some(Target::Eval("app/review".into())),
            Some(Target::Eval("p2/check".into())),
        ]
    );
    assert!(monitor.tree.opened().contains(&vec!["a:app".to_owned()]));
    let mut idle = Monitor::new("/fixture-state".into(), None);
    press(&mut idle, KeyCode::Char('!'));
    assert!(idle.notice.as_deref().unwrap().contains("Open a Run"));
}
