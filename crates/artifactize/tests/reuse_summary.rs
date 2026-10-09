mod support;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};
use support::{FakeProvider, openai, os::bin};

/// Checkouts of one project sharing a state directory, with a fake OpenAI provider
/// that passes every Agent review.
struct Fixture {
    root: tempfile::TempDir,
    provider: FakeProvider,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("home")).unwrap();
        let provider = FakeProvider::start(|request| {
            assert_eq!(request.path, "/v1/responses");
            let verdict = json!({"verdict":"GREEN","note":"The docs match the API."});
            openai::completed(
                request,
                vec![openai::message(&verdict.to_string())],
                openai::usage(120, 30),
            )
        });
        Self { root, provider }
    }

    /// `versions` are the api, web, style, docs and brand contents; each one is its fingerprint.
    fn checkout(&self, name: &str, versions: [&str; 5]) -> PathBuf {
        let repo = self.root.path().join(name);
        let runtime = json!({"kind":"runtime","command":bin("/bin/true"),"args":[]});
        // web/tests is RED when its version says "broken".
        let web = json!({
            "kind":"runtime",
            "command":bin("/bin/sh"),
            "args":["-c","! grep -q broken version"],
        });
        let agent = json!({
            "kind":"agent",
            "backend":"openai",
            "model":"fake-exact-model",
            "reasoning":"high",
            "timeout_ms":15000,
        });
        let artifacts = [
            ("api", "tests", runtime.clone(), "Run the API tests."),
            ("web", "tests", web, "Run the web tests."),
            ("style", "contrast", runtime, "Check {web} colors."),
            ("docs", "review", agent, "Review the docs against {api}."),
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
            let mut declaration = json!({
                "id":eval,
                "title":eval,
                "profile":profile,
                "payload":{"instruction":instruction},
            });
            if artifact == "docs" {
                declaration["pass_schema"] = json!({
                    "type":"object",
                    "properties":{"note":{"type":"string"}},
                    "required":["note"],
                });
            }
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
            .env("ARTIFACTIZE_OPENAI_BASE_URL", self.provider.openai_base())
            .env("OPENAI_API_KEY", "fake-openai-key")
            .env("ARTIFACTIZE_REMOTE", "off")
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
    support::declaration::write(folder.join("index.artf"), declaration.to_string()).unwrap();
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

/// A reused tally: none of these results came from another profile.
fn reused(total: u64, runtime: u64, agent: u64, human: u64) -> Value {
    let mut tally = tally(total, runtime, agent, human);
    tally["otherProfile"] = json!(0);
    tally
}

/// A verify that records the Human request and stops waiting for it at once (exit 3).
const RECORD: [&str; 4] = ["verify", "--all", "--timeout-ms", "1"];

fn line<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.lines()
        .find(|line| line.trim_start().starts_with(prefix))
        .unwrap_or_else(|| panic!("no {prefix} line in:\n{text}"))
}

#[test]
fn merge_run_marks_reuse_counts_savings_and_matches_the_status_prediction() {
    // Each eval's key covers its own Artifact and the ones it names: style names web, docs
    // names api, and brand names docs and style.
    let fixture = Fixture::new();
    // Branch A changes api; its first Run executes everything and waits for the Human.
    let a = fixture.checkout("a", ["a", "base", "base", "base", "base"]);
    let first = fixture.json(&a, &RECORD, 3);
    assert_eq!(first["summary"]["executed"], tally(5, 3, 1, 1));
    assert_eq!(first["summary"]["reused"], reused(0, 0, 0, 0));
    assert_eq!(first["usage"]["saved"], json!({}));
    fixture.sign(&a, &first);

    // Branch B changes api and web, so docs and style review again; the Human signoff of the
    // unchanged docs and style is reused.
    let b = fixture.checkout("b", ["b", "b", "base", "base", "base"]);
    let second = fixture.json(&b, &["verify", "--all"], 0);
    assert_eq!(second["summary"]["executed"], tally(4, 3, 1, 0));
    assert_eq!(second["summary"]["reused"], reused(1, 0, 0, 1));
    let review = request(&second, "docs/review");
    assert_eq!(
        review["result"],
        json!({"verdict":"GREEN","note":"The docs match the API."})
    );
    let spent = &second["usage"]["spent"];
    assert_eq!(spent["inputTokens"], 120, "{spent}");
    assert_eq!(&second["summary"]["usage"], spent);
    // Each branch's Agent review went to the fake once; nothing else was asked.
    let calls = fixture.provider.requests();
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|call| call.body["model"] == "fake-exact-model"
                && call.headers["authorization"] == "Bearer fake-openai-key")
    );

    // The merge takes api from B and resolves web anew.
    let merge = fixture.checkout("merge", ["b", "merged", "base", "base", "base"]);
    let status = fixture.json(&merge, &["status"], 1);
    let brand = status["evals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|eval| eval["id"] == "brand/signoff");
    assert!(
        brand.unwrap()["reason"]
            .as_str()
            .unwrap()
            .ends_with("its gates still wait for: style/contrast"),
        "{status}"
    );
    let text = fixture.text(&merge, &["status"], 1);
    assert_eq!(
        line(&text, "Verify actions:"),
        "Verify actions: will execute 1, will reuse 3, wait 1, blocked 0"
    );

    let text = fixture.text(&merge, &["verify", "--all"], 0);
    let id = line(&text, "Run:").strip_prefix("Run: ").unwrap();
    let run = fixture.json(&merge, &["run", "show", id], 0);
    // style waits for web's new result, then reviews again because its key covers web.
    let mut expected = predicted(&status);
    assert_eq!(expected["style/contrast"], "wait");
    expected.insert("style/contrast".into(), "execute".into());
    assert_eq!(expected, taken(&run));
    assert_eq!(taken(&run)["web/tests"], "execute");
    assert_eq!(run["executionsStarted"], 2);
    assert_eq!(run["summary"]["executed"], tally(2, 2, 0, 0));
    assert_eq!(run["summary"]["reused"], reused(3, 1, 1, 1));
    assert_eq!(
        line(&text, "Summary:"),
        "Summary: executed 2 (runtime 2, agent 0, human 0), reused 3 (runtime 1, agent 1, human 1)"
    );
    for (eval, source) in [
        ("api/tests", &second),
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

    // Reused Agent usage is saved, not spent, and the provider was not asked again.
    assert_eq!(fixture.provider.requests().len(), 2);
    assert_eq!(run["usage"], json!({"spent":{},"saved":spent}));
    assert!(line(&text, "Usage:").starts_with("Usage: spent none; saved "));
    let reused_review = request(&run, "docs/review");
    assert!(reused_review["usage"].is_null());
    assert_eq!(reused_review["reusedUsage"], review["usage"]);
    let shown = fixture.json(
        &merge,
        &["request", "show", reused_review["id"].as_str().unwrap()],
        0,
    );
    assert_eq!(shown["summary"]["attempts"], 0);
    assert_eq!(shown["summary"]["usage"], json!({}));
    assert_eq!(shown["summary"]["usageState"], "none");
}
#[test]
fn status_predicts_cached_results_behind_a_red_dependency_as_reuse() {
    let fixture = Fixture::new();
    let base = fixture.checkout("base", ["base"; 5]);
    let first = fixture.json(&base, &RECORD, 3);
    fixture.sign(&base, &first);

    // web turns RED. style names web, so its key changes; brand names style and docs, whose
    // fingerprints did not change, so it keeps its key. The first Run executes web; the second
    // finds its RED result cached.
    let red = fixture.checkout("red", ["base", "broken", "base", "base", "base"]);
    let status = fixture.json(&red, &["status"], 1);
    assert_eq!(predicted(&status)["style/contrast"], "wait");
    for executed in [1, 0] {
        let run = fixture.json(&red, &["verify", "--all"], 1);
        assert_eq!(run["summary"]["executed"]["total"], executed);
        assert_eq!(request(&run, "web/tests")["status"], "RED");
        assert_eq!(taken(&run)["style/contrast"], "blocked");
        assert_eq!(taken(&run)["brand/signoff"], "reuse");
    }
    let status = fixture.json(&red, &["status"], 1);
    let run = fixture.json(&red, &["verify", "--all"], 1);
    assert_eq!(predicted(&status), taken(&run), "{status}");
    // verify attaches cached results behind a blocked gate, so status says reuse, not blocked.
    let row = |eval: &str| {
        status["evals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == eval)
            .unwrap()
            .clone()
    };
    let brand = row("brand/signoff");
    assert_eq!(
        (&brand["state"], &brand["action"]),
        (&json!("BLOCKED"), &json!("reuse"))
    );
    assert!(
        brand["reason"]
            .as_str()
            .unwrap()
            .ends_with("blocked by RED: style/contrast"),
        "{brand}"
    );
    let style = row("style/contrast");
    assert_eq!(
        (&style["state"], &style["action"]),
        (&json!("BLOCKED"), &json!("blocked"))
    );
    assert_eq!(style["changes"]["summary"], "dependency web changed");
    let text = fixture.text(&red, &["status"], 1);
    assert!(
        line(&text, "brand/signoff").ends_with("BLOCKED — reuse"),
        "{text}"
    );
    assert_eq!(
        line(&text, "Verify actions:"),
        "Verify actions: will execute 0, will reuse 4, wait 0, blocked 1"
    );
}

#[test]
fn an_agent_model_reasoning_or_limit_change_alone_reuses_the_review() {
    let fixture = Fixture::new();
    let repo = fixture.checkout("base", ["base"; 5]);
    let first = fixture.json(&repo, &RECORD, 3);
    let review = request(&first, "docs/review").clone();
    assert_eq!(review["status"], "GREEN");
    let calls = fixture.provider.requests().len();
    assert!(calls > 0);

    // Another backend setting is an execution option, not part of the reuse key.
    let path = repo.join("docs/index.artf");
    let mut declaration: Value = support::declaration::read(fs::read(&path).unwrap()).unwrap();
    declaration["evals"][0]["profile"] = json!({"kind":"agent","backend":"openai",
        "model":"fake-other-model","reasoning":"low","timeout_ms":60000,"max_tokens":4000});
    support::declaration::write(&path, declaration.to_string()).unwrap();
    let status = fixture.json(&repo, &["status"], 1);
    let docs = status["evals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|eval| eval["id"] == "docs/review")
        .unwrap();
    assert_eq!(docs["action"], "reuse");
    assert!(
        docs["reason"]
            .as_str()
            .unwrap()
            .contains("produced by profile openai fake-exact-model high"),
        "{docs}"
    );
    let text = fixture.text(&repo, &RECORD, 3);
    assert!(
        line(&text, "docs/review").ends_with(&format!(
            "]: GREEN (reused from {}, profile openai fake-exact-model high)",
            first["id"].as_str().unwrap()
        )),
        "{text}"
    );
    assert_eq!(fixture.provider.requests().len(), calls);
    let id = line(&text, "Run:").strip_prefix("Run: ").unwrap();
    let run = fixture.json(&repo, &["run", "show", id], 0);
    let reused = request(&run, "docs/review");
    assert_eq!(reused["executionId"], review["executionId"]);
    assert_eq!(reused["profile"]["model"], "fake-exact-model");
    assert_eq!(reused["requestedProfile"]["model"], "fake-other-model");
    assert_eq!(
        reused["options"],
        json!({"backend":"openai","model":"fake-exact-model","reasoning":"high","timeoutMs":15000})
    );
    assert_eq!(run["summary"]["reused"]["otherProfile"], 1);
}

#[test]
fn reuse_only_runs_tests_and_takes_reviews_only_from_the_cache() {
    let fixture = Fixture::new();
    let ci = ["verify", "--all", "--reuse-only", "agent,human"];
    // Nothing is cached: the runtime evals execute, the Agent review is not executed, and the
    // sign-off that names it waits behind it. No model or person is asked.
    let base = fixture.checkout("base", ["base"; 5]);
    let first = fixture.json(&base, &ci, 4);
    assert_eq!(first["status"], "INCOMPLETE");
    assert_eq!(first["reuseOnly"], json!(["agent", "human"]));
    assert_eq!(first["summary"]["executed"], tally(3, 3, 0, 0));
    assert_eq!(fixture.provider.requests().len(), 0);
    let review = request(&first, "docs/review");
    assert_eq!(review["status"], "STALE");
    assert!(review["executionId"].is_null());
    assert!(
        review["blockedReason"]
            .as_str()
            .unwrap()
            .starts_with("Not reused (--reuse-only agent)"),
        "{review}"
    );
    assert_eq!(
        request(&first, "brand/signoff")["status"],
        "WAIT_DEPENDENCY"
    );
    let saved = fixture.json(&base, &["request", "list"], 0);
    assert!(
        saved
            .as_array()
            .unwrap()
            .iter()
            .all(|request| request["status"] != "WAITING_HUMAN")
    );
    let text = fixture.text(&base, &ci, 4);
    assert!(
        line(&text, "docs/review").contains("]: STALE — Not reused (--reuse-only agent)"),
        "{text}"
    );
    assert!(line(&text, "Reason:").contains("not executed (--reuse-only)"));

    // A local verify produces the review and the sign-off; CI then reuses every result.
    let local = fixture.json(&base, &RECORD, 3);
    fixture.sign(&base, &local);
    assert_eq!(fixture.provider.requests().len(), 1);
    let cached = fixture.json(&base, &ci, 0);
    assert_eq!(cached["summary"]["executed"], tally(0, 0, 0, 0));
    assert_eq!(cached["summary"]["reused"], reused(5, 3, 1, 1));

    // A change to api executes its unlisted runtime eval again, and the review that names api
    // has nothing to reuse. The sign-off's key covers docs and style, not api: it is reused.
    let changed = fixture.checkout("changed", ["changed", "base", "base", "base", "base"]);
    let run = fixture.json(&changed, &ci, 4);
    assert_eq!(run["summary"]["executed"], tally(1, 1, 0, 0));
    assert_eq!(run["summary"]["reused"], reused(3, 2, 0, 1));
    assert_eq!(request(&run, "api/tests")["status"], "GREEN");
    assert_eq!(request(&run, "docs/review")["status"], "STALE");
    assert_eq!(fixture.provider.requests().len(), 1);
}
