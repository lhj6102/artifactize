use serde_json::json;

use super::*;

fn old(name: &str, table: Value) -> Old {
    Old {
        file: PathBuf::from(format!("{name}.artf")),
        name: name.into(),
        table,
    }
}

fn read_back(converted: &Converted) -> crate::config::RepoConfig {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".artifactize")).unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("README.md"), "# Readme\n").unwrap();
    for (file, text) in &converted.files {
        std::fs::write(root.path().join(".artifactize").join(file), text).unwrap();
    }
    let config = crate::config::read_workspace_config(root.path());
    let files: Vec<_> = converted.files.iter().map(|(_, text)| text.as_str()).collect();
    config.unwrap_or_else(|error| panic!("{error}\n\n{}", files.join("\n---\n")))
}

#[test]
fn old_declarations_read_back_as_the_same_project() {
    let converted = convert(
        &[
            old(
                "app",
                json!({
                    "path": "src/",
                    "tags": ["core"],
                    "fingerprint": {"files": ["**"], "ignore": ["*.tmp"]},
                    "views": {
                        "agent_tools": {
                            "test": {
                                "protocol": "json",
                                "input_schema": {"type": "object"},
                                "command": "cargo",
                                "args": ["test", "--manifest-path={app}/Cargo.toml"],
                                "description": "Test {artifactName}.",
                            },
                            "read": {"builtin": "read"},
                        },
                        "human_tools": {
                            "open": {"builtin": "open", "args": ["{artifactPath}"]},
                        },
                    },
                    "evals": {
                        "review": {
                            "title": "Matches the spec",
                            "red_at": "P1",
                            "profile": {"kind": "agent", "backend": "codex", "model": "gpt-5.5"},
                            "payload": {"instruction": "Compare {app} with {spec}; keep {not a name} and @{ as text."},
                        },
                        "1-tests": {
                            "title": "Tests pass",
                            "profile": {"kind": "runtime", "command": "cargo", "args": ["test", "{app}"]},
                            "payload": {"instruction": "Run the tests of {spec}."},
                        },
                        "signoff": {
                            "title": "Owner approves",
                            "profile": {"kind": "human"},
                            "payload": {"instruction": "Approve {app}."},
                            "fail_schema": {"type": "object"},
                        },
                    },
                }),
            ),
            old(
                "spec",
                json!({
                    "path": "README.md",
                    "type": "markdown",
                    "evals": [{
                        "id": "clear",
                        "title": "Clear",
                        "profile": {"kind": "agent", "backend": "codex", "model": "gpt-5.5"},
                        "payload": {"instruction": "Is {spec} clear?"},
                    }],
                }),
            ),
        ],
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .unwrap();
    let config = read_back(&converted);
    let app = &config.artifacts[&name("app")];
    assert_eq!(app.tags, ["core"]);
    assert_eq!(app.toolsets, ["app"]);
    let spec = &config.artifacts[&name("spec")];
    assert_eq!(spec.toolsets, ["markdown"]);
    assert_eq!(config.profiles.len(), 1, "one profile, shared");
    let review = config.evals.iter().find(|eval| eval.id.as_str() == "app/review").unwrap();
    assert_eq!(
        review.declaration.instruction(),
        Some("Compare app with spec; keep {not a name} and @{ as text.")
    );
    let tests = config.evals.iter().find(|eval| eval.id.as_str() == "app/e1-tests").unwrap();
    assert_eq!(tests.declaration.inputs(), [name("spec")]);
    let warnings = converted.warnings.join("\n");
    assert!(warnings.contains("app/1-tests"), "{warnings}");
    assert!(warnings.contains("fail_schema"), "{warnings}");
}

#[test]
fn identical_tools_are_one_shared_toolset_and_variants_named_profiles() {
    let views = json!({"agent_tools": {"read": {"builtin": "read"}}});
    let agent = |model: &str| json!({"kind": "agent", "backend": "codex", "model": model});
    let converted = convert(
        &[
            old("a", json!({"path": "src/", "views": views})),
            old(
                "b",
                json!({
                    "path": "README.md",
                    "views": views,
                    "evals": {"check": {
                        "title": "Check",
                        "profile": agent("gpt-5.5"),
                        "profile_variants": {"fast": agent("gpt-5.5-mini")},
                        "payload": {"instruction": "Check {b}."},
                    }},
                }),
            ),
        ],
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .unwrap();
    let names: Vec<_> = converted.files.iter().map(|(file, _)| file.clone()).collect();
    assert_eq!(names, ["a.artf", "b.artf", "tools.artf", "profiles.artf"].map(PathBuf::from));
    let config = read_back(&converted);
    assert_eq!(config.artifacts[&name("b")].toolsets, ["a"]);
    let profiles: Vec<_> = config.profiles.keys().map(|name| name.as_str().to_owned()).collect();
    assert_eq!(profiles, ["fast", "gpt-5-5"]);
}

#[test]
fn old_references_are_bare_names_outside_escapes_quotes_and_nesting() {
    let source = r#"{first} {first}/file {a-b_9} \{escaped} \{also\} {after} {{doubled}} ${variable} { "key": "{json}" } {nested {ignored}} {'}'} {`}`} {"\\\"}"} {last} {unfinished {hidden}"#;
    assert_eq!(
        references(source),
        ["first", "a-b_9", "after", "last"]
    );
    assert_eq!(
        references(r"\{no} \\{yes} \\\{no} \\\\{again}"),
        ["yes", "again"]
    );
    assert_eq!(
        references("é{unicode} {_invalid} {9valid} {bad.name} { spaced }"),
        ["unicode", "9valid"]
    );
    assert_eq!(
        references(&format!("{{{}}} {{{}}}", "x".repeat(64), "y".repeat(65))),
        ["x".repeat(64)]
    );
}

fn name(text: &str) -> crate::config::ArtifactName {
    text.parse().unwrap()
}
