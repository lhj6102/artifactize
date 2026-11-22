//! Chat drives the same Verify and tool protocol as the app through a scripted provider.
#[path = "support/run.rs"]
mod owned_run;
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

use std::{
    collections::BTreeSet,
    fs,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use artifactize_core::{
    changes::Subscription,
    chat::{Block, Chat, Model, Outcome, ToolKind, TurnState, View},
    runs, store,
    types::{RunId, SessionId},
};
use serde_json::json;
use support::{FakeProvider, Gate, Reply, openai};

async fn observe(
    chat: &Chat,
    id: &SessionId,
    changes: &mut Subscription,
    ready: impl Fn(&View) -> bool,
) -> View {
    tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
        loop {
            let view = chat.open(id).await.unwrap();
            if ready(&view) {
                return view;
            }
            changes.next_reconciled().await;
        }
    })
    .await
    .expect("the observed Chat condition must be reached")
}

fn cards(view: &View) -> Vec<RunId> {
    view.turns
        .iter()
        .flat_map(|turn| &turn.blocks)
        .filter_map(|block| match block {
            Block::Run { card } => Some(card.id.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn custom_provider_streams_reads_edits_and_verifies_then_stop_keeps_the_session() {
    let root = owned_run::Root::from(support::os::tempdir());
    let worktree = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&worktree).unwrap();
    fs::write(worktree.join("notes.txt"), "before").unwrap();
    fs::write(worktree.join("Case.txt"), "keep this file").unwrap();
    let lines = (1..=160)
        .map(|line| format!("output {line:03}\n"))
        .collect::<String>();
    fs::write(worktree.join("lines.txt"), lines).unwrap();
    let long = format!("begin {} end", "한".repeat(7_000));
    let noisy = support::os::exiting_program(root.path(), 7, &long);

    support::declaration::write(&worktree, worktree.join("index.artf"), json!({
        "name":"app", "fingerprint":false, "evals":[{
            "id":"test", "title":"Check", "profile":{"kind":"runtime","command":support::os::true_program(),"args":[]},
            "payload":{"instruction":"Check the fixture."}
        }]
    }).to_string()).unwrap();
    support::os::git(&worktree, &["init", "-q", "--initial-branch=first"]);
    support::os::git(
        &worktree,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Fixture",
        ],
    );
    root.track(&state, &worktree);
    let final_answer = Gate::default();
    let interrupted = Gate::default();
    final_answer.close();
    interrupted.close();
    let stop_ready = Arc::new(tokio::sync::Notify::new());
    let provider = FakeProvider::start({
        let final_answer = final_answer.clone();
        let interrupted = interrupted.clone();
        let stop_ready = stop_ready.clone();
        let call = AtomicUsize::new(0);
        move |request| {
            if request.path.ends_with("/models") {
                return openai::models(&["scripted-model", "second-model"]);
            }
            let output = match call.fetch_add(1, Ordering::Relaxed) {
                0 => vec![
                    openai::message("Reading the notes."),
                    openai::function_call("call_read", "read", &json!({"path":"notes.txt"})),
                ],
                1 => vec![openai::function_call(
                    "call_edit",
                    "edit",
                    &json!({"path":"notes.txt","old_text":"before","new_text":"after"}),
                )],
                2 => vec![
                    openai::function_call(
                        "call_command",
                        "run_command",
                        &json!({"program":support::os::cat_program(),"args":["notes.txt"]}),
                    ),
                    openai::function_call(
                        "call_lines",
                        "run_command",
                        &json!({"program":support::os::cat_program(),"args":["lines.txt"]}),
                    ),
                    openai::function_call("call_bytes", "run_command", &json!({"program":noisy})),
                    openai::function_call(
                        "call_alias",
                        "write",
                        &json!({"path":"case.txt","text":"must not overwrite"}),
                    ),
                    openai::function_call(
                        "call_verify",
                        "verify_start",
                        &json!({"scope":{"kind":"all"}}),
                    ),
                ],
                3 => {
                    final_answer.wait();
                    vec![openai::message("The changes were sent to Verify.")]
                }
                4 => {
                    stop_ready.notify_one();
                    interrupted.wait();
                    vec![openai::message("This answer was stopped.")]
                }
                5 => vec![openai::message("Ready to continue.")],
                _ => {
                    return Reply::Json(
                        500,
                        json!({"error":{"message":"unexpected provider call"}}),
                    );
                }
            };
            openai::completed(request, output, openai::usage(12, 8))
        }
    });
    let cli = support::os::cli();
    let added = support::os::run(
        Command::new(&cli)
            .arg("--repo")
            .arg(&worktree)
            .arg("--state-dir")
            .arg(&state)
            .args([
                "provider",
                "add",
                "scripted",
                "--api",
                "openai-responses",
                "--url",
                &provider.openai_base(),
            ]),
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );

    let selection = Model {
        backend: "scripted".parse().unwrap(),
        model: "scripted-model".parse().unwrap(),
        effort: None,
    };
    let chat = Chat::new(state.clone(), cli.clone());
    let wrong = Model {
        model: "not-listed".parse().unwrap(),
        ..selection.clone()
    };
    assert!(
        chat.create(&worktree, Some(wrong))
            .await
            .unwrap_err()
            .contains("does not list")
    );
    let session = chat
        .create(&worktree, Some(selection.clone()))
        .await
        .unwrap();
    assert!(chat.open(&session.id).await.unwrap().turns.is_empty());
    assert_eq!(session.outcome(), None);
    assert!(!session.unseen());
    let mut changes = Subscription::new(&state).await;
    chat.send(&session.id, "Edit and verify the notes".into(), None)
        .await
        .unwrap();
    let mid = observe(&chat, &session.id, &mut changes, |view| {
        view.running && cards(view).len() == 1
    })
    .await;
    let reader = Chat::new(state.clone(), cli.clone());
    let entries = reader.list(Some(&worktree), None).await.unwrap();
    assert_eq!(entries[0].id, session.id);
    assert_eq!(entries[0].outcome(), Some(Outcome::Running));
    assert!(!entries[0].unseen());
    reader.mark_seen(&session.id).await.unwrap();
    assert!(
        entries[0].running,
        "a separately opened catalog sees the live owner"
    );
    assert!(
        mid.turns[0]
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Text { text } if text == "Reading the notes."))
    );
    assert!(mid.turns[0].blocks.iter().any(|block| matches!(block, Block::Tool { tool_kind: ToolKind::Read, summary, output: Some(output), failed: Some(false), .. } if summary == "read notes.txt" && output.contains("before"))));
    assert_eq!(
        fs::read_to_string(worktree.join("notes.txt")).unwrap(),
        "after"
    );
    assert!(mid.turns[0].blocks.iter().any(|block| matches!(block, Block::Tool { name, output: Some(output), failed: Some(false), .. } if name == "run_command" && output.contains("after"))));
    let lines = mid.turns[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Tool {
                tool_kind: ToolKind::Command,
                summary,
                exit_code: Some(0),
                output: Some(output),
                omitted_lines,
                output_truncated: true,
                ..
            } if summary.ends_with("lines.txt (exit 0)") => Some((output, *omitted_lines)),
            _ => None,
        })
        .expect("the command discloses its bounded line excerpt");
    assert!(lines.0.starts_with("output 001\n"));
    assert!(lines.0.trim_end().ends_with("output 160"));
    assert!(!lines.0.contains("output 080"));
    assert!(lines.0.lines().count() <= 80);
    assert_eq!(
        lines.1,
        160 - lines
            .0
            .lines()
            .filter(|line| line.starts_with("output "))
            .count()
    );
    let noisy = mid.turns[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Tool {
                tool_kind: ToolKind::Command,
                summary,
                exit_code: Some(7),
                output: Some(output),
                omitted_lines: 0,
                output_truncated: true,
                failed: Some(true),
                ..
            } if summary.ends_with(" (exit 7)") => Some(output),
            _ => None,
        })
        .expect("the failing command reports its exit and clips a long UTF-8 line");
    assert!(noisy.contains("begin 한"));
    assert!(noisy.trim_end().ends_with("한 end"));
    assert!(noisy.len() <= 8 * 1024);
    assert!(
        mid.turns[0]
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Tool {
        tool_kind: ToolKind::Edit,
        summary,
        failed: Some(true),
        ..
    } if summary == "write case.txt"))
    );
    assert_eq!(
        fs::read_to_string(worktree.join("Case.txt")).unwrap(),
        "keep this file"
    );
    assert!(
        !fs::read_dir(&worktree)
            .unwrap()
            .any(|entry| entry.unwrap().file_name() == "case.txt")
    );
    let run = cards(&mid).remove(0);
    let run_view = store::read_run(&state, run.as_str()).await.unwrap();
    assert_eq!(
        run_view.run.origin,
        Some(runs::Origin::Chat {
            session_id: session.id.clone(),
            title: "Edit and verify the notes".into()
        })
    );
    let next_model = Model {
        model: "second-model".parse().unwrap(),
        ..selection.clone()
    };
    let changed_while_streaming = reader
        .set_model(&session.id, next_model.clone())
        .await
        .unwrap();
    assert!(changed_while_streaming.running);
    assert_eq!(changed_while_streaming.selection, next_model);
    final_answer.open();
    let ended = observe(&chat, &session.id, &mut changes, |view| !view.running).await;
    assert!(matches!(ended.turns[0].state, TurnState::Completed));
    assert_eq!(ended.session.outcome(), Some(Outcome::Completed));
    assert!(ended.session.unseen());

    // New handle and connection: no in-memory message map participates in the restored view.
    let restarted = Chat::new(state.clone(), cli.clone());
    let restored = restarted.open(&session.id).await.unwrap();
    assert_eq!(cards(&restored), [run]);
    assert!(restored.session.unseen());
    restarted.mark_seen(&session.id).await.unwrap();
    let restarted = Chat::new(state.clone(), cli);
    let seen = restarted.open(&session.id).await.unwrap().session;
    assert_eq!(seen.outcome(), Some(Outcome::Completed));
    assert!(!seen.unseen());
    assert_eq!(seen.updated_at, ended.session.updated_at);
    let entries = restarted.list(Some(&worktree), None).await.unwrap();
    assert_eq!(entries[0].id, session.id);
    assert!(!entries[0].running);
    assert_eq!(
        restarted.default_model(&worktree).await.unwrap(),
        Some(next_model)
    );
    assert_eq!(restored.session.title, "Edit and verify the notes");
    restarted
        .send(&session.id, "Stop this answer".into(), None)
        .await
        .unwrap();
    tokio::time::timeout(
        support::os::patience(Duration::from_secs(10)),
        stop_ready.notified(),
    )
    .await
    .unwrap();
    assert!(restarted.stop(&session.id).await.unwrap());
    let stopped = observe(&restarted, &session.id, &mut changes, |view| {
        !view.running && view.turns.len() == 2
    })
    .await;
    assert!(matches!(stopped.turns[1].state, TurnState::Stopped));
    assert_eq!(stopped.session.outcome(), Some(Outcome::Stopped));
    assert!(stopped.session.unseen());
    interrupted.open();
    let changed = Model {
        model: "second-model".parse().unwrap(),
        ..selection
    };
    restarted
        .send(&session.id, "Continue".into(), Some(changed.clone()))
        .await
        .unwrap();
    let continued = observe(&restarted, &session.id, &mut changes, |view| {
        !view.running && view.turns.len() == 3
    })
    .await;
    assert_eq!(continued.session.selection, changed);
    assert!(matches!(continued.turns[2].state, TurnState::Completed));
    assert!(
        continued.turns[2]
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Text { text } if text == "Ready to continue."))
    );
    let seen = provider.requests();
    let request = seen
        .iter()
        .find(|req| req.path.ends_with("/responses"))
        .unwrap();
    let names = request.body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<BTreeSet<_>>();
    for name in [
        "read",
        "list",
        "glob",
        "grep",
        "edit",
        "write",
        "run_command",
        "verify_start",
        "verify_wait",
        "run_show",
        "request_show",
        "config_graph",
    ] {
        assert!(names.contains(name), "missing {name}: {names:?}");
    }
    support::os::git(&worktree, &["checkout", "-q", "-b", "second"]);
    let second = Model {
        model: "scripted-model".parse().unwrap(),
        ..changed.clone()
    };
    restarted
        .create(&worktree, Some(second.clone()))
        .await
        .unwrap();
    assert_eq!(
        restarted.default_model(&worktree).await.unwrap(),
        Some(second)
    );
    let previous = restarted.open(&session.id).await.unwrap();
    assert_eq!(previous.session.label().branch, "first");
    assert!(
        previous
            .session
            .unavailable
            .as_deref()
            .unwrap()
            .contains("first")
    );
    assert!(
        restarted
            .send(&session.id, "Must not run here".into(), None)
            .await
            .unwrap_err()
            .contains("Check out first")
    );
    assert_eq!(restarted.open(&session.id).await.unwrap().turns.len(), 3);
    support::os::git(&worktree, &["checkout", "-q", "first"]);
    assert_eq!(
        restarted.default_model(&worktree).await.unwrap(),
        Some(changed)
    );
    assert!(
        restarted
            .open(&session.id)
            .await
            .unwrap()
            .session
            .unavailable
            .is_none()
    );
    changes.close().await.unwrap();
}

/// Flatten only the tool definition envelopes that provider APIs use, not parameters.
fn offered(request: &support::Request) -> BTreeSet<&str> {
    let tools = request.body["tools"].as_array().unwrap();
    tools
        .iter()
        .flat_map(|tool| {
            if let Some(group) = tool["functionDeclarations"]
                .as_array()
                .or_else(|| tool["function_declarations"].as_array())
            {
                group
                    .iter()
                    .filter_map(|function| function["name"].as_str())
                    .collect::<Vec<_>>()
            } else {
                tool["name"]
                    .as_str()
                    .or_else(|| tool["function"]["name"].as_str())
                    .into_iter()
                    .collect()
            }
        })
        .collect()
}

fn worktree(root: &owned_run::Root) -> (std::path::PathBuf, std::path::PathBuf) {
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(
        &repo,
        repo.join("index.artf"),
        "name = 'app'\nbasis = true\n",
    )
    .unwrap();
    root.track(&state, &repo);
    (repo, state)
}

const FILE_AND_VERIFY_TOOLS: &[&str] = &[
    "read",
    "list",
    "glob",
    "grep",
    "write",
    "edit",
    "run_command",
    "verify_start",
    "verify_wait",
    "verify_status",
    "verify_result",
    "verify_cancel",
    "config_check",
    "config_graph",
    "request_show",
    "run_show",
    "session_show",
];

#[tokio::test]
async fn chat_completions_anthropic_and_gemini_get_the_worktree_and_verify_tools() {
    for (api, model, pointer) in [
        ("openai-chat", "o3-mini", "/reasoning_effort"),
        ("anthropic", "claude-opus-4-6", "/output_config/effort"),
        (
            "gemini",
            "gemini-3.1-pro-preview",
            "/generationConfig/thinkingConfig/thinkingLevel",
        ),
    ] {
        let provider = FakeProvider::start(move |request| match (api, request.method.as_str()) {
            ("openai-chat", "GET") => openai::models(&[model]),
            ("openai-chat", "POST") => support::chat::text(request, "Ready.", 4, 2),
            ("anthropic", "GET") => support::anthropic::models(&[model]),
            ("anthropic", "POST") => support::anthropic::text(request, "Ready.", 4, 2),
            ("gemini", "GET") => support::gemini::models(&[model]),
            ("gemini", "POST") => support::gemini::text(model, "Ready.", 4, 2),
            _ => unreachable!("fixture provider and HTTP method"),
        });
        let root = owned_run::Root::from(support::os::tempdir());
        let (repo, state) = worktree(&root);
        let cli = support::os::cli();
        let base = if api == "openai-chat" {
            provider.openai_base()
        } else {
            provider.url.clone()
        };
        let output = support::os::run(
            Command::new(&cli)
                .arg("--repo")
                .arg(&repo)
                .arg("--state-dir")
                .arg(&state)
                .args(["provider", "add", "scripted", "--api", api, "--url", &base]),
        );
        assert!(
            output.status.success(),
            "{api}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let chat = Chat::new(state.clone(), cli);
        let session = chat
            .create(
                &repo,
                Some(Model {
                    backend: "scripted".parse().unwrap(),
                    model: model.parse().unwrap(),
                    effort: Some("high".into()),
                }),
            )
            .await
            .unwrap();
        let mut changes = Subscription::new(&state).await;
        chat.send(&session.id, "What can you do?".into(), None)
            .await
            .unwrap();
        let view = observe(&chat, &session.id, &mut changes, |view| !view.running).await;
        assert!(
            matches!(&view.turns[0].state, TurnState::Completed),
            "{api}: {:?}",
            view.turns[0].state
        );
        let requests = provider.requests();
        let request = requests
            .iter()
            .find(|request| request.method == "POST")
            .unwrap();
        assert_eq!(
            request.body.pointer(pointer),
            Some(&json!("high")),
            "{api}: {}",
            request.body
        );
        let names = offered(request);
        for name in FILE_AND_VERIFY_TOOLS {
            assert!(names.contains(name), "{api} did not get {name}: {names:?}");
        }
        changes.close().await.unwrap();
    }
}

const CODEX_FIXTURE: &str = "ARTIFACTIZE_CHAT_CODEX_FIXTURE";

#[tokio::test]
async fn codex_chat_fixture() {
    if std::env::var_os(CODEX_FIXTURE).is_none() {
        return;
    }
    let root = owned_run::Root::from(support::os::tempdir());
    let (repo, state) = worktree(&root);
    let chat = Chat::new(state.clone(), support::os::cli());
    let model = Model {
        backend: "codex".parse().unwrap(),
        model: "fake-codex".parse().unwrap(),
        effort: Some("medium".into()),
    };
    assert_eq!(
        chat.default_model(&repo).await.unwrap(),
        Some(model.clone())
    );
    let session = chat.create(&repo, None).await.unwrap();
    assert_eq!(session.selection, model);
    let mut changes = Subscription::new(&state).await;
    let rejected = Model {
        effort: Some("bogus".into()),
        ..model.clone()
    };
    assert!(
        chat.send(&session.id, "Reject this effort".into(), Some(rejected))
            .await
            .unwrap_err()
            .contains("does not offer effort")
    );
    let selection = Model {
        effort: Some("high".into()),
        ..model
    };
    let saved = chat
        .set_model(&session.id, selection.clone())
        .await
        .unwrap();
    assert_eq!(saved.selection, selection);
    assert!(chat.open(&session.id).await.unwrap().turns.is_empty());
    assert_eq!(
        Chat::new(state.clone(), support::os::cli())
            .open(&session.id)
            .await
            .unwrap()
            .session
            .selection,
        selection
    );
    chat.send(&session.id, "What can you do?".into(), None)
        .await
        .unwrap();
    let view = observe(&chat, &session.id, &mut changes, |view| !view.running).await;
    assert!(
        matches!(&view.turns[0].state, TurnState::Completed),
        "{:?}",
        view.turns[0].state
    );
    assert_eq!(
        Chat::new(state, support::os::cli())
            .default_model(&repo)
            .await
            .unwrap(),
        Some(selection)
    );
    changes.close().await.unwrap();
}

#[test]
fn codex_account_transport_gets_the_worktree_and_verify_tools() {
    let root = support::os::tempdir();
    // Synthetic, read-only input owned by the fixture. No saved login is used.
    let fixture = root.path().join("codex-fixture.json");
    let token = support::codex::jwt("fixture-account", support::codex::VALID_EXPIRY);
    fs::write(&fixture, json!({"tokens":{"access_token":token,"refresh_token":"unused","account_id":"fixture-account"}}).to_string()).unwrap();
    let provider = FakeProvider::start(|request| {
        if request.method == "GET" {
            Reply::Json(
                200,
                json!({"models":[{"slug":"fake-codex","visibility":"list","supported_reasoning_levels":[{"effort":"medium"},{"effort":"high"}],"default_reasoning_level":"medium"}]}),
            )
        } else {
            support::codex::completed(
                request,
                vec![openai::message("Ready.")],
                openai::usage(4, 2),
            )
        }
    });
    let output = support::os::run(
        support::os::test_command("codex_chat_fixture", &[])
            .env(CODEX_FIXTURE, "1")
            .env("PATH", support::os::isolated_path(root.path()))
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("GEMINI_API_KEY")
            .env("ARTIFACTIZE_CODEX_AUTH_FILE", &fixture)
            .env("ARTIFACTIZE_CODEX_BASE_URL", provider.codex_base()),
    );
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = provider.requests();
    let request = requests
        .iter()
        .find(|request| request.method == "POST")
        .unwrap();
    assert_eq!(request.body["reasoning"]["effort"], "high");
    let names = offered(request);
    for name in FILE_AND_VERIFY_TOOLS {
        assert!(names.contains(name), "Codex did not get {name}: {names:?}");
    }
}

const CLAUDE_FIXTURE: &str = "ARTIFACTIZE_CHAT_CLAUDE_FIXTURE";

#[test]
fn claude_code_lists_the_models_efforts_and_gets_the_selection_as_an_argument() {
    let root = support::os::tempdir();
    let script = root.path().join("claude-chat.py");
    fs::write(&script, include_str!("fixtures/claude-chat.py")).unwrap();
    support::os::compiled_program(
        root.path(),
        "claude",
        &format!(
            r#"
fn main() -> std::process::ExitCode {{
    let code = std::process::Command::new({python:?}).args(["-I", "-X", "utf8", {script:?}])
        .args(std::env::args_os().skip(1)).status().unwrap();
    if code.success() {{ std::process::ExitCode::SUCCESS }} else {{ std::process::ExitCode::FAILURE }}
}}
"#,
            python = support::os::python_program(),
            script = script.to_str().unwrap()
        ),
    );
    let output = support::os::run_test(
        "claude_chat_fixture",
        &[
            (CLAUDE_FIXTURE, "1".into()),
            ("PATH", support::os::prepend_path(root.path())),
        ],
    );
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn claude_chat_fixture() {
    if std::env::var_os(CLAUDE_FIXTURE).is_none() {
        return;
    }
    let root = owned_run::Root::from(support::os::tempdir());
    let (repo, state) = worktree(&root);
    let backend = "claude-agent"
        .parse::<artifactize_core::config::Backend>()
        .unwrap();
    let listed = artifactize_core::llm::models::list(backend.clone(), Some(&state), Some(&repo))
        .await
        .unwrap();
    assert_eq!(listed.models[0].efforts, ["high", "low"]);
    assert_eq!(listed.models[0].default_effort.as_deref(), Some("high"));
    assert!(listed.models[1].efforts.is_empty());
    let chat = Chat::new(state.clone(), support::os::cli());
    let mut selection = Model {
        backend,
        model: "claude-fixture".parse().unwrap(),
        effort: Some("max".into()),
    };
    assert!(
        chat.create(&repo, Some(selection.clone()))
            .await
            .unwrap_err()
            .contains("does not offer effort")
    );
    selection.effort = Some("low".into());
    let session = chat.create(&repo, Some(selection)).await.unwrap();
    owned_run::subscribed(&state, async |changes| {
        chat.send(&session.id, "Ready?".into(), None).await.unwrap();
        let view = observe(&chat, &session.id, changes, |view| !view.running).await;
        assert!(
            matches!(&view.turns[0].state, TurnState::Completed),
            "{:?}",
            view.turns[0].state
        );
    })
    .await;
    chat.close().await;
}

const RESTART_STATE: &str = "ARTIFACTIZE_CHAT_RESTART_STATE";
const RESTART_SESSION: &str = "ARTIFACTIZE_CHAT_RESTART_SESSION";
const WAIT_FOR_STOP: &str = "ARTIFACTIZE_CHAT_WAIT_FOR_STOP";

#[tokio::test]
async fn interrupted_chat_owner_fixture() {
    let Some(state) = std::env::var_os(RESTART_STATE) else {
        return;
    };
    let id = std::env::var(RESTART_SESSION).unwrap().parse().unwrap();
    let state = std::path::PathBuf::from(state);
    let chat = Chat::new(state.clone(), support::os::cli());
    chat.send(&id, "This turn will lose its process.".into(), None)
        .await
        .unwrap();
    if std::env::var_os(WAIT_FOR_STOP).is_some() {
        let mut changes = Subscription::new(&state).await;
        let stopped = observe(&chat, &id, &mut changes, |view| !view.running).await;
        assert!(matches!(
            stopped.turns.last().unwrap().state,
            TurnState::Stopped
        ));
        changes.close().await.unwrap();
        chat.close().await;
    }
    // Without the flag, the runtime exits while the model is held by the parent fixture.
}

#[tokio::test]
async fn chat_can_resume_after_its_turn_owner_process_exits() {
    let root = owned_run::Root::from(support::os::tempdir());
    let (repo, state) = worktree(&root);
    let hold = Gate::default();
    hold.close();
    let closing = Gate::default();
    closing.close();
    let closing_request = Arc::new(tokio::sync::Notify::new());
    let provider = FakeProvider::start({
        let hold = hold.clone();
        let closing = closing.clone();
        let closing_request = closing_request.clone();
        move |request| {
            if request.method == "GET" {
                return openai::models(&["restart-model"]);
            }
            let text = if request.body.to_string().contains("Close a running turn") {
                closing_request.notify_one();
                closing.wait();
                "This did not stop."
            } else if request
                .body
                .to_string()
                .contains("Resume the interrupted session")
            {
                "Resumed."
            } else {
                hold.wait();
                "The departed process cannot save this."
            };
            openai::completed(request, vec![openai::message(text)], openai::usage(4, 2))
        }
    });
    let cli = support::os::cli();
    let output = support::os::run(
        Command::new(&cli)
            .arg("--repo")
            .arg(&repo)
            .arg("--state-dir")
            .arg(&state)
            .args([
                "provider",
                "add",
                "scripted",
                "--api",
                "openai-responses",
                "--url",
                &provider.openai_base(),
            ]),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let chat = Chat::new(state.clone(), cli.clone());
    let session = chat
        .create(
            &repo,
            Some(Model {
                backend: "scripted".parse().unwrap(),
                model: "restart-model".parse().unwrap(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "interrupted_chat_owner_fixture"])
        .env(RESTART_STATE, &state)
        .env(RESTART_SESSION, session.id.as_str())
        .kill_on_drop(true);
    let output = child.output().await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    hold.open();
    let mut death_changes = Subscription::new(&state).await;
    assert_eq!(
        chat.reconcile_interrupted().await.unwrap(),
        [session.id.clone()]
    );
    let notified = tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
        loop {
            if matches!(death_changes.next().await, artifactize_core::changes::Change::SessionInvalidated(id) if id == session.id) {
                break;
            }
        }
    }).await;
    death_changes.close().await.unwrap();
    assert!(
        notified.is_ok(),
        "live readers receive a session change when its owner dies"
    );
    assert!(chat.reconcile_interrupted().await.unwrap().is_empty());
    let interrupted = chat.open(&session.id).await.unwrap();
    assert!(!interrupted.running);
    assert!(matches!(interrupted.turns[0].state, TurnState::Interrupted));
    assert_eq!(interrupted.session.outcome(), Some(Outcome::Interrupted));
    assert!(interrupted.session.unseen());
    chat.mark_seen(&session.id).await.unwrap();
    let recovered = Chat::new(state.clone(), cli.clone())
        .open(&session.id)
        .await
        .unwrap();
    assert_eq!(recovered.session.outcome(), Some(Outcome::Interrupted));
    assert!(!recovered.session.unseen());
    let mut changes = Subscription::new(&state).await;
    chat.send(&session.id, "Resume the interrupted session".into(), None)
        .await
        .unwrap();
    let resumed = observe(&chat, &session.id, &mut changes, |view| {
        !view.running && view.turns.len() == 2
    })
    .await;
    assert!(matches!(resumed.turns[0].state, TurnState::Interrupted));
    assert!(matches!(resumed.turns[1].state, TurnState::Completed));
    // A separate app process owns the provider turn; this process may still stop it.
    hold.close();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "interrupted_chat_owner_fixture"])
        .env(RESTART_STATE, &state)
        .env(RESTART_SESSION, session.id.as_str())
        .env(WAIT_FOR_STOP, "1")
        .kill_on_drop(true);
    let child = child.spawn().unwrap();
    let active = observe(&chat, &session.id, &mut changes, |view| {
        view.running && view.turns.len() == 3
    })
    .await;
    assert!(matches!(active.turns[2].state, TurnState::Running));
    assert!(chat.stop(&session.id).await.unwrap());
    let stopped = observe(&chat, &session.id, &mut changes, |view| !view.running).await;
    hold.open();
    let ended = tokio::time::timeout(
        support::os::patience(Duration::from_secs(30)),
        child.wait_with_output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(ended.status.success());
    assert!(matches!(stopped.turns[2].state, TurnState::Stopped));
    assert!(!chat.stop(&session.id).await.unwrap());
    chat.send(&session.id, "Close a running turn".into(), None)
        .await
        .unwrap();
    tokio::time::timeout(
        support::os::patience(Duration::from_secs(30)),
        closing_request.notified(),
    )
    .await
    .expect("the provider received the turn that close must stop");
    tokio::time::timeout(support::os::patience(Duration::from_secs(30)), chat.close())
        .await
        .expect("close waits for the turn to record cancellation");
    assert!(
        chat.send(&session.id, "Not accepted after close".into(), None)
            .await
            .is_err()
    );
    let reopened = Chat::new(state.clone(), cli)
        .open(&session.id)
        .await
        .unwrap();
    assert!(!reopened.running);
    assert!(matches!(reopened.turns[2].state, TurnState::Stopped));
    closing.open();
    changes.close().await.unwrap();
}
