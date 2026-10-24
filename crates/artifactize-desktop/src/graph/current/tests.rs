use super::{Currents, Ended, Request};
use crate::ids::ScanId;
use artifactize_core::monitor::live::Place;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

fn place() -> Place {
    Place::Folder("repo".into())
}

/// The scan a request started.
fn started(request: Request) -> (ScanId, CancellationToken, watch::Sender<Ended>) {
    match request {
        Request::Run { scan, token, ended } => (scan, token, ended),
        Request::Join(_) => panic!("joined a running scan instead of starting one"),
    }
}

/// The running scan a request waits for.
fn joined(request: Request) -> watch::Receiver<Ended> {
    match request {
        Request::Join(ended) => ended,
        Request::Run { .. } => panic!("started a scan instead of joining the one running"),
    }
}

#[test]
fn a_request_while_the_scan_runs_waits_for_it_and_gets_its_outcome() {
    let currents = Currents::default();
    let (scan, token, ended) = started(currents.request(&place()));
    let mut waiting = joined(currents.request(&place()));
    assert_eq!(*waiting.borrow(), None, "nothing has ended yet");
    assert!(
        !token.is_cancelled(),
        "a second request never stops the scan"
    );
    currents.end(&place(), scan, None);
    ended.send_replace(Some(Err("the worktree is unreadable".into())));
    assert_eq!(
        *waiting.borrow_and_update(),
        Some(Err("the worktree is unreadable".into()))
    );
    // Once it ended, the next request scans again.
    let (next, ..) = started(currents.request(&place()));
    assert!(next > scan);
}

#[test]
fn a_scan_whose_task_is_gone_is_not_waited_for() {
    let currents = Currents::default();
    let (scan, _token, ended) = started(currents.request(&place()));
    drop(ended);
    let (next, ..) = started(currents.request(&place()));
    assert!(next > scan);
}

#[test]
fn a_replaced_state_stops_the_scan_and_the_next_request_starts_another() {
    let currents = Currents::default();
    let (scan, token, _ended) = started(currents.request(&place()));
    currents.reset();
    assert!(token.is_cancelled());
    let (next, ..) = started(currents.request(&place()));
    assert!(next > scan);
}
