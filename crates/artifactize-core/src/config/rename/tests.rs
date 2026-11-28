//! Renaming at its seam: definitions on disk in, the same project under the new name out.

use std::fs;

use super::*;

fn name(text: &str) -> ArtifactName {
    text.parse().unwrap()
}

#[test]
fn a_rename_changes_the_binding_every_reference_and_nothing_else() {
    let directory = crate::test_os::tempdir();
    let root = directory.path();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    let definitions = root.join(".artifactize");
    fs::create_dir_all(&definitions).unwrap();
    fs::write(
        definitions.join("docs.artf"),
        "// The docs.\nartf docs : files = { path = \"docs/\" }\n",
    )
    .unwrap();
    fs::write(
        definitions.join("profiles.artfprofiles"),
        "profile reviewer : agent = { backend = \"openai\", model = \"fixture\" }\n",
    )
    .unwrap();
    // `docs` is also a word in prose: it is no Artifact.
    fs::write(
        definitions.join("app.artf"),
        r#"tool reading = { agent read = builtin(op = "read") }
artf! checked(input) : files & reading = {
  path = "src/"
  parts = [input]
  eval matches : reviewer = {
    title = "Matches the docs"
    instruction = "Compare @{artf self} with @{artf docs}."
  }
  eval tests : runtime(command = "true", args = ["--docs=@{path docs}"]) = {
    title = "Tests"
    deps = [docs]
  }
}
artf app = checked!(input = docs)
"#,
    )
    .unwrap();
    let renamed = rename(root, &name("docs"), &name("guides")).unwrap();
    assert_eq!(
        renamed.moved,
        Some((
            ".artifactize/docs.artf".into(),
            ".artifactize/guides.artf".into()
        ))
    );
    let app = fs::read_to_string(definitions.join("app.artf")).unwrap();
    assert!(app.contains("with @{artf guides}."), "{app}");
    assert!(
        app.contains("--docs=@{path guides}") && app.contains("deps = [guides]"),
        "{app}"
    );
    assert!(
        app.contains("checked!(input = guides)") && app.contains("Matches the docs"),
        "{app}"
    );
    assert_eq!(
        fs::read_to_string(definitions.join("guides.artf")).unwrap(),
        "// The docs.\nartf guides : files = { path = \"docs/\" }\n"
    );
    let config = crate::config::read_workspace_config(root).unwrap();
    assert!(
        config.artifacts[&name("app")]
            .parts
            .contains(&name("guides"))
    );
    let error = rename(root, &name("guides"), &name("APP"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("already declared"), "{error}");
    // Toolsets and profiles share the Artifacts' names: neither name is free.
    for taken in ["reading", "reviewer"] {
        let error = rename(root, &name("guides"), &name(taken))
            .unwrap_err()
            .to_string();
        assert!(error.contains("already declared as"), "{taken}: {error}");
    }
}
