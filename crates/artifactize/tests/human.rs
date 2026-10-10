use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use artifactize::{
    human,
    project::{self, VerifyOptions, selection::Selection},
    store::{self, Receipts, RunView},
};
use rusqlite::Connection;
use serde_json::{Value, json};
use support::os::bin;
use tokio_util::sync::CancellationToken;

mod support;

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(fingerprint: bool) -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        write_human(&repo, fingerprint);
        Self {
            repo,
            state: root.path().join("state"),
            _root: root,
        }
    }

    async fn verify(&self, options: VerifyOptions) -> RunView {
        tokio::time::timeout(
            Duration::from_secs(10),
            project::verify(
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

    async fn receipts(&self) -> Receipts {
        Receipts::open(&self.state, &self.repo).await.unwrap()
    }

    fn database(&self) -> Connection {
        Connection::open(self.state.join(store::DATABASE)).unwrap()
    }

    fn cli_verify(&self) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(["verify", "--all", "--timeout-ms", "1", "--json"])
            .output()
            .unwrap();
        // The Human wait times out at once; the request stays open for submission.
        assert_eq!(
            output.status.code(),
            Some(3),
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
                    "command":bin("cat"),
                    "args":["fingerprint"],
                },
                "fail":{"description":"Fail","kind":"output","command":bin("false"),"args":[]},
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
            json!({"script":{"command":bin("cat"),"args":["fingerprint"]}});
    } else {
        declaration["fingerprint"] = json!(false);
    }
    support::declaration::write(path.join("index.artf"), declaration.to_string()).unwrap();
}

/// Options that record Human requests and return at once instead of waiting for them.
fn returning() -> VerifyOptions {
    VerifyOptions {
        wait_timeout: Duration::from_millis(1),
        ..Default::default()
    }
}

fn green() -> Value {
    json!({"verdict":"GREEN","approved":true})
}

#[tokio::test]
async fn waiting_survives_verifier_exit_and_zero_budget_with_idempotent_claims() {
    let fixture = Fixture::new(true);
    let run = fixture.cli_verify();
    let id = run["requests"][0]["id"].as_str().unwrap();
    assert_eq!(run["status"], "INCOMPLETE");
    assert_eq!(run["requests"][0]["status"], "WAITING_HUMAN");
    assert_eq!(run["executionsStarted"], 0);
    let receipts = fixture.receipts().await;
    let alice_id = "alice".parse().unwrap();
    let bob_id = "bob".parse().unwrap();
    let (alice, bob) = tokio::join!(
        human::claim(&receipts, id, &alice_id),
        human::claim(&receipts, id, &bob_id)
    );
    assert_ne!(alice.is_ok(), bob.is_ok());
    let claim = alice.or(bob).unwrap();
    assert_eq!(
        human::claim(&receipts, id, &claim.reviewer)
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
    assert_eq!(follower.requests[0].status().as_str(), "WAITING_HUMAN");
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
    assert_eq!(default.requests[0].status().as_str(), "WAITING_HUMAN");
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
            id,
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    human::claim(&receipts, id, &"alice".parse().unwrap())
        .await
        .unwrap();
    assert!(
        human::run_human_tool(
            &receipts,
            id,
            &"bob".parse().unwrap(),
            "inspect_review",
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert!(
        human::submit(
            &receipts,
            id,
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
            "read_review",
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    let result = human::run_human_tool(
        &receipts,
        id,
        &"alice".parse().unwrap(),
        "inspect_review",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!result.is_error());
    let result = human::run_human_tool(
        &receipts,
        id,
        &"alice".parse().unwrap(),
        "fail_review",
        CancellationToken::new(),
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
                id,
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
            "WAITING_HUMAN"
        );
    }
    let result = human::submit(
        &receipts,
        id,
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
            id,
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert!(
        human::claim(&receipts, id, &"alice".parse().unwrap())
            .await
            .is_err()
    );
    assert!(
        human::run_human_tool(
            &receipts,
            id,
            &"alice".parse().unwrap(),
            "inspect_review",
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(
        fixture
            .database()
            .query_row(
                "SELECT count(*) FROM requests WHERE claimed_by IS NOT NULL",
                [],
                |row| row.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        fixture
            .database()
            .query_row(
                "SELECT count(*) FROM executions WHERE completed_at IS NOT NULL",
                [],
                |row| row.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn fingerprint_change_before_submit_or_tool_settles_error_without_publishing() {
    for tool in [false, true] {
        let fixture = Fixture::new(true);
        let run = fixture.verify(returning()).await;
        let id = &run.requests[0].id;
        let receipts = fixture.receipts().await;
        human::claim(&receipts, id, &"alice".parse().unwrap())
            .await
            .unwrap();
        fs::write(fixture.repo.join("fingerprint"), "human-v2\n").unwrap();
        let error = if tool {
            human::run_human_tool(
                &receipts,
                id,
                &"alice".parse().unwrap(),
                "inspect_review",
                CancellationToken::new(),
            )
            .await
            .unwrap_err()
        } else {
            human::submit(
                &receipts,
                id,
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
        assert_eq!(
            fixture
                .database()
                .query_row(
                    "SELECT count(*) FROM executions WHERE status='WAITING_HUMAN'",
                    [],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
            0
        );
        assert!(
            receipts
                .cached_execution(run.requests[0].key.as_deref().unwrap())
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn scoped_declaration_changes_and_new_children_refuse_reconnection() {
    let fixture = Fixture::new(false);
    let run = fixture.verify(returning()).await;
    let id = &run.requests[0].id;
    let receipts = fixture.receipts().await;
    human::claim(&receipts, id, &"alice".parse().unwrap())
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
            "inspect_review",
            CancellationToken::new()
        )
        .await
        .unwrap_err()
        .contains("declarations changed")
    );
    assert!(
        human::submit(
            &receipts,
            id,
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
        id,
        &"alice".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn submitted_fingerprint_unblocks_dependents_on_next_verify() {
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
                    "profile":{"kind":"runtime","command":"true","args":[]},
                    "payload":{"instruction":"Check child."},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    let run = fixture.verify(returning()).await;
    assert_eq!(run.run.status().as_str(), "INCOMPLETE");
    let waiting = run
        .requests
        .iter()
        .find(|r| r.status() == artifactize::types::RequestStatus::WaitingHuman)
        .unwrap();
    assert_eq!(
        run.requests
            .iter()
            .find(|r| r.target == "parent")
            .unwrap()
            .status()
            .as_str(),
        "WAIT_DEPENDENCY"
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
    let complete = fixture.verify(returning()).await;
    assert_eq!(complete.run.status().as_str(), "GREEN");
    assert_eq!(complete.run.executions_started, 1);
    assert!(
        complete
            .requests
            .iter()
            .all(|r| r.status() == artifactize::types::RequestStatus::Green)
    );
}

#[tokio::test]
async fn cross_repo_waiters_share_claim_tools_and_one_published_result() {
    let fixture = Fixture::new(true);
    let original = fixture.cli_verify();
    let other = Fixture::new(true);
    let run = project::verify(
        &other.repo,
        Some(&fixture.state),
        &Selection::All,
        &returning(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(run.requests[0].status().as_str(), "WAITING_HUMAN");
    let receipts = fixture.receipts().await;
    let follower = &run.requests[0].id;
    let owner = original["requests"][0]["id"].as_str().unwrap();
    assert_eq!(
        human::claim(&receipts, follower, &"alice".parse().unwrap())
            .await
            .unwrap()
            .request_id
            .as_str(),
        owner
    );
    assert!(
        human::claim(&receipts, owner, &"bob".parse().unwrap())
            .await
            .is_err()
    );
    fs::write(other.repo.join("fingerprint"), "different-repo-input\n").unwrap();
    let result = human::run_human_tool(
        &receipts,
        follower,
        &"alice".parse().unwrap(),
        "inspect_review",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap()["content"][0]["text"],
        "human-v1\n"
    );
    human::submit(
        &receipts,
        follower,
        &"alice".parse().unwrap(),
        &json!({"verdict":"RED","reason":"Needs work"}),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        human::submit(
            &receipts,
            owner,
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    let settled = store::read_run(&fixture.state, &run.run.id).await.unwrap();
    assert_eq!(settled.requests[0].status().as_str(), "RED");
    receipts.finish(&run.run, &run.requests).await.unwrap();
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
    let completed = project::verify(
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
    assert_eq!(
        fixture
            .database()
            .query_row("SELECT count(*) FROM executions", [], |row| row
                .get::<_, u32>(0))
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn only_the_claimant_unclaims_a_waiting_request_including_through_followers() {
    let fixture = Fixture::new(true);
    let original = fixture.cli_verify();
    let owner = original["requests"][0]["id"].as_str().unwrap();
    let other = Fixture::new(true);
    let run = project::verify(
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
        human::unclaim(&receipts, owner, &"alice".parse().unwrap())
            .await
            .unwrap_err()
            .contains("claimant")
    );
    let claim = human::claim(&receipts, owner, &"alice".parse().unwrap())
        .await
        .unwrap();
    for (id, reviewer) in [(owner, "bob"), (follower.as_str(), "bob"), (owner, "")] {
        match reviewer.parse() {
            Ok(reviewer) => assert!(human::unclaim(&receipts, id, &reviewer).await.is_err()),
            Err(_) => assert!(
                reviewer.is_empty(),
                "invalid reviewer rejected at the input edge"
            ),
        }
    }
    // A follower forwards the release to its original request, like a claim.
    let released = human::unclaim(&receipts, follower, &"alice".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        (released.request_id.as_str(), released.claimed_at),
        (owner, claim.claimed_at)
    );
    assert!(
        human::unclaim(&receipts, owner, &"alice".parse().unwrap())
            .await
            .is_err()
    );
    assert!(
        human::submit(
            &receipts,
            owner,
            &"alice".parse().unwrap(),
            &green(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(
        human::claim(&receipts, follower, &"bob".parse().unwrap())
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
        owner,
        &"bob".parse().unwrap(),
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    for id in [owner, follower.as_str()] {
        assert!(
            human::unclaim(&receipts, id, &"bob".parse().unwrap())
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
    human::claim(&receipts, id, &"alice".parse().unwrap())
        .await
        .unwrap();
    let result = green();
    let reviewer = "alice".parse().unwrap();
    let (first, second) = tokio::join!(
        human::submit(&receipts, id, &reviewer, &result, CancellationToken::new()),
        human::submit(&receipts, id, &reviewer, &result, CancellationToken::new()),
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
    assert_eq!(forced.requests[0].status().as_str(), "WAITING_HUMAN");
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
    let cached = receipts
        .cached_execution(run.requests[0].key.as_deref().unwrap())
        .await
        .unwrap()
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
    assert_eq!(changed.requests[0].status().as_str(), "WAITING_HUMAN");
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
    assert_eq!(next.requests[0].status().as_str(), "WAITING_HUMAN");
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
    assert_eq!(saved.requests[2].status().as_str(), "WAITING_HUMAN");
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
    assert_eq!(status.evals[2].state, project::EvalCondition::WaitingHuman);
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
        fixture
            .database()
            .query_row::<u32, _, _>(
                "SELECT count(*) FROM executions WHERE completed_at IS NOT NULL",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        2
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
    assert_eq!(request.status().as_str(), "WAITING_HUMAN");
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
