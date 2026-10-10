//! Agent results pin the files their tools execute (`execution_paths`) in their provenance.

mod support;

use std::{fs, path::Path, process::Command};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use support::{FakeProvider, openai, os::bin};

struct Fixture {
    root: tempfile::TempDir,
    provider: FakeProvider,
}

impl Fixture {
    /// A repository whose Agent eval has a tool executing `bin/sim` and the `rules` folder,
    /// and a fake OpenAI provider that passes every review.
    fn new() -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join("bin")).unwrap();
        fs::create_dir_all(repo.join("rules/nested")).unwrap();
        fs::create_dir(root.path().join("home")).unwrap();
        fs::write(repo.join("bin/sim"), "simulator v1").unwrap();
        fs::write(repo.join("rules/a.json"), "{}").unwrap();
        fs::write(repo.join("rules/nested/b.json"), "[]").unwrap();
        fs::create_dir(repo.join("app")).unwrap();
        fs::write(repo.join("app/page.md"), "# Page\n").unwrap();
        support::declaration::write(
            repo.join("app/index.artf"),
            json!({
                "name":"app",
                "fingerprint":{},
                "views":{
                    "agent_tools":{
                        "sim":{
                            "description":"Simulate {artifactName}.",
                            "input_schema":{"type":"object","additionalProperties":false},
                            "protocol":"json",
                            "command":bin("/bin/true"),
                            "args":[],
                            "execution_paths":["bin/sim","rules"],
                        },
                    },
                },
                "evals":[
                    {
                        "id":"review",
                        "title":"Review",
                        "profile":{"kind":"agent","backend":"openai","model":"fake-exact-model"},
                        "payload":{"instruction":"Review the page."},
                    },
                ],
            })
            .to_string(),
        )
        .unwrap();
        let provider = FakeProvider::start(|request| {
            openai::completed(
                request,
                vec![openai::message(&json!({"verdict":"GREEN"}).to_string())],
                openai::usage(10, 5),
            )
        });
        Self { root, provider }
    }

    fn repo(&self) -> std::path::PathBuf {
        self.root.path().join("repo")
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let root = self.root.path();
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(self.repo())
            .arg("--state-dir")
            .arg(root.join("state"))
            .args(args)
            .arg("--json")
            .env(support::os::home_env(), root.join("home"))
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
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

fn sha256(path: &Path) -> String {
    Sha256::digest(fs::read(path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn agent_results_pin_their_tools_execution_paths_and_keep_the_pins_on_reuse() {
    let fixture = Fixture::new();
    let repo = fixture.repo();
    let first = fixture.json(&["verify", "--all"], 0);
    let request = &first["requests"][0];
    assert_eq!(request["status"], "GREEN", "{request}");
    let pins = &request["provenance"]["executionPaths"]["sim_app"];
    assert_eq!(pins["bin/sim"], sha256(&repo.join("bin/sim")));
    let rules = pins["rules"].as_str().unwrap().to_owned();
    assert_eq!(rules.len(), 64);
    let key = request["key"].as_str().unwrap();
    let record = fixture.json(&["cache", "show", key], 0);
    assert_eq!(
        record["provenance"]["executionPaths"],
        request["provenance"]["executionPaths"]
    );
    let calls = fixture.provider.requests().len();

    // The pins are provenance, not part of the key: a reuse keeps the original pins.
    fs::write(repo.join("bin/sim"), "simulator v2").unwrap();
    let reused = fixture.json(&["verify", "--all"], 0);
    assert_eq!(reused["executionsStarted"], 0);
    assert_eq!(
        reused["requests"][0]["provenance"], request["provenance"],
        "a reuse shows which binary produced the verdict"
    );
    assert_eq!(fixture.provider.requests().len(), calls);

    // A new review pins the current files; any change in a pinned folder changes its pin.
    fs::write(repo.join("rules/nested/b.json"), "[1]").unwrap();
    let forced = fixture.json(&["verify", "--all", "--force"], 0);
    let pins = &forced["requests"][0]["provenance"]["executionPaths"]["sim_app"];
    assert_eq!(pins["bin/sim"], sha256(&repo.join("bin/sim")));
    assert_ne!(pins["rules"], rules.as_str());

    // A pinned path that cannot be hashed fails the review before the Agent is called.
    fs::remove_file(repo.join("bin/sim")).unwrap();
    let failed = fixture.json(&["verify", "--all", "--force"], 2);
    let request = &failed["requests"][0];
    assert_eq!(request["errorCode"], "PREPARATION_FAILED");
    assert!(
        request["error"]
            .as_str()
            .unwrap()
            .contains("Tool sim_app executionPaths bin/sim"),
        "{request}"
    );
    assert_eq!(fixture.provider.requests().len(), calls + 1);
}
