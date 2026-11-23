use super::*;

#[tokio::test]
async fn stopping_for_quit_cancels_local_work_even_without_a_saved_owner() {
    let root = crate::test_os::tempdir();
    let chat = Chat::discover(root.path().join("no-state"));
    let id = "chat-preparing".parse().unwrap();
    let active = chat.claim(&id).unwrap();
    assert!(!active.cancel.is_cancelled());
    chat.stop_running().await.unwrap();
    assert!(active.cancel.is_cancelled());
}

#[tokio::test]
async fn moving_a_branch_prefers_a_usable_session_but_keeps_stranded_history_readable() {
    let root = crate::test_os::tempdir();
    let worktree = root.path().join("current");
    let state = root.path().join("state");
    std::fs::create_dir(&worktree).unwrap();
    let worktree = crate::test_os::canonical(&worktree);
    crate::test_os::git(&worktree, &["init", "--quiet", "--initial-branch=main"]);
    let identity = repository::identify(&worktree);
    let make = |id: &str, path: PathBuf, repository, stamp: &str| Session {
        id: id.parse().unwrap(),
        title: id.into(),
        worktree: path,
        repository: Some(repository),
        selection: Model {
            backend: "codex".parse().unwrap(),
            model: "fixture-model".parse().unwrap(),
            effort: None,
        },
        view: SessionView::Graph,
        created_at: stamp.parse().unwrap(),
        updated_at: stamp.parse().unwrap(),
        activity: None,
        running: false,
        unavailable: None,
    };
    let usable = make(
        "chat-live",
        worktree.clone(),
        identity.clone(),
        "2026-11-01T10:00:00Z",
    );
    let old_path = worktree.with_file_name("moved-away");
    let stranded = make(
        "chat-stranded",
        old_path.clone(),
        repository::Identity {
            worktree_path: Some(old_path),
            ..identity
        },
        "2026-11-02T10:00:00Z",
    );
    let branch = usable.branch();
    let receipts = store::OwnedReceipts::open(&state, &worktree).await.unwrap();
    receipts.create_chat(&usable).await.unwrap();
    receipts.create_chat(&stranded).await.unwrap();
    drop(receipts);
    let chat = Chat::discover(state);
    let preferred = chat.latest_for_navigation(&branch).await.unwrap().unwrap();
    assert_eq!(preferred.id, usable.id);
    assert!(preferred.unavailable.is_none());
    let nested = worktree.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let session = make(
        "chat-nested",
        nested.clone(),
        repository::identify(&nested),
        "2026-11-01T11:00:00Z",
    );
    let state = root.path().join("state");
    let writer = store::OwnedReceipts::open(&state, &worktree).await.unwrap();
    writer.create_chat(&session).await.unwrap();
    drop(writer);
    assert_eq!(
        chat.latest_for_navigation(&branch)
            .await
            .unwrap()
            .unwrap()
            .id,
        session.id
    );
    assert_eq!(
        chat.current_worktree(&branch).await.unwrap(),
        Some(worktree.clone())
    );
    crate::test_os::git(&worktree, &["symbolic-ref", "HEAD", "refs/heads/other"]);
    let old_nested = stranded.worktree.join("former-subdirectory");
    assert_eq!(
        chat.latest_for_unavailable_checkout(&old_nested)
            .await
            .unwrap()
            .unwrap()
            .id,
        stranded.id
    );
    let fallback = chat.latest_for_navigation(&branch).await.unwrap().unwrap();
    assert_eq!(fallback.id, stranded.id);
    assert_eq!(
        fallback.unavailable.as_deref(),
        Some("This Chat's worktree no longer exists.")
    );
    assert_eq!(
        chat.open(&fallback.id).await.unwrap().session.title,
        stranded.title
    );
    assert!(
        chat.latest_for_navigation(&Place::Folder(root.path().join("never-used")))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn catalog_search_matches_all_words_across_labels_without_reordering() {
    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    let worktree = root.path().join("Acme");
    let other = root.path().join("AcmeTools");
    for directory in [&worktree, &other] {
        std::fs::create_dir(directory).unwrap();
    }
    let receipts = store::OwnedReceipts::open(&state, &worktree).await.unwrap();
    // Write fixture rows in a different order from their fixed activity timestamps.
    let fixtures = [
        (
            "chat-new",
            &worktree,
            "feature/Billing",
            "Fix checkout tax",
            "2026-10-23T12:00:00Z",
        ),
        (
            "chat-unrelated",
            &worktree,
            "feature/Billing",
            "Fix reports",
            "2026-10-24T12:00:00Z",
        ),
        (
            "chat-old",
            &other,
            "feature/Billing",
            "Check CHECKOUT logs",
            "2026-10-21T12:00:00Z",
        ),
        (
            "chat-branch",
            &worktree,
            "feature/Other",
            "Fix checkout tax",
            "2026-10-22T12:00:00Z",
        ),
    ];
    for (id, directory, branch, title, time) in fixtures {
        let time = time.parse().unwrap();
        let directory = crate::platform::canonicalize(directory).unwrap();
        let session = Session {
            id: id.parse().unwrap(),
            title: title.into(),
            repository: Some(repository::Identity {
                common_dir: Some(directory.join(".git")),
                worktree_path: Some(directory.clone()),
                branch: Some(branch.into()),
            }),
            worktree: directory,
            selection: Model {
                backend: "codex".parse().unwrap(),
                model: "fixture-model".parse().unwrap(),
                effort: None,
            },
            view: SessionView::Conversation,
            created_at: time,
            updated_at: time,
            activity: None,
            running: false,
            unavailable: None,
        };
        receipts.create_chat(&session).await.unwrap();
    }
    let saved_model = Model {
        backend: "codex".parse().unwrap(),
        model: "fixture-model".parse().unwrap(),
        effort: None,
    };
    let checkout = crate::test_os::canonical(&worktree);
    crate::test_os::git(
        &checkout,
        &["init", "--quiet", "--initial-branch=feature/Billing"],
    );
    let newer = Session {
        id: "chat-newest-on-branch".parse().unwrap(),
        title: "The latest session".into(),
        worktree: checkout.clone(),
        repository: Some(repository::identify(&checkout)),
        selection: saved_model,
        view: SessionView::Conversation,
        created_at: "2026-10-25T12:00:00Z".parse().unwrap(),
        updated_at: "2026-10-25T12:00:00Z".parse().unwrap(),
        activity: None,
        running: false,
        unavailable: None,
    };
    let branch = newer.branch();
    drop(receipts);

    let chat = Chat::discover(state.clone());
    let ids = |sessions: Vec<Session>| {
        sessions
            .into_iter()
            .map(|s| s.id.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(chat.list(None, None).await.unwrap()),
        ["chat-unrelated", "chat-new", "chat-branch", "chat-old"]
    );
    assert_eq!(
        ids(chat
            .list(None, Some("  CHECKOUT BiLLing\tACME "))
            .await
            .unwrap()),
        ["chat-new", "chat-old"]
    );
    assert_eq!(
        ids(chat
            .list(Some(&worktree), Some("checkout billing acme"))
            .await
            .unwrap()),
        ["chat-new"]
    );
    assert!(
        chat.list(None, Some("checkout billing acme missing"))
            .await
            .unwrap()
            .is_empty()
    );
    let receipts = store::OwnedReceipts::open(&state, &worktree).await.unwrap();
    receipts.create_chat(&newer).await.unwrap();
    drop(receipts);
    // Switching an old Chat to Graph is not a turn and does not reorder the branch.
    chat.set_view(&"chat-new".parse().unwrap(), SessionView::Graph)
        .await
        .unwrap();
    assert_eq!(
        chat.latest_for_branch(&branch).await.unwrap().unwrap().id,
        newer.id
    );
}

#[test]
fn a_saved_mcp_command_result_exposes_its_exit_and_decodes_its_output() {
    use rig_core::message::{
        AssistantContent, ToolCall, ToolFunction, ToolName, ToolResultContent,
    };
    use serde_json::json;

    let root = crate::test_os::tempdir();
    let file = root.path().join("chat-sessions").join("conversation.jsonl");
    let id = "chat-mcp-result".parse().unwrap();
    let mut recorder = Recorder::chat(root.path(), &file, &id, 1).unwrap();
    recorder.event(Kind::Send(Send {
        text: Some("Run the fixture".into()),
        framing: None,
        files_changed: None,
        tools: None,
    }));
    let call = ToolCall::from_wire(
        "call-command",
        ToolFunction::new(
            ToolName::new("run_command").unwrap(),
            json!({"program":"fixture"}),
        ),
    );
    recorder.message(
        1,
        &rig_core::message::Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call.clone())],
        },
        false,
    );
    // Claude Code reports MCP's JSON content as text, including its escaped line separators.
    let result = ToolResultContent::text(
        r#"{"exitCode":0,"stdout":"one\ntwo\n","stderr":"","truncated":false}"#,
    );
    recorder.tool_results(
        2,
        &rig_core::message::Message::tool_results(vec![call.result(vec![result])]),
        &[false],
    );
    recorder.event(Kind::Answer(Answer::Completed("Done".into())));
    recorder.check_saved().unwrap();
    drop(recorder);
    let saved = Conversation::load(&file).unwrap();
    let turns = transcript::turns(saved.as_ref(), false);
    assert!(matches!(&turns[0].blocks[0], Block::Tool {
        result: Some(transcript::ToolResult { exit_code: Some(0), omitted_lines: 0, output_truncated: false, output, .. }), summary, ..
    } if summary == "ran fixture (exit 0)" && output == "one\ntwo\n"));
}
