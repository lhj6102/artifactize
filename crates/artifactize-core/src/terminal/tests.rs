//! Terminal seams: a host serves attachments of one worktree, outlives every client and ends
//! with its worktree; a restore offer waits for a person's choice. The host runs in this test
//! process through the same `serve` the hidden CLI entry calls, so no installed or earlier
//! built CLI is ever started. The terminal runs the system's default shell, as it does for
//! a person; the typed lines come from `test_os::terminal_echo` and work in every such shell.
use super::*;
use crate::{local_transport::Endpoint, test_os};
use std::path::Path;
use tokio::{sync::mpsc, task::JoinHandle};

/// A watchdog reports a stuck fixture (a slow shell start included); it never orders events.
const WAIT: std::time::Duration = std::time::Duration::from_secs(60);
/// Retry interval while the in-process host binds its endpoint.
const RETRY: std::time::Duration = std::time::Duration::from_millis(20);
const COLS: u16 = 80;
const ROWS: u16 = 24;

/// A primary checkout `root/project` with one commit.
fn project(root: &Path) -> PathBuf {
    let main = root.join("project");
    std::fs::create_dir(&main).unwrap();
    test_os::git(&main, &["init", "--quiet", "--initial-branch=main"]);
    test_os::git(
        &main,
        &[
            "-c",
            "user.name=artifactize",
            "-c",
            "user.email=artifactize@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "first",
        ],
    );
    test_os::canonical(&main)
}

/// One in-process host. It owns the worktree's endpoint directory, which outlives the host by
/// design (its lock is never unlinked), and removes it once the host has returned.
struct Host {
    task: Option<JoinHandle<Result<(), String>>>,
    directory: PathBuf,
}
impl Host {
    /// Serve `worktree` and return once its endpoint accepts connections, so `attach` connects
    /// instead of launching a CLI host of its own.
    async fn start(worktree: &Path) -> Self {
        let identity = crate::repository::worktree_id(worktree).unwrap();
        let endpoint = Endpoint::namespaced(&identity, "terminal").unwrap();
        let directory = endpoint.directory().to_owned();
        let task = tokio::spawn(serve(worktree.to_owned(), COLS, ROWS));
        let mut host = Self {
            task: Some(task),
            directory,
        };
        let ready = tokio::time::timeout(test_os::patience(WAIT), async {
            while endpoint.connect().await.is_err() {
                assert!(
                    !host.task.as_ref().unwrap().is_finished(),
                    "host ended early"
                );
                tokio::time::sleep(RETRY).await;
            }
        })
        .await;
        if ready.is_err() {
            host.task.take().unwrap().abort();
            panic!("the terminal host did not listen");
        }
        host
    }
    /// Wait for the host's own end, then release its endpoint directory.
    async fn ended(mut self) {
        let task = self.task.take().unwrap();
        let result = tokio::time::timeout(test_os::patience(WAIT), task)
            .await
            .expect("the host ends")
            .unwrap();
        assert_eq!(result, Ok(()));
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            // An unwinding test drops its worktree next; the host notices and ends itself.
            task.abort();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Frames received so far; every wait names the exact frame or output it needs.
struct Frames {
    receiver: mpsc::Receiver<TerminalFrame>,
    output: Vec<u8>,
}
impl Frames {
    fn new(receiver: mpsc::Receiver<TerminalFrame>) -> Self {
        Self {
            receiver,
            output: Vec::new(),
        }
    }
    async fn next(&mut self) -> Option<TerminalFrame> {
        let frame = tokio::time::timeout(test_os::patience(WAIT), self.receiver.recv())
            .await
            .expect("a terminal frame arrives");
        if let Some(TerminalFrame::Output { data }) = &frame {
            self.output.extend_from_slice(data);
        }
        frame
    }
    async fn snapshot(&mut self) -> Vec<u8> {
        loop {
            match self.next().await {
                Some(TerminalFrame::Snapshot { data, .. }) => return data,
                Some(_) => {}
                None => panic!("the attachment closed before a snapshot"),
            }
        }
    }
    async fn printed(&mut self, text: &str) {
        while !contains(&self.output, text) {
            assert!(
                self.next().await.is_some(),
                "closed before {text:?} printed"
            );
        }
    }
    async fn ended(&mut self) {
        loop {
            match self.next().await {
                Some(TerminalFrame::Ended { .. }) => return,
                Some(_) => {}
                None => panic!("the attachment closed without the host's end"),
            }
        }
    }
}

fn contains(bytes: &[u8], text: &str) -> bool {
    bytes
        .windows(text.len())
        .any(|window| window == text.as_bytes())
}

/// Ask the shell to exit and wait until the host has ended with it.
async fn exit(terminal: &Attachment, frames: &mut Frames, host: Host) {
    terminal.input(b"exit\r".to_vec()).await.unwrap();
    frames.ended().await;
    host.ended().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_outlives_its_clients_and_a_reattach_shows_the_screen_so_far() {
    let root = test_os::tempdir();
    let worktree = project(root.path());
    let host = Host::start(&worktree).await;

    let (first, frames) = attach(&worktree, COLS, ROWS).await.unwrap();
    let mut frames = Frames::new(frames);
    frames.snapshot().await;
    first
        .input(test_os::terminal_echo("detach", "marker"))
        .await
        .unwrap();
    frames.printed("detachmarker").await;
    // Collapsing the panel detaches: the attachment closes and the host keeps running.
    first.detach().await.unwrap();
    while let Some(frame) = frames.next().await {
        assert!(!matches!(frame, TerminalFrame::Ended { .. }));
    }

    let (second, frames) = attach(&worktree, COLS, ROWS).await.unwrap();
    let mut frames = Frames::new(frames);
    assert!(contains(&frames.snapshot().await, "detachmarker"));
    second
        .input(test_os::terminal_echo("closed", "marker"))
        .await
        .unwrap();
    frames.printed("closedmarker").await;
    // Closing the app drops its attachments without a detach request.
    drop((second, frames));

    let (third, frames) = attach(&worktree, COLS, ROWS).await.unwrap();
    let mut frames = Frames::new(frames);
    let screen = frames.snapshot().await;
    assert!(contains(&screen, "detachmarker"));
    assert!(contains(&screen, "closedmarker"));
    exit(&third, &mut frames, host).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_the_worktree_ends_its_host() {
    let root = test_os::tempdir();
    let main = project(root.path());
    let created = crate::repository::create_worktree(&main, "HEAD").unwrap();
    let host = Host::start(&created.worktree).await;
    let (_terminal, frames) = attach(&created.worktree, COLS, ROWS).await.unwrap();
    let mut frames = Frames::new(frames);
    frames.snapshot().await;

    crate::repository::remove_worktree(&created.worktree).unwrap();
    frames.ended().await;
    host.ended().await;
    // With the worktree gone there is nothing to attach to, and no host is started for it.
    assert!(attach(&created.worktree, COLS, ROWS).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_offer_runs_nothing_until_a_choice_is_sent() {
    let root = test_os::tempdir();
    let worktree = project(root.path());
    let programs = root.path().join("programs");
    std::fs::create_dir(&programs).unwrap();
    let program = test_os::working_directory_program(&programs, "where");
    let offered = worktree.join("offered");
    std::fs::create_dir(&offered).unwrap();
    // What a previous host of this worktree saved before it was gone.
    let command = TerminalCommand {
        program: crate::platform::path_text(&program),
        args: Vec::new(),
    };
    metadata::Metadata {
        cwd: Some(offered.clone()),
        command: Some(command.clone()),
        harness: None,
        session_id: None,
    }
    .save(&crate::repository::worktree_id(&worktree).unwrap())
    .unwrap();
    let host = Host::start(&worktree).await;

    let (terminal, frames) = attach(&worktree, COLS, ROWS).await.unwrap();
    let mut frames = Frames::new(frames);
    let offer = loop {
        match frames.next().await {
            Some(TerminalFrame::RestoreOffer { offer }) => break offer,
            Some(_) => {}
            None => panic!("no restore offer"),
        }
    };
    assert_eq!(offer.command, Some(command));
    assert_eq!(offer.cwd.as_deref(), Some(offered.as_path()));
    // A fresh shell answers while the offer waits, and the offered program has not run.
    terminal
        .input(test_os::terminal_echo("fresh", "shell"))
        .await
        .unwrap();
    frames.printed("freshshell").await;
    let ran = test_os::path_text(&offered);
    assert!(!contains(&frames.output, &ran));

    terminal.restore(RestoreChoice::Rerun).await.unwrap();
    frames.printed(&ran).await;
    // The offer is used up: nothing can run it a second time.
    assert!(terminal.restore(RestoreChoice::Rerun).await.is_err());
    exit(&terminal, &mut frames, host).await;
}

#[tokio::test]
async fn an_endpoint_others_can_reach_is_refused_and_not_repaired() {
    let root = test_os::tempdir();
    let worktree = project(root.path());
    let identity = crate::repository::worktree_id(&worktree).unwrap();
    let directory = Endpoint::namespaced(&identity, "terminal")
        .unwrap()
        .directory()
        .to_owned();
    let host = Host {
        task: None,
        directory: directory.clone(),
    };
    test_os::share_dir(&directory);
    assert!(attach(&worktree, COLS, ROWS).await.is_err());
    assert!(!test_os::private_dir(&directory));
    drop(host);
}
