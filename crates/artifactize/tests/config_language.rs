//! The `config` commands of the `.artf` language at the CLI seam: expand, symbols, migrate,
//! rename and the located errors `config check` prints.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

struct Fixture(
    PathBuf,
    #[expect(
        dead_code,
        reason = "keep the fixture directory alive until the test ends"
    )]
    tempfile::TempDir,
);

const DEFINITIONS: &str = r#"profile reviewer = agent(backend = "openai", model = "fixture")

artf! crate(dir) : files = {
  path = "crates/@{param dir}/"
  eval readable : reviewer = {
    title = "Readable"
    instruction = "Is @{artf self} readable next to @{artf docs}?"
  }
}

// The documentation.
artf docs : markdown = { path = "docs/guide.md" }
artf core = crate!(dir = "core")
"#;

impl Fixture {
    fn new() -> Self {
        let parent = support::os::environment_variable("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .join("test-fixtures");
        fs::create_dir_all(&parent).unwrap();
        let directory = support::os::tempdir_in(&parent, "language-");
        let fixture = Self(support::os::canonical(directory.path()), directory);
        fixture.write("crates/core/lib.rs", "");
        fixture.write("docs/guide.md", "# Guide\n");
        fixture
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(support::os::cli())
            .current_dir(&self.0)
            .env("ARTIFACTIZE_STATE_HOME", self.0.join("state-home"))
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

#[test]
fn expand_and_symbols_show_what_the_code_declares() {
    let fixture = Fixture::new();
    fixture.write(".artifactize/project.artf", DEFINITIONS);
    let output = fixture.run(&["config", "expand", "core"]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.starts_with("// .artifactize/project.artf:13 (by crate!)\n"),
        "{text}"
    );
    assert!(
        text.contains("artf core : files = {\n  path = \"crates/core/\""),
        "{text}"
    );
    assert!(
        text.contains("instruction = \"Is @{artf self} readable next to @{artf docs}?\""),
        "{text}"
    );
    let expanded = fixture.json(&["--json", "config", "expand"]);
    let names: Vec<_> = expanded
        .as_array()
        .unwrap()
        .iter()
        .map(|expansion| expansion["artifact"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["core", "docs"]);
    let symbols = fixture.json(&["--json", "config", "symbols"]);
    let crate_macro = symbols
        .as_array()
        .unwrap()
        .iter()
        .find(|symbol| symbol["name"] == "crate")
        .unwrap();
    assert_eq!(crate_macro["macro"], true);
    assert_eq!(crate_macro["params"][0]["name"], "dir");
    let missing = fixture.run(&["config", "expand", "nowhere"]);
    assert!(!missing.status.success());
}

#[test]
fn check_prints_the_line_and_the_macro_call_of_an_error() {
    let fixture = Fixture::new();
    fixture.write(
        ".artifactize/project.artf",
        &DEFINITIONS.replace(
            "artf core = crate!(dir = \"core\")",
            "artf core = crate!(dir = \"core\", size = 1)",
        ),
    );
    let output = fixture.run(&["config", "check"]);
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains("line 13") && error.contains("size"),
        "{error}"
    );
}

#[test]
fn migrate_converts_old_definitions_once_and_rename_follows_references() {
    let fixture = Fixture::new();
    fixture.write(
        ".artifactize/docs.artf",
        "path = \"docs/guide.md\"\ntype = \"markdown\"\n[evals.clear]\ntitle = \"Clear\"\nprofile = { kind = \"agent\", backend = \"openai\", model = \"fixture\" }\npayload = { instruction = \"Is {docs} clear?\" }\n",
    );
    let check = fixture.run(&["config", "check"]);
    let error = String::from_utf8(check.stderr).unwrap();
    assert!(error.contains("run artifactize config migrate"), "{error}");
    let migrated = fixture.json(&["--json", "config", "migrate"]);
    assert!(
        migrated["written"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".artifactize/docs.artf"),
        "{migrated}"
    );
    let again = fixture.run(&["config", "migrate"]);
    assert!(
        String::from_utf8(again.stdout)
            .unwrap()
            .contains("already in the .artf language")
    );
    let renamed = fixture.json(&["--json", "config", "rename", "docs", "guide"]);
    assert_eq!(renamed["moved"][1], ".artifactize/guide.artf");
    let text = fs::read_to_string(fixture.0.join(".artifactize/guide.artf")).unwrap();
    assert!(
        text.contains("artf guide : markdown") && text.contains("Is @{artf guide} clear?"),
        "{text}"
    );
    assert!(fixture.run(&["config", "check"]).status.success());
}

#[test]
fn migrate_gives_an_untyped_folder_an_agent_reads_the_file_tools() {
    // From before Artifact types: a folder came with the file tools its Agent eval reads it by.
    let fixture = Fixture::new();
    fixture.write(
        "crates/core/index.artf",
        "name = \"core\"\n[evals.readable]\ntitle = \"Readable\"\nprofile = { kind = \"agent\", backend = \"openai\", model = \"fixture\" }\npayload.instruction = \"Is {core} readable?\"\n",
    );
    fixture.json(&["--json", "config", "migrate"]);
    let text = fs::read_to_string(fixture.0.join(".artifactize/core.artf")).unwrap();
    assert!(text.contains("artf core : files = {"), "{text}");
    let check = fixture.run(&["config", "check"]);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
}
