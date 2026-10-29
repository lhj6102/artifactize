use crate::config::CONFIG_FILE;

use serde_json::{Value, json};
use std::fs;

use super::*;
use crate::config::read_workspace_config;
use crate::test_os::{symlink_dir, symlink_file};

/// A repository in a temporary directory, at its physical path; removed on drop.
struct Fixture(
    PathBuf,
    #[expect(dead_code, reason = "owns the directory")] tempfile::TempDir,
);

impl Fixture {
    fn new() -> Self {
        let directory = crate::test_os::tempdir();
        Self(directory.path().to_owned(), directory)
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
fn files_belong_to_the_nearest_marked_folder_and_nesting_is_no_relation() {
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
        config.artifacts["parent"].nested,
        BTreeMap::from([
            ("unmarked/deep".parse().unwrap(), "child".parse().unwrap()),
            (
                "unmarked/deeper".parse().unwrap(),
                "neighbor".parse().unwrap()
            ),
        ])
    );
    assert_eq!(
        config.artifacts["child"].nested,
        BTreeMap::from([("leaf".parse().unwrap(), "leaf".parse().unwrap())])
    );
    assert_eq!(config.artifacts["parent"].views.agent_tools.len(), 1);
    assert!(config.artifacts["child"].views.agent_tools.is_empty());
    assert_eq!(config.evals.len(), 1);
    // Alone, the parent reaches only its own files: the nested folders are other Artifacts.
    let scope = artifact_scope(&config, &[&"parent".parse().unwrap()]).unwrap();
    assert!(
        scope
            .resolve_path(&"parent".parse().unwrap(), "unmarked/deep/input.txt")
            .unwrap_err()
            .to_string()
            .contains("outside this review: child")
    );
    let names = ["parent", "child", "neighbor", "leaf"].map(|name| name.parse().unwrap());
    let scope = artifact_scope(&config, &names.iter().collect::<Vec<_>>()).unwrap();
    assert_eq!(
        scope
            .resolve_path(&"parent".parse().unwrap(), "unmarked/material.txt")
            .unwrap(),
        ScopedPath {
            artifact_id: ArtifactId::new("parent").unwrap(),
            path: "unmarked/material.txt".into(),
        }
    );
    assert_eq!(
        scope
            .resolve_path(&"parent".parse().unwrap(), "unmarked/deep/input.txt")
            .unwrap(),
        ScopedPath {
            artifact_id: ArtifactId::new("child").unwrap(),
            path: "input.txt".into(),
        }
    );
    assert_eq!(
        scope
            .resolve_path(&"parent".parse().unwrap(), "unmarked/deeper")
            .unwrap()
            .artifact_id,
        "neighbor"
    );
    assert_eq!(
        scope
            .resolve_path(&"parent".parse().unwrap(), "unmarked/deep/leaf")
            .unwrap()
            .artifact_id,
        "leaf"
    );
    assert!(
        !config
            .relations
            .iter()
            .any(|edge| edge.kind == RelationKind::Child)
    );
}

#[test]
fn names_compose_through_their_nearest_declared_leading_name() {
    let fixture = Fixture::new();
    // Folders do not follow the names: composition comes from names alone.
    fixture.artifact("a", json!({"name":"game"}));
    fixture.artifact("b", json!({"name":"game::combat"}));
    fixture.artifact("a/c", json!({"name":"game::combat::skills::fireball"}));
    fixture.artifact("d", json!({"name":"gameplay::notes"}));
    let config = fixture.config();
    let child = |source: &str, target: &str| Relation {
        source: source.parse().unwrap(),
        target: target.parse().unwrap(),
        kind: RelationKind::Child,
    };
    let composition: Vec<_> = config
        .relations
        .iter()
        .filter(|edge| edge.kind == RelationKind::Child)
        .cloned()
        .collect();
    assert_eq!(
        composition,
        [
            child("game::combat", "game"),
            child("game::combat::skills::fireball", "game::combat"),
        ]
    );
    let game = "game".parse().unwrap();
    assert_eq!(
        config.descendants(&game).collect::<Vec<_>>(),
        ["game::combat", "game::combat::skills::fireball"]
    );
    assert_eq!(config.parent(&"gameplay::notes".parse().unwrap()), None);
}

#[test]
fn references_keep_canonical_ids() {
    let fixture = Fixture::new();
    fixture.artifact(
        "review",
        json!({"name":"review",
        "evals":[eval("Read {input}/file, {input}, {review}; \\{literal} {{literal}}.")]}),
    );
    fixture.artifact("data", json!({"name":"input"}));
    fixture.write("data/file", "not instruction text");
    let config = fixture.config();
    let eval = &config.evals[0];
    assert_eq!(
        eval.references,
        BTreeMap::from([
            ("input".parse().unwrap(), "input".parse().unwrap()),
            ("review".parse().unwrap(), "review".parse().unwrap()),
        ])
    );
    assert_eq!(eval.deps, ["input"]);
    let scope = eval_scope(&config, eval).unwrap();
    assert_eq!(scope.artifacts.len(), 2);
    assert_eq!(
        scope
            .resolve_input(&config.root, &"input".parse().unwrap(), "file")
            .unwrap(),
        fixture.0.join("data/file")
    );
    let source = &eval.declaration.payload().as_ref().unwrap().instruction;
    assert_eq!(
        parse_artifact_instruction(source, &scope, &eval.references),
        vec![
            InstructionPart::Text("Read ".into()),
            InstructionPart::Artifact("input".parse().unwrap()),
            InstructionPart::Text("/file, ".into()),
            InstructionPart::Artifact("input".parse().unwrap()),
            InstructionPart::Text(", ".into()),
            InstructionPart::Artifact("review".parse().unwrap()),
            InstructionPart::Text("; \\{literal} {{literal}}.".into()),
        ]
    );
    assert_eq!(
        eval.declaration.payload().as_ref().unwrap().extra["ownerField"]["unchanged"],
        "{unknown}"
    );
    assert!(!source.contains("not instruction text"));
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
        json!({"name":"input","evals":[eval("Read {hidden}.")]}),
    );
    fixture.artifact("data/child", json!({"name":"child"}));
    fixture.artifact("part", json!({"name":"input::part"}));
    fixture.artifact("hidden", json!({"name":"hidden"}));
    let config = fixture.config();
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.target == "review")
        .unwrap();
    let scope = eval_scope(&config, eval).unwrap();
    assert_eq!(
        scope
            .artifacts
            .keys()
            .map(ArtifactName::as_str)
            .collect::<Vec<_>>(),
        ["input", "review"]
    );
    assert!(
        scope
            .resolve_path(&"hidden".parse().unwrap(), "")
            .unwrap_err()
            .to_string()
            .contains("outside this review")
    );
    assert_eq!(
        parse_artifact_instruction("{hidden} {input}", &scope, &BTreeMap::new()),
        vec![
            InstructionPart::Text("{hidden} ".into()),
            InstructionPart::Artifact("input".parse().unwrap()),
        ]
    );
}

#[test]
fn logical_paths_reject_traversal_and_noncanonical_components() {
    let fixture = Fixture::new();
    fixture.artifact("owner", json!({"name":"owner"}));
    let config = fixture.config();
    let scope = artifact_scope(&config, &[&"owner".parse().unwrap()]).unwrap();
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
        &crate::test_os::windows_drive("file"),
        "a:b",
        "a\0b",
        "a\nb",
        "a\u{7f}b",
    ] {
        assert!(
            scope.resolve_path(&"owner".parse().unwrap(), path).is_err(),
            "{path:?}"
        );
    }
    assert!(
        scope
            .resolve_path(&"owner".parse().unwrap(), &"a".repeat(4096))
            .is_ok()
    );
    assert!(
        scope
            .resolve_path(&"owner".parse().unwrap(), &"a".repeat(4097))
            .is_err()
    );
    assert!(
        scope
            .resolve_path(&"owner".parse().unwrap(), &"🦀".repeat(2048))
            .is_ok()
    );
    assert!(
        scope
            .resolve_path(&"owner".parse().unwrap(), &"🦀".repeat(2049))
            .is_err()
    );
    assert!(scope.resolve_path(&"owner".parse().unwrap(), "").is_ok());
    assert!(
        scope
            .resolve_path(&"owner".parse().unwrap(), "a/b c")
            .is_ok()
    );
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
    // Native directory links need no special privilege, including Windows junctions.
    crate::test_os::link_dir(
        &fixture.0.join("owner/directory"),
        &fixture.0.join("owner/joined"),
    );
    crate::test_os::link_dir(&outside.0, &fixture.0.join("owner/escape"));
    links.extend(["joined/file", "escape/secret"]);
    let config = fixture.config();
    let scope = artifact_scope(&config, &[&"owner".parse().unwrap()]).unwrap();
    for path in links {
        assert!(
            scope
                .resolve_input(&config.root, &"owner".parse().unwrap(), path)
                .unwrap_err()
                .to_string()
                .contains("symlinks"),
            "{path}"
        );
    }
    assert!(
        scope
            .resolve_input(&config.root, &"owner".parse().unwrap(), "missing")
            .is_err()
    );
    assert_eq!(
        scope
            .resolve_input(&config.root, &"owner".parse().unwrap(), "directory/file")
            .unwrap(),
        fixture.0.join("owner/directory/file")
    );
    assert!(scoped_path(&fixture.0, Path::new("../outside")).is_err());
    assert!(scoped_path(&fixture.0, &outside.0).is_err());
    fs::rename(fixture.0.join("owner"), fixture.0.join("old-owner")).unwrap();
    let symlinked = symlink_dir("old-owner", fixture.0.join("owner")).is_some();
    if symlinked {
        assert!(
            scope
                .resolve_input(&config.root, &"owner".parse().unwrap(), "directory/file")
                .unwrap_err()
                .to_string()
                .contains("symlinks")
        );
    }
    if symlinked {
        crate::test_os::remove_link_dir(&fixture.0.join("owner"));
    }
    crate::test_os::link_dir(&fixture.0.join("old-owner"), &fixture.0.join("owner"));
    assert!(
        scope
            .resolve_input(&config.root, &"owner".parse().unwrap(), "directory/file")
            .unwrap_err()
            .to_string()
            .contains("symlinks")
    );
}

#[test]
fn argument_only_references_add_dependencies_and_resolve_without_shell_expansion() {
    let fixture = Fixture::new();
    let args = json!([
        "{input}/sub/file",
        "--data={input}/sub/file",
        "{input}",
        "$ARTIFACTIZE_FIXTURE_VALUE",
        "literal;command",
        "\\{missing}",
        "{{missing}}",
        "${missing}"
    ]);
    let mut declared = eval("No instruction dependencies.");
    declared["profile"] =
        json!({"kind":"runtime","command":crate::test_os::echo_program(),"args":args});
    fixture.artifact("review", json!({"name":"review","evals":[declared]}));
    fixture.artifact("data", json!({"name":"input"}));
    fixture.write("data/sub/file", "input");
    let config = fixture.config();
    let eval = &config.evals[0];
    assert!(eval.references.is_empty());
    assert_eq!(eval.deps, ["input"]);
    assert!(config.relations.contains(&Relation {
        source: "input".parse().unwrap(),
        target: "review".parse().unwrap(),
        kind: RelationKind::Argument {
            eval_id: "review/check".parse().unwrap(),
            index: 0,
            name: "input".parse().unwrap(),
            path: "sub/file".parse().unwrap()
        },
    }));
    let scope = eval_scope(&config, eval).unwrap();
    let Profile::Runtime { args, command, .. } = eval.declaration.profile() else {
        panic!()
    };
    let resolved = resolve_argv(&config, &scope, &"review".parse().unwrap(), args).unwrap();
    let file = fixture.0.join("data").join("sub").join("file");
    let file = crate::platform::path_text(&file);
    assert_eq!(
        &resolved[..3],
        [
            file.clone(),
            format!("--data={file}"),
            crate::platform::path_text(&fixture.0.join("data"))
        ]
    );
    assert_eq!(&resolved[3..], &args[3..]);
    assert_eq!(*command, crate::test_os::echo_program());
    let restricted = artifact_scope(&config, &[&"review".parse().unwrap()]).unwrap();
    assert!(resolve_argv(&config, &restricted, &"review".parse().unwrap(), args).is_err());
}

#[test]
fn an_argument_path_into_a_nested_artifact_names_that_artifact_instead() {
    let fixture = Fixture::new();
    let mut declared = eval("Inspect.");
    declared["profile"] = json!({"kind":"runtime","command":crate::test_os::cat_program(),
        "args":["{input}/nested/file"]});
    fixture.artifact("review", json!({"name":"review","evals":[declared]}));
    fixture.artifact("data", json!({"name":"input"}));
    fixture.artifact("data/nested", json!({"name":"child"}));
    let error = read_workspace_config(&fixture.0).unwrap_err().to_string();
    assert!(
        error.contains("{input}/nested/file is in Artifact child; reference {child}/file"),
        "{error}"
    );
}

#[test]
fn argument_only_reference_is_an_edge() {
    let fixture = Fixture::new();
    let mut declared = eval("Inspect.");
    declared["profile"] =
        json!({"kind":"runtime","command":crate::test_os::cat_program(),"args":["{input}/file"]});
    fixture.artifact("review", json!({"name":"review","evals":[declared]}));
    fixture.artifact("data", json!({"name":"input"}));
    let config = fixture.config();
    assert_eq!(config.relations.len(), 1);
    assert_eq!(config.evals[0].deps, ["input"]);
    let scope = eval_scope(&config, &config.evals[0]).unwrap();
    assert!(scope.artifacts.contains_key("input"));
    // Config validation does not demand runtime input existence.
    assert!(
        resolve_argv(
            &config,
            &scope,
            &"review".parse().unwrap(),
            &["{input}/file".into()]
        )
        .is_err()
    );
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
        declared["profile"] =
            json!({"kind":"runtime","command":crate::test_os::echo_program(),"args":[argument]});
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
