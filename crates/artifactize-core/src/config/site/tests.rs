use std::fs;

use super::*;
use crate::review::ArtifactDeclarations;

struct Workspace(
    PathBuf,
    #[expect(dead_code, reason = "owns the directory")] tempfile::TempDir,
);

impl Workspace {
    /// A graph Artifact, folder Artifact `app` composing folder Artifact `lib`, file Artifact
    /// `main` in app's folder, and `docs`, whose eval reads `{app}`.
    fn new() -> Self {
        let directory = crate::test_os::tempdir();
        let workspace = Self(directory.path().to_owned(), directory);
        workspace.write(".artf", "name = \"graph\"\n");
        workspace.write(
            "app/index.artf",
            r#"name = "app"

[views.human_tools]
notes = { description = "Print {artifactName}'s library notes.", builtin = "read", args = ["{lib}/README.md"] }
"#,
        );
        workspace.write(
            "app/lib/index.artf",
            r#"name = "lib"

[views.human_tools]
open = { description = "Open the library.", builtin = "open", args = ["{artifactPath}"] }
"#,
        );
        workspace.write("app/lib/README.md", "# Library\n");
        workspace.write("app/main.rs", "fn main() {}\n");
        workspace.write("app/main.rs.artf", "name = \"main\"\n");
        workspace.write(
            "docs/index.artf",
            r#"name = "docs"

[evals.read]
title = "The docs describe the app"
profile = { kind = "human" }
payload = { instruction = "Compare the docs with {app}." }
"#,
        );
        workspace
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn name(name: &str) -> ArtifactName {
        name.parse().unwrap()
    }

    fn read(&self, name: &str, site: &Site) -> Result<ArtifactDeclarations, ScopeReadError> {
        ArtifactDeclarations::read(&self.0, &Self::name(name), site)
    }
}

#[test]
fn sites_place_every_artifact_where_a_full_read_does() {
    let workspace = Workspace::new();
    let full = read_workspace_config(&workspace.0).unwrap();
    let placed: BTreeMap<_, _> = full
        .artifacts
        .iter()
        .map(|(name, artifact)| (name.clone(), artifact.site()))
        .collect();
    assert_eq!(placed.len(), 5);
    assert_eq!(read_sites(&workspace.0).unwrap(), placed);
}

#[test]
fn an_artifacts_declarations_are_its_own_and_those_it_composes() {
    let workspace = Workspace::new();
    let sites = read_sites(&workspace.0).unwrap();
    // Declarations it does not compose are not read: broken ones elsewhere change nothing,
    // though a full read refuses them.
    workspace.write("docs/index.artf", "name = \"docs\"\n[evals.read\n");
    workspace.write("app/main.rs.artf", "name = 7\n");
    assert!(read_workspace_config(&workspace.0).is_err());
    let app = workspace
        .read("app", &sites[&Workspace::name("app")])
        .unwrap();
    let tools = app.tools().unwrap();
    // Only its own tools; the child it reads from is in its scope.
    assert_eq!(tools.len(), 1);
    let (tool, command) = &tools[0];
    assert_eq!(tool.name.as_str(), "notes_app");
    assert_eq!(tool.description, "Print app's library notes.");
    assert_eq!(tool.action(command.as_ref()), "prints app/lib/README.md");
    assert!(command.is_none(), "a builtin runs no command line");
    // A file Artifact and the graph Artifact are read from their own declarations.
    workspace.write("app/main.rs.artf", "name = \"main\"\n");
    for name in ["main", "graph"] {
        let declarations = workspace.read(name, &sites[&Workspace::name(name)]);
        assert!(declarations.unwrap().tools().unwrap().is_empty(), "{name}");
    }
    // A child is read as its own Artifact too.
    let declared = workspace
        .read("lib", &sites[&Workspace::name("lib")])
        .unwrap();
    assert_eq!(declared.tools().unwrap()[0].0.name.as_str(), "open_lib");
}

#[test]
fn a_broken_declaration_in_its_scope_fails_as_a_full_read_does() {
    let workspace = Workspace::new();
    let sites = read_sites(&workspace.0).unwrap();
    workspace.write("app/lib/index.artf", "name = \"lib\"\nbasis = \"yes\"\n");
    let full = read_workspace_config(&workspace.0).unwrap_err().to_string();
    let Err(ScopeReadError::Config(scoped)) =
        workspace.read("app", &sites[&Workspace::name("app")])
    else {
        panic!("a broken child is a configuration error");
    };
    assert_eq!(scoped.to_string(), full);
}

#[test]
fn a_site_that_no_longer_declares_the_artifact_has_moved() {
    let workspace = Workspace::new();
    let sites = read_sites(&workspace.0).unwrap();
    let moved = |name: &str, site: &Site| {
        matches!(
            workspace.read(name, site),
            Err(ScopeReadError::Moved { .. })
        )
    };
    // Another Artifact's site, or one of another kind at the same path.
    assert!(moved("app", &sites[&Workspace::name("docs")]));
    let folder = Site {
        kind: ArtifactKind::Folder,
        path: "app/main.rs".into(),
    };
    assert!(moved("main", &folder));
    // Renamed.
    workspace.write("docs/index.artf", "name = \"guide\"\n");
    assert!(moved("docs", &sites[&Workspace::name("docs")]));
    // Kept out of discovery.
    workspace.write(IGNORE_FILE, "app/\n");
    assert!(moved("lib", &sites[&Workspace::name("lib")]));
    fs::remove_file(workspace.0.join(IGNORE_FILE)).unwrap();
    // Removed.
    fs::remove_dir_all(workspace.0.join("app")).unwrap();
    assert!(moved("app", &sites[&Workspace::name("app")]));
}
