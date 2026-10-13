//! C1–C3: observable cancellation, deferred sign-offs and scheduling seams.
use artifactize_core::{
    changes::Subscription,
    human, runs,
    store::{self, Receipts, RunView},
    types::{RequestStatus, RunId, RunStatus},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
    script: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = support::os::tempdir();
        let repo = support::os::canonical(root.path()).join("repo");
        fs::create_dir(&repo).unwrap();
        let script = root.path().join("controlled.py");
        support::os::write_python_script(
            &script,
            "import pathlib, sys, time\nname = sys.argv[1]\npathlib.Path(name + '-ready').write_text('ready')\nwhile not pathlib.Path(name + '-release').exists(): time.sleep(0.01)\nsys.exit(int(pathlib.Path(name + '-release').read_text()))\n",
        );
        Self {
            repo,
            state: root.path().join("state"),
            script,
            _root: root,
        }
    }
    fn runtime(&self, id: &str, deps: &[&str]) -> Value {
        json!({"id":id,"title":id,"profile":{"kind":"runtime","command":support::os::python_program(),"args":[support::os::path_text(&self.script),id]},"payload":{"instruction":format!("Check {}.", deps.iter().map(|name| format!("{{{name}}}")).collect::<Vec<_>>().join(" "))}})
    }
    fn declare(&self, name: &str, evals: Vec<Value>) {
        let folder = self.repo.join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("fingerprint"), name).unwrap();
        support::declaration::write(folder.join("index.artf"), json!({"name":name,"fingerprint":{"script":{"command":support::os::cat_program(),"args":["fingerprint"]}},"evals":evals}).to_string()).unwrap();
    }
    fn command(&self) -> Command {
        let mut command = Command::new(support::os::cli());
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .env("ARTIFACTIZE_REMOTE", "off");
        command
    }
    fn context(&self) -> runs::Context {
        runs::Context::new(&self.repo, Some(&self.state), &support::os::cli()).unwrap()
    }
    fn submit(&self, args: &[&str], provider: Option<&support::FakeProvider>) -> RunId {
        let mut command = self.command();
        command.args(["verify", "--all", "--json"]).args(args);
        if let Some(provider) = provider {
            command
                .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
                .env("OPENAI_API_KEY", "fake-openai-key");
        }
        let output = support::os::run(&mut command);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["run"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }
    async fn until(&self, id: &RunId, predicate: impl Fn(&RunView) -> bool) -> RunView {
        let mut changes = Subscription::new(&self.state).await;
        let result = tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
            loop {
                let view = runs::view(&self.context(), id).await.unwrap();
                if predicate(&view) {
                    return view;
                }
                assert_eq!(
                    view.run.status(),
                    RunStatus::Running,
                    "{}",
                    serde_json::to_string_pretty(&view).unwrap()
                );
                changes.next_reconciled().await;
            }
        })
        .await;
        if result.is_err() {
            let view = runs::view(&self.context(), id).await.unwrap();
            panic!(
                "observable state not reached: {}\n{}",
                serde_json::to_string_pretty(&view).unwrap(),
                fs::read_to_string(
                    self.state
                        .join("runs")
                        .join(id.as_str())
                        .join("worker.stderr")
                )
                .unwrap_or_default()
            );
        }
        result.unwrap()
    }
    async fn settled(&self, id: &RunId) -> RunView {
        let observation = runs::wait(
            &self.context(),
            id,
            Some(support::os::patience(Duration::from_secs(30))),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(observation.state.is_terminal(), "{observation:?}");
        runs::view(&self.context(), id).await.unwrap()
    }
    fn release(&self, artifact: &str, eval: &str, code: u8) {
        fs::write(
            self.repo.join(artifact).join(format!("{eval}-release")),
            code.to_string(),
        )
        .unwrap();
    }
    fn ready(&self, artifact: &str, eval: &str) -> bool {
        self.repo
            .join(artifact)
            .join(format!("{eval}-ready"))
            .exists()
    }
    async fn sign(&self, view: &RunView, eval: &str, verdict: &str) {
        let request = view
            .requests
            .iter()
            .find(|request| request.eval_id.as_str() == eval)
            .unwrap();
        let receipts = Receipts::open(&self.state, &self.repo).await.unwrap();
        let reviewer = "reviewer".parse().unwrap();
        human::claim(&receipts, &request.id, &reviewer)
            .await
            .unwrap();
        human::submit(
            &receipts,
            &request.id,
            &reviewer,
            &json!({"verdict":verdict}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    }
}
fn status(view: &RunView, eval: &str) -> RequestStatus {
    view.requests
        .iter()
        .find(|request| request.eval_id.as_str() == eval)
        .unwrap()
        .status()
}
fn human_eval(deps: &[&str], blocking: bool) -> Value {
    let mut eval = json!({"id":"sign","title":"Sign off","profile":{"kind":"human"},"payload":{"instruction":format!("Review {}.", deps.iter().map(|name| format!("{{{name}}}")).collect::<Vec<_>>().join(" "))}});
    if blocking {
        eval["gate"] = json!("blocking");
    }
    eval
}

#[tokio::test]
async fn c1_speculation_starts_all_kinds_cancels_transitive_red_and_keeps_green() {
    for verdict in ["RED", "GREEN"] {
        let fixture = Fixture::new();
        let gate = support::Gate::default();
        gate.close();
        let started = Arc::new(AtomicBool::new(false));
        let provider = support::FakeProvider::start({
            let gate = gate.clone();
            let started = started.clone();
            move |request| {
                started.store(true, Ordering::SeqCst);
                gate.wait();
                support::openai::completed(
                    request,
                    vec![support::openai::message(
                        &json!({"verdict":"GREEN"}).to_string(),
                    )],
                    support::openai::usage(1, 1),
                )
            }
        });
        fixture.declare("up", vec![fixture.runtime("check", &[])]);
        fixture.declare("down", vec![fixture.runtime("runtime", &["up"]),
            json!({"id":"agent","title":"Agent","profile":{"kind":"agent","backend":"openai","model":"fake-exact-model"},"payload":{"instruction":"Review {up}."}}), human_eval(&["up"], false)]);
        fixture.declare("leaf", vec![fixture.runtime("check", &["down"])]);
        let run = fixture.submit(&["--speculate", "--jobs", "5"], Some(&provider));
        let active = fixture
            .until(&run, |view| {
                fixture.ready("up", "check")
                    && fixture.ready("down", "runtime")
                    && fixture.ready("leaf", "check")
                    && started.load(Ordering::SeqCst)
                    && status(view, "down/sign") == RequestStatus::WaitingHuman
            })
            .await;
        assert_eq!(
            status(&active, "up/check"),
            RequestStatus::Running,
            "downstream started before upstream ended"
        );
        let runtime_child = active
            .requests
            .iter()
            .find(|request| request.eval_id.as_str() == "down/runtime")
            .unwrap()
            .child
            .clone()
            .unwrap();
        if verdict == "GREEN" {
            fixture.sign(&active, "down/sign", "GREEN").await;
            fixture.release("down", "runtime", 0);
            fixture.release("leaf", "check", 0);
            gate.open();
            fixture
                .until(&run, |view| {
                    status(view, "down/agent") == RequestStatus::Green
                        && status(view, "down/runtime") == RequestStatus::Green
                        && status(view, "leaf/check") == RequestStatus::Green
                })
                .await;
            fixture.release("up", "check", 0);
            let ended = fixture.settled(&run).await;
            assert_eq!(ended.run.status(), RunStatus::Green);
            assert_eq!(
                ended.run.executions_started, 4,
                "upstream GREEN retains completed speculation"
            );
            assert_eq!(
                artifactize_core::cache::list(&fixture.state, false)
                    .await
                    .unwrap()
                    .len(),
                5
            );
        } else {
            fixture.release("up", "check", 1);
            let ended = fixture.settled(&run).await;
            gate.open();
            assert_eq!(ended.run.status(), RunStatus::Red);
            for eval in ["down/runtime", "down/agent", "down/sign", "leaf/check"] {
                assert_eq!(status(&ended, eval), RequestStatus::Blocked);
            }
            let receipts = Receipts::open(&fixture.state, &fixture.repo).await.unwrap();
            let sign = ended
                .requests
                .iter()
                .find(|request| request.eval_id.as_str() == "down/sign")
                .unwrap();
            assert!(
                human::claim(&receipts, &sign.id, &"reviewer".parse().unwrap())
                    .await
                    .is_err()
            );
            assert!(
                store::read_waiting(&fixture.state, None)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(!support::os::running(runtime_child.pid.get()));
            let cache = artifactize_core::cache::list(&fixture.state, false)
                .await
                .unwrap();
            assert!(
                cache
                    .iter()
                    .all(|entry| entry.eval_id.as_str() == "up/check"),
                "cancelled work has no reusable record"
            );
        }
    }
}

#[tokio::test]
async fn c2_deferred_signoffs_are_late_provisional_terminal_reusable_and_strict_when_blocking() {
    for verdict in ["GREEN", "RED"] {
        let fixture = Fixture::new();
        fixture.declare(
            "up",
            vec![fixture.runtime("check", &[]), human_eval(&[], false)],
        );
        fixture.declare("down", vec![fixture.runtime("check", &["up"])]);
        let run = fixture.submit(&["--defer-human", "--request-key", "mode-choice"], None);
        let active = fixture.until(&run, |_| fixture.ready("up", "check")).await;
        assert!(
            store::read_waiting(&fixture.state, None)
                .await
                .unwrap()
                .is_empty(),
            "no person reviews before machine evidence finishes"
        );
        assert_eq!(status(&active, "up/check"), RequestStatus::Running);
        fixture.release("up", "check", 0);
        let awaiting = fixture
            .until(&run, |view| {
                status(view, "up/sign") == RequestStatus::Deferred && fixture.ready("down", "check")
            })
            .await;
        assert_eq!(
            status(&awaiting, "down/check"),
            RequestStatus::Running,
            "dependents run before sign-off"
        );
        fixture.release("down", "check", 0);
        let ended = fixture.settled(&run).await;
        assert_eq!(ended.run.status(), RunStatus::SignoffsPending);
        assert_eq!(status(&ended, "down/check"), RequestStatus::Provisional);
        assert!(
            !ended
                .run
                .validation
                .snapshot()
                .unwrap()
                .satisfied
                .value()
                .copied()
                .unwrap()
        );
        let status_output = fixture
            .command()
            .args(["status", "--all", "--json"])
            .output()
            .unwrap();
        assert_eq!(status_output.status.code(), Some(1));
        let current: Value = serde_json::from_slice(&status_output.stdout).unwrap();
        let state_of = |id: &str| {
            current["evals"]
                .as_array()
                .unwrap()
                .iter()
                .find(|eval| eval["id"] == id)
                .unwrap()["state"]
                .clone()
        };
        assert_eq!(state_of("up/sign"), "DEFERRED");
        assert_eq!(state_of("down/check"), "PROVISIONAL");
        let query = artifactize_core::review::search::DEFAULT.parse().unwrap();
        let found = artifactize_core::review::search::search(
            &fixture.state,
            &query,
            &"reviewer".parse().unwrap(),
            0,
        )
        .await
        .unwrap();
        assert_eq!(
            found.requests.len(),
            1,
            "default review lists deferred sign-offs"
        );
        let result = runs::result(&fixture.context(), &run, None).await.unwrap();
        assert!(result.ready);
        assert_eq!(
            serde_json::to_value(result.verdict).unwrap(),
            "SIGNOFFS_PENDING"
        );
        assert_eq!(
            store::read_waiting_originals(&fixture.state, None)
                .await
                .unwrap()
                .len(),
            1
        );
        let retry = fixture.submit(&["--speculate", "--request-key", "mode-choice"], None);
        assert_eq!(retry, run);
        assert!(
            runs::view(&fixture.context(), &retry)
                .await
                .unwrap()
                .run
                .modes
                .defer_human
        );
        assert!(
            !runs::view(&fixture.context(), &retry)
                .await
                .unwrap()
                .run
                .modes
                .speculate
        );
        let output = fixture
            .command()
            .args(["verify", "--all", "--wait", "--request-key", "mode-choice"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(5));
        let ci = fixture.submit(&["--defer-human", "--reuse-only", "agent,human"], None);
        let ci = fixture.settled(&ci).await;
        assert_eq!(
            ci.run.status(),
            RunStatus::Incomplete,
            "CI never counts deferred evidence as approval"
        );
        fixture.sign(&ended, "up/sign", verdict).await;
        let next = fixture.submit(&["--defer-human"], None);
        let next = fixture.settled(&next).await;
        assert_eq!(next.run.executions_started, 0);
        if verdict == "GREEN" {
            assert_eq!(next.run.status(), RunStatus::Green);
            assert!(
                next.run
                    .validation
                    .snapshot()
                    .unwrap()
                    .satisfied
                    .value()
                    .copied()
                    .unwrap()
            );
            assert_eq!(status(&next, "down/check"), RequestStatus::Green);
        } else {
            assert_eq!(next.run.status(), RunStatus::Red);
            assert_eq!(status(&next, "down/check"), RequestStatus::Blocked);
        }
    }
    let fixture = Fixture::new();
    fixture.declare(
        "up",
        vec![fixture.runtime("check", &[]), human_eval(&[], true)],
    );
    fixture.declare("down", vec![fixture.runtime("check", &["up"])]);
    let run = fixture.submit(&["--defer-human"], None);
    fixture.until(&run, |_| fixture.ready("up", "check")).await;
    assert!(
        store::read_waiting(&fixture.state, None)
            .await
            .unwrap()
            .is_empty(),
        "blocking sign-offs also wait for machine evidence before issuance"
    );
    fixture.release("up", "check", 0);
    let awaiting = fixture
        .until(&run, |view| {
            status(view, "up/sign") == RequestStatus::WaitingHuman
                && status(view, "up/check") == RequestStatus::Green
        })
        .await;
    assert!(
        !fixture.ready("down", "check"),
        "gate=blocking retains its pending gate"
    );
    fixture.sign(&awaiting, "up/sign", "GREEN").await;
    fixture
        .until(&run, |_| fixture.ready("down", "check"))
        .await;
    fixture.release("down", "check", 0);
    assert_eq!(fixture.settled(&run).await.run.status(), RunStatus::Green);
}

#[tokio::test]
async fn c3_capacity_prioritizes_unblockable_longest_paths_before_speculation() {
    let fixture = Fixture::new();
    fixture.declare("a-short", vec![fixture.runtime("check", &[])]);
    fixture.declare("b-tail", vec![fixture.runtime("check", &["c-middle"])]);
    fixture.declare("c-middle", vec![fixture.runtime("check", &["z-root"])]);
    fixture.declare("z-root", vec![fixture.runtime("check", &[])]);
    let run = fixture.submit(
        &["--speculate", "--jobs", "1", "--max-executions", "4"],
        None,
    );
    fixture
        .until(&run, |_| fixture.ready("z-root", "check"))
        .await;
    assert!(!fixture.ready("a-short", "check"));
    assert!(!fixture.ready("c-middle", "check"));
    fixture.release("z-root", "check", 0);
    fixture
        .until(&run, |_| fixture.ready("c-middle", "check"))
        .await;
    assert!(!fixture.ready("a-short", "check"));
    assert!(!fixture.ready("b-tail", "check"));
    fixture.release("c-middle", "check", 0);
    fixture
        .until(&run, |_| fixture.ready("a-short", "check"))
        .await;
    assert!(!fixture.ready("b-tail", "check"));
    fixture.release("a-short", "check", 0);
    fixture
        .until(&run, |_| fixture.ready("b-tail", "check"))
        .await;
    fixture.release("b-tail", "check", 0);
    let ended = fixture.settled(&run).await;
    assert_eq!(ended.run.status(), RunStatus::Green);
    assert_eq!(ended.run.executions_started, 4);

    // A ready Agent also preempts speculative work holding its machine-wide backend
    // slot. Cancellation releases capacity before the ready review is admitted.
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.state).unwrap();
    fs::write(
        fixture.state.join("limits.json"),
        r#"{"backends":{"openai":1}}"#,
    )
    .unwrap();
    fixture.declare("up", vec![human_eval(&[], false)]);
    fixture.declare("trigger", vec![human_eval(&[], false)]);
    let agent = |title: &str, dependency: &str| json!({"id":"check","title":title,"profile":{"kind":"agent","backend":"openai","model":"fake-exact-model"},"payload":{"instruction":format!("{title}: review {{{dependency}}}.")}});
    fixture.declare("a-spec", vec![agent("Speculative", "up")]);
    fixture.declare("b-ready", vec![agent("Ready", "trigger")]);
    let gate = support::Gate::default();
    gate.close();
    let spec_starts = Arc::new(AtomicUsize::new(0));
    let ready_started = Arc::new(AtomicBool::new(false));
    let provider = support::FakeProvider::start({
        let (gate, spec_starts, ready_started) =
            (gate.clone(), spec_starts.clone(), ready_started.clone());
        move |request| {
            if request.body.to_string().contains("Speculative") {
                if spec_starts.fetch_add(1, Ordering::SeqCst) == 0 {
                    gate.wait();
                }
            } else {
                assert_eq!(
                    spec_starts.load(Ordering::SeqCst),
                    1,
                    "ready review precedes speculative redispatch"
                );
                ready_started.store(true, Ordering::SeqCst);
            }
            support::openai::completed(
                request,
                vec![support::openai::message(
                    &json!({"verdict":"GREEN"}).to_string(),
                )],
                support::openai::usage(1, 1),
            )
        }
    });
    let run = fixture.submit(
        &["--speculate", "--jobs", "2", "--max-executions", "3"],
        Some(&provider),
    );
    let active = fixture
        .until(&run, |view| {
            spec_starts.load(Ordering::SeqCst) == 1
                && status(view, "up/sign") == RequestStatus::WaitingHuman
                && status(view, "trigger/sign") == RequestStatus::WaitingHuman
        })
        .await;
    assert!(!ready_started.load(Ordering::SeqCst));
    fixture.sign(&active, "trigger/sign", "GREEN").await;
    fixture
        .until(&run, |_| ready_started.load(Ordering::SeqCst))
        .await;
    gate.open();
    let reviewed = fixture
        .until(&run, |view| {
            status(view, "b-ready/check") == RequestStatus::Green
                && status(view, "a-spec/check") == RequestStatus::Green
        })
        .await;
    assert_eq!(spec_starts.load(Ordering::SeqCst), 2);
    fixture.sign(&reviewed, "up/sign", "GREEN").await;
    let ended = fixture.settled(&run).await;
    assert_eq!(ended.run.status(), RunStatus::Green);
    assert_eq!(
        ended.run.executions_started, 3,
        "yielded executor starts still consume the cap"
    );
}
