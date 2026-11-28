use super::*;

fn test_direct_target() -> ChatTarget {
    ChatTarget::Direct {
        backend: DirectBackend::Builtin {
            provider: HttpProvider::Codex,
        },
        model: "fixture-model".parse().unwrap(),
        effort: None,
    }
}

fn test_session(
    id: &str,
    title: &str,
    worktree: PathBuf,
    repository: Option<repository::Identity>,
    target: Option<ChatTarget>,
    view: SessionView,
    timestamp: &str,
) -> Session {
    let legacy_migration_notice = target
        .as_ref()
        .is_some_and(ChatTarget::is_legacy_claude_agent);
    let stamp: crate::types::Timestamp = timestamp.parse().unwrap();
    Session {
        id: id.parse().unwrap(),
        title: title.into(),
        worktree,
        repository,
        target,
        view,
        created_at: stamp,
        updated_at: stamp,
        activity: None,
        moved_from: None,
        acp_binding: None,
        running: false,
        waiting_permission: false,
        live_config_options: Vec::new(),
        verify_bridge_state: VerifyBridgeState::NotConnected,
        legacy_migration_notice,
        unavailable: None,
    }
}

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
    let make = |id: &str, path: PathBuf, repository, stamp: &str| {
        test_session(
            id,
            id,
            path,
            Some(repository),
            Some(test_direct_target()),
            SessionView::Graph,
            stamp,
        )
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
        let directory = crate::platform::canonicalize(directory).unwrap();
        let session = test_session(
            id,
            title,
            directory.clone(),
            Some(repository::Identity {
                common_dir: Some(directory.join(".git")),
                worktree_path: Some(directory),
                branch: Some(branch.into()),
            }),
            Some(test_direct_target()),
            SessionView::Conversation,
            time,
        );
        receipts.create_chat(&session).await.unwrap();
    }
    let checkout = crate::test_os::canonical(&worktree);
    crate::test_os::git(
        &checkout,
        &["init", "--quiet", "--initial-branch=feature/Billing"],
    );
    let newer = test_session(
        "chat-newest-on-branch",
        "The latest session",
        checkout.clone(),
        Some(repository::identify(&checkout)),
        Some(test_direct_target()),
        SessionView::Conversation,
        "2026-10-25T12:00:00Z",
    );
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
fn a_turn_hands_off_from_the_engine_its_last_send_ran_on_whatever_target_is_saved() {
    let root = crate::test_os::tempdir();
    let file = root.path().join("chat-sessions").join("handoff.jsonl");
    let id = "chat-handoff".parse().unwrap();
    let mut recorder = Recorder::chat(root.path(), &file, &id, 1).unwrap();
    recorder.event(Kind::Send(Send {
        text: Some("First".into()),
        engine: Some("Direct (codex / small)".into()),
        ..Send::default()
    }));
    recorder.event(Kind::Answer(Answer::Completed("done".into())));
    drop(recorder);
    let conversation = Conversation::load(&file).unwrap();
    let saved = || Some("from the saved target".to_owned());
    // The picker changed the engine: the handoff names the one the last turn ran on.
    assert_eq!(
        handoff_from(conversation.as_ref(), "ACP (antigravity)", saved),
        Some("Direct (codex / small)".into())
    );
    // The same engine again hands off from nothing, whatever the saved target says.
    assert_eq!(
        handoff_from(conversation.as_ref(), "Direct (codex / small)", saved),
        None
    );
    // Before any turn, or for sends that predate the record, the saved target decides.
    assert_eq!(handoff_from(None, "ACP (antigravity)", saved), saved());
    let older = root.path().join("chat-sessions").join("older.jsonl");
    let mut recorder = Recorder::chat(root.path(), &older, &id, 1).unwrap();
    recorder.event(Kind::Send(Send {
        text: Some("First".into()),
        ..Send::default()
    }));
    drop(recorder);
    let older = Conversation::load(&older).unwrap();
    assert_eq!(
        handoff_from(older.as_ref(), "ACP (antigravity)", saved),
        saved()
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
        ..Send::default()
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
    let turns = transcript::turns(saved.as_ref(), false, &[]);
    assert!(matches!(&turns[0].blocks[0], Block::Tool {
        result: Some(transcript::ToolResult { exit_code: Some(0), omitted_lines: 0, output_truncated: false, output, .. }), summary, ..
    } if summary == "ran fixture (exit 0)" && output == "one\ntwo\n"));
}

#[test]
fn legacy_session_wire_migrates_codex_antigravity_and_retired_claude_agent() {
    let codex_record = serde_json::json!({
        "id": "chat-legacy-codex", "title": "Older chat", "worktree": "workspace",
        "selection": {"backend": "codex", "model": "saved-model", "effort": "high"},
        "createdAt": "2026-11-21T00:00:00Z", "updatedAt": "2026-11-21T00:00:00Z",
    });
    let session: Session = serde_json::from_value(codex_record).unwrap();
    assert!(!session.legacy_migration_notice);
    assert_eq!(
        session.target,
        Some(ChatTarget::Direct {
            backend: DirectBackend::Builtin {
                provider: HttpProvider::Codex
            },
            model: "saved-model".parse().unwrap(),
            effort: Some("high".into()),
        })
    );

    let agy_record = serde_json::json!({
        "id": "chat-legacy-agy", "title": "AGY chat", "worktree": "workspace",
        "model": {"backend": "antigravity", "model": "gemini-3-pro", "effort": "medium"},
        "createdAt": "2026-11-21T00:00:00Z", "updatedAt": "2026-11-21T00:00:00Z",
    });
    let session: Session = serde_json::from_value(agy_record).unwrap();
    assert!(session.legacy_migration_notice);
    assert!(matches!(
        session.target,
        Some(ChatTarget::Acp { ref agent, ref config })
        if agent.as_str() == "antigravity"
            && config.len() == 2
            && config[0].id == "model"
            && config[0].value == "gemini-3-pro"
            && config[1].id == "thought_level"
            && config[1].value == "medium"
    ));

    let claude_record = serde_json::json!({
        "id": "chat-legacy-claude", "title": "Claude chat", "worktree": "workspace",
        "model": {"backend": "claude-agent", "model": "sonnet", "effort": "high"},
        "createdAt": "2026-11-21T00:00:00Z", "updatedAt": "2026-11-21T00:00:00Z",
    });
    let session: Session = serde_json::from_value(claude_record).unwrap();
    assert!(session.legacy_migration_notice);
    assert_eq!(
        session.target,
        Some(ChatTarget::LegacyClaudeAgent {
            model: "sonnet".parse().unwrap(),
            effort: Some("high".into()),
        })
    );
}

#[tokio::test]
async fn permission_cas_enforces_stop_precedence_and_validates_options() {
    use crate::acp::{PermissionOption, PermissionOptionKind, PermissionStatus};
    use crate::store::chat::{NewPermission, Owner, PermissionDelivery};

    let root = crate::test_os::tempdir();
    let worktree = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir(&worktree).unwrap();
    let worktree = crate::test_os::canonical(&worktree);
    let session = test_session(
        "chat-perm",
        "Permission CAS",
        worktree.clone(),
        Some(repository::identify(&worktree)),
        Some(ChatTarget::Acp {
            agent: "antigravity".parse().unwrap(),
            config: Vec::new(),
        }),
        SessionView::Conversation,
        "2026-11-21T00:00:00Z",
    );
    let receipts = store::OwnedReceipts::open(&state, &worktree).await.unwrap();
    receipts.create_chat(&session).await.unwrap();
    let owner = Owner::current().unwrap();
    receipts
        .claim_chat(&state, &session.id, &owner)
        .await
        .unwrap();

    let perm = NewPermission {
        session_id: session.id.clone(),
        owner_token: owner.token.clone(),
        permission_id: "perm-1".into(),
        rpc_request_id: "42".into(),
        title: "Run tests".into(),
        tool_kind: ToolKind::Command,
        summary: "cargo test".into(),
        detail: None,
        options: vec![
            PermissionOption {
                option_id: "allow_once".into(),
                name: "Allow once".into(),
                kind: PermissionOptionKind::AllowOnce,
                persists_in_agent_settings: false,
            },
            PermissionOption {
                option_id: "reject_once".into(),
                name: "Deny".into(),
                kind: PermissionOptionKind::RejectOnce,
                persists_in_agent_settings: false,
            },
        ],
        created_at: "2026-11-21T00:00:01Z".parse().unwrap(),
    };
    assert!(receipts.insert_chat_permission(perm).await.unwrap());

    // Invalid option_id is rejected before mutating state.
    assert!(
        receipts
            .respond_chat_permission(&session.id, "perm-1", "bogus")
            .await
            .is_err()
    );
    // Nothing is written to the agent for a choice nobody made yet.
    assert_eq!(
        receipts
            .deliver_chat_permission(&session.id, &owner.token, "perm-1", |_| async {
                panic!("nothing is written before a choice")
            })
            .await
            .unwrap(),
        PermissionDelivery::StillPending
    );

    // Valid selection transitions PENDING -> SELECTED, then Stop cancels SELECTED before delivery.
    assert!(
        receipts
            .respond_chat_permission(&session.id, "perm-1", "allow_once")
            .await
            .unwrap()
    );
    assert!(
        receipts
            .stop_chat_by_token(&session.id, &owner.token)
            .await
            .unwrap()
    );
    assert_eq!(
        receipts
            .deliver_chat_permission(&session.id, &owner.token, "perm-1", |_| async {
                panic!("nothing is written after a Stop")
            })
            .await
            .unwrap(),
        PermissionDelivery::Cancelled
    );
    let rows = store::chat::list_permissions(&state, &session.id)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, PermissionStatus::Cancelled);
}
