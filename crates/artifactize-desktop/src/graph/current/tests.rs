use super::{Currents, Fresh, Request, ScanStatus, Stage, view};
use crate::{browse::BranchKey, ids::ScanId};
use artifactize_core::monitor::live::{Current, Place};
use std::path::PathBuf;
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
    match view(currents, BranchKey::new(&place())).unwrap().scan {
        ScanStatus::Failed { reason } => Some(reason),
        ScanStatus::Scanning | ScanStatus::Scanned => None,
    }
}

/// The scan a request started.
fn started(request: Request) -> (ScanId, CancellationToken, watch::Sender<Stage>) {
    match request {
        Request::Run(Fresh {
            scan, token, stage, ..
        }) => (scan, token, stage),
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
    let (scan, token, stage) =
        started(currents.request(&place(), PathBuf::from("worktree"), false));
    let mut shown = joined(currents.request(&place(), PathBuf::from("worktree"), false));
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
    let (next, ..) = started(currents.request(&place(), PathBuf::from("worktree"), false));
    assert!(next > scan);
    assert_eq!(failure(&currents), None);
}

#[test]
fn a_scan_whose_task_is_gone_is_not_waited_for() {
    let currents = Currents::default();
    let (scan, _token, stage) =
        started(currents.request(&place(), PathBuf::from("worktree"), false));
    drop(stage);
    let (next, ..) = started(currents.request(&place(), PathBuf::from("worktree"), false));
    assert!(next > scan);
}

#[test]
fn stopping_cancels_the_running_scan_and_ends_once_its_task_lets_go() {
    let currents = Currents::default();
    let (_, token, stage) = started(currents.request(&place(), PathBuf::from("worktree"), false));
    let mut stopped = std::pin::pin!(currents.stop());
    assert!(token.is_cancelled());
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(stopped.as_mut().poll(&mut context).is_pending());
    // Its last step is not the end: the task still holds its stage.
    stage.send_replace(Stage::Ended(Err("cancelled".into())));
    assert!(stopped.as_mut().poll(&mut context).is_pending());
    drop(stage);
    assert!(stopped.as_mut().poll(&mut context).is_ready());
    // A later request scans again rather than joining the stopped scan.
    started(currents.request(&place(), PathBuf::from("worktree"), false));
}

#[test]
fn a_replaced_state_stops_the_scan_and_keeps_nothing_it_finds() {
    let currents = Currents::default();
    let (scan, token, _stage) =
        started(currents.request(&place(), PathBuf::from("worktree"), false));
    currents.reset();
    assert!(token.is_cancelled());
    assert!(
        currents.end(&place(), scan, None).is_none(),
        "its end is not announced"
    );
    let (next, ..) = started(currents.request(&place(), PathBuf::from("worktree"), false));
    assert!(next > scan);
}

#[test]
fn a_refresh_while_the_scan_runs_shows_it_then_scans_once_more() {
    let currents = Currents::default();
    let (scan, token, _stage) =
        started(currents.request(&place(), PathBuf::from("worktree"), false));
    // Files may have changed after the running scan read them.
    joined(currents.request(&place(), PathBuf::from("moved-worktree"), true));
    joined(currents.request(&place(), PathBuf::from("moved-worktree"), true));
    assert!(!token.is_cancelled(), "a refresh never stops the scan");
    let Some(Some(Fresh {
        scan: next,
        workspace,
        ..
    })) = currents.end(&place(), scan, None)
    else {
        panic!("no scan followed the refresh");
    };
    assert!(next > scan);
    // The two refreshes asked for one scan more, not two.
    assert_eq!(workspace, PathBuf::from("moved-worktree"));
    assert!(matches!(currents.end(&place(), next, None), Some(None)));
}

#[test]
fn a_run_finishing_during_the_scan_queues_a_fresh_forecast() {
    let currents = Currents::default();
    let workspace = PathBuf::from("worktree");
    let (scan, token, _stage) = started(currents.request(&place(), workspace.clone(), false));
    currents.recheck_after_run(&place());
    joined(currents.request(&place(), workspace.clone(), false));
    assert!(!token.is_cancelled());
    let Some(Some(Fresh {
        scan: next,
        workspace: queued,
        ..
    })) = currents.end(&place(), scan, None)
    else {
        panic!("completion did not schedule a new forecast");
    };
    assert_eq!(queued, workspace);
    assert!(matches!(currents.end(&place(), next, None), Some(None)));
}

#[test]
fn ordinary_completion_and_failure_each_invalidate_the_last_published_scan() {
    let currents = Currents::default();
    let mut changes = currents.take_changes().unwrap();
    for failure in [None, Some("fixture checkout failed".to_owned())] {
        let branch = place();
        let (scan, _, _stage) =
            started(currents.request(&branch, PathBuf::from("worktree"), false));
        assert!(currents.publish(&branch, scan, empty()));
        assert!(matches!(
            currents.end(&branch, scan, failure.clone()),
            Some(None)
        ));
        assert_eq!(changes.try_recv().unwrap(), BranchKey::new(&branch));
        let view = view(&currents, BranchKey::new(&branch)).unwrap();
        assert_eq!(
            view.scan,
            failure.map_or(ScanStatus::Scanned, |reason| ScanStatus::Failed { reason })
        );
    }
}
