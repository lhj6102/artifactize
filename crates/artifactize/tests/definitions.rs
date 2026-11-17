//! Definition discovery, migration and write-back through public process/file boundaries.
use artifactize_core::config::{
    edit::{self, Operation, Refusal},
    read_workspace_config,
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

struct Project {
    root: tempfile::TempDir,
    repo: PathBuf,
}
impl Project {
    fn new() -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join(".artifactize")).unwrap();
        Self { root, repo }
    }
    fn write(&self, path: &str, source: &str) {
        let file = self.repo.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }
    fn node(&self, name: &str, source: &str) {
        self.write(&format!(".artifactize/{name}.artf"), source);
    }
    fn cli(&self, args: &[&str]) -> Output {
        Command::new(support::os::cli())
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(self.root.path().join("state"))
            .args(args)
            .record_output()
            .unwrap()
    }
    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.cli(args);
        assert_eq!(
            output.status.code(),
            Some(code),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
fn name(value: &str) -> artifactize_core::config::ArtifactName {
    value.parse().unwrap()
}
fn runtime() -> String {
    format!(
        "\n[evals.check]\ntitle = \"Check\"\nprofile = {{ kind = \"runtime\", command = {:?}, args = [] }}\npayload.instruction = \"Check.\"\n",
        support::os::true_program()
    )
}

#[test]
fn names_paths_definitions_and_explicit_parts_are_independent() {
    let project = Project::new();
    project.node("outer", "path = \"src/\"\nparts = [\"inner\"]\n");
    project.node("family/inner", "path = \"src/lib/\"\nbasis = true\n");
    project.node("file", "path = \"README.md\"\nbasis = true\n");
    project.node(
        "aspect",
        "path = \"src/\"\nselect = [\"main.rs\"]\nbasis = true\n",
    );
    project.write("src/lib/lib.rs", "library");
    project.write("src/main.rs", "main");
    project.write("README.md", "readme");
    let graph = project.json(&["config", "graph", "--json"], 0);
    assert_eq!(
        graph["artifacts"]["outer"]["parts"],
        serde_json::json!(["inner"])
    );
    assert_eq!(
        graph["artifacts"]["inner"]["definition"],
        ".artifactize/family/inner.artf"
    );
    assert_eq!(graph["artifacts"]["aspect"]["path"], "src");
    assert_eq!(graph["artifacts"]["aspect"]["kind"], "aspect");
    assert_eq!(graph["artifacts"]["file"]["kind"], "file");
    assert!(project.cli(&["config", "check"]).status.success());
}

#[test]
fn malformed_names_and_paths_are_positioned_and_old_form_gets_one_hint() {
    for (file, source, expected) in [
        (
            "node",
            "name = \"other\"\npath = \"./\"\n",
            "the file name is the Artifact's name",
        ),
        ("node", "path = \"../escape/\"\n", "path"),
        ("node", "path = \"/absolute/\"\n", "path"),
        ("node", "path = \"foo//bar/\"\n", "path"),
    ] {
        let project = Project::new();
        project.node(file, source);
        let output = project.cli(&["config", "check"]);
        assert_eq!(output.status.code(), Some(2));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(
            error.contains("line") && error.contains("column"),
            "{error}"
        );
    }
    let project = Project::new();
    project.node("node", "path = \"./\"\n");
    project.node("sub/Node", "path = \"./\"\n");
    let error = project.json(&["config", "check", "--json"], 2)["error"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        error.contains("node.artf") && error.contains("Node.artf"),
        "{error}"
    );
    let project = Project::new();
    fs::remove_dir(project.repo.join(".artifactize")).unwrap();
    project.write("index.artf", "name = \"old\"\n");
    let error = project.json(&["config", "check", "--json"], 2)["error"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(error.contains("config migrate"), "{error}");
}

#[test]
fn missing_targets_are_notes_not_definition_errors() {
    let project = Project::new();
    project.node("future", &format!("path = \"future/\"\n{}", runtime()));
    let check = project.json(&["config", "check", "--json"], 0);
    assert_eq!(check["notes"].as_array().unwrap().len(), 1);
    project.node("wrong", "path = 'wrong/'\nbasis = true\n");
    project.write("wrong", "not a directory");
    let check = project.json(&["config", "check", "--json"], 0);
    assert_eq!(check["notes"].as_array().unwrap().len(), 2);
    project.write("future/source", "implementation");
    fs::remove_file(project.repo.join("wrong")).unwrap();
    fs::create_dir(project.repo.join("wrong")).unwrap();
    let check = project.json(&["config", "check", "--json"], 0);
    assert!(check.get("notes").is_none());
}

#[test]
fn edits_are_validated_compare_content_and_preserve_untouched_text() {
    let project = Project::new();
    project.node("whole", "# Whole\npath = \"src/\" # target\nbasis = true\n");
    project.node("part", "path = \"src/lib/\"\nbasis = true\n");
    let mut based = edit::read(&project.repo).unwrap();
    let changed = edit::apply(
        &project.repo,
        &based,
        Operation::AddPart {
            at: None,
            whole: name("whole"),
            part: name("part"),
        },
    )
    .unwrap();
    based.extend(changed);
    let whole = fs::read_to_string(project.repo.join(".artifactize/whole.artf")).unwrap();
    assert!(whole.starts_with("# Whole\npath = \"src/\" # target\n"));
    let before = edit::read(&project.repo).unwrap();
    assert!(matches!(
        edit::apply(
            &project.repo,
            &based,
            Operation::AddPart {
                at: None,
                whole: name("part"),
                part: name("whole")
            }
        ),
        Err(Refusal::Invalid { .. })
    ));
    assert_eq!(before, edit::read(&project.repo).unwrap());
    project.node("whole", "path = \"other/\"\nbasis = true\n");
    assert!(matches!(
        edit::apply(
            &project.repo,
            &based,
            Operation::SetPath {
                name: name("whole"),
                path: "next/".into()
            }
        ),
        Err(Refusal::Changed {
            current: Some(_),
            ..
        })
    ));
    assert_eq!(
        fs::read_to_string(project.repo.join(".artifactize/whole.artf")).unwrap(),
        "path = \"other/\"\nbasis = true\n"
    );
}

#[test]
fn create_move_source_delete_and_layout_share_the_file_contract() {
    let project = Project::new();
    project.node("a", "path = \"a/\"\nparts = [\"part\"]\nbasis = true\n");
    project.node("b", "path = \"b/\"\nbasis = true\n");
    project.node("part", "path = \"shared/\"\nbasis = true\n");
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::MovePart {
            at: None,
            part: name("part"),
            from: Some(name("a")),
            to: Some(name("b")),
        },
    )
    .unwrap();
    let config = read_workspace_config(&project.repo).unwrap();
    assert!(config.artifacts["a"].parts.is_empty());
    assert!(config.artifacts["b"].parts.contains("part"));
    let based = edit::read(&project.repo).unwrap();
    assert!(
        edit::apply(
            &project.repo,
            &based,
            Operation::Delete { name: name("part") }
        )
        .is_err()
    );
    let mut based = edit::read(&project.repo).unwrap();
    based.insert(PathBuf::from(".artifactize/new.artf"), None);
    edit::apply(
        &project.repo,
        &based,
        Operation::Create {
            name: name("new"),
            path: "new/".into(),
        },
    )
    .unwrap();
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::SaveSource {
            name: name("new"),
            source: "# Edited\npath = \"new/\"\nbasis = true\n".into(),
        },
    )
    .unwrap();
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::SavePlaces {
            places: [
                ("a".parse().unwrap(), [10., 20.]),
                ("b/part".parse().unwrap(), [3., 4.]),
                ("unknown".parse().unwrap(), [1., 1.]),
            ]
            .into(),
        },
    )
    .unwrap();
    let layout = edit::read_layout(&project.repo).unwrap();
    assert_eq!(layout.nodes.len(), 2);
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::ResetPlaces { whole: None },
    )
    .unwrap();
    assert_eq!(edit::read_layout(&project.repo).unwrap().nodes.len(), 1);
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::Delete { name: name("new") },
    )
    .unwrap();
    assert!(!project.repo.join(".artifactize/new.artf").exists());
    project.write(".artifactize/layout.toml", "malformed [");
    assert!(project.cli(&["config", "check"]).status.success());
}

#[test]
fn rename_updates_references_parts_and_layout_without_touching_escaped_braces() {
    let project = Project::new();
    project.node("family/old", "# Keep me\npath = \"src/\"\nbasis = true\n");
    project.node("consumer", &format!("path = \"./\"\nparts = [\"old\"]\n[evals.check]\ntitle = \"Check\"\nprofile = {{ kind = \"runtime\", command = {:?}, args = [\"{{old}}/x\"] }}\npayload.instruction = \"Read {{old}}, not {{{{old}}}}.\"\n", support::os::true_program()));
    let consumer_file = project.repo.join(".artifactize/consumer.artf");
    let mut source = fs::read_to_string(&consumer_file).unwrap();
    source.push_str("\n[annotations.style.parts]\nold = [3, 4]\n");
    fs::write(consumer_file, source).unwrap();
    let mut based = edit::read(&project.repo).unwrap();
    based.insert(PathBuf::from(".artifactize/family/new.artf"), None);
    edit::apply(
        &project.repo,
        &based,
        Operation::Rename {
            old: name("old"),
            new: name("new"),
        },
    )
    .unwrap();
    assert!(!project.repo.join(".artifactize/family/old.artf").exists());
    assert!(
        fs::read_to_string(project.repo.join(".artifactize/family/new.artf"))
            .unwrap()
            .starts_with("# Keep me")
    );
    let source = fs::read_to_string(project.repo.join(".artifactize/consumer.artf")).unwrap();
    assert!(source.contains("{new}/x"));
    assert!(source.contains("{{old}}"));
    assert!(source.contains("parts = [\"new\"]"));
    assert!(
        edit::read_layout(&project.repo)
            .unwrap()
            .nodes
            .contains_key(&"consumer/new".parse::<edit::PlacementKey>().unwrap())
    );
}

#[test]
fn migration_keeps_comments_makes_parts_explicit_and_leaves_nested_project() {
    let project = Project::new();
    fs::remove_dir(project.repo.join(".artifactize")).unwrap();
    project.write(".artf", "# Structure\nname = \"graph\"\nbasis = true\n");
    project.write("index.artf", "# Root\nname = \"root\"\n");
    project.write(
        "lib/index.artf",
        "# Library\nname = \"lib\"\nbasis = true\n",
    );
    project.write("lib/main.rs", "fn main() {}\n");
    project.write("lib/main.rs.artf", "name = \"main\"\nbasis = true\n");
    project.write(".artfignore", "nested/\n");
    project.write("nested/index.artf", "name = \"separate\"\nbasis = true\n");
    project.json(&["config", "migrate", "--json"], 0);
    assert!(project.repo.join("nested/index.artf").is_file());
    assert!(!project.repo.join("index.artf").exists());
    assert!(!project.repo.join(".artfignore").exists());
    let source = fs::read_to_string(project.repo.join(".artifactize/root.artf")).unwrap();
    assert!(source.contains("# Root"));
    assert!(source.contains(".artifactize/"));
    let config = read_workspace_config(&project.repo).unwrap();
    assert!(config.artifacts["root"].parts.contains("lib"));
    assert!(config.artifacts["lib"].parts.contains("main"));
    assert_eq!(
        config.artifacts["graph"].path.local(),
        Some(Path::new(".artifactize"))
    );
    let before = edit::read(&project.repo).unwrap();
    project.json(&["config", "migrate", "--json"], 2);
    assert_eq!(before, edit::read(&project.repo).unwrap());
}

#[test]
fn annotations_and_toml_spelling_never_change_definition_review_reuse() {
    let project = Project::new();
    project.node(
        "definitions",
        &format!("path = \".artifactize/\"\n{}", runtime()),
    );
    project.node("source", "path = \"src/\"\nbasis = true\n");
    project.write("src/input", "material");
    let first = project.json(&["verify", "--all", "--wait", "--json"], 0);
    project.node("source", "# Formatting is not meaning\nbasis=true\npath='src/'\n[annotations.style]\nat='broken but inert'\n[annotations.comment]\ntext='not a criterion'\n[annotations.future]\nanything=1979-05-27\n");
    let second = project.json(&["verify", "--all", "--wait", "--json"], 0);
    assert_eq!(
        first["requests"][0]["executionId"],
        second["requests"][0]["executionId"]
    );
    project.node(
        "source",
        "path = \"src/\"\nbasis = true\nannotations = 42\n",
    );
    assert!(project.cli(&["config", "check"]).status.success());
    project.node("source", "path = \"src/\"\nbasis = true\nunknown = 42\n");
    assert_eq!(project.cli(&["config", "check"]).status.code(), Some(2));
}

#[tokio::test]
async fn file_builtin_shares_the_definition_view_without_stripping_user_schema_keys() {
    use artifactize_core::tools::{Content, Registry};
    use tokio_util::sync::CancellationToken;
    let project = Project::new();
    project.node(
        "definitions",
        "path = \".artifactize/\"\nbasis = true\n[views.agent_tools.read]\nbuiltin = \"read\"\n",
    );
    project.node("source", "path = \"src/\"\n[evals.review]\ntitle = 'Review'\nprofile = { kind = 'human' }\npayload.instruction = 'Review.'\npass_schema = { type = 'object', properties.comment = { type = 'string' } }\n[annotations.comment]\ntext = 'hidden-note'\n");
    let config = read_workspace_config(&project.repo).unwrap();
    let registry = Registry::for_artifact(&config, "definitions").unwrap();
    let result = registry
        .call(
            "read_definitions",
            serde_json::json!({"path":"source.artf"}),
            project.root.path(),
            CancellationToken::new(),
        )
        .await;
    assert!(!result.is_error(), "{result:?}");
    let contents = result.into_content();
    let Content::Json { data } = &contents[0] else {
        panic!("read output")
    };
    let text = data.to_string();
    assert!(!text.contains("hidden-note"));
    assert!(!text.contains("annotations"));
    assert!(text.contains("comment"));
}

#[test]
fn annotation_edits_keep_future_data_and_move_comments_and_places_together() {
    let project = Project::new();
    project.node("whole", "path = \"whole/\"\nbasis = true\n[annotations.future]\n# Keep exactly\nextra = 'future-value'\n");
    project.node("part", "path = \"part/\"\nbasis = true\n");
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::SavePlaces {
            places: [
                ("whole".parse().unwrap(), [10., 20.]),
                ("part".parse().unwrap(), [30., 40.]),
            ]
            .into(),
        },
    )
    .unwrap();
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::AddPart {
            whole: name("whole"),
            part: name("part"),
            at: None,
        },
    )
    .unwrap();
    let current = edit::read_project(&project.repo).unwrap();
    assert_eq!(current.nodes["whole"].annotations.parts["part"], [30., 40.]);
    assert_eq!(current.nodes["part"].annotations.at, None);
    assert_eq!(current.nodes["whole"].target, "whole/");
    assert_eq!(current.nodes["whole"].identity.len(), 64);
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::SetComment {
            name: name("whole"),
            text: "One line\nAnother".into(),
        },
    )
    .unwrap();
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::MoveComment {
            from: name("whole"),
            to: name("part"),
        },
    )
    .unwrap();
    let current = edit::read_project(&project.repo).unwrap();
    assert_eq!(current.nodes["whole"].annotations.comment, None);
    assert_eq!(
        current.nodes["part"].annotations.comment.as_deref(),
        Some("One line\nAnother")
    );
    assert!(
        current.files[Path::new(".artifactize/whole.artf")]
            .as_ref()
            .unwrap()
            .contains("# Keep exactly\nextra = 'future-value'")
    );
    let based = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &based,
        Operation::SetComment {
            name: name("whole"),
            text: "Already occupied".into(),
        },
    )
    .unwrap();
    let before = edit::read(&project.repo).unwrap();
    let error = edit::apply(
        &project.repo,
        &before,
        Operation::MoveComment {
            from: name("part"),
            to: name("whole"),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("already has a comment"));
    assert_eq!(before, edit::read(&project.repo).unwrap());
}

#[test]
fn unreadable_annotation_edits_and_multifile_conflicts_write_nothing() {
    let project = Project::new();
    project.node(
        "a",
        "path = \"a/\"\nbasis = true\n[annotations.style]\nat = 'bad'\n",
    );
    project.node("b", "path = \"b/\"\nbasis = true\n");
    let before = edit::read(&project.repo).unwrap();
    let error = edit::apply(
        &project.repo,
        &before,
        Operation::SavePlaces {
            places: [
                ("a".parse().unwrap(), [1., 2.]),
                ("b".parse().unwrap(), [3., 4.]),
            ]
            .into(),
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        Refusal::Invalid {
            line: Some(_),
            column: Some(_),
            ..
        }
    ));
    assert_eq!(before, edit::read(&project.repo).unwrap());
    edit::apply(
        &project.repo,
        &before,
        Operation::SetComment {
            name: name("a"),
            text: "A note does not touch the broken style".into(),
        },
    )
    .unwrap();
    let based = edit::read(&project.repo).unwrap();
    project.node(
        "b",
        "path = \"b/\"\nbasis = true\n[annotations.comment]\ntext = 'Changed externally'\n",
    );
    let before = edit::read(&project.repo).unwrap();
    assert!(matches!(
        edit::apply(
            &project.repo,
            &based,
            Operation::MoveComment {
                from: name("a"),
                to: name("b")
            }
        ),
        Err(Refusal::NotAllowed { .. }) | Err(Refusal::Changed { .. })
    ));
    assert_eq!(before, edit::read(&project.repo).unwrap());
}

#[test]
fn site_reads_keep_the_target_kind_and_its_aspect_selection_together() {
    let repo = support::os::tempdir();
    fs::create_dir(repo.path().join(".artifactize")).unwrap();
    fs::write(
        repo.path().join(".artifactize/slice.artf"),
        "path = 'src/'\nselect = ['chosen.txt']\nbasis = true\n",
    )
    .unwrap();
    let sites = artifactize_core::config::read_sites(repo.path()).unwrap();
    let site = &sites["slice"];
    assert_eq!(
        site.path.kind(),
        artifactize_core::config::ArtifactKind::Aspect
    );
    assert_eq!(site.path.local(), Some(Path::new("src")));
    assert_eq!(site.path.select()[0].as_str(), "chosen.txt");
}

#[test]
fn placement_wire_keys_are_validated_before_becoming_edit_operations() {
    for key in ["", "whole/part/extra", "whole/", "../part", "whole/CON"] {
        assert!(
            serde_json::from_value::<Operation>(
                serde_json::json!({"kind":"savePlaces", "places":{key:[1,2]}})
            )
            .is_err(),
            "{key:?}"
        );
    }
    let operation: Operation = serde_json::from_value(
        serde_json::json!({"kind":"savePlaces","places":{"whole/part":[1,2]}}),
    )
    .unwrap();
    let Operation::SavePlaces { places } = operation else {
        panic!("save places")
    };
    assert_eq!(places.keys().next().unwrap().owner().as_str(), "whole");
    assert_eq!(
        serde_json::to_value(places).unwrap(),
        serde_json::json!({"whole/part":[1.0,2.0]})
    );
}

fn apply_with_footprint(project: &Project, operation: Operation, expected: &[&str]) {
    use std::collections::BTreeSet;
    let snapshot = edit::read_project(&project.repo).unwrap();
    let before = edit::read(&project.repo).unwrap();
    let footprint = edit::footprint(&snapshot, &operation).unwrap();
    assert_eq!(
        edit::read(&project.repo).unwrap(),
        before,
        "preview writes nothing"
    );
    let expected = expected.iter().map(PathBuf::from).collect::<BTreeSet<_>>();
    assert_eq!(
        footprint.files.keys().cloned().collect::<BTreeSet<_>>(),
        expected
    );
    for (file, base) in &footprint.files {
        assert_eq!(
            *base,
            before.get(file).cloned().flatten(),
            "snapshot base for {file:?}"
        );
    }
    let bases = footprint.files.into_iter().collect();
    edit::apply(&project.repo, &bases, operation).unwrap();
    let after = edit::read(&project.repo).unwrap();
    let changed = before
        .keys()
        .chain(after.keys())
        .filter(|file| before.get(*file) != after.get(*file))
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        changed, expected,
        "the published footprint is the actual disk write set"
    );
}

#[test]
fn operation_footprints_own_destinations_and_exact_multifile_write_sets() {
    let project = Project::new();
    project.node(
        "group/leaf",
        "path = 'leaf/'\nbasis = true\n[annotations.style]\nat = [1,2]\n",
    );
    project.node("whole", "path = './'\nbasis = true\nparts = ['leaf']\n[annotations.style.parts]\nleaf = [3,4]\n[annotations.comment]\ntext = 'A note'\n");
    project.node("other", "path = 'other/'\nbasis = true\n");
    project.node(
        "reader",
        &format!(
            "path = 'reader/'{}",
            runtime().replace("Check.", "Read {leaf}.")
        ),
    );
    project.node(
        "note",
        "path = 'note/'\nbasis = true\n[annotations.future]\ntext = '{leaf} is just a note'\n",
    );
    apply_with_footprint(
        &project,
        Operation::Create {
            name: name("fresh"),
            path: "future/".into(),
        },
        &[".artifactize/fresh.artf"],
    );
    apply_with_footprint(
        &project,
        Operation::Rename {
            old: name("leaf"),
            new: name("renamed"),
        },
        &[
            ".artifactize/group/leaf.artf",
            ".artifactize/group/renamed.artf",
            ".artifactize/whole.artf",
            ".artifactize/reader.artf",
        ],
    );
    apply_with_footprint(
        &project,
        Operation::MovePart {
            part: name("renamed"),
            from: Some(name("whole")),
            to: Some(name("other")),
            at: Some([9., 10.]),
        },
        &[
            ".artifactize/group/renamed.artf",
            ".artifactize/whole.artf",
            ".artifactize/other.artf",
        ],
    );
    apply_with_footprint(
        &project,
        Operation::MoveComment {
            from: name("whole"),
            to: name("other"),
        },
        &[".artifactize/whole.artf", ".artifactize/other.artf"],
    );
    apply_with_footprint(
        &project,
        Operation::SetPath {
            name: name("fresh"),
            path: "future/".into(),
        },
        &[],
    );
    apply_with_footprint(
        &project,
        Operation::Delete {
            name: name("fresh"),
        },
        &[".artifactize/fresh.artf"],
    );
}

#[test]
fn a_snapshot_footprint_never_adopts_a_later_disk_edit_as_its_base() {
    let project = Project::new();
    let original = "path = './'\nbasis = true\n";
    project.node("page", original);
    let snapshot = edit::read_project(&project.repo).unwrap();
    let changed = "path = 'changed/'\nbasis = true\n";
    project.node("page", changed);
    let operation = Operation::SetComment {
        name: name("page"),
        text: "Draft-safe note".into(),
    };
    let footprint = edit::footprint(&snapshot, &operation).unwrap();
    assert_eq!(
        footprint.files[Path::new(".artifactize/page.artf")].as_deref(),
        Some(original)
    );
    let bases = footprint.files.into_iter().collect();
    assert!(
        matches!(edit::apply(&project.repo, &bases, operation), Err(Refusal::Changed { current: Some(text), .. }) if text == changed)
    );
    assert_eq!(
        fs::read_to_string(project.repo.join(".artifactize/page.artf")).unwrap(),
        changed
    );
}

#[test]
fn status_reports_unavailable_targets_per_eval_without_losing_other_forecasts() {
    let project = Project::new();
    project.node("missing", &format!("path = 'missing/'\n{}", runtime()));
    project.node("wrong", &format!("path = 'wrong/'\n{}", runtime()));
    project.write("wrong", "not a directory");
    project.node("ready", &format!("path = 'ready/'\n{}", runtime()));
    project.write("ready/input", "ready");
    let status = project.json(&["status", "--json"], 1);
    let eval = |id: &str| {
        status["evals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|eval| eval["id"] == id)
            .unwrap()
    };
    for id in ["missing/check", "wrong/check"] {
        assert_eq!(eval(id)["state"], "ERROR");
        assert!(
            eval(id)["reason"]
                .as_str()
                .unwrap()
                .starts_with("PREPARATION_FAILED:")
        );
    }
    assert_eq!(eval("ready/check")["action"], "execute");
    assert_ne!(eval("ready/check")["state"], "ERROR");
    assert!(
        !project.root.path().join("state").exists(),
        "status remains read-only"
    );
}
