#[path = "support/run.rs"]
mod owned_run;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use artifactize_core::{
    human,
    project::{self, VerifyOptions, selection::Selection},
    store::{self, OwnedReceipts, RunView},
};
use serde_json::{Value, json};
use support::os::{cat_program, false_program, true_program};
use tokio_util::sync::CancellationToken;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

struct Fixture {
    _root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(fingerprint: bool) -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        write_human(&repo, fingerprint);
        let state = root.path().join("state");
        root.track(&state, &repo);
        Self {
            repo,
            state,
            _root: root,
        }
    }

    async fn verify(&self, options: VerifyOptions) -> RunView {
        tokio::time::timeout(
            Duration::from_secs(10),
            support::runs::record(
                &self.repo,
                Some(&self.state),
                &Selection::All,
                &options,
                CancellationToken::new(),
            ),
        )
        .await
        .expect("verify must exit while Humans wait")
        .unwrap()
    }

    async fn receipts(&self) -> OwnedReceipts {
        OwnedReceipts::open(&self.state, &self.repo).await.unwrap()
    }

    fn cli_verify(&self) -> Value {
        let output = Command::new(support::os::cli())
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(["verify", "--all", "--json"])
            .record_output()
            .unwrap();
        // Submission returns at once; the request stays open for the person.
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

fn write_human(path: &Path, fingerprint: bool) {
    fs::create_dir_all(path).unwrap();
    support::declaration::write(path.join("fingerprint"), "human-v1\n").unwrap();
    let mut declaration = json!({
        "name":"review",
        "views":{
            "human_tools":{
                "inspect":{
                    "description":"Inspect",
                    "kind":"output",
                    "command":cat_program(),
                    "args":["fingerprint"],
                },
                "fail":{"description":"Fail","kind":"output","command":false_program(),"args":[]},
            },
            "agent_tools":{"read":{"builtin":"read"}},
        },
        "evals":[
            {
                "id":"check",
                "title":"Human check",
                "profile":{"kind":"human"},
                "payload":{"instruction":"Review."},
                "pass_schema":{
                    "type":"object",
                    "properties":{"approved":{"const":true}},
                    "required":["approved"],
                    "additionalProperties":false,
                },
                "fail_schema":{
                    "type":"object",
                    "properties":{"reason":{"type":"string","minLength":1}},
                    "required":["reason"],
                    "additionalProperties":false,
                },
            },
        ],
    });
    if fingerprint {
        declaration["fingerprint"] =
            json!({"script":{"command":cat_program(),"args":["fingerprint"]}});
    } else {
        declaration["fingerprint"] = json!(false);
    }
    support::declaration::write(path.join("index.artf"), declaration.to_string()).unwrap();
}

/// Options that record Human requests and return at once instead of waiting for them.
fn returning() -> VerifyOptions {
    VerifyOptions {
        ..Default::default()
    }
}

fn green() -> Value {
    json!({"verdict":"GREEN","approved":true})
}

/// A request ID parsed once, where a test names one.
fn rid(id: &str) -> artifactize_core::types::RequestId {
    id.parse().unwrap()
}

#[tokio::test]
async fn waiting_survives_verifier_exit_and_zero_budget_with_idempotent_claims() {
    let fixture = Fixture::new(true);
    let run = fixture.cli_verify();
    let id = run["requests"][0]["id"].as_str().unwrap();
    assert_eq!(run["status"], "WAITING_HUMAN");
    assert_eq!(run["requests"][0]["status"], "DEFERRED");
    assert_eq!(run["executionsStarted"], 0);
    let receipts = fixture.receipts().await;
    let alice_id = "alice".parse().unwrap();
    let bob_id = "bob".parse().unwrap();
    let request = rid(id);
    let (alice, bob) = tokio::join!(
        human::claim(&receipts, &request, &alice_id),
        human::claim(&receipts, &request, &bob_id)
    );
    assert_ne!(alice.is_ok(), bob.is_ok());
    let claim = alice.or(bob).unwrap();
    assert_eq!(
        human::claim(&receipts, &rid(id), &claim.reviewer)
            .await
            .unwrap()
            .claimed_at,
        claim.claimed_at
    );
    let follower = fixture
        .verify(VerifyOptions {
            max_executions: Some(0),
            ..returning()
        })
        .await;
    assert_eq!(follower.requests[0].status().as_str(), "DEFERRED");
    assert_eq!(
        follower.requests[0].execution_id.as_deref(),
        run["requests"][0]["executionId"].as_str()
    );
    let default = Fixture::new(false)
        .verify(VerifyOptions {
            max_executions: Some(0),
            ..returning()
        })
        .await;
    assert_eq!(default.requests[0].status().as_str(), "DEFERRED");
    assert_eq!(default.run.executions_started, 0);
}

#[tokio::test]
async fn claimant_only_tools_and_correctable_schema_errors_then_exactly_once_submit() {
    let fixture = Fixture::new(false);
    let run = fixture.verify(returning()).await;
    let id = &run.requests[0].id;
    let receipts = fixture.receipts().await;
    assert!(
        human::submit(
            &receipts,
            &rid(id),
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    human::claim(&receipts, &rid(id), &"alice".parse().unwrap())
        .await
        .unwrap();
    assert!(
        human::run_human_tool(
            &receipts,
            id,
            &"bob".parse().unwrap(),
            &"inspect_review".parse().unwrap(),
            CancellationToken::new(),
            artifactize_core::review::log::Run::unwatched()
        )
        .await
        .is_err()
    );
    assert!(
        human::submit(
            &receipts,
            &rid(id),
            &"bob".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert!(
        human::run_human_tool(
            &receipts,
            id,
            &"alice".parse().unwrap(),
            &"read_review".parse().unwrap(),
            CancellationToken::new(),
            artifactize_core::review::log::Run::unwatched()
        )
        .await
        .is_err()
    );
    let result = human::run_human_tool(
        &receipts,
        id,
        &"alice".parse().unwrap(),
        &"inspect_review".parse().unwrap(),
        CancellationToken::new(),
        artifactize_core::review::log::Run::unwatched(),
    )
    .await
    .unwrap();
    assert!(!result.is_error());
    let result = human::run_human_tool(
        &receipts,
        id,
        &"alice".parse().unwrap(),
        &"fail_review".parse().unwrap(),
        CancellationToken::new(),
        artifactize_core::review::log::Run::unwatched(),
    )
    .await
    .unwrap();
    assert!(result.is_error());
    for result in [
        json!({"verdict":"GREEN"}),
        json!({"verdict":"RED","reason":3}),
        json!({"verdict":"BLUE"}),
        json!({"verdict":"GREEN","approved":true,"extra":1}),
        json!({"verdict":"RED","reason":"x".repeat(256_000)}),
    ] {
        assert!(
            human::submit(
                &receipts,
                &rid(id),
                &"alice".parse().unwrap(),
                &result,
                CancellationToken::new()
            )
            .await
            .is_err()
        );
        assert_eq!(
            store::read_run(&fixture.state, &run.run.id)
                .await
                .unwrap()
                .requests[0]
                .status()
                .as_str(),
            "DEFERRED"
        );
    }
    let result = human::submit(
        &receipts,
        &rid(id),
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.status().as_str(), "GREEN");
    assert!(
        human::submit(
            &receipts,
            &rid(id),
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert!(
        human::claim(&receipts, &rid(id), &"alice".parse().unwrap())
            .await
            .is_err()
    );
    assert!(
        human::run_human_tool(
            &receipts,
            id,
            &"alice".parse().unwrap(),
            &"inspect_review".parse().unwrap(),
            CancellationToken::new(),
            artifactize_core::review::log::Run::unwatched()
        )
        .await
        .is_err()
    );
    let saved = store::read_request(&fixture.state, id).await.unwrap();
    assert_eq!(saved.request.status().as_str(), "GREEN");
    assert!(
        saved.claim.is_none(),
        "submission releases the public claim"
    );
    assert!(
        artifactize_core::cache::list(&fixture.state, true)
            .await
            .unwrap()
            .is_empty(),
        "fingerprint=false must not publish reusable evidence"
    );
}

#[tokio::test]
async fn fingerprint_change_before_submit_or_tool_settles_error_without_publishing() {
    for tool in [false, true] {
        let fixture = Fixture::new(true);
        let run = fixture.verify(returning()).await;
        let id = &run.requests[0].id;
        let receipts = fixture.receipts().await;
        human::claim(&receipts, &rid(id), &"alice".parse().unwrap())
            .await
            .unwrap();
        fs::write(fixture.repo.join("fingerprint"), "human-v2\n").unwrap();
        let error = if tool {
            human::run_human_tool(
                &receipts,
                id,
                &"alice".parse().unwrap(),
                &"inspect_review".parse().unwrap(),
                CancellationToken::new(),
                artifactize_core::review::log::Run::unwatched(),
            )
            .await
            .unwrap_err()
        } else {
            human::submit(
                &receipts,
                &rid(id),
                &"alice".parse().unwrap(),
                &green(),
                CancellationToken::new(),
            )
            .await
            .unwrap_err()
        };
        assert!(
            error.contains("Fingerprint changed during review"),
            "{error}"
        );
        let saved = store::read_run(&fixture.state, &run.run.id).await.unwrap();
        assert_eq!(saved.requests[0].status().as_str(), "ERROR");
        assert_eq!(
            saved.requests[0].error_code().as_deref(),
            Some("INPUT_CHANGED")
        );
        assert!(saved.requests[0].result().is_none());
        assert!(
            store::read_keyed_executions(&fixture.state, &[run.requests[0].key.clone().unwrap()],)
                .await
                .unwrap()
                .is_empty(),
            "the failed Human owner must not remain available for joining"
        );
        assert!(
            artifactize_core::cache::show(
                &fixture.state,
                run.requests[0].key.as_deref().unwrap(),
                false,
            )
            .await
            .unwrap()
            .is_empty()
        );
    }
}

#[tokio::test]
async fn scoped_declaration_changes_and_new_children_refuse_reconnection() {
    let fixture = Fixture::new(false);
    let run = fixture.verify(returning()).await;
    let id = &run.requests[0].id;
    let receipts = fixture.receipts().await;
    human::claim(&receipts, &rid(id), &"alice".parse().unwrap())
        .await
        .unwrap();
    fs::create_dir(fixture.repo.join("child")).unwrap();
    support::declaration::write(
        fixture.repo.join("child/index.artf"),
        r#"{"name":"child","basis":true}"#,
    )
    .unwrap();
    assert!(
        human::run_human_tool(
            &receipts,
            id,
            &"alice".parse().unwrap(),
            &"inspect_review".parse().unwrap(),
            CancellationToken::new(),
            artifactize_core::review::log::Run::unwatched()
        )
        .await
        .unwrap_err()
        .contains("declarations changed")
    );
    assert!(
        human::submit(
            &receipts,
            &rid(id),
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    fs::remove_file(fixture.repo.join("child/index.artf")).unwrap();
    human::submit(
        &receipts,
        &rid(id),
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_submitted_signoff_settles_its_provisional_dependents_in_the_owned_run() {
    let fixture = Fixture::new(false);
    write_human(&fixture.repo.join("child"), true);
    support::declaration::write(
        fixture.repo.join("index.artf"),
        json!({
            "name":"parent",
            "evals":[
                {
                    "id":"test",
                    "title":"Dependent",
                    "profile":{"kind":"runtime","command":true_program(),"args":[]},
                    "payload":{"instruction":"Check {review}."},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    let run = fixture.verify(returning()).await;
    assert_eq!(run.run.status().as_str(), "WAITING_HUMAN");
    let waiting = run
        .requests
        .iter()
        .find(|r| r.status() == artifactize_core::types::RequestStatus::Deferred)
        .unwrap();
    assert_eq!(
        run.requests
            .iter()
            .find(|r| r.target == "parent")
            .unwrap()
            .status()
            .as_str(),
        "PROVISIONAL",
        "a pending sign-off holds up nothing that relies on it"
    );
    let receipts = fixture.receipts().await;
    human::claim(&receipts, &waiting.id, &"alice".parse().unwrap())
        .await
        .unwrap();
    human::submit(
        &receipts,
        &waiting.id,
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let observed = owned_run::terminal(&fixture.state, &run.run.id).await;
    assert_eq!(
        observed.run.status(),
        artifactize_core::types::RunStatus::Green
    );
    assert_eq!(
        store::read_run(&fixture.state, &run.run.id)
            .await
            .unwrap()
            .run
            .executions_started,
        1
    );
    let complete = fixture.verify(returning()).await;
    assert_eq!(complete.run.status().as_str(), "GREEN");
    assert_eq!(complete.run.executions_started, 0);
    assert!(
        complete
            .requests
            .iter()
            .all(|r| r.status() == artifactize_core::types::RequestStatus::Green)
    );
}

#[tokio::test]
async fn an_open_run_reads_running_again_once_its_last_signoff_is_submitted() {
    let fixture = Fixture::new(true);
    let release = fixture._root.path().join("release");
    // The sign-off holds up nothing: the root's machine eval runs on PROVISIONAL meanwhile.
    write_human(&fixture.repo.join("child"), true);
    support::declaration::write(
        fixture.repo.join("index.artf"),
        json!({
            "name":"work",
            "evals":[
                {
                    "id":"wait",
                    "title":"Machine work",
                    "profile":{
                        "kind":"runtime",
                        "command":support::os::waiting_program(fixture._root.path()),
                        "args":[support::os::path_text(&release)],
                    },
                    "payload":{"instruction":"Wait."},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    // Started without a barrier: the machine eval runs until the test releases it.
    let context = artifactize_core::runs::Context::new(
        &fixture.repo,
        Some(&fixture.state),
        &support::os::cli(),
    )
    .unwrap();
    let id = artifactize_core::runs::start(&context, &Selection::All, &returning(), None)
        .await
        .unwrap()
        .run;
    let mut changes = artifactize_core::changes::Subscription::new(&fixture.state).await;
    let status = |view: &RunView, target: &str| {
        view.requests
            .iter()
            .find(|request| request.target == target)
            .unwrap()
            .status()
    };
    let waiting = tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
        loop {
            let view = store::read_run(&fixture.state, id.as_str()).await.unwrap();
            if status(&view, "review").is_human_wait()
                && status(&view, "work") == artifactize_core::types::RequestStatus::Running
            {
                return view;
            }
            changes.next_state().await;
        }
    })
    .await
    .expect("the sign-off waits while the machine eval runs");
    let shown = |view: &RunView| view.run.status().as_str();
    assert_eq!(shown(&waiting), "WAITING_HUMAN");
    // It follows from the open sign-off; the saved Run stays RUNNING.
    let saved: (String, String) = rusqlite::Connection::open_with_flags(
        fixture.state.join(store::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row(
        "SELECT status,json_extract(data,'$.status') FROM runs WHERE id=?",
        [id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap();
    assert_eq!(saved, ("RUNNING".into(), "RUNNING".into()));

    let receipts = fixture.receipts().await;
    let signoff = &waiting
        .requests
        .iter()
        .find(|request| request.target == "review")
        .unwrap()
        .id;
    let alice = "alice".parse().unwrap();
    human::claim(&receipts, signoff, &alice).await.unwrap();
    human::submit(
        &receipts,
        signoff,
        &alice,
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    // Every reader shows the open Run RUNNING again while its machine eval still runs.
    let running = store::read_run(&fixture.state, id.as_str()).await.unwrap();
    assert_eq!(status(&running, "work").as_str(), "RUNNING");
    assert_eq!(shown(&running), "RUNNING");
    let listed = store::read_runs(&fixture.state, Some(&fixture.repo), 10, 0)
        .await
        .unwrap();
    assert_eq!(listed[0].status.as_str(), "RUNNING");
    let catalog = store::read_catalog(&fixture.state).await.unwrap();
    assert_eq!(catalog[0].status.as_str(), "RUNNING");

    fs::write(&release, "").unwrap();
    let ended = owned_run::terminal(&fixture.state, &id).await;
    assert_eq!(shown(&ended), "GREEN");
}

#[tokio::test]
async fn cross_repo_waiters_share_claim_tools_and_one_published_result() {
    let fixture = Fixture::new(true);
    let original = fixture.cli_verify();
    let mut other = Fixture::new(true);
    // The follower owns its Run in the shared state before its own repository drops.
    other.state = fixture.state.clone();
    other._root.track(&other.state, &other.repo);
    let run = support::runs::record(
        &other.repo,
        Some(&fixture.state),
        &Selection::All,
        &returning(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(run.requests[0].status().as_str(), "DEFERRED");
    let receipts = fixture.receipts().await;
    let follower = &run.requests[0].id;
    let owner = original["requests"][0]["id"].as_str().unwrap();
    assert_eq!(
        human::claim(&receipts, &rid(follower), &"alice".parse().unwrap())
            .await
            .unwrap()
            .request_id
            .as_str(),
        owner
    );
    assert!(
        human::claim(&receipts, &rid(owner), &"bob".parse().unwrap())
            .await
            .is_err()
    );
    fs::write(other.repo.join("fingerprint"), "different-repo-input\n").unwrap();
    let result = human::run_human_tool(
        &receipts,
        follower,
        &"alice".parse().unwrap(),
        &"inspect_review".parse().unwrap(),
        CancellationToken::new(),
        artifactize_core::review::log::Run::unwatched(),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap()["content"][0]["text"],
        "human-v1\n"
    );
    human::submit(
        &receipts,
        &rid(follower),
        &"alice".parse().unwrap(),
        &json!({"verdict":"RED","reason":"Needs work"}),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        human::submit(
            &receipts,
            &rid(owner),
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    // The owned worker, not the observer, must save the follower's completed Run.
    let settled = owned_run::terminal(&fixture.state, &run.run.id).await;
    assert_eq!(settled.run.status().as_str(), "RED");
    assert_eq!(settled.requests[0].status().as_str(), "RED");
    assert_eq!(
        store::read_run(&fixture.state, &run.run.id)
            .await
            .unwrap()
            .requests[0]
            .status()
            .as_str(),
        "RED"
    );
    fs::write(other.repo.join("fingerprint"), "human-v1\n").unwrap();
    let completed = support::runs::record(
        &other.repo,
        Some(&fixture.state),
        &Selection::All,
        &returning(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(completed.run.status().as_str(), "RED");
    assert_eq!(
        completed.requests[0]
            .result()
            .as_ref()
            .unwrap()
            .owner_fields()["reason"],
        "Needs work"
    );
    assert_eq!(
        completed.requests[0].execution_id.as_deref(),
        original["requests"][0]["executionId"].as_str()
    );
    let history = artifactize_core::cache::list(&fixture.state, true)
        .await
        .unwrap();
    assert_eq!(history.len(), 1, "the joined review publishes one result");
    assert_eq!(
        history[0].execution_id,
        completed.requests[0].execution_id.clone().unwrap()
    );
}

#[tokio::test]
async fn only_the_claimant_unclaims_a_waiting_request_including_through_followers() {
    let fixture = Fixture::new(true);
    let original = fixture.cli_verify();
    let owner = original["requests"][0]["id"].as_str().unwrap();
    let mut other = Fixture::new(true);
    // The follower owns its Run in the shared state before its own repository drops.
    other.state = fixture.state.clone();
    other._root.track(&other.state, &other.repo);
    let run = support::runs::record(
        &other.repo,
        Some(&fixture.state),
        &Selection::All,
        &returning(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let follower = &run.requests[0].id;
    let receipts = fixture.receipts().await;
    assert!(
        human::unclaim(&receipts, &rid(owner), &"alice".parse().unwrap())
            .await
            .unwrap_err()
            .contains("claimant")
    );
    let claim = human::claim(&receipts, &rid(owner), &"alice".parse().unwrap())
        .await
        .unwrap();
    assert!(
        "".parse::<artifactize_core::types::ReviewerId>().is_err(),
        "an empty reviewer must be rejected at the input edge"
    );
    for id in [owner, follower.as_str()] {
        assert!(
            human::unclaim(&receipts, &rid(id), &"bob".parse().unwrap())
                .await
                .is_err()
        );
    }
    // A follower forwards the release to its original request, like a claim.
    let released = human::unclaim(&receipts, &rid(follower), &"alice".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        (released.request_id.as_str(), released.claimed_at),
        (owner, claim.claimed_at)
    );
    assert!(
        human::unclaim(&receipts, &rid(owner), &"alice".parse().unwrap())
            .await
            .is_err()
    );
    assert!(
        human::submit(
            &receipts,
            &rid(owner),
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(
        human::claim(&receipts, &rid(follower), &"bob".parse().unwrap())
            .await
            .unwrap()
            .request_id
            .as_str(),
        owner
    );
    assert_eq!(
        store::read_request(&fixture.state, owner)
            .await
            .unwrap()
            .claim
            .unwrap()
            .reviewer,
        "bob"
    );
    human::submit(
        &receipts,
        &rid(owner),
        &"bob".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    for id in [owner, follower.as_str()] {
        assert!(
            human::unclaim(&receipts, &rid(id), &"bob".parse().unwrap())
                .await
                .unwrap_err()
                .contains("not waiting")
        );
    }
}

#[tokio::test]
async fn concurrent_submissions_commit_only_once() {
    let fixture = Fixture::new(true);
    let run = fixture.verify(returning()).await;
    let id = &run.requests[0].id;
    let receipts = fixture.receipts().await;
    human::claim(&receipts, &rid(id), &"alice".parse().unwrap())
        .await
        .unwrap();
    let result = green();
    let reviewer = "alice".parse().unwrap();
    let request = rid(id);
    let (first, second) = tokio::join!(
        human::submit(
            &receipts,
            &request,
            &reviewer,
            &result,
            CancellationToken::new()
        ),
        human::submit(
            &receipts,
            &request,
            &reviewer,
            &result,
            CancellationToken::new()
        ),
    );
    assert_ne!(first.is_ok(), second.is_ok());
}

#[tokio::test]
async fn forced_human_checks_fingerprint_without_replacing_cache() {
    let fixture = Fixture::new(true);
    let run = fixture.verify(returning()).await;
    let receipts = fixture.receipts().await;
    human::claim(&receipts, &run.requests[0].id, &"alice".parse().unwrap())
        .await
        .unwrap();
    human::submit(
        &receipts,
        &run.requests[0].id,
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let forced = fixture
        .verify(VerifyOptions {
            force: true,
            ..returning()
        })
        .await;
    assert_eq!(forced.requests[0].status().as_str(), "DEFERRED");
    human::claim(&receipts, &forced.requests[0].id, &"alice".parse().unwrap())
        .await
        .unwrap();
    fs::write(fixture.repo.join("fingerprint"), "changed\n").unwrap();
    assert!(
        human::submit(
            &receipts,
            &forced.requests[0].id,
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    let cached = artifactize_core::cache::show(
        &fixture.state,
        run.requests[0].key.as_deref().unwrap(),
        false,
    )
    .await
    .unwrap()
    .pop()
    .unwrap();
    assert_eq!(cached.provenance.request_id, run.requests[0].id);
    assert_eq!(cached.status().as_str(), "GREEN");
}

#[tokio::test]
async fn omitted_fingerprint_reuses_human_signoff_and_rechecks_changed_inputs() {
    let fixture = Fixture::new(false);
    let path = fixture.repo.join("index.artf");
    let mut declaration: Value = support::declaration::read(fs::read(&path).unwrap()).unwrap();
    declaration.as_object_mut().unwrap().remove("fingerprint");
    support::declaration::write(&path, declaration.to_string()).unwrap();
    let run = fixture.verify(returning()).await;
    assert!(
        run.requests[0]
            .fingerprint
            .as_ref()
            .unwrap()
            .starts_with("artifactsum:")
    );
    let receipts = fixture.receipts().await;
    human::claim(&receipts, &run.requests[0].id, &"alice".parse().unwrap())
        .await
        .unwrap();
    human::submit(
        &receipts,
        &run.requests[0].id,
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let next = fixture.verify(returning()).await;
    assert_eq!(next.requests[0].status().as_str(), "GREEN");
    assert_eq!(next.requests[0].execution_id, run.requests[0].execution_id);
    fs::write(fixture.repo.join("fingerprint"), "human-v2\n").unwrap();
    let changed = fixture.verify(returning()).await;
    assert_eq!(changed.requests[0].status().as_str(), "DEFERRED");
    assert_ne!(changed.requests[0].key, run.requests[0].key);
    human::claim(
        &receipts,
        &changed.requests[0].id,
        &"alice".parse().unwrap(),
    )
    .await
    .unwrap();
    fs::write(fixture.repo.join("fingerprint"), "human-v3\n").unwrap();
    let error = human::submit(
        &receipts,
        &changed.requests[0].id,
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error.contains("Fingerprint changed during review"),
        "{error}"
    );
}

#[tokio::test]
async fn fingerprint_false_results_are_not_reused_by_a_new_verify() {
    let fixture = Fixture::new(false);
    let run = fixture.verify(returning()).await;
    let receipts = fixture.receipts().await;
    human::claim(&receipts, &run.requests[0].id, &"alice".parse().unwrap())
        .await
        .unwrap();
    human::submit(
        &receipts,
        &run.requests[0].id,
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let next = fixture.verify(returning()).await;
    assert_eq!(next.requests[0].status().as_str(), "DEFERRED");
    assert_ne!(next.requests[0].execution_id, run.requests[0].execution_id);
}

#[tokio::test]
async fn human_forwarding_settlement_and_status_are_scoped_to_the_definition() {
    let fixture = Fixture::new(true);
    let path = fixture.repo.join("index.artf");
    let mut declaration: Value = support::declaration::read(fs::read(&path).unwrap()).unwrap();
    let mut same = declaration["evals"][0].clone();
    same["id"] = json!("same");
    same["title"] = json!("Same criteria");
    let mut different = same.clone();
    different["id"] = json!("zz-different");
    different["pass_schema"]["properties"]["approved"]["const"] = json!(false);
    declaration["evals"]
        .as_array_mut()
        .unwrap()
        .extend([same, different]);
    support::declaration::write(path, declaration.to_string()).unwrap();
    let run = fixture.verify(returning()).await;
    assert_eq!(run.requests[0].execution_id, run.requests[1].execution_id);
    assert_eq!(run.requests[0].eval_def_hash, run.requests[1].eval_def_hash);
    assert_ne!(run.requests[0].execution_id, run.requests[2].execution_id);
    assert_ne!(run.requests[0].eval_def_hash, run.requests[2].eval_def_hash);
    let receipts = fixture.receipts().await;
    let claim = human::claim(&receipts, &run.requests[1].id, &"alice".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(claim.request_id, run.requests[0].id);
    human::submit(
        &receipts,
        &run.requests[1].id,
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let saved = store::read_run(&fixture.state, &run.run.id).await.unwrap();
    assert_eq!(saved.requests[0].status().as_str(), "GREEN");
    assert_eq!(saved.requests[1].status().as_str(), "GREEN");
    assert_eq!(saved.requests[2].status().as_str(), "DEFERRED");
    let status = project::status(
        &fixture.repo,
        Some(&fixture.state),
        &Selection::All,
        &returning(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(status.counts.reuse, 2);
    assert_eq!(status.counts.wait, 1);
    assert_eq!(status.evals[2].state, project::EvalCondition::Deferred);
    assert_eq!(
        human::claim(&receipts, &run.requests[2].id, &"bob".parse().unwrap())
            .await
            .unwrap()
            .request_id
            .as_str(),
        run.requests[2].id.as_str()
    );
    human::submit(
        &receipts,
        &run.requests[2].id,
        &"bob".parse().unwrap(),
        &json!({"verdict":"RED","reason":"Different review"}),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let reused = fixture.verify(returning()).await;
    assert_eq!(reused.requests[0].status().as_str(), "GREEN");
    assert_eq!(reused.requests[1].status().as_str(), "GREEN");
    assert_eq!(reused.requests[2].status().as_str(), "RED");
    for (new, original) in reused.requests.iter().zip(&run.requests) {
        assert_eq!(new.execution_id, original.execution_id);
        assert_eq!(
            new.provenance.as_ref().unwrap().eval_def_hash,
            original.eval_def_hash
        );
    }
    assert_eq!(
        artifactize_core::cache::list(&fixture.state, true)
            .await
            .unwrap()
            .len(),
        2,
        "the two distinct definitions publish independent review evidence"
    );
    let status = project::status(
        &fixture.repo,
        Some(&fixture.state),
        &Selection::All,
        &returning(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(status.counts.reuse, 3);
    assert_eq!(status.evals[2].state, project::EvalCondition::Red);
}

#[tokio::test]
async fn a_variant_human_signoff_reconnects_and_its_record_names_the_variant() {
    let fixture = Fixture::new(true);
    let path = fixture.repo.join("index.artf");
    let mut declaration: Value = support::declaration::read(fs::read(&path).unwrap()).unwrap();
    declaration["evals"][0]["profile_variants"] = json!({"lead":{"kind":"human"}});
    support::declaration::write(&path, declaration.to_string()).unwrap();
    let run = fixture
        .verify(VerifyOptions {
            profile: Some(project::selection::ProfileSelection::Named(
                "lead".parse().unwrap(),
            )),
            ..returning()
        })
        .await;
    let request = &run.requests[0];
    assert_eq!(request.status().as_str(), "DEFERRED");
    assert_eq!(request.options.variant.as_deref(), Some("lead"));
    let receipts = fixture.receipts().await;
    human::claim(&receipts, &request.id, &"alice".parse().unwrap())
        .await
        .unwrap();
    let settled = human::submit(
        &receipts,
        &request.id,
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(settled.status().as_str(), "GREEN");
    // The declared profile reuses the sign-off the variant produced.
    let reused = fixture.verify(returning()).await;
    assert_eq!(reused.requests[0].status().as_str(), "GREEN");
    assert_eq!(reused.requests[0].execution_id, request.execution_id);
    assert_eq!(reused.requests[0].options.variant.as_deref(), Some("lead"));
    assert_eq!(reused.requests[0].reviewer.as_deref(), Some("alice"));
}
