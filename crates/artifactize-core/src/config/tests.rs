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
        read_workspace_config(self.0.path()).unwrap_err().to_string()
    }
}

fn name(text: &str) -> ArtifactName {
    text.parse().unwrap()
}

const SHARED: &str = r#"
profile reviewer = agent(backend = "openai", model = "fixture")

tool cargo = {
  agent test = command(
    protocol = "json",
    command = "cargo",
    args = ["test", "--manifest-path=@{path self/Cargo.toml}"],
    description = "Test @{artf self}.",
    input_schema = { type = "object" },
  )
}

eval! follows(rule = "rules") : reviewer = {
  title = "Follows the rules"
  instruction = "Check @{artf self} against @{artf rules}."
  red_at = "P1"
}

artf! crate(dir, extra_evals = {}) : files & cargo = {
  path = "crates/@{param dir}/"
  eval style = follows!()
  ...extra_evals
}

artf rules : files = { path = "rules/", basis = true }
"#;

#[test]
fn macros_expand_with_defaults_spreads_and_a_late_bound_self() {
    let project = Project::new(&[
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
    assert_eq!((binding.line, binding.macro_name.as_deref()), (1, Some("crate!")));
    let eval = |id: &str| config.evals.iter().find(|eval| eval.id.as_str() == id).unwrap();
    assert_eq!(
        eval("core/style").declaration.instruction(),
        Some("Check core against rules.")
    );
    assert_eq!(eval("cli/docs").declaration.instruction(), Some("Read cli."));
    // `self` in a shared toolset is the Artifact composing it.
    let crate::config::AgentTool::Command(test) =
        &config.artifacts[&name("cli")].views.agent_tools[&"cargo.test".parse::<ToolFqn>().unwrap()]
    else {
        panic!("a command tool");
    };
    assert_eq!(test.args[1].to_string(), "--manifest-path=@{path cli/Cargo.toml}");
    assert_eq!(config.profiles.len(), 1);
    assert!(config.evals.iter().all(|eval| eval.id.as_str() != "core/docs"));
}

#[test]
fn errors_name_the_line_and_the_macro_call_that_made_it() {
    let project = Project::new(&[
        ("shared.artf", SHARED),
        ("crates.artf", "artf core = crate!(dir = \"core\", extra_evals = { eval x : human = { title = \"X\" } })\n"),
    ]);
    let error = project.error();
    assert!(error.contains("crates.artf"), "{error}");
    assert!(error.contains("Eval `x` needs an `instruction`"), "{error}");
    let project = Project::new(&[
        ("shared.artf", SHARED),
        ("crates.artf", "artf core = crate!(dir = \"core\", colour = \"red\")\n"),
    ]);
    let error = project.error();
    assert!(error.contains("line 1") && error.contains("colour"), "{error}");
    assert!(error.contains("note:") && error.contains("shared.artf"), "{error}");
}

#[test]
fn names_are_unique_ignoring_case_and_macros_do_not_recurse() {
    let project = Project::new(&[
        ("a.artf", "artf app = { path = \"docs/\" }\n"),
        ("b.artf", "artf App = { path = \"rules/\" }\n"),
    ]);
    assert!(project.error().contains("differ ignoring case"), "{}", project.error());
    let project = Project::new(&[
        ("a.artf", "artf app = { path = \"docs/\" }\n"),
        ("b.artf", "artf app = { path = \"rules/\" }\n"),
    ]);
    let error = project.error();
    assert!(error.contains("note:") && error.contains("a.artf"), "{error}");
    let project = Project::new(&[(
        "loop.artf",
        "eval! a() : human = { title = \"A\", instruction = \"A\" }\nartf! x() = { path = \"docs/\", eval one = x!() }\nartf y = x!()\n",
    )]);
    assert!(!project.error().is_empty());
    let project = Project::new(&[(
        "loop.artf",
        "profile! p() = p!()\nartf y = {\n  path = \"docs/\"\n  eval e : p!() = { title = \"E\", instruction = \"E\" }\n}\n",
    )]);
    let error = project.error();
    assert!(error.contains("calls itself"), "{error}");
}

#[test]
fn toml_definitions_ask_for_migration() {
    let project = Project::new(&[("app.artf", "path = \"docs/\"\n[evals.check]\ntitle = \"Check\"\n")]);
    assert!(project.error().contains("run artifactize config migrate"), "{}", project.error());
}

#[test]
fn identical_tools_merge_and_different_ones_with_one_name_conflict() {
    let service = r#"tool! svc(port) = {
  namespace = "svc"
  agent ping = command(protocol = "json", command = "curl", args = ["localhost:@{param port}"], description = "Ping.", input_schema = { type = "object" })
}
"#;
    let project = Project::new(&[(
        "app.artf",
        &format!("{service}artf app : svc!(port = \"1\") & svc!(port = \"1\") = {{ path = \"docs/\" }}\n"),
    )]);
    assert_eq!(project.read().artifacts[&name("app")].views.agent_tools.len(), 1);
    let project = Project::new(&[(
        "app.artf",
        &format!("{service}artf app : svc!(port = \"1\") & svc!(port = \"2\") = {{ path = \"docs/\" }}\n"),
    )]);
    let error = project.error();
    assert!(error.contains("both named `svc.ping`") && error.contains("note:"), "{error}");
}

#[test]
fn slots_render_for_their_audience_and_runtime_instructions_take_none() {
    let project = Project::new(&[(
        "app.artf",
        r#"profile reviewer = agent(backend = "openai", model = "fixture")
artf rules : files = { path = "rules/", basis = true }
artf app : files = {
  path = "docs/"
  eval agent : reviewer = {
    title = "Agent"
    instruction = "Use @{tool self agent files.read} on @{path rules/style.md}."
  }
  eval person : human = {
    title = "Person"
    instruction = "Open @{tool self human files.view}; compare with @{artf rules}."
  }
  eval check : runtime(command = "true", args = ["@{path rules}"]) = {
    title = "Check"
    inputs = [rules]
  }
}
"#,
    )]);
    let config = project.read();
    let eval = |id: &str| config.evals.iter().find(|eval| eval.id.as_str() == id).unwrap();
    assert_eq!(
        eval("app/agent").declaration.instruction(),
        Some("Use files_read_app on rules/style.md.")
    );
    assert_eq!(
        eval("app/person").declaration.instruction(),
        Some("Open files.view; compare with rules.")
    );
    assert_eq!(eval("app/check").declaration.inputs(), [name("rules")]);
    assert!(eval("app/check").deps.contains(&name("rules")));
    for (text, expected) in [
        ("@{tool self agent files.nothing}", "files.nothing"),
        ("@{artf missing}", "missing"),
    ] {
        project.write(
            "app.artf",
            &format!(
                "profile reviewer = agent(backend = \"openai\", model = \"fixture\")\nartf app : files = {{\n  path = \"docs/\"\n  eval agent : reviewer = {{ title = \"A\", instruction = \"{text}\" }}\n}}\n"
            ),
        );
        let error = project.error();
        assert!(error.contains(expected) && error.contains("line 4"), "{error}");
    }
    project.write(
        "app.artf",
        "artf app = {\n  path = \"docs/\"\n  eval check : runtime(command = \"true\", args = []) = { title = \"C\", instruction = \"About @{artf app}.\" }\n}\n",
    );
    assert!(project.error().contains("names no Artifact"), "{}", project.error());
}

#[test]
fn an_agent_eval_names_a_declared_profile() {
    let project = Project::new(&[(
        "app.artf",
        "artf app = {\n  path = \"docs/\"\n  eval agent : agent(backend = \"openai\", model = \"m\") = { title = \"A\", instruction = \"A\" }\n}\n",
    )]);
    assert!(project.error().contains("declare `profile NAME = agent(…)`"), "{}", project.error());
}

#[test]
fn meta_artifacts_under_definitions_default_to_a_semantic_fingerprint() {
    let project = Project::new(&[
        ("design/app.artf", "artf app = { path = \"docs/\" }\n"),
        ("graph.artf", "artf design : artifactize = { path = \".artifactize/design/\" }\n"),
    ]);
    let config = project.read();
    assert!(matches!(
        config.artifacts[&name("design")].fingerprint,
        Some(Fingerprint::Graph)
    ));
    // Comments and layout change no meaning; a changed field does.
    let fingerprint = || {
        crate::graph_structure::read_scoped(project.0.path(), &project.read().artifacts[&name("design")])
            .unwrap()
            .fingerprint()
    };
    let before = fingerprint();
    project.write("design/app.artf", "// The app's docs.\nartf app = {\n  path = \"docs/\"\n}\n");
    assert_eq!(fingerprint(), before);
    project.write("design/app.artf", "artf app = { path = \"rules/\" }\n");
    assert_ne!(fingerprint(), before);
}

#[test]
fn expand_shows_the_binding_as_written_and_the_declaration_it_makes() {
    let project = Project::new(&[
        ("shared.artf", SHARED),
        ("crates.artf", "// The core crate.\nartf core = crate!(dir = \"core\")\n"),
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
    assert!(core.expanded.contains("eval style : reviewer = {"), "{}", core.expanded);
    // An argument's path slot reads as the slot it is, not as escaped text.
    assert!(
        core.expanded.contains("\"--manifest-path=@{path core/Cargo.toml}\""),
        "{}",
        core.expanded
    );
    assert_eq!(core.evals[0].from.as_deref(), Some("follows!"));
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
}
