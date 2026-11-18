//! Definition editing and ownership through the public library, CLI and files.
use artifactize_core::{
    cache::{self, Parallelism},
    config::{
        ArtifactName,
        edit::{self, Operation, Refusal},
        read_workspace_config,
    },
    scope,
};
use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};
use tokio_util::sync::CancellationToken;
#[path = "support/run.rs"]
mod owned_run;
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

struct Project {
    root: owned_run::Root,
    repo: PathBuf,
}
impl Project {
    fn new() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join(".artifactize")).unwrap();
        root.track(&root.path().join("state"), &repo);
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
    fn source(&self, name: &str) -> String {
        fs::read_to_string(self.repo.join(format!(".artifactize/{name}.artf"))).unwrap()
    }
    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = Command::new(support::os::cli())
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(self.root.path().join("state"))
            .args(args)
            .arg("--json")
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
    }
}
fn name(value: &str) -> ArtifactName {
    value.parse().unwrap()
}

#[test]
fn path_edits_keep_comments_and_stale_source_can_be_reloaded_then_repaired() {
    let project = Project::new();
    project.node(
        "a",
        "# Before\npath = 'before/' # Keep this\nbasis = true\n",
    );
    let base = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &base,
        Operation::SetPath {
            name: name("a"),
            path: "after/".into(),
        },
    )
    .unwrap();
    let changed = project.source("a");
    assert!(changed.starts_with("# Before\n"));
    assert!(changed.contains(" # Keep this\n"));
    let base = edit::read(&project.repo).unwrap();
    let broken = "path = [\n";
    project.node("a", broken);
    let operation = Operation::SaveSource {
        name: name("a"),
        source: changed.clone(),
    };
    assert!(
        matches!(edit::apply(&project.repo, &base, operation.clone()), Err(Refusal::Changed { current: Some(text), .. }) if text == broken)
    );
    assert_eq!(project.source("a"), broken);
    let reloaded = edit::read(&project.repo).unwrap();
    edit::apply(&project.repo, &reloaded, operation).unwrap();
    assert_eq!(project.source("a"), changed);
    read_workspace_config(&project.repo).unwrap();
}

#[test]
fn a_comment_destination_changed_on_disk_returns_its_text_without_touching_either_file() {
    let project = Project::new();
    let from = "path = './'\nbasis = true\n[annotations.comment]\ntext = 'source note'\n";
    project.node("a", from);
    project.node("b", "path = './'\nbasis = true\n");
    let base = edit::read(&project.repo).unwrap();
    let occupied = "path = './'\nbasis = true\n[annotations.comment]\ntext = 'another note'\n";
    project.node("b", occupied);
    assert!(
        matches!(edit::apply(&project.repo, &base, Operation::MoveComment { from: name("a"), to: name("b") }), Err(Refusal::Changed { file, current: Some(text) }) if file == std::path::Path::new(".artifactize/b.artf") && text == occupied)
    );
    assert_eq!(project.source("a"), from);
    assert_eq!(project.source("b"), occupied);
}

#[test]
fn renames_do_not_rewrite_plain_tool_inputs_fixed_file_names_or_reserved_human_operands() {
    let project = Project::new();
    project.write("data/file.txt", "evidence");
    project.write("{input}", "a filename, not a reference");
    project.write("file.txt", "the consumer's file");
    project.node("input", "path = 'data/'\nbasis = true\n");
    project.node("artifactPath", "path = 'data/'\nbasis = true\n");
    project.node("consumer", &format!(r#"path = './'
basis = true
[views.agent_tools.plain]
description = "A plain argument"
command = {program:?}
protocol = "plain"
args = ["{{input}}"]
input_schema = {{ type = "object", properties = {{ input = {{ type = "string" }} }}, required = ["input"] }}
[views.agent_tools.fixed]
builtin = "read"
args = ["{{input}}"]
[views.agent_tools.json]
description = "An Artifact argument"
command = {program:?}
protocol = "json"
args = ["{{input}}/file.txt"]
[views.human_tools.read]
builtin = "read"
args = ["{{artifactPath}}/file.txt"]
"#, program = support::os::true_program()));
    let mut base = edit::read(&project.repo).unwrap();
    base.insert(".artifactize/renamed.artf".into(), None);
    edit::apply(
        &project.repo,
        &base,
        Operation::Rename {
            old: name("input"),
            new: name("renamed"),
        },
    )
    .unwrap();
    let source = project.source("consumer");
    assert!(source.contains("args = [\"{input}\"]"));
    assert!(source.contains("args = [\"{renamed}/file.txt\"]"));
    let mut base = edit::read(&project.repo).unwrap();
    base.insert(".artifactize/other.artf".into(), None);
    edit::apply(
        &project.repo,
        &base,
        Operation::Rename {
            old: name("artifactPath"),
            new: name("other"),
        },
    )
    .unwrap();
    assert!(
        project
            .source("consumer")
            .contains("{artifactPath}/file.txt")
    );
    let base = edit::read(&project.repo).unwrap();
    edit::apply(
        &project.repo,
        &base,
        Operation::Delete {
            name: name("other"),
        },
    )
    .unwrap();
    read_workspace_config(&project.repo).unwrap();
}

#[tokio::test]
async fn equal_and_ancestor_folder_parts_own_the_covered_inputs_and_require_a_named_reference() {
    for (declaration, operand) in [
        ("path = 'data/'\n", "sub/file.txt"),
        ("path = 'data/sub/'\n", "file.txt"),
        ("path = 'data/sub/file.txt'\n", ""),
        (
            "path = 'data/'\nselect = ['sub/file.txt']\n",
            "sub/file.txt",
        ),
    ] {
        let project = Project::new();
        project.node(
            "whole",
            &format!("{declaration}parts = ['part']\nbasis = true\n"),
        );
        project.node("part", "path = 'data/'\nbasis = true\n");
        project.write("data/sub/file.txt", "before");
        let config = read_workspace_config(&project.repo).unwrap();
        let whole = name("whole");
        let part = name("part");
        let narrow = scope::artifact_scope(&config, &[&whole]).unwrap();
        assert!(
            narrow
                .resolve_input(&config.root, &whole, operand)
                .unwrap_err()
                .to_string()
                .contains("folder part part")
        );
        let admitted = scope::artifact_scope(&config, &[&whole, &part]).unwrap();
        assert_eq!(
            admitted
                .resolve_input(&config.root, &whole, operand)
                .unwrap(),
            support::os::canonical(&project.repo.join("data/sub/file.txt"))
        );
        let before = cache::prepare(
            &config,
            [&whole, &part],
            project.root.path(),
            &Parallelism::new(2).unwrap(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(
            before[&whole]
                .manifest
                .as_ref()
                .unwrap()
                .files
                .as_ref()
                .unwrap()
                .is_empty()
        );
        project.write("data/sub/file.txt", "after");
        let after = cache::prepare(
            &config,
            [&whole, &part],
            project.root.path(),
            &Parallelism::new(2).unwrap(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(before[&whole].value, after[&whole].value);
        assert_ne!(before[&part].value, after[&part].value);
    }
}

#[test]
fn missing_aspect_inputs_fail_only_their_evals_then_pass_when_implemented() {
    let project = Project::new();
    project.write("future/existing.txt", "folder exists");
    project.write("healthy/data.txt", "healthy");
    let check = format!(
        "[evals.check]\ntitle = 'Check'\nprofile = {{ kind = 'runtime', command = {:?}, args = [] }}\npayload.instruction = 'Check.'\n",
        support::os::true_program()
    );
    project.node(
        "future",
        &format!("path = 'future/'\nselect = ['pending.txt']\n{check}"),
    );
    project.node("healthy", &format!("path = 'healthy/'\n{check}"));
    let config = project.json(&["config", "check"], 0);
    assert_eq!(config["notes"].as_array().unwrap().len(), 1);
    let first = project.json(&["verify", "--all", "--wait"], 2);
    let requests = first["requests"].as_array().unwrap();
    assert_eq!(
        requests
            .iter()
            .find(|request| request["evalId"] == "future/check")
            .unwrap()["errorCode"],
        "PREPARATION_FAILED"
    );
    assert_eq!(
        requests
            .iter()
            .find(|request| request["evalId"] == "healthy/check")
            .unwrap()["status"],
        "GREEN"
    );
    project.write("future/pending.txt", "implemented");
    let second = project.json(&["verify", "--all", "--wait"], 0);
    assert_eq!(second["executionsStarted"], 1);
}

#[test]
fn malformed_style_places_only_reset_the_affected_box_but_refuse_writes_to_that_section() {
    let project = Project::new();
    project.node("part", "path = 'child/'\nbasis = true\n");
    for (at, parts, parent_placed) in [
        ("'broken'", "[10, 20]", false),
        ("[1, 2]", "'broken'", true),
    ] {
        let source = format!(
            "path = './'\nparts = ['part']\nbasis = true\n[annotations.style]\nat = {at}\n[annotations.style.parts]\npart = {parts}\n"
        );
        project.node("whole", &source);
        let read = edit::read_project(&project.repo).unwrap();
        let annotations = &read.nodes[&name("whole")].annotations;
        assert_eq!(annotations.at.is_some(), parent_placed);
        assert_eq!(
            annotations.parts.contains_key(&name("part")),
            !parent_placed
        );
        assert!(annotations.style_error.is_some());
        let refusal = edit::apply(
            &project.repo,
            &read.files,
            Operation::SavePlaces {
                places: [("whole".parse().unwrap(), [3.0, 4.0])].into(),
            },
        )
        .unwrap_err();
        assert!(matches!(
            refusal,
            Refusal::Invalid {
                line: Some(_),
                column: Some(_),
                ..
            }
        ));
        assert_eq!(project.source("whole"), source);
    }
}

#[test]
fn an_optimistic_base_cannot_read_a_path_outside_the_definition_directory() {
    let project = Project::new();
    let source = "path = './'\nbasis = true\n";
    project.node("outside", source);
    fs::write(project.root.path().join("outside.artf"), "not project data").unwrap();
    let base = [(PathBuf::from("../outside.artf"), None)]
        .into_iter()
        .collect();
    let refusal = edit::apply(
        &project.repo,
        &base,
        Operation::SaveSource {
            name: name("outside"),
            source: source.into(),
        },
    )
    .unwrap_err();
    assert!(matches!(refusal, Refusal::NotAllowed { .. }));
    assert_eq!(project.source("outside"), source);
}

#[test]
fn rename_keeps_an_unreadable_style_it_does_not_need_to_write() {
    let project = Project::new();
    project.node("part", "path = 'part/'\nbasis = true\n");
    project.node("whole", "path = './'\nparts = ['part']\nbasis = true\n[annotations.style]\nat = 'unreadable but untouched'\n");
    let mut base = edit::read(&project.repo).unwrap();
    base.insert(".artifactize/renamed.artf".into(), None);
    edit::apply(
        &project.repo,
        &base,
        Operation::Rename {
            old: name("part"),
            new: name("renamed"),
        },
    )
    .unwrap();
    assert!(
        project
            .source("whole")
            .contains("at = 'unreadable but untouched'\n")
    );
    assert_eq!(
        read_workspace_config(&project.repo).unwrap().artifacts[&name("whole")].parts,
        [name("renamed")].into()
    );
}

#[test]
fn reserved_names_are_refused_before_creating_a_native_file() {
    let project = Project::new();
    let error = edit::apply(
        &project.repo,
        &edit::BasedOn::default(),
        Operation::Create {
            name: name(support::os::reserved_device_name()),
            path: "./".into(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("device name"));
    assert_eq!(
        fs::read_dir(project.repo.join(".artifactize"))
            .unwrap()
            .count(),
        0
    );
}
