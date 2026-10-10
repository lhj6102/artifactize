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
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
        support::declaration::write(
            release.join("index.artf"),
            json!({
                "name":"release",
                "fingerprint":{"files":["."]},
                "views":{
                    "human_tools":{
                        "notes":{
                            "description":"Print the release notes.",
                            "kind":"output",
                            "command":bin("cat"),
                            "args":["{artifactPath}/notes.md"],
                        },
                        "fail":{
                            "description":"Fail.",
                            "kind":"output",
                            "command":bin("false"),
                            "args":[],
                        },
                        "open":{
                            "description":"Open.",
                            "kind":"launch",
                            "command":bin("true"),
                            "args":["{artifactPath}"],
                        },
                    },
                },
                "evals":[
                    {
                        "id":"signoff",
                        "title":"A person approves the release",
                        "profile":{"kind":"human"},
                        "payload":{"instruction":"Read the release notes and approve them."},
                        "pass_schema":{
                            "type":"object",
                            "properties":{"approved":{"const":true}},
                            "required":["approved"],
                            "additionalProperties":false,
                        },
                        "fail_schema":{
                            "type":"object",
                            "properties":{"reason":{"type":"string","minLength":1}},
                            "required":["reason"],
                            "additionalProperties":false,
                        },
                    },
                ],
            })
            .to_string(),
        )
        .unwrap();
        fs::create_dir_all(repo.join("ship")).unwrap();
        support::declaration::write(
            repo.join("ship/index.artf"),
            json!({
                "name":"ship",
                "evals":[
                    {
                        "id":"check",
                        "title":"Ship",
                        "profile":{"kind":"runtime","command":bin("true"),"args":[]},
                        "payload":{"instruction":"Ship {release}."},
                    },
                ],
            })
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
                return view.request.id.to_string();
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

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Press a key and carry out its jobs and refreshes like the terminal driver does.
async fn press(review: &mut Review, key: impl Into<KeyEvent>) -> Action {
    let mut action = review.key(key.into());
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
    assert!(
        list.contains("Read the release notes and approve them."),
        "{list}"
    );
    assert_eq!(press(&mut review, KeyCode::Enter).await, Action::None);
    let details = screen(&mut review);
    assert!(details.contains("WAITING_HUMAN · unclaimed"), "{details}");
    assert!(details.contains("fail_release (output)") && details.contains("open_release (launch)"));
    assert_eq!(fixture.claim(&id).await, None, "opening claims nothing");

    // Tools wait for an explicit claim.
    press(&mut review, KeyCode::Tab).await;
    press(&mut review, KeyCode::Enter).await;
    assert!(screen(&mut review).contains("Claim this request before reviewing it."));
    assert_eq!(fixture.claim(&id).await, None);
    press(&mut review, KeyCode::Char('c')).await;
    assert_eq!(fixture.claim(&id).await.as_deref(), Some("alice"));
    assert!(screen(&mut review).contains("REVIEW (yours)"));

    // fail_release, notes_release, open_release: the focused Tools pane shows the resolved
    // command of the selected tool, and Enter runs it at once.
    press(&mut review, KeyCode::Char('j')).await;
    let release = support::os::canonical(&fixture.repo.join("release"));
    let selected = screen(&mut review);
    // A long path may be cut at the pane's edge: compare the text without the layout.
    let unwrapped = |text: &str| -> String {
        text.chars()
            .filter(|c| !c.is_whitespace() && !"│┌┐└┘─".contains(*c))
            .collect()
    };
    let notes = release.join("notes.md").display().to_string();
    let command = artifactize::review::shell([bin("cat").as_str(), notes.as_str()]);
    let shown = format!("$ {command}");
    let prefix: String = unwrapped(&shown).chars().take(20).collect();
    assert!(unwrapped(&selected).contains(&prefix), "{selected}");
    assert!(!selected.contains("{artifactPath}"), "{selected}");
    press(&mut review, KeyCode::Enter).await;
    assert_eq!(review.mode(), &Mode::Request);
    let notes = screen(&mut review);
    assert!(
        notes.contains("# Release notes") && notes.contains("- Ready."),
        "{notes}"
    );
    assert!(notes.contains("claimed by you (alice)"), "{notes}");

    press(&mut review, KeyCode::Char('k')).await;
    press(&mut review, KeyCode::Enter).await;
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
    assert!(screen(&mut review).contains("open_release launched."));
    press(&mut review, KeyCode::Char('k')).await;
    press(&mut review, KeyCode::Enter).await;
    assert!(screen(&mut review).contains("notes_release finished."));

    // RED with an empty reason fails validation and keeps the form.
    press(&mut review, KeyCode::Char('r')).await;
    press(&mut review, ctrl('s')).await;
    let invalid = screen(&mut review);
    assert!(
        invalid.contains(r#"- instancePath "/reason": "" is shorter than 1 character"#),
        "{invalid}"
    );
    assert!(matches!(review.mode(), Mode::Form(_)));
    let view = store::read_request(&fixture.state, &id).await.unwrap();
    assert_eq!(view.request.status.as_str(), "WAITING_HUMAN");
    press(&mut review, KeyCode::Esc).await;
    press(&mut review, KeyCode::Char('g')).await;
    assert!(screen(&mut review).contains("approved*: true (fixed)"));
    // Nothing else waits after the submission, so the review hands back to its caller.
    assert_eq!(press(&mut review, ctrl('s')).await, Action::Quit);

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
    assert!(screen(&mut review).contains("claimed by bob · read-only"));
    assert_eq!(press(&mut review, KeyCode::Char('c')).await, Action::None);
    assert!(screen(&mut review).contains("Claimed by bob; read-only for alice."));
    for key in [
        KeyEvent::from(KeyCode::Char('g')),
        KeyEvent::from(KeyCode::Char('u')),
        ctrl('s'),
    ] {
        assert_eq!(press(&mut review, key).await, Action::None);
        assert!(screen(&mut review).contains("Claim this request before reviewing it."));
    }
    assert_eq!(fixture.claim(&id).await.as_deref(), Some("bob"));
    human::unclaim(&receipts, &id, "bob").await.unwrap();
    review.refresh().await;

    // c claims, u releases, and another reviewer can then claim.
    press(&mut review, KeyCode::Char('c')).await;
    assert_eq!(fixture.claim(&id).await.as_deref(), Some("alice"));
    press(&mut review, KeyCode::Char('u')).await;
    assert!(screen(&mut review).contains("Claim released."));
    assert_eq!(fixture.claim(&id).await, None);
    human::claim(&receipts, &id, "bob").await.unwrap();
    human::unclaim(&receipts, &id, "bob").await.unwrap();

    // Quitting after a claim this session took asks; releasing frees it for others.
    press(&mut review, KeyCode::Char('c')).await;
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
    // Bracketed paste is on while the review runs and off again before the screen is restored.
    let on = output.find("\x1b[?2004h").expect("bracketed paste enabled");
    let off = output
        .rfind("\x1b[?2004l")
        .expect("bracketed paste disabled");
    assert!(on < off, "{output:?}");
}
