#[path = "support/run.rs"]
mod owned_run;

use std::{
    fs,
    path::Path,
    process::{Command, Output},
    time::Duration,
};

use serde_json::{Value, json};
use support::os::{echo_program, false_program, true_program};

#[path = "support/order.rs"]
mod order;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

fn command(repo: &Path, state: &Path) -> Command {
    let mut command = Command::new(support::os::cli());
    command
        .arg("--repo")
        .arg(repo)
        .arg("--state-dir")
        .arg(state);
    command
}

fn output(repo: &Path, state: &Path, args: &[&str], code: i32) -> Output {
    let output = command(repo, state).args(args).record_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn query(repo: &Path, state: &Path, args: &[&str], code: i32) -> Value {
    let mut args = args.to_vec();
    args.push("--json");
    serde_json::from_slice(&output(repo, state, &args, code).stdout).unwrap()
}

async fn saved_run(state: &Path, id: &str) -> Value {
    let view = artifactize_core::store::read_run(state, id).await.unwrap();
    serde_json::to_value(view).unwrap()
}

async fn wait_for_other_verdicts(state: &Path, id: &str) {
    use artifactize_core::types::{RequestStatus, RunStatus};

    const WATCHDOG: Duration = Duration::from_secs(30);
    let expected = [
        ("other/pass", RequestStatus::Green),
        ("other/fail", RequestStatus::Red),
        ("notes/human", RequestStatus::Deferred),
    ];
    owned_run::subscribed(state, async |changes| {
        tokio::time::timeout(support::os::patience(WATCHDOG), async {
            loop {
                let run = artifactize_core::store::read_run(state, id).await.unwrap();
                if run.run.status() == RunStatus::WaitingHuman
                    && expected.iter().all(|(eval, verdict)| {
                        run.requests
                            .iter()
                            .any(|request| &request.eval_id == eval && request.status() == *verdict)
                    })
                {
                    return;
                }
                changes.next_reconciled().await;
            }
        })
        .await
        .expect("Run did not save the two machine verdicts and deferred sign-off");
    })
    .await;
}

#[tokio::test]
async fn saved_definitions_and_paged_runs_survive_repository_removal() {
    let root = owned_run::Root::from(support::os::tempdir());
    let first = root.path().join("first");
    let second = root.path().join("second");
    let state = root.path().join("state");
    fs::create_dir_all(first.join("scenarios/checkout")).unwrap();
    fs::create_dir_all(first.join("scenarios/search")).unwrap();
    fs::create_dir(first.join("input")).unwrap();
    fs::create_dir(first.join("unselected")).unwrap();
    fs::create_dir(&second).unwrap();
    // Own submissions before any command or observation can unwind.
    root.track(&state, &first);
    root.track(&state, &second);
    support::declaration::write(
        &(first),
        first.join("input/index.artf"),
        r#"{"name":"input","basis":true}"#,
    )
    .unwrap();
    support::declaration::write(
        &(first),
        first.join("unselected/index.artf"),
        r#"{"name":"unselected","basis":true}"#,
    )
    .unwrap();
    for (name, title) in [("checkout", "Checkout"), ("search", "Search")] {
        support::declaration::write(
            &(first),
            first.join(format!("scenarios/{name}/index.artf")),
            json!({
                "name":name,
                "evals":[
                    {
                        "id":"review",
                        "title":title,
                        "profile":{"kind":"runtime","command":false_program(),"args":[]},
                        "profile_variants":{
                            "brief":{
                                "kind":"runtime",
                                "command":echo_program(),
                                "args":["saved result"],
                            },
                        },
                        "payload":{"instruction":"Inspect {input}."},
                        "pass_schema":{"type":"object"},
                    },
                ],
            })
            .to_string(),
        )
        .unwrap();
    }
    support::declaration::write(
        &(second),
        second.join("index.artf"),
        json!({
            "name":"other",
            "evals":[
                {
                    "id":"pass",
                    "title":"Pass",
                    "profile":{"kind":"runtime","command":true_program(),"args":[]},
                    "payload":{"instruction":"Pass."},
                },
                {
                    "id":"fail",
                    "title":"Fail",
                    "profile":{"kind":"runtime","command":false_program(),"args":[]},
                    "payload":{"instruction":"Fail."},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    // A sign-off on its own file Artifact, so the RED machine eval above does not block it.
    fs::write(
        support::declaration::source_path(second.join("notes.md")),
        "notes",
    )
    .unwrap();
    support::declaration::write(
        &(second),
        second.join("notes.md.artf"),
        json!({
            "name":"notes",
            "evals":[
                {
                    "id":"human",
                    "title":"Human",
                    "profile":{"kind":"human"},
                    "payload":{"instruction":"Inspect."},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();

    let selected = query(
        &first,
        &state,
        &[
            "verify",
            "--wait",
            "--artifacts",
            "checkout,search",
            "--profile",
            "brief",
            "--force",
            "--jobs",
            "2",
            "--max-executions",
            "3",
        ],
        0,
    );
    owned_run::hold_signoffs(&state);
    // Submission returns while the owned Run stays WAITING_HUMAN with its sign-off open.
    // Read the submit-only response itself, not the saved audit.
    let accepted = support::os::run(command(&second, &state).args(["verify", "--all", "--json"]));
    assert_eq!(accepted.status.code(), Some(0), "{accepted:?}");
    let other: Value = serde_json::from_slice(&accepted.stdout).unwrap();
    let other_id = other["run"].as_str().unwrap();
    let last = query(
        &first,
        &state,
        &[
            "verify",
            "--wait",
            "--eval",
            "checkout/review",
            "--profile",
            "brief",
            "--comment",
            "Checking out — étape 2",
        ],
        0,
    );
    wait_for_other_verdicts(&state, other_id).await;
    let other = saved_run(&state, other_id).await;
    // Fixed creation times make ordering and pagination independent of clock resolution.
    let mut audits = Vec::new();
    for (run, created) in [
        (selected, "2000-01-01T00:00:00.000000000Z"),
        (other, "2000-01-02T00:00:00.000000000Z"),
        (last, "2000-01-03T00:00:00.000000000Z"),
    ] {
        let id = run["id"].as_str().unwrap();
        if run["status"] != "WAITING_HUMAN" {
            support::runs::finished_worker(
                &state,
                &first,
                &id.parse().unwrap(),
                support::os::running,
            );
        }
        order::created_at(&state, id, created);
        audits.push(saved_run(&state, id).await);
    }
    let [selected, other, last] = audits.try_into().expect("three saved Run audits");
    assert_eq!(selected["profile"], "brief");
    assert_eq!(
        selected["selection"],
        json!({"kind":"artifacts","artifactIds":["checkout","search"]})
    );
    assert_eq!(selected["jobs"], 2);
    assert_eq!(selected["maxExecutions"], 3);
    assert_eq!(selected["force"], true);
    assert!(selected.get("ignoreGates").is_none());
    let definitions = &selected["definitions"];
    assert_eq!(definitions["artifacts"].as_object().unwrap().len(), 3);
    assert_eq!(definitions["artifacts"]["input"]["basis"], true);
    assert!(definitions["artifacts"].get("unselected").is_none());
    assert_eq!(
        definitions["artifacts"]["checkout"]["path"],
        "scenarios/checkout"
    );
    assert_eq!(
        definitions["artifacts"]["search"]["path"],
        "scenarios/search"
    );
    assert_eq!(definitions["evals"][0]["declaration"]["title"], "Checkout");
    assert_eq!(
        definitions["evals"][0]["declaration"]["profile"]["command"],
        echo_program()
    );
    assert_eq!(
        definitions["evals"][0]["declaration"]["passSchema"],
        json!({"type":"object"})
    );
    assert_eq!(
        last["definitions"]["artifacts"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["checkout", "input"]
    );

    fs::remove_dir_all(&first).unwrap();
    // A historical query must not discover even the surviving repository's declarations.
    support::declaration::write(&(second), second.join("index.artf"), "not JSON").unwrap();
    let before = support::declaration::read_file(state.join("state.sqlite")).unwrap();
    let all = query(&first, &state, &["run", "list", "--all"], 0);
    assert_eq!(all.as_array().unwrap().len(), 3);
    for (summary, (saved, counts)) in all.as_array().unwrap().iter().zip([
        (&last, json!({"GREEN":1})),
        (&other, json!({"GREEN":1,"RED":1,"DEFERRED":1})),
        (&selected, json!({"GREEN":2})),
    ]) {
        for field in [
            "id",
            "repoPath",
            "createdAt",
            "completedAt",
            "status",
            "comment",
        ] {
            assert_eq!(summary[field], saved[field], "{field}");
        }
        assert_eq!(summary["counts"], counts);
    }
    assert_eq!(all[0]["comment"], "Checking out — étape 2");
    assert!(
        all[1].get("comment").is_none(),
        "a Run without a comment lists none"
    );
    for (saved, status, evals) in [
        (
            &selected,
            "GREEN",
            &[("checkout/review", "GREEN"), ("search/review", "GREEN")][..],
        ),
        (&last, "GREEN", &[("checkout/review", "GREEN")]),
        (
            &other,
            "WAITING_HUMAN",
            &[
                ("other/pass", "GREEN"),
                ("other/fail", "RED"),
                ("notes/human", "DEFERRED"),
            ],
        ),
    ] {
        let reopened = query(
            &first,
            &state,
            &["run", "show", saved["id"].as_str().unwrap()],
            0,
        );
        assert_eq!(reopened["id"], saved["id"]);
        assert_eq!(reopened["status"], status);
        assert_eq!(reopened["createdAt"], saved["createdAt"]);
        let requests = reopened["requests"].as_array().unwrap();
        assert_eq!(requests.len(), evals.len());
        for &(eval, verdict) in evals {
            let request = requests
                .iter()
                .find(|request| request["evalId"] == eval)
                .unwrap();
            assert_eq!(request["status"], verdict);
            if eval.ends_with("/review") {
                assert_eq!(request["result"]["stdout"], "saved result\n");
            }
        }
        assert_eq!(reopened["definitions"], saved["definitions"]);
        for request in saved["requests"].as_array().unwrap() {
            let shown = query(
                &first,
                &state,
                &["request", "show", request["id"].as_str().unwrap()],
                0,
            );
            let eval = saved["definitions"]["evals"]
                .as_array()
                .unwrap()
                .iter()
                .find(|eval| eval["id"] == request["evalId"])
                .unwrap();
            assert_eq!(shown["definition"]["eval"], *eval);
            assert_eq!(
                shown["definition"]["artifact"],
                saved["definitions"]["artifacts"][request["target"].as_str().unwrap()]
            );
            assert_eq!(shown["result"], request["result"]);
        }
    }
    assert_eq!(
        selected["requests"][0]["result"]["stdout"],
        "saved result\n"
    );
    assert_eq!(query(&second, &state, &["run", "list"], 0), json!([all[1]]));
    assert_eq!(
        query(&first, &state, &["run", "list", "--repo-only"], 0),
        json!([all[0], all[2]])
    );
    let alias = root.path().join("alias");
    support::os::link_dir(&second, &alias);
    assert_eq!(query(&alias, &state, &["run", "list"], 0), json!([all[1]]));
    let current = Command::new(support::os::cli())
        .current_dir(&second)
        .arg("--state-dir")
        .arg(&state)
        .args(["run", "list", "--json"])
        .record_output()
        .unwrap();
    assert!(current.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&current.stdout).unwrap(),
        json!([all[1]])
    );
    assert_eq!(
        query(
            &first,
            &state,
            &["run", "list", "--all", "--limit", "1", "--offset", "1"],
            0
        ),
        json!([all[1]])
    );
    assert_eq!(
        query(
            &first,
            &state,
            &["run", "list", "--limit", "1", "--offset", "1"],
            0
        ),
        json!([all[2]])
    );
    assert_eq!(
        query(
            &first,
            &state,
            &["run", "list", "--all", "--offset", "3"],
            0
        ),
        json!([])
    );
    assert_eq!(
        query(&first, &state, &["run", "list", "--all", "--limit", "0"], 0),
        json!([])
    );
    let text =
        String::from_utf8(output(&first, &state, &["run", "list", "--all"], 0).stdout).unwrap();
    assert!(text.contains("DEFERRED=1"));
    assert!(text.contains("\tChecking out — étape 2\n"), "{text}");
    assert!(text.contains(selected["id"].as_str().unwrap()));
    assert_eq!(
        support::declaration::read_file(state.join("state.sqlite")).unwrap(),
        before
    );
}

#[test]
fn listing_missing_state_is_inert_and_flags_are_validated() {
    let root = support::os::tempdir();
    let repo = root.path().join("missing-repo");
    let state = root.path().join("missing-state");
    assert_eq!(query(&repo, &state, &["run", "list"], 0), json!([]));
    assert_eq!(
        query(&repo, &state, &["run", "list", "--all"], 0),
        json!([])
    );
    for args in [
        vec!["run", "list", "--all", "--repo-only"],
        vec!["run", "list", "--limit", "-1"],
        vec!["run", "list", "--offset", "-1"],
        vec!["run", "list", "--limit", "4294967296"],
    ] {
        assert!(query(&repo, &state, &args, 2)["error"].is_string());
    }
    let long = "x".repeat(201);
    for comment in ["", " padded", "two\nlines", long.as_str()] {
        assert!(
            query(&repo, &state, &["verify", "--all", "--comment", comment], 2)["error"]
                .is_string(),
            "{comment:?}"
        );
    }
    assert!(!state.exists());
}
