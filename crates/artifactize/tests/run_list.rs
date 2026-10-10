use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};

use serde_json::{Value, json};
use support::os::bin;

mod support;

fn command(repo: &Path, state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
    command
        .arg("--repo")
        .arg(repo)
        .arg("--state-dir")
        .arg(state);
    command
}

fn output(repo: &Path, state: &Path, args: &[&str], code: i32) -> Output {
    let output = command(repo, state).args(args).output().unwrap();
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

fn counts(run: &Value) -> Value {
    let mut counts = BTreeMap::<&str, u64>::new();
    for request in run["requests"].as_array().unwrap() {
        *counts
            .entry(request["status"].as_str().unwrap())
            .or_default() += 1;
    }
    json!(counts)
}

#[test]
fn saved_definitions_and_paged_runs_survive_repository_removal() {
    let root = support::os::tempdir();
    let first = root.path().join("first");
    let second = root.path().join("second");
    let state = root.path().join("state");
    fs::create_dir_all(first.join("scenarios/checkout")).unwrap();
    fs::create_dir_all(first.join("scenarios/search")).unwrap();
    fs::create_dir(first.join("input")).unwrap();
    fs::create_dir(first.join("unselected")).unwrap();
    fs::create_dir(&second).unwrap();
    support::declaration::write(
        first.join("input/index.artf"),
        r#"{"name":"input","basis":true}"#,
    )
    .unwrap();
    support::declaration::write(
        first.join("unselected/index.artf"),
        r#"{"name":"unselected","basis":true}"#,
    )
    .unwrap();
    for (name, title) in [("checkout", "Checkout"), ("search", "Search")] {
        support::declaration::write(
            first.join(format!("scenarios/{name}/index.artf")),
            json!({
                "name":name,
                "evals":[
                    {
                        "id":"review",
                        "title":title,
                        "profile":{"kind":"runtime","command":bin("/bin/false"),"args":[]},
                        "profile_variants":{
                            "brief":{
                                "kind":"runtime",
                                "command":bin("/bin/echo"),
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
        second.join("index.artf"),
        json!({
            "name":"other",
            "evals":[
                {
                    "id":"pass",
                    "title":"Pass",
                    "profile":{"kind":"runtime","command":bin("/bin/true"),"args":[]},
                    "payload":{"instruction":"Pass."},
                },
                {
                    "id":"fail",
                    "title":"Fail",
                    "profile":{"kind":"runtime","command":bin("/bin/false"),"args":[]},
                    "payload":{"instruction":"Fail."},
                },
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
            "--artifacts",
            "checkout,search",
            "--profile",
            "brief",
            "--force",
            "--ignore-gates",
            "--jobs",
            "2",
            "--max-executions",
            "3",
        ],
        0,
    );
    // The Human wait times out at once, so the Run ends INCOMPLETE with its request open.
    let other = query(
        &second,
        &state,
        &["verify", "--all", "--timeout-ms", "1"],
        3,
    );
    let last = query(
        &first,
        &state,
        &["verify", "--eval", "checkout/review", "--profile", "brief"],
        0,
    );
    assert_eq!(selected["profile"], "brief");
    assert_eq!(
        selected["selection"],
        json!({"kind":"artifacts","artifactIds":["checkout","search"]})
    );
    assert_eq!(selected["jobs"], 2);
    assert_eq!(selected["maxExecutions"], 3);
    assert_eq!(selected["force"], true);
    assert_eq!(selected["ignoreGates"], true);
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
        bin("/bin/echo")
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
    support::declaration::write(second.join("index.artf"), "not JSON").unwrap();
    let before = fs::read(state.join("state.sqlite")).unwrap();
    let all = query(&first, &state, &["run", "list", "--all"], 0);
    assert_eq!(all.as_array().unwrap().len(), 3);
    for (summary, saved) in all
        .as_array()
        .unwrap()
        .iter()
        .zip([&last, &other, &selected])
    {
        for field in ["id", "repoPath", "createdAt", "completedAt", "status"] {
            assert_eq!(summary[field], saved[field], "{field}");
        }
        assert_eq!(summary["counts"], counts(saved));
    }
    assert_eq!(
        all[1]["counts"],
        json!({"GREEN":1,"RED":1,"WAITING_HUMAN":1})
    );
    for saved in [&selected, &last, &other] {
        let shown = query(
            &first,
            &state,
            &["run", "show", saved["id"].as_str().unwrap()],
            0,
        );
        assert_eq!(shown, *saved);
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
    let current = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .current_dir(&second)
        .arg("--state-dir")
        .arg(&state)
        .args(["run", "list", "--json"])
        .output()
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
    assert!(text.contains("WAITING_HUMAN=1"));
    assert!(text.contains(selected["id"].as_str().unwrap()));
    assert_eq!(fs::read(state.join("state.sqlite")).unwrap(), before);
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
    assert!(!state.exists());
}
