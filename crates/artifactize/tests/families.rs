use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};

use artifactize::{
    config::{RepoConfig, StaleKey, read_workspace_config},
    graph::Graph,
    scope::{RelationKind, artifact_scope, eval_scope},
};
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        copy_directory(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/families"),
            &repo,
        );
        fs::set_permissions(
            repo.join("scenarios/stale_key.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        Self { root, repo }
    }

    fn write(&self, path: &str, value: Value) {
        let path = self.repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }

    fn read(&self, path: &str) -> Value {
        serde_json::from_slice(&fs::read(self.repo.join(path)).unwrap()).unwrap()
    }

    fn config(&self) -> RepoConfig {
        read_workspace_config(&self.repo).unwrap()
    }

    fn error(&self) -> String {
        read_workspace_config(&self.repo).unwrap_err().to_string()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(self.root.path().join("state"))
            .env("ARTIFACTIZE_STATE_HOME", self.root.path().join("home"));
        command
    }
}

fn copy_directory(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
        }
    }
}

fn output_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn file_instances_expand_to_ordinary_artifacts_evals_scopes_and_relations() {
    let fixture = Fixture::new();
    let config = fixture.config();
    assert_eq!(config.artifacts.len(), 3);
    assert!(!config.artifacts.contains_key("scenarios"));
    assert_eq!(config.families["scenarios"], Path::new("scenarios"));
    let parent = &config.artifacts["project"];
    assert_eq!(parent.children["scenarios/checkout"], "checkout");
    assert_eq!(parent.children["scenarios/search"], "search");
    assert_eq!(
        config
            .evals
            .iter()
            .map(|eval| eval.id.as_str())
            .collect::<Vec<_>>(),
        ["checkout/review", "search/review"]
    );
    assert!(
        config
            .relations
            .iter()
            .any(|relation| relation.source == "checkout"
                && relation.target == "project"
                && relation.kind
                    == RelationKind::Child {
                        path: "scenarios/checkout".into()
                    })
    );
    Graph::new(&config).unwrap();
    for id in ["checkout", "search"] {
        let artifact = &config.artifacts[id];
        let family = artifact.family.as_ref().unwrap();
        assert_eq!(artifact.path, Path::new("scenarios"));
        assert_eq!(family.name, "scenarios");
        assert_eq!(family.instances.as_deref(), Some("instances.json"));
        assert_eq!(family.material, [format!("{id}.txt")]);
        assert!(artifact.children.is_empty());
        let Some(StaleKey::Script {
            command,
            args,
            inputs,
            ..
        }) = &artifact.stale_key
        else {
            panic!()
        };
        assert_eq!(command, "/bin/sh");
        assert_eq!(args, &["stale_key.sh"]);
        assert_eq!(inputs, &["check.sh"]);
    }
    assert_eq!(
        config.artifacts["checkout"].views.agent_tools["detail"].input_schema()["properties"]["id"]
            ["enum"],
        json!(["summary", "navigation"])
    );
    assert_eq!(
        config.artifacts["search"].views.agent_tools["detail"].input_schema()["properties"]["id"]["enum"],
        json!(["message", "query"])
    );
    let scope = artifact_scope(&config, &["project"]).unwrap();
    let resolved = scope
        .resolve_path("project", "scenarios/checkout/checkout.txt")
        .unwrap();
    assert_eq!(resolved.artifact_id, "checkout");
    assert_eq!(resolved.path, "checkout.txt");
    assert_eq!(
        scope
            .resolve_input(&fixture.repo, "project", "scenarios/checkout/checkout.txt")
            .unwrap(),
        fixture.repo.join("scenarios/checkout.txt")
    );
    assert!(
        scope
            .resolve_path("project", "scenarios")
            .unwrap_err()
            .0
            .contains("address one of its instances")
    );
    assert!(
        scope
            .resolve_path("project", "scenarios/checkout.txt")
            .is_err()
    );
    let scope = eval_scope(&config, &config.evals[0]).unwrap();
    assert_eq!(
        scope.artifacts.keys().copied().collect::<Vec<_>>(),
        ["checkout"]
    );
    assert!(scope.resolve_path("search", "search.txt").is_err());
    // Material marks stale_key ownership, not a filesystem sandbox inside the shared folder.
    assert!(
        scope
            .resolve_input(&fixture.repo, "checkout", "search.txt")
            .is_ok()
    );
    assert!(!fixture.repo.join("scenarios/stale_key-ran").exists());
    assert!(!fixture.root.path().join("state").exists());

    let mut list = fixture.read("scenarios/instances.json");
    list["checkout"]["params"]["instruction"] = json!("Inspect {checkout} using {search}.");
    fixture.write("scenarios/instances.json", list);
    let config = fixture.config();
    assert_eq!(config.evals[0].deps, ["search"]);
    assert!(
        config
            .relations
            .iter()
            .any(|relation| relation.source == "search"
                && relation.target == "checkout"
                && matches!(relation.kind, RelationKind::Instruction { .. }))
    );
}

#[test]
fn config_check_is_inert_and_verify_keeps_runtime_verdicts_separate() {
    let fixture = Fixture::new();
    let stale_key = fixture.repo.join("scenarios/stale_key.sh");
    fs::write(&stale_key, "#!/bin/sh\ntouch stale_key-ran\nexit 1\n").unwrap();
    let output = fixture
        .command()
        .args(["config", "check", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        output_json(&output),
        json!({"ok":true,"artifacts":3,"evals":2})
    );
    assert!(!fixture.root.path().join("state").exists());
    assert!(!fixture.repo.join("scenarios/stale_key-ran").exists());
    fs::write(
        stale_key,
        include_str!("fixtures/families/scenarios/stale_key.sh"),
    )
    .unwrap();
    let output = fixture
        .command()
        .args(["verify", "--all", "--json"])
        .output()
        .unwrap();
    let run = output_json(&output);
    assert!(output.status.success(), "{run}");
    assert_eq!(run["status"], "GREEN");
    let requests = run["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 2);
    let checkout = requests
        .iter()
        .find(|request| request["evalId"] == "checkout/review")
        .unwrap();
    let search = requests
        .iter()
        .find(|request| request["evalId"] == "search/review")
        .unwrap();
    assert_eq!(checkout["staleKey"], "checkout:READY");
    assert_eq!(search["staleKey"], "search:SEARCH");
    assert_eq!(checkout["result"]["stdout"], "READY\n");
    assert_eq!(search["result"]["stdout"], "SEARCH\n");
    assert_ne!(checkout["runDir"], search["runDir"]);
    fs::write(fixture.repo.join("scenarios/search.txt"), "BROKEN\n").unwrap();
    let output = fixture
        .command()
        .args(["verify", "--all", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let run = output_json(&output);
    let requests = run["requests"].as_array().unwrap();
    assert_eq!(
        requests
            .iter()
            .find(|request| request["evalId"] == "checkout/review")
            .unwrap()["result"]["verdict"],
        "GREEN"
    );
    assert_eq!(
        requests
            .iter()
            .find(|request| request["evalId"] == "search/review")
            .unwrap()["result"]["verdict"],
        "RED"
    );
    assert!(!fixture.repo.join("scenarios/stale_key-ran").exists());
}

#[test]
fn family_names_are_reserved_and_templates_cannot_be_targets_or_nested() {
    let fixture = Fixture::new();
    let template = fixture.read("scenarios/artifactize.json");
    fixture.write("artifactize.json", template);
    assert!(fixture.error().contains("cannot be the workspace root"));
    fixture.write("artifactize.json", json!({"name":"project","basis":true}));
    fixture.write(
        "scenarios/nested/artifactize.json",
        json!({"name":"nested"}),
    );
    assert!(fixture.error().contains("cannot contain nested"));
    fs::remove_dir_all(fixture.repo.join("scenarios/nested")).unwrap();
    let mut template = fixture.read("scenarios/artifactize.json");
    template["reviewPolicy"] = json!({});
    fixture.write("scenarios/artifactize.json", template.clone());
    assert!(fixture.error().contains("cannot declare reviewPolicy"));
    template.as_object_mut().unwrap().remove("reviewPolicy");
    fixture.write("scenarios/artifactize.json", template);

    fixture.write("another/artifactize.json", json!({"name":"scenarios"}));
    assert!(
        fixture
            .error()
            .contains("Duplicate Artifact name: scenarios")
    );
    fs::remove_dir_all(fixture.repo.join("another")).unwrap();
    fixture.write("z/artifactize.json", json!({"name":"scenarios"}));
    assert!(
        fixture
            .error()
            .contains("Duplicate Artifact name: scenarios")
    );
    fixture.write("z/artifactize.json", json!({"name":"checkout"}));
    assert!(
        fixture
            .error()
            .contains("Duplicate Artifact name: checkout")
    );
    fs::remove_dir_all(fixture.repo.join("z")).unwrap();
    let list = fixture.read("scenarios/instances.json");
    fixture.write("scenarios/instances.json", json!({"scenarios":{}}));
    assert!(fixture.error().contains("reuse its family name"));
    fixture.write("scenarios/instances.json", json!({"bad/name":{}}));
    assert!(fixture.error().contains("Instance name"));
    fixture.write("scenarios/instances.json", json!({"physical":{}}));
    fs::write(fixture.repo.join("scenarios/physical"), "material").unwrap();
    assert!(fixture.error().contains("conflicts with a physical entry"));
    fixture.write("scenarios/instances.json", list);

    fixture.write(
        "artifactize.json",
        json!({"name":"project","mounts":{"source":"scenarios"}}),
    );
    assert!(fixture.error().contains("mount one of its instances"));
    fixture.write(
        "artifactize.json",
        json!({"name":"project","mounts":{"scenarios":"checkout"}}),
    );
    assert!(fixture.error().contains("Ambiguous mount alias scenarios"));
    fixture.write("artifactize.json", json!({"name":"project","evals":[{"id":"review","title":"Review","profile":{"kind":"runtime","command":"true","args":[]},"payload":{"instruction":"Inspect {scenarios}."}}]}));
    assert!(fixture.error().contains("reference one of its instances"));
}

#[test]
fn instance_lists_and_material_cannot_escape_or_follow_links() {
    let fixture = Fixture::new();
    let original = fixture.read("scenarios/artifactize.json");
    let mut template = original.clone();
    template["family"]["instances"] = json!("artifactize.json");
    fixture.write("scenarios/artifactize.json", template.clone());
    assert!(fixture.error().contains("other than artifactize.json"));
    template["family"]["instances"] = json!("../list.json");
    fixture.write("scenarios/artifactize.json", template.clone());
    assert!(fixture.error().contains("safe project-relative path"));
    fs::create_dir(fixture.repo.join("scenarios/directory")).unwrap();
    template["family"]["instances"] = json!("directory");
    fixture.write("scenarios/artifactize.json", template.clone());
    assert!(fixture.error().contains("must be a regular file"));
    symlink("instances.json", fixture.repo.join("scenarios/linked.json")).unwrap();
    template["family"]["instances"] = json!("linked.json");
    fixture.write("scenarios/artifactize.json", template);
    assert!(fixture.error().contains("symlinks"));
    fixture.write("scenarios/artifactize.json", original);
    let list = fixture.read("scenarios/instances.json");
    fs::write(fixture.repo.join("scenarios/instances.json"), "not JSON").unwrap();
    assert!(fixture.error().contains("Instance list instances.json"));
    let mut invalid = list;
    invalid["checkout"]["material"] = json!(["artifactize.json"]);
    fixture.write("scenarios/instances.json", invalid.clone());
    assert!(
        fixture
            .error()
            .contains("cannot claim the family declaration")
    );
    invalid["checkout"]["material"] = json!(["instances.json"]);
    fixture.write("scenarios/instances.json", invalid.clone());
    assert!(
        fixture
            .error()
            .contains("cannot claim the family declaration")
    );
    invalid["checkout"]["material"] = json!(["../artifactize.json"]);
    fixture.write("scenarios/instances.json", invalid.clone());
    assert!(fixture.error().contains("safe project-relative path"));
    invalid["checkout"]["material"] = json!(["missing"]);
    fixture.write("scenarios/instances.json", invalid.clone());
    assert!(
        fixture
            .error()
            .contains("must exist inside its family folder")
    );
    invalid["checkout"]["material"] = json!(["linked.json"]);
    fixture.write("scenarios/instances.json", invalid.clone());
    assert!(fixture.error().contains("symlinks"));
    symlink("directory", fixture.repo.join("scenarios/linked-directory")).unwrap();
    fs::write(fixture.repo.join("scenarios/directory/file"), "material").unwrap();
    invalid["checkout"]["material"] = json!(["linked-directory/file"]);
    fixture.write("scenarios/instances.json", invalid.clone());
    assert!(fixture.error().contains("symlinks"));
    symlink(
        "../search.txt",
        fixture.repo.join("scenarios/directory/link"),
    )
    .unwrap();
    invalid["checkout"]["material"] = json!(["directory"]);
    fixture.write("scenarios/instances.json", invalid);
    assert_eq!(
        fixture.config().artifacts["checkout"]
            .family
            .as_ref()
            .unwrap()
            .material,
        ["directory"]
    );
}

#[test]
fn family_selectors_expand_and_deduplicate_across_positional_csv_and_files() {
    use artifactize::project::selection::{Selection, read_selection_file};

    let fixture = Fixture::new();
    let config = fixture.config();
    let family = Selection::Artifact {
        artifact_id: "scenarios".into(),
    };
    let selected = family.resolve(&config).unwrap();
    assert_eq!(selected.roots, ["checkout", "search"]);
    assert_eq!(
        selected
            .evals
            .iter()
            .map(|eval| eval.id.as_str())
            .collect::<Vec<_>>(),
        ["checkout/review", "search/review"]
    );
    let json_path = fixture.root.path().join("selection.json");
    let line_path = fixture.root.path().join("selection.txt");
    fs::write(&json_path, r#"["search","scenarios","checkout"]"#).unwrap();
    fs::write(&line_path, "search\nscenarios\ncheckout\n").unwrap();
    let select = |path: &Path| Selection::Artifacts {
        artifact_ids: read_selection_file(path).unwrap(),
    };
    assert_eq!(select(&json_path), select(&line_path));
    let selected = select(&json_path).resolve(&config).unwrap();
    assert_eq!(selected.roots, ["search", "checkout"]);
    assert_eq!(
        selected
            .evals
            .iter()
            .map(|eval| eval.id.as_str())
            .collect::<Vec<_>>(),
        ["search/review", "checkout/review"]
    );

    let output = fixture
        .command()
        .args(["verify", "scenarios", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let run = output_json(&output);
    assert_eq!(
        run["selection"],
        json!({"kind":"artifact", "artifactId":"scenarios"})
    );
    assert_eq!(run["requests"].as_array().unwrap().len(), 2);
    let csv = fixture
        .command()
        .args([
            "verify",
            "--artifacts",
            "search,scenarios,checkout",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(csv.status.success());
    let csv = output_json(&csv);
    for path in [&json_path, &line_path] {
        let output = fixture
            .command()
            .args(["verify", "--artifacts-file"])
            .arg(path)
            .arg("--json")
            .output()
            .unwrap();
        assert!(output.status.success());
        let run = output_json(&output);
        assert_eq!(run["selection"], csv["selection"]);
        assert_eq!(run["requests"].as_array().unwrap().len(), 2);
        assert!(
            run["requests"]
                .as_array()
                .unwrap()
                .iter()
                .all(|request| request["result"]["verdict"] == "GREEN")
        );
    }
    let output = fixture
        .command()
        .args(["verify", "--eval", "scenarios/review", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output_json(&output)["error"]
            .as_str()
            .unwrap()
            .contains("Unknown Eval")
    );
}

#[test]
fn family_profile_variants_are_substituted_per_instance_before_selection() {
    use artifactize::project::selection::{ProfileSelection, Selection, select_profiles};

    let fixture = Fixture::new();
    let mut template = fixture.read("scenarios/artifactize.json");
    template["evals"][0]["profileVariants"] = json!({"echo":{
        "kind":"runtime", "command":"/bin/echo", "args":[{"$param":"/expected"}]
    }});
    fixture.write("scenarios/artifactize.json", template.clone());
    let selection = Selection::Artifact {
        artifact_id: "scenarios".into(),
    };
    let config = select_profiles(
        fixture.config(),
        &selection,
        Some(&ProfileSelection::Named("echo".into())),
        false,
    )
    .unwrap();
    let selected = selection.resolve(&config).unwrap();
    assert_eq!(
        serde_json::to_value(&selected.evals[0].declaration.profile).unwrap()["args"],
        json!(["READY"])
    );
    assert_eq!(
        serde_json::to_value(&selected.evals[1].declaration.profile).unwrap()["args"],
        json!(["SEARCH"])
    );
    let output = fixture
        .command()
        .args(["verify", "scenarios", "--profile", "echo", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let run = output_json(&output);
    let requests = run["requests"].as_array().unwrap();
    assert_eq!(
        requests
            .iter()
            .find(|request| request["evalId"] == "checkout/review")
            .unwrap()["result"]["stdout"],
        "READY\n"
    );
    assert_eq!(
        requests
            .iter()
            .find(|request| request["evalId"] == "search/review")
            .unwrap()["result"]["stdout"],
        "SEARCH\n"
    );
    assert_eq!(fixture.read("scenarios/artifactize.json"), template);
}
