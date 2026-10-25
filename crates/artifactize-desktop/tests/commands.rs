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

[evals.signoff.pass_schema]
type = "object"
properties.approved = { const = true }
properties.comment = { type = "string" }
required = ["approved", "comment"]
additionalProperties = false

[evals.signoff.fail_schema]
type = "object"
properties.reason = { type = "string", minLength = 3 }
properties.steps = { type = "array", items = { type = "string" } }
required = ["reason"]
additionalProperties = false
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
        fs::write(repo.join(name).join("index.artf"), text).unwrap();
    }
    fs::write(repo.join("release/notes.md"), "# Release notes\n- Ready.\n").unwrap();
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
    let launch = Launch {
        mode,
        state: fixture.state.clone(),
        repo: Some(fixture.repo.clone()),
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

const STILL: fn() -> Value = || json!({"run": null, "close": false, "query": null, "older": null, "full": true, "resync": false});

/// Show Run `run` in the Main area.
fn open_run(run: &str) -> Value {
    json!({"run": run, "close": false, "query": null, "older": null, "full": false, "resync": false})
}

/// Star `branch`, or take its star away.
fn star(branch: &Value, on: bool) -> Value {
    json!({"run": null, "close": false, "query": null, "older": null, "star": {"branch": branch, "on": on}, "full": false, "resync": false})
}

/// Read the state afresh with the pane's search `query`.
fn search(query: &str) -> Value {
    json!({"run": null, "close": false, "query": query, "older": null, "full": true, "resync": false})
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
    assert_eq!(view["run"], Value::Null, "nothing chosen yet");
    // The waiting stack lists the sign-off with its project and branch.
    assert_eq!(view["waiting"][0]["eval"], "release/signoff");
    assert_eq!(view["waiting"][0]["project"], "repo");
    assert_eq!(view["waiting"][0]["branch"], branch["name"]);

    let run = chip["id"].as_str().unwrap().to_owned();
    let view = invoke(&window, "browse", json!({"navigate": open_run(&run)})).unwrap();
    assert_eq!(
        tree_names(&window, &view),
        ["Run", "release", "signoff", "ship", "check"]
    );
    let shown = view["run"]["id"].as_str().unwrap();
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
fn the_pane_search_narrows_the_branches_and_waiting_reviews_alike_and_completes() {
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
    assert_eq!(query, json!({"text": "sort:recent", "error": null}));
    assert_eq!(shown("repo:rep").0, 1);
    // Free text also finds a waiting review by its eval; the branch stays only by its own name.
    let (branches, waiting, _) = shown("signoff");
    assert_eq!((branches, waiting), (0, 1));
    assert_eq!(shown("repo:elsewhere").0 + shown("repo:elsewhere").1, 0);
    // An invalid search lists nothing and says why.
    let (branches, waiting, query) = shown("sort:size");
    assert_eq!((branches, waiting), (0, 0));
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
}

#[test]
#[ignore = "run by command_tests_run_without_an_inherited_remote_store"]
fn a_human_review_claims_runs_rejects_an_invalid_red_and_submits_green() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    let listed = waiting(&window);
    assert_eq!(listed[0]["id"], id);
    assert_eq!(listed[0]["claim"], "-");
    assert_eq!(listed.as_array().unwrap().len(), 1);

    let open = invoke(&window, "human_open", json!({"id": id, "reload": true})).unwrap();
    assert_eq!(open["stage"], "claim");
    assert_eq!(open["tools"][0]["name"], "notes_release");
    assert_eq!(open["tools"][0]["action"], "prints release/notes.md");
    // Tools need an explicit claim; opening claimed nothing.
    let refused = invoke(
        &window,
        "human_run",
        json!({"id": id, "tool": "notes_release", "job": 1}),
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
        json!({"id": id, "tool": "notes_release", "job": 3}),
    )
    .unwrap();
    // What it printed reaches the Tool log as events; the command answers with the notice.
    assert_eq!(ran["text"], "notes_release finished.");

    let red = invoke(&window, "human_form", json!({"id": id, "verdict": "RED"})).unwrap();
    assert_eq!(red["fields"][0]["name"], "reason");
    assert_eq!(red["fields"][0]["input"], json!({"kind": "text"}));
    // The library's form adds and removes a list's items and keeps one to type in.
    let steps = |draft: &Value| draft["fields"][1][1].clone();
    let draft = json!({"verdict": "RED", "fields": [["reason", {"kind": "text", "value": ""}], ["steps", {"kind": "list", "value": ["a"]}]]});
    let added = invoke(
        &window,
        "human_add_item",
        json!({"id": id, "draft": draft, "name": "steps"}),
    )
    .unwrap();
    assert_eq!(steps(&added), json!({"kind": "list", "value": ["a", ""]}));
    let removed = invoke(
        &window,
        "human_remove_item",
        json!({"id": id, "draft": added, "name": "steps", "item": 0}),
    )
    .unwrap();
    assert_eq!(steps(&removed), json!({"kind": "list", "value": [""]}));
    let removed = invoke(
        &window,
        "human_remove_item",
        json!({"id": id, "draft": removed, "name": "steps", "item": 0}),
    )
    .unwrap();
    assert_eq!(steps(&removed), json!({"kind": "list", "value": [""]}));
    let invalid = invoke(
        &window,
        "human_submit",
        json!({"id": id, "job": 4, "draft": {"verdict": "RED", "fields": [["reason", {"kind": "text", "value": "no"}]]}}),
    )
    .unwrap_err();
    assert_eq!(invalid["kind"], "invalid");
    assert!(
        invalid["message"].as_str().unwrap().contains("reason"),
        "{invalid}"
    );
    assert_eq!(
        saved(&fixture, "release/signoff").request.status(),
        artifactize_core::types::RequestStatus::Deferred
    );

    let green = invoke(&window, "human_form", json!({"id": id, "verdict": "GREEN"})).unwrap();
    assert_eq!(
        green["fields"][0]["input"],
        json!({"kind": "fixed", "value": "true"})
    );
    let draft = json!({"verdict": "GREEN", "fields": [["approved", {"kind": "fixed"}], ["comment", {"kind": "text", "value": "확인했습니다"}]]});
    // Choosing the verdict again hands the kept draft to the library, which answers with it.
    let again = invoke(
        &window,
        "human_form",
        json!({"id": id, "verdict": "GREEN", "kept": draft}),
    )
    .unwrap();
    assert_eq!(again["draft"]["fields"][1][1]["value"], "확인했습니다");
    // The editor of owner-fields JSON is gone: a Human result is entered in the form only.
    assert!(invoke(&window, "human_json", json!({"id": id, "draft": draft})).is_err());
    let submitted = invoke(
        &window,
        "human_submit",
        json!({"id": id, "job": 5, "draft": draft}),
    )
    .unwrap();
    assert_eq!(submitted["settled"], "published");
    assert_eq!(submitted["view"]["stage"], "completed");
    let saved = saved(&fixture, "release/signoff");
    assert_eq!(
        saved.request.result().map(|result| result.to_json()),
        Some(json!({"verdict": "GREEN", "approved": true, "comment": "확인했습니다"}))
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
        fixture.state.join("remote.json"),
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
        json!({"id": id, "job": 2, "draft": {"verdict": "GREEN", "fields": [], "json": "{\"approved\": true, \"comment\": \"\"}"}}),
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
        let _ = fs::remove_file(state.join(format!("{database}{suffix}")));
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
    let path = artifactize_core::agent::session::path(&fixture.state, session).unwrap();
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
        view.request.state = artifactize_core::store::RequestState::completed(
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
        view.request.session_id = Some("other-session".parse().unwrap());
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
            {"id": "a:release", "name": "release", "space": null, "label": "release", "light": "active", "note": null},
            {"id": "a:ship", "name": "ship", "space": null, "label": "ship", "light": "waiting", "note": null},
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
        json!({"id": id, "job": 2, "draft": {"verdict": "GREEN", "fields": [["approved", {"kind": "fixed"}], ["comment", {"kind": "text", "value": "ok"}]], "json": null}}),
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
    // The Run ended: the timeline starts with the Current, then the Run with its final status.
    let chips = &view["branches"][0]["chips"];
    assert_eq!(chips[0], json!({"kind": "current"}));
    assert_eq!(chips[1]["running"], json!(false));
    assert_eq!(chips[1]["status"], "GREEN", "{view}");
    assert!(
        chips[1]["age"].as_str().unwrap().ends_with(" ago"),
        "{view}"
    );
    // The shape the caller holds comes back as its lights alone, in its order.
    let after = graph(&monitor, read["shape"].clone());
    assert_eq!(after["shape"], read["shape"]);
    assert_eq!(
        after["update"],
        json!({"kind": "lights", "nodes": ["green", "green"], "edges": ["green"]}),
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
        support::declaration::to_toml(json!({
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
fn a_current_scans_the_worktree_as_it_is_now_with_what_verify_would_do() {
    let root = os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join(".artf"), "name = 'graph'\n[evals.review]\ntitle = 'Review the graph'\nprofile = { kind = 'human' }\npayload = { instruction = 'Review {app}.' }\n").unwrap();
    fs::write(repo.join("index.artf"), "name = 'app'\n").unwrap();
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
    // The folder the app was started for is listed before its first Run, ending with Current.
    let view = invoke(&viewer, "browse", json!({"navigate": STILL()})).unwrap();
    let branch = view["launch"].clone();
    assert_eq!(view["branches"][0]["key"], branch);
    assert_eq!(view["branches"][0]["chips"], json!([{"kind": "current"}]));

    let structure = artifactize_core::graph_structure::read(&fixture.repo).unwrap();
    // The first step is the graph, its evals still being checked, the rest come as they are
    // known.
    let (first, scanned) = scan_current(&viewer, &branch);
    assert_eq!(first["fingerprint"], structure.fingerprint().as_str());
    assert_eq!(first["artifacts"], 2);
    assert!(scanned["revision"].as_u64() >= first["revision"].as_u64());
    assert_eq!(scanned["checking"], 0);
    assert_eq!(scanned["failure"], Value::Null);
    assert_eq!(
        invoke(&viewer, "current_structure", json!({"branch": branch})).unwrap(),
        structure.tree()
    );
    // Never verified: the Human eval would run; the Artifact without evals is reused.
    let source = json!({"kind": "current", "branch": branch});
    let graph = invoke(&viewer, "graph", json!({"source": source, "known": null})).unwrap();
    // A Current comes whole on every read, its shape named.
    assert_eq!(graph["update"]["kind"], "shape");
    assert!(graph["shape"].as_u64().is_some(), "{graph}");
    let graph = &graph["update"];
    assert_eq!(
        graph["nodes"],
        json!([
            {"id": "a:app", "name": "app", "space": null, "label": "app", "light": "reused", "note": "reused"},
            {"id": "a:graph", "name": "graph", "space": null, "label": "graph", "light": "run", "note": "would run (1 eval)"},
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
            "shape": {"nodes": graph["nodes"], "edges": graph["edges"]},
            "moreUses": 0,
            "moreUsedBy": 0,
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
    assert_eq!(detail["card"]["instruction"], "Review {app}.");
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
    fs::write(
        fixture.repo.join("index.artf"),
        "name = 'app'\nbasis = true\n",
    )
    .unwrap();
    let (again, _) = scan_current(&viewer, &branch);
    assert_ne!(again["fingerprint"], scanned["fingerprint"]);
    assert!(again["revision"].as_u64() > scanned["revision"].as_u64());
    assert_eq!(
        invoke(&viewer, "current_structure", json!({"branch": branch})).unwrap(),
        artifactize_core::graph_structure::read(&fixture.repo)
            .unwrap()
            .tree()
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
    assert_eq!(tools[0]["name"], "notes_release");
    assert_eq!(tools[0]["operation"], "notes");
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
        json!({"branch": branch, "artifact": "release", "tool": "notes_release", "job": 1}),
    )
    .unwrap();
    assert_eq!(
        ran,
        json!({"text": "notes_release finished.", "error": false})
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
        json!({"branch": branch, "artifact": "ship", "tool": "notes_release", "job": 2}),
    )
    .unwrap();
    assert_eq!(
        refused,
        json!({"text": "notes_release did not run.", "error": true})
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
    assert_eq!(
        list("release").unwrap()[0]["action"],
        "prints archive/release/notes.md"
    );
    // Renamed, it declares another Artifact. Only its own declarations are read: ship's
    // reference to the old name, which a whole-workspace read refuses, is not.
    let file = fixture.repo.join("archive/release/index.artf");
    let renamed = fs::read_to_string(&file)
        .unwrap()
        .replace(r#"name = "release""#, r#"name = "notes""#);
    fs::write(&file, renamed).unwrap();
    assert_eq!(list("release").unwrap_err()["kind"], "missing");
    assert_eq!(list("notes").unwrap()[0]["name"], "notes_notes");
}
