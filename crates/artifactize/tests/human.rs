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
    store::{self, OwnedReceipts, Receipts, RunView},
    types::{FailureCode, RequestId},
};
use futures_util::FutureExt;
use serde_json::{Value, json};
use support::os::{cat_program, false_program, true_program};
use tokio_util::sync::CancellationToken;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

const DRAFT_FIXTURE: &str = "ARTIFACTIZE_HUMAN_DRAFT_FIXTURE";
const DRAFT_REQUEST: &str = "ARTIFACTIZE_HUMAN_DRAFT_REQUEST";

#[tokio::test]
async fn draft_fixture_writer() {
    let Some(state) = std::env::var_os(DRAFT_FIXTURE).map(PathBuf::from) else {
        return;
    };
    let id = std::env::var(DRAFT_REQUEST).unwrap().parse().unwrap();
    let reviewer = "alice".parse().unwrap();
    artifactize_core::review::append_point(&state, &id, &reviewer, "L42:C9", "Second process")
        .await
        .unwrap();
    artifactize_core::changes::drain().await;
}

#[tokio::test]
async fn draft_is_shared_across_processes_and_cleared_on_submission() {
    use artifactize_core::review::{
        self, Answer, DraftEdit, Job, append_point, edit_draft, read_draft, remove_point,
    };

    let fixture = Fixture::new(false);
    let run = fixture.verify(returning()).await;
    let id = &run.requests[0].id;
    let receipts = fixture.receipts().await;
    let alice = "alice".parse().unwrap();
    let bob = "bob".parse().unwrap();
    let state = &fixture.state;
    human::claim(&receipts, id, &alice).await.unwrap();

    use artifactize_core::changes::{Change, Subscription};
    let mut changes = Subscription::new(state).await;
    let notification = async {
        // A reconciliation can supersede an exact notification, but it still tells every
        // subscribed window to reload the draft. Don't treat the registration's baseline as
        // proof that the other process has written anything.
        assert_eq!(changes.next().await, Change::Resync);
        loop {
            match changes.next().await {
                Change::DraftInvalidated(changed) if &changed == id => break,
                Change::StateInvalidated | Change::Resync
                    if read_draft(state, id, &alice).await.unwrap().is_some() =>
                {
                    break;
                }
                _ => {}
            }
        }
    };
    let mut writer = tokio::process::Command::new(std::env::current_exe().unwrap());
    writer
        .args(["--exact", "draft_fixture_writer"])
        .env(DRAFT_FIXTURE, state)
        .env(DRAFT_REQUEST, id.as_str())
        .kill_on_drop(true);
    let (output, noticed) = tokio::join!(
        writer.output(),
        tokio::time::timeout(support::os::patience(Duration::from_secs(30)), notification),
    );
    let output = output.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    noticed.expect("a subscribed window is told to reload the separate writer's draft");
    changes.close().await.unwrap();
    let initial = read_draft(state, id, &alice).await.unwrap().unwrap();
    assert_eq!(initial.points[0].text, "L42:C9");
    assert_eq!(read_draft(state, id, &bob).await.unwrap(), None);
    assert!(
        append_point(state, id, &bob, "#unauthorized", "fixture")
            .await
            .is_err()
    );

    human::unclaim(&receipts, id, &alice).await.unwrap();
    assert_eq!(
        read_draft(state, id, &alice).await.unwrap(),
        Some(initial.clone())
    );
    assert!(
        append_point(state, id, &alice, "#released", "fixture")
            .await
            .is_err()
    );
    human::claim(&receipts, id, &alice).await.unwrap();
    fs::write(fixture.repo.join("preview.md"), "# Preview").unwrap();
    fs::write(
        fixture.repo.join(".artifactize/preview.artf"),
        "artf preview : markdown = { path = \"preview.md\", basis = true }\n",
    )
    .unwrap();
    let viewer = artifactize_core::viewer::Session::open(
        &fixture.repo,
        &"preview".parse().unwrap(),
        &"markdown.view".parse().unwrap(),
    )
    .unwrap();
    let active = artifactize_core::viewer::Review {
        id: id.clone(),
        reviewer: alice.clone(),
    };
    let form = edit_draft(
        state,
        id,
        &alice,
        DraftEdit::Verdict {
            verdict: artifactize_core::runtime::Verdict::Green,
        },
    )
    .await
    .unwrap();
    let pointed = viewer
        .pointing(state, &active, "#checkout .total")
        .await
        .unwrap();
    assert_eq!(pointed.issues, form.issues);
    assert_eq!(pointed.verdict, form.verdict);
    assert_eq!(pointed.points.len(), 2);
    assert_eq!(pointed.points[1].text, "#checkout .total");
    assert!(pointed.points[1].source.starts_with("preview · "));
    assert!(
        store::read_request(state, id.as_str())
            .await
            .unwrap()
            .request
            .status()
            .is_human_wait()
    );
    let removed = remove_point(state, id, &alice, initial.points[0].clone())
        .await
        .unwrap();
    assert_eq!(removed.points, pointed.points[1..]);
    drop(receipts);
    assert_eq!(
        read_draft(state, id, &alice).await.unwrap(),
        Some(removed.clone())
    );

    let submitted = Job::Submit {
        id: id.clone(),
        answer: Answer::SavedDraft,
        claim: false,
    }
    .run(
        state.clone(),
        alice.clone(),
        CancellationToken::new(),
        review::log::channel().0,
    )
    .await;
    assert!(
        matches!(submitted, review::Outcome::Submitted { result: Ok(_), .. }),
        "{:?}",
        submitted.notice()
    );
    assert_eq!(read_draft(state, id, &alice).await.unwrap(), None);
    let saved = store::read_request(state, id.as_str()).await.unwrap();
    assert_eq!(saved.request.review_notes, None);
    assert_eq!(
        serde_json::to_value(saved.request.result().unwrap()).unwrap(),
        json!({"verdict":"GREEN","kind":"human","issues":[]})
    );
    assert!(
        append_point(state, id, &alice, "#settled", "fixture")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn schema_ten_reviews_keep_their_receipts_and_gain_drafts() {
    let fixture = Fixture::new(false);
    let run = fixture.verify(returning()).await;
    let id = &run.requests[0].id;
    let db = rusqlite::Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    db.execute_batch(&format!("{} PRAGMA user_version=10;", support::schema::TEN))
        .unwrap();
    drop(db);
    let alice = "alice".parse().unwrap();
    let receipts = fixture.receipts().await;
    assert_eq!(
        store::state_schema(&fixture.state).unwrap(),
        Some(store::STATE_SCHEMA_VERSION)
    );
    human::claim(&receipts, id, &alice).await.unwrap();
    let db = rusqlite::Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    db.execute(
        "INSERT INTO review_drafts(request_id, reviewer, data) VALUES (?, 'alice', ?)",
        rusqlite::params![
            id,
            json!({"verdict":null,"green":[],"red":[],"notes":"A note\nwith a second line"})
                .to_string()
        ],
    )
    .unwrap();
    drop(db);
    let draft = artifactize_core::review::read_draft(&fixture.state, id, &alice)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.points.len(), 1);
    assert_eq!(draft.points[0].text, "A note\nwith a second line");
    assert_eq!(draft.points[0].source, "Earlier notes");
    let updated =
        artifactize_core::review::append_point(&fixture.state, id, &alice, "L3:C4", "fixture")
            .await
            .unwrap();
    assert_eq!(updated.points[0], draft.points[0]);
    assert_eq!(updated.points[1].text, "L3:C4");
    assert_eq!(
        artifactize_core::review::read_draft(&fixture.state, id, &alice)
            .await
            .unwrap(),
        Some(updated)
    );
}

#[tokio::test]
async fn a_draft_saved_with_reasons_keeps_them_as_issues_and_submits_them() {
    use artifactize_core::review::{self, Answer, DraftEdit, Job, Readiness, read_draft};

    let fixture = Fixture::new(false);
    let run = fixture.verify(returning()).await;
    let id = &run.requests[0].id;
    let alice = "alice".parse().unwrap();
    let receipts = fixture.receipts().await;
    human::claim(&receipts, id, &alice).await.unwrap();
    drop(receipts);
    let db = rusqlite::Connection::open(fixture.state.join(store::DATABASE)).unwrap();
    db.execute(
        "INSERT INTO review_drafts(request_id, reviewer, data) VALUES (?, 'alice', ?)",
        rusqlite::params![
            id,
            json!({
                "verdict": "RED",
                "reasons": ["The totals are wrong"],
                "green": [["approved", {"kind": "fixed"}]],
                "red": [],
                "points": [],
            })
            .to_string()
        ],
    )
    .unwrap();
    drop(db);

    let draft = read_draft(&fixture.state, id, &alice)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.issues, ["The totals are wrong"]);
    assert_eq!(
        draft.readiness(artifactize_core::runtime::Verdict::Red),
        Readiness::Ready
    );
    // The first save keeps them as issues.
    let saved = review::edit_draft(
        &fixture.state,
        id,
        &alice,
        DraftEdit::AddIssue {
            text: "The footer overlaps".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        read_draft(&fixture.state, id, &alice).await.unwrap(),
        Some(saved)
    );

    let submitted = Job::Submit {
        id: id.clone(),
        answer: Answer::SavedDraft,
        claim: false,
    }
    .run(
        fixture.state.clone(),
        alice.clone(),
        CancellationToken::new(),
        review::log::channel().0,
    )
    .await;
    assert!(
        matches!(submitted, review::Outcome::Submitted { result: Ok(_), .. }),
        "{:?}",
        submitted.notice()
    );
    let settled = store::read_request(&fixture.state, id.as_str())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(settled.request.result().unwrap()).unwrap(),
        json!({
            "verdict": "RED",
            "kind": "human",
            "issues": [{"reason": "The totals are wrong"}, {"reason": "The footer overlaps"}],
        })
    );
    assert_eq!(read_draft(&fixture.state, id, &alice).await.unwrap(), None);
}

#[test]
fn a_long_open_viewer_cannot_read_a_new_folder_parts_files() {
    let root = support::os::tempdir();
    let repo = root.path();
    fs::create_dir(repo.join(".artifactize")).unwrap();
    fs::create_dir_all(repo.join("parent/child")).unwrap();
    fs::write(repo.join("parent/child/report.txt"), "report contents").unwrap();
    fs::write(repo.join("parent/home.txt"), "parent contents").unwrap();
    fs::write(
        repo.join(".artifactize/parent.artf"),
        "artf parent : files = { path = \"parent/\", basis = true }\n",
    )
    .unwrap();
    let artifact = "parent".parse().unwrap();
    let view = "files.view".parse().unwrap();
    let child = "child/report.txt".parse().unwrap();
    let original = artifactize_core::viewer::Session::open(repo, &artifact, &view).unwrap();
    assert_eq!(original.read_text(&child).unwrap(), "report contents");

    fs::write(
        repo.join(".artifactize/child.artf"),
        "artf child : files = { path = \"parent/child/\", basis = true }\n",
    )
    .unwrap();
    fs::write(
        repo.join(".artifactize/parent.artf"),
        "artf parent : files = { path = \"parent/\", parts = [child] }\n",
    )
    .unwrap();
    assert!(
        original
            .read_text(&child)
            .unwrap_err()
            .contains("Reopen the viewer")
    );
    let reopened = artifactize_core::viewer::Session::open(repo, &artifact, &view).unwrap();
    assert_ne!(original.scope(), reopened.scope());
    assert!(reopened.read_text(&child).is_err());
    assert_eq!(
        reopened.read_text(&"home.txt".parse().unwrap()).unwrap(),
        "parent contents"
    );
}

#[tokio::test]
async fn a_claimed_review_opens_by_its_exact_reference_outside_the_default_search() {
    use artifactize_core::review::{Target, search};
    let fixture = Fixture::new(false);
    let run = fixture.verify(returning()).await;
    let id = &run.requests[0].id;
    let alice = "alice".parse().unwrap();
    let query = "type:human waiting".parse().unwrap();
    let page = search::search(&fixture.state, &query, &alice, 0)
        .await
        .unwrap();
    assert_eq!(page.requests.len(), 1);
    assert_eq!(&page.requests[0].0.request.id, id);
    let before = search::open(&fixture.state, id, &query, &alice)
        .await
        .unwrap();
    assert!(!before.outside_filter());
    let receipts = fixture.receipts().await;
    human::claim(&receipts, id, &alice).await.unwrap();
    assert!(
        search::search(&fixture.state, &query, &alice, 0)
            .await
            .unwrap()
            .requests
            .is_empty()
    );
    let opened = search::open(&fixture.state, id, &query, &alice)
        .await
        .unwrap();
    assert!(opened.outside_filter());
    assert_eq!(opened.outside_reason.as_deref(), Some("Claimed by you."));
    assert_eq!(&opened.view.request.id, id);
    let target = Target::from(opened.view.request.as_ref());
    assert_eq!(target.run, run.run.id);
    assert_eq!(target.node.as_str(), "e:review/check");
    let existing = "kind:human status:waiting_human is:mine".parse().unwrap();
    assert!(
        !search::open(&fixture.state, id, &existing, &alice)
            .await
            .unwrap()
            .outside_filter()
    );
}

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
        write_human(&(repo), &repo, fingerprint);
        let state = root.path().join("state");
        root.track(&state, &repo);
        Self {
            repo,
            state,
            _root: root,
        }
    }

    /// A repository whose Human eval's scope is its Artifact `review` and the `spec` its
    /// instruction references, beside an Artifact `other` outside that scope.
    fn referencing() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        write_human(&(repo), &repo.join("review"), false);
        let review = repo.join("review/index.artf");
        let declaration = support::declaration::read_text(&review).unwrap();
        assert!(declaration.contains("Review."));
        fs::write(
            support::declaration::source_path(&review),
            declaration.replace("Review.", "Review @{artf spec}."),
        )
        .unwrap();
        for name in ["spec", "other"] {
            fs::create_dir(repo.join(name)).unwrap();
            support::declaration::write(
                &(repo),
                repo.join(name).join("index.artf"),
                json!({"name":name,"basis":true}).to_string(),
            )
            .unwrap();
        }
        let state = root.path().join("state");
        root.track(&state, &repo);
        Self {
            repo,
            state,
            _root: root,
        }
    }

    async fn verify(&self, options: VerifyOptions) -> RunView {
        // A hang guard, not a timing assertion: a loaded machine runs the whole file at once.
        tokio::time::timeout(
            support::os::patience(Duration::from_secs(60)),
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

    /// The separate SQLite worker must be closed before teardown, even on assertion panic.
    async fn with_human_receipts<T>(
        &self,
        id: &RequestId,
        action: impl AsyncFnOnce(&Receipts, PathBuf) -> T,
    ) -> T {
        let (receipts, repo) = human::open(&self.state, id).await.unwrap();
        let outcome = std::panic::AssertUnwindSafe(async { action(&receipts, repo).await })
            .catch_unwind()
            .await;
        let closed = receipts.close().await;
        match outcome {
            Ok(value) => {
                closed.expect("close the Human action's receipt connection");
                value
            }
            Err(panic) => {
                if let Err(error) = closed {
                    eprintln!("Human receipt connection cleanup failed: {error}");
                }
                std::panic::resume_unwind(panic)
            }
        }
    }

    fn cli_verify(&self) -> Value {
        let recorded = Command::new(support::os::cli())
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(["verify", "--all", "--json"])
            .recorded()
            .unwrap();
        let output = &recorded.output;
        // Submission returns at once; the request stays open for the person.
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        recorded.saved_json().expect("submitted Human Run")
    }

    /// End Runs as soon as only unclaimed sign-offs are left, as a YELLOW Run whose
    /// worktree is deleted afterwards has.
    fn no_idle_grace(&self) {
        let output = Command::new(support::os::cli())
            .arg("--state-dir")
            .arg(&self.state)
            .args(["limits", "--signoff-idle-grace", "0"])
            .record_output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Wait until the Run of a `cli_verify` has ended with only its sign-off left.
    async fn yellow(&self, submitted: &Value) {
        let id = submitted["id"].as_str().unwrap().parse().unwrap();
        let ended = owned_run::terminal(&self.state, &id).await;
        assert_eq!(ended.run.status().as_str(), "YELLOW");
    }

    /// Delete this fixture's repository folder, as removing a worktree does, after its ended
    /// Runs are released.
    fn remove_repo(&self) {
        self._root.release(&self.state, &self.repo).unwrap();
        fs::remove_dir_all(&self.repo).unwrap();
    }
}

/// Every waiting sign-off by request, including one held by a reviewer.
async fn listed(state: &Path) -> Vec<String> {
    let query = "kind:human status:waiting_human".parse().unwrap();
    artifactize_core::review::search::search(state, &query, &"reviewer".parse().unwrap(), 0)
        .await
        .unwrap()
        .requests
        .into_iter()
        .map(|(view, _)| view.request.id.as_str().to_owned())
        .collect()
}

/// A request's status and failure code.
async fn outcome(state: &Path, id: &str) -> (String, Option<FailureCode>) {
    let view = store::read_request(state, id).await.unwrap();
    (
        view.request.status().as_str().to_owned(),
        view.request.error_code(),
    )
}

fn write_human(root: &Path, path: &Path, fingerprint: bool) {
    fs::create_dir_all(path).unwrap();
    std::fs::write(
        support::declaration::source_path(path.join("fingerprint")),
        "human-v1\n",
    )
    .unwrap();
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
            },
        ],
    });
    if fingerprint {
        declaration["fingerprint"] =
            json!({"script":{"command":cat_program(),"args":["fingerprint"]}});
    } else {
        declaration["fingerprint"] = json!(false);
    }
    support::declaration::write(root, path.join("index.artf"), declaration.to_string()).unwrap();
}

/// Options that record Human requests and return at once instead of waiting for them.
fn returning() -> VerifyOptions {
    VerifyOptions {
        ..Default::default()
    }
}

fn green() -> human::Submission {
    human::Submission::try_from(json!({"verdict":"GREEN"})).unwrap()
}

/// A request ID parsed once, where a test names one.
fn rid(id: &str) -> artifactize_core::types::RequestId {
    id.parse().unwrap()
}

#[tokio::test]
async fn a_folder_whole_with_parts_can_be_claimed_inspected_submitted_and_reused() {
    let fixture = Fixture::new(true);
    fs::write(
        support::declaration::source_path(fixture.repo.join("part.txt")),
        "file part",
    )
    .unwrap();
    support::declaration::write(
        &(fixture.repo),
        fixture.repo.join("part.txt.artf"),
        json!({"name":"file-part","basis":true}).to_string(),
    )
    .unwrap();
    fs::create_dir(fixture.repo.join("nested")).unwrap();
    fs::write(
        support::declaration::source_path(fixture.repo.join("nested/input.txt")),
        "folder part",
    )
    .unwrap();
    support::declaration::write(
        &(fixture.repo),
        fixture.repo.join("nested/index.artf"),
        json!({"name":"folder-part","basis":true}).to_string(),
    )
    .unwrap();
    let run = fixture.verify(returning()).await;
    let request = &run.requests[0];
    assert_eq!(request.status().as_str(), "DEFERRED");
    let receipts = fixture.receipts().await;
    let reviewer = "alice".parse().unwrap();
    human::claim(&receipts, &request.id, &reviewer)
        .await
        .unwrap();
    let inspected = human::run_human_tool(
        &receipts,
        &request.id,
        &reviewer,
        &"review_inspect_review".parse().unwrap(),
        CancellationToken::new(),
        artifactize_core::review::log::Run::unwatched(),
    )
    .await
    .unwrap();
    assert!(!inspected.is_error());
    let submitted = human::submit(
        &receipts,
        &request.id,
        &reviewer,
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(submitted.status().as_str(), "GREEN");
    drop(receipts);
    let reused = fixture.verify(returning()).await;
    assert_eq!(reused.requests[0].status().as_str(), "GREEN");
    assert_eq!(reused.run.executions_started, 0);
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
            &"review_inspect_review".parse().unwrap(),
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
            &"review_read_review".parse().unwrap(),
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
        &"review_inspect_review".parse().unwrap(),
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
        &"review_fail_review".parse().unwrap(),
        CancellationToken::new(),
        artifactize_core::review::log::Run::unwatched(),
    )
    .await
    .unwrap();
    assert!(result.is_error());
    for result in [
        json!({"verdict":"RED"}),
        json!({"verdict":"RED","issues":[3]}),
        json!({"verdict":"BLUE"}),
        json!({"verdict":"GREEN","issues":["still wrong"]}),
        json!({"verdict":"GREEN","approved":true}),
        json!({"verdict":"RED","issues":["x".repeat(256_000)]}),
    ] {
        let refused = match human::Submission::try_from(result) {
            Ok(result) => human::submit(
                &receipts,
                &rid(id),
                &"alice".parse().unwrap(),
                &result,
                CancellationToken::new(),
            )
            .await
            .map(|_| ()),
            Err(error) => Err(error),
        };
        assert!(refused.is_err());
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
            &"review_inspect_review".parse().unwrap(),
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
        fs::write(
            support::declaration::source_path(fixture.repo.join("fingerprint")),
            "human-v2\n",
        )
        .unwrap();
        let error = if tool {
            human::run_human_tool(
                &receipts,
                id,
                &"alice".parse().unwrap(),
                &"review_inspect_review".parse().unwrap(),
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

/// How a waiting request's declarations stop matching those it recorded.
#[derive(Clone, Copy)]
enum Change {
    /// The declaration of the Artifact the eval references differs.
    Referenced,
    /// The eval's own declaration differs.
    Title,
    /// The eval is no longer declared.
    Removed,
}

/// The Human action that meets the changed declarations.
#[derive(Clone, Copy)]
enum Action {
    Tool,
    Submit,
    /// Resolving what the tools would run, which needs no claim.
    Commands,
}

#[tokio::test]
async fn changed_declarations_end_the_request_error_and_the_next_verify_asks_again() {
    for (change, action, message) in [
        (Change::Referenced, Action::Submit, "declarations changed"),
        (Change::Title, Action::Commands, "declarations changed"),
        (Change::Removed, Action::Commands, "no longer exists"),
    ] {
        let fixture = Fixture::referencing();
        let run = fixture.verify(returning()).await;
        let id = run.requests[0].id.clone();
        let alice = "alice".parse().unwrap();
        let receipts = fixture.receipts().await;
        human::claim(&receipts, &id, &alice).await.unwrap();
        let review = fixture.repo.join("review");
        match change {
            Change::Referenced => support::declaration::write(
                &(fixture.repo),
                fixture.repo.join("spec/index.artf"),
                r#"{"name":"spec","basis":true,"tags":["changed"]}"#,
            )
            .unwrap(),
            Change::Title => {
                let recorded = support::declaration::read_text(review.join("index.artf")).unwrap();
                assert!(recorded.contains("Human check"));
                fs::write(
                    support::declaration::source_path(review.join("index.artf")),
                    recorded.replace("Human check", "Another check"),
                )
                .unwrap();
            }
            Change::Removed => support::declaration::write(
                &(fixture.repo),
                review.join("index.artf"),
                r#"{"name":"review","basis":true}"#,
            )
            .unwrap(),
        }
        let error = match action {
            Action::Tool => human::run_human_tool(
                &receipts,
                &id,
                &alice,
                &"review_inspect_review".parse().unwrap(),
                CancellationToken::new(),
                artifactize_core::review::log::Run::unwatched(),
            )
            .await
            .map(drop)
            .unwrap_err(),
            Action::Submit => {
                human::submit(&receipts, &id, &alice, &green(), CancellationToken::new())
                    .await
                    .map(drop)
                    .unwrap_err()
            }
            Action::Commands => human::tool_commands(&receipts, &id)
                .await
                .map(drop)
                .unwrap_err(),
        };
        assert!(error.contains(message), "{error}");
        let saved = store::read_request(&fixture.state, &id).await.unwrap();
        assert_eq!(saved.request.status().as_str(), "ERROR");
        assert_eq!(saved.request.error_code(), Some(FailureCode::InputChanged));
        assert!(saved.request.result().is_none());
        assert!(saved.claim.is_none(), "the claim ends with the request");
        assert!(listed(&fixture.state).await.is_empty());
        let ended = owned_run::terminal(&fixture.state, &run.run.id).await;
        assert_eq!(ended.run.status().as_str(), "ERROR");
        // The next verify asks for the sign-off again, where the eval is still declared.
        if !matches!(change, Change::Removed) {
            fixture.verify(returning()).await;
            let asked = listed(&fixture.state).await;
            assert_eq!(asked.len(), 1);
            assert_ne!(asked[0], id.as_str());
        }
    }
}

#[tokio::test]
async fn unreadable_declarations_refuse_the_action_and_the_request_keeps_waiting() {
    let fixture = Fixture::new(false);
    let run = fixture.verify(returning()).await;
    let id = run.requests[0].id.clone();
    let alice = "alice".parse().unwrap();
    let receipts = fixture.receipts().await;
    human::claim(&receipts, &id, &alice).await.unwrap();
    let declaration = fixture.repo.join("index.artf");
    let recorded = support::declaration::read_file(&declaration).unwrap();
    fs::write(support::declaration::source_path(&declaration), "name = ").unwrap();
    human::submit(&receipts, &id, &alice, &green(), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(listed(&fixture.state).await, [id.as_str()]);
    fs::write(support::declaration::source_path(&declaration), recorded).unwrap();
    let submitted = human::submit(&receipts, &id, &alice, &green(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(submitted.status().as_str(), "GREEN");
}

#[tokio::test]
async fn human_actions_never_read_the_runs_saved_definitions() {
    let fixture = Fixture::referencing();
    let run = fixture.verify(returning()).await;
    let id = run.requests[0].id.clone();
    // The Run's saved graph no longer decodes: a reader of it fails. (Definitions are one
    // program now, so the workspace itself must read.)
    struct SavedDefinition {
        db: rusqlite::Connection,
        run_id: artifactize_core::types::RunId,
        json: String,
    }
    impl Drop for SavedDefinition {
        fn drop(&mut self) {
            let restored = self.db.execute(
                "UPDATE run_definitions SET data=? WHERE run_id=?",
                [&self.json, self.run_id.as_str()],
            );
            if !matches!(restored, Ok(1)) {
                if std::thread::panicking() {
                    eprintln!("restoring the fixture's saved definition failed: {restored:?}");
                } else {
                    panic!("restoring the fixture's saved definition failed: {restored:?}");
                }
            }
        }
    }
    let database = fixture.state.join(store::DATABASE);
    let saved = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row(
        "SELECT data FROM run_definitions WHERE run_id=?",
        [&run.run.id],
        |row| row.get(0),
    )
    .unwrap();
    let definition = SavedDefinition {
        db: rusqlite::Connection::open(&database).unwrap(),
        run_id: run.run.id.clone(),
        json: saved,
    };
    definition
        .db
        .execute(
            "UPDATE run_definitions SET data='{' WHERE run_id=?",
            [&run.run.id],
        )
        .unwrap();
    assert!(store::read_request(&fixture.state, &id).await.is_err());
    let alice = "alice".parse().unwrap();
    fixture
        .with_human_receipts(&id, async |receipts, repo| {
            assert_eq!(repo, support::os::canonical(&fixture.repo));
            human::claim(receipts, &id, &alice).await.unwrap();
            let tool = human::run_human_tool(
                receipts,
                &id,
                &alice,
                &"review_inspect_review".parse().unwrap(),
                CancellationToken::new(),
                artifactize_core::review::log::Run::unwatched(),
            )
            .await
            .unwrap();
            assert!(!tool.is_error());
        })
        .await;
    let submitted = human::submit_and_publish(
        &fixture.state,
        &id,
        &alice,
        &green(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(submitted.status().as_str(), "GREEN");
    // Restore on either path before the Run-owning fixture cleans up.
    drop(definition);
}

#[tokio::test]
async fn a_submitted_signoff_settles_its_provisional_dependents_in_the_owned_run() {
    let fixture = Fixture::new(false);
    support::declaration::write(
        &(fixture.repo),
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
    write_human(&(fixture.repo), &fixture.repo.join("child"), true);
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
    support::declaration::write(
        &(fixture.repo),
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
    write_human(&(fixture.repo), &fixture.repo.join("child"), true);
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
    let status = |view: &RunView, target: &str| {
        view.requests
            .iter()
            .find(|request| request.target == target)
            .unwrap()
            .status()
    };
    let waiting = owned_run::subscribed(&fixture.state, async |changes| {
        tokio::time::timeout(support::os::patience(Duration::from_secs(30)), async {
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
        .expect("the sign-off waits while the machine eval runs")
    })
    .await;
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

    fs::write(support::declaration::source_path(&release), "").unwrap();
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
    fs::write(
        support::declaration::source_path(other.repo.join("fingerprint")),
        "different-repo-input\n",
    )
    .unwrap();
    let result = human::run_human_tool(
        &receipts,
        follower,
        &"alice".parse().unwrap(),
        &"review_inspect_review".parse().unwrap(),
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
        &human::Submission::try_from(json!({"verdict":"RED","issues":["Needs work"]})).unwrap(),
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
    fs::write(
        support::declaration::source_path(other.repo.join("fingerprint")),
        "human-v1\n",
    )
    .unwrap();
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
            .issues()[0]
            .reason,
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
    fs::write(
        support::declaration::source_path(fixture.repo.join("fingerprint")),
        "changed\n",
    )
    .unwrap();
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
    let mut declaration: Value =
        support::declaration::read(support::declaration::read_file(&path).unwrap()).unwrap();
    declaration.as_object_mut().unwrap().remove("fingerprint");
    support::declaration::write(&(fixture.repo), &path, declaration.to_string()).unwrap();
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
    fs::write(
        support::declaration::source_path(fixture.repo.join("fingerprint")),
        "human-v2\n",
    )
    .unwrap();
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
    fs::write(
        support::declaration::source_path(fixture.repo.join("fingerprint")),
        "human-v3\n",
    )
    .unwrap();
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
    let mut declaration: Value =
        support::declaration::read(support::declaration::read_file(&path).unwrap()).unwrap();
    let mut same = declaration["evals"][0].clone();
    same["id"] = json!("same");
    same["title"] = json!("Same criteria");
    let mut different = same.clone();
    different["id"] = json!("zz-different");
    different["payload"]["instruction"] = json!("Review it differently.");
    declaration["evals"]
        .as_array_mut()
        .unwrap()
        .extend([same, different]);
    support::declaration::write(&(fixture.repo), path, declaration.to_string()).unwrap();
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
        &human::Submission::try_from(json!({"verdict":"RED","issues":["Different review"]})).unwrap(),
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
async fn a_removed_original_leaves_review_and_a_remaining_follower_takes_its_signoff_over() {
    let fixture = Fixture::new(true);
    fixture.no_idle_grace();
    let original = fixture.cli_verify();
    let mut other = Fixture::new(true);
    other.state = fixture.state.clone();
    other._root.track(&other.state, &other.repo);
    let joined = other.cli_verify();
    fixture.yellow(&original).await;
    fixture.yellow(&joined).await;
    let owner = original["requests"][0]["id"].as_str().unwrap();
    let follower = joined["requests"][0]["id"].as_str().unwrap();
    assert_eq!(listed(&fixture.state).await, [owner]);
    let owner_id = rid(owner);
    fixture
        .with_human_receipts(&owner_id, async |receipts, _| {
            human::claim(receipts, &owner_id, &"alice".parse().unwrap())
                .await
                .unwrap();
        })
        .await;
    fixture.remove_repo();
    assert_eq!(listed(&fixture.state).await, [follower]);
    assert_eq!(
        outcome(&fixture.state, owner).await,
        ("ERROR".to_owned(), Some(FailureCode::RepositoryRemoved))
    );
    // Its own repository now holds the review: a claim, tools and the submission reach it.
    let follower_id = rid(follower);
    fixture
        .with_human_receipts(&follower_id, async |receipts, _| {
            let bob = "bob".parse().unwrap();
            assert_eq!(
                human::claim(receipts, &follower_id, &bob)
                    .await
                    .unwrap()
                    .request_id
                    .as_str(),
                follower
            );
            let inspected = human::run_human_tool(
                receipts,
                &follower_id,
                &bob,
                &"review_inspect_review".parse().unwrap(),
                CancellationToken::new(),
                artifactize_core::review::log::Run::unwatched(),
            )
            .await
            .unwrap();
            assert_eq!(
                serde_json::to_value(inspected).unwrap()["content"][0]["text"],
                "human-v1\n"
            );
            human::submit(
                receipts,
                &follower_id,
                &bob,
                &green(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        })
        .await;
    assert_eq!(outcome(&fixture.state, follower).await.0, "GREEN");
    assert!(listed(&fixture.state).await.is_empty());
}

#[tokio::test]
async fn a_removed_follower_ends_alone_and_the_last_removed_repository_ends_the_signoff() {
    let fixture = Fixture::new(true);
    fixture.no_idle_grace();
    let original = fixture.cli_verify();
    let mut other = Fixture::new(true);
    other.state = fixture.state.clone();
    other._root.track(&other.state, &other.repo);
    let joined = other.cli_verify();
    fixture.yellow(&original).await;
    fixture.yellow(&joined).await;
    let owner = original["requests"][0]["id"].as_str().unwrap();
    let follower = joined["requests"][0]["id"].as_str().unwrap();
    other.remove_repo();
    assert_eq!(listed(&fixture.state).await, [owner]);
    assert_eq!(
        outcome(&fixture.state, follower).await,
        ("ERROR".to_owned(), Some(FailureCode::RepositoryRemoved))
    );
    assert_eq!(outcome(&fixture.state, owner).await.0, "DEFERRED");
    fixture.remove_repo();
    assert!(listed(&fixture.state).await.is_empty());
    assert_eq!(
        outcome(&fixture.state, owner).await,
        ("ERROR".to_owned(), Some(FailureCode::RepositoryRemoved))
    );
}
