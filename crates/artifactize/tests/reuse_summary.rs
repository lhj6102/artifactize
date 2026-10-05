use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};

/// Checkouts of one project sharing a state directory.
struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("home")).unwrap();
        Self { root }
    }

    /// `versions` are the api, web, style, docs and brand contents; each one is its fingerprint.
    fn checkout(&self, name: &str, versions: [&str; 5]) -> PathBuf {
        let repo = self.root.path().join(name);
        let runtime = json!({"kind":"runtime","command":"/bin/true","args":[]});
        // web/tests is RED when its version says "broken".
        let web =
            json!({"kind":"runtime","command":"/bin/sh","args":["-c","! grep -q broken version"]});
        let artifacts = [
            ("api", "tests", runtime.clone(), "Run the API tests."),
            ("web", "tests", web, "Run the web tests."),
            ("style", "contrast", runtime.clone(), "Check {web} colors."),
            ("docs", "review", runtime, "Review the docs against {api}."),
            (
                "brand",
                "signoff",
                json!({"kind":"human"}),
                "Sign off {docs} and {style}.",
            ),
        ];
        write(&repo, json!({"name":"root","basis":true}));
        for ((artifact, eval, profile, instruction), version) in artifacts.into_iter().zip(versions)
        {
            let folder = repo.join(artifact);
            let declaration = json!({"id":eval,"title":eval,"profile":profile,"payload":{"instruction":instruction}});
            write(
                &folder,
                json!({"name":artifact,"evals":[declaration],
                    "fingerprint":{"script":{"command":"cat","args":["version"]}}}),
            );
            fs::write(folder.join("version"), format!("{artifact}-{version}\n")).unwrap();
        }
        repo
    }

    fn run(&self, repo: &Path, args: &[&str], code: i32) -> Output {
        let root = self.root.path();
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(root.join("state"))
            .args(args)
            .env("HOME", root.join("home"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn json(&self, repo: &Path, args: &[&str], code: i32) -> Value {
        let output = self.run(repo, &[args, &["--json"]].concat(), code);
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn text(&self, repo: &Path, args: &[&str], code: i32) -> String {
        String::from_utf8(self.run(repo, args, code).stdout).unwrap()
    }

    /// Submits the Human signoff that `run` recorded, publishing it for reuse.
    fn sign(&self, repo: &Path, run: &Value) {
        let id = request(run, "brand/signoff")["id"].as_str().unwrap();
        self.json(repo, &["request", "claim", id, "--reviewer", "alice"], 0);
        let submit = ["request", "submit", id, "--verdict", "GREEN"];
        self.json(repo, &[&submit[..], &["--reviewer", "alice"]].concat(), 0);
    }
}

fn write(folder: &Path, declaration: Value) {
    fs::create_dir_all(folder).unwrap();
    fs::write(folder.join("artifactize.json"), declaration.to_string()).unwrap();
}

fn request<'a>(run: &'a Value, eval: &str) -> &'a Value {
    run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|request| request["evalId"] == eval)
        .unwrap()
}

/// The action status predicts for each included eval.
fn predicted(status: &Value) -> BTreeMap<String, String> {
    let evals = status["evals"].as_array().unwrap().iter();
    evals
        .filter(|eval| eval["included"] == true)
        .map(|eval| {
            (
                eval["id"].as_str().unwrap().into(),
                eval["action"].as_str().unwrap().into(),
            )
        })
        .collect()
}

/// What verify did with each request, in status action terms.
fn taken(run: &Value) -> BTreeMap<String, String> {
    let requests = run["requests"].as_array().unwrap().iter();
    requests
        .map(|request| {
            let action = match &request["provenance"]["requestId"] {
                Value::Null if request["status"] == "BLOCKED" => "blocked",
                Value::Null => "wait",
                source if *source == request["id"] => "execute",
                _ => "reuse",
            };
            (request["evalId"].as_str().unwrap().into(), action.into())
        })
        .collect()
}

fn tally(total: u64, runtime: u64, agent: u64, human: u64) -> Value {
    json!({"total":total,"runtime":runtime,"agent":agent,"human":human})
}

fn line<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.lines()
        .find(|line| line.trim_start().starts_with(prefix))
        .unwrap_or_else(|| panic!("no {prefix} line in:\n{text}"))
}

#[test]
fn merge_run_marks_reuse_counts_savings_and_matches_the_status_prediction() {
    let fixture = Fixture::new();
    // Branch A changes api; its first Run executes everything and waits for the Human.
    let a = fixture.checkout("a", ["a", "base", "base", "base", "base"]);
    let first = fixture.json(&a, &["verify", "--all"], 4);
    assert_eq!(first["summary"]["executed"], tally(5, 4, 0, 1));
    assert_eq!(first["summary"]["reused"], tally(0, 0, 0, 0));
    assert_eq!(first["usage"]["saved"], json!({}));
    fixture.sign(&a, &first);

    // Branch B changes web and docs; unchanged style and the Human signoff are reused.
    let b = fixture.checkout("b", ["base", "b", "base", "b", "base"]);
    let second = fixture.json(&b, &["verify", "--all"], 0);
    assert_eq!(second["summary"]["executed"], tally(3, 3, 0, 0));
    assert_eq!(second["summary"]["reused"], tally(2, 1, 0, 1));

    // The merge takes api from A and docs from B, and resolves web anew.
    let merge = fixture.checkout("merge", ["a", "merged", "base", "b", "base"]);
    let status = fixture.json(&merge, &["status"], 1);
    let style = status["evals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|eval| eval["id"] == "style/contrast");
    assert!(
        style.unwrap()["reason"]
            .as_str()
            .unwrap()
            .ends_with("its gates still wait for: web/tests"),
        "{status}"
    );
    let text = fixture.text(&merge, &["status"], 1);
    assert_eq!(
        line(&text, "Verify actions:"),
        "Verify actions: will execute 1, will reuse 4, wait 0, blocked 0"
    );

    let text = fixture.text(&merge, &["verify", "--all"], 0);
    let id = line(&text, "Run:").strip_prefix("Run: ").unwrap();
    let run = fixture.json(&merge, &["run", "show", id], 0);
    assert_eq!(predicted(&status), taken(&run));
    assert_eq!(taken(&run)["web/tests"], "execute");
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["summary"]["executed"], tally(1, 1, 0, 0));
    assert_eq!(run["summary"]["reused"], tally(4, 3, 0, 1));
    assert_eq!(
        line(&text, "Summary:"),
        "Summary: executed 1 (runtime 1, agent 0, human 0), reused 4 (runtime 3, agent 0, human 1)"
    );
    for (eval, source) in [
        ("api/tests", &first),
        ("style/contrast", &first),
        ("brand/signoff", &first),
        ("docs/review", &second),
    ] {
        let source = source["id"].as_str().unwrap();
        assert!(
            line(&text, eval).ends_with(&format!("]: GREEN (reused from {source})")),
            "{text}"
        );
        assert_eq!(request(&run, eval)["provenance"]["runId"], source);
    }
    assert!(line(&text, "web/tests").ends_with("]: GREEN"), "{text}");
}

#[test]
fn status_predicts_cached_results_behind_a_red_dependency_as_reuse() {
    let fixture = Fixture::new();
    let base = fixture.checkout("base", ["base"; 5]);
    let first = fixture.json(&base, &["verify", "--all"], 4);
    fixture.sign(&base, &first);

    // web turns RED. The first Run executes it; the second finds its RED result cached.
    let red = fixture.checkout("red", ["base", "broken", "base", "base", "base"]);
    for executed in [1, 0] {
        let status = fixture.json(&red, &["status"], 1);
        let run = fixture.json(&red, &["verify", "--all"], 1);
        assert_eq!(predicted(&status), taken(&run), "{status}");
        assert_eq!(run["summary"]["executed"]["total"], executed);
        assert_eq!(request(&run, "web/tests")["status"], "RED");
    }
    // verify attaches cached results behind the RED gate, so status says reuse, not blocked.
    let status = fixture.json(&red, &["status"], 1);
    for (eval, gate) in [
        ("style/contrast", "web/tests"),
        ("brand/signoff", "style/contrast"),
    ] {
        let row = status["evals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == eval);
        let row = row.unwrap();
        assert_eq!(
            (&row["state"], &row["action"]),
            (&json!("BLOCKED"), &json!("reuse"))
        );
        assert!(
            row["reason"]
                .as_str()
                .unwrap()
                .ends_with(&format!("blocked by RED: {gate}"))
        );
    }
    let text = fixture.text(&red, &["status"], 1);
    assert!(
        line(&text, "style/contrast").ends_with("BLOCKED — reuse"),
        "{text}"
    );
    assert_eq!(
        line(&text, "Verify actions:"),
        "Verify actions: will execute 0, will reuse 5, wait 0, blocked 0"
    );

    // An uncached eval behind the RED gate is still blocked; its cached dependent is reused.
    let blocked = fixture.checkout("blocked", ["base", "broken", "new", "base", "base"]);
    let status = fixture.json(&blocked, &["status"], 1);
    let run = fixture.json(&blocked, &["verify", "--all"], 1);
    assert_eq!(predicted(&status), taken(&run));
    assert_eq!(taken(&run)["style/contrast"], "blocked");
    assert_eq!(taken(&run)["brand/signoff"], "reuse");
}
