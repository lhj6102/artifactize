//! C1–C4: observable cancellation, deferred sign-offs, the idle grace that holds a Run open for
//! them, and scheduling seams, including the hidden eval link from a parent Artifact to its
//! children.
#[path = "support/run.rs"]
mod owned_run;

use artifactize_core::{
    human,
    store::{self, OwnedReceipts as Receipts, RunView},
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
    _root: owned_run::Root,
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
        let state = root.path().join("state");
        let root = owned_run::Root::from(root);
        root.track(&state, &repo);
        let fixture = Self {
            repo,
            state,
            script,
            _root: root,
        };
        // A Run ends as soon as only unclaimed sign-offs are left, unless a test holds it open.
        fixture.grace(0);
        fixture
    }
    /// Set the state's sign-off idle grace through the CLI, for the Runs started afterwards.
    fn grace(&self, seconds: u64) {
        let output = support::os::run(self.command().args([
            "limits",
            "--signoff-idle-grace",
            &seconds.to_string(),
        ]));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn runtime(&self, id: &str, deps: &[&str]) -> Value {
        json!({"id":id,"title":id,"profile":{"kind":"runtime","command":support::os::python_program(),"args":[support::os::path_text(&self.script),id]},"payload":{"instruction":format!("Check {}.", deps.iter().map(|name| format!("{{{name}}}")).collect::<Vec<_>>().join(" "))}})
    }
    fn declare(&self, name: &str, evals: Vec<Value>) {
        self.declare_at(name, name, evals);
    }
    fn declare_at(&self, path: &str, name: &str, evals: Vec<Value>) {
        let folder = self.repo.join(path);
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
        owned_run::subscribed(&self.state, async |changes| {
            let result =
                tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
                    loop {
                        let view = store::read_run(&self.state, id).await.unwrap();
                        if predicate(&view) {
                            return view;
                        }
                        assert!(
                            view.run.status().is_open(),
                            "{}",
                            serde_json::to_string_pretty(&view).unwrap()
                        );
                        changes.next_reconciled().await;
                    }
                })
                .await;
            if result.is_err() {
                let view = store::read_run(&self.state, id).await.unwrap();
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
        })
        .await
    }
    async fn settled(&self, id: &RunId) -> RunView {
        owned_run::terminal(&self.state, id).await
    }

    fn release(&self, artifact: &str, eval: &str, code: u8) {
        use std::io::Write;
        let folder = self.repo.join(artifact);
        let mut marker = tempfile::NamedTempFile::new_in(&folder).unwrap();
        write!(marker, "{code}").unwrap();
        // Publish complete control bytes at once; the child must never parse a partial write.
        marker
            .persist(folder.join(format!("{eval}-release")))
            .unwrap();
    }
    fn ready(&self, artifact: &str, eval: &str) -> bool {
        fs::read_to_string(self.repo.join(artifact).join(format!("{eval}-ready")))
            .is_ok_and(|contents| contents == "ready")
    }
    async fn claim(&self, view: &RunView, eval: &str) {
        let request = view
            .requests
            .iter()
            .find(|request| request.eval_id.as_str() == eval)
            .unwrap();
        let receipts = Receipts::open(&self.state, &self.repo).await.unwrap();
        human::claim(&receipts, &request.id, &"reviewer".parse().unwrap())
            .await
            .unwrap();
    }
    async fn unclaim(&self, view: &RunView, eval: &str) {
        let request = view
            .requests
            .iter()
            .find(|request| request.eval_id.as_str() == eval)
            .unwrap();
        let receipts = Receipts::open(&self.state, &self.repo).await.unwrap();
        human::unclaim(&receipts, &request.id, &"reviewer".parse().unwrap())
            .await
            .unwrap();
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
fn human_eval(deps: &[&str]) -> Value {
    json!({"id":"sign","title":"Sign off","profile":{"kind":"human"},"payload":{"instruction":format!("Review {}.", deps.iter().map(|name| format!("{{{name}}}")).collect::<Vec<_>>().join(" "))}})
}

#[tokio::test]
async fn c1_speculation_starts_machine_work_never_signoffs_cancels_transitive_red_and_keeps_green()
{
    for verdict in ["RED", "GREEN"] {
        let fixture = Fixture::new();
        // The Run stays open for its sign-offs until they are submitted.
        fixture.grace(24 * 60 * 60);
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
            json!({"id":"agent","title":"Agent","profile":{"kind":"agent","backend":"openai","model":"fake-exact-model"},"payload":{"instruction":"Review {up}."}}), human_eval(&["up"])]);
        fixture.declare("leaf", vec![fixture.runtime("check", &["down"])]);
        // Even a sign-off without machine siblings is never asked speculatively.
        fixture.declare("notes", vec![human_eval(&["up"])]);
        let run = fixture.submit(&["--speculate", "--jobs", "5"], Some(&provider));
        let active = fixture
            .until(&run, |view| {
                fixture.ready("up", "check")
                    && fixture.ready("down", "runtime")
                    && fixture.ready("leaf", "check")
                    && started.load(Ordering::SeqCst)
                    && status(view, "up/check") == RequestStatus::Running
                    && status(view, "down/runtime") == RequestStatus::Running
                    && status(view, "leaf/check") == RequestStatus::Running
                    && view.requests.iter().any(|request| {
                        request.eval_id.as_str() == "down/runtime" && request.child.is_some()
                    })
            })
            .await;
        assert_eq!(
            status(&active, "up/check"),
            RequestStatus::Running,
            "downstream started before upstream ended"
        );
        for eval in ["down/sign", "notes/sign"] {
            assert!(
                !status(&active, eval).is_human_wait(),
                "no person is asked before the evals {eval} relies on are GREEN"
            );
        }
        assert_eq!(active.run.status(), RunStatus::Running);
        let runtime_child = active
            .requests
            .iter()
            .find(|request| request.eval_id.as_str() == "down/runtime")
            .unwrap()
            .child
            .clone()
            .unwrap();
        if verdict == "GREEN" {
            fixture.release("down", "runtime", 0);
            fixture.release("leaf", "check", 0);
            gate.open();
            let speculated = fixture
                .until(&run, |view| {
                    status(view, "down/agent") == RequestStatus::Green
                        && status(view, "down/runtime") == RequestStatus::Green
                        && status(view, "leaf/check") == RequestStatus::Green
                })
                .await;
            // Speculative GREENs still wait for up, so no person is asked yet.
            assert!(!status(&speculated, "down/sign").is_human_wait());
            fixture.release("up", "check", 0);
            let signoff = fixture
                .until(&run, |view| {
                    status(view, "down/sign") == RequestStatus::Deferred
                        && status(view, "notes/sign") == RequestStatus::Deferred
                })
                .await;
            assert_eq!(
                signoff.run.status(),
                RunStatus::WaitingHuman,
                "a Run that asked for a sign-off waits for it"
            );
            fixture.sign(&signoff, "notes/sign", "GREEN").await;
            fixture.sign(&signoff, "down/sign", "GREEN").await;
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
                6
            );
        } else {
            fixture.release("up", "check", 1);
            let ended = fixture.settled(&run).await;
            gate.open();
            assert_eq!(ended.run.status(), RunStatus::Red);
            for eval in [
                "down/runtime",
                "down/agent",
                "down/sign",
                "leaf/check",
                "notes/sign",
            ] {
                assert_eq!(status(&ended, eval), RequestStatus::Blocked);
            }
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
async fn c2_signoffs_are_late_provisional_end_yellow_and_are_reused_by_the_next_verify() {
    for verdict in ["GREEN", "RED"] {
        let fixture = Fixture::new();
        fixture.declare("up", vec![fixture.runtime("check", &[]), human_eval(&[])]);
        fixture.declare("down", vec![fixture.runtime("check", &["up"])]);
        let run = fixture.submit(&["--request-key", "mode-choice"], None);
        let active = fixture
            .until(&run, |view| {
                fixture.ready("up", "check") && status(view, "up/check") == RequestStatus::Running
            })
            .await;
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
                status(view, "up/sign") == RequestStatus::Deferred
                    && fixture.ready("down", "check")
                    && status(view, "down/check") == RequestStatus::Running
            })
            .await;
        assert_eq!(
            status(&awaiting, "down/check"),
            RequestStatus::Running,
            "dependents run before sign-off"
        );
        fixture.release("down", "check", 0);
        let ended = fixture.settled(&run).await;
        assert_eq!(ended.run.status(), RunStatus::Yellow);
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
        let status_output = support::os::run(fixture.command().args(["status", "--all", "--json"]));
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
            !store::read_run(&fixture.state, &retry)
                .await
                .unwrap()
                .run
                .modes
                .speculate
        );
        let output = support::os::run(fixture.command().args([
            "verify",
            "--all",
            "--wait",
            "--request-key",
            "mode-choice",
        ]));
        assert_eq!(output.status.code(), Some(5));
        let ci = fixture.submit(&["--reuse-only", "agent,human"], None);
        let ci = fixture.settled(&ci).await;
        assert_eq!(
            ci.run.status(),
            RunStatus::Incomplete,
            "CI never counts deferred evidence as approval"
        );
        fixture.sign(&ended, "up/sign", verdict).await;
        let next = fixture.submit(&[], None);
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
            // The cached result is still taken behind the RED sign-off; the Run is not.
            assert_eq!(next.run.status(), RunStatus::Red);
            assert_eq!(status(&next, "down/check"), RequestStatus::Green);
        }
    }
}

#[tokio::test]
async fn c2_removed_gate_and_defer_human_are_refused_with_their_replacement() {
    let fixture = Fixture::new();
    let mut eval = human_eval(&[]);
    eval["gate"] = json!("blocking");
    fixture.declare("up", vec![eval]);
    let output = support::os::run(fixture.command().args(["verify", "--all"]));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "gate was removed in 0.16.0; remove it. A Human sign-off never holds up other evals."
        ),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.declare("up", vec![human_eval(&[])]);
    let output = support::os::run(fixture.command().args(["verify", "--all", "--defer-human"]));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--defer-human was removed in 0.16.0"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn c2_a_claim_holds_the_run_open_until_it_is_submitted_or_released() {
    for submit in [true, false] {
        let fixture = Fixture::new();
        fixture.declare("notes", vec![human_eval(&[])]);
        fixture.declare("slow", vec![fixture.runtime("check", &[])]);
        let run = fixture.submit(&[], None);
        let asked = fixture
            .until(&run, |view| {
                status(view, "notes/sign") == RequestStatus::Deferred
                    && fixture.ready("slow", "check")
            })
            .await;
        assert_eq!(
            asked.run.status(),
            RunStatus::WaitingHuman,
            "the Run waits from its first sign-off request"
        );
        fixture.claim(&asked, "notes/sign").await;
        fixture.release("slow", "check", 0);
        // Even with no idle grace, the claim keeps the Run open after its other work ended.
        let held = fixture
            .until(&run, |view| {
                status(view, "slow/check") == RequestStatus::Green
            })
            .await;
        assert_eq!(held.run.status(), RunStatus::WaitingHuman);
        let ended = if submit {
            fixture.sign(&held, "notes/sign", "GREEN").await;
            let ended = fixture.settled(&run).await;
            assert_eq!(
                ended.run.status(),
                RunStatus::Green,
                "the Run settles with its sign-off"
            );
            ended
        } else {
            fixture.unclaim(&held, "notes/sign").await;
            let ended = fixture.settled(&run).await;
            assert_eq!(
                ended.run.status(),
                RunStatus::Yellow,
                "a released claim leaves only an unclaimed sign-off"
            );
            assert_eq!(status(&ended, "notes/sign"), RequestStatus::Deferred);
            ended
        };
        let output = support::os::run(fixture.command().args([
            "run",
            "show",
            ended.run.id.as_str(),
            "--wait",
        ]));
        assert_eq!(output.status.code(), Some(if submit { 0 } else { 5 }));
    }
}

#[tokio::test]
async fn c2_the_idle_grace_holds_a_run_with_only_signoffs_left_open() {
    let fixture = Fixture::new();
    fixture.grace(24 * 60 * 60);
    fixture.declare("notes", vec![human_eval(&[])]);
    // Nothing but the sign-off: the Run is idle from its start, and nobody has claimed it.
    let run = fixture.submit(&[], None);
    let waiting = fixture
        .until(&run, |view| {
            status(view, "notes/sign") == RequestStatus::Deferred
                && view.run.control.as_ref().is_some_and(|control| {
                    control.activity == artifactize_core::runs::Activity::Waiting
                })
        })
        .await;
    assert_eq!(waiting.run.status(), RunStatus::WaitingHuman);
    let observed =
        support::os::run(
            fixture
                .command()
                .args(["run", "show", run.as_str(), "--json"]),
        );
    assert_eq!(
        serde_json::from_slice::<Value>(&observed.stdout).unwrap()["status"],
        "WAITING_HUMAN"
    );
    fixture.sign(&waiting, "notes/sign", "RED").await;
    assert_eq!(fixture.settled(&run).await.run.status(), RunStatus::Red);

    let output = support::os::run(fixture.command().args(["limits", "--json"]));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["signoffIdleGraceSeconds"],
        86400
    );
    let output =
        support::os::run(
            fixture
                .command()
                .args(["limits", "--signoff-idle-grace", "86401"]),
        );
    assert!(!output.status.success(), "a grace past a day is refused");
}

#[tokio::test]
async fn c2_a_parent_without_evals_settles_yellow_while_its_constituent_awaits_a_signoff() {
    let fixture = Fixture::new();
    fixture.declare("notes", Vec::new());
    fixture.declare_at("notes/draft", "notes::draft", vec![human_eval(&[])]);
    let run = fixture.submit(&[], None);
    assert_eq!(fixture.settled(&run).await.run.status(), RunStatus::Yellow);
}

#[tokio::test]
async fn c2_a_run_that_an_earlier_version_ended_signoffs_pending_reads_as_yellow() {
    let fixture = Fixture::new();
    fixture.declare("notes", vec![human_eval(&[])]);
    let run = fixture.submit(&[], None);
    assert_eq!(fixture.settled(&run).await.run.status(), RunStatus::Yellow);
    // Back to schema 9, which called the same end SIGNOFFS_PENDING.
    let db = rusqlite::Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    db.execute(
        "UPDATE runs SET status='SIGNOFFS_PENDING',data=json_set(data,'$.status','SIGNOFFS_PENDING') WHERE id=?",
        [run.as_str()],
    )
    .unwrap();
    db.execute_batch("PRAGMA user_version=9;").unwrap();
    drop(db);
    let upgraded = store::read_run(&fixture.state, &run).await.unwrap();
    assert_eq!(upgraded.run.status(), RunStatus::Yellow);
    let db = rusqlite::Connection::open_with_flags(
        fixture.state.join(store::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT status FROM runs WHERE id=?",
            [run.as_str()],
            |row| { row.get::<_, String>(0) }
        )
        .unwrap(),
        "YELLOW"
    );
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        store::STATE_SCHEMA_VERSION
    );
}

#[tokio::test]
async fn c2_cancelling_a_run_ends_its_deferred_signoff_and_the_requests_joined_to_it() {
    let fixture = Fixture::new();
    fixture.declare("up", vec![human_eval(&[])]);
    fixture.declare("slow", vec![fixture.runtime("check", &[])]);
    let run = fixture.submit(&[], None);
    fixture
        .until(&run, |view| {
            status(view, "up/sign") == RequestStatus::Deferred && fixture.ready("slow", "check")
        })
        .await;
    // Another Run joins the same sign-off while the first one still runs.
    let output = support::os::run(
        fixture
            .command()
            .args(["verify", "--eval", "up/sign", "--json"]),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let follower: RunId = serde_json::from_slice::<Value>(&output.stdout).unwrap()["run"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let follower = fixture.settled(&follower).await;
    assert_eq!(status(&follower, "up/sign"), RequestStatus::Deferred);

    let output = support::os::run(fixture.command().args(["run", "cancel", run.as_str()]));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cancelled = fixture.settled(&run).await;
    assert_eq!(cancelled.run.status(), RunStatus::Error);
    assert_eq!(status(&cancelled, "up/sign"), RequestStatus::Error);
    let follower = store::read_run(&fixture.state, &follower.run.id)
        .await
        .unwrap();
    assert_eq!(
        status(&follower, "up/sign"),
        RequestStatus::Error,
        "a request joined to the cancelled sign-off ends with it"
    );
    assert!(
        store::read_waiting(&fixture.state, None)
            .await
            .unwrap()
            .is_empty(),
        "no sign-off is offered that can no longer be claimed"
    );
}

#[tokio::test]
async fn c2_new_human_issuance_supersedes_changed_deferred_signoffs_and_followers() {
    for claimed in [false, true] {
        let fixture = Fixture::new();
        fixture.declare("up", vec![fixture.runtime("check", &[]), human_eval(&[])]);
        fixture.release("up", "check", 0);
        let first = fixture.submit(&[], None);
        let first = fixture.settled(&first).await;
        assert_eq!(first.run.status(), RunStatus::Yellow);
        let original = first
            .requests
            .iter()
            .find(|q| q.eval_id.as_str() == "up/sign")
            .unwrap();
        let joined = fixture.submit(&[], None);
        let joined = fixture.settled(&joined).await;
        let follower = joined
            .requests
            .iter()
            .find(|q| q.eval_id.as_str() == "up/sign")
            .unwrap();
        assert_eq!(follower.status(), RequestStatus::Deferred);
        assert_eq!(
            original.execution_id, follower.execution_id,
            "same-key requests still join"
        );
        let receipts = Receipts::open(&fixture.state, &fixture.repo).await.unwrap();
        let reviewer = "reviewer".parse().unwrap();
        if claimed {
            human::claim(&receipts, &original.id, &reviewer)
                .await
                .unwrap();
        }

        fs::write(fixture.repo.join("up/fingerprint"), "changed").unwrap();
        fs::remove_file(fixture.repo.join("up/check-ready")).unwrap();
        fs::remove_file(fixture.repo.join("up/check-release")).unwrap();
        let newer = fixture.submit(&[], None);
        fixture
            .until(&newer, |view| {
                fixture.ready("up", "check") && status(view, "up/check") == RequestStatus::Running
            })
            .await;
        let waiting = store::read_waiting(&fixture.state, None).await.unwrap();
        assert!(
            waiting.iter().any(|q| q.request.id == original.id),
            "acceptance alone does not retire a sign-off"
        );
        assert!(waiting.iter().any(|q| q.request.id == follower.id));
        fixture.release("up", "check", 0);
        let issued = fixture
            .until(&newer, |view| {
                status(view, "up/sign") == RequestStatus::Deferred
            })
            .await;
        for (run, request) in [(&first.run.id, original), (&joined.run.id, follower)] {
            let ended = store::read_run(&fixture.state, run).await.unwrap();
            let signoff = ended.requests.iter().find(|q| q.id == request.id).unwrap();
            assert_eq!(signoff.status(), RequestStatus::Error);
            assert_eq!(
                signoff.error_code(),
                Some(artifactize_core::types::FailureCode::Superseded)
            );
            assert!(
                human::claim(&receipts, &request.id, &reviewer)
                    .await
                    .is_err()
            );
        }
        let waiting = store::read_waiting(&fixture.state, None).await.unwrap();
        assert!(
            waiting
                .iter()
                .all(|q| q.request.id != original.id && q.request.id != follower.id)
        );
        assert_eq!(waiting.len(), 1);
        fixture.sign(&issued, "up/sign", "GREEN").await;
        fixture.settled(&newer).await;
    }
}

#[tokio::test]
async fn c2_superseding_a_signoff_leaves_other_worktrees() {
    let fixture = Fixture::new();
    fixture.declare("up", vec![human_eval(&[])]);
    fs::write(fixture.repo.join("up/fingerprint"), "deferred").unwrap();
    let deferred = fixture.submit(&[], None);
    let deferred = fixture.settled(&deferred).await;

    let mut other = Fixture::new();
    other.state = fixture.state.clone();
    other._root.track(&other.state, &other.repo);
    other.declare("up", vec![human_eval(&[])]);
    fs::write(other.repo.join("up/fingerprint"), "other-tree").unwrap();
    let other_run = other.submit(&[], None);
    let other_run = other.settled(&other_run).await;

    fs::write(fixture.repo.join("up/fingerprint"), "newest").unwrap();
    let newest = fixture.submit(&[], None);
    fixture.settled(&newest).await;
    assert_eq!(
        status(
            &store::read_run(&fixture.state, &deferred.run.id)
                .await
                .unwrap(),
            "up/sign"
        ),
        RequestStatus::Error
    );
    let receipts = Receipts::open(&fixture.state, &fixture.repo).await.unwrap();
    let request = &other_run.requests[0];
    let waiting = store::read_waiting(&fixture.state, None).await.unwrap();
    assert!(
        waiting.iter().any(|q| q.request.id == request.id),
        "request={request:#?}, waiting={waiting:#?}"
    );
    human::claim(&receipts, &request.id, &"reviewer".parse().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn c2_joining_a_new_key_also_supersedes_earlier_deferred_signoffs() {
    let fixture = Fixture::new();
    fixture.declare("up", vec![human_eval(&[])]);
    let first = fixture.submit(&[], None);
    let first = fixture.settled(&first).await;

    let mut other = Fixture::new();
    other.state = fixture.state.clone();
    other._root.track(&other.state, &other.repo);
    other.declare("up", vec![human_eval(&[])]);
    fs::write(other.repo.join("up/fingerprint"), "new-key").unwrap();
    let active = other.submit(&[], None);
    let active = other.settled(&active).await;
    assert_ne!(
        first.requests[0].key, active.requests[0].key,
        "first={first:#?}, active={active:#?}"
    );
    fs::write(fixture.repo.join("up/fingerprint"), "new-key").unwrap();
    let joined = fixture.submit(&[], None);
    let joined = fixture.settled(&joined).await;
    assert_eq!(
        joined.requests[0].execution_id,
        active.requests[0].execution_id
    );
    assert_eq!(
        status(
            &store::read_run(&fixture.state, &first.run.id)
                .await
                .unwrap(),
            "up/sign"
        ),
        RequestStatus::Error
    );
    assert_eq!(
        status(
            &store::read_run(&fixture.state, &active.run.id)
                .await
                .unwrap(),
            "up/sign"
        ),
        RequestStatus::Deferred
    );
    fixture.sign(&joined, "up/sign", "GREEN").await;
}

#[tokio::test]
async fn c2_retiring_a_deferred_follower_preserves_its_original_in_another_tree() {
    let original = Fixture::new();
    original.declare("up", vec![human_eval(&[])]);
    let run = original.submit(&[], None);
    let issued = original
        .until(&run, |view| status(view, "up/sign").is_human_wait())
        .await;
    let mut follower = Fixture::new();
    follower.state = original.state.clone();
    follower._root.track(&follower.state, &follower.repo);
    follower.declare("up", vec![human_eval(&[])]);
    let joined = follower.submit(&[], None);
    let joined = follower.settled(&joined).await;
    assert_eq!(
        joined.requests[0].execution_id,
        issued.requests[0].execution_id
    );

    fs::write(follower.repo.join("up/fingerprint"), "changed").unwrap();
    let newer = follower.submit(&[], None);
    follower.settled(&newer).await;
    let retired = store::read_request(&follower.state, joined.requests[0].id.as_str())
        .await
        .unwrap();
    assert_eq!(
        retired.request.error_code(),
        Some(artifactize_core::types::FailureCode::Superseded)
    );
    let receipts = Receipts::open(&follower.state, &follower.repo)
        .await
        .unwrap();
    assert!(
        human::claim(&receipts, &retired.request.id, &"reviewer".parse().unwrap())
            .await
            .is_err()
    );
    let waiting = store::read_waiting(&original.state, None).await.unwrap();
    assert!(
        waiting
            .iter()
            .any(|view| view.request.id == issued.requests[0].id)
    );
    assert!(
        waiting
            .iter()
            .all(|view| view.request.id != retired.request.id)
    );
    original.sign(&issued, "up/sign", "GREEN").await;
    original.settled(&run).await;
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
        .until(&run, |view| {
            fixture.ready("z-root", "check")
                && status(view, "z-root/check") == RequestStatus::Running
        })
        .await;
    assert!(!fixture.ready("a-short", "check"));
    assert!(!fixture.ready("c-middle", "check"));
    fixture.release("z-root", "check", 0);
    fixture
        .until(&run, |view| {
            fixture.ready("c-middle", "check")
                && status(view, "c-middle/check") == RequestStatus::Running
        })
        .await;
    assert!(!fixture.ready("a-short", "check"));
    assert!(!fixture.ready("b-tail", "check"));
    fixture.release("c-middle", "check", 0);
    fixture
        .until(&run, |view| {
            fixture.ready("a-short", "check")
                && status(view, "a-short/check") == RequestStatus::Running
        })
        .await;
    assert!(!fixture.ready("b-tail", "check"));
    fixture.release("a-short", "check", 0);
    fixture
        .until(&run, |view| {
            fixture.ready("b-tail", "check")
                && status(view, "b-tail/check") == RequestStatus::Running
        })
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
        r#"{"backends":{"openai":1},"signoffIdleGraceSeconds":0}"#,
    )
    .unwrap();
    fixture.declare("up", vec![fixture.runtime("check", &[])]);
    fixture.declare("trigger", vec![fixture.runtime("check", &[])]);
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
        &["--speculate", "--jobs", "3", "--max-executions", "5"],
        Some(&provider),
    );
    fixture
        .until(&run, |view| {
            spec_starts.load(Ordering::SeqCst) == 1
                && fixture.ready("up", "check")
                && fixture.ready("trigger", "check")
                && status(view, "up/check") == RequestStatus::Running
                && status(view, "trigger/check") == RequestStatus::Running
        })
        .await;
    assert!(!ready_started.load(Ordering::SeqCst));
    fixture.release("trigger", "check", 0);
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
    assert_eq!(status(&reviewed, "up/check"), RequestStatus::Running);
    fixture.release("up", "check", 0);
    let ended = fixture.settled(&run).await;
    assert_eq!(ended.run.status(), RunStatus::Green);
    assert_eq!(
        ended.run.executions_started, 5,
        "yielded executor starts still consume the cap"
    );
}

#[tokio::test]
async fn c4_a_parent_runs_beside_its_constituent_and_fails_with_it() {
    for modes in [&["--jobs", "2"][..], &["--speculate", "--jobs", "2"]] {
        let fixture = Fixture::new();
        fixture.declare("app", vec![fixture.runtime("check", &[])]);
        fixture.declare_at("app/lib", "app::lib", vec![fixture.runtime("check", &[])]);
        let run = fixture.submit(modes, None);
        // A constituent is no input: both evals run at once.
        fixture
            .until(&run, |_| {
                fixture.ready("app/lib", "check") && fixture.ready("app", "check")
            })
            .await;
        fixture.release("app/lib", "check", 1);
        fixture.release("app", "check", 0);
        let ended = fixture.settled(&run).await;
        assert_eq!(ended.run.status(), RunStatus::Red, "{modes:?}");
        assert_eq!(
            status(&ended, "app/check"),
            RequestStatus::Green,
            "{modes:?}"
        );
        assert_eq!(
            status(&ended, "app::lib/check"),
            RequestStatus::Red,
            "{modes:?}"
        );
    }
}
