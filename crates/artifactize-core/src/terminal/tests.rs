//! Terminal seams: a host serves attachments of one worktree, outlives every client and ends
//! with its worktree; its endpoint refuses what others could reach. The restore offer runs
//! through the real CLI host (crates/artifactize/tests/terminal.rs). The host runs in this test
//! process through the same `serve` the hidden CLI entry calls, so no installed or earlier
//! built CLI is ever started. The terminal runs the system's default shell, as it does for
//! a person; the typed lines come from `test_os::terminal_echo` and work in every such shell.
use super::*;
use crate::{local_transport::Endpoint, test_os};
use futures_util::FutureExt;
use std::path::Path;
use tokio::sync::mpsc;

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

/// The worktree's endpoint directory, which outlives a host by design (its election lock is
/// never unlinked). The fixture removes it once nothing serves or uses it any more.
struct EndpointDirectory(PathBuf);
impl Drop for EndpointDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Serve `worktree` in this process while `body` runs, through the same `serve` the hidden CLI
/// entry calls. The body starts once the endpoint accepts connections, so `attach` connects
/// instead of launching a CLI host of its own. Afterwards the host is ended, if the body did
/// not end it, and joined, an unwinding body included; only then is its endpoint released.
/// Returns what the host returned.
async fn hosted(worktree: &Path, body: impl Future<Output = ()>) -> Result<(), String> {
    let identity = crate::repository::worktree_id(worktree).unwrap();
    let endpoint = Endpoint::namespaced(&identity, "terminal").unwrap();
    let _directory = EndpointDirectory(endpoint.directory().to_owned());
    let mut task = tokio::spawn(serve(worktree.to_owned(), COLS, ROWS));
    let outcome = std::panic::AssertUnwindSafe(async {
        tokio::time::timeout(test_os::patience(WAIT), async {
            while endpoint.connect().await.is_err() {
                assert!(!task.is_finished(), "the host ended before it listened");
                tokio::time::sleep(RETRY).await;
            }
        })
        .await
        .expect("the terminal host listens");
        body.await;
    })
    .catch_unwind()
    .await;
    // A host ends itself once its worktree's Git identity is gone, on every system.
    let end = || {
        let _ = std::fs::remove_dir_all(&identity);
    };
    if outcome.is_err() {
        end();
    }
    let served = match tokio::time::timeout(test_os::patience(WAIT), &mut task).await {
        Ok(joined) => joined,
        Err(_) => {
            end();
            tokio::time::timeout(test_os::patience(WAIT), &mut task)
                .await
                .expect("the host ends once its identity is gone")
        }
    };
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    served.expect("the host task joins")
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

/// Ask the shell to exit and wait until the host says it ended with it.
async fn exit(terminal: &Attachment, frames: &mut Frames) {
    terminal.input(b"exit\r".to_vec()).await.unwrap();
    frames.ended().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_outlives_its_clients_and_a_reattach_shows_the_screen_so_far() {
    let root = test_os::tempdir();
    let worktree = project(root.path());
    let served = hosted(&worktree, async {
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
        exit(&third, &mut frames).await;
    })
    .await;
    assert_eq!(served, Ok(()));
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_the_worktree_ends_its_host() {
    let root = test_os::tempdir();
    let main = project(root.path());
    let created = crate::repository::create_worktree(&main, "HEAD").unwrap();
    let served = hosted(&created.worktree, async {
        let (_terminal, frames) = attach(&created.worktree, COLS, ROWS).await.unwrap();
        let mut frames = Frames::new(frames);
        frames.snapshot().await;

        crate::repository::remove_worktree(&created.worktree).unwrap();
        frames.ended().await;
    })
    .await;
    assert_eq!(served, Ok(()));
    // With the worktree gone there is nothing to attach to, and no host is started for it.
    assert!(attach(&created.worktree, COLS, ROWS).await.is_err());
}

#[tokio::test]
async fn an_endpoint_others_can_reach_is_refused_and_not_repaired() {
    let root = test_os::tempdir();
    let worktree = project(root.path());
    let identity = crate::repository::worktree_id(&worktree).unwrap();
    let directory = EndpointDirectory(
        Endpoint::namespaced(&identity, "terminal")
            .unwrap()
            .directory()
            .to_owned(),
    );
    test_os::share_dir(&directory.0);
    assert!(attach(&worktree, COLS, ROWS).await.is_err());
    assert!(!test_os::private_dir(&directory.0));
}
