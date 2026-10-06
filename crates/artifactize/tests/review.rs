use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
#[cfg(unix)]
use std::{
    io::{Read, Write},
    path::Path,
    sync::mpsc,
};

use artifactize::{
    human,
    review::{Action, Mode, Review},
    store,
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use support::os::bin;

mod support;

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

/// The review-demo shape: a Human sign-off with `output` and `launch` tools, and a dependent.
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let release = repo.join("release");
        fs::create_dir_all(&release).unwrap();
        fs::write(release.join("notes.md"), "# Release notes\n- Ready.\n").unwrap();
        fs::write(
            release.join("artifactize.json"),
            json!({
                "name":"release","fingerprint":{"files":["."]},
                "views":{"humanTools":{
                    "notes":{"description":"Print the release notes.","kind":"output","command":bin("cat"),"args":["{artifactPath}/notes.md"]},
                    "fail":{"description":"Fail.","kind":"output","command":bin("false"),"args":[]},
                    "open":{"description":"Open.","kind":"launch","command":bin("true"),"args":["{artifactPath}"]}
                }},
                "evals":[{"id":"signoff","title":"A person approves the release","profile":{"kind":"human"},
                    "payload":{"instruction":"Read the release notes and approve them."},
                    "passSchema":{"type":"object","properties":{"approved":{"const":true}},"required":["approved"],"additionalProperties":false},
                    "failSchema":{"type":"object","properties":{"reason":{"type":"string","minLength":1}},"required":["reason"],"additionalProperties":false}}]
            })
            .to_string(),
        )
        .unwrap();
        fs::create_dir_all(repo.join("ship")).unwrap();
        fs::write(
            repo.join("ship/artifactize.json"),
            json!({"name":"ship","evals":[{"id":"check","title":"Ship","profile":{"kind":"runtime","command":bin("true"),"args":[]},
                "payload":{"instruction":"Ship {release}."}}]})
            .to_string(),
        )
        .unwrap();
        Self {
            repo,
            state: root.path().join("state"),
            _root: root,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .env("USER", "alice")
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state);
        command
    }

    async fn waiting(&self) -> String {
        let deadline = Instant::now() + support::os::patience(Duration::from_secs(15));
        loop {
            let waiting = store::read_waiting(&self.state, Some(&self.repo))
                .await
                .unwrap();
            if let Some(view) = waiting.first() {
                return view.request.id.clone();
            }
            assert!(Instant::now() < deadline, "Human request did not appear");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn receipts(&self) -> store::Receipts {
        store::Receipts::open(&self.state, &self.repo)
            .await
            .unwrap()
    }

    async fn claim(&self, id: &str) -> Option<String> {
        let view = store::read_request(&self.state, id).await.unwrap();
        view.claim.map(|claim| claim.reviewer)
    }
}

fn screen(review: &mut Review) -> String {
    let mut terminal = Terminal::new(TestBackend::new(160, 50)).unwrap();
    terminal.draw(|frame| review.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .chunks(buffer.area.width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Press a key and carry out its jobs and refreshes like the terminal driver does.
async fn press(review: &mut Review, code: KeyCode) -> Action {
    let mut action = review.key(KeyEvent::from(code));
    loop {
        action = match action {
            Action::Start(job) => {
                let outcome = review.start(job).await;
                review.finish(outcome)
            }
            Action::Refresh => review.refresh().await,
            action => return action,
        };
    }
}

fn finish(mut child: Child) -> (Option<i32>, Value) {
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(20));
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("verify did not finish");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    (
        output.status.code(),
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!("{}", String::from_utf8_lossy(&output.stderr));
        }),
    )
}

#[tokio::test]
async fn review_claims_runs_tools_and_submits_while_verify_waits() {
    let fixture = Fixture::new();
    let child = fixture
        .command()
        .args(["verify", "--all", "--timeout-ms", "30000", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let id = fixture.waiting().await;
    let mut review = Review::new(
        fixture.state.clone(),
        Some(fixture.repo.clone()),
        "alice".into(),
        None,
    );
    review.refresh().await;
    let list = screen(&mut review);
    assert!(
        list.contains("Waiting Human reviews (1)") && list.contains(&id),
        "{list}"
    );
    assert_eq!(press(&mut review, KeyCode::Enter).await, Action::None);
    let details = screen(&mut review);
    assert!(details.contains("Claim: unclaimed"), "{details}");
    assert!(details.contains("fail_release (output)") && details.contains("open_release (launch)"));
    assert_eq!(fixture.claim(&id).await, None, "opening claims nothing");

    // fail_release, notes_release, open_release: run notes first, after confirming it.
    press(&mut review, KeyCode::Char('j')).await;
    press(&mut review, KeyCode::Enter).await;
    let release = support::os::canonical(&fixture.repo.join("release"));
    let confirm = screen(&mut review);
    assert!(
        confirm.contains(&format!(
            "Repository: {}",
            support::os::canonical(&fixture.repo).display()
        )),
        "{confirm}"
    );
    let notes = release.join("notes.md").display().to_string();
    let command = artifactize::review::shell([bin("cat").as_str(), notes.as_str()]);
    let expected = format!("Command: {command}");
    // A stand-in's long path wraps inside the dialog: compare the text without the layout.
    let unwrapped = |text: &str| -> String {
        text.chars()
            .filter(|c| !c.is_whitespace() && !"│┌┐└┘─".contains(*c))
            .collect()
    };
    assert!(
        confirm.contains(&expected)
            || (support::os::stand_ins() && unwrapped(&confirm).contains(&unwrapped(&expected))),
        "{confirm}"
    );
    assert_eq!(
        fixture.claim(&id).await,
        None,
        "confirmation claims nothing"
    );
    press(&mut review, KeyCode::Char('y')).await;
    let notes = screen(&mut review);
    assert!(
        notes.contains("# Release notes") && notes.contains("- Ready."),
        "{notes}"
    );
    assert!(notes.contains("claimed by you (alice)"), "{notes}");
    assert_eq!(fixture.claim(&id).await.as_deref(), Some("alice"));

    press(&mut review, KeyCode::Char('k')).await;
    press(&mut review, KeyCode::Enter).await;
    assert!(matches!(review.mode(), Mode::Confirm { .. }));
    press(&mut review, KeyCode::Char('y')).await;
    let failed = screen(&mut review);
    assert!(
        failed.contains("Output · fail_release · tool error"),
        "{failed}"
    );
    assert!(
        failed.contains("Human tool exited unsuccessfully"),
        "{failed}"
    );
    press(&mut review, KeyCode::Char('j')).await;
    press(&mut review, KeyCode::Char('j')).await;
    press(&mut review, KeyCode::Enter).await;
    press(&mut review, KeyCode::Char('y')).await;
    assert!(screen(&mut review).contains("open_release launched."));
    // Confirmed in this session: notes runs again without asking.
    press(&mut review, KeyCode::Char('k')).await;
    press(&mut review, KeyCode::Enter).await;
    assert_eq!(review.mode(), &Mode::Request);
    assert!(screen(&mut review).contains("notes_release finished."));

    press(&mut review, KeyCode::Char('s')).await;
    press(&mut review, KeyCode::Char('r')).await;
    press(&mut review, KeyCode::Enter).await;
    let invalid = screen(&mut review);
    assert!(
        invalid.contains(r#"- instancePath "/reason": "" is shorter than 1 character"#),
        "{invalid}"
    );
    assert!(matches!(review.mode(), Mode::Form(_)));
    let view = store::read_request(&fixture.state, &id).await.unwrap();
    assert_eq!(view.request.status, "WAITING_HUMAN");
    press(&mut review, KeyCode::Esc).await;
    press(&mut review, KeyCode::Char('s')).await;
    press(&mut review, KeyCode::Char('g')).await;
    assert!(screen(&mut review).contains("approved*: true (fixed)"));
    // Nothing else waits after the submission, so the review hands back to its caller.
    assert_eq!(press(&mut review, KeyCode::Enter).await, Action::Quit);

    let (code, run) = finish(child);
    assert_eq!(code, Some(0), "{run}");
    assert!(
        run["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "GREEN")
    );
    let view = store::read_request(&fixture.state, &id).await.unwrap();
    assert_eq!(view.request.result.unwrap()["approved"], true);
    assert!(view.claim.is_none());
}

#[tokio::test]
async fn review_respects_other_claims_and_releases_its_own() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["verify", "--all", "--timeout-ms", "1"])
        .output()
        .unwrap();
    // The Human wait times out at once; the request stays open.
    assert_eq!(output.status.code(), Some(3));
    let id = fixture.waiting().await;
    let receipts = fixture.receipts().await;
    human::claim(&receipts, &id, "bob").await.unwrap();
    let mut review = Review::new(
        fixture.state.clone(),
        Some(fixture.repo.clone()),
        "alice".into(),
        Some(id.clone()),
    );
    review.refresh().await;
    for key in [KeyCode::Enter, KeyCode::Char('s'), KeyCode::Char('u')] {
        assert_eq!(press(&mut review, key).await, Action::None);
        assert!(screen(&mut review).contains("Claimed by bob; read-only for alice."));
    }
    assert_eq!(fixture.claim(&id).await.as_deref(), Some("bob"));
    human::unclaim(&receipts, &id, "bob").await.unwrap();
    review.refresh().await;

    // The unclaim key releases, and another reviewer can then claim.
    press(&mut review, KeyCode::Char('j')).await;
    press(&mut review, KeyCode::Enter).await;
    press(&mut review, KeyCode::Char('y')).await;
    assert_eq!(fixture.claim(&id).await.as_deref(), Some("alice"));
    press(&mut review, KeyCode::Char('u')).await;
    assert!(screen(&mut review).contains("Claim released."));
    assert_eq!(fixture.claim(&id).await, None);
    human::claim(&receipts, &id, "bob").await.unwrap();
    human::unclaim(&receipts, &id, "bob").await.unwrap();

    // Quitting after a claim this session took asks; releasing frees it for others.
    press(&mut review, KeyCode::Enter).await;
    assert_eq!(fixture.claim(&id).await.as_deref(), Some("alice"));
    assert_eq!(press(&mut review, KeyCode::Char('q')).await, Action::None);
    assert!(screen(&mut review).contains("This session claimed 1 request(s)"));
    assert_eq!(press(&mut review, KeyCode::Char('u')).await, Action::Quit);
    assert_eq!(fixture.claim(&id).await, None);
    human::claim(&receipts, &id, "bob").await.unwrap();
}

#[test]
fn review_rejects_unusable_options_before_taking_the_terminal() {
    let fixture = Fixture::new();
    for (args, error) in [
        (vec!["review", "--json"], "--json is not supported"),
        (vec!["review", "--all"], "--repo or --all"),
        (vec!["review", "missing"], "Review request not found."),
        (vec!["review", "--reviewer", " "], "reviewer id"),
    ] {
        let output = fixture.command().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        // --json reports errors on stdout.
        let text = [output.stdout, output.stderr].concat();
        let text = String::from_utf8_lossy(&text);
        assert!(text.contains(error), "{args:?}: {text}");
    }
}

#[cfg(unix)]
fn pty(command: &str) -> (Child, mpsc::Receiver<Vec<u8>>) {
    let mut child = Command::new("script")
        .args(["-qec", command, "/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(read @ 1..) = stdout.read(&mut buffer) {
            if sender.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    (child, receiver)
}

/// Needs a pseudo-terminal from script(1); Windows has ConPTY, but no such tool to drive it.
#[cfg(unix)]
#[test]
fn pty_review_restores_the_terminal_on_quit() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let command = format!(
        "stty cols 100 rows 30; exec '{}' --state-dir '{}' review --all --reviewer tester",
        env!("CARGO_BIN_EXE_artifactize"),
        Path::new(&state).display()
    );
    let (mut child, receiver) = pty(&command);
    let mut output = Vec::new();
    while !String::from_utf8_lossy(&output).contains("waiting.") {
        let chunk = receiver.recv_timeout(Duration::from_secs(10));
        output.extend(chunk.expect("review did not draw"));
    }
    child.stdin.take().unwrap().write_all(b"q").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Ok(chunk) = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        output.extend(chunk);
    }
    assert!(child.wait().unwrap().success());
    let output = String::from_utf8_lossy(&output);
    assert!(output.starts_with("\x1b[?1049h"), "{output:?}");
    assert!(output.ends_with("\x1b[?25h\x1b[?1049l"), "{output:?}");
}
