use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use artifactize::{
    monitor::{self, Monitor, Node, Target},
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
    fs::write(path.join("artifactize.json"), declaration.to_string()).unwrap();
}

fn eval(id: &str, profile: Value, instruction: &str) -> Value {
    json!({"id":id,"title":format!("Check {id}"),"profile":profile,"payload":{"instruction":instruction}})
}

fn runtime(command: &str, args: &[&str]) -> Value {
    json!({"kind":"runtime","command":command,"args":args,"timeoutMs":20000})
}

impl Fixture {
    /// Repository alpha has a family, a cycle, a child, a mount, and fingerprint-cached GREEN/RED results;
    /// beta has a long-running eval gated on a release file and a Human eval.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let alpha = root.path().join("alpha");
        let beta = root.path().join("beta");
        let release = root.path().join("release");
        declare(
            &alpha.join("input"),
            json!({"name":"input","basis":true,"fingerprint":{}}),
        );
        declare(
            &alpha.join("scenarios"),
            json!({"name":"scenarios",
                "family":{"instances":{"checkout":{"material":["material.txt"]},"search":{"material":["material.txt"]}}},
                "evals":[eval("review", runtime("true", &[]), "Inspect {input}.")]}),
        );
        fs::write(alpha.join("scenarios/material.txt"), "material").unwrap();
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
            json!({"name":"red","fingerprint":{"script":{"command":"echo","args":["red-v1"]}},
                "evals":[eval("check", runtime("sh", &["-c", "echo finding; exit 7"]), "Check {input}.")]}),
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
        .find(|line| line.contains(live.as_str()))
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
    assert!(progress.contains("running slow/wait"), "{progress}");
    assert!(
        progress.contains("waiting Human human/review · unclaimed"),
        "{progress}"
    );
    assert!(progress.contains("RUNNING 1") && progress.contains("WAITING_HUMAN 1"));

    fs::write(&fixture.release, "").unwrap();
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(15));
    loop {
        all.refresh().await;
        if screen(&mut all).contains("GREEN 1") {
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
    assert!(screen(&mut all).contains("waiting Human human/review · unclaimed"));
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
        done.contains("GREEN 2") && done.contains("validation SATISFIED"),
        "{done}"
    );
    assert!(!done.contains("running slow/wait"));
    assert_eq!(press(&mut all, KeyCode::Esc), monitor::Action::Refresh);
    all.refresh().await;
    assert!(screen(&mut all).contains("Runs (3)"));
    assert_eq!(press(&mut all, KeyCode::Esc), monitor::Action::Quit);
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
    for id in view.run.definitions["artifacts"]
        .as_object()
        .unwrap()
        .keys()
    {
        assert_eq!(
            all.iter()
                .filter(|node| node.id == format!("a:{id}"))
                .count(),
            1,
            "{id}"
        );
    }
    for request in &requests {
        assert!(
            node(&format!("e:{}", request.request.eval_id))
                .status
                .is_some()
        );
    }
    let family = nodes.iter().find(|node| node.id == "f:scenarios").unwrap();
    assert_eq!(
        family
            .children
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["a:checkout", "a:search"]
    );
    assert_eq!(family.status.as_deref(), Some("GREEN"));
    assert!(node("a:cycle-a").text.contains('↻') && node("a:cycle-b").text.contains('↻'));
    let cycle_input = node("a:cycle-a")
        .children
        .iter()
        .find(|node| node.id == "r:cycle-b")
        .unwrap();
    assert!(
        cycle_input.text.contains("{cycle-b} in cycle-a/check ↻"),
        "{}",
        cycle_input.text
    );
    let mounted = &node("a:cycle-b").children;
    assert!(mounted.iter().any(|node| node.text.contains("mount base")));
    assert!(
        node("a:red")
            .children
            .iter()
            .any(|node| node.id == "r:part" && node.text.contains("child part"))
    );
    assert_eq!(node("e:red/check").status.as_deref(), Some("RED"));
    assert!(node("e:checkout/review").text.contains("← input"));

    let red = monitor::detail(&view, &requests, &Target::Eval("red/check".into()), now);
    assert_eq!(red.field("Status"), Some("RED — criteria not met"));
    assert_eq!(red.field("Fingerprint"), Some("red-v1"));
    let covers = red.field("Key covers").unwrap();
    assert!(
        covers.starts_with("input content:") && covers.contains("\npart content:"),
        "{covers}"
    );
    assert!(covers.ends_with("\nred red-v1"), "{covers}");
    assert_eq!(red.field("Options"), Some("timeoutMs 20000"));
    let source = red.field("Source").unwrap();
    assert!(
        source.starts_with(&format!("reused from Run {first} request {first}-")),
        "{source}"
    );
    let result: Value = serde_json::from_str(red.field("Result").unwrap()).unwrap();
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
    let reused = monitor::detail(&view, &requests, &Target::Eval("cycle-a/check".into()), now);
    assert_eq!(reused.field("Status"), Some("GREEN — criteria met"));
    assert!(
        reused
            .field("Source")
            .unwrap()
            .starts_with("reused from Run")
    );
    let artifact = monitor::detail(&view, &requests, &Target::Artifact("cycle-b".into()), now);
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
    let original = monitor::detail(&old, &old_requests, &Target::Eval("red/check".into()), now);
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
        run.contains("family scenarios") && run.contains("validation NOT SATISFIED (unmet: red)"),
        "{run}"
    );
    // Collapsed families stay reachable: walk to the family node and expand it.
    let mut steps = 0;
    while monitor.target() != Some(Target::Family("scenarios".into())) {
        press(&mut monitor, KeyCode::Down);
        screen(&mut monitor);
        steps += 1;
        assert!(steps < 40, "family node not reachable");
    }
    assert!(!screen(&mut monitor).contains("checkout  GREEN"));
    press(&mut monitor, KeyCode::Right);
    screen(&mut monitor);
    press(&mut monitor, KeyCode::Down);
    let expanded = screen(&mut monitor);
    assert!(expanded.contains("checkout  GREEN 1/1"), "{expanded}");
    assert_eq!(monitor.target(), Some(Target::Artifact("checkout".into())));
    for _ in 0..3 {
        monitor.refresh().await;
        screen(&mut monitor);
    }
    writer.execute_batch("ROLLBACK").unwrap();
    drop(writer);
    assert_eq!(dump(&fixture.state), before);
}

/// Needs a pseudo-terminal from script(1); Windows has ConPTY, but no such tool to drive it.
#[cfg(unix)]
#[test]
fn pty_session_restores_the_terminal_on_quit() {
    use std::{
        io::{Read, Write},
        sync::mpsc,
    };

    let root = tempfile::tempdir().unwrap();
    let both = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .args(["--repo", ".", "monitor", "--all"])
        .output()
        .unwrap();
    assert_eq!(both.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&both.stderr).contains("--repo or --all"));
    let command = format!(
        "stty cols 100 rows 30; exec '{}' --state-dir '{}' monitor --all",
        env!("CARGO_BIN_EXE_artifactize"),
        root.path().join("state").display()
    );
    let mut child = Command::new("script")
        .args(["-qec", &command, "/dev/null"])
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
    let mut output = Vec::new();
    // Ratatui skips unchanged blank cells, so wait for one word of the refreshed empty list.
    while !String::from_utf8_lossy(&output).contains("verify`.") {
        let chunk = receiver.recv_timeout(Duration::from_secs(10));
        output.extend(chunk.expect("monitor did not draw"));
    }
    child.stdin.take().unwrap().write_all(b"q").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Ok(chunk) = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        output.extend(chunk);
    }
    assert!(finish(child).status.success());
    let output = String::from_utf8_lossy(&output);
    assert!(output.starts_with("\x1b[?1049h"), "{output:?}");
    assert!(output.ends_with("\x1b[?25h\x1b[?1049l"), "{output:?}");
}
