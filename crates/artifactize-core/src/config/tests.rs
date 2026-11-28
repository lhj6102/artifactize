//! The `.artf` language at its seam: definition files in, the project or a located error out.

use std::fs;

use super::*;

struct Project(tempfile::TempDir);

impl Project {
    fn new(files: &[(&str, &str)]) -> Self {
        let directory = crate::test_os::tempdir();
        for folder in ["crates/core", "crates/cli", "rules", "docs"] {
            fs::create_dir_all(directory.path().join(folder)).unwrap();
        }
        fs::write(directory.path().join("README.md"), "# Readme\n").unwrap();
        let project = Self(directory);
        for (file, text) in files {
            project.write(file, text);
        }
        project
    }

    fn write(&self, file: &str, text: &str) {
        let path = self.0.path().join(".artifactize").join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self) -> RepoConfig {
        read_workspace_config(self.0.path()).unwrap_or_else(|error| panic!("{error}"))
    }

    fn error(&self) -> String {
        read_workspace_config(self.0.path())
            .unwrap_err()
            .to_string()
    }
}

fn name(text: &str) -> ArtifactName {
    text.parse().unwrap()
}

const PROFILES: &str = r#"profile reviewer : agent = { backend = "openai", model = "fixture" }
"#;

const SHARED: &str = r#"
tool cargo = {
  agent test = command(
    protocol = "json",
    command = "cargo",
    args = ["test", "--manifest-path=@{path self/Cargo.toml}"],
    description = "Test @{artf self}.",
    input_schema = { type = "object" },
  )
}

artf! crate(dir, extra_evals = {}) : files & cargo = {
  path = "crates/@{param dir}/"
  eval style : reviewer = {
    title = "Follows the rules"
    instruction = "Check @{artf self} against @{artf rules}."
    red_at = "P1"
  }
  ...extra_evals
}

artf rules : files = { path = "rules/", basis = true }
"#;

#[test]
fn macros_expand_with_defaults_spreads_and_a_late_bound_self() {
    let project = Project::new(&[
        ("profiles.artfprofiles", PROFILES),
        ("shared.artf", SHARED),
        (
            "crates.artf",
            r#"artf core = crate!(dir = "core")
artf cli = crate!(
  dir = "cli",
  extra_evals = {
    eval docs : human = { title = "Docs read well", instruction = "Read @{artf self}." }
  },
)
"#,
        ),
    ]);
    let config = project.read();
    let core = &config.artifacts[&name("core")];
    assert_eq!(core.path.local(), Some(std::path::Path::new("crates/core")));
    assert_eq!(core.toolsets, ["files", "cargo"]);
    let binding = core.binding.as_ref().unwrap();
    assert_eq!(
        (binding.line, binding.macro_name.as_deref()),
        (1, Some("crate!"))
    );
    let eval = |id: &str| {
        config
            .evals
            .iter()
            .find(|eval| eval.id.as_str() == id)
            .unwrap()
    };
    assert_eq!(
        eval("core/style").declaration.instruction(),
        Some("Check core against rules.")
    );
    assert_eq!(
        eval("cli/docs").declaration.instruction(),
        Some("Read cli.")
    );
    // `self` in a shared toolset is the Artifact composing it.
    let crate::config::AgentTool::Command(test) = &config.artifacts[&name("cli")].views.agent_tools
        [&"cargo.test".parse::<ToolFqn>().unwrap()]
    else {
        panic!("a command tool");
    };
    assert_eq!(
        test.args[1].to_string(),
        "--manifest-path=@{path cli/Cargo.toml}"
    );
    assert_eq!(config.profiles.len(), 1);
    assert!(
        config
            .evals
            .iter()
            .all(|eval| eval.id.as_str() != "core/docs")
    );
}

#[test]
fn errors_name_the_line_and_the_macro_call_that_made_it() {
    let project = Project::new(&[
        ("profiles.artfprofiles", PROFILES),
        ("shared.artf", SHARED),
        (
            "crates.artf",
            "artf core = crate!(dir = \"core\", extra_evals = { eval x : human = { title = \"X\" } })\n",
        ),
    ]);
    let error = project.error();
    assert!(error.contains("crates.artf"), "{error}");
    assert!(error.contains("Eval `x` needs an `instruction`"), "{error}");
    let project = Project::new(&[
        ("profiles.artfprofiles", PROFILES),
        ("shared.artf", SHARED),
        (
            "crates.artf",
            "artf core = crate!(dir = \"core\", colour = \"red\")\n",
        ),
    ]);
    let error = project.error();
    assert!(
        error.contains("line 1") && error.contains("colour"),
        "{error}"
    );
    assert!(
        error.contains("note:") && error.contains("shared.artf"),
        "{error}"
    );
}

#[test]
fn a_name_is_declared_once_across_kinds_and_artifacts_differ_ignoring_case() {
    let project = Project::new(&[
        ("a.artf", "artf app = { path = \"docs/\" }\n"),
        ("b.artf", "artf App = { path = \"rules/\" }\n"),
    ]);
    assert!(
        project.error().contains("differ ignoring case"),
        "{}",
        project.error()
    );
    let project = Project::new(&[
        ("a.artf", "artf app = { path = \"docs/\" }\n"),
        ("b.artf", "artf app = { path = \"rules/\" }\n"),
    ]);
    let error = project.error();
    assert!(
        error.contains("note:") && error.contains("a.artf"),
        "{error}"
    );
    // What follows `:` reads unambiguously: a toolset, an Artifact and a profile never share
    // a name.
    for (file, text) in [
        (
            "b.artf",
            "tool app = { agent read = builtin(op = \"read\") }\n",
        ),
        (
            "b.artfprofiles",
            "profile app : agent = { backend = \"openai\", model = \"m\" }\n",
        ),
        ("b.artf", "artf! app() = { path = \"rules/\" }\n"),
    ] {
        let project = Project::new(&[
            ("a.artf", "artf app = { path = \"docs/\" }\n"),
            (file, text),
        ]);
        let error = project.error();
        assert!(error.contains("already declared as"), "{error}");
    }
}

#[test]
fn toml_definitions_ask_for_migration() {
    let project = Project::new(&[(
        "app.artf",
        "path = \"docs/\"\n[evals.check]\ntitle = \"Check\"\n",
    )]);
    assert!(
        project.error().contains("run artifactize config migrate"),
        "{}",
        project.error()
    );
}

#[test]
fn identical_tools_merge_and_different_ones_with_one_name_conflict() {
    let ping = |port: &str| {
        format!(
            "agent ping = command(protocol = \"json\", command = \"curl\", args = [\"localhost:{port}\"], description = \"Ping.\", input_schema = {{ type = \"object\" }})"
        )
    };
    let project = Project::new(&[(
        "app.artf",
        &format!(
            "tool svc = {{\n  {}\n}}\nartf app : svc & svc = {{ path = \"docs/\" }}\n",
            ping("1")
        ),
    )]);
    assert_eq!(
        project.read().artifacts[&name("app")]
            .views
            .agent_tools
            .len(),
        1
    );
    let project = Project::new(&[(
        "app.artf",
        &format!(
            "tool svc = {{\n  {}\n  {}\n}}\nartf app : svc = {{ path = \"docs/\" }}\n",
            ping("1"),
            ping("2")
        ),
    )]);
    let error = project.error();
    assert!(
        error.contains("both named `svc.ping`") && error.contains("note:"),
        "{error}"
    );
}

#[test]
fn instructions_name_artifacts_and_runtime_evals_list_their_deps() {
    let project = Project::new(&[
        ("profiles.artfprofiles", PROFILES),
        (
            "app.artf",
            r#"artf rules : files = { path = "rules/", basis = true }
artf app : files = {
  path = "docs/"
  eval agent : reviewer = {
    title = "Agent"
    instruction = "Read @{artf self} against @{artf rules}."
  }
  // The command reads the rules it is given.
  eval check : runtime(command = "true", args = ["@{path rules}"]) = {
    title = "Check"
    deps = [rules]
  }
}
"#,
        ),
    ]);
    let config = project.read();
    let eval = |id: &str| {
        config
            .evals
            .iter()
            .find(|eval| eval.id.as_str() == id)
            .unwrap()
    };
    assert_eq!(
        eval("app/agent").declaration.instruction(),
        Some("Read app against rules.")
    );
    assert_eq!(eval("app/check").declaration.inputs(), [name("rules")]);
    assert!(eval("app/check").deps.contains(&name("rules")));
    assert_eq!(eval("app/check").declaration.instruction(), None);
    for (eval, says) in [
        (
            "eval agent : reviewer = { title = \"A\", instruction = \"Use @{tool self agent files.read}.\" }",
            "There is no tool slot",
        ),
        (
            "eval agent : reviewer = { title = \"A\", instruction = \"Read @{path self/a.md}.\" }",
            "a `@{path …}` belongs in a command's arguments",
        ),
        (
            "eval agent : reviewer = { title = \"A\", instruction = \"Read @{artf missing}.\" }",
            "missing",
        ),
        (
            "eval check : runtime(command = \"true\", args = []) = { title = \"C\", instruction = \"About it.\" }",
            "takes no `instruction`",
        ),
        (
            "eval agent : reviewer = { title = \"A\", instruction = \"A\", deps = [app] }",
            "`deps` is for runtime evals",
        ),
    ] {
        project.write(
            "app.artf",
            &format!("artf app : files = {{\n  path = \"docs/\"\n  {eval}\n}}\n"),
        );
        let error = project.error();
        assert!(error.contains(says) && error.contains("line 3"), "{error}");
    }
}

#[test]
fn a_macro_fills_paths_and_arguments_never_prose() {
    for prose in [
        "title = \"@{param what} reads well\", instruction = \"Read @{artf self}.\"",
        "title = \"Reads well\", instruction = what",
    ] {
        let project = Project::new(&[(
            "app.artf",
            &format!(
                "artf! doc(what) = {{\n  path = \"docs/\"\n  eval reads : human = {{ {prose} }}\n}}\nartf app = doc!(what = \"the guide\")\n"
            ),
        )]);
        let error = project.error();
        assert!(error.contains("written out where it stands"), "{error}");
    }
}

#[test]
fn an_agent_eval_names_a_profile_of_an_artfprofiles_file() {
    let project = Project::new(&[(
        "app.artf",
        "artf app = {\n  path = \"docs/\"\n  eval agent : agent(backend = \"openai\", model = \"m\") = { title = \"A\", instruction = \"A\" }\n}\n",
    )]);
    assert!(
        project.error().contains("profile NAME : agent"),
        "{}",
        project.error()
    );
    project.write(
        "app.artf",
        "artf app = {\n  path = \"docs/\"\n  eval agent : nobody = { title = \"A\", instruction = \"A\" }\n}\n",
    );
    assert!(
        project.error().contains("Unknown profile `nobody`"),
        "{}",
        project.error()
    );
}

#[test]
fn a_profile_runs_the_branch_its_environment_variable_names() {
    let project = Project::new(&[
        (
            "profiles.artfprofiles",
            r#"profile reviewer : agent = match TIER {
  default => { backend = "openai", model = "everyday" },
  heavy => { backend = "openai", model = "careful", reasoning = "high" },
}
profile translator : agent = { backend = "openai", model = "words" }
"#,
        ),
        (
            "app.artf",
            "artf app : files = {\n  path = \"docs/\"\n  eval agent : reviewer = { title = \"A\", instruction = \"A\" }\n  eval words : translator = { title = \"W\", instruction = \"W\" }\n}\n",
        ),
    ]);
    let read = |tier: Option<&str>| {
        discovery::read_workspace_declarations_with(project.0.path(), &|name| {
            (name == "TIER").then(|| tier.map(str::to_owned)).flatten()
        })
    };
    let model = |config: &RepoConfig, id: &str| {
        let eval = config
            .evals
            .iter()
            .find(|eval| eval.id.as_str() == id)
            .unwrap();
        serde_json::to_value(eval.declaration.profile()).unwrap()["model"].clone()
    };
    let unset = read(None).unwrap();
    assert_eq!(model(&unset, "app/agent"), "everyday");
    assert_eq!(model(&read(Some("")).unwrap(), "app/agent"), "everyday");
    let heavy = read(Some("heavy")).unwrap();
    assert_eq!(model(&heavy, "app/agent"), "careful");
    // A profile without a `match` runs the same in every environment.
    assert_eq!(model(&heavy, "app/words"), "words");
    // The graph a Run saves says which value chose the branches.
    let graph = serde_json::to_value(
        crate::query::graph(&heavy, &crate::project::selection::Selection::All).unwrap(),
    )
    .unwrap();
    assert_eq!(graph["environment"], serde_json::json!({ "TIER": "heavy" }));
    // A value no branch names is refused rather than run as `default`.
    let error = read(Some("haevy")).unwrap_err().to_string();
    assert!(
        error.contains("TIER=haevy") && error.contains("`heavy`"),
        "{error}"
    );
    // Every branch is checked, the ones the environment does not select included.
    project.write(
        "profiles.artfprofiles",
        "profile reviewer : agent = match TIER {\n  default => { backend = \"openai\", model = \"everyday\" },\n  heavy => { backend = \"openai\" },\n}\nprofile translator : agent = { backend = \"openai\", model = \"words\" }\n",
    );
    assert!(read(None).is_err());
    project.write(
        "profiles.artfprofiles",
        "profile reviewer : agent = match TIER {\n  heavy => { backend = \"openai\", model = \"careful\" },\n}\nprofile translator : agent = { backend = \"openai\", model = \"words\" }\n",
    );
    let error = read(Some("heavy")).unwrap_err().to_string();
    assert!(error.contains("without a `default` branch"), "{error}");
}

#[test]
fn meta_artifacts_under_definitions_default_to_a_semantic_fingerprint() {
    let project = Project::new(&[
        ("design/app.artf", "artf app = { path = \"docs/\" }\n"),
        (
            "graph.artf",
            "artf design : artifactize = { path = \".artifactize/design/\" }\n",
        ),
    ]);
    let config = project.read();
    assert!(matches!(
        config.artifacts[&name("design")].fingerprint,
        Some(Fingerprint::Graph)
    ));
    // Comments and layout change no meaning; a changed field does.
    let fingerprint = || {
        crate::graph_structure::read_scoped(
            project.0.path(),
            &project.read().artifacts[&name("design")],
        )
        .unwrap()
        .fingerprint()
    };
    let before = fingerprint();
    project.write(
        "design/app.artf",
        "// The app's docs.\nartf app = {\n  path = \"docs/\"\n}\n",
    );
    assert_eq!(fingerprint(), before);
    project.write("design/app.artf", "artf app = { path = \"rules/\" }\n");
    assert_ne!(fingerprint(), before);
}

#[test]
fn expand_shows_the_binding_as_written_and_the_declaration_it_makes() {
    let project = Project::new(&[
        ("profiles.artfprofiles", PROFILES),
        ("shared.artf", SHARED),
        (
            "crates.artf",
            "// The core crate.\nartf core = crate!(dir = \"core\")\n",
        ),
    ]);
    let [core] = expand::expand(project.0.path(), Some(&name("core")))
        .unwrap()
        .try_into()
        .unwrap();
    assert_eq!((core.line, core.macro_name.as_deref()), (2, Some("crate!")));
    assert_eq!(core.authored, "artf core = crate!(dir = \"core\")");
    assert!(
        core.expanded
            .starts_with("artf core : files & cargo = {\n  path = \"crates/core/\"\n"),
        "{}",
        core.expanded
    );
    assert!(
        core.expanded.contains("eval style : reviewer = {"),
        "{}",
        core.expanded
    );
    // The expansion is code: it reads back as the same Artifact beside the shared definitions.
    project.write("crates.artf", &core.expanded);
    let config = project.read();
    assert_eq!(
        config.artifacts[&name("core")].path.local(),
        Some(std::path::Path::new("crates/core"))
    );
    let symbols = expand::symbols(project.0.path()).unwrap();
    let crate_macro = symbols
        .iter()
        .find(|symbol| symbol.name == "crate" && symbol.is_macro)
        .unwrap();
    let params: Vec<_> = crate_macro
        .params
        .as_ref()
        .unwrap()
        .iter()
        .map(|param| (param.name.as_str(), param.default.as_deref()))
        .collect();
    assert_eq!(params, [("dir", None), ("extra_evals", Some("{}"))]);
    assert!(symbols.iter().any(|symbol| symbol.kind == "tool"
        && symbol.name == "files"
        && symbol.plugin.as_deref() == Some("files")));
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol.kind == "profile" && symbol.name == "reviewer")
    );
    // A runtime argument's path slot reads as the slot it is, not as escaped text.
    project.write(
        "crates.artf",
        "artf core = {\n  path = \"crates/core/\"\n  eval build : runtime(command = \"cargo\", args = [\"--manifest-path=@{path self/Cargo.toml}\"]) = { title = \"Builds\" }\n}\n",
    );
    let [core] = expand::expand(project.0.path(), Some(&name("core")))
        .unwrap()
        .try_into()
        .unwrap();
    assert!(
        core.expanded
            .contains("args = [\"--manifest-path=@{path core/Cargo.toml}\"]"),
        "{}",
        core.expanded
    );
}
