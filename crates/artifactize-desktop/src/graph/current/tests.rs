use super::{Currents, Request, Stage};
use crate::ids::ScanId;
use artifactize_core::monitor::live::Place;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

fn place() -> Place {
    Place::Folder("repo".into())
}

/// The scan a request started.
fn started(request: Request) -> (ScanId, CancellationToken, watch::Sender<Stage>) {
    match request {
        Request::Run { scan, token, stage } => (scan, token, stage),
        Request::Join(_) => panic!("joined a running scan instead of starting one"),
    }
}

/// The running scan a request shows.
fn joined(request: Request) -> watch::Receiver<Stage> {
    match request {
        Request::Join(stage) => stage,
        Request::Run { .. } => panic!("started a scan instead of joining the one running"),
    }
}

#[test]
fn a_request_while_the_scan_runs_shows_it_and_gets_its_outcome() {
    let currents = Currents::default();
    let (scan, token, stage) = started(currents.request(&place()));
    let mut shown = joined(currents.request(&place()));
    assert_eq!(*shown.borrow(), Stage::Started, "nothing published yet");
    assert!(
        !token.is_cancelled(),
        "a second request never stops the scan"
    );
    assert!(currents.end(&place(), scan, Some("the worktree is unreadable".into())));
    stage.send_replace(Stage::Ended(Err("the worktree is unreadable".into())));
    assert_eq!(
        *shown.borrow_and_update(),
        Stage::Ended(Err("the worktree is unreadable".into()))
    );
    // Once it ended, the next request scans again, its failure forgotten.
    let (next, ..) = started(currents.request(&place()));
    assert!(next > scan);
    assert!(currents.failed.lock().unwrap().is_empty());
}

#[test]
fn a_scan_whose_task_is_gone_is_not_waited_for() {
    let currents = Currents::default();
    let (scan, _token, stage) = started(currents.request(&place()));
    drop(stage);
    let (next, ..) = started(currents.request(&place()));
    assert!(next > scan);
}

#[test]
fn a_replaced_state_stops_the_scan_and_keeps_nothing_it_finds() {
    let currents = Currents::default();
    let (scan, token, _stage) = started(currents.request(&place()));
    currents.reset();
    assert!(token.is_cancelled());
    assert!(
        !currents.end(&place(), scan, None),
        "its end is not announced"
    );
    let (next, ..) = started(currents.request(&place()));
    assert!(next > scan);
}
