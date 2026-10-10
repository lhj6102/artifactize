use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use artifactize::{
    monitor::{self, Completion, EvalView, Kind, Monitor, Node, NotRun, Target, Upstream},
    store::{self, RequestView, RunView},
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Terminal, backend::TestBackend};
use rusqlite::Connection;
use serde_json::{Value, json};
use time::OffsetDateTime;

mod support;

struct Fixture {
    _root: tempfile::TempDir,
    alpha: PathBuf,
    beta: PathBuf,
    state: PathBuf,
    release: PathBuf,
}

fn declare(path: &Path, declaration: Value) {
    fs::create_dir_all(path).unwrap();
    support::declaration::write(path.join("index.artf"), declaration.to_string()).unwrap();
}

fn eval(id: &str, profile: Value, instruction: &str) -> Value {
    json!({
        "id":id,
        "title":format!("Check {id}"),
        "profile":profile,
        "payload":{"instruction":instruction},
    })
}

fn runtime(command: &str, args: &[&str]) -> Value {
    json!({"kind":"runtime","command":command,"args":args,"timeout_ms":20000})
}

impl Fixture {
    /// Repository alpha has a cycle, a child, a mount, and fingerprint-cached GREEN/RED results;
    /// beta has a long-running eval gated on a release file and a Human eval.
    fn new() -> Self {
        let root = support::os::tempdir();
        let alpha = root.path().join("alpha");
        let beta = root.path().join("beta");
        let release = root.path().join("release");
        declare(
            &alpha.join("input"),
            json!({"name":"input","basis":true,"fingerprint":{}}),
        );
        for name in ["checkout", "search"] {
            declare(
                &alpha.join("scenarios").join(name),
                json!({"name":name,
                    "evals":[eval("review", runtime("true", &[]), "Inspect {input}.")]}),
            );
        }
        declare(
            &alpha.join("cycle-a"),
            json!({"name":"cycle-a","fingerprint":{"script":{"command":"echo","args":["a-v1"]}},
                "evals":[eval("check", runtime("true", &[]), "Check {cycle-b}.")]}),
        );
        declare(
            &alpha.join("cycle-b"),
            json!({"name":"cycle-b","mounts":{"base":"input"},
                "fingerprint":{"script":{"command":"echo","args":["b-v1"]}},
                "evals":[eval("check", runtime("true", &[]), "Check {cycle-a}.")]}),
        );
        declare(
            &alpha.join("red"),
            json!({
                "name":"red",
                "fingerprint":{"script":{"command":"echo","args":["red-v1"]}},
                "evals":[
                    eval("check", runtime("sh", &["-c", "echo finding; exit 7"]), "Check {input}."),
                ],
            }),
        );
        declare(
            &alpha.join("red/part"),
            json!({"name":"part","basis":true,"fingerprint":{}}),
        );
        let wait = format!(
            "while [ ! -e '{}' ]; do sleep 0.05; done",
            release.display()
        );
        declare(
            &beta.join("slow"),
            json!({"name":"slow","evals":[eval("wait", runtime("sh", &["-c", &wait]), "Wait.")]}),
        );
        declare(
            &beta.join("human"),
            json!({"name":"human","evals":[eval("review", json!({"kind":"human"}), "Approve.")]}),
        );
        Self {
            state: root.path().join("state"),
            _root: root,
            alpha,
            beta,
            release,
        }
    }

    fn command(&self, repo: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            // The declarations name `sh`, `echo` and `true` for PATH to find.
            .env("PATH", support::os::path())
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(&self.state);
        command
    }

    fn json(&self, repo: &Path, args: &[&str], code: i32) -> Value {
        let output = self
            .command(repo)
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Two finished alpha Runs; the second reuses the fingerprint-cached results of the first.
    fn seed(&self) -> (String, String) {
        let first = self.json(&self.alpha, &["verify", "--all"], 1);
        let second = self.json(&self.alpha, &["verify", "--all"], 1);
        (
            first["id"].as_str().unwrap().into(),
            second["id"].as_str().unwrap().into(),
        )
    }

    async fn load(&self, id: &str) -> (RunView, Vec<RequestView>) {
        (
            store::read_run(&self.state, id).await.unwrap(),
            store::read_requests(&self.state, Some(id)).await.unwrap(),
        )
    }
}

fn screen(monitor: &mut Monitor) -> String {
    let mut terminal = Terminal::new(TestBackend::new(200, 60)).unwrap();
    terminal.draw(|frame| monitor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .chunks(buffer.area.width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn press(monitor: &mut Monitor, code: KeyCode) -> monitor::Action {
    monitor.key(KeyEvent::from(code))
}

fn finish(mut child: Child) -> std::process::Output {
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(20));
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("verify did not finish");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
}

#[tokio::test]
async fn live_verify_progress_and_runs_across_repositories() {
    let fixture = Fixture::new();
    let (_, second) = fixture.seed();
    let child = fixture
        .command(&fixture.beta)
        .args(["verify", "--all", "--timeout-ms", "30000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(15));
    let (live, human) = loop {
        let runs = store::read_runs(&fixture.state, Some(&fixture.beta), 10, 0)
            .await
            .unwrap();
        if let Some(run) = runs.first() {
            let (view, requests) = fixture.load(&run.id).await;
            let progress = monitor::progress(&view, &requests, OffsetDateTime::now_utc());
            if !progress.running.is_empty() && !progress.waiting.is_empty() {
                assert_eq!(progress.status.as_str(), "RUNNING");
                assert_eq!(progress.validation, "pending");
                assert_eq!(progress.running[0].0, "slow/wait");
                assert_eq!(
                    progress.waiting[0],
                    ("human/review".into(), "unclaimed".into())
                );
                let human = requests
                    .iter()
                    .find(|view| view.request.eval_id == "human/review")
                    .unwrap();
                break (run.id.clone(), human.request.id.clone());
            }
        }
        assert!(Instant::now() < deadline, "live Run did not appear");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    let mut all = Monitor::new(fixture.state.clone(), None);
    all.refresh().await;
    let list = screen(&mut all);
    assert!(list.contains("all repositories"), "{list}");
    let live_row = list
        .lines()
        .find(|line| line.contains(live.as_str()) && line.contains("RUNNING"))
        .unwrap();
    assert!(
        live_row.contains("RUNNING") && live_row.contains("beta"),
        "{list}"
    );
    assert!(list.contains(&second));
    let mut scoped = Monitor::new(fixture.state.clone(), Some(fixture.alpha.clone()));
    scoped.refresh().await;
    let alpha = screen(&mut scoped);
    assert!(
        alpha.contains(&second) && !alpha.contains(live.as_str()),
        "{alpha}"
    );
    assert_eq!(
        press(&mut scoped, KeyCode::Char('q')),
        monitor::Action::Quit
    );

    // The newest Run is first; opening it shows the live progress pane.
    assert_eq!(press(&mut all, KeyCode::Enter), monitor::Action::Refresh);
    all.refresh().await;
    let progress = screen(&mut all);
    assert!(progress.contains(&format!("Run {live}")), "{progress}");
    assert!(progress.contains("◐ slow/wait  running"), "{progress}");
    assert!(
        progress.contains("? human/review  waiting Human · unclaimed"),
        "{progress}"
    );
    assert!(
        progress.contains("◐ RUNNING · validation pending · ◐1 ?1"),
        "{progress}"
    );

    fs::write(&fixture.release, "").unwrap();
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(15));
    loop {
        all.refresh().await;
        if screen(&mut all).contains("◐ RUNNING · validation pending · ?1 ✓1") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "runtime completion was not observed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    fixture.json(
        &fixture.beta,
        &["request", "claim", &human, "--reviewer", "tester"],
        0,
    );
    assert!(screen(&mut all).contains("? human/review  waiting Human · unclaimed"));
    all.refresh().await;
    assert!(screen(&mut all).contains("claimed by tester"));
    fixture.json(
        &fixture.beta,
        &[
            "request",
            "submit",
            &human,
            "--verdict",
            "GREEN",
            "--reviewer",
            "tester",
        ],
        0,
    );
    let output = finish(child);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    all.refresh().await;
    let done = screen(&mut all);
    assert!(
        done.contains("✓ GREEN · SATISFIED at Run end · ✓2 · took"),
        "{done}"
    );
    assert!(!done.contains("slow/wait  running"));
    assert_eq!(press(&mut all, KeyCode::Esc), monitor::Action::None);
    all.refresh().await;
    assert!(screen(&mut all).contains("Runs (3)"));
    // Esc steps back to Scope and never quits; q does.
    for _ in 0..2 {
        assert_eq!(press(&mut all, KeyCode::Esc), monitor::Action::None);
        assert_eq!(all.focus, monitor::Pane::Repositories);
    }
    assert_eq!(press(&mut all, KeyCode::Char('q')), monitor::Action::Quit);
}

fn flatten<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for node in nodes {
        out.push(node);
        flatten(&node.children, out);
    }
}

fn dump(state: &Path) -> Vec<String> {
    let db = Connection::open(state.join("state.sqlite")).unwrap();
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut rows = Vec::new();
    for table in tables {
        let mut statement = db.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let columns = statement.column_count();
        let mut query = statement.query([]).unwrap();
        while let Some(row) = query.next().unwrap() {
            let values: Vec<_> = (0..columns)
                .map(|index| format!("{:?}", row.get_ref(index).unwrap()))
                .collect();
            rows.push(format!("{table}: {}", values.join("|")));
        }
    }
    rows.sort();
    rows
}

#[tokio::test]
async fn saved_tree_details_without_repository_or_writes() {
    let fixture = Fixture::new();
    let (first, second) = fixture.seed();
    fs::remove_dir_all(&fixture.alpha).unwrap();
    let before = dump(&fixture.state);
    // Readers never need the write lock a concurrent verify holds.
    let writer = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();

    let now = OffsetDateTime::now_utc();
    let (view, requests) = fixture.load(&second).await;
    let reused = view.requests.iter().filter(|request| {
        let source = request.provenance.as_ref();
        source.is_some_and(|source| source.request_id != request.id)
    });
    let work = monitor::progress(&view, &requests, now).work;
    assert!(
        work.contains(&format!(
            "executed {} · reused {}",
            view.run.executions_started,
            reused.count()
        )),
        "{work}"
    );
    let nodes = monitor::tree(&view, &requests, now);
    let mut all = Vec::new();
    flatten(&nodes, &mut all);
    let node = |id: &str| *all.iter().find(|node| node.id == id).unwrap();
    for (id, _) in view.run.definitions.graph().unwrap().artifacts() {
        assert_eq!(
            all.iter()
                .filter(|node| node.id.as_str() == format!("a:{id}"))
                .count(),
            1,
            "{id}"
        );
    }
    let eval = |id: &str| match &node(id).kind {
        Kind::Eval(view) => view.clone(),
        kind => panic!("{kind:?}"),
    };
    for request in &requests {
        assert_ne!(
            eval(&format!("e:{}", request.request.eval_id)),
            EvalView::NotRun(NotRun::Absent)
        );
    }
    for id in ["a:checkout", "a:search"] {
        assert_eq!(node(id).line(), format!("✓ {}  1/1", &id[2..]));
        assert!(node(id).done());
    }
    // Peers are marked, and the cycle is never a wait between them.
    assert_eq!(node("a:cycle-a").marks, "  ↻ cycle-b");
    assert_eq!(node("a:cycle-b").marks, "  ↻ cycle-a");
    for id in ["e:cycle-a/check", "e:cycle-b/check"] {
        let upstream: Vec<_> = node(id)
            .upstream
            .iter()
            .map(|up| up.artifact.as_str())
            .collect();
        assert_eq!(upstream, ["input"], "{id}");
    }
    // One row per eval: relations live in the Artifact detail, not in the tree.
    assert!(all.iter().all(|node| !node.id.starts_with("r:")));
    assert_eq!(eval("e:red/check"), EvalView::Failed { verdict: true });
    let red = node("e:red/check").line();
    assert!(red.starts_with("✗ check  RED · exitCode 7"), "{red}");
    assert_eq!(
        node("e:checkout/review").upstream,
        [Upstream {
            artifact: "input".parse().unwrap(),
            completion: Completion::Complete
        }]
    );
    let waits = monitor::detail(
        &view,
        &requests,
        &Target::Eval("cycle-a/check".parse().unwrap()),
        now,
    );
    assert_eq!(
        waits.field("Waits for"),
        Some("↑ input ✓ complete · mount base → cycle-b")
    );

    let red = monitor::detail(
        &view,
        &requests,
        &Target::Eval("red/check".parse().unwrap()),
        now,
    );
    assert_eq!(red.field("Status"), Some("RED — criteria not met"));
    assert_eq!(red.field("Fingerprint"), Some("red-v1"));
    let covers = red.field("Key covers").unwrap();
    assert!(
        covers.starts_with("input artifactsum:") && covers.contains("\npart artifactsum:"),
        "{covers}"
    );
    assert!(covers.ends_with("\nred red-v1"), "{covers}");
    assert_eq!(red.field("Options"), Some("timeoutMs 20000"));
    let source = red.field("Source").unwrap();
    assert!(
        source.starts_with(&format!("reused from Run {first} request {first}-")),
        "{source}"
    );
    // The Outcome shows the verdict first; the whole result is under What.
    let summary = red.field("Result").unwrap();
    assert!(
        summary.starts_with("verdict RED\n")
            && summary.contains("\nexitCode 7")
            && !summary.contains("stdout"),
        "{summary}"
    );
    let result: Value = serde_json::from_str(red.field("Raw result").unwrap()).unwrap();
    assert_eq!(result["verdict"], "RED");
    assert_eq!(result["exitCode"], 7);
    assert_eq!(result["stdout"], "finding\n");
    assert_eq!(
        red.field("Profile"),
        Some("runtime sh -c echo finding; exit 7")
    );
    assert!(
        red.field("Usage")
            .unwrap()
            .starts_with("reused: spent none")
    );
    let reused = monitor::detail(
        &view,
        &requests,
        &Target::Eval("cycle-a/check".parse().unwrap()),
        now,
    );
    assert_eq!(reused.field("Status"), Some("GREEN — criteria met"));
    assert!(
        reused
            .field("Source")
            .unwrap()
            .starts_with("reused from Run")
    );
    let artifact = monitor::detail(
        &view,
        &requests,
        &Target::Artifact("cycle-b".parse().unwrap()),
        now,
    );
    assert_eq!(artifact.field("Cycle"), Some("↻ cycle-a, cycle-b"));
    assert!(
        artifact
            .field("Inputs")
            .unwrap()
            .contains("input — mount base")
    );
    assert!(
        artifact
            .field("Used by")
            .unwrap()
            .contains("cycle-a — {cycle-b}")
    );
    let (old, old_requests) = fixture.load(&first).await;
    let original = monitor::detail(
        &old,
        &old_requests,
        &Target::Eval("red/check".parse().unwrap()),
        now,
    );
    assert_eq!(original.field("Source"), Some("executed in this Run"));

    let mut monitor = Monitor::new(fixture.state.clone(), None);
    monitor.refresh().await;
    let list = screen(&mut monitor);
    assert!(list.contains(&first) && list.contains(&second), "{list}");
    assert_eq!(
        press(&mut monitor, KeyCode::Enter),
        monitor::Action::Refresh
    );
    monitor.refresh().await;
    let run = screen(&mut monitor);
    assert!(
        run.contains("▸ ✓ checkout ") && run.contains("NOT SATISFIED at Run end"),
        "{run}"
    );
    assert_eq!(
        monitor::detail(&view, &requests, &Target::Run, now).field("Validation"),
        Some("NOT SATISFIED at Run end (unmet: red)")
    );
    // The cursor starts on the RED eval.
    assert_eq!(
        monitor.target(),
        Some(Target::Eval("red/check".parse().unwrap()))
    );
    // Folded Artifacts stay reachable: unfold an Artifact to select its eval.
    for _ in 0..40 {
        press(&mut monitor, KeyCode::Up);
        screen(&mut monitor);
    }
    let mut steps = 0;
    while monitor.target() != Some(Target::Artifact("checkout".parse().unwrap())) {
        press(&mut monitor, KeyCode::Down);
        screen(&mut monitor);
        steps += 1;
        assert!(steps < 40, "Artifact node not reachable");
    }
    press(&mut monitor, KeyCode::Char('l'));
    screen(&mut monitor);
    press(&mut monitor, KeyCode::Down);
    let expanded = screen(&mut monitor);
    assert!(expanded.contains("▾ ✓ checkout "), "{expanded}");
    assert!(expanded.contains("✓ review "), "{expanded}");
    assert_eq!(
        monitor.target(),
        Some(Target::Eval("checkout/review".parse().unwrap()))
    );
    for _ in 0..3 {
        monitor.refresh().await;
        screen(&mut monitor);
    }
    writer.execute_batch("ROLLBACK").unwrap();
    drop(writer);
    assert_eq!(dump(&fixture.state), before);
}

/// Gates in the tree follow the evidence and effective statuses the Run's graph used:
/// saved results outside a partial Run, and GREEN results masked behind a RED upstream.
#[tokio::test]
async fn tree_gates_follow_the_runs_evidence_and_effective_statuses() {
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    let flag = root.path().join("fail");
    let check = format!("test ! -e '{}'", flag.display());
    let fingerprint =
        |name: &str| json!({"script":{"command":"echo","args":[format!("{name}-v1")]}});
    declare(
        &repo.join("a"),
        json!({"name":"a","fingerprint":fingerprint("a"),
            "evals":[eval("x", runtime("sh", &["-c", &check]), "Check.")]}),
    );
    declare(
        &repo.join("b"),
        json!({"name":"b","fingerprint":fingerprint("b"),
            "evals":[eval("x", runtime("true", &[]), "Check {a}.")]}),
    );
    declare(
        &repo.join("c"),
        json!({"name":"c","fingerprint":fingerprint("c"),"evals":[
            eval("x", runtime("true", &[]), "Check {b}."),
            eval("y", runtime("true", &[]), "Check {b}.")]}),
    );
    let verify = |args: &[&str], code: i32| -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .env("PATH", support::os::path())
            .arg("--repo")
            .arg(&repo)
            .arg("--state-dir")
            .arg(&state)
            .arg("verify")
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        value["id"].as_str().unwrap().to_owned()
    };
    let load = |id: String| {
        let state = state.clone();
        async move {
            (
                store::read_run(&state, &id).await.unwrap(),
                store::read_requests(&state, Some(&id)).await.unwrap(),
            )
        }
    };
    let now = OffsetDateTime::now_utc();
    verify(&["--all"], 0);

    // A partial forced Run of c: a and b have no requests, only saved results.
    let (view, requests) = load(verify(&["c", "--force", "--jobs", "1"], 0)).await;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        view.run.evidence,
        [
            (
                "a/x".parse().unwrap(),
                artifactize::types::RequestStatus::Green
            ),
            (
                "b/x".parse().unwrap(),
                artifactize::types::RequestStatus::Green
            ),
        ]
        .into()
    );
    let nodes = monitor::tree(&view, &requests, now);
    let mut all = Vec::new();
    flatten(&nodes, &mut all);
    let node = |id: &str| *all.iter().find(|node| node.id == id).unwrap();
    assert_eq!(
        node("e:b/x").kind,
        Kind::Eval(EvalView::Done(monitor::Source::Saved))
    );
    assert_eq!(
        node("e:c/y").kind,
        Kind::Eval(EvalView::Done(monitor::Source::Executed))
    );
    assert!(all.iter().all(|node| !node.changed));

    // a turns RED; b's reused GREEN is masked BLOCKED, and c is blocked by b.
    fs::write(&flag, "").unwrap();
    let (view, requests) = load(verify(
        &["--evals", "a/x,c/x,c/y", "--recursive", "--force"],
        1,
    ))
    .await;
    let saved = |id: &str| {
        serde_json::to_value(&view.run.validation).unwrap()["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["id"] == id)
            .unwrap()["status"]
            .clone()
    };
    assert_eq!(
        (saved("b"), saved("c")),
        (json!("BLOCKED"), json!("BLOCKED"))
    );
    let nodes = monitor::tree(&view, &requests, now);
    let mut all = Vec::new();
    flatten(&nodes, &mut all);
    let node = |id: &str| *all.iter().find(|node| node.id == id).unwrap();
    assert_eq!(
        node("e:a/x").kind,
        Kind::Eval(EvalView::Failed { verdict: true })
    );
    assert_eq!(
        node("e:b/x").kind,
        Kind::Eval(EvalView::Done(monitor::Source::Reused))
    );
    assert_eq!(node("a:b").line(), "⊘ b  done, but blocked by a  0/1");
    assert_eq!(
        node("e:c/x").kind,
        Kind::Eval(EvalView::BlockedBy(vec!["b".parse().unwrap()]))
    );
    assert_eq!(node("a:c").line(), "⊘ c  not run: blocked by b  0/2");
    assert!(
        all.iter().all(|node| !node.changed),
        "nothing changed after the Run"
    );
}

#[test]
fn pty_session_restores_the_terminal_on_quit() {
    let root = support::os::tempdir();
    let both = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .args(["--repo", ".", "monitor", "--all"])
        .output()
        .unwrap();
    assert_eq!(both.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&both.stderr).contains("--repo or --all"));
    let state = root.path().join("state");
    let (mut terminal, receiver) = support::os::PseudoTerminal::start(
        Path::new(env!("CARGO_BIN_EXE_artifactize")),
        &["--state-dir", state.to_str().unwrap(), "monitor", "--all"],
        &[],
        100,
        30,
    );
    let wait = support::os::patience(Duration::from_secs(10));
    let mut output = Vec::new();
    // Ratatui skips unchanged blank cells, so wait for one word of the refreshed empty list.
    let frame = "verify`.";
    while !String::from_utf8_lossy(&output).contains(frame) {
        output.extend(receiver.recv_timeout(wait).expect("monitor did not draw"));
    }
    terminal.type_text(b"q");
    assert!(terminal.finish());
    // The output ends once the terminal has closed.
    while let Ok(chunk) = receiver.recv_timeout(wait) {
        output.extend(chunk);
    }
    support::os::assert_restored(&String::from_utf8_lossy(&output), frame);
}
