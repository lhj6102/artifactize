//! Definitions are an ordinary folder target through verification, reuse-only and status.
use serde_json::{Value, json};
use std::{fs, process::Command};
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;
#[path = "support/run.rs"]
mod owned_run;

#[test]
fn ordinary_definition_reviews_reuse_only_while_their_inputs_stay_unchanged() {
    let root = owned_run::Root::from(support::os::tempdir());
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    root.track(&state, &repo);
    fs::create_dir_all(repo.join(".artifactize")).unwrap();
    let graph_file = repo.join(".artifactize/project-graph.artf");
    fs::write(&graph_file, support::declaration::to_toml(json!({
        "path":".artifactize/",
        "evals":{"check":{"title":"Check project definitions","profile":{"kind":"runtime","command":support::os::true_program(),"args":[]},"payload":{"instruction":"Check the definitions."}}}
    })).unwrap()).unwrap();
    let app = repo.join(".artifactize/app.artf");
    fs::write(&app, "path = './'\nbasis = true\n").unwrap();
    fs::write(repo.join("data.txt"), "first contents").unwrap();
    let invoke = |args: &[&str], code: i32| -> Value {
        let output = Command::new(support::os::cli())
            .arg("--repo")
            .arg(&repo)
            .arg("--state-dir")
            .arg(&state)
            .args(args)
            .arg("--json")
            .env("ARTIFACTIZE_REMOTE", "off")
            .record_output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let first = invoke(&["verify", "--wait", "project-graph"], 0);
    let key = first["requests"][0]["key"].clone();
    assert_eq!(first["executionsStarted"], 1);
    assert!(key.as_str().is_some());
    fs::write(repo.join("data.txt"), "unrelated content changed").unwrap();
    let reused = invoke(
        &[
            "verify",
            "--wait",
            "project-graph",
            "--reuse-only",
            "runtime",
        ],
        0,
    );
    assert_eq!(reused["executionsStarted"], 0);
    assert_eq!(
        reused["requests"][0]["executionId"],
        first["requests"][0]["executionId"]
    );
    // Tags are definition content too, not an excluded graph-specific option.
    fs::write(&app, "path = './'\nbasis = true\ntags = ['new-tag']\n").unwrap();
    let stale = invoke(
        &[
            "verify",
            "--wait",
            "project-graph",
            "--reuse-only",
            "runtime",
        ],
        4,
    );
    assert_eq!(stale["executionsStarted"], 0);
    let changed = invoke(&["verify", "--wait", "--all"], 0);
    assert_eq!(changed["executionsStarted"], 1);
    assert_ne!(changed["requests"][0]["key"], key);
    let status = invoke(&["status", "--all"], 0);
    assert_eq!(status["satisfied"], true);
    assert!(
        status["evals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|eval| eval["id"] == "project-graph/check"
                && eval["key"] == changed["requests"][0]["key"])
    );
    fs::write(&graph_file, "path = '.artifactize/'\nbasis = true\n").unwrap();
    let basis = invoke(&["verify", "--wait", "--all"], 0);
    assert_eq!(basis["validation"]["satisfied"], true);
    assert_eq!(basis["executionsStarted"], 0);
}
