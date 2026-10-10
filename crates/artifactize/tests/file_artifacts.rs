use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use artifactize::{
    cache::{self, Parallelism},
    config::{ArtifactKind, Fingerprint, RepoConfig, read_workspace_config},
    monitor::{self, Target},
    scope::{artifact_scope, eval_scope, resolve_argv},
    store,
    tools::{Content, Registry},
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

mod support;

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = support::os::tempdir();
        let base = support::os::canonical(root.path());
        let repo = base.join("repo");
        fs::create_dir(&repo).unwrap();
        Self {
            state: base.join("state"),
            repo,
            _root: root,
        }
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn declare(&self, path: &str, declaration: Value) {
        self.write(path, &support::declaration::to_toml(declaration).unwrap());
    }

    fn config(&self) -> RepoConfig {
        read_workspace_config(&self.repo).unwrap()
    }

    fn error(&self, path: &str, message: &str) {
        let error = read_workspace_config(&self.repo).unwrap_err();
        assert_eq!(error.path, self.repo.join(path));
        assert!(error.message.contains(message), "{error}");
    }

    fn run(&self, args: &[&str], code: i32) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
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

    fn json(&self, args: &[&str], code: i32) -> Value {
        serde_json::from_slice(&self.run(&[args, &["--json"]].concat(), code).stdout).unwrap()
    }
}

fn runtime(name: &str, args: &[&str]) -> Value {
    json!({
        "name":name,
        "evals":[
            {
                "id":"check",
                "title":"Check",
                "profile":{"kind":"runtime","command":support::os::bin("/bin/sh"),"args":args},
                "payload":{"instruction":"Check."},
            },
        ],
    })
}

#[test]
fn root_and_nested_sidecars_coexist_without_changing_folder_ownership() {
    let fixture = Fixture::new();
    fixture.declare("index.artf", json!({"name":"folder"}));
    fixture.write("hero.png", "pixels");
    fixture.declare("hero.png.artf", json!({"name":"hero"}));
    fixture.write("nested/Makefile", "all:");
    fixture.declare("nested/Makefile.artf", json!({"name":"build"}));
    fixture.declare("nested/deep/index.artf", json!({"name":"deep"}));
    let config = fixture.config();
    assert_eq!(config.artifacts.len(), 4);
    assert_eq!(config.artifacts["hero"].kind, ArtifactKind::File);
    assert_eq!(config.artifacts["hero"].path, Path::new("hero.png"));
    assert_eq!(config.artifacts["build"].folder(), Path::new("nested"));
    assert_eq!(
        config.artifacts["build"].declaration_path(),
        Path::new("nested/Makefile.artf")
    );
    assert_eq!(
        config.artifacts["folder"].children,
        std::collections::BTreeMap::from([("nested/deep".into(), "deep".parse().unwrap())])
    );
    assert!(config.artifacts["hero"].children.is_empty());
    assert!(config.artifacts["build"].children.is_empty());
    assert_eq!(config.relations.len(), 1);
    assert!(
        !artifact_scope(&config, &["folder"])
            .unwrap()
            .artifacts
            .contains_key("hero")
    );
    assert_eq!(
        artifact_scope(&config, &["hero"]).unwrap().artifacts.len(),
        1
    );
}

#[test]
fn a_workspace_may_contain_only_a_file_artifact_and_index_is_always_a_folder_marker() {
    let fixture = Fixture::new();
    fixture.write("Makefile", "all:");
    fixture.declare("Makefile.artf", json!({"name":"build"}));
    assert_eq!(fixture.config().artifacts["build"].kind, ArtifactKind::File);
    fixture.write("index", "ordinary file");
    // There is no sidecar spelling for the file `index`: index.artf always owns the folder.
    fixture.declare("index.artf", json!({"name":"index-file"}));
    let config = fixture.config();
    assert_eq!(config.artifacts["index-file"].kind, ArtifactKind::Folder);
    assert_eq!(config.artifacts["index-file"].path, Path::new(""));
    assert!(config.artifacts["index-file"].children.is_empty());
}

#[test]
fn missing_directory_declaration_and_declaration_targets_are_rejected_at_the_sidecar() {
    for (target, setup, message) in [
        ("missing.txt", 0, "is missing"),
        ("directory", 1, "must be a regular file"),
        ("input.artf", 2, "must not be a declaration"),
        ("input", 3, "declaration must be a regular file"),
    ] {
        let fixture = Fixture::new();
        let sidecar = format!("{target}.artf");
        if setup == 1 {
            fs::create_dir(fixture.repo.join(target)).unwrap();
        }
        if setup == 2 {
            fixture.write(target, "name = 'declaration'");
        }
        if setup == 3 {
            fixture.write(target, "input");
            fs::create_dir(fixture.repo.join(&sidecar)).unwrap();
        } else {
            fixture.declare(&sidecar, json!({"name":"file"}));
        }
        fixture.error(&sidecar, message);
    }
    let fixture = Fixture::new();
    fixture.declare(".artf", json!({"name":"file"}));
    fixture.error(".artf", "must not be a declaration");
}

#[test]
fn symlink_targets_and_declarations_are_rejected() {
    let fixture = Fixture::new();
    fixture.write("actual", "input");
    if support::os::symlink_file(fixture.repo.join("actual"), fixture.repo.join("linked")).is_none()
    {
        return;
    }
    fixture.declare("linked.artf", json!({"name":"file"}));
    fixture.error("linked.artf", "not a symlink");
    fs::remove_file(fixture.repo.join("linked.artf")).unwrap();
    fixture.write("actual-declaration", "name = 'file'");
    if support::os::symlink_file(
        fixture.repo.join("actual-declaration"),
        fixture.repo.join("actual.artf"),
    )
    .is_none()
    {
        return;
    }
    fixture.error("actual.artf", "declaration must be a regular file");
}

// Sockets and FIFOs are Unix entries; Windows has no special files to declare.
#[cfg(unix)]
#[test]
fn special_targets_and_declarations_are_rejected() {
    let fixture = Fixture::new();
    let _socket = support::os::socket(&fixture.repo.join("socket"));
    fixture.declare("socket.artf", json!({"name":"file"}));
    fixture.error("socket.artf", "special file");
    fs::remove_file(fixture.repo.join("socket.artf")).unwrap();
    fixture.write("input", "input");
    let _declaration = support::os::socket(&fixture.repo.join("input.artf"));
    fixture.error("input.artf", "declaration must be a regular file");
}

#[test]
fn names_are_required_and_globally_unique_and_policy_is_root_folder_only() {
    let fixture = Fixture::new();
    fixture.write("input", "input");
    fixture.write("input.artf", "basis = true");
    fixture.error("input.artf", "missing field `name`");
    fixture.declare("index.artf", json!({"name":"folder"}));
    fixture.declare("input.artf", json!({"name":"folder"}));
    fixture.error("input.artf", "Duplicate Artifact name");
    fixture.declare(
        "input.artf",
        json!({"name":"file","review_policy":{"dependency_gates":"ignore"}}),
    );
    fixture.error("input.artf", "root index.artf");
    fixture.declare("input.artf", json!({"name":"file"}));
    fixture.write("nested/input", "input");
    fixture.declare("nested/input.artf", json!({"name":"file"}));
    fixture.error("nested/input.artf", "Duplicate Artifact name");
}

#[test]
fn explicit_file_fingerprints_allow_only_the_target_and_reject_ignore_even_if_empty() {
    let fixture = Fixture::new();
    fixture.write("input.txt", "input");
    for fingerprint in [
        json!({"files":["."]}),
        json!({"files":["sibling"]}),
        json!({}),
        json!({"files":["input.txt"],"ignore":[]}),
    ] {
        fixture.declare(
            "input.txt.artf",
            json!({"name":"file","fingerprint":fingerprint}),
        );
        assert!(read_workspace_config(&fixture.repo).is_err());
    }
    fixture.declare(
        "input.txt.artf",
        json!({"name":"file","fingerprint":{"files":["input.txt"]}}),
    );
    assert!(matches!(
        &fixture.config().artifacts["file"].fingerprint,
        Some(Fingerprint::Artifactsum { files, .. }) if files == &["input.txt"]
    ));
    fixture.declare("input.txt.artf", json!({"name":"file","fingerprint":false}));
    assert!(fixture.config().artifacts["file"].fingerprint.is_none());
}

async fn fingerprints(
    config: &RepoConfig,
    output: &Path,
) -> std::collections::BTreeMap<String, cache::PreparedFingerprint> {
    cache::prepare(
        config,
        config.artifacts.keys().map(|name| name.as_str()),
        output,
        &Parallelism::new(2),
        CancellationToken::new(),
    )
    .await
    .unwrap()
    .into_iter()
    .map(|(id, fingerprint)| (id.to_owned(), fingerprint))
    .collect()
}

#[tokio::test]
async fn file_artifactsum_hashes_only_owner_relative_target_bytes_while_folder_still_covers_it() {
    let fixture = Fixture::new();
    fixture.declare("index.artf", json!({"name":"folder"}));
    fixture.write("nested/input.txt", "original");
    fixture.declare("nested/input.txt.artf", json!({"name":"file"}));
    fixture.write("nested/sibling", "sibling");
    let first = fingerprints(&fixture.config(), &fixture.state).await;
    // Fixed wire-format golden, independent of the production digest implementation.
    assert_eq!(
        first["file"].value.to_string(),
        "artifactsum:6f987753e034e0a93c2498dd75777512502d3bdb2b6cd8d3a9d03b4b48aaf579"
    );
    assert_eq!(
        first["file"]
            .manifest
            .as_ref()
            .unwrap()
            .files
            .as_ref()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        [&"input.txt".to_owned()]
    );
    fixture.declare(
        "nested/input.txt.artf",
        json!({"name":"file","tags":["edited"]}),
    );
    let declaration = fingerprints(&fixture.config(), &fixture.state).await;
    assert_eq!(first["folder"].value, declaration["folder"].value);
    assert_eq!(first["file"].value, declaration["file"].value);
    fixture.write("nested/sibling", "changed");
    let sibling = fingerprints(&fixture.config(), &fixture.state).await;
    assert_eq!(first["file"].value, sibling["file"].value);
    assert_ne!(first["folder"].value, sibling["folder"].value);
    fixture.write("nested/input.txt", "changed");
    let target = fingerprints(&fixture.config(), &fixture.state).await;
    assert_ne!(sibling["file"].value, target["file"].value);
    assert_ne!(sibling["folder"].value, target["folder"].value);
}

async fn call(registry: &Registry<'_>, name: &str, args: Value, state: &Path) -> Value {
    let result = registry
        .call(name, args, state, CancellationToken::new())
        .await;
    assert!(!result.is_error, "{result:?}");
    let [Content::Json { data }] = result.content.as_slice() else {
        panic!("{result:?}")
    };
    data.clone()
}

#[tokio::test]
async fn builtins_expose_only_the_target_mounts_and_explicit_references() {
    let fixture = Fixture::new();
    fixture.write("files/target.txt", "visible needle\n");
    fixture.write("files/sibling.txt", "secret needle\n");
    fixture.declare("files/child/index.artf", json!({"name":"child"}));
    fixture.declare("files/target.txt.artf", json!({
        "name":"file",
        "mounts":{"external":"other"},
        "views":{
            "agent_tools":{
                "list":{"builtin":"list"},
                "glob":{"builtin":"glob"},
                "grep":{"builtin":"grep"},
                "read":{"builtin":"read"},
            },
        },
        "evals":[
            {
                "id":"review",
                "title":"Review",
                "profile":{"kind":"agent","backend":"openai","model":"offline","reasoning":"high"},
                "payload":{"instruction":"Read {reference}."},
            },
        ],
    }));
    fixture.write("other/input.txt", "mounted needle\n");
    fixture.declare("other/input.txt.artf", json!({"name":"other"}));
    fixture.write("references/rules", "rules");
    fixture.declare(
        "references/rules.artf",
        json!({
            "name":"reference",
            "basis":true,
            "views":{"agent_tools":{"read":{"builtin":"read"}}},
        }),
    );
    let config = fixture.config();
    let registry = Registry::new(&config, "file/review").unwrap();
    let names = registry
        .list()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"read_reference"));
    assert!(!names.iter().any(|name| name.ends_with("_child")));
    let listing = call(&registry, "list_file", json!({}), &fixture.state).await;
    assert_eq!(
        call(&registry, "list_file", json!({"path":"."}), &fixture.state).await,
        listing
    );
    assert_eq!(
        listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["external", "target.txt"]
    );
    let glob = call(
        &registry,
        "glob_file",
        json!({"pattern":"**/*"}),
        &fixture.state,
    )
    .await;
    assert_eq!(glob["files"], json!(["external/input.txt", "target.txt"]));
    let grep = call(
        &registry,
        "grep_file",
        json!({"pattern":"needle"}),
        &fixture.state,
    )
    .await;
    assert_eq!(grep["matches"].as_array().unwrap().len(), 2);
    assert_eq!(
        call(
            &registry,
            "read_file",
            json!({"path":"target.txt"}),
            &fixture.state
        )
        .await["lines"][0]["text"],
        "visible needle\n"
    );
    assert_eq!(
        call(
            &registry,
            "read_file",
            json!({"path":"external/input.txt"}),
            &fixture.state
        )
        .await["lines"][0]["text"],
        "mounted needle\n"
    );
    assert_eq!(
        call(
            &registry,
            "read_reference",
            json!({"path":"rules"}),
            &fixture.state
        )
        .await["lines"][0]["text"],
        "rules"
    );
    for (tool, args) in [
        ("read_file", json!({"path":"sibling.txt"})),
        ("read_file", json!({"path":"target.txt.artf"})),
        ("read_file", json!({"path":"child/index.artf"})),
        ("read_file", json!({"path":"../other/input.txt"})),
        ("glob_file", json!({"path":"child","pattern":"**/*"})),
        (
            "grep_file",
            json!({"path":"sibling.txt","pattern":"secret"}),
        ),
    ] {
        assert!(
            registry
                .call(tool, args, &fixture.state, CancellationToken::new())
                .await
                .is_error
        );
    }
}

#[test]
fn file_mount_alias_cannot_shadow_the_target() {
    let fixture = Fixture::new();
    fixture.write("Makefile", "all:");
    fixture.declare("other/index.artf", json!({"name":"other"}));
    fixture.declare(
        "Makefile.artf",
        json!({"name":"file","mounts":{"Makefile":"other"}}),
    );
    fixture.error("Makefile.artf", "conflicts with the target file");
}

#[test]
fn argv_and_instruction_references_resolve_to_files_and_reject_file_suffixes() {
    let fixture = Fixture::new();
    fixture.write("files/input.txt", "input");
    fixture.declare(
        "files/input.txt.artf",
        runtime("file", &["{file}", "--input={file}"]),
    );
    let config = fixture.config();
    let eval = &config.evals[0];
    let args = resolve_argv(
        &config,
        &eval_scope(&config, eval).unwrap(),
        "file",
        &["{file}".into(), "--input={file}".into()],
    )
    .unwrap();
    let file = support::os::canonical(&config.root.join("files/input.txt"))
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(args, [file.clone(), format!("--input={file}")]);
    for args in [["{file}/sub"], ["{file}/input.txt"]] {
        fixture.declare("files/input.txt.artf", runtime("file", &args));
        fixture.error("files/input.txt.artf", "cannot have a /path suffix");
    }
    fixture.declare(
        "files/input.txt.artf",
        json!({
            "name":"file",
            "evals":[
                {
                    "id":"review",
                    "title":"Review",
                    "profile":{"kind":"human"},
                    "payload":{"instruction":"Read {file}/sub."},
                },
            ],
        }),
    );
    fixture.error("files/input.txt.artf", "cannot have a /path suffix");
    fixture.declare(
        "files/input.txt.artf",
        json!({"name":"file","fingerprint":{"script":{"command":"echo","args":["{file}/sub"]}}}),
    );
    fixture.error("files/input.txt.artf", "cannot have a /path suffix");
}

#[test]
fn human_tools_use_containing_folder_and_resolve_only_scoped_paths() {
    let fixture = Fixture::new();
    fixture.write("files/input.txt", "input");
    fixture.declare(
        "files/input.txt.artf",
        json!({
            "name":"file",
            "views":{
                "human_tools":{
                    "open":{
                        "description":"Open",
                        "kind":"output",
                        "command":support::os::bin("/bin/cat"),
                        "args":["{artifactPath}","{file}"],
                    },
                },
            },
        }),
    );
    let config = fixture.config();
    let registry = artifactize::tools::human::Registry::for_artifact(&config, "file").unwrap();
    let command = registry.command("open_file").unwrap();
    assert_eq!(command.cwd, config.root.join("files"));
    assert_eq!(
        command.args,
        vec![
            support::os::canonical(&config.root.join("files/input.txt"))
                .to_str()
                .unwrap();
            2
        ]
    );
    fixture.declare(
        "files/input.txt.artf",
        json!({
            "name":"file",
            "views":{
                "human_tools":{
                    "open":{
                        "description":"Open",
                        "kind":"output",
                        "command":"cat",
                        "args":["{artifactPath}/sibling"],
                    },
                },
            },
        }),
    );
    fixture.error("files/input.txt.artf", "cannot have a /path suffix");
}

#[tokio::test]
async fn fingerprint_scripts_run_in_containing_folder_with_file_arguments_and_relative_scripts() {
    let fixture = Fixture::new();
    fixture.write("files/input.txt", "input");
    fixture.write(
        "files/hash.sh",
        "test -f input.txt && test \"$1\" -ef input.txt && printf file-v1\n",
    );
    fixture.declare(
        "files/input.txt.artf",
        json!({
            "name":"file",
            "fingerprint":{
                "script":{
                    "command":support::os::bin("/bin/sh"),
                    "args":["hash.sh","{file}"],
                    "files":["hash.sh"],
                },
            },
        }),
    );
    let result = fingerprints(&fixture.config(), &fixture.state).await;
    assert_eq!(result["file"].value.to_string(), "file-v1");
}

#[tokio::test]
async fn runtime_verify_reuses_after_sibling_changes_but_not_target_changes_and_displays_file_kind()
{
    let fixture = Fixture::new();
    fixture.write("files/input.txt", "input");
    fixture.declare(
        "files/input.txt.artf",
        runtime(
            "file",
            &[
                "-c",
                "test -f input.txt && test \"$1\" -ef input.txt",
                "sh",
                "{file}",
            ],
        ),
    );
    let first = fixture.json(&["verify", "--all"], 0);
    assert_eq!(first["requests"][0]["result"]["verdict"], "GREEN");
    assert_eq!(
        first["requests"][0]["cwd"],
        support::os::path_text(&fixture.config().root.join("files"))
    );
    fixture.write("files/sibling", "unrelated");
    let second = fixture.json(&["verify", "--all"], 0);
    assert_eq!(
        first["requests"][0]["executionId"],
        second["requests"][0]["executionId"]
    );
    fixture.write("files/input.txt", "changed");
    let third = fixture.json(&["verify", "--all"], 0);
    assert_ne!(
        second["requests"][0]["executionId"],
        third["requests"][0]["executionId"]
    );
    let graph = fixture.json(&["config", "graph"], 0);
    assert_eq!(graph["artifacts"]["file"]["kind"], "file");
    assert_eq!(graph["artifacts"]["file"]["path"], "files/input.txt");
    assert!(
        String::from_utf8(fixture.run(&["config", "graph"], 0).stdout)
            .unwrap()
            .contains("[file] (files/input.txt)")
    );
    let status = fixture.json(&["status"], 0);
    assert_eq!(status["artifacts"][0]["kind"], "file");
    assert_eq!(status["artifacts"][0]["path"], "files/input.txt");
    assert!(
        String::from_utf8(fixture.run(&["status"], 0).stdout)
            .unwrap()
            .contains("[file: files/input.txt]")
    );
    let saved = store::read_run(&fixture.state, third["id"].as_str().unwrap())
        .await
        .unwrap();
    let detail = monitor::detail(
        &saved,
        &[],
        &Target::Artifact("file".into()),
        time::OffsetDateTime::now_utc(),
    );
    let nodes = monitor::tree(&saved, &[], time::OffsetDateTime::now_utc());
    assert!(
        nodes
            .iter()
            .any(|node| node.id == "a:file" && node.marks.contains("[file]"))
    );
    assert!(detail.title.contains("[file]"));
    assert_eq!(detail.field("Path"), Some("files/input.txt"));
    assert_eq!(
        serde_json::to_value(&saved.run.definitions).unwrap()["artifacts"]["file"]["kind"],
        "file"
    );
}

#[tokio::test]
async fn dependency_evals_can_live_on_files_and_target_files() {
    let fixture = Fixture::new();
    fixture.write("input.txt", "input");
    fixture.declare("input.txt.artf", runtime("input", &["-c", "exit 0"]));
    fixture.write("ready.txt", "ready");
    fixture.declare("ready.txt.artf", json!({
        "name":"ready",
        "evals":[
            {"id":"ready","title":"Ready","profile":{"kind":"dependency","depends_on":["input"]}},
        ],
    }));
    fixture.declare("folder/index.artf", json!({
        "name":"folder",
        "evals":[
            {"id":"ready","title":"Ready","profile":{"kind":"dependency","depends_on":["ready"]}},
        ],
    }));
    let run = fixture.json(&["verify", "--all"], 0);
    for id in ["ready/ready", "folder/ready"] {
        let request = run["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|request| request["evalId"] == id)
            .unwrap();
        assert_eq!(request["status"], "GREEN");
        assert!(request["executionId"].is_null());
    }
    fixture.declare("input.txt.artf", runtime("input", &["-c", "exit 1"]));
    let run = fixture.json(&["verify", "--all"], 1);
    let blocked = run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|request| request["evalId"] == "ready/ready")
        .unwrap();
    assert_ne!(blocked["status"], "GREEN");
    assert!(
        blocked["blockedBy"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == "input/check")
    );
}

#[tokio::test]
async fn command_tools_run_in_the_containing_folder_and_preserve_file_scope_metadata() {
    let fixture = Fixture::new();
    fixture.write("files/input.txt", "input");
    fixture.write(
        "files/tool.py",
        "import json,os,sys\nx=json.load(sys.stdin)\nprint(json.dumps({'content':[{'type':'json','data':{'cwd':os.getcwd(),'argv':sys.argv[1:],'artifactPath':x['context']['artifactPath'],'scope':x['context']['scope']}}]}))\n",
    );
    fixture.declare(
        "files/input.txt.artf",
        json!({
            "name":"file",
            "views":{
                "agent_tools":{
                    "inspect":{
                        "description":"Inspect",
                        "protocol":"json",
                        "command":"python3",
                        "args":["tool.py","{file}"],
                    },
                },
            },
        }),
    );
    let config = fixture.config();
    let registry = Registry::for_artifact(&config, "file").unwrap();
    registry.preflight("inspect_file").unwrap();
    let data = call(&registry, "inspect_file", json!({}), &fixture.state).await;
    assert_eq!(data["cwd"], config.root.join("files").to_str().unwrap());
    assert_eq!(
        data["argv"][0],
        support::os::canonical(&config.root.join("files/input.txt"))
            .to_str()
            .unwrap()
    );
    assert_eq!(
        data["artifactPath"],
        support::os::canonical(&config.root.join("files/input.txt"))
            .to_str()
            .unwrap()
    );
    assert_eq!(data["scope"]["file"]["kind"], "file");
    assert_eq!(
        data["scope"]["file"]["path"],
        support::os::canonical(&config.root.join("files/input.txt"))
            .to_str()
            .unwrap()
    );
}

#[tokio::test]
async fn changed_targets_cannot_be_replaced_by_directories_or_symlinks_after_discovery() {
    let fixture = Fixture::new();
    fixture.write("input.txt", "input");
    fixture.declare(
        "input.txt.artf",
        json!({
            "name":"file",
            "views":{"agent_tools":{"read":{"builtin":"read"},"list":{"builtin":"list"}}},
        }),
    );
    let config = fixture.config();
    let registry = Registry::for_artifact(&config, "file").unwrap();
    fs::remove_file(fixture.repo.join("input.txt")).unwrap();
    fs::create_dir(fixture.repo.join("input.txt")).unwrap();
    fixture.write("input.txt/secret", "secret");
    assert!(
        cache::prepare(
            &config,
            ["file"],
            &fixture.state,
            &Parallelism::new(1),
            CancellationToken::new()
        )
        .await
        .unwrap_err()
        .contains("must remain a regular file")
    );
    for (tool, args) in [
        ("read_file", json!({"path":"input.txt"})),
        ("read_file", json!({"path":"input.txt/secret"})),
        ("list_file", json!({})),
    ] {
        assert!(
            registry
                .call(tool, args, &fixture.state, CancellationToken::new())
                .await
                .is_error
        );
    }
    fs::remove_dir_all(fixture.repo.join("input.txt")).unwrap();
    fixture.write("sibling.txt", "secret");
    if support::os::symlink_file(
        fixture.repo.join("sibling.txt"),
        fixture.repo.join("input.txt"),
    )
    .is_none()
    {
        return;
    }
    assert!(
        registry
            .call(
                "read_file",
                json!({"path":"input.txt"}),
                &fixture.state,
                CancellationToken::new()
            )
            .await
            .is_error
    );
}

#[test]
fn json_tool_file_references_reject_suffixes_at_the_declaration() {
    let fixture = Fixture::new();
    fixture.write("input", "input");
    fixture.declare(
        "input.artf",
        json!({
            "name":"file",
            "views":{
                "agent_tools":{
                    "inspect":{
                        "description":"Inspect",
                        "protocol":"json",
                        "command":"cat",
                        "args":["{file}/sibling"],
                    },
                },
            },
        }),
    );
    fixture.error("input.artf", "cannot have a /path suffix");
}

#[test]
fn artfignore_skips_sidecars_in_excluded_folders() {
    let fixture = Fixture::new();
    fixture.declare("index.artf", json!({"name":"folder"}));
    fixture.write("excluded/missing.artf", "invalid TOML");
    fixture.write(".artfignore", "excluded/\n");
    assert_eq!(fixture.config().artifacts.len(), 1);
}

#[tokio::test]
async fn relative_and_mounted_tool_executables_work_on_file_artifacts() {
    let fixture = Fixture::new();
    fixture.write("files/input.txt", "input");
    fs::create_dir_all(fixture.repo.join("files")).unwrap();
    support::os::write_script(
        &fixture.repo.join("files/inspect"),
        "test -f input.txt && printf files\n",
    );
    support::os::make_executable(&fixture.repo.join("files/inspect"));
    fixture.declare(
        "files/input.txt.artf",
        json!({
            "name":"file",
            "views":{
                "agent_tools":{
                    "inspect":{
                        "description":"Inspect",
                        "protocol":"plain",
                        "command":"./inspect",
                        "args":[],
                    },
                },
            },
        }),
    );
    let config = fixture.config();
    let registry = Registry::for_artifact(&config, "file").unwrap();
    registry.preflight("inspect_file").unwrap();
    let result = registry
        .call(
            "inspect_file",
            json!({}),
            &fixture.state,
            CancellationToken::new(),
        )
        .await;
    assert!(!result.is_error, "{result:?}");
    assert_eq!(
        result.content,
        vec![Content::Text {
            text: "files".into()
        }]
    );
    fs::create_dir_all(fixture.repo.join("tools")).unwrap();
    support::os::write_script(&fixture.repo.join("tools/inspect"), "printf mounted\n");
    support::os::make_executable(&fixture.repo.join("tools/inspect"));
    fixture.declare("tools/index.artf", json!({"name":"tools","basis":true}));
    fixture.declare(
        "files/input.txt.artf",
        json!({
            "name":"file",
            "mounts":{"bin":"tools"},
            "views":{
                "agent_tools":{
                    "inspect":{
                        "description":"Inspect",
                        "protocol":"plain",
                        "command":"bin/inspect",
                        "args":[],
                    },
                },
            },
        }),
    );
    let config = fixture.config();
    let registry = Registry::for_artifact(&config, "file").unwrap();
    registry.preflight("inspect_file").unwrap();
    let result = registry
        .call(
            "inspect_file",
            json!({}),
            &fixture.state,
            CancellationToken::new(),
        )
        .await;
    assert!(!result.is_error, "{result:?}");
    assert_eq!(
        result.content,
        vec![Content::Text {
            text: "mounted".into()
        }]
    );
}

#[test]
fn sidecar_checks_keep_s2_source_locations_and_literal_owner_data() {
    let fixture = Fixture::new();
    fixture.write("input.txt", "input");
    fixture.write(
        "input.txt.artf",
        "name = 'file'\n[evals.review]\ntitle = 'Review'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.', owner = { '$__toml_private_datetime' = 'ordinary string' } }\n",
    );
    assert_eq!(
        fixture.config().evals[0]
            .declaration
            .payload
            .as_ref()
            .unwrap()
            .extra["owner"]["$__toml_private_datetime"],
        "ordinary string"
    );
    fixture.write(
        "input.txt.artf",
        "name = 'file'\n[evals.review]\ntitle = 'Review'\nprofile = { kind = 'human' }\npayload.instruction = 'Check {file}/sub.'\n",
    );
    let error = read_workspace_config(&fixture.repo).unwrap_err();
    assert!(
        error.message.contains("line 5") && error.message.contains("cannot have a /path suffix"),
        "{error}"
    );
    fixture.write(
        "input.txt.artf",
        "name = 'file'\nfingerprint = { files = ['input.txt'], ignore = [] }\n",
    );
    let error = read_workspace_config(&fixture.repo).unwrap_err();
    assert!(
        error.message.contains("line 2") && error.message.contains("fingerprint.ignore"),
        "{error}"
    );
}

#[test]
fn changing_folder_to_file_does_not_reuse_equal_artifactsum_evidence() {
    let fixture = Fixture::new();
    fixture.write("input.txt", "same");
    let declaration = runtime("file", &["-c", "exit 0"]);
    fixture.declare("index.artf", declaration.clone());
    let folder = fixture.json(&["verify", "--all"], 0);
    fs::remove_file(fixture.repo.join("index.artf")).unwrap();
    fixture.declare("input.txt.artf", declaration);
    let file = fixture.json(&["verify", "--all"], 0);
    assert_eq!(
        folder["requests"][0]["fingerprint"],
        file["requests"][0]["fingerprint"]
    );
    assert_ne!(folder["requests"][0]["key"], file["requests"][0]["key"]);
    assert_ne!(
        folder["requests"][0]["executionId"],
        file["requests"][0]["executionId"]
    );
}

#[test]
fn reuse_key_covers_kinds_of_target_mount_child_and_reference_but_never_paths_or_tags() {
    let fixture = Fixture::new();
    fixture.declare(
        "owner/index.artf",
        json!({
            "name":"owner",
            "mounts":{"mounted":"mount"},
            "evals":[
                {
                    "id":"check",
                    "title":"Check",
                    "profile":{"kind":"runtime","command":"true","args":[]},
                    "payload":{"instruction":"Check {reference}."},
                },
            ],
        }),
    );
    fixture.declare(
        "owner/child/index.artf",
        json!({"name":"child","basis":true}),
    );
    fixture.declare("mount/index.artf", json!({"name":"mount","basis":true}));
    fixture.declare(
        "reference/index.artf",
        json!({"name":"reference","basis":true}),
    );
    let mut config = fixture.config();
    let fingerprints = ["owner", "mount", "child", "reference"]
        .into_iter()
        .map(|id| {
            (
                id,
                cache::PreparedFingerprint {
                    value: "same".parse().unwrap(),
                    manifest: None,
                },
            )
        })
        .collect();
    let key = cache::eval_key(&config, &config.evals[0], &fingerprints)
        .unwrap()
        .value;
    for id in ["owner", "mount", "child", "reference"] {
        config.artifacts.get_mut(id).unwrap().kind = ArtifactKind::File;
        assert_ne!(
            key,
            cache::eval_key(&config, &config.evals[0], &fingerprints)
                .unwrap()
                .value,
            "{id}"
        );
        config.artifacts.get_mut(id).unwrap().kind = ArtifactKind::Folder;
        config.artifacts.get_mut(id).unwrap().path = PathBuf::from(format!("moved/{id}"));
        config.artifacts.get_mut(id).unwrap().tags = vec!["changed".into()];
        assert_eq!(
            key,
            cache::eval_key(&config, &config.evals[0], &fingerprints)
                .unwrap()
                .value
        );
    }
}

#[test]
fn runtime_target_replacements_fail_even_with_constant_or_disabled_fingerprints() {
    for script in ["rm input.txt; mkdir input.txt", "rm input.txt"] {
        for fingerprint in [
            json!(false),
            json!({"script":{"command":support::os::bin("/bin/echo"),"args":["constant"]}}),
        ] {
            let fixture = Fixture::new();
            fixture.write("input.txt", "input");
            let mut declaration = runtime("file", &["-c", script]);
            declaration["fingerprint"] = fingerprint;
            fixture.declare("input.txt.artf", declaration);
            let run = fixture.json(&["verify", "--all"], 2);
            assert_eq!(run["requests"][0]["status"], "ERROR");
            assert_eq!(run["requests"][0]["errorCode"], "INPUT_CHANGED");
        }
    }
}

#[tokio::test]
async fn script_fingerprint_preparation_and_recheck_reject_every_invalid_target_kind() {
    for kind in ["missing", "directory", "symlink"] {
        let fixture = Fixture::new();
        fixture.write("input.txt", "input");
        let mut declaration = runtime("file", &["-c", "exit 0"]);
        declaration["fingerprint"] =
            json!({"script":{"command":support::os::bin("/bin/echo"),"args":["constant"]}});
        fixture.declare("input.txt.artf", declaration);
        let config = fixture.config();
        fs::remove_file(fixture.repo.join("input.txt")).unwrap();
        if kind == "directory" {
            fs::create_dir(fixture.repo.join("input.txt")).unwrap();
        }
        if kind == "symlink" {
            fixture.write("sibling.txt", "sibling");
            if support::os::symlink_file(
                fixture.repo.join("sibling.txt"),
                fixture.repo.join("input.txt"),
            )
            .is_none()
            {
                continue;
            }
        }
        assert!(
            cache::prepare(
                &config,
                ["file"],
                &fixture.state,
                &Parallelism::new(1),
                CancellationToken::new()
            )
            .await
            .unwrap_err()
            .contains("File Artifact")
        );
        assert!(
            cache::recheck(
                &config,
                &config.evals[0],
                &fixture.state,
                &Parallelism::new(1),
                CancellationToken::new()
            )
            .await
            .unwrap_err()
            .contains("File Artifact")
        );
    }
}

#[test]
fn disabled_fingerprint_target_deleted_by_dependency_script_is_not_executed() {
    let fixture = Fixture::new();
    fixture.write("files/input.txt", "input");
    let mut declaration = runtime("file", &["-c", "touch started"]);
    declaration["fingerprint"] = json!(false);
    declaration["mounts"] = json!({"dep":"dep"});
    fixture.declare("files/input.txt.artf", declaration);
    fixture.declare(
        "dep/index.artf",
        json!({
            "name":"dep",
            "basis":true,
            "fingerprint":{
                "script":{
                    "command":support::os::bin("/bin/sh"),
                    "args":["-c","rm ../files/input.txt; printf constant"],
                },
            },
        }),
    );
    let run = fixture.json(&["verify", "--all"], 2);
    assert_eq!(run["requests"][0]["errorCode"], "PREPARATION_FAILED");
    assert!(!fixture.repo.join("files/started").exists());
}

#[tokio::test]
async fn mount_alias_can_shadow_a_sibling_not_visible_in_the_file_virtual_root() {
    let fixture = Fixture::new();
    fixture.write("target", "target");
    fixture.write("alias", "hidden sibling");
    fixture.write("other/visible", "mounted");
    fixture.declare("other/index.artf", json!({"name":"other","basis":true}));
    fixture.declare(
        "target.artf",
        json!({
            "name":"file",
            "mounts":{"alias":"other"},
            "views":{"agent_tools":{"list":{"builtin":"list"},"read":{"builtin":"read"}}},
        }),
    );
    let config = fixture.config();
    let registry = Registry::for_artifact(&config, "file").unwrap();
    let listing = call(&registry, "list_file", json!({}), &fixture.state).await;
    assert_eq!(listing["entries"][0]["kind"], "mount");
    assert_eq!(
        call(
            &registry,
            "read_file",
            json!({"path":"alias/visible"}),
            &fixture.state
        )
        .await["lines"][0]["text"],
        "mounted"
    );
}

#[tokio::test]
async fn command_and_human_tools_revalidate_file_targets_on_every_call() {
    for kind in ["directory", "missing", "symlink"] {
        let fixture = Fixture::new();
        fixture.write("input.txt", "input");
        fixture.declare(
            "input.txt.artf",
            json!({
                "name":"file",
                "views":{
                    "agent_tools":{
                        "inspect":{
                            "description":"Inspect",
                            "protocol":"json",
                            "command":"python3",
                            "args":["-c","open('started','w').write('yes');print('{}')"],
                        },
                    },
                    "human_tools":{
                        "inspect":{
                            "description":"Inspect",
                            "kind":"output",
                            "command":support::os::bin("/bin/sh"),
                            "args":["-c","touch started"],
                        },
                    },
                },
            }),
        );
        let config = fixture.config();
        let agent = Registry::for_artifact(&config, "file").unwrap();
        let human = artifactize::tools::human::Registry::for_artifact(&config, "file").unwrap();
        fs::remove_file(fixture.repo.join("input.txt")).unwrap();
        if kind == "directory" {
            fs::create_dir(fixture.repo.join("input.txt")).unwrap();
        }
        if kind == "symlink" {
            fixture.write("sibling", "secret");
            if support::os::symlink_file(
                fixture.repo.join("sibling"),
                fixture.repo.join("input.txt"),
            )
            .is_none()
            {
                continue;
            }
        }
        assert!(
            agent
                .call(
                    "inspect_file",
                    json!({}),
                    &fixture.state,
                    CancellationToken::new()
                )
                .await
                .is_error
        );
        assert!(
            human
                .call("inspect_file", CancellationToken::new())
                .await
                .is_error
        );
        assert!(!fixture.repo.join("started").exists());
    }
}

#[test]
fn artfignore_applies_to_individual_sidecars_and_declaration_target_prechecks() {
    let fixture = Fixture::new();
    fixture.declare("index.artf", json!({"name":"folder"}));
    fixture.write("scratch.txt.artf", "invalid TOML without a target");
    fixture.write("nested/draft.artf.artf", "invalid declaration target");
    fixture.write(".artfignore", "scratch.txt.artf\nnested/*.artf\n");
    assert_eq!(fixture.config().artifacts.len(), 1);
    fixture.write(".artfignore", "scratch.txt.artf\n");
    fixture.error("nested/draft.artf.artf", "must not be a declaration");
}

// Windows file names cannot contain ':' at all.
#[cfg(unix)]
#[test]
fn colon_targets_are_rejected_before_scoped_tools_can_be_declared() {
    let fixture = Fixture::new();
    fixture.write("a:b", "input");
    fixture.declare("a:b.artf", json!({"name":"file","fingerprint":false}));
    fixture.error("a:b.artf", "must not contain ':'");
}

// Search-only (execute without read) directories are a Unix permission.
#[cfg(unix)]
#[test]
fn verify_accepts_state_below_search_only_ancestors_but_still_probes_fixed_markers() {
    let fixture = Fixture::new();
    fixture.declare("index.artf", runtime("folder", &["-c", "exit 0"]));
    let ancestor = fixture._root.path().join("search-only");
    let parent = ancestor.join("writable");
    fs::create_dir_all(&parent).unwrap();
    support::os::set_mode(&ancestor, 0o111);
    let state = parent.join("state");
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--repo")
        .arg(&fixture.repo)
        .arg("--state-dir")
        .arg(&state)
        .args(["verify", "--all", "--json"])
        .output()
        .unwrap();
    support::os::set_mode(&ancestor, 0o700);
    assert!(output.status.success(), "{output:?}");
    fs::write(ancestor.join("artifactize.json"), "legacy marker").unwrap();
    support::os::set_mode(&ancestor, 0o111);
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--repo")
        .arg(&fixture.repo)
        .arg("--state-dir")
        .arg(parent.join("blocked-state"))
        .args(["verify", "--all", "--json"])
        .output()
        .unwrap();
    support::os::set_mode(&ancestor, 0o700);
    assert!(!output.status.success());
    assert!(!parent.join("blocked-state").exists());
}

#[tokio::test]
async fn remote_records_roundtrip_file_kinds_and_cannot_match_legacy_or_tampered_keys() {
    let fixture = Fixture::new();
    fixture.write("input.txt", "input");
    fixture.declare("input.txt.artf", runtime("file", &["-c", "exit 0"]));
    let run = fixture.json(&["verify", "--all"], 0);
    let key = run["requests"][0]["key"].as_str().unwrap();
    let execution = cache::show(&fixture.state, key, false)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(execution.artifact_kinds["file"], ArtifactKind::File);
    for full in [false, true] {
        let mut record = artifactize::remote::Record::new(&execution, full).unwrap();
        let wire = serde_json::to_value(&record).unwrap();
        assert_eq!(wire["artifactKinds"]["file"], "file");
        record
            .artifact_kinds
            .insert("file".into(), ArtifactKind::Folder);
        assert!(record.validate().is_err());
        record.artifact_kinds.clear();
        assert!(
            record.validate().is_err(),
            "legacy key inputs must never match a kind-aware key"
        );
        let mut record = artifactize::remote::Record::new(&execution, full).unwrap();
        record.publisher = Some("fixture".into());
        record.published_at = Some("2026-10-09T00:00:00Z".parse().unwrap());
        let mirrored = record.mirror("fixture").unwrap();
        assert_eq!(mirrored.artifact_kinds, execution.artifact_kinds);
        assert_eq!(mirrored.key, execution.key);
    }
}

#[test]
fn a_derived_file_eval_never_turns_green_after_its_target_was_deleted() {
    let fixture = Fixture::new();
    fixture.write("ready.txt", "ready");
    fixture.declare("ready.txt.artf", json!({
        "name":"ready",
        "fingerprint":false,
        "evals":[
            {"id":"ready","title":"Ready","profile":{"kind":"dependency","depends_on":["trigger"]}},
        ],
    }));
    fixture.declare(
        "trigger/index.artf",
        runtime("trigger", &["-c", "rm ../ready.txt"]),
    );
    let run = fixture.json(&["verify", "--all"], 2);
    let request = run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|request| request["evalId"] == "ready/ready")
        .unwrap();
    assert_eq!(request["status"], "ERROR");
    assert_eq!(request["errorCode"], "PREPARATION_FAILED");
    assert_eq!(run["status"], "ERROR");
}

#[test]
fn removed_file_target_cannot_reuse_green_evidence_after_preparation() {
    let fixture = Fixture::new();
    fixture.write("files/input.txt", "input");
    let mut declaration = runtime("file", &["-c", "exit 0"]);
    declaration["mounts"] = json!({"dep":"dep"});
    fixture.declare("files/input.txt.artf", declaration);
    fixture.declare(
        "dep/index.artf",
        json!({
            "name":"dep",
            "basis":true,
            "fingerprint":{
                "script":{"command":support::os::bin("/bin/sh"),"args":["-c","printf constant"]},
            },
        }),
    );
    let first = fixture.json(&["verify", "--all"], 0);
    fixture.declare(
        "dep/index.artf",
        json!({
            "name":"dep",
            "basis":true,
            "fingerprint":{
                "script":{
                    "command":support::os::bin("/bin/sh"),
                    "args":["-c","rm ../files/input.txt; printf constant"],
                },
            },
        }),
    );
    let run = fixture.json(&["verify", "--all"], 2);
    assert_eq!(first["requests"][0]["key"], run["requests"][0]["key"]);
    assert_eq!(run["requests"][0]["errorCode"], "PREPARATION_FAILED");
    assert!(run["requests"][0]["executionId"].is_null());
}
