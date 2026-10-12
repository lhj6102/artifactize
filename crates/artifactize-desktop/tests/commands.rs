//! The command boundary: JSON in and out of the IPC handler, on state a real `verify` made.
//! A Human sign-off with a built-in tool and owner schemas waits; a dependency eval waits for
//! it. Built-in tools and dependency evals run alike on every OS.
use artifactize::project::{VerifyOptions, selection::Selection};
use artifactize_desktop::{Launch, Mode};
use serde_json::{Value, json};
use std::{fs, time::Duration};
use tauri::{
    WebviewWindow,
    ipc::{CallbackFn, InvokeBody},
    test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets},
    webview::InvokeRequest,
};

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
    let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    for (name, text) in [("release", RELEASE), ("ship", SHIP)] {
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let options = VerifyOptions {
            wait_timeout: Duration::from_millis(1),
            ..VerifyOptions::default()
        };
        // The wait ends at once; the Human request stays waiting.
        artifactize::project::verify(
            &repo,
            Some(&state),
            &Selection::All,
            &options,
            Default::default(),
        )
        .await
        .unwrap();
    });
    Fixture {
        _root: root,
        repo,
        state,
    }
}

fn window(fixture: &Fixture, mode: Mode) -> WebviewWindow<MockRuntime> {
    let launch = Launch {
        mode,
        state: fixture.state.clone(),
        repo: Some(fixture.repo.clone()),
        reviewer: "alice".parse().unwrap(),
        request: None,
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

fn saved(fixture: &Fixture, eval: &str) -> artifactize::store::RequestView {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let runs = artifactize::store::read_runs(&fixture.state, None, 1, 0)
            .await
            .unwrap();
        artifactize::store::read_run_requests(&fixture.state, &runs[0].id)
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
        "human_waiting",
        json!({"all": false, "reload": true}),
    )
    .unwrap();
    assert_eq!(waiting[0]["id"], id);
    assert_eq!(waiting[0]["claim"], "-");

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
    .unwrap();
    assert_eq!(refused["notice"]["error"], json!(true));
    assert!(saved(&fixture, "release/signoff").claim.is_none());

    let claimed = invoke(
        &window,
        "human_claim",
        json!({"id": id, "job": 2, "standalone": true}),
    )
    .unwrap();
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
        artifactize::types::RequestStatus::WaitingHuman
    );

    let green = invoke(&window, "human_form", json!({"id": id, "verdict": "GREEN"})).unwrap();
    assert_eq!(
        green["fields"][0]["input"],
        json!({"kind": "fixed", "value": "true"})
    );
    let submitted = invoke(
        &window,
        "human_submit",
        json!({"id": id, "job": 5, "draft": {"verdict": "GREEN", "fields": [["approved", {"kind": "fixed"}], ["comment", {"kind": "text", "value": "확인했습니다"}]], "json": null}}),
    )
    .unwrap();
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
        "human_waiting",
        json!({"all": false, "reload": true}),
    )
    .unwrap();
    assert_eq!(waiting, json!([]));
}
