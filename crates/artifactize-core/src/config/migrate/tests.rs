//! Migration at its seam: an old project on disk in, a project that reads out.

use std::fs;

use super::*;

fn project() -> tempfile::TempDir {
    let directory = crate::test_os::tempdir();
    fs::create_dir_all(directory.path().join("docs")).unwrap();
    fs::write(directory.path().join("docs/guide.md"), "# Guide\n").unwrap();
    directory
}

#[test]
fn definitions_of_0_25_with_aliases_and_settings_become_the_language() {
    let directory = project();
    let definitions = directory.path().join(".artifactize");
    fs::create_dir_all(&definitions).unwrap();
    fs::write(
        definitions.join("settings.toml"),
        "[profiles.careful]\nbackend = \"openai\"\nmodel = \"fixture\"\n",
    )
    .unwrap();
    fs::write(
        definitions.join("docs.artf"),
        r#"# The guide's folder.
path = "docs/"
type = "markdown"
annotations = { x = 1 }

[evals.clear]
title = "Clear"
profile = !careful(reasoning = "high")
payload = { instruction = """
# Not a comment
Is {docs} clear?""" }

[evals.signoff]
title = "Approved"
profile = { kind = "human" }
payload = { instruction = "Approve {docs}." }
pass_schema = { type = "object" }
"#,
    )
    .unwrap();
    let migration = migrate(directory.path()).unwrap();
    let text = fs::read_to_string(definitions.join("docs.artf")).unwrap();
    assert!(text.starts_with("// The guide's folder.\nartf docs : markdown = {"), "{text}");
    assert!(!text.contains("// Not a comment"), "{text}");
    let config = crate::config::read_workspace_config(directory.path()).unwrap();
    let docs = &config.artifacts[&"docs".parse::<crate::config::ArtifactName>().unwrap()];
    assert_eq!(docs.toolsets, ["markdown"]);
    let clear = config.evals.iter().find(|eval| eval.id.as_str() == "docs/clear").unwrap();
    assert_eq!(clear.declaration.instruction(), Some("# Not a comment\nIs docs clear?"));
    let crate::config::Profile::Agent { model, .. } = clear.declaration.profile() else {
        panic!("an Agent eval");
    };
    assert_eq!(model.as_str(), "fixture");
    assert!(config.profiles.contains_key("careful"));
    assert!(!definitions.join("settings.toml").exists());
    assert!(migration.removed.contains(&PathBuf::from(".artifactize/settings.toml")));
    assert!(migration.warnings.iter().any(|warning| warning.contains("pass_schema")));
    // Once is enough: the project is already in the language.
    let again = migrate(directory.path()).unwrap();
    assert!(again.written.is_empty() && again.removed.is_empty());
}

#[test]
fn in_tree_markers_become_definitions_with_explicit_parts() {
    let directory = project();
    fs::write(
        directory.path().join("index.artf"),
        "name = \"app\"\n[evals.check]\ntitle = \"Check\"\nprofile = { kind = \"runtime\", command = \"true\", args = [\"{guide}\"] }\npayload = { instruction = \"Run.\" }\n",
    )
    .unwrap();
    fs::write(directory.path().join("docs/guide.md.artf"), "name = \"guide\"\n").unwrap();
    let migration = migrate(directory.path()).unwrap();
    assert!(!directory.path().join("index.artf").exists());
    assert_eq!(migration.removed.len(), 2);
    let config = crate::config::read_workspace_config(directory.path()).unwrap();
    let app = &config.artifacts[&"app".parse::<crate::config::ArtifactName>().unwrap()];
    assert!(app.parts.contains("guide"));
    assert_eq!(config.evals[0].deps, ["guide".parse::<crate::config::ArtifactName>().unwrap()]);
}

#[test]
fn a_project_that_would_not_read_is_left_untouched() {
    let directory = project();
    let definitions = directory.path().join(".artifactize");
    fs::create_dir_all(&definitions).unwrap();
    let old = "path = \"docs/\"\n[evals.check]\ntitle = \"Check\"\nprofile = !missing\npayload = { instruction = \"x\" }\n";
    fs::write(definitions.join("docs.artf"), old).unwrap();
    let error = migrate(directory.path()).unwrap_err().to_string();
    assert!(error.contains("Unknown profile alias !missing"), "{error}");
    assert_eq!(fs::read_to_string(definitions.join("docs.artf")).unwrap(), old);
    let old = "path = \"docs/\"\nparts = [\"nowhere\"]\n";
    fs::write(definitions.join("docs.artf"), old).unwrap();
    assert!(migrate(directory.path()).is_err());
    assert_eq!(fs::read_to_string(definitions.join("docs.artf")).unwrap(), old);
}

#[test]
fn old_plugins_name_their_namespace_and_declare_a_toolset() {
    let directory = project();
    let plugin = directory.path().join(".artifactize/plugins/html");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(plugin.join("index.html"), "<p>viewer</p>").unwrap();
    fs::write(
        plugin.join("plugin.toml"),
        "type = \"html\"\ntitle = \"HTML\"\n[viewer]\nentry = \"index.html\"\n[views.human_tools.view]\nkind = \"viewer\"\ndescription = \"Open {artifactName}.\"\n[views.agent_tools.read]\nbuiltin = \"read\"\n",
    )
    .unwrap();
    fs::write(
        directory.path().join(".artifactize/docs.artf"),
        "path = \"docs/\"\ntype = \"html\"\n",
    )
    .unwrap();
    migrate(directory.path()).unwrap();
    let manifest = fs::read_to_string(plugin.join("plugin.toml")).unwrap();
    assert!(manifest.contains("namespace = \"html\""), "{manifest}");
    let tools = fs::read_to_string(plugin.join("html.artf")).unwrap();
    assert!(tools.contains("human view = viewer(description = \"Open @{artf self}.\")"), "{tools}");
    let config = crate::config::read_workspace_config(directory.path()).unwrap();
    let docs = &config.artifacts[&"docs".parse::<crate::config::ArtifactName>().unwrap()];
    assert_eq!(docs.toolsets, ["html"]);
    assert_eq!(docs.views.human_tools.len(), 1);
}
