//! The command boundary: JSON in and out of the IPC handler, on state a real `verify` made.
//! A Human sign-off with a built-in tool and owner schemas waits; a dependency eval waits for
//! it. Built-in tools and dependency evals run alike on every OS.
use artifactize_core::project::{VerifyOptions, selection::Selection};
use artifactize_desktop::{Launch, Mode};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tauri::{
    WebviewWindow,
    ipc::{CallbackFn, InvokeBody},
    test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets},
    webview::InvokeRequest,
};

/// The CLI tests' operating-system helpers: private directories and files.
#[path = "../../artifactize-core/tests/support/os.rs"]
mod os;
#[path = "../../artifactize-core/tests/support/runs.rs"]
#[expect(dead_code, reason = "desktop fixtures only record Human requests")]
mod run_fixture;

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
required = ["reason"]
additionalProperties = false
"#;

const SHIP: &str = r#"name = "ship"

[evals.check]
title = "The release is signed off"
profile = { kind = "dependency", depends_on = ["release"] }
"#;

struct Fixture {
    _root: tempfile::TempDir,
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
        fs::write(repo.join(name).join("index.artf"), text).unwrap();
    }
    fs::write(repo.join("release/notes.md"), "# Release notes\n- Ready.\n").unwrap();
    // Its own repository, not a worktree of the one the tests build in.
    let init = std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(&repo)
        .status()
        .unwrap();
    assert!(init.success());
    verify(&repo, &state);
    Fixture {
        _root: root,
        repo,
        state,
    }
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

fn window(fixture: &Fixture, mode: Mode) -> WebviewWindow<MockRuntime> {
    let launch = Launch {
        mode,
        state: fixture.state.clone(),
        repo: Some(fixture.repo.clone()),
        reviewer: "alice".parse().unwrap(),
        request: None,
        search: artifactize_core::review::search::DEFAULT.into(),
    };
    let app = artifactize_desktop::app(mock_builder(), launch)
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    // The app lives as long as the test; its window keeps a handle to it.
    std::mem::forget(app);
    window
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

const STILL: fn() -> Value =
    || json!({"scope": null, "run": null, "paging": null, "full": true, "resync": false});

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

fn tree_names(view: &Value) -> Vec<String> {
    view["run"]["tree"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|node| {
            std::iter::once(node["name"].as_str().unwrap().to_owned()).chain(
                node["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|child| child["name"].as_str().unwrap().to_owned()),
            )
        })
        .collect()
}

#[test]
fn browse_reads_the_catalog_runs_and_tree_and_detail_reads_the_snapshot() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Monitor);
    let view = invoke(&window, "browse", json!({"navigate": STILL()})).unwrap();

    // The repository starts selected from --repo; the catalog still lists ALL.
    let labels: Vec<_> = view["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["label"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(labels[..2], ["ALL", "repo"]);
    assert!(labels[2].ends_with(" · repo"), "{labels:?}");
    assert_eq!(view["runs"].as_array().unwrap().len(), 1);
    assert_eq!(
        tree_names(&view),
        ["Run", "release", "signoff", "ship", "check"]
    );
    let signoff = &view["run"]["tree"][1]["children"][0];
    assert_eq!(signoff["urgent"], json!(true));
    assert_eq!(signoff["text"][0]["text"], "Human sign-off · unclaimed");
    // ship waits for release: its eval marks release as an unmet upstream Artifact.
    let check = &view["run"]["tree"][2]["children"][0];
    assert_eq!(
        check["upstream"],
        json!([{"node": "a:release", "complete": false}])
    );

    // Selecting ALL keeps the Run; the Detail comes from the shown snapshot.
    let all = invoke(
        &window,
        "browse",
        json!({"navigate": {"scope": {"kind": "all"}, "run": null, "paging": null, "full": false, "resync": false}}),
    )
    .unwrap();
    assert_eq!(all["scopeLabel"], "all repositories");
    let run = all["selectedRun"].as_str().unwrap();
    let detail = invoke(
        &window,
        "detail",
        json!({"run": run, "node": "e:release/signoff"}),
    )
    .unwrap();
    assert_eq!(detail["profile"], "human");
    assert_eq!(detail["sections"][0]["name"], "outcome");
    assert_eq!(
        detail["sections"][0]["fields"][0],
        json!({"key": "Status", "value": "WAITING_HUMAN — waiting for a Human submission"})
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
        json!({"run": "run-gone", "node": "e:release/signoff"}),
    );
    assert_eq!(missing.unwrap_err()["kind"], "missing");
}

#[test]
fn a_human_review_claims_runs_rejects_an_invalid_red_and_submits_green() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    let waiting = invoke(
        &window,
        "review_search",
        json!({"query": artifactize_core::review::search::DEFAULT, "start": 0, "reload": true}),
    )
    .unwrap();
    assert_eq!(waiting["rows"][0]["id"], id);
    assert_eq!(waiting["rows"][0]["claim"], "-");

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
    assert_eq!(ran["output"]["text"], "# Release notes\n- Ready.");
    assert_eq!(ran["notice"]["text"], "notes_release finished.");

    let red = invoke(&window, "human_form", json!({"id": id, "verdict": "RED"})).unwrap();
    assert_eq!(red["fields"][0]["name"], "reason");
    assert_eq!(red["fields"][0]["input"], json!({"kind": "text"}));
    let invalid = invoke(
        &window,
        "human_submit",
        json!({"id": id, "job": 4, "draft": {"verdict": "RED", "fields": [["reason", {"kind": "text", "value": "no"}]], "json": null}}),
    )
    .unwrap_err();
    assert_eq!(invalid["kind"], "invalid");
    assert!(
        invalid["message"].as_str().unwrap().contains("reason"),
        "{invalid}"
    );
    assert_eq!(
        saved(&fixture, "release/signoff").request.status(),
        artifactize_core::types::RequestStatus::WaitingHuman
    );

    let green = invoke(&window, "human_form", json!({"id": id, "verdict": "GREEN"})).unwrap();
    assert_eq!(
        green["fields"][0]["input"],
        json!({"kind": "fixed", "value": "true"})
    );
    // The typed draft as owner-fields JSON, as the terminal review hands it to $EDITOR.
    let draft = json!({"verdict": "GREEN", "fields": [["approved", {"kind": "fixed"}], ["comment", {"kind": "text", "value": "확인했습니다"}]], "json": null});
    // Choosing the verdict again hands the kept draft to the library, which answers with it.
    let again = invoke(
        &window,
        "human_form",
        json!({"id": id, "verdict": "GREEN", "kept": draft}),
    )
    .unwrap();
    assert_eq!(again["draft"]["fields"][1][1]["value"], "확인했습니다");
    let text = invoke(&window, "human_json", json!({"id": id, "draft": draft})).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(text.as_str().unwrap()).unwrap(),
        json!({"approved": true, "comment": "확인했습니다"})
    );
    let submitted = invoke(
        &window,
        "human_submit",
        json!({"id": id, "job": 5, "draft": {"verdict": "GREEN", "fields": [], "json": text}}),
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
    let waiting = invoke(
        &window,
        "review_search",
        json!({"query": artifactize_core::review::search::DEFAULT, "start": 0, "reload": true}),
    )
    .unwrap();
    assert_eq!(waiting["rows"], json!([]));
}

#[test]
fn a_replaced_state_drops_the_old_reviews_claims_and_cached_views() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let waiting = |reload: bool| {
        invoke(
            &window,
            "review_search",
            json!({"query": artifactize_core::review::search::DEFAULT, "start": 0, "reload": reload}),
        )
        .unwrap()
    };
    let old = saved(&fixture, "release/signoff").request.id.to_string();
    assert_eq!(waiting(true)["generation"], 0);
    invoke(&window, "human_open", json!({"id": old, "reload": true})).unwrap();
    invoke(&window, "human_claim", json!({"id": old, "job": 1})).unwrap();
    // An ordinary refresh keeps the generation, the claim taken here and the cached view.
    assert_eq!(waiting(true)["generation"], 0);
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([old])
    );

    fs::remove_dir_all(&fixture.state).unwrap();
    verify(&fixture.repo, &fixture.state);
    let replaced = waiting(true);
    assert_eq!(replaced["generation"], 1);
    assert_ne!(replaced["rows"][0]["id"], old);
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([])
    );
    // The old request's view is gone with its state, not redrawn as still actionable.
    assert!(invoke(&window, "human_open", json!({"id": old, "reload": false})).is_err());
    assert_eq!(waiting(true)["generation"], 1);
}

#[test]
fn a_publication_failure_after_local_settlement_is_saved_locally() {
    let fixture = fixture();
    // No real credentials: a loopback store rejects a fake token after local settlement.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
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
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut connection, _) = listener.accept().unwrap();
        let mut buffer = [0; 4096];
        let _ = connection.read(&mut buffer).unwrap();
        connection
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
    });
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
    server.join().unwrap();
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
fn a_backup_restored_over_the_same_state_drops_the_old_reviews_and_claims() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let id = saved(&fixture, "release/signoff").request.id.to_string();
    let waiting = || {
        invoke(
            &window,
            "review_search",
            json!({"query": artifactize_core::review::search::DEFAULT, "start": 0, "reload": true}),
        )
        .unwrap()
    };
    assert_eq!(waiting()["generation"], 0);
    let backup = fixture._root.path().join("backup");
    back_up(&fixture.state, &backup);
    invoke(&window, "human_open", json!({"id": id, "reload": true})).unwrap();
    invoke(&window, "human_claim", json!({"id": id, "job": 1})).unwrap();
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([id])
    );

    let identity = artifactize_core::store::read_state_id(&fixture.state).unwrap();
    restore(&backup, &fixture.state);
    assert_eq!(
        artifactize_core::store::read_state_id(&fixture.state).unwrap(),
        identity,
        "the restored database keeps the state's identity"
    );
    let restored = waiting();
    assert_eq!(restored["generation"], 1);
    assert_eq!(restored["rows"][0]["id"], id);
    assert_eq!(restored["rows"][0]["claim"], "-");
    assert_eq!(
        invoke(&window, "human_taken", json!({})).unwrap(),
        json!([])
    );
    let open = invoke(&window, "human_open", json!({"id": id, "reload": false})).unwrap();
    assert_eq!(open["stage"], "claim");
    assert_eq!(open["generation"], 1);
}

/// An Agent request of its own Run with a saved conversation of `rows` answers.
async fn agent_request(fixture: &Fixture, session: &str, rows: usize) -> String {
    let receipts = artifactize_core::store::Receipts::open(&fixture.state, &fixture.repo)
        .await
        .unwrap();
    let run: artifactize_core::store::Run = serde_json::from_value(json!({
        "id": "run-agent",
        "repoPath": fixture.repo,
        "stateDir": fixture.state,
        "status": "RUNNING",
        "createdAt": "2026-01-01T00:00:00Z",
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
        "status": "RUNNING",
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

/// Read windows of `handle` until its indexing settles.
fn window_of(window: &WebviewWindow<MockRuntime>, handle: &Value, movement: Value) -> Value {
    let mut movement = movement;
    loop {
        let view = invoke(
            window,
            "transcript_window",
            json!({"handle": handle, "columns": 60, "rows": 5, "movement": movement, "toggle": null, "changed": false}),
        )
        .unwrap();
        movement = Value::Null;
        if view["loading"] == json!(false)
            && view["rows"].as_array().is_some_and(|rows| !rows.is_empty())
        {
            return view;
        }
    }
}

#[test]
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
        let receipts = artifactize_core::store::Receipts::open(&fixture.state, &fixture.repo)
            .await
            .unwrap();
        let mut view = artifactize_core::store::read_request(&fixture.state, &id)
            .await
            .unwrap();
        view.request.state = artifactize_core::store::RequestState::completed(
            serde_json::from_value(json!({"verdict": "GREEN"})).unwrap(),
            artifactize_core::types::Timestamp::now(),
        );
        receipts.save_request(&view.request).await.unwrap();
    });
    // Resolve again while window jobs keep the reader busy: it waits for them, never replaces it.
    let reading = {
        let window = window.clone();
        let handle = handle.clone();
        std::thread::spawn(move || {
            for _ in 0..200 {
                invoke(
                    &window,
                    "transcript_window",
                    json!({"handle": handle, "columns": 60, "rows": 5, "movement": null, "toggle": null, "changed": true}),
                )
                .unwrap();
            }
        })
    };
    let again = invoke(
        &window,
        "transcript_open",
        json!({"id": id, "handle": handle}),
    )
    .unwrap();
    reading.join().unwrap();
    assert_eq!(
        again["handle"], handle,
        "an unchanged session keeps its reader"
    );
    let kept = window_of(&window, &handle, Value::Null);
    assert_eq!(kept["following"], json!(false), "and its paused position");
    assert_eq!(kept["rows"], paused["rows"]);

    // Another session replaces the reader.
    runtime.block_on(async {
        let receipts = artifactize_core::store::Receipts::open(&fixture.state, &fixture.repo)
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
fn the_review_search_lists_any_request_and_opens_a_read_only_detail() {
    let fixture = fixture();
    let window = window(&fixture, Mode::Review);
    let search = |query: &str| {
        invoke(
            &window,
            "review_search",
            json!({"query": query, "start": 0, "reload": true}),
        )
        .unwrap()
    };
    // The default lists the waiting Human sign-off only.
    let waiting = search(artifactize_core::review::search::DEFAULT);
    assert_eq!(waiting["waiting"], json!(true));
    let evals: Vec<_> = waiting["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["eval"].clone())
        .collect();
    assert_eq!(evals, [json!("release/signoff")]);
    // Any kind and status: the dependency eval that waits for the sign-off.
    let dependency = search("kind:dependency");
    assert_eq!(dependency["waiting"], json!(false));
    assert_eq!(dependency["rows"][0]["eval"], "ship/check");
    assert_eq!(dependency["rows"][0]["kind"], "dependency");
    let id = dependency["rows"][0]["id"].as_str().unwrap().to_owned();
    let detail = invoke(&window, "review_detail", json!({"id": id})).unwrap();
    assert_eq!(detail["profile"], "dependency");
    assert_eq!(detail["sections"][0]["name"], "outcome");
    // An invalid query lists nothing and says why.
    let invalid = search("kind:robot");
    assert_eq!(invalid["rows"], json!([]));
    assert!(invalid["error"].as_str().unwrap().contains("Unknown kind"));
    // Completion offers values from the requests the terms before the last one find.
    let completion = invoke(
        &window,
        "review_complete",
        json!({"query": "kind:dependency eval:"}),
    )
    .unwrap();
    assert_eq!(completion["candidates"], json!(["eval:ship/check"]));
    let completion = invoke(&window, "review_complete", json!({"query": "kind:hu"})).unwrap();
    assert_eq!(completion["candidates"], json!(["kind:human"]));
}

#[test]
fn an_agent_request_found_by_the_search_opens_read_only_with_its_session() {
    let fixture = fixture();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let id = runtime.block_on(agent_request(&fixture, "fixture-session", 3));
    let window = window(&fixture, Mode::Review);
    let found = invoke(
        &window,
        "review_search",
        json!({"query": "kind:agent", "start": 0, "reload": true}),
    )
    .unwrap();
    assert_eq!(found["rows"][0]["id"], id);
    let detail = invoke(&window, "review_detail", json!({"id": id})).unwrap();
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
fn the_run_graph_lights_artifacts_and_edges_and_follows_a_submission() {
    let fixture = fixture();
    let monitor = window(&fixture, Mode::Monitor);
    let view = invoke(&monitor, "browse", json!({"navigate": STILL()})).unwrap();
    let run = view["selectedRun"].as_str().unwrap().to_owned();
    let graph = |window: &WebviewWindow<MockRuntime>| {
        invoke(window, "run_graph", json!({"run": run})).unwrap()
    };
    // release waits for its Human; ship's dependency eval waits for release and lights the
    // edge itself (waiting), not with release's own light (active).
    let before = graph(&monitor);
    assert_eq!(
        before["nodes"],
        json!([
            {"id": "a:release", "name": "release", "light": "active"},
            {"id": "a:ship", "name": "ship", "light": "waiting"},
        ])
    );
    assert_eq!(
        before["edges"],
        json!([{"source": "a:release", "target": "a:ship", "light": "waiting", "dependency": true}])
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
    invoke(&monitor, "browse", json!({"navigate": STILL()})).unwrap();
    let after = graph(&monitor);
    assert_eq!(after["nodes"][0]["light"], "green");
    assert_eq!(
        after["nodes"][1]["light"], "green",
        "the derived eval follows"
    );
    assert_eq!(after["edges"][0]["light"], "green");
    // Another Run than the shown one is refused.
    let missing = invoke(&monitor, "run_graph", json!({"run": "run-gone"}));
    assert_eq!(missing.unwrap_err()["kind"], "missing");
}

#[test]
fn the_run_graph_lights_an_error_and_blocks_what_follows_it() {
    // broken's command cannot start: ERROR, not a verdict. after refers to broken.
    let broken = r#"name = "broken"

[evals.check]
title = "A command that is not installed"
profile = { kind = "runtime", command = "artifactize-test-command-that-does-not-exist", args = [] }

[evals.check.payload]
instruction = """
Run the check."""
"#;
    let after = r#"name = "after"

[evals.check]
title = "After broken"
profile = { kind = "runtime", command = "true", args = [] }

[evals.check.payload]
instruction = """
Check {broken}."""
"#;
    let fixture = fixture_with(&[("broken", broken), ("after", after)]);
    let monitor = window(&fixture, Mode::Monitor);
    let view = invoke(&monitor, "browse", json!({"navigate": STILL()})).unwrap();
    let run = view["selectedRun"].as_str().unwrap().to_owned();
    let graph = invoke(&monitor, "run_graph", json!({"run": run})).unwrap();
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
        &json!({"source": "a:broken", "target": "a:after", "light": "error", "dependency": false})
    );
}

#[test]
fn graph_view_projects_the_same_structure_as_the_fingerprint_and_agent_tool() {
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
        _root: root,
        repo,
        state,
    };
    let viewer = window(&fixture, Mode::Graph);
    let structure = artifactize_core::graph_structure::read(&fixture.repo).unwrap();
    let view = invoke(&viewer, "current_graph", json!({})).unwrap();
    assert_eq!(view["fingerprint"], structure.fingerprint().as_str());
    assert_eq!(view["structure"], structure.tree());
    assert_eq!(
        view["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["app", "graph"]
    );
    assert_eq!(
        view["graph"]["edges"],
        json!([{"source":"a:app", "target":"a:graph", "light":"waiting", "dependency":false}])
    );
    assert_eq!(view["tree"][1]["children"][0]["name"], "Review the graph");
    // There was never a verification or saved Run. Refresh reads the changed declaration.
    fs::write(
        fixture.repo.join("index.artf"),
        "name = 'app'\nbasis = true\n",
    )
    .unwrap();
    let refreshed = invoke(&viewer, "current_graph", json!({})).unwrap();
    assert_ne!(refreshed["fingerprint"], view["fingerprint"]);
    assert_eq!(
        refreshed["structure"],
        artifactize_core::graph_structure::read(&fixture.repo)
            .unwrap()
            .tree()
    );
}
