use super::*;
use serde_json::json;

fn entry() -> Entry {
    Entry {
        repository: "/work/artifactize".into(),
        eval: "release/signoff".into(),
        request: "run-a-2".into(),
        reviewer: Some("bob".into()),
        mine: false,
        kind: ProfileKind::Human,
        status: RequestStatus::WaitingHuman,
        tags: vec!["type:style".into()],
    }
}

fn matches(query: &str, entry: &Entry) -> bool {
    query.parse::<Query>().unwrap().matches(entry)
}

#[test]
fn free_text_matches_any_listed_column_case_insensitively() {
    let entry = entry();
    for text in ["ARTIFACTIZE", "signoff", "run-a", "Bob"] {
        assert!(matches(text, &entry), "{text}");
    }
    assert!(!matches("carol", &entry));
    // Tags, kinds and statuses are not listed columns; only their keys reach them.
    assert!(!matches("style", &entry));
    assert!(!matches("waiting", &entry));
}

#[test]
fn keys_restrict_a_term_to_one_field_and_terms_and_together() {
    let entry = entry();
    assert!(matches("repo:artifactize eval:sign reviewer:bo", &entry));
    assert!(!matches("repo:signoff", &entry));
    assert!(!matches("repo:artifactize reviewer:carol", &entry));
    assert!(matches("request:A-2", &entry));
    assert!(matches("tag:type:style", &entry));
    assert!(matches("tag:STYLE", &entry));
    assert!(!matches("tag:docs", &entry));
    let unclaimed = Entry {
        reviewer: None,
        ..entry.clone()
    };
    assert!(!matches("reviewer:bob", &unclaimed));
    // Quoted values keep their spaces; a quoted word is free text even with a colon.
    let spaced = Entry {
        repository: "/work/my repo".into(),
        ..entry.clone()
    };
    assert!(matches(r#"repo:"my repo""#, &spaced));
    assert!(!matches(r#""type:style""#, &entry));
}

#[test]
fn kinds_and_statuses_match_exactly_and_qualifiers_follow_claims() {
    let entry = entry();
    assert!(matches(DEFAULT, &entry));
    assert!(matches("KIND:Human Status:WAITING_HUMAN", &entry));
    assert!(!matches("kind:agent", &entry));
    assert!(!matches("status:green", &entry));
    let agent = Entry {
        kind: ProfileKind::Agent,
        status: RequestStatus::Green,
        reviewer: None,
        ..entry.clone()
    };
    assert!(matches("kind:agent status:green", &agent));
    assert!(!matches(DEFAULT, &agent));
    // Claims exist only on waiting Human requests: a settled or non-Human request is neither
    // claimed nor unclaimed.
    assert!(!matches("is:unclaimed", &agent));
    assert!(!matches("is:claimed", &agent));
    assert!(!matches("is:mine", &agent));
    assert!(matches("is:claimed", &entry));
    assert!(!matches("is:mine", &entry));
    assert!(matches(
        "is:mine",
        &Entry {
            mine: true,
            ..entry.clone()
        }
    ));
    assert!(matches(
        "is:unclaimed",
        &Entry {
            reviewer: None,
            ..entry
        }
    ));
}

#[test]
fn unknown_keys_values_and_empty_values_are_errors_that_name_the_choices() {
    let error = "rep:x".parse::<Query>().unwrap_err();
    assert!(
        error.contains("`rep:`") && error.contains("repo:"),
        "{error}"
    );
    let error = "kind:robot".parse::<Query>().unwrap_err();
    assert!(
        error.contains("runtime, agent, human or dependency"),
        "{error}"
    );
    let error = "status:waiting".parse::<Query>().unwrap_err();
    assert!(error.contains("waiting_human"), "{error}");
    let error = "is:urgent".parse::<Query>().unwrap_err();
    assert!(
        error.contains("is:mine, is:claimed or is:unclaimed"),
        "{error}"
    );
    assert!(
        "repo:"
            .parse::<Query>()
            .unwrap_err()
            .contains("needs a value")
    );
    assert_eq!("   ".parse::<Query>().unwrap(), Query::default());
}

#[test]
fn a_repository_starts_from_the_default_query_with_its_term() {
    assert_eq!(Query::text_for(None), DEFAULT);
    assert_eq!(
        Query::text_for(Some(Path::new("/work/my repo"))),
        r#"kind:human status:waiting_human repo:"/work/my repo""#
    );
    assert!(
        Query::text_for(Some(Path::new("/work/my repo")))
            .parse::<Query>()
            .unwrap()
            .matches(&Entry {
                repository: "/work/my repo".into(),
                ..entry()
            })
    );
}

#[test]
fn completion_offers_keys_then_values_of_the_listed_requests() {
    let entries = [
        entry(),
        Entry {
            repository: "/work/website".into(),
            eval: "docs/links".into(),
            reviewer: None,
            tags: vec!["docs".into()],
            ..entry()
        },
    ];
    let keys = complete("", &entries);
    assert_eq!(keys.keep, "");
    assert_eq!(
        keys.candidates,
        [
            "repo:",
            "eval:",
            "request:",
            "reviewer:",
            "tag:",
            "kind:",
            "status:",
            "is:"
        ]
    );
    let partial = complete("kind:human re", &entries);
    assert_eq!(partial.keep, "kind:human ");
    assert_eq!(partial.candidates, ["repo:", "request:", "reviewer:"]);
    assert_eq!(
        complete("repo:", &entries).candidates,
        ["repo:artifactize", "repo:website"]
    );
    assert_eq!(
        complete("eval:link", &entries).candidates,
        ["eval:docs/links"]
    );
    assert_eq!(complete("reviewer:", &entries).candidates, ["reviewer:bob"]);
    assert_eq!(
        complete("tag:", &entries).candidates,
        ["tag:docs", "tag:type:style"]
    );
    assert_eq!(
        complete("is:", &entries).candidates,
        ["is:claimed", "is:mine", "is:unclaimed"]
    );
    assert_eq!(
        complete("kind:", &entries).candidates,
        [
            "kind:agent",
            "kind:dependency",
            "kind:human",
            "kind:runtime"
        ]
    );
    assert!(
        complete("status:wait", &entries)
            .candidates
            .contains(&"status:waiting_human".to_owned())
    );
    assert!(complete("rep:", &entries).candidates.is_empty());
    // A value with spaces completes quoted.
    let spaced = [Entry {
        repository: "/work/my repo".into(),
        ..entry()
    }];
    assert_eq!(
        complete("repo:my", &spaced).candidates,
        [r#"repo:"my repo""#]
    );
}

/// A Run with `count` GREEN runtime requests and one waiting Human request at its end.
async fn state(count: usize) -> (tempfile::TempDir, std::path::PathBuf) {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(&repo).unwrap();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    let run: store::Run = serde_json::from_value(json!({"id":"run-1","repoPath":repo,
        "stateDir":state,"status":"GREEN","createdAt":"2026-01-01T00:00:00Z",
        "completedAt":"2026-01-01T00:01:00Z","selection":{"kind":"all"},"validation":null}))
    .unwrap();
    let mut requests: Vec<_> = (0..count)
        .map(|index| {
            crate::monitor::tests::request(&format!("app/check{index:03}"), "GREEN", json!({}))
                .request
        })
        .collect();
    requests.push(
        crate::monitor::tests::request(
            "app/signoff",
            "WAITING_HUMAN",
            json!({"profile":{"kind":"human"},"requestedProfile":{"kind":"human"}}),
        )
        .request,
    );
    receipts.create_run(&run, &requests).await.unwrap();
    (root, state)
}

#[tokio::test]
async fn search_pages_through_every_request_and_the_default_finds_waiting_human_ones() {
    let (_root, state) = state(2 * PAGE + 10).await;
    let reviewer: ReviewerId = "alice".parse().unwrap();
    let everything = Query::default();
    let first = search(&state, &everything, &reviewer, 0).await.unwrap();
    assert_eq!(first.requests.len(), PAGE);
    assert_eq!(first.next, Some(PAGE as u64));
    let second = search(&state, &everything, &reviewer, first.next.unwrap())
        .await
        .unwrap();
    assert_eq!(second.requests[0].1.eval, format!("app/check{PAGE:03}"));
    let third = search(&state, &everything, &reviewer, second.next.unwrap())
        .await
        .unwrap();
    assert_eq!(third.requests.len(), 11);
    assert_eq!(third.next, None);

    let waiting = search(&state, &DEFAULT.parse().unwrap(), &reviewer, 0)
        .await
        .unwrap();
    let evals: Vec<_> = waiting
        .requests
        .iter()
        .map(|(_, entry)| entry.eval.as_str())
        .collect();
    assert_eq!(evals, ["app/signoff"]);
    assert_eq!(waiting.next, None);
    // A narrow query still scans every batch for its matches.
    let late = search(&state, &"eval:check205".parse().unwrap(), &reviewer, 0)
        .await
        .unwrap();
    assert_eq!(late.requests.len(), 1);
}
