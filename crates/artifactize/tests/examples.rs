//! The example projects run offline from the built binary, as their READMEs describe.
//! Commands run from each example folder with an isolated state home; no Agent eval runs.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tempfile::TempDir;

mod support;

fn example(name: &str) -> PathBuf {
    support::os::canonical(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(name),
    )
}

struct Session(TempDir);

impl Session {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn command(&self, repo: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            // The examples name `grep` and `cat` for PATH to find.
            .env("PATH", support::os::path())
            .current_dir(repo)
            .args(args)
            .env("ARTIFACTIZE_STATE_HOME", self.0.path().join("state"))
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY");
        command
    }

    fn output(&self, repo: &Path, args: &[&str], code: i32) -> Output {
        expect(self.command(repo, args).output().unwrap(), args, code)
    }

    fn json(&self, repo: &Path, args: &[&str], code: i32) -> Value {
        parse(&self.output(repo, args, code))
    }

    /// A private copy for edits that turn an example RED.
    fn copy(&self, name: &str) -> PathBuf {
        let target = self.0.path().join(name);
        support::copy_directory(&example(name), &target);
        target
    }
}

fn expect(output: Output, args: &[&str], code: i32) -> Output {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{args:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn parse(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Every file below `root`, so tests notice anything an example writes into its own folder.
fn files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path.strip_prefix(root).unwrap().to_owned());
            }
        }
    }
    found.sort();
    found
}

fn append(path: &Path, text: &str) {
    let mut contents = fs::read_to_string(path).unwrap();
    contents.push_str(text);
    fs::write(path, contents).unwrap();
}

fn requests(run: &Value) -> BTreeMap<String, Value> {
    run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|request| {
            (
                request["evalId"].as_str().unwrap().to_owned(),
                request.clone(),
            )
        })
        .collect()
}

fn statuses(run: &Value) -> BTreeMap<String, String> {
    requests(run)
        .into_iter()
        .map(|(id, request)| (id, request["status"].as_str().unwrap().to_owned()))
        .collect()
}

fn executions(run: &Value) -> BTreeMap<String, Value> {
    requests(run)
        .into_iter()
        .map(|(id, request)| (id, request["executionId"].clone()))
        .collect()
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn every_example_passes_static_queries_and_reports_status() {
    let session = Session::new();
    for (name, artifacts, evals) in [
        ("runtime-relations", 4, 3),
        ("agent-tools", 2, 2),
        ("family", 4, 3),
    ] {
        let repo = example(name);
        let before = files(&repo);
        assert_eq!(
            session.json(&repo, &["config", "check", "--json"], 0),
            json!({"artifacts": artifacts, "evals": evals, "ok": true}),
            "{name}"
        );
        let graph = session.json(&repo, &["config", "graph", "--json"], 0);
        assert_eq!(graph["evals"].as_array().unwrap().len(), evals, "{name}");
        let status = session.json(&repo, &["status", "--json"], 1);
        assert_eq!(status["satisfied"], false, "{name}");
        session.output(&repo, &["status"], 1);
        assert_eq!(
            files(&repo),
            before,
            "{name} must not write into its folder"
        );
    }

    let graph = session.json(
        &example("runtime-relations"),
        &["config", "graph", "--json"],
        0,
    );
    let mut kinds: Vec<_> = graph["relations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|relation| relation["kind"].as_str().unwrap())
        .collect();
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds, ["argv", "child", "instruction", "mount"]);
    assert_eq!(graph["artifacts"]["glossary"]["basis"], true);
    assert_eq!(
        graph["artifacts"]["guide"]["children"],
        json!({"intro": "intro", "usage": "usage"})
    );

    let graph = session.json(
        &example("family"),
        &["config", "graph", "posts", "--json"],
        0,
    );
    assert_eq!(
        graph["families"]["posts"]["artifactIds"],
        json!(["release-notes", "tip", "welcome"])
    );
}

#[test]
fn runtime_relations_reuses_fingerprints_and_turns_red() {
    let session = Session::new();
    let repo = example("runtime-relations");
    let before = files(&repo);
    let first = session.json(&repo, &["verify", "--all", "--json"], 0);
    assert_eq!(first["executionsStarted"], 3);
    let second = session.json(&repo, &["verify", "--all", "--json"], 0);
    assert_eq!(second["executionsStarted"], 0);
    assert_eq!(executions(&second), executions(&first));
    let status = session.json(&repo, &["status", "--json"], 0);
    assert_eq!(status["counts"]["reuse"], 3);
    let run = session.json(&repo, &["run", "show", first["id"].as_str().unwrap()], 0);
    assert_eq!(
        statuses(&run),
        map(&[
            ("guide/terms", "GREEN"),
            ("intro/heading", "GREEN"),
            ("usage/heading", "GREEN"),
        ])
    );
    assert_eq!(files(&repo), before);

    // README: an undefined bold term makes guide/terms RED; intro/heading is reused.
    let copy = session.copy("runtime-relations");
    let page = copy.join("guide/usage/page.md");
    append(&page, "Results live in the **cache**.\n");
    let status = session.json(&copy, &["status", "--json"], 1);
    let summaries: BTreeMap<_, _> = status["evals"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|eval| Some((eval["id"].as_str()?, eval["changes"]["summary"].as_str()?)))
        .collect();
    assert_eq!(
        summaries,
        BTreeMap::from([
            ("guide/terms", "dependency usage changed"),
            ("usage/heading", "changed: page.md"),
        ])
    );
    let red = session.json(&copy, &["verify", "--all", "--json"], 1);
    assert_eq!(
        statuses(&red),
        map(&[
            ("guide/terms", "RED"),
            ("intro/heading", "GREEN"),
            ("usage/heading", "GREEN"),
        ])
    );
    assert_eq!(red["executionsStarted"], 2);
    let terms = &requests(&red)["guide/terms"];
    assert!(
        terms["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("cache: NOT DEFINED"),
        "{terms}"
    );

    // README: a page without its heading is RED and blocks its parent.
    let contents = fs::read_to_string(&page).unwrap();
    fs::write(&page, contents.split_once('\n').unwrap().1).unwrap();
    let blocked = session.json(&copy, &["verify", "--all", "--json"], 1);
    assert_eq!(
        statuses(&blocked),
        map(&[
            ("guide/terms", "BLOCKED"),
            ("intro/heading", "GREEN"),
            ("usage/heading", "RED"),
        ])
    );
}

#[test]
fn family_selectors_expand_instances_and_reuse_each_instance() {
    let session = Session::new();
    let repo = example("family");
    let before = files(&repo);
    let family = session.json(&repo, &["verify", "posts", "--json"], 0);
    assert_eq!(
        statuses(&family),
        map(&[
            ("release-notes/style", "GREEN"),
            ("tip/style", "GREEN"),
            ("welcome/style", "GREEN"),
        ])
    );
    let by_eval = requests(&family);
    assert_eq!(
        by_eval["tip/style"]["title"],
        "The brief post fits in 40 words and follows the house style"
    );
    assert_eq!(
        by_eval["tip/style"]["argv"].as_array().unwrap().last(),
        Some(&json!("40"))
    );
    assert_eq!(
        by_eval["welcome/style"]["argv"].as_array().unwrap().last(),
        Some(&json!("120"))
    );

    let instance = session.json(&repo, &["verify", "tip", "--json"], 0);
    assert_eq!(statuses(&instance), map(&[("tip/style", "GREEN")]));
    assert_eq!(instance["executionsStarted"], 0);
    let evals = session.json(
        &repo,
        &["verify", "--evals", "welcome/style,tip/style", "--json"],
        0,
    );
    assert_eq!(evals["executionsStarted"], 0);
    let status = session.json(&repo, &["status", "posts", "--json"], 0);
    assert_eq!(status["counts"]["reuse"], 3);
    assert_eq!(files(&repo), before);

    // README: a banned word in tip.md re-runs only tip/style.
    let copy = session.copy("family");
    append(
        &copy.join("posts/tip.md"),
        "It helps you leverage every run.\n",
    );
    let red = session.json(&copy, &["verify", "posts", "--json"], 1);
    assert_eq!(
        statuses(&red),
        map(&[
            ("release-notes/style", "GREEN"),
            ("tip/style", "RED"),
            ("welcome/style", "GREEN"),
        ])
    );
    assert_eq!(red["executionsStarted"], 1);
}

#[test]
fn agent_tools_checks_declared_tools_and_completes_a_human_signoff() {
    let session = Session::new();
    // The section tool runs /bin/sh; a copy names the stand-in where it is not at that path.
    let repo = if support::os::stand_ins() {
        session.copy("agent-tools")
    } else {
        example("agent-tools")
    };
    let before = files(&repo);

    // A stub keeps the static launch check independent of the host; nothing is launched.
    let bin = session.0.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fs::write(bin.join("xdg-open"), "#!/bin/sh\nexit 0\n").unwrap();
    support::os::make_executable(&bin.join("xdg-open"));
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&support::os::path())),
    )
    .unwrap();
    let output = session
        .command(&repo, &["tools", "check"])
        .env("PATH", path)
        .output()
        .unwrap();
    let report = parse(&expect(output, &["tools", "check"], 0));
    assert_eq!(report["ok"], true, "{report}");
    let names: Vec<Vec<&str>> = report["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scope| {
            scope["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["name"].as_str().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(
        names,
        [
            vec![
                "coverage_spec",
                "grep_spec",
                "read_spec",
                "section_spec",
                "view_image_spec"
            ],
            vec!["notes_spec", "open_spec"],
        ]
    );

    // The declared json and plain tools run locally through an explicit check.
    let execute = |tool: &str, args: &str| {
        let args = [
            "tools",
            "check",
            "--execute",
            "--artifact",
            "spec",
            "--audience",
            "agent",
            "--tool",
            tool,
            "--args",
            args,
        ];
        session.json(&repo, &args, 0)["result"]["content"][0].clone()
    };
    let coverage = execute("coverage", "{}");
    assert_eq!(coverage["type"], "json");
    assert_eq!(coverage["data"]["uncited"], json!([]));
    assert_eq!(
        coverage["data"]["requirements"].as_array().unwrap().len(),
        4
    );
    // section.sh reads lines and matches patterns as only a POSIX shell does.
    if !support::os::stand_ins() {
        let section = execute("section", r#"{"title":"Dry run"}"#);
        assert!(
            section["text"]
                .as_str()
                .unwrap()
                .starts_with("## Dry run\n")
        );
    }
    let image = execute("view_image", r#"{"path":"diagram.png"}"#);
    assert_eq!(image["mimeType"], "image/png");

    // README: the Human sign-off through request claim, tool and submit, while verify waits.
    let signoff = ["verify", "--eval", "spec/signoff", "--json"];
    let verify = session
        .command(&repo, &signoff)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(10));
    let run_id = loop {
        let listed = session.json(&repo, &["request", "list", "--json"], 0);
        // The request is saved QUEUED first, then turns WAITING_HUMAN.
        if let Some(request) = listed.as_array().unwrap().first()
            && request["status"] == "WAITING_HUMAN"
        {
            break request["runId"].as_str().unwrap().to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "the sign-off request did not start waiting: {listed}"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let listed = session.json(&repo, &["request", "list", "--run", &run_id, "--json"], 0);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let request = listed[0]["id"].as_str().unwrap();
    let reviewer = ["--reviewer", "example"];
    session.output(
        &repo,
        &[&["request", "claim", request][..], &reviewer].concat(),
        0,
    );
    let notes = session.output(
        &repo,
        &[&["request", "tool", request, "notes_spec"][..], &reviewer].concat(),
        0,
    );
    assert!(String::from_utf8_lossy(&notes.stdout).contains("## Open questions"));
    let submit = [
        &["request", "submit", request, "--verdict", "GREEN"][..],
        &["--fields", r#"{"approved":true}"#],
        &reviewer,
    ]
    .concat();
    session.output(&repo, &submit, 0);
    // The waiting verify records the sign-off; the Run stays INCOMPLETE without spec/review.
    let run = parse(&expect(verify.wait_with_output().unwrap(), &signoff, 4));
    assert_eq!(run["status"], "INCOMPLETE");
    assert_eq!(run, session.json(&repo, &["run", "show", &run_id], 0));
    assert_eq!(statuses(&run), map(&[("spec/signoff", "GREEN")]));
    assert_eq!(requests(&run)["spec/signoff"]["result"]["approved"], true);
    assert_eq!(files(&repo), before);
}
