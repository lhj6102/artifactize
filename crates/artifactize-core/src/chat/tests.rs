use super::*;

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
            },
            created_at: time,
            updated_at: time,
            activity: None,
            running: false,
            unavailable: None,
        };
        receipts.create_chat(&session).await.unwrap();
    }
    drop(receipts);

    let chat = Chat::discover(state);
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
}
