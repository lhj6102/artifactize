//! The command boundary: JSON in and out of the IPC handler, on state a real `verify` made.
//! A Human sign-off with a built-in tool and owner schemas waits; a runtime Eval
//! references it. Built-in tools and runtime Evals run alike on every OS.
use artifactize_core::project::{VerifyOptions, selection::Selection};
use artifactize_desktop::{App, Launch, Mode};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tauri::{
    Manager, WebviewWindow,
    ipc::{CallbackFn, InvokeBody},
    test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets},
    webview::InvokeRequest,
};

/// The CLI tests' owned provider, Run and operating-system fixtures.
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::{FakeProvider, Reply, os, runs as run_fixture};

const RELEASE: &str = r#"name = "release"
fingerprint = { files = ["."] }

[views.human_tools]
notes = { description = "Print the release notes.", builtin = "read", args = ["{artifactPath}/notes.md"] }

[evals.signoff]
title = "A person approves the release"
profile = { kind = "human" }
payload = { instruction = "Read the release notes and approve them." }
"#;

const SHIP: &str = r#"name = "ship"

[evals.check]
title = "The signed release passes the shipping check"
profile = { kind = "runtime", command = "fixture-runtime", args = [] }
payload.instruction = "Inspect {release} before shipping."
"#;

struct Fixture {
    _root: run_fixture::RepoLease,
    repo: std::path::PathBuf,
    state: std::path::PathBuf,
}

/// A repository whose Run waits for the Human sign-off, made by the library's `verify`.
fn fixture() -> Fixture {
    fixture_with(&[])
}

/// The fixture with `extra` Artifacts beside release and ship, as `(folder, index.artf)`.
fn fixture_with(extra: &[(&str, &str)]) -> Fixture {
    // Below the OS temp root (TMPDIR), as the CLI tests do: token storage refuses Git trees.
    let root = os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    for (name, text) in [("release", RELEASE), ("ship", SHIP)]
        .into_iter()
        .chain(extra.iter().copied())
    {
        fs::create_dir_all(repo.join(name)).unwrap();
        let text = text.replace(
            "\"fixture-runtime\"",
            &serde_json::to_string(&os::true_program()).unwrap(),
        );
        support::declaration::write(&repo, repo.join(name).join("index.artf"), text).unwrap();
    }
    fs::write(
        support::declaration::source_path(repo.join("release/notes.md")),
        "# Release notes\n- Ready.\n",
    )
    .unwrap();
    // Its own repository, not a worktree of the one the tests build in.
    os::git(&repo, &["init", "-q"]);
    let fixture = Fixture {
        _root: run_fixture::RepoLease::new(root, &state, &repo),
        repo,
        state,
    };
    verify(&fixture.repo, &fixture.state);
    fixture
}

/// A Run of `repo` in `state` whose Human request is left waiting.
fn verify(repo: &Path, state: &Path) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let options = VerifyOptions {
            ..VerifyOptions::default()
        };
        // Observe the saved waiting request; no verification timeout settles it.
        run_fixture::record(
            repo,
            Some(state),
            &Selection::All,
            &options,
            Default::default(),
        )
        .await
        .unwrap();
    });
}

/// Keep the mock app alive without leaking it, its transcript readers or its Current scans.
struct Window {
    window: WebviewWindow<MockRuntime>,
    app: tauri::App<MockRuntime>,
}

impl std::ops::Deref for Window {
    type Target = WebviewWindow<MockRuntime>;

    fn deref(&self) -> &Self::Target {
        &self.window
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // Stop the Current scans and destroy the window before the app and the fixture's
        // folders go, including when an assertion panics or a scan outlives its wait.
        let stopped = self.app.state::<App>().close();
        let _ = self.window.destroy();
        tauri::async_runtime::block_on(stopped);
    }
}

fn window(fixture: &Fixture, mode: Mode) -> Window {
    launched_window(fixture, mode, Some(fixture.repo.clone()))
}

fn launched_window(fixture: &Fixture, mode: Mode, repo: Option<std::path::PathBuf>) -> Window {
    let launch = Launch {
        mode,
        state: fixture.state.clone(),
        repo,
        reviewer: "alice".parse().unwrap(),
        request: None,
    };
    let app = artifactize_desktop::app(mock_builder(), launch)
        // This command-only fixture does not need the live event subscription. Do not start
        // an app-lifetime background task on Tauri's process-global test runtime.
        .setup(|_| Ok(()))
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    Window { window, app }
}

/// Invoke `command` with `args` through the IPC handler; an error is the serialized failure.
fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, args: Value) -> Result<Value, Value> {
    tauri::test::get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
    )
    .map(|body| body.deserialize::<Value>().unwrap())
}

fn invoke_viewer(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
    args: Value,
) -> Result<Value, Value> {
    let label = format!("viewer-{}", args["session"].as_u64().unwrap());
    let handle = window.app_handle();
    let caller = handle.get_webview_window(&label).unwrap_or_else(|| {
        tauri::WebviewWindowBuilder::new(handle, label, Default::default())
            .build()
            .unwrap()
    });
    invoke(&caller, command, args)
}

const STILL: fn() -> Value = || json!({"run": {"kind": "keep"}, "query": null, "older": null, "full": true, "resync": false});

/// Show Run `run` in the Main area.
fn open_run(run: &str) -> Value {
    json!({"run": {"kind": "open", "id": run}, "query": null, "older": null, "full": false, "resync": false})
}

/// Star `branch`, or take its star away.
fn star(branch: &Value, on: bool) -> Value {
    json!({"run": {"kind": "keep"}, "query": null, "older": null, "star": {"branch": branch, "on": on}, "full": false, "resync": false})
}

/// Read the state afresh with the pane's search `query`.
fn search(query: &str) -> Value {
    json!({"run": {"kind": "keep"}, "query": query, "older": null, "full": true, "resync": false})
}

/// The waiting Human reviews the Pane lists, read afresh.
fn waiting(window: &WebviewWindow<MockRuntime>) -> Value {
    invoke(window, "browse", json!({"navigate": STILL()})).unwrap()["waiting"].clone()
}

/// The fixture's one branch as the Pane lists it.
fn branch(window: &WebviewWindow<MockRuntime>) -> Value {
    let view = invoke(window, "browse", json!({"navigate": STILL()})).unwrap();
    let [branch] = view["branches"].as_array().unwrap().as_slice() else {
        panic!("one branch: {view}");
    };
    branch["key"].clone()
}

fn saved(fixture: &Fixture, eval: &str) -> artifactize_core::store::RequestView {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let runs = artifactize_core::store::read_runs(&fixture.state, None, 1, 0)
            .await
            .unwrap();
        artifactize_core::store::read_run_requests(&fixture.state, &runs[0].id)
            .await
            .unwrap()
            .1
            .into_iter()
            .find(|view| view.request.eval_id.as_str() == eval)
            .unwrap()
    })
}

/// The shown Run's tree row `node` with its children, as the Detail reads it.
fn row(window: &WebviewWindow<MockRuntime>, run: &str, node: &str) -> Value {
    invoke(
        window,
        "detail_row",
        json!({"source": {"kind": "run", "run": run}, "node": node}),
    )
    .unwrap()
}

/// The names of the shown Run's tree: its Run row, then each Artifact in the graph's order with
/// its evals.
fn tree_names(window: &WebviewWindow<MockRuntime>, view: &Value) -> Vec<String> {
    let run = view["run"]["id"].as_str().unwrap();
    let graph = invoke(
        window,
        "graph",
        json!({"source": {"kind": "run", "run": run}, "known": null}),
    )
    .unwrap();
    let artifacts = graph["update"]["nodes"].as_array().unwrap().clone();
    std::iter::once(row(window, run, view["run"]["node"].as_str().unwrap()))
        .chain(
            artifacts
                .iter()
                .map(|node| row(window, run, node["id"].as_str().unwrap())),
        )
        .flat_map(|node| {
            let children: Vec<_> = node["children"]
                .as_array()
                .unwrap()
                .iter()
                .map(|child| child["name"].as_str().unwrap().to_owned())
                .collect();
            std::iter::once(node["name"].as_str().unwrap().to_owned()).chain(children)
        })
        .collect()
}

/// The command tests run in a child without the remote store variables, so a developer's
/// `ARTIFACTIZE_REMOTE` never adds a network input to their `verify` and submissions; a
/// remote comes only from the state a test writes.
#[test]
fn command_tests_run_without_an_inherited_remote_store() {
    // The publication test isolates its own remote and stays out of this child.
    let mut command = os::ignored_tests(&[]);
    for variable in [
        "ARTIFACTIZE_REMOTE",
        "ARTIFACTIZE_REMOTE_TOKEN",
        "ARTIFACTIZE_REMOTE_SHARE",
    ] {
        command.env_remove(variable);
    }
    // Many tests, so no probe deadline: the output drains while they run.
    let output = os::run(&mut command);
    assert!(
        output.status.success(),
        "command tests failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn browse_lists_the_branch_with_its_runs_and_detail_reads_the_shown_run() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Monitor);
    let view = invoke(&window, "browse", json!({"navigate": STILL()})).unwrap();

    // One live branch: the project's checked-out branch, the one --repo names.
    let identity = artifactize_core::repository::identify(&fixture.repo);
    let [branch] = view["branches"].as_array().unwrap().as_slice() else {
        panic!("one branch: {view}");
    };
    assert_eq!(branch["project"], "repo");
    assert_eq!(branch["name"], identity.branch.unwrap());
    assert_eq!(view["launch"], branch["key"]);
    // Its timeline holds the Run in progress, the Human waiting: no Current yet.
    let [chip] = branch["chips"].as_array().unwrap().as_slice() else {
        panic!("one chip: {branch}");
    };
    assert_eq!(chip["kind"], "run");
    assert_eq!(chip["running"], json!(true));
    assert_eq!(chip["status"], "Waiting for sign-off");
    // A running Run's age is its time so far, not a time ago.
    assert!(!chip["age"].as_str().unwrap().ends_with(" ago"), "{chip}");
    assert_eq!(view["run"], json!({"kind": "none"}), "nothing chosen yet");
    // The waiting stack lists the sign-off with its project and branch.
    assert_eq!(view["waiting"][0]["eval"], "release/signoff");
    assert_eq!(view["waiting"][0]["project"], "repo");
    assert_eq!(view["waiting"][0]["branch"], branch["name"]);

    let run = chip["id"].as_str().unwrap().to_owned();
    let key = branch["key"].clone();
    invoke(
        &window,
        "browse",
        json!({"navigate": search("repo:not-listed")}),
    )
    .unwrap();
    let view = invoke(&window, "browse", json!({"navigate": open_run(&run)})).unwrap();
    assert_eq!(view["branches"], json!([]), "the branch is filtered out");
    assert_eq!(
        view["run"]["branch"], key,
        "the Run still names its live branch"
    );
    assert_eq!(
        tree_names(&window, &view),
        ["Run", "release", "signoff", "ship", "check"]
    );
    let shown = view["run"]["id"].as_str().unwrap();
    // The header row names what needs attention: the sign-off that waits.
    assert_eq!(
        view["run"]["needs"],
        json!([{"mark": {"status": "DEFERRED", "glyph": "?"}, "text": "release/signoff"}])
    );
    // The Run's Detail leads with what the header row leaves out: its status, its tally, its
    // time and its comment, each said once rather than again among its sections.
    let outcome = invoke(
        &window,
        "detail",
        json!({"source": {"kind": "run", "run": run}, "node": view["run"]["node"]}),
    )
    .unwrap();
    assert_eq!(outcome["target"], "run");
    assert_eq!(outcome["mark"], view["run"]["mark"]);
    assert_eq!(outcome["outcome"]["failed"], json!([]));
    assert!(
        outcome["outcome"]["tally"]
            .as_array()
            .is_some_and(|tally| !tally.is_empty()),
        "{outcome}"
    );
    assert!(
        outcome["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|section| section["fields"].as_array().unwrap())
            .all(|field| !["Status", "Counts", "Validation", "Comment"]
                .contains(&field["key"].as_str().unwrap())),
        "{outcome}"
    );
    let signoff = &row(&window, shown, "a:release")["children"][0];
    assert_eq!(signoff["urgent"], json!(true));
    assert_eq!(signoff["text"][0]["text"], "Human sign-off · unclaimed");
    // ship waits for release: its eval marks release as an unmet upstream Artifact.
    let check = &row(&window, shown, "a:ship")["children"][0];
    assert_eq!(
        check["upstream"],
        json!([{"node": "a:release", "complete": false}])
    );

    // The Detail comes from the shown snapshot.
    let detail = invoke(
        &window,
        "detail",
        json!({"source": {"kind": "run", "run": run}, "node": "e:release/signoff"}),
    )
    .unwrap();
    assert_eq!(detail["profile"], "human");
    assert_eq!(detail["sections"][0]["name"], "outcome");
    assert_eq!(
        detail["sections"][0]["fields"][0],
        json!({"key": "Status", "value": "DEFERRED — waiting for a Human sign-off; other evals go on"})
    );
    // A request without an Agent session has no transcript to open, and says why.
    let request = detail["request"].as_str().unwrap();
    let opened = invoke(&window, "transcript_open", json!({"id": request})).unwrap();
    assert_eq!(opened["handle"], Value::Null);
    assert!(
        opened["text"].as_str().is_some_and(|text| !text.is_empty()),
        "{opened}"
    );
    let missing = invoke(
        &window,
        "detail",
        json!({"source": {"kind": "run", "run": "run-gone"}, "node": "e:release/signoff"}),
    );
    assert_eq!(missing.unwrap_err()["kind"], "missing");
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn quit_never_releases_a_replacement_claim_of_the_same_reviewer() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    let offered = invoke(&window, "human_taken", json!({})).unwrap();
    assert_eq!(offered, json!([id]));
    let replacement = tauri::async_runtime::block_on(async {
        let request = id.parse().unwrap();
        let reviewer = "alice".parse().unwrap();
        let (receipts, _) = artifactize_core::human::open(&fixture.state, &request)
            .await
            .unwrap();
        let old = artifactize_core::human::unclaim(&receipts, &request, &reviewer)
            .await
            .unwrap();
        let current = artifactize_core::human::claim(&receipts, &request, &reviewer)
            .await
            .unwrap();
        assert_ne!(old, current);
        receipts.close().await.unwrap();
        current
    });
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([])
    );
    invoke(
        &window,
        "human_release_taken",
        json!({"ids": offered, "job": 2}),
    )
    .unwrap();
    assert_eq!(saved(&fixture, "release/signoff").claim, Some(replacement));
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn quit_cannot_leave_a_claim_acquired_after_the_confirmation_snapshot() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([])
    );
    invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    let refused = invoke(&window, "main_close", json!({})).unwrap_err();
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("Release the review claims")
    );
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([id])
    );
    invoke(
        &window,
        "human_release_taken",
        json!({"ids": [id], "job": 2}),
    )
    .unwrap();
    assert!(saved(&fixture, "release/signoff").claim.is_none());
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn verify_browse_and_review_search_are_independent() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Monitor);
    let shown = |query: &str| {
        let view = invoke(&window, "browse", json!({"navigate": search(query)})).unwrap();
        (
            view["branches"].as_array().unwrap().len(),
            view["waiting"].as_array().unwrap().len(),
            view["query"].clone(),
        )
    };
    let (branches, waiting, query) = shown(artifactize_core::monitor::live::DEFAULT);
    assert_eq!((branches, waiting), (1, 1));
    assert_eq!(
        query,
        json!({"text": "sort:recent", "error": null, "start": "sort:recent"})
    );
    assert_eq!(shown("repo:rep").0, 1);
    let branch = artifactize_core::repository::identify(&fixture.repo)
        .branch
        .unwrap();
    let reviews = |query: &str| {
        invoke(
            &window,
            "human_search",
            json!({"query": query, "start": null}),
        )
        .unwrap()
    };
    assert_eq!(
        reviews(&format!("type:human repo:repo branch:{branch}"))["reviews"][0]["branch"],
        branch
    );
    assert_eq!(
        reviews("type:human branch:no-such-branch")["reviews"],
        json!([])
    );
    let complete = invoke(
        &window,
        "human_search_complete",
        json!({"query": "type:human branch:"}),
    )
    .unwrap();
    assert_eq!(complete["candidates"], json!([format!("branch:{branch}")]));
    // Neither a free-text branch query nor a scoped one changes the waiting review count.
    let (branches, waiting, _) = shown("signoff");
    assert_eq!((branches, waiting), (0, 1));
    let (branches, waiting, _) = shown("repo:elsewhere");
    assert_eq!((branches, waiting), (0, 1));
    // An invalid Verify search lists no branches and says why, but does not filter Review.
    let (branches, waiting, query) = shown("sort:size");
    assert_eq!((branches, waiting), (0, 1));
    assert!(
        query["error"].as_str().unwrap().contains("sort:size"),
        "{query}"
    );
    let completion = invoke(
        &window,
        "pane_complete",
        json!({"query": "sort:recent repo:"}),
    )
    .unwrap();
    assert_eq!(
        completion,
        json!({"keep": "sort:recent ", "candidates": ["repo:repo"]})
    );
    assert_eq!(
        invoke(&window, "human_waiting_count", json!({})).unwrap(),
        1
    );
    let page = invoke(
        &window,
        "human_search",
        json!({"query": "type:human waiting", "start": null}),
    )
    .unwrap();
    let review = &page["reviews"][0];
    assert_eq!(page["reviews"].as_array().unwrap().len(), 1);
    assert_eq!(review["eval"], "release/signoff");
    let id = review["id"].as_str().unwrap();
    invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    assert_eq!(
        invoke(&window, "human_waiting_count", json!({})).unwrap(),
        0
    );
    let exact = invoke(
        &window,
        "human_open_reference",
        json!({"id": id, "query": "type:human waiting"}),
    )
    .unwrap();
    assert_eq!(exact["view"]["id"], id);
    assert_eq!(exact["view"]["target"], review["target"]);
    assert_eq!(exact["outsideFilter"], true);
    assert_eq!(exact["outsideReason"], "Claimed by you.");
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_human_review_claims_runs_refuses_what_readiness_refuses_and_submits_green() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    let listed = waiting(&window);
    assert_eq!(listed[0]["id"], id);
    assert_eq!(listed[0]["claim"], "-");
    assert_eq!(listed.as_array().unwrap().len(), 1);

    let open = invoke(&window, "human_open", json!({"id": id, "reload": true})).unwrap();
    assert_eq!(open["stage"], "claim");
    assert_eq!(open["tools"][0]["name"], "release_notes_release");
    assert_eq!(open["tools"][0]["action"], "prints release/notes.md");
    // Tools need an explicit claim; opening claimed nothing.
    let refused = invoke(
        &window,
        "human_run",
        json!({"id": id, "tool": "release_notes_release", "job": 1}),
    )
    .unwrap_err();
    // The library's own refusal, in its words.
    assert_eq!(refused["kind"], "action");
    assert_eq!(refused["message"], artifactize_core::review::CLAIM_FIRST);
    assert!(saved(&fixture, "release/signoff").claim.is_none());

    let claimed = invoke(&window, "human_claim", json!({"id": id, "job": 2})).unwrap();
    assert_eq!(claimed["view"]["stage"], "review");
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([id])
    );
    let ran = invoke(
        &window,
        "human_run",
        json!({"id": id, "tool": "release_notes_release", "job": 3}),
    )
    .unwrap();
    // What it printed reaches the Tool log as events; the command answers with the notice.
    assert_eq!(ran["text"], "release_notes_release finished.");

    // The library's readiness, in its words: RED needs an issue, GREEN takes none.
    for (verdict, issues, refusal) in [
        ("RED", json!([]), artifactize_core::human::RED_WITHOUT_ISSUE),
        (
            "GREEN",
            json!(["Off by one"]),
            artifactize_core::human::GREEN_WITH_ISSUES,
        ),
    ] {
        let invalid = invoke(
            &window,
            "human_submit",
            json!({"id": id, "job": 4, "verdict": verdict, "issues": issues}),
        )
        .unwrap_err();
        assert_eq!(invalid["kind"], "invalid");
        assert_eq!(invalid["message"], refusal, "{invalid}");
    }
    assert_eq!(
        saved(&fixture, "release/signoff").request.status(),
        artifactize_core::types::RequestStatus::Deferred
    );
    // The per-verdict forms are gone: a Human answer is a verdict and its issues.
    assert!(invoke(&window, "human_form", json!({"id": id, "verdict": "GREEN"})).is_err());
    let submitted = invoke(
        &window,
        "human_submit",
        json!({"id": id, "job": 5, "verdict": "GREEN", "issues": []}),
    )
    .unwrap();
    assert_eq!(submitted["settled"], "published");
    assert_eq!(submitted["view"]["stage"], "completed");
    let saved = saved(&fixture, "release/signoff");
    assert_eq!(
        saved.request.result().map(|result| result.to_json()),
        Some(json!({"verdict": "GREEN", "kind": "human", "issues": []}))
    );
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([])
    );
    // The submitted review stops waiting: the stack is empty.
    assert_eq!(waiting(&window), json!([]));
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_review_cancels_its_tool_before_a_submission_from_another_window() {
    finish_a_review_with_a_running_tool(true);
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn release_and_quit_cancels_a_claimed_reviews_tool_before_releasing_it() {
    finish_a_review_with_a_running_tool(false);
}

fn finish_a_review_with_a_running_tool(submit: bool) {
    let inputs = os::tempdir();
    let started = inputs.path().join("started");
    let release = inputs.path().join("finish");
    let arguments = json!(["touch", started, "wait", release]);
    let tool = json!(os::steps_program(inputs.path()));
    let declaration = format!(
        r#"name = "busy"
fingerprint = false

[views.human_tools]
wait = {{ description = "Wait for the test gate.", command = {tool}, args = {arguments}, kind = "output" }}

[evals.approve]
title = "Approve the fixture"
profile = {{ kind = "human" }}
payload.instruction = "Review the fixture."
"#
    );
    let fixture = fixture_with(&[("busy", &declaration)]);
    let window = window(&fixture, Mode::Review);
    let other = tauri::WebviewWindowBuilder::new(&window.app, "other", Default::default())
        .build()
        .unwrap();
    let id = saved(&fixture, "busy/approve").request.id.to_string();
    invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    let fresh = invoke(&window, "human_draft", json!({"id": id})).unwrap();
    assert!(fresh["verdict"].is_null());
    let no_verdict =
        invoke(&window, "human_draft_submit", json!({"id": id, "job": 2})).unwrap_err();
    assert_eq!(no_verdict["kind"], "invalid");
    invoke(
        &window,
        "human_draft_edit",
        json!({"id": id, "change": {"kind": "verdict", "verdict": "GREEN"}}),
    )
    .unwrap();
    let saved_draft = invoke(&other, "human_draft", json!({"id": id})).unwrap();
    assert_eq!(saved_draft["verdict"], "GREEN");

    struct FinishOnDrop<'a>(&'a Path);
    impl Drop for FinishOnDrop<'_> {
        fn drop(&mut self) {
            let _ = fs::write(self.0, "");
        }
    }
    std::thread::scope(|scope| {
        let first = &window.window;
        let id = id.as_str();
        let running = scope.spawn(move || {
            invoke(
                first,
                "human_run",
                json!({"id": id, "tool": "busy_wait_busy", "job": 2}),
            )
        });
        let gate = FinishOnDrop(&release);
        os::wait_until(std::time::Duration::from_secs(15), || started.is_file());
        let rejected = invoke(&other, "human_release", json!({"ids": [id], "job": 2})).unwrap_err();
        assert_eq!(rejected["kind"], "action");
        assert!(
            rejected["message"]
                .as_str()
                .unwrap()
                .contains("work in progress")
        );
        let rejected = invoke(
            &other,
            "human_run",
            json!({"id": id, "tool": "busy_wait_busy", "job": 2}),
        )
        .unwrap_err();
        assert_eq!(rejected["kind"], "action");
        if submit {
            let submitted =
                invoke(&other, "human_draft_submit", json!({"id": id, "job": 2})).unwrap();
            assert_eq!(submitted["settled"], "published");
        } else {
            let released = invoke(
                &other,
                "human_release_taken",
                json!({"ids": [id], "job": 2}),
            )
            .unwrap();
            assert_eq!(released["notice"]["error"], false);
        }
        let ran = running.join().unwrap().unwrap();
        assert_eq!(
            ran["error"], true,
            "the unfinished tool was cancelled: {ran}"
        );
        assert!(
            !release.exists(),
            "the tool was never told to finish on its own"
        );
        drop(gate);
    });
    let saved = saved(&fixture, "busy/approve");
    assert!(saved.claim.is_none());
    if submit {
        assert_eq!(
            saved.request.status(),
            artifactize_core::types::RequestStatus::Green
        );
    } else {
        assert!(saved.request.status().is_human_wait());
        let draft = invoke(&other, "human_draft", json!({"id": id})).unwrap();
        assert_eq!(draft["verdict"], "GREEN");
    }
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn two_windows_add_issues_to_one_draft() {
    let fixture = fixture();
    let first = window(&fixture, Mode::Review);
    let second = tauri::WebviewWindowBuilder::new(&first.app, "second", Default::default())
        .build()
        .unwrap();
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    invoke(&first, "human_claim", json!({"id":id,"job":1})).unwrap();
    for (window, text) in [(&*first, "saved in Review"), (&second, "saved in the pet")] {
        invoke(
            window,
            "human_draft_edit",
            json!({"id":id,"change":{"kind":"addIssue","text":text}}),
        )
        .unwrap();
    }
    let draft = invoke(&first, "human_draft", json!({"id":id})).unwrap();
    assert_eq!(
        draft["issues"],
        json!(["saved in Review", "saved in the pet"])
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn an_edit_during_submission_is_saved_without_settling_the_older_answer() {
    let inputs = os::tempdir();
    let steps = inputs.path().join("fingerprint-steps");
    let started = inputs.path().join("started");
    let release = inputs.path().join("finish");
    fs::write(&steps, "print\nstable-fingerprint\n").unwrap();
    let command = json!(os::steps_program(inputs.path()));
    let args = json!(["from", steps]);
    let declaration = format!(
        r#"name = "editable"
type = "files"
fingerprint = {{ script = {{ command = {command}, args = {args}, timeout_ms = 45000 }} }}

[evals.approve]
title = "Approve the fixture"
profile = {{ kind = "human" }}
payload.instruction = "Review the fixture."
"#
    );
    let fixture = fixture_with(&[("editable", &declaration)]);
    let window = window(&fixture, Mode::Review);
    let other = tauri::WebviewWindowBuilder::new(&window.app, "other", Default::default())
        .build()
        .unwrap();
    let id = saved(&fixture, "editable/approve").request.id.to_string();
    invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    invoke(
        &window,
        "human_draft_edit",
        json!({"id": id, "change": {"kind": "verdict", "verdict": "RED"}}),
    )
    .unwrap();
    invoke(
        &window,
        "human_draft_edit",
        json!({"id": id, "change": {"kind": "addIssue", "text": "Before Submit"}}),
    )
    .unwrap();
    let edit = |issue: &str| json!({"id": id, "change": {"kind": "editIssue", "index": 0, "previous": "Before Submit", "text": issue}});
    // An answer given whole while a draft is saved is not the draft's: it is refused.
    let stale = invoke(
        &other,
        "human_submit",
        json!({"id": id, "job": 5, "verdict": "RED", "issues": ["Stale copy"]}),
    )
    .unwrap_err();
    assert!(
        stale["message"]
            .as_str()
            .unwrap()
            .contains("verdict or issues changed"),
        "{stale}"
    );

    fs::write(
        &steps,
        format!(
            "touch\n{}\nwait\n{}\nprint\nstable-fingerprint\n",
            started.to_str().unwrap(),
            release.to_str().unwrap()
        ),
    )
    .unwrap();

    struct OpenGate<'a>(&'a Path);
    impl Drop for OpenGate<'_> {
        fn drop(&mut self) {
            let _ = fs::write(self.0, "");
        }
    }
    std::thread::scope(|scope| {
        let first = &window.window;
        let id = &id;
        let submitted =
            scope.spawn(move || invoke(first, "human_draft_submit", json!({"id": id, "job": 2})));
        let gate = OpenGate(&release);
        os::wait_until(std::time::Duration::from_secs(15), || started.is_file());
        invoke(&other, "human_draft_edit", edit("Saved during Submit")).unwrap();
        drop(gate);
        let rejected = submitted.join().unwrap().unwrap_err();
        assert_eq!(rejected["kind"], "invalid");
        assert!(
            rejected["message"]
                .as_str()
                .unwrap()
                .contains("changed while submitting")
        );
    });
    assert!(
        saved(&fixture, "editable/approve")
            .request
            .status()
            .is_human_wait()
    );
    let submitted = invoke(&other, "human_draft_submit", json!({"id": id, "job": 3})).unwrap();
    assert_eq!(submitted["settled"], "published");
    assert_eq!(
        saved(&fixture, "editable/approve")
            .request
            .result()
            .unwrap()
            .to_json()["issues"],
        json!([{"reason": "Saved during Submit"}])
    );
}

/// A Human eval whose answer is a verdict and its issues.
const PLAIN: &str = r#"name = "plain"
fingerprint = false

[evals.approve]
title = "Approve the fixture"
profile = { kind = "human" }
payload.instruction = "Review the fixture."
"#;

fn blocked(reason: &str) -> Value {
    json!({"kind": "blocked", "reason": reason})
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn issues_are_edited_in_order_survive_a_restart_and_a_release_and_submit_as_red() {
    let fixture = fixture_with(&[("plain", PLAIN)]);
    let window = window(&fixture, Mode::Review);
    let other = tauri::WebviewWindowBuilder::new(&window.app, "other", Default::default())
        .build()
        .unwrap();
    let id = saved(&fixture, "plain/approve").request.id.to_string();
    invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    let change = |change: Value| json!({"id": id, "change": change});

    let fresh = invoke(&window, "human_draft", json!({"id": id})).unwrap();
    assert_eq!(fresh["issues"], json!([]));
    assert_eq!(fresh["readiness"]["green"], json!({"kind": "ready"}));
    assert_eq!(
        fresh["readiness"]["red"],
        blocked(artifactize_core::human::RED_WITHOUT_ISSUE)
    );

    let empty = invoke(
        &window,
        "human_draft_edit",
        change(json!({"kind": "addIssue", "text": "  \n"})),
    )
    .unwrap_err();
    assert_eq!(empty["kind"], "invalid");
    let first = invoke(
        &window,
        "human_draft_edit",
        change(json!({"kind": "addIssue", "text": "Totals are wrong"})),
    )
    .unwrap();
    assert_eq!(first["readiness"]["red"], json!({"kind": "ready"}));
    assert_eq!(
        first["readiness"]["green"],
        blocked(artifactize_core::human::GREEN_WITH_ISSUES)
    );
    invoke(
        &other,
        "human_draft_edit",
        change(json!({"kind": "addIssue", "text": "The footer overlaps"})),
    )
    .unwrap();

    // An edit or removal names the issue it saw; an issue changed meanwhile is refused.
    for stale in [
        json!({"kind": "editIssue", "index": 0, "previous": "Totals look wrong", "text": "x"}),
        json!({"kind": "removeIssue", "index": 1, "previous": "Totals are wrong"}),
        json!({"kind": "removeIssue", "index": 2, "previous": "The footer overlaps"}),
    ] {
        let refused = invoke(&window, "human_draft_edit", change(stale)).unwrap_err();
        assert_eq!(refused["kind"], "action", "{refused}");
    }
    let cleared = invoke(
        &window,
        "human_draft_edit",
        change(
            json!({"kind": "editIssue", "index": 0, "previous": "Totals are wrong", "text": " "}),
        ),
    )
    .unwrap_err();
    assert_eq!(cleared["kind"], "invalid");
    invoke(
        &window,
        "human_draft_edit",
        change(json!({"kind": "editIssue", "index": 0, "previous": "Totals are wrong", "text": "Totals are off by one"})),
    )
    .unwrap();
    invoke(
        &window,
        "human_draft_edit",
        change(json!({"kind": "addIssue", "text": "A typo"})),
    )
    .unwrap();
    let removed = invoke(
        &window,
        "human_draft_edit",
        change(json!({"kind": "removeIssue", "index": 2, "previous": "A typo"})),
    )
    .unwrap();
    let issues = json!(["Totals are off by one", "The footer overlaps"]);
    assert_eq!(removed["issues"], issues);

    // The draft outlives the app and a release.
    drop(other);
    drop(window);
    let window = self::window(&fixture, Mode::Review);
    let restarted = invoke(&window, "human_draft", json!({"id": id})).unwrap();
    assert_eq!(restarted["issues"], issues);
    invoke(&window, "human_release", json!({"ids": [id], "job": 1})).unwrap();
    let released = invoke(&window, "human_draft", json!({"id": id})).unwrap();
    assert_eq!(released["issues"], issues);
    invoke(&window, "human_claim", json!({"id": id, "job": 2})).unwrap();

    // GREEN with issues is refused; RED takes them.
    invoke(
        &window,
        "human_draft_edit",
        change(json!({"kind": "verdict", "verdict": "GREEN"})),
    )
    .unwrap();
    let refused = invoke(&window, "human_draft_submit", json!({"id": id, "job": 3})).unwrap_err();
    assert_eq!(refused["kind"], "invalid");
    assert_eq!(
        refused["message"],
        artifactize_core::human::GREEN_WITH_ISSUES
    );
    invoke(
        &window,
        "human_draft_edit",
        change(json!({"kind": "verdict", "verdict": "RED"})),
    )
    .unwrap();
    let submitted = invoke(&window, "human_draft_submit", json!({"id": id, "job": 4})).unwrap();
    assert_eq!(submitted["settled"], "published");
    assert_eq!(
        saved(&fixture, "plain/approve")
            .request
            .result()
            .map(|result| result.to_json()),
        Some(json!({
            "verdict": "RED",
            "kind": "human",
            "issues": [{"reason": "Totals are off by one"}, {"reason": "The footer overlaps"}],
        }))
    );
    let after = invoke(&window, "human_draft", json!({"id": id})).unwrap();
    assert_eq!(after["issues"], json!([]));
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_delayed_pet_show_cannot_redisplay_a_released_review() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.clone();
    invoke(&window, "human_claim", json!({"id":id,"job":1})).unwrap();
    invoke(&window, "active_review_set", json!({"request":id})).unwrap();
    let shown = invoke(&window, "pet_show", json!({"request":id})).unwrap_err();
    assert_eq!(shown["message"], "The app has no windows yet.");
    invoke(&window, "human_release", json!({"ids":[id],"job":2})).unwrap();
    assert_eq!(
        invoke(&window, "pet_show", json!({"request":id})).unwrap(),
        Value::Null
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_shared_review_opens_the_original_artifact_viewer_not_the_followers() {
    const PAGE: &str = r#"name = "page"
type = "files"
fingerprint = { files = ["content.txt"] }

[evals.approve]
title = "Review the page"
profile = { kind = "human" }
payload.instruction = "Read the page."

[evals.another]
title = "Review the page separately"
profile = { kind = "human" }
payload.instruction = "Read the page separately."
"#;
    let prepare = |repo: &Path| {
        fs::create_dir_all(repo.join("page")).unwrap();
        support::declaration::write(repo, repo.join("page/index.artf"), PAGE).unwrap();
        fs::write(
            support::declaration::source_path(repo.join("page/content.txt")),
            "original",
        )
        .unwrap();
    };
    let root = os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    prepare(&repo);
    let fixture = Fixture {
        _root: run_fixture::RepoLease::new(root, &state, &repo),
        repo,
        state,
    };
    verify(&fixture.repo, &fixture.state);
    let original = saved(&fixture, "page/approve").request.id.to_string();
    let root = os::tempdir();
    let follower_repo = root.path().join("follower");
    prepare(&follower_repo);
    support::declaration::write(
        &follower_repo,
        follower_repo.join("page/index.artf"),
        PAGE.replace(
            "title = \"Review the page\"",
            "title = \"Review the follower page\"",
        ),
    )
    .unwrap();
    let _lease = run_fixture::RepoLease::new(root, &fixture.state, &follower_repo);
    verify(&follower_repo, &fixture.state);
    fs::write(
        support::declaration::source_path(follower_repo.join("page/content.txt")),
        "follower",
    )
    .unwrap();
    let follower = saved(&fixture, "page/approve");
    let id = follower.request.id.to_string();
    assert_ne!(id, original);
    let follower_run = follower.request.run_id.to_string();
    assert_eq!(
        follower.execution.unwrap().provenance.request_id.as_str(),
        original
    );

    let window = window(&fixture, Mode::Review);
    let claim = invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    assert_eq!(claim["view"]["stage"], "review");
    assert_eq!(claim["view"]["id"], id);
    assert_eq!(claim["view"]["target"]["run"], follower_run);
    assert_eq!(claim["view"]["owner"], original);
    let reopened = invoke(&window, "human_open", json!({"id": id, "reload": true})).unwrap();
    assert_eq!(reopened["target"], claim["view"]["target"]);
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["title"], "page/approve · Review the follower page");
    let launched = invoke(
        &window,
        "human_run",
        json!({"id": id, "tool": "files_view_page", "job": 2}),
    )
    .unwrap();
    let denied = invoke(
        &window,
        "viewer_read_text",
        json!({"session": launched["viewer"]["session"], "path":"content.txt"}),
    )
    .unwrap_err();
    assert_eq!(
        denied["message"],
        "This window does not own that Artifact Viewer session."
    );
    let contents = invoke_viewer(
        &window,
        "viewer_read_text",
        json!({"session": launched["viewer"]["session"], "path": "content.txt"}),
    )
    .unwrap();
    assert_eq!(contents, "original");

    let refused = invoke(
        &window,
        "human_run",
        json!({"id":id,"tool":"files_view_page","job":20}),
    )
    .unwrap_err();
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("work in progress")
    );
    let session = &launched["viewer"]["session"];
    let pane = invoke(&window, "browse", json!({"navigate": STILL()})).unwrap();
    let follower_branch = pane["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["project"] == "follower")
        .unwrap();
    let same_name = invoke(
        &window,
        "artifact_tool_run",
        json!({"branch": follower_branch["key"], "artifact": "page", "tool": "files_view_page", "job": 3}),
    )
    .unwrap();
    let same_name_session = &same_name["viewer"]["session"];
    let context = |session: &Value| {
        invoke_viewer(&window, "viewer_context", json!({"session": session})).unwrap()
    };
    assert_eq!(context(session)["petMatch"], false);
    // The follower is what the pet shows; only the original worktree can receive its Point.
    invoke(&window, "active_review_set", json!({"request": id})).unwrap();
    let active = context(session);
    let wrong_worktree = context(same_name_session);
    assert_eq!(active["review"], id);
    assert_eq!(wrong_worktree["review"], id);
    assert_eq!(wrong_worktree["artifact"], active["artifact"]);
    assert_eq!(active["petMatch"], true);
    assert_eq!(
        wrong_worktree["petMatch"], true,
        "the shown follower's viewer can point at the shared draft"
    );

    let result = invoke_viewer(
        &window,
        "viewer_point",
        json!({"session": session, "review": id, "text": "Original worktree"}),
    )
    .unwrap();
    assert!(result.is_null());
    let other = saved(&fixture, "page/another").request.id.to_string();
    invoke(&window, "active_review_set", json!({"request": other})).unwrap();
    assert!(context(session)["review"].is_null());
    let refused = invoke_viewer(
        &window,
        "viewer_point",
        json!({"session": session, "review": id, "text": "Do not append to the earlier review"}),
    )
    .unwrap_err();
    assert_eq!(refused["kind"], "action");
    // Another app writes a claim; the first window still knows which Review was shown.
    let external = launched_window(&fixture, Mode::Review, None);
    invoke(&external, "human_claim", json!({"id": other, "job": 4})).unwrap();
    assert_eq!(context(session)["review"], other);
    let refused = invoke_viewer(
        &window,
        "viewer_point",
        json!({"session": session, "review": active["review"], "text": "Delayed Point"}),
    )
    .unwrap_err();
    assert_eq!(refused["kind"], "action");
    for (review, expected) in [
        (&original, Some("Original worktree")),
        (&id, Some("Original worktree")),
        (&other, None),
    ] {
        let draft = invoke(&window, "human_draft", json!({"id": review})).unwrap();
        assert_eq!(draft["points"][0]["text"].as_str(), expected);
    }
    let result = invoke_viewer(
        &window,
        "viewer_point",
        json!({"session": session, "review": other, "text": "content.txt L1:C1"}),
    )
    .unwrap();
    assert!(result.is_null());
    let draft = invoke(&window, "human_draft", json!({"id": other})).unwrap();
    let point = &draft["points"][0];
    assert_eq!(point["text"], "content.txt L1:C1");
    let edited = invoke(
        &window,
        "human_draft_remove_point",
        json!({"id": other, "point": point}),
    )
    .unwrap();
    assert_eq!(edited["points"], json!([]));
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_legacy_git_run_can_prepare_and_open_its_scoped_viewer() {
    let fixture = fixture_with(&[(
        "page",
        r#"name = "page"
type = "files"
[evals.approve]
title = "Review the page"
profile = { kind = "human" }
payload.instruction = "Read the page."
"#,
    )]);
    let request = saved(&fixture, "page/approve").request;
    tauri::async_runtime::block_on(async {
        let mut run = artifactize_core::store::read_run(&fixture.state, &request.run_id)
            .await
            .unwrap()
            .run;
        assert!(run.repository.common_dir.is_some());
        run.repository = Default::default();
        let store = artifactize_core::store::OwnedReceipts::open(&fixture.state, &fixture.repo)
            .await
            .unwrap();
        store.save_run(&run).await.unwrap();
    });
    let window = window(&fixture, Mode::Review);
    let id = &request.id;
    invoke(&window, "human_claim", json!({"id":id,"job":1})).unwrap();
    invoke(&window, "active_review_set", json!({"request":id})).unwrap();
    let launch = invoke(
        &window,
        "human_run",
        json!({"id":id,"tool":"files_view_page","job":2}),
    )
    .unwrap();
    let viewer = &launch["viewer"];
    let context = invoke_viewer(
        &window,
        "viewer_context",
        json!({"session":viewer["session"]}),
    )
    .unwrap();
    assert_eq!(context["petMatch"], true);
    // These command fixtures have no window actions attached. This error is after the real
    // checkout and scope checks: older Git records reach the same native opener as new ones.
    let native = invoke(
        &window,
        "viewer_window",
        json!({"session":viewer["session"],"title":viewer["title"]}),
    )
    .unwrap_err();
    assert_eq!(
        native,
        json!({"kind":"action","message":"The app has no windows yet."})
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_new_branch_target_matches_the_pet_instead_of_the_old_viewer() {
    let root = os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir_all(repo.join(".artifactize")).unwrap();
    os::git(&repo, &["init", "-q", "-b", "primary"]);
    let declare = |target: &str| {
        fs::create_dir_all(repo.join(target)).unwrap();
        fs::write(repo.join(target).join("content.txt"), target).unwrap();
        fs::write(
            repo.join(".artifactize/page.artf"),
            format!(
                r#"artf page : files = {{
  path = "{target}/"
  fingerprint = {{ files = ["content.txt"] }}

  eval approve : human = {{
    title = "Review the page"
    instruction = "Read the page."
  }}
}}
"#,
            ),
        )
        .unwrap();
        os::git(&repo, &["add", ".artifactize/page.artf", target]);
        os::git(
            &repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-qm",
                target,
            ],
        );
    };
    declare("primary");
    let fixture = Fixture {
        _root: run_fixture::RepoLease::new(root, &state, &repo),
        repo: repo.clone(),
        state,
    };
    let window = window(&fixture, Mode::Review);
    let open_review_viewer = |job: u32| {
        verify(&fixture.repo, &fixture.state);
        let id = saved(&fixture, "page/approve").request.id.to_string();
        invoke(&window, "human_claim", json!({"id": id, "job": job})).unwrap();
        invoke(&window, "active_review_set", json!({"request": id})).unwrap();
        let ran = invoke(
            &window,
            "human_run",
            json!({"id": id, "tool": "files_view_page", "job": job + 1}),
        )
        .unwrap();
        ran["viewer"]["session"].clone()
    };
    let context = |session: &Value| {
        invoke_viewer(&window, "viewer_context", json!({"session": session})).unwrap()
    };
    let old = open_review_viewer(1);
    assert_eq!(context(&old)["petMatch"], true);

    os::git(&repo, &["switch", "-qc", "alternate"]);
    assert_eq!(context(&old)["petMatch"], false);
    let refused = invoke_viewer(
        &window,
        "viewer_read_text",
        json!({"session":old,"path":"content.txt"}),
    )
    .unwrap_err();
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("checkout changed")
    );
    invoke(&window, "browse", json!({"navigate":STILL()})).unwrap();
    let other = invoke(&window, "browse", json!({"navigate":STILL()})).unwrap()["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["name"] == "alternate")
        .unwrap()["key"]
        .clone();
    let elsewhere = invoke(
        &window,
        "artifact_tool_run",
        json!({
            "branch": other, "artifact": "page", "tool": "files_view_page", "job": 8,
        }),
    )
    .unwrap()["viewer"]["session"]
        .clone();
    assert_eq!(context(&elsewhere)["target"], "primary");
    assert_eq!(
        context(&elsewhere)["petMatch"],
        false,
        "the name and path did not change, only the branch"
    );
    declare("alternate");
    let new_session = open_review_viewer(3);
    let old = context(&old);
    let new = context(&new_session);
    assert_eq!(old["review"], Value::Null);
    assert!(new["review"].is_string());
    assert_eq!(old["artifact"], new["artifact"]);
    assert_eq!(old["type"], new["type"]);
    assert_eq!(old["target"], "primary");
    assert_eq!(new["target"], "alternate");
    assert_eq!(old["petMatch"], false);
    assert_eq!(new["petMatch"], true);
    // This Viewer was opened from Current, not through the review whose pending tool launch
    // Release will cancel. Its bridge can still be queried after the claim is gone.
    let current = invoke(
        &window,
        "artifact_tool_run",
        json!({"branch":other,"artifact":"page","tool":"files_view_page","job":9}),
    )
    .unwrap()["viewer"]["session"]
        .clone();
    assert_eq!(context(&current)["review"], new["review"]);
    let release = invoke(
        &window,
        "human_release_taken",
        json!({"ids":[new["review"]],"job":11}),
    )
    .unwrap();
    assert_eq!(release["notice"]["error"], false);
    assert_eq!(
        context(&current)["review"],
        Value::Null,
        "releasing the taken claim immediately takes Point away"
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_review_never_goes_back_to_a_view_read_before_its_newest() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    // The stack is read with the review unclaimed; the review is then opened and claimed.
    let shown = invoke(&window, "browse", json!({"navigate": STILL()})).unwrap();
    assert_eq!(shown["waiting"][0]["claim"], "-");
    let branch = shown["branches"][0]["key"].clone();
    let open = invoke(&window, "human_open", json!({"id": id, "reload": true})).unwrap();
    let claimed = invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    assert_eq!(claimed["view"]["stage"], "review");
    // A pane step that does not read the stack again keeps the stack it read before the claim;
    // the review's redraw still shows the claim, not that older read.
    invoke(&window, "browse", json!({"navigate": star(&branch, true)})).unwrap();
    let redrawn = invoke(&window, "human_open", json!({"id": id, "reload": false})).unwrap();
    assert_eq!(redrawn["stage"], "review");
    // Each view says which read it is: a later read has a higher number.
    let read = |view: &Value| view["read"].as_u64().unwrap();
    assert!(
        read(&claimed["view"]) > read(&open),
        "{open} then {claimed}"
    );
    assert_eq!(read(&redrawn), read(&claimed["view"]));
    // A stack read after the claim is newer than the review's last read; its redraw shows it.
    assert_ne!(waiting(&window)[0]["claim"], "-");
    let after = invoke(&window, "human_open", json!({"id": id, "reload": false})).unwrap();
    assert_eq!(after["stage"], "review");
    assert!(
        read(&after) > read(&claimed["view"]),
        "{claimed} then {after}"
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_replaced_state_drops_the_old_reviews_claims_and_cached_views() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let old = saved(&fixture, "release/signoff").request.id.to_string();
    let generation = |id: &str| {
        invoke(&window, "human_open", json!({"id": id, "reload": true})).unwrap()["generation"]
            .clone()
    };
    waiting(&window);
    assert_eq!(generation(&old), 0);
    invoke(&window, "human_claim", json!({"id": old, "job": 1})).unwrap();
    // An ordinary refresh keeps the generation, the claim taken here and the cached view.
    waiting(&window);
    assert_eq!(generation(&old), 0);
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([old])
    );

    // End the old fixture worker before its state disappears; this is teardown, not a read.
    fixture._root.release().unwrap();
    fs::remove_dir_all(&fixture.state).unwrap();
    verify(&fixture.repo, &fixture.state);
    let replaced = invoke(&window, "browse", json!({"navigate": STILL()})).unwrap();
    assert_eq!(replaced["replaced"], json!(true));
    let new = replaced["waiting"][0]["id"].as_str().unwrap().to_owned();
    assert_ne!(new, old);
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([])
    );
    // The old request's view is gone with its state, not redrawn as still actionable.
    assert!(invoke(&window, "human_open", json!({"id": old, "reload": false})).is_err());
    assert_eq!(generation(&new), 1);
}

#[test]
fn a_publication_failure_after_local_settlement_is_saved_locally() {
    const ISOLATED: &str = "ARTIFACTIZE_TEST_PUBLICATION_FAILURE";
    if os::environment_variable(ISOLATED).is_none() {
        let output = os::run_test_without(
            "a_publication_failure_after_local_settlement_is_saved_locally",
            &[(ISOLATED, "1".into())],
            &[
                "ARTIFACTIZE_REMOTE",
                "ARTIFACTIZE_REMOTE_TOKEN",
                "ARTIFACTIZE_REMOTE_SHARE",
            ],
        );
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let fixture = fixture();
    // No real credentials: a loopback store rejects a fake token after local settlement.
    let server = FakeProvider::start(|_| Reply::Json(403, json!({"error": "forbidden"})));
    let url = format!("{}/", server.url);
    fs::write(
        support::declaration::source_path(fixture.state.join("remote.json")),
        json!({"url": url, "share": "summary"}).to_string(),
    )
    .unwrap();
    let credentials = fixture.state.join("auth");
    os::create_private_dir_all(&credentials);
    os::write_private_file(
        &credentials.join("remote-token.json"),
        json!({"url": url, "token": "fixture-token-never-real"}).to_string(),
    );
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    invoke(&window, "human_open", json!({"id": id, "reload": true})).unwrap();
    invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    let submitted = invoke(
        &window,
        "human_submit",
        json!({"id": id, "job": 2, "verdict": "GREEN", "issues": []}),
    )
    .unwrap();
    assert_eq!(submitted["settled"], "savedLocally");
    assert_eq!(submitted["notice"]["error"], json!(true));
    assert!(
        submitted["notice"]["text"]
            .as_str()
            .unwrap()
            .contains("saved locally"),
        "{submitted}"
    );
    // The completed local result stays; nothing invites a second submission.
    assert_eq!(submitted["view"]["stage"], "completed");
    assert_eq!(
        saved(&fixture, "release/signoff").request.status(),
        artifactize_core::types::RequestStatus::Green
    );
}

/// Copy the state's database files (`state.sqlite` and its sidecars) into `backup`.
fn back_up(state: &Path, backup: &Path) {
    fs::create_dir_all(backup).unwrap();
    for suffix in ["", "-wal"] {
        let name = format!("{}{suffix}", artifactize_core::store::DATABASE);
        if state.join(&name).exists() {
            fs::copy(state.join(&name), backup.join(&name)).unwrap();
        }
    }
}

/// Restore `backup` over the state's database files the way a backup tool does: a new file
/// renamed into place, so the database keeps its state identity but is another file.
fn restore(backup: &Path, state: &Path) {
    let database = artifactize_core::store::DATABASE;
    for suffix in ["-shm", "-wal"] {
        let _ = support::declaration::remove_file(state.join(format!("{database}{suffix}")));
    }
    for suffix in ["", "-wal"] {
        let name = format!("{database}{suffix}");
        if backup.join(&name).exists() {
            let staged = state.join(format!("{name}.restoring"));
            fs::copy(backup.join(&name), &staged).unwrap();
            fs::rename(&staged, state.join(&name)).unwrap();
        }
    }
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_backup_restored_over_the_same_state_drops_the_old_reviews_and_claims() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    waiting(&window);
    let backup = fixture._root.path().join("backup");
    back_up(&fixture.state, &backup);
    invoke(&window, "human_open", json!({"id": id, "reload": true})).unwrap();
    invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([id])
    );

    let identity = artifactize_core::store::read_state_id(&fixture.state).unwrap();
    // Preserve the saved backup but stop every worker before replacing its live database.
    fixture._root.release().unwrap();
    restore(&backup, &fixture.state);
    assert_eq!(
        artifactize_core::store::read_state_id(&fixture.state).unwrap(),
        identity,
        "the restored database keeps the state's identity"
    );
    let restored = invoke(&window, "browse", json!({"navigate": STILL()})).unwrap();
    assert_eq!(restored["replaced"], json!(true));
    assert_eq!(restored["waiting"][0]["id"], id);
    assert_eq!(restored["waiting"][0]["claim"], "-");
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([])
    );
    let open = invoke(&window, "human_open", json!({"id": id, "reload": false})).unwrap();
    assert_eq!(open["stage"], "claim");
    assert_eq!(open["generation"], 1);
}

/// A deferred Agent request with a saved conversation of `rows` answers. Its recorded Run
/// has ended with sign-offs pending; it must not claim a live worker that was never started.
async fn agent_request(fixture: &Fixture, session: &str, rows: usize) -> String {
    let receipts = artifactize_core::store::OwnedReceipts::open(&fixture.state, &fixture.repo)
        .await
        .unwrap();
    let run: artifactize_core::store::Run = serde_json::from_value(json!({
        "id": "run-agent",
        "repoPath": fixture.repo,
        "stateDir": fixture.state,
        "status": "YELLOW",
        "createdAt": "2026-01-01T00:00:00Z",
        "completedAt": "2026-01-01T00:00:01Z",
        "selection": {"kind": "all"},
        "validation": null,
        "definitions": {"artifacts": {"release": {"path": "release"}}},
    }))
    .unwrap();
    let request: artifactize_core::store::Request = serde_json::from_value(json!({
        "id": "run-agent-1",
        "runId": "run-agent",
        "evalId": "release/review",
        "target": "release",
        "title": "An Agent reviews the release",
        "profile": {"kind": "agent", "backend": "openai", "model": "fixture"},
        "requestedProfile": {"kind": "agent", "backend": "openai", "model": "fixture"},
        "evalDefHash": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        "payload": {"instruction": "Review."},
        "references": {},
        "deps": [],
        "status": "DEFERRED",
        "result": null,
        "error": null,
        "completedAt": null,
        "createdAt": "2026-01-01T00:00:00Z",
        "startedAt": "2026-01-01T00:00:01Z",
        "cwd": fixture.repo,
        "sessionId": session,
    }))
    .unwrap();
    receipts.create_run(&run, &[request]).await.unwrap();
    // The recorded conversation: a review header, then one answer per row.
    let path = artifactize_core::agent::session::path(&fixture.state, &session.parse().unwrap());
    os::create_private_dir_all(path.parent().unwrap());
    let answer = |text: String| {
        serde_json::to_string(&artifactize_core::agent::session::Event {
            at: None,
            send: None,
            kind: artifactize_core::agent::session::Kind::Answer(
                artifactize_core::agent::session::Answer::Completed(text),
            ),
        })
        .unwrap()
    };
    let lines: Vec<String> = std::iter::once(r#"{"kind":"review"}"#.to_owned())
        .chain((0..rows).map(|row| answer(format!("row {row}"))))
        .collect();
    os::write_private_file(&path, lines.join("\n") + "\n");
    "run-agent-1".into()
}

/// A failure bound for background jobs (transcripts, Current scans), never the ordering or
/// success condition.
const JOB_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Read windows of `handle` until its indexing settles.
fn window_of(window: &WebviewWindow<MockRuntime>, handle: &Value, movement: Value) -> Value {
    let deadline = std::time::Instant::now() + JOB_TIMEOUT;
    let mut movement = movement;
    loop {
        let view = invoke(
            window,
            "transcript_window",
            json!({"handle": handle, "columns": 60, "rows": 5, "movement": movement, "toggle": null, "changed": false}),
        )
        .unwrap();
        movement = Value::Null;
        if view["loading"] == json!(false) {
            return view;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "transcript indexing did not settle: {view}"
        );
    }
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_session_resolved_again_keeps_its_reader_and_position_until_it_changes() {
    let fixture = fixture();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let id = runtime.block_on(agent_request(&fixture, "fixture-session", 40));
    let window = window(&fixture, Mode::Monitor);
    let opened = invoke(
        &window,
        "transcript_open",
        json!({"id": id, "handle": null}),
    )
    .unwrap();
    let handle = opened["handle"].clone();
    assert_eq!(opened["session"], "fixture-session");
    window_of(&window, &handle, Value::Null);
    // Scroll up into history: paused, away from the bottom.
    let paused = window_of(&window, &handle, json!({"kind": "up", "rows": 10}));
    assert_eq!(paused["following"], json!(false));

    // The request completes with the same session: its evidence stamp changes, the reader stays.
    runtime.block_on(async {
        let receipts = artifactize_core::store::OwnedReceipts::open(&fixture.state, &fixture.repo)
            .await
            .unwrap();
        let mut view = artifactize_core::store::read_request(&fixture.state, &id)
            .await
            .unwrap();
        std::sync::Arc::make_mut(&mut view.request).state =
            artifactize_core::store::RequestState::completed(
                serde_json::from_value(json!({"verdict": "GREEN"})).unwrap(),
                "2026-01-01T00:00:02Z".parse().unwrap(),
            );
        receipts.save_request(&view.request).await.unwrap();
    });
    // Resolve after the saved completion, without assuming repeated calls made a window
    // job overlap it. The observable contract is the kept handle and paused position.
    let again = invoke(
        &window,
        "transcript_open",
        json!({"id": id, "handle": handle}),
    )
    .unwrap();
    assert_eq!(
        again["handle"], handle,
        "an unchanged session keeps its reader"
    );
    let kept = window_of(&window, &handle, Value::Null);
    assert_eq!(kept["following"], json!(false), "and its paused position");
    assert_eq!(kept["rows"], paused["rows"]);

    // Another session replaces the reader.
    runtime.block_on(async {
        let receipts = artifactize_core::store::OwnedReceipts::open(&fixture.state, &fixture.repo)
            .await
            .unwrap();
        let mut view = artifactize_core::store::read_request(&fixture.state, &id)
            .await
            .unwrap();
        std::sync::Arc::make_mut(&mut view.request).session_id =
            Some("other-session".parse().unwrap());
        receipts.save_request(&view.request).await.unwrap();
    });
    let replaced = invoke(
        &window,
        "transcript_open",
        json!({"id": id, "handle": handle}),
    )
    .unwrap();
    assert_ne!(replaced["handle"], handle);
    assert_eq!(replaced["session"], "other-session");
    assert!(invoke(
        &window,
        "transcript_window",
        json!({"handle": handle, "columns": 60, "rows": 5, "movement": null, "toggle": null, "changed": false}),
    )
    .is_err());
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_request_opened_by_id_that_is_not_a_human_review_opens_a_read_only_detail() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    // The runtime Eval that waits for the sign-off.
    let id = saved(&fixture, "ship/check").request.id.to_string();
    let detail = invoke(&window, "request_detail", json!({"id": id})).unwrap();
    assert_eq!(detail["profile"], "runtime");
    assert_eq!(detail["request"], id);
    assert_eq!(detail["sections"][0]["name"], "outcome");
    let missing = invoke(&window, "request_detail", json!({"id": "run-gone-1"}));
    assert_eq!(missing.unwrap_err()["kind"], "read");
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn an_agent_request_opened_by_id_opens_read_only_with_its_session() {
    let fixture = fixture();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let id = runtime.block_on(agent_request(&fixture, "fixture-session", 3));
    let window = window(&fixture, Mode::Review);
    let detail = invoke(&window, "request_detail", json!({"id": id})).unwrap();
    assert_eq!(detail["profile"], "agent");
    assert_eq!(detail["request"], id);
    let opened = invoke(
        &window,
        "transcript_open",
        json!({"id": id, "handle": null}),
    )
    .unwrap();
    let rows = window_of(&window, &opened["handle"], Value::Null);
    assert!(rows["rows"].to_string().contains("row 2"), "{rows}");
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn the_definitions_are_read_only_with_each_bindings_source() {
    let fixture = fixture();
    fs::write(fixture.repo.join("tip.md"), "# Tip\n").unwrap();
    fs::write(
        fixture.repo.join(".artifactize").join("posts.artf"),
        "// A post.\nartf! post(file) = {\n  path = \"@{param file}\"\n  basis = true\n}\n\nartf tip = post!(file = \"tip.md\")\n",
    )
    .unwrap();
    let monitor = window(&fixture, Mode::Monitor);
    let branch = branch(&monitor);
    let read = invoke(&monitor, "definitions", json!({"branch": branch})).unwrap();
    let node = |name: &str| {
        read["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"] == name)
            .unwrap_or_else(|| panic!("{name} in {read}"))
            .clone()
    };
    let ship = node("ship");
    assert_eq!(ship["definition"], ".artifactize/ship.artf", "{read}");
    assert_eq!(ship["target"], "ship/");
    assert_eq!(ship["macroName"], Value::Null);
    let tip = node("tip");
    assert_eq!(
        [
            &tip["definition"],
            &tip["line"],
            &tip["macroName"],
            &tip["target"]
        ],
        [
            &json!(".artifactize/posts.artf"),
            &json!(7),
            &json!("post!"),
            &json!("tip.md")
        ],
        "{tip}"
    );
    // Written out: its source is the code, with nothing to expand.
    let source = |artifact: &str| {
        invoke(
            &monitor,
            "definition_source",
            json!({"branch": branch, "artifact": artifact}),
        )
        .unwrap()
    };
    let ship = source("ship");
    assert_eq!(ship["file"], ".artifactize/ship.artf");
    assert!(
        ship["authored"].as_str().unwrap().starts_with("artf ship"),
        "{ship}"
    );
    assert_eq!(ship["expanded"], Value::Null);
    // A macro call: the call as written, and the declaration it makes.
    let tip = source("tip");
    assert_eq!(tip["authored"], "artf tip = post!(file = \"tip.md\")");
    assert!(
        tip["expanded"]
            .as_str()
            .unwrap()
            .contains("path = \"tip.md\""),
        "{tip}"
    );
    // Nothing writes them: the editing commands are gone.
    for command in ["definition_edit", "definition_check"] {
        assert!(invoke(&monitor, command, json!({"branch": branch})).is_err());
    }
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_star_marks_a_branch_and_is_kept_for_the_next_monitor() {
    let fixture = fixture();
    let monitor = window(&fixture, Mode::Monitor);
    let view = invoke(&monitor, "browse", json!({"navigate": STILL()})).unwrap();
    assert_eq!(view["branches"][0]["starred"], json!(false));
    let branch = view["branches"][0]["key"].clone();
    let starred = invoke(&monitor, "browse", json!({"navigate": star(&branch, true)})).unwrap();
    assert_eq!(starred["branches"][0]["starred"], json!(true), "{starred}");
    // Kept in the state directory, where another monitor of this user reads it.
    assert_eq!(
        artifactize_core::monitor::live::starred(Some(&fixture.state))
            .unwrap()
            .len(),
        1
    );
    let unstarred = invoke(
        &monitor,
        "browse",
        json!({"navigate": star(&branch, false)}),
    )
    .unwrap();
    assert_eq!(unstarred["branches"][0]["starred"], json!(false));
    assert!(
        artifactize_core::monitor::live::starred(Some(&fixture.state))
            .unwrap()
            .is_empty()
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn the_run_graph_lights_artifacts_and_edges_and_follows_a_submission() {
    let fixture = fixture();
    let monitor = window(&fixture, Mode::Monitor);
    let view = invoke(&monitor, "browse", json!({"navigate": STILL()})).unwrap();
    let run = view["branches"][0]["chips"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    invoke(&monitor, "browse", json!({"navigate": open_run(&run)})).unwrap();
    let source = json!({"kind": "run", "run": run});
    let graph = |window: &WebviewWindow<MockRuntime>, known: Value| {
        invoke(window, "graph", json!({"source": source, "known": known})).unwrap()
    };
    // Release waits for its Human; the downstream runtime Eval waits for release.
    // The edge shows its input Artifact’s active light.
    let read = graph(&monitor, Value::Null);
    assert_eq!(read["source"], source);
    assert_eq!(read["update"]["kind"], "shape");
    let before = &read["update"];
    assert_eq!(
        before["nodes"],
        json!([
            {"id": "a:release", "name": "release", "light": "active", "tally": {"passed": 0, "of": 1}, "note": null, "parts": []},
            {"id": "a:ship", "name": "ship", "light": "waiting", "tally": {"passed": 0, "of": 1}, "note": null, "parts": []},
        ])
    );
    assert_eq!(
        before["edges"],
        json!([{"source": "a:release", "target": "a:ship", "light": "active"}])
    );
    // An Artifact between its neighbours: ship uses release, which nothing else uses.
    assert_eq!(
        invoke(
            &monitor,
            "neighbours",
            json!({"source": source, "node": "a:ship"})
        )
        .unwrap(),
        json!({
            "shape": {
                "nodes": before["nodes"],
                "edges": before["edges"],
            },
            "moreUses": 0,
            "moreUsedBy": 0,
            "wholes": [],
            "copies": 1,
        })
    );
    // An Artifact the Run lacks has no row and no neighbours.
    assert_eq!(row(&monitor, &run, "a:gone"), Value::Null);
    assert_eq!(
        invoke(
            &monitor,
            "neighbours",
            json!({"source": source, "node": "a:gone"})
        )
        .unwrap()["shape"],
        json!({"nodes": [], "edges": []})
    );

    let id = saved(&fixture, "release/signoff").request.id.to_string();
    let human = window(&fixture, Mode::Review);
    invoke(&human, "human_claim", json!({"id": id, "job": 1})).unwrap();
    invoke(
        &human,
        "human_submit",
        json!({"id": id, "job": 2, "verdict": "GREEN", "issues": []}),
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // A portable observation bound, not a scheduling or duration assertion.
        const SETTLEMENT_BOUND: std::time::Duration = std::time::Duration::from_secs(30);
        let mut changes = artifactize_core::changes::Subscription::new(&fixture.state).await;
        let settled = tokio::time::timeout(SETTLEMENT_BOUND, async {
            loop {
                let current = artifactize_core::store::read_run(&fixture.state, &run)
                    .await
                    .unwrap();
                if !current.run.status().is_open() {
                    break current;
                }
                changes.next_reconciled().await;
            }
        })
        .await;
        changes.close().await.unwrap();
        assert_eq!(
            settled.unwrap().run.status(),
            artifactize_core::types::RunStatus::Green
        );
    });
    let view = invoke(&monitor, "browse", json!({"navigate": STILL()})).unwrap();
    // The Run ended: the timeline shows the Run with its final status.
    let chips = &view["branches"][0]["chips"];
    assert_eq!(chips[0]["running"], json!(false));
    assert_eq!(chips[0]["status"], "GREEN", "{view}");
    assert!(
        chips[0]["age"].as_str().unwrap().ends_with(" ago"),
        "{view}"
    );
    // The shape the caller holds comes back as its lights alone, in its order.
    let after = graph(&monitor, read["shape"].clone());
    assert_eq!(after["shape"], read["shape"]);
    assert_eq!(
        after["update"],
        json!({
            "kind": "lights",
            "nodes": ["green", "green"],
            "tallies": [{"passed": 1, "of": 1}, {"passed": 1, "of": 1}],
            "edges": ["green"],
        }),
        "the downstream runtime Eval completes"
    );
    // A caller holding another shape gets this one whole.
    let other = graph(&monitor, json!(read["shape"].as_u64().unwrap() + 1));
    assert_eq!(other["update"]["kind"], "shape");
    // Another Run than the shown one is refused.
    let missing = invoke(
        &monitor,
        "graph",
        json!({"source": {"kind": "run", "run": "run-gone"}, "known": null}),
    );
    assert_eq!(missing.unwrap_err()["kind"], "missing");
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn the_run_graph_lights_an_error_and_blocks_what_follows_it() {
    // A private, absent path cannot become an installed command through the user's PATH.
    // Neither eval needs an executable: broken errors on launch, and after must not launch.
    let programs = os::tempdir();
    let absent = artifactize_tools::files::path_text(&programs.path().join("not-installed"));
    let declaration = |name: &str, title: &str, instruction: &str| {
        support::declaration::to_artf(json!({
            "name": name,
            "evals": {"check": {
                "title": title,
                "profile": {"kind": "runtime", "command": absent, "args": []},
                "payload": {"instruction": instruction},
            }},
        }))
        .unwrap()
    };
    let broken = declaration(
        "broken",
        "A command that is not installed",
        "Run the check.",
    );
    let after = declaration("after", "After broken", "Check {broken}.");
    let fixture = fixture_with(&[("broken", &broken), ("after", &after)]);
    let monitor = window(&fixture, Mode::Monitor);
    let view = invoke(&monitor, "browse", json!({"navigate": STILL()})).unwrap();
    let run = view["branches"][0]["chips"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    invoke(&monitor, "browse", json!({"navigate": open_run(&run)})).unwrap();
    let graph = invoke(
        &monitor,
        "graph",
        json!({"source": {"kind": "run", "run": run}, "known": null}),
    )
    .unwrap();
    let graph = &graph["update"];
    let light = |id: &str| {
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == id)
            .unwrap_or_else(|| panic!("{id} in {graph}"))["light"]
            .clone()
    };
    assert_eq!(light("a:broken"), "error");
    assert_eq!(light("a:after"), "blocked");
    let edge = graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["source"] == "a:broken")
        .unwrap();
    assert_eq!(
        edge,
        &json!({"source": "a:broken", "target": "a:after", "light": "error"})
    );
}

/// Scan `branch`'s Current and read its newest step until the scan ends.
fn scan_current(window: &WebviewWindow<MockRuntime>, branch: &Value) -> (Value, Value) {
    let first = invoke(
        window,
        "current_scan",
        json!({"branch": branch, "refresh": false}),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + JOB_TIMEOUT;
    let mut view = first.clone();
    while view["scanning"] == json!(true) {
        assert!(
            std::time::Instant::now() < deadline,
            "the scan did not end: {view}"
        );
        view = invoke(window, "current_view", json!({"branch": branch})).unwrap();
    }
    (first, view)
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn opening_a_review_does_not_retire_its_claim_when_declarations_change() {
    let fixture = fixture();
    let owner = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.clone();
    invoke(&owner, "human_claim", json!({"id":id,"job":1})).unwrap();
    let changed = RELEASE.replace(
        "Read the release notes and approve them.",
        "Read again and approve.",
    );
    support::declaration::write(
        &fixture.repo,
        fixture.repo.join("release/index.artf"),
        &changed,
    )
    .unwrap();
    let viewer = launched_window(&fixture, Mode::Review, None);
    let opened = invoke(
        &viewer,
        "human_open_reference",
        json!({"id":id,"query":"type:human waiting"}),
    )
    .unwrap();
    assert_eq!(opened["view"]["stage"], "review");
    let retained = saved(&fixture, "release/signoff");
    assert!(retained.request.status().is_human_wait());
    assert_eq!(retained.claim.unwrap().reviewer.as_str(), "alice");
    assert!(opened["view"]["tools"][0]["error"].is_string());
    support::declaration::write(
        &fixture.repo,
        fixture.repo.join("release/index.artf"),
        RELEASE,
    )
    .unwrap();
    let restored = invoke(&viewer, "human_open", json!({"id":id,"reload":true})).unwrap();
    assert!(restored["tools"][0]["error"].is_null());
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_run_reference_keeps_its_chat_branch_and_discovers_a_new_checkout() {
    let fixture = fixture();
    let original = launched_window(&fixture, Mode::Monitor, None);
    let first = invoke(&original, "browse", json!({"navigate":STILL()})).unwrap();
    let branch = first["branches"][0]["key"].clone();
    let name = branch["branch"].as_str().unwrap();
    let provider = FakeProvider::start(|_| support::openai::models(&["fixture-chat"]));
    invoke(&original, "provider_custom_save", json!({
        "name": "scripted", "target": {"kind": "endpoint", "api": "openaiResponses", "url": provider.openai_base()},
        "key": {"kind": "none"},
    })).unwrap();
    let chat = invoke(
        &original,
        "chat_create",
        json!({"worktree":os::path_text(&fixture.repo), "branch":branch,
            "selection":{"engine":"direct","backend":{"kind":"custom","name":"scripted"},"model":"fixture-chat","effort":null}}),
    )
    .unwrap();
    let run = saved(&fixture, "release/signoff")
        .request
        .run_id
        .to_string();
    os::git(
        &fixture.repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "first",
        ],
    );
    os::git(&fixture.repo, &["switch", "--quiet", "-c", "parked"]);

    let restarted = launched_window(&fixture, Mode::Monitor, None);
    let open = || {
        invoke(
            &restarted,
            "browse",
            json!({
                "navigate":{"run":{"kind":"reference","id":run},"full":false,"resync":false},
            }),
        )
        .unwrap()
    };
    let retired = open();
    assert_eq!(retired["run"]["id"], run);
    assert_eq!(retired["run"]["branch"], branch);
    assert_eq!(retired["run"]["chatBranch"], branch);
    let archived_chat = invoke(
        &restarted,
        "chat_latest_for_branch",
        json!({"branch":retired["run"]["chatBranch"]}),
    )
    .unwrap();
    assert_eq!(archived_chat["id"], chat["id"]);
    assert!(archived_chat["unavailable"].is_string());
    let historical = retired["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == branch)
        .unwrap();
    assert_eq!(historical["chips"][0]["id"], run);
    assert_eq!(historical["historical"], true);
    let moved = fixture.repo.with_file_name("new-worktree");
    os::git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "--quiet",
            os::native_path_text(&moved),
            name,
        ],
    );

    // No periodic watcher runs in the mock. Linking again must discover the worktree.
    let shown = open();
    assert_eq!(shown["run"]["branch"], branch);
    let live = shown["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == branch)
        .unwrap();
    assert_eq!(live["chips"][0]["id"], run);
    assert_eq!(live["historical"], false);
    assert_eq!(
        Path::new(live["workspace"].as_str().unwrap()),
        Path::new(&os::path_text(&os::canonical(&moved)))
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_direct_run_reference_reveals_its_branch_without_resetting_verify_search() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Monitor);
    let run = saved(&fixture, "release/signoff")
        .request
        .run_id
        .to_string();
    for query in ["repo:other-project", "repo:"] {
        let filtered = invoke(&window, "browse", json!({"navigate": search(query)})).unwrap();
        assert_eq!(filtered["branches"], json!([]));
        let choices = invoke(&window, "chat_branches", json!({})).unwrap();
        assert_eq!(choices[0]["project"], "repo");
        assert_eq!(
            invoke(&window, "redraw", json!({})).unwrap()["query"]["text"],
            query
        );

        let shown = invoke(
            &window,
            "browse",
            json!({"navigate":{"run":{"kind":"reference","id":run},"query":format!("{query} other"),"full":false,"resync":false}}),
        )
        .unwrap();
        assert_eq!(shown["query"]["text"], format!("{query} other"));
        assert_eq!(shown["branches"][0]["chips"][0]["id"], run);
    }
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_chat_only_checkout_keeps_its_view_and_current_after_restart() {
    let root = os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir_all(repo.join("nested")).unwrap();
    os::git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    support::declaration::write(
        &repo,
        repo.join("index.artf"),
        "name = 'app'\nbasis = true\n",
    )
    .unwrap();
    let fixture = Fixture {
        _root: run_fixture::RepoLease::new(root, &state, &repo),
        repo,
        state,
    };
    let provider =
        FakeProvider::start(|_| support::openai::models(&["another-model", "fixture-chat"]));
    let first = window(&fixture, Mode::Monitor);
    invoke(&first, "provider_custom_save", json!({
        "name": "scripted", "target": {"kind": "endpoint", "api": "openaiResponses", "url": provider.openai_base()},
        "key": {"kind": "none"},
    })).unwrap();
    let make = || {
        invoke(
            &first,
            "chat_create",
            json!({
                "worktree": os::path_text(&fixture.repo),
                "selection": {"engine": "direct", "backend": {"kind": "custom", "name": "scripted"}, "model": "fixture-chat", "effort": null},
            }),
        )
        .unwrap()
    };
    let session = make();
    assert_eq!(session["view"], "conversation");
    let branch = session["branchKey"].clone();
    let id = session["id"].clone();
    invoke(&first, "chat_set_view", json!({"id":id,"view":"graph"})).unwrap();
    let from_nested = invoke(
        &first,
        "chat_default_target",
        json!({
            "worktree": os::path_text(&fixture.repo.join("nested")),
        }),
    )
    .unwrap();
    assert_eq!(from_nested, session["target"]);
    let away = fixture.repo.with_file_name("moved-away");
    fs::rename(&fixture.repo, &away).unwrap();
    let old_launch = window(&fixture, Mode::Graph);
    let started = invoke(&old_launch, "launch", json!({})).unwrap();
    assert_eq!(started["graphBranch"], branch);
    let history = invoke(
        &old_launch,
        "chat_latest_for_branch",
        json!({"branch":started["graphBranch"]}),
    )
    .unwrap();
    assert_eq!(history["id"], id);
    assert_eq!(
        history["unavailable"],
        "This Chat's worktree no longer exists."
    );
    drop(old_launch);
    fs::rename(away, &fixture.repo).unwrap();
    let metadata = fixture.repo.join(".git");
    let parked_metadata = fixture.repo.with_file_name("parked-metadata");
    fs::rename(&metadata, &parked_metadata).unwrap();
    let unregistered = window(&fixture, Mode::Graph);
    let launch = invoke(&unregistered, "launch", json!({})).unwrap();
    assert_eq!(launch["graphBranch"], branch);
    let history = invoke(
        &unregistered,
        "chat_latest_for_branch",
        json!({"branch":launch["graphBranch"]}),
    )
    .unwrap();
    assert_eq!(history["id"], id);
    assert!(history["unavailable"].is_string());
    drop(unregistered);
    fs::rename(parked_metadata, metadata).unwrap();

    os::git(&fixture.repo, &["symbolic-ref", "HEAD", "refs/heads/other"]);
    let refused = invoke(
        &first,
        "chat_create",
        json!({
            "worktree": os::path_text(&fixture.repo), "branch": branch, "selection": null,
        }),
    )
    .unwrap_err();
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("no longer checked out")
    );
    let other = make();
    assert_ne!(other["branchKey"], branch);
    assert_eq!(
        invoke(&first, "chat_latest_for_branch", json!({"branch": branch})).unwrap()["unavailable"],
        "Check out main in this worktree to continue this Chat."
    );
    os::git(&fixture.repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    drop(first);

    let restarted = launched_window(&fixture, Mode::Monitor, None);
    assert_eq!(
        invoke(&restarted, "browse", json!({"navigate":STILL()})).unwrap()["branches"],
        json!([])
    );
    let latest = invoke(
        &restarted,
        "chat_latest_for_branch",
        json!({"branch":branch}),
    )
    .unwrap();
    assert_eq!(latest["id"], id);
    assert_eq!(latest["view"], "graph");
    let restored = invoke(&restarted, "chat_open", json!({"id":id})).unwrap();
    assert_eq!(restored["session"]["view"], "graph");
    assert_eq!(restored["session"]["unavailable"], Value::Null);
    let (_, current) = scan_current(&restarted, &branch);
    assert_eq!(current["artifacts"], 1);
    let definitions = invoke(&restarted, "definitions", json!({"branch":branch})).unwrap();
    assert!(definitions.to_string().contains("app"));
    let source = invoke(
        &restarted,
        "definition_source",
        json!({"branch":branch, "artifact":"app"}),
    )
    .unwrap();
    assert!(
        source["authored"].as_str().unwrap().contains("artf app"),
        "{source}"
    );

    os::git(&fixture.repo, &["symbolic-ref", "HEAD", "refs/heads/other"]);
    let stale = invoke(&restarted, "chat_open", json!({"id":id})).unwrap();
    assert_eq!(
        stale["session"]["unavailable"],
        "Check out main in this worktree to continue this Chat."
    );
    assert!(
        invoke(
            &restarted,
            "current_scan",
            json!({"branch":branch,"refresh":true})
        )
        .is_err()
    );
    assert!(invoke(&restarted, "definitions", json!({"branch":branch})).is_err());
    assert_eq!(
        invoke(
            &restarted,
            "chat_latest_for_branch",
            json!({"branch":other["branchKey"]})
        )
        .unwrap()["id"],
        other["id"]
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn chat_set_worktree_moves_the_session_to_a_listed_project_and_survives_restart() {
    let root = os::tempdir();
    let origin = root.path().join("origin");
    let destination = root.path().join("destination");
    let invalid = root.path().join("not-listed");
    let state = root.path().join("state");
    for (repo, branch) in [
        (&origin, "old-branch"),
        (&destination, "new-branch"),
        (&invalid, "other"),
    ] {
        fs::create_dir(repo).unwrap();
        os::git(repo, &["init", "--quiet", "--initial-branch", branch]);
        support::declaration::write(
            repo,
            repo.join("index.artf"),
            "name = 'app'\nbasis = true\n",
        )
        .unwrap();
    }
    let fixture = Fixture {
        repo: os::canonical(&origin),
        state: state.clone(),
        _root: run_fixture::RepoLease::new(root, &state, &origin),
    };
    let first = window(&fixture, Mode::Monitor);
    let other = launched_window(&fixture, Mode::Monitor, Some(os::canonical(&destination)));
    let create = |window: &Window| {
        let choices = invoke(window, "chat_branches", json!({})).unwrap();
        let workspace = &choices[0]["workspace"];
        invoke(
            window,
            "chat_create",
            json!({"worktree": workspace, "selection":null}),
        )
        .unwrap()
    };
    let original = create(&first);
    let saved_destination = create(&other);
    let id = &original["id"];
    invoke(&first, "chat_set_view", json!({"id":id,"view":"graph"})).unwrap();
    let listed = invoke(&first, "chat_branches", json!({})).unwrap();
    let target = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|branch| branch["key"] == saved_destination["branchKey"])
        .unwrap();
    let move_to = |worktree: &Value| {
        invoke(
            &first,
            "chat_set_worktree",
            json!({"id":id,"worktree":worktree}),
        )
    };
    let invalid_path = os::path_text(&os::canonical(&invalid));
    let failure = move_to(&json!(invalid_path)).unwrap_err();
    assert_eq!(failure["kind"], "invalid");
    assert!(failure["message"].as_str().unwrap().contains(&invalid_path));
    let moved = move_to(&target["workspace"]).unwrap();
    for field in ["id", "title", "model", "createdAt"] {
        assert_eq!(moved[field], original[field]);
    }
    assert_eq!(moved["worktree"], target["workspace"]);
    assert_eq!(moved["branchKey"], target["key"]);
    assert_eq!(moved["branch"], "new-branch");
    assert_eq!(moved["project"], "destination");
    assert_eq!(moved["view"], "graph");
    assert_eq!(moved["unavailable"], Value::Null);
    assert_eq!(move_to(&target["workspace"]).unwrap(), moved);
    let old_list = invoke(
        &first,
        "chat_list",
        json!({"worktree":original["worktree"],"query":null}),
    )
    .unwrap();
    assert_eq!(old_list, json!([]));
    let listing = invoke(
        &first,
        "chat_list",
        json!({"worktree":target["workspace"],"query":"destination new-branch"}),
    )
    .unwrap();
    assert!(listing.as_array().unwrap().contains(&moved));
    let latest = |window: &Window, branch: &Value| {
        invoke(window, "chat_latest_for_branch", json!({"branch":branch})).unwrap()
    };
    assert_eq!(latest(&first, &original["branchKey"]), Value::Null);
    assert_eq!(latest(&first, &moved["branchKey"]), moved);
    drop(other);
    drop(first);

    let restarted = window(&fixture, Mode::Monitor);
    let restored = invoke(&restarted, "chat_open", json!({"id":id})).unwrap();
    assert_eq!(restored["session"], moved);
    assert_eq!(restored["turns"], json!([]));
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn review_search_omits_a_deleted_repository_even_with_no_waiting_filter() {
    let fixture = fixture();
    let monitor = window(&fixture, Mode::Review);
    let search = || {
        invoke(
            &monitor,
            "human_search",
            json!({"query":"type:human","start":null}),
        )
        .unwrap()
    };
    assert_eq!(search()["reviews"].as_array().unwrap().len(), 1);
    fixture._root.release().unwrap();
    fs::remove_dir_all(&fixture.repo).unwrap();
    assert_eq!(search()["reviews"], json!([]));
    let completion = invoke(
        &monitor,
        "human_search_complete",
        json!({"query":"type:human repo:r"}),
    )
    .unwrap();
    assert_eq!(completion["candidates"], json!([]));
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_cancelled_release_does_not_mutate_the_saved_claim() {
    let fixture = fixture();
    let monitor = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.clone();
    let cancelled = tokio_util::sync::CancellationToken::new();
    cancelled.cancel();
    let (log, _events) = artifactize_core::review::log::channel();
    let claimed = tauri::async_runtime::block_on(
        artifactize_core::review::Job::Claim { id: id.clone() }.run(
            fixture.state.clone(),
            "alice".parse().unwrap(),
            cancelled,
            log,
        ),
    );
    assert!(
        matches!(claimed, artifactize_core::review::Outcome::Claimed { result: Err(error) } if error == "Review action cancelled.")
    );
    assert!(saved(&fixture, "release/signoff").claim.is_none());
    invoke(&monitor, "human_claim", json!({"id":id,"job":1})).unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    let (log, _events) = artifactize_core::review::log::channel();
    let released = tauri::async_runtime::block_on(
        artifactize_core::review::Job::Release {
            ids: vec![id.clone()],
            quit: false,
        }
        .run(fixture.state.clone(), "alice".parse().unwrap(), cancel, log),
    );
    assert!(
        matches!(released, artifactize_core::review::Outcome::Released { failed, .. }
        if failed == [(id.clone(), "Review action cancelled.".into())])
    );
    assert_eq!(
        saved(&fixture, "release/signoff")
            .claim
            .unwrap()
            .reviewer
            .as_str(),
        "alice"
    );
    invoke(&monitor, "human_release", json!({"ids":[id],"job":2})).unwrap();
    assert!(saved(&fixture, "release/signoff").claim.is_none());
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_current_scans_the_worktree_as_it_is_now_with_what_verify_would_do() {
    let root = os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(&repo, repo.join(".artf"), "name = 'graph'\n[evals.review]\ntitle = 'Review the graph'\nprofile = { kind = 'human' }\npayload = { instruction = 'Review {app}.' }\n").unwrap();
    support::declaration::write(&repo, repo.join("index.artf"), "name = 'app'\n").unwrap();
    // A part of app: its folder holds it.
    fs::create_dir(repo.join("lib")).unwrap();
    support::declaration::write(&repo, repo.join("lib/index.artf"), "name = 'lib'\n").unwrap();
    let launch = Launch::parse([
        "artifactize-desktop".into(),
        "graph".into(),
        "--repo".into(),
        repo.clone().into_os_string(),
        "--state-dir".into(),
        state.clone().into_os_string(),
        "--reviewer".into(),
        "alice".into(),
    ])
    .unwrap();
    assert_eq!(launch.mode, Mode::Graph);
    let fixture = Fixture {
        _root: run_fixture::RepoLease::new(root, &state, &repo),
        repo,
        state,
    };
    let viewer = window(&fixture, Mode::Graph);
    // The folder the app was started for is listed before its first Run.
    let view = invoke(&viewer, "browse", json!({"navigate": STILL()})).unwrap();
    let branch = view["launch"].clone();
    assert_eq!(view["branches"][0]["key"], branch);
    assert_eq!(view["branches"][0]["chips"], json!([]));

    let structure = artifactize_core::graph_structure::read(&fixture.repo).unwrap();
    // The first step is the graph, its evals still being checked, the rest come as they are
    // known.
    let (first, scanned) = scan_current(&viewer, &branch);
    assert_eq!(first["fingerprint"], structure.fingerprint().as_str());
    assert_eq!(first["artifacts"], 3);
    assert!(scanned["revision"].as_u64() >= first["revision"].as_u64());
    assert_eq!(scanned["checking"], 0);
    assert_eq!(scanned["failure"], Value::Null);
    // Never verified: the Human eval would run; the Artifacts without evals are reused, app
    // since its part lib is. lib is drawn inside app, by no edge.
    let source = json!({"kind": "current", "branch": branch});
    let graph = invoke(&viewer, "graph", json!({"source": source, "known": null})).unwrap();
    // A Current comes whole on every read, its shape named.
    assert_eq!(graph["update"]["kind"], "shape");
    assert!(graph["shape"].as_u64().is_some(), "{graph}");
    let graph = &graph["update"];
    assert_eq!(
        graph["nodes"],
        json!([
            {"id": "a:app", "name": "app", "light": "reused", "tally": null, "note": "reused", "parts": ["a:lib"]},
            {"id": "a:graph", "name": "graph", "light": "run", "tally": {"passed": 0, "of": 1}, "note": "would run (1 eval)", "parts": []},
            {"id": "a:lib", "name": "lib", "light": "reused", "tally": null, "note": "reused", "parts": []},
        ])
    );
    assert_eq!(
        graph["edges"],
        json!([{"source": "a:app", "target": "a:graph", "light": "reused"}])
    );
    // graph between its neighbours: it uses app, which nothing else uses.
    assert_eq!(
        invoke(
            &viewer,
            "neighbours",
            json!({"source": source, "node": "a:graph"})
        )
        .unwrap(),
        json!({
            "shape": {"nodes": graph["nodes"].as_array().unwrap()[..2], "edges": graph["edges"]},
            "moreUses": 0,
            "moreUsedBy": 0,
            "wholes": [],
            "copies": 1,
        })
    );
    // Its row, with its eval, as the Detail's chips show them; none for an Artifact it lacks.
    let row = invoke(
        &viewer,
        "detail_row",
        json!({"source": source, "node": "a:graph"}),
    )
    .unwrap();
    assert_eq!(row["children"][0]["name"], "review");
    assert_eq!(
        invoke(
            &viewer,
            "detail_row",
            json!({"source": source, "node": "a:gone"})
        )
        .unwrap(),
        Value::Null
    );
    let detail = invoke(
        &viewer,
        "detail",
        json!({"source": source, "node": "e:graph/review"}),
    )
    .unwrap();
    assert_eq!(detail["plan"]["text"], "would run");
    assert_eq!(detail["card"]["instruction"], "Review app.");
    assert_eq!(detail["card"]["result"], Value::Null, "nothing to reuse");
    assert_eq!(detail["profile"], "human");
    // Nothing was verified or recorded.
    assert!(
        !fixture
            .state
            .join(artifactize_core::store::DATABASE)
            .exists()
    );

    // Scanning again reads the changed declaration.
    support::declaration::write(
        &fixture.repo,
        fixture.repo.join("index.artf"),
        "name = 'app'\nbasis = true\n",
    )
    .unwrap();
    let (again, _) = scan_current(&viewer, &branch);
    assert_ne!(again["fingerprint"], scanned["fingerprint"]);
    assert!(again["revision"].as_u64() > scanned["revision"].as_u64());
    assert_eq!(
        again["fingerprint"],
        artifactize_core::graph_structure::read(&fixture.repo)
            .unwrap()
            .fingerprint()
            .as_str()
    );
    // A branch the Pane does not list has no Current.
    let elsewhere = json!({"kind": "folder", "path": "/nowhere"});
    let missing = invoke(
        &viewer,
        "current_scan",
        json!({"branch": elsewhere, "refresh": false}),
    );
    assert_eq!(missing.unwrap_err()["kind"], "missing");
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn an_artifacts_own_tools_run_in_its_worktree_without_a_review_or_claim() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Monitor);
    let branch = branch(&window);
    let tools = invoke(
        &window,
        "artifact_tools",
        json!({"branch": branch, "artifact": "release"}),
    )
    .unwrap();
    assert_eq!(tools[0]["name"], "release_notes_release");
    assert_eq!(tools[0]["operation"], "release.notes");
    assert_eq!(tools[0]["action"], "prints release/notes.md");
    // ship declares none of its own.
    assert_eq!(
        invoke(
            &window,
            "artifact_tools",
            json!({"branch": branch, "artifact": "ship"}),
        )
        .unwrap(),
        json!([])
    );
    let ran = invoke(
        &window,
        "artifact_tool_run",
        json!({"branch": branch, "artifact": "release", "tool": "release_notes_release", "job": 1}),
    )
    .unwrap();
    assert_eq!(
        ran,
        json!({"text": "release_notes_release finished.", "error": false})
    );
    // Nobody claimed the waiting review, and nothing was recorded on it.
    let saved = saved(&fixture, "release/signoff");
    assert!(saved.claim.is_none());
    assert_eq!(
        saved.request.status(),
        artifactize_core::types::RequestStatus::Deferred
    );
    // Another Artifact's tool is not this one's to run.
    let refused = invoke(
        &window,
        "artifact_tool_run",
        json!({"branch": branch, "artifact": "ship", "tool": "release_notes_release", "job": 2}),
    )
    .unwrap();
    assert_eq!(
        refused,
        json!({"text": "release_notes_release did not run.", "error": true})
    );
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn an_artifacts_tools_follow_its_declaration_when_it_moves_or_is_renamed() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Monitor);
    let branch = branch(&window);
    let list = |artifact: &str| {
        invoke(
            &window,
            "artifact_tools",
            json!({"branch": branch, "artifact": artifact}),
        )
    };
    assert_eq!(
        list("release").unwrap()[0]["action"],
        "prints release/notes.md"
    );
    // The folder moves: its tools are read where it is declared now.
    fs::create_dir_all(fixture.repo.join("archive")).unwrap();
    fs::rename(
        fixture.repo.join("release"),
        fixture.repo.join("archive/release"),
    )
    .unwrap();
    let file = fixture.repo.join(".artifactize/release.artf");
    let moved = fs::read_to_string(&file)
        .unwrap()
        .replace("path = \"release/\"", "path = \"archive/release/\"");
    fs::write(&file, moved).unwrap();
    assert_eq!(
        list("release").unwrap()[0]["action"],
        "prints archive/release/notes.md"
    );
    // Bound to another name, it is another Artifact; ship's input follows the new name.
    let renamed = fs::read_to_string(&file)
        .unwrap()
        .replace("artf release ", "artf notes ");
    fs::write(&file, renamed).unwrap();
    let ship = fixture.repo.join(".artifactize/ship.artf");
    let follows = fs::read_to_string(&ship).unwrap();
    assert!(follows.contains("inputs = [release]"), "{follows}");
    fs::write(
        &ship,
        follows.replace("inputs = [release]", "inputs = [notes]"),
    )
    .unwrap();
    assert_eq!(list("release").unwrap_err()["kind"], "missing");
    assert_eq!(list("notes").unwrap()[0]["name"], "release_notes_notes");
}
