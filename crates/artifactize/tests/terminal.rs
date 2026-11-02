//! A gone terminal's restore offer, through the real CLI host: nothing runs until a person
//! chooses, and a chosen rerun runs through the fresh shell in its remembered directory.
use std::path::{Path, PathBuf};

use artifactize_core::terminal::{self, RestoreChoice, TerminalFrame};
use serde_json::json;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::os;

#[path = "support/terminal.rs"]
mod endpoint;

/// A watchdog reports a stuck fixture (a slow shell start included); it never orders events.
const WAIT: std::time::Duration = std::time::Duration::from_secs(60);
/// Poll interval while the host process starts listening.
const RETRY: std::time::Duration = std::time::Duration::from_millis(20);
const COLS: u16 = 80;
const ROWS: u16 = 24;

/// A primary checkout `root/project` with one commit.
fn project(root: &Path) -> PathBuf {
    let main = root.join("project");
    std::fs::create_dir(&main).unwrap();
    os::git(&main, &["init", "--quiet", "--initial-branch=main"]);
    os::git(
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
    os::canonical(&main)
}

/// The worktree's endpoint directory, removed after the host that used it is gone.
struct EndpointDirectory(PathBuf);
impl Drop for EndpointDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn contains(bytes: &[u8], text: &str) -> bool {
    bytes
        .windows(text.len())
        .any(|window| window == text.as_bytes())
}

/// Frames received so far, with every byte of output, for waits on an exact frame or text.
struct Frames {
    receiver: tokio::sync::mpsc::Receiver<TerminalFrame>,
    output: Vec<u8>,
}
impl Frames {
    async fn next(&mut self) -> TerminalFrame {
        let frame = tokio::time::timeout(os::patience(WAIT), self.receiver.recv())
            .await
            .expect("a terminal frame arrives")
            .expect("the attachment stays open");
        if let TerminalFrame::Output { data } = &frame {
            self.output.extend_from_slice(data);
        }
        frame
    }
    async fn printed(&mut self, text: &str) {
        while !contains(&self.output, text) {
            self.next().await;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_offer_runs_nothing_until_a_choice_is_sent() {
    let root = os::tempdir();
    let worktree = project(root.path());
    let programs = root.path().join("programs");
    std::fs::create_dir(&programs).unwrap();
    let label = "restored in ";
    let program = os::labelled_working_directory_program(&programs, "where", label);
    let offered = worktree.join("offered");
    std::fs::create_dir(&offered).unwrap();
    // What a previous host of this worktree saved before it was gone, laid out by hand.
    let metadata = artifactize_core::repository::worktree_id(&worktree)
        .unwrap()
        .join("artifactize-terminal.json");
    let saved = serde_json::to_vec_pretty(&json!({
        "cwd": os::path_text(&offered),
        "command": { "program": os::path_text(&program), "args": [] },
        "harness": null,
        "session_id": null,
    }))
    .unwrap();
    os::write_private_file(&metadata, &saved);

    let _endpoint = EndpointDirectory(endpoint::endpoint_directory(&worktree));
    let mut host = os::spawn_test(
        tokio::process::Command::new(os::cli())
            .arg("__terminal-host")
            .arg(&worktree)
            .args([COLS.to_string(), ROWS.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null()),
    );
    // The host saves the restore metadata again, in its own layout, once it listens; only
    // then does `attach` connect to this owned host rather than launch one of its own.
    tokio::time::timeout(os::patience(WAIT), async {
        while std::fs::read(&metadata).unwrap() == saved {
            tokio::time::sleep(RETRY).await;
        }
    })
    .await
    .expect("the terminal host listens");

    let (attachment, receiver) = terminal::attach(&worktree, COLS, ROWS).await.unwrap();
    let mut frames = Frames {
        receiver,
        output: Vec::new(),
    };
    let offer = loop {
        if let TerminalFrame::RestoreOffer { offer } = frames.next().await {
            break offer;
        }
    };
    assert_eq!(
        offer.command.map(|command| command.program),
        Some(os::path_text(&program))
    );
    assert_eq!(offer.cwd.as_deref(), Some(offered.as_path()));
    // A fresh shell answers while the offer waits, and the offered program has not run.
    attachment
        .input(os::terminal_echo("fresh", "shell"))
        .await
        .unwrap();
    frames.printed("freshshell").await;
    assert!(!contains(&frames.output, label));

    attachment.restore(RestoreChoice::Rerun).await.unwrap();
    frames
        .printed(&format!("{label}{}", os::path_text(&offered)))
        .await;
    // The offer is used up: nothing can run it a second time.
    assert!(attachment.restore(RestoreChoice::Rerun).await.is_err());

    attachment.input(b"exit\r".to_vec()).await.unwrap();
    while !matches!(frames.next().await, TerminalFrame::Ended { .. }) {}
    assert!(host.wait().await.success());
}
