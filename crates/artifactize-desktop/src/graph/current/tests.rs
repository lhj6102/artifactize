use super::{Currents, Fresh, Request, Stage, view};
use crate::{browse::BranchKey, ids::ScanId};
use artifactize_core::monitor::live::{Current, Place};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

fn place() -> Place {
    Place::Folder("repo".into())
}

/// A scan step that found no Artifacts.
fn empty() -> Current {
    Current {
        scanned_at: time::OffsetDateTime::UNIX_EPOCH,
        fingerprint: "v1:empty".parse().unwrap(),
        structure: "".into(),
        artifacts: Vec::new(),
        edges: Vec::new(),
    }
}

/// Why the branch's last scan failed, as the window reads it.
fn failure(currents: &Currents) -> Option<String> {
    view(currents, BranchKey::new(&place())).unwrap().failure
}

/// The scan a request started.
fn started(request: Request) -> (ScanId, CancellationToken, watch::Sender<Stage>) {
    match request {
        Request::Run(Fresh { scan, token, stage }) => (scan, token, stage),
        Request::Join(_) => panic!("joined a running scan instead of starting one"),
    }
}

/// The running scan a request shows.
fn joined(request: Request) -> watch::Receiver<Stage> {
    match request {
        Request::Join(stage) => stage,
        Request::Run(_) => panic!("started a scan instead of joining the one running"),
    }
}

#[test]
fn a_request_while_the_scan_runs_shows_it_and_gets_its_outcome() {
    let currents = Currents::default();
    let (scan, token, stage) = started(currents.request(&place(), false));
    let mut shown = joined(currents.request(&place(), false));
    assert_eq!(*shown.borrow(), Stage::Started, "nothing published yet");
    assert!(
        !token.is_cancelled(),
        "a second request never stops the scan"
    );
    assert!(currents.publish(&place(), scan, empty()));
    assert!(matches!(
        currents.end(&place(), scan, Some("the worktree is unreadable".into())),
        Some(None)
    ));
    stage.send_replace(Stage::Ended(Err("the worktree is unreadable".into())));
    assert_eq!(
        *shown.borrow_and_update(),
        Stage::Ended(Err("the worktree is unreadable".into()))
    );
    assert_eq!(
        failure(&currents).as_deref(),
        Some("the worktree is unreadable")
    );
    // Once it ended, the next request scans again, its failure forgotten.
    let (next, ..) = started(currents.request(&place(), false));
    assert!(next > scan);
    assert_eq!(failure(&currents), None);
}

#[test]
fn a_scan_whose_task_is_gone_is_not_waited_for() {
    let currents = Currents::default();
    let (scan, _token, stage) = started(currents.request(&place(), false));
    drop(stage);
    let (next, ..) = started(currents.request(&place(), false));
    assert!(next > scan);
}

#[test]
fn a_replaced_state_stops_the_scan_and_keeps_nothing_it_finds() {
    let currents = Currents::default();
    let (scan, token, _stage) = started(currents.request(&place(), false));
    currents.reset();
    assert!(token.is_cancelled());
    assert!(
        currents.end(&place(), scan, None).is_none(),
        "its end is not announced"
    );
    let (next, ..) = started(currents.request(&place(), false));
    assert!(next > scan);
}

#[test]
fn a_refresh_while_the_scan_runs_shows_it_then_scans_once_more() {
    let currents = Currents::default();
    let (scan, token, _stage) = started(currents.request(&place(), false));
    // Files may have changed after the running scan read them.
    joined(currents.request(&place(), true));
    joined(currents.request(&place(), true));
    assert!(!token.is_cancelled(), "a refresh never stops the scan");
    let Some(Some(Fresh { scan: next, .. })) = currents.end(&place(), scan, None) else {
        panic!("no scan followed the refresh");
    };
    assert!(next > scan);
    // The two refreshes asked for one scan more, not two.
    assert!(matches!(currents.end(&place(), next, None), Some(None)));
}
