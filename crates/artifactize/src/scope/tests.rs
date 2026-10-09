use crate::config::CONFIG_FILE;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::*;
use crate::config::read_workspace_config;
use crate::test_os::{symlink_dir, symlink_file};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-fixtures")
            .join(format!("scope-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self(crate::platform::canonicalize(&root).unwrap())
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        crate::test_declaration::write(path, contents).unwrap();
    }

    fn artifact(&self, path: &str, value: Value) {
        self.write(&format!("{path}/{CONFIG_FILE}"), &value.to_string());
    }

    fn config(&self) -> RepoConfig {
        read_workspace_config(&self.0).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn eval(instruction: &str) -> Value {
    json!({"id":"check", "title":"Check", "profile":{"kind":"human"},
        "payload":{"instruction":instruction,"ownerField":{"unchanged":"{unknown}"}}})
}

#[test]
fn reference_parser_preserves_brace_groups_and_escape_parity() {
    let source = r#"{first} {first}/file {a-b_9} \{escaped} \{also\} {after} {{doubled}} ${variable} { "key": "{json}" } {nested {ignored}} {'}'} {`}`} {"\\\"}"} {last} {unfinished {hidden}"#;
    assert_eq!(
        instruction_references(source),
        ["first", "a-b_9", "after", "last"]
    );
    assert_eq!(
        instruction_references(r"\{no} \\{yes} \\\{no} \\\\{again}"),
        ["yes", "again"]
    );
    assert_eq!(
        instruction_references("é{unicode} {_invalid} {9valid} {bad.name} { spaced }"),
        ["unicode", "9valid"]
    );
    assert_eq!(
        instruction_references(&format!("{{{}}} {{{}}}", "x".repeat(64), "y".repeat(65))),
        ["x".repeat(64)]
    );
}

#[test]
fn ownership_is_nearest_marked_ancestor_without_inherited_declarations() {
    let fixture = Fixture::new();
    fixture.artifact(
        "outer",
        json!({
            "name":"parent",
            "evals":[eval("Inspect {child}.")],
            "views":{
                "agent_tools":{
                    "read":{"description":"Read","protocol":"json","command":"not-run","args":[]},
                },
            },
        }),
    );
    fixture.artifact("outer/unmarked/deep", json!({"name":"child"}));
    fixture.artifact(
        "outer/unmarked/deep/leaf",
        json!({"name":"leaf","basis":true}),
    );
    fixture.artifact("outer/unmarked/deeper", json!({"name":"neighbor"}));
    fixture.write("outer/unmarked/material.txt", "material");
    fixture.write("outer/unmarked/deep/input.txt", "child");
    let config = fixture.config();
    assert_eq!(
        config.artifacts["parent"].children,
        BTreeMap::from([
            ("unmarked/deep".into(), "child".into()),
            ("unmarked/deeper".into(), "neighbor".into()),
        ])
    );
    assert_eq!(
        config.artifacts["child"].children,
        BTreeMap::from([("leaf".into(), "leaf".into())])
    );
    assert_eq!(config.artifacts["parent"].views.agent_tools.len(), 1);
    assert!(config.artifacts["child"].views.agent_tools.is_empty());
    assert!(config.artifacts["child"].mounts.is_empty());
    assert_eq!(config.evals.len(), 1);
    let scope = artifact_scope(&config, &["parent"]).unwrap();
    assert_eq!(
        scope
            .resolve_path("parent", "unmarked/material.txt")
            .unwrap(),
        ScopedPath {
            artifact_id: "parent".into(),
            path: "unmarked/material.txt".into(),
        }
    );
    assert_eq!(
        scope
            .resolve_path("parent", "unmarked/deep/input.txt")
            .unwrap(),
        ScopedPath {
            artifact_id: "child".into(),
            path: "input.txt".into(),
        }
    );
    assert_eq!(
        scope
            .resolve_path("parent", "unmarked/deeper")
            .unwrap()
            .artifact_id,
        "neighbor"
    );
    assert_eq!(
        scope
            .resolve_path("parent", "unmarked/deep/leaf")
            .unwrap()
            .artifact_id,
        "leaf"
    );
    assert!(config.relations.contains(&Relation {
        source: "child".into(),
        target: "parent".into(),
        kind: RelationKind::Child {
            path: "unmarked/deep".into()
        },
    }));
    assert!(
        !config
            .relations
            .iter()
            .any(|edge| edge.source == "leaf" && edge.target == "parent")
    );
}

#[test]
fn aliases_keep_canonical_ids_and_cycles_consume_components() {
    let fixture = Fixture::new();
    fixture.artifact(
        "review",
        json!({"name":"review","mounts":{"one":"input","two":"input"},
        "evals":[eval("Read {one}/file, {two}, {review}; \\{literal} {{literal}}.")]}),
    );
    fixture.artifact("data", json!({"name":"input","mounts":{"back":"review"}}));
    fixture.write("data/file", "not instruction text");
    let config = fixture.config();
    let eval = &config.evals[0];
    assert_eq!(
        eval.references,
        BTreeMap::from([
            ("one".into(), "input".into()),
            ("two".into(), "input".into()),
            ("review".into(), "review".into()),
        ])
    );
    assert_eq!(eval.deps, ["input"]);
    let scope = eval_scope(&config, eval).unwrap();
    assert_eq!(scope.artifacts.len(), 2);
    let location = scope.resolve_path("review", "one/back/two/file").unwrap();
    assert_eq!(
        location,
        ScopedPath {
            artifact_id: "input".into(),
            path: "file".into()
        }
    );
    assert_eq!(
        scope
            .resolve_input(&config.root, "review", "two/file")
            .unwrap(),
        fixture.0.join("data/file")
    );
    let source = &eval.declaration.payload.as_ref().unwrap().instruction;
    assert_eq!(
        parse_artifact_instruction(source, &scope, &eval.references),
        vec![
            InstructionPart::Text("Read ".into()),
            InstructionPart::Artifact("input".into()),
            InstructionPart::Text("/file, ".into()),
            InstructionPart::Artifact("input".into()),
            InstructionPart::Text(", ".into()),
            InstructionPart::Artifact("review".into()),
            InstructionPart::Text("; \\{literal} {{literal}}.".into()),
        ]
    );
    assert_eq!(
        eval.declaration.payload.as_ref().unwrap().extra["ownerField"]["unchanged"],
        "{unknown}"
    );
    assert!(!source.contains("not instruction text"));
    assert!(!fixture.0.join("review/one").exists());
}

#[test]
fn observation_scope_never_follows_other_evals_instructions() {
    let fixture = Fixture::new();
    fixture.artifact(
        "review",
        json!({"name":"review","evals":[eval("Read {input}.")]}),
    );
    fixture.artifact(
        "data",
        json!({"name":"input","mounts":{"support":"support"},"evals":[eval("Read {hidden}.")]}),
    );
    fixture.artifact("data/child", json!({"name":"child"}));
    fixture.artifact("hidden", json!({"name":"hidden"}));
    fixture.artifact("support", json!({"name":"support"}));
    let config = fixture.config();
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.target == "review")
        .unwrap();
    let scope = eval_scope(&config, eval).unwrap();
    assert_eq!(
        scope.artifacts.keys().copied().collect::<Vec<_>>(),
        ["child", "input", "review", "support"]
    );
    assert!(
        scope
            .resolve_path("hidden", "")
            .unwrap_err()
            .to_string()
            .contains("outside this review")
    );
    assert_eq!(
        parse_artifact_instruction("{hidden} {input}", &scope, &BTreeMap::new()),
        vec![
            InstructionPart::Text("{hidden} ".into()),
            InstructionPart::Artifact("input".into()),
        ]
    );
}

#[test]
fn mount_validation_rejects_unknown_ambiguous_and_physical_aliases() {
    let fixture = Fixture::new();
    fixture.artifact("input", json!({"name":"input"}));
    fixture.artifact("third", json!({"name":"third"}));
    for (alias, target, message) in [
        ("source", "missing", "Unknown mount target"),
        ("input", "third", "Ambiguous mount alias"),
        ("review", "input", "Ambiguous mount alias"),
    ] {
        fixture.artifact("review", json!({"name":"review","mounts":{alias: target}}));
        assert!(
            read_workspace_config(&fixture.0)
                .unwrap_err()
                .to_string()
                .contains(message)
        );
    }
    fixture.artifact(
        "review",
        json!({"name":"review","mounts":{"input":"input","self":"review"}}),
    );
    fixture.config();
    fixture.write("review/input", "physical file");
    assert!(
        read_workspace_config(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains("physical entry")
    );
    fs::remove_file(fixture.0.join("review/input")).unwrap();
    fs::create_dir(fixture.0.join("review/input")).unwrap();
    assert!(
        read_workspace_config(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains("physical entry")
    );
    fs::remove_dir(fixture.0.join("review/input")).unwrap();
    if symlink_file("missing", fixture.0.join("review/input")).is_some() {
        assert!(
            read_workspace_config(&fixture.0)
                .unwrap_err()
                .to_string()
                .contains("physical entry")
        );
    }
    #[cfg(windows)]
    {
        let _ = fs::remove_file(fixture.0.join("review/input"));
        crate::test_os::junction(&fixture.0, &fixture.0.join("review/input"));
        assert!(
            read_workspace_config(&fixture.0)
                .unwrap_err()
                .to_string()
                .contains("physical entry")
        );
    }
}

#[test]
fn logical_paths_reject_traversal_and_noncanonical_components() {
    let fixture = Fixture::new();
    fixture.artifact("owner", json!({"name":"owner"}));
    let config = fixture.config();
    let scope = artifact_scope(&config, &["owner"]).unwrap();
    for path in [
        "/absolute",
        "../escape",
        "a/../b",
        "a/./b",
        ".",
        "..",
        "a//b",
        "a/",
        "//",
        "a\\b",
        "C:/file",
        "a:b",
        "a\0b",
        "a\nb",
        "a\u{7f}b",
    ] {
        assert!(scope.resolve_path("owner", path).is_err(), "{path:?}");
    }
    assert!(scope.resolve_path("owner", &"a".repeat(4096)).is_ok());
    assert!(scope.resolve_path("owner", &"a".repeat(4097)).is_err());
    assert!(scope.resolve_path("owner", &"🦀".repeat(2048)).is_ok());
    assert!(scope.resolve_path("owner", &"🦀".repeat(2049)).is_err());
    assert!(scope.resolve_path("owner", "").is_ok());
    assert!(scope.resolve_path("owner", "a/b c").is_ok());
}

#[test]
fn scoped_inputs_reject_internal_external_dangling_and_owner_symlinks() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fixture.artifact("owner", json!({"name":"owner"}));
    fixture.write("owner/directory/file", "input");
    outside.write("secret", "outside");
    let mut links = Vec::new();
    for (name, target, directory) in [
        ("internal", Path::new("directory").join("file"), false),
        ("linkdir", PathBuf::from("directory"), true),
        ("outside", outside.0.clone(), true),
        ("dangling", PathBuf::from("missing"), false),
    ] {
        let link = fixture.0.join("owner").join(name);
        let created = if directory {
            symlink_dir(target, link)
        } else {
            symlink_file(target, link)
        };
        if created.is_some() {
            links.extend(match name {
                "internal" => ["internal"].as_slice(),
                "linkdir" => &["linkdir/file"],
                "outside" => &["outside/secret"],
                _ => &["dangling"],
            });
        }
    }
    // Junctions redirect a path as directory symlinks do, and need no privilege.
    #[cfg(windows)]
    {
        crate::test_os::junction(
            &fixture.0.join("owner/directory"),
            &fixture.0.join("owner/joined"),
        );
        crate::test_os::junction(&outside.0, &fixture.0.join("owner/escape"));
        links.extend(["joined/file", "escape/secret"]);
    }
    let config = fixture.config();
    let scope = artifact_scope(&config, &["owner"]).unwrap();
    for path in links {
        assert!(
            scope
                .resolve_input(&config.root, "owner", path)
                .unwrap_err()
                .to_string()
                .contains("symlinks"),
            "{path}"
        );
    }
    assert!(
        scope
            .resolve_input(&config.root, "owner", "missing")
            .is_err()
    );
    assert_eq!(
        scope
            .resolve_input(&config.root, "owner", "directory/file")
            .unwrap(),
        fixture.0.join("owner/directory/file")
    );
    assert!(scoped_path(&fixture.0, Path::new("../outside")).is_err());
    assert!(scoped_path(&fixture.0, &outside.0).is_err());
    fs::rename(fixture.0.join("owner"), fixture.0.join("old-owner")).unwrap();
    if symlink_dir("old-owner", fixture.0.join("owner")).is_some() {
        assert!(
            scope
                .resolve_input(&config.root, "owner", "directory/file")
                .unwrap_err()
                .to_string()
                .contains("symlinks")
        );
    }
    #[cfg(windows)]
    {
        // A directory symlink is removed as a directory on Windows.
        let _ = fs::remove_dir(fixture.0.join("owner"));
        crate::test_os::junction(&fixture.0.join("old-owner"), &fixture.0.join("owner"));
        assert!(
            scope
                .resolve_input(&config.root, "owner", "directory/file")
                .unwrap_err()
                .to_string()
                .contains("symlinks")
        );
    }
}

#[test]
fn argument_only_references_add_dependencies_and_resolve_without_shell_expansion() {
    let fixture = Fixture::new();
    let args = json!([
        "{input}/nested/file",
        "--data={input}/nested/file",
        "{review}/source/nested/file",
        "{input}",
        "$HOME",
        "literal;command",
        "\\{missing}",
        "{{missing}}",
        "${missing}"
    ]);
    let mut declared = eval("No instruction dependencies.");
    declared["profile"] = json!({"kind":"runtime","command":"echo","args":args});
    fixture.artifact(
        "review",
        json!({"name":"review","mounts":{"source":"input"},"evals":[declared]}),
    );
    fixture.artifact("data", json!({"name":"input"}));
    fixture.artifact("data/nested", json!({"name":"child"}));
    fixture.write("data/nested/file", "input");
    let config = fixture.config();
    let eval = &config.evals[0];
    assert!(eval.references.is_empty());
    assert_eq!(eval.deps, ["input"]);
    assert!(config.relations.contains(&Relation {
        source: "input".into(),
        target: "review".into(),
        kind: RelationKind::Argument {
            eval_id: "review/check".into(),
            index: 0,
            name: "input".into(),
            path: "nested/file".into()
        },
    }));
    let scope = eval_scope(&config, eval).unwrap();
    let Profile::Runtime { args, command, .. } = &eval.declaration.profile else {
        panic!()
    };
    let resolved = resolve_argv(&config, &scope, "review", args).unwrap();
    let file = fixture.0.join("data").join("nested").join("file");
    let file = file.display().to_string();
    assert_eq!(
        &resolved[..4],
        [
            file.clone(),
            format!("--data={file}"),
            file,
            fixture.0.join("data").display().to_string()
        ]
    );
    assert_eq!(&resolved[4..], &args[4..]);
    assert_eq!(command, "echo");
    let restricted = artifact_scope(&config, &["child"]).unwrap();
    assert!(resolve_argv(&config, &restricted, "review", args).is_err());
}

#[test]
fn argument_only_global_reference_is_an_edge_even_without_a_mount() {
    let fixture = Fixture::new();
    let mut declared = eval("Inspect.");
    declared["profile"] = json!({"kind":"runtime","command":"cat","args":["{input}/file"]});
    fixture.artifact("review", json!({"name":"review","evals":[declared]}));
    fixture.artifact("data", json!({"name":"input"}));
    let config = fixture.config();
    assert_eq!(config.relations.len(), 1);
    assert_eq!(config.evals[0].deps, ["input"]);
    let scope = eval_scope(&config, &config.evals[0]).unwrap();
    assert!(scope.artifacts.contains_key("input"));
    // Config validation does not demand runtime input existence.
    assert!(resolve_argv(&config, &scope, "review", &["{input}/file".into()]).is_err());
}

#[test]
fn invalid_runtime_references_fail_statically_but_literals_stay_literal() {
    let fixture = Fixture::new();
    for argument in [
        "{missing}",
        "{review}/../escape",
        "{review}/a//b",
        "{review}/",
        "{review}suffix",
        "prefix{review}",
        "{review}/{review}",
        "--input={review}/a\\b",
    ] {
        let mut declared = eval("Inspect.");
        declared["profile"] = json!({"kind":"runtime","command":"echo","args":[argument]});
        fixture.artifact("review", json!({"name":"review","evals":[declared]}));
        assert!(read_workspace_config(&fixture.0).is_err(), "{argument}");
    }
    fixture.artifact(
        "review",
        json!({"name":"review","evals":[eval("Unknown {missing}.")]}),
    );
    assert!(
        read_workspace_config(&fixture.0)
            .unwrap_err()
            .to_string()
            .contains("Unknown Artifact reference")
    );
    fixture.artifact(
        "review",
        json!({
            "name":"review",
            "evals":[eval(r"Literal \{missing} {{missing}} ${missing} { unmatched")],
        }),
    );
    fixture.config();
}
