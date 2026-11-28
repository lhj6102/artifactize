use super::*;
use serde_json::json;
use std::fs;
use tokio_util::sync::CancellationToken;

const GRAPH: &str = "name = 'graph'\n[evals.review]\ntitle = 'Review graph'\nprofile = { kind = 'human' }\npayload = { instruction = 'Review the project structure.' }\n";
const APP: &str = "name = 'app'\ntags = ['code']\n[evals.review]\ntitle = 'Review app'\nprofile = { kind = 'agent', backend = 'openai', model = 'model-one', reasoning = 'high', timeout_ms = 10000, max_tool_calls = 10, max_tokens = 1000 }\npayload = { instruction = 'Review {leaf} and {app}.' }\nred_at = 'P1'\n[evals.command]\ntitle = 'Runtime criteria'\nprofile = { kind = 'runtime', command = 'runtime-program', args = ['{leaf}/input.txt'], timeout_ms = 1234 }\npayload = { instruction = 'Check.' }\n";

fn fixture() -> tempfile::TempDir {
    let repo = crate::test_os::tempdir();
    crate::test_declaration::write(repo.path(), repo.path().join(".artf"), GRAPH).unwrap();
    crate::test_declaration::write(repo.path(), repo.path().join("index.artf"), APP).unwrap();
    fs::create_dir(repo.path().join("child")).unwrap();
    crate::test_declaration::write(repo.path(), repo.path().join("child/index.artf"), "name = 'leaf'\nbasis = true\nfingerprint = { script = { command = 'fingerprint-program', args = ['version'], files = ['input.txt'], timeout_ms = 1000 } }\n").unwrap();
    fs::write(
        crate::test_declaration::source_path(repo.path().join("child/input.txt")),
        "data",
    )
    .unwrap();
    repo
}

#[test]
fn graph_fingerprint_tracks_relations_and_criteria_not_execution_or_location() {
    let repo = fixture();
    let fingerprint = || read(repo.path()).unwrap().fingerprint();
    let baseline = fingerprint();
    // Each independent change is meaningful even when no input bytes change.
    for changed in [
        APP.replace("Review app", "A different title"),
        APP.replace("command = 'runtime-program'", "command = 'different-program'"),
        APP.replace("args = ['{leaf}/input.txt']", "args = ['{leaf}/input.txt', 'criterion']"),
        APP.replace("tags = ['code']", "tags = ['code']\n[views.agent_tools.graph]\nbuiltin = 'graph'\nargs = []"),
        APP.replace("Review {leaf} and {app}.", "Review {app}."),
        APP.replace("red_at = 'P1'", "red_at = 'P2'"),
        APP.replace("red_at = 'P1'\n", ""),
        APP.replace("kind = 'agent', backend = 'openai', model = 'model-one', reasoning = 'high', timeout_ms = 10000, max_tool_calls = 10, max_tokens = 1000", "kind = 'human'").replace("red_at = 'P1'\n", ""),
    ] {
        crate::test_declaration::write(repo.path(), repo.path().join("index.artf"), changed).unwrap();
        assert_ne!(fingerprint(), baseline);
    }
    crate::test_declaration::write(repo.path(), repo.path().join("index.artf"), APP).unwrap();
    // Execution options and the profile's name never enter extraction.
    let excluded = APP
        .replace("['code']", "['changed-tag']")
        .replace(
            "title = 'Review app'",
            "title = 'Review app'\nmutex = ['use-computer:display']",
        )
        .replace("backend = 'openai'", "backend = 'codex'")
        .replace("model-one", "model-two")
        .replace("reasoning = 'high'", "reasoning = 'low'")
        .replace("timeout_ms = 10000", "timeout_ms = 20000")
        .replace("timeout_ms = 1234", "timeout_ms = 5678")
        .replace("max_tool_calls = 10", "max_tool_calls = 20")
        .replace("max_tokens = 1000", "max_tokens = 2000")
        + "\n[evals.review.profile_variants]\nalternate = { kind = 'agent', backend = 'openai', model = 'alternate' }\n";
    crate::test_declaration::write(repo.path(), repo.path().join("index.artf"), excluded).unwrap();
    assert_eq!(fingerprint(), baseline);
    let leaf = crate::test_declaration::read_text(repo.path().join("child/index.artf")).unwrap();
    crate::test_declaration::write(
        repo.path(),
        repo.path().join("child/index.artf"),
        leaf.replace("timeout_ms = 1000", "timeout_ms = 9000"),
    )
    .unwrap();
    fs::write(
        crate::test_declaration::source_path(repo.path().join("child/input.txt")),
        "changed contents",
    )
    .unwrap();
    assert_eq!(fingerprint(), baseline);
    fs::rename(repo.path().join("child"), repo.path().join("elsewhere")).unwrap();
    assert_eq!(fingerprint(), baseline);
    fs::write(
        crate::test_declaration::source_path(repo.path().join(".artifactize/leaf.artf")),
        leaf.replace("version", "new-version"),
    )
    .unwrap();
    assert_ne!(fingerprint(), baseline);
    // Its own criteria belong to the graph too; no recursive fingerprint dependency.
    fs::write(
        crate::test_declaration::source_path(repo.path().join(".artifactize/leaf.artf")),
        leaf,
    )
    .unwrap();
    crate::test_declaration::write(
        repo.path(),
        repo.path().join(".artf"),
        GRAPH.replace("Review graph", "New graph title"),
    )
    .unwrap();
    assert_ne!(fingerprint(), baseline);
    crate::test_declaration::write(
        repo.path(),
        repo.path().join(".artf"),
        GRAPH.replace("Review the project structure.", "Review the new structure."),
    )
    .unwrap();
    assert_ne!(fingerprint(), baseline);
}

#[test]
fn definition_targets_are_ordinary_folder_nodes() {
    let repo = fixture();
    for fingerprint in ["false", "{}"] {
        crate::test_declaration::write(
            repo.path(),
            repo.path().join(".artf"),
            format!("name = 'graph'\nfingerprint = {fingerprint}\nparts = ['leaf']\n"),
        )
        .unwrap();
        let config = crate::config::read_workspace_config(repo.path()).unwrap();
        assert_eq!(config.artifacts["graph"].kind(), ArtifactKind::Folder);
        assert_eq!(
            config.artifacts["graph"].path.local(),
            Some(std::path::Path::new(".artifactize"))
        );
        assert!(config.artifacts["graph"].parts.contains("leaf"));
    }
}

#[tokio::test]
async fn graph_agent_tool_prints_current_extracted_structure_from_any_artifact() {
    let repo = fixture();
    crate::test_declaration::write(repo.path(), repo.path().join("index.artf"), format!("{APP}\n[views.agent_tools.structure]\nbuiltin = 'graph'\nargs = []\n[views.human_tools.viewer]\nbuiltin = 'graph'\nkind = 'launch'\nargs = []\n")).unwrap();
    let config = crate::config::read_workspace_config(repo.path()).unwrap();
    let registry = crate::tools::Registry::for_artifact(&config, &"app".parse().unwrap()).unwrap();
    // Change an unrelated definition after creating the registry: current, not saved state.
    crate::test_declaration::write(
        repo.path(),
        repo.path().join(".artf"),
        GRAPH.replace("Review graph", "Changed graph title"),
    )
    .unwrap();
    let result = registry
        .call(
            "app-tools_structure_app",
            json!({}),
            repo.path(),
            CancellationToken::new(),
        )
        .await;
    assert!(!result.is_error(), "{result:?}");
    let content = result.into_content();
    let crate::tools::Content::Text { text } = &content[0] else {
        panic!("text tree")
    };
    assert!(text.contains("leaf → app"));
    assert!(text.contains("graph/review: Changed graph title"));
    let human =
        crate::tools::human::Registry::for_artifact(&config, &"app".parse().unwrap()).unwrap();
    human
        .preflight(&"app-tools_viewer_app".parse().unwrap())
        .unwrap();
    assert_eq!(
        human.list().next().unwrap().kind,
        crate::config::HumanToolKind::Launch
    );
}

#[tokio::test]
async fn artifact_type_exposes_the_same_scoped_declarations_to_both_tools() {
    let repo = crate::test_os::tempdir();
    let definitions = repo.path().join(".artifactize");
    let scoped = definitions.join("design");
    fs::create_dir_all(&scoped).unwrap();
    fs::write(repo.path().join("inside.txt"), "design").unwrap();
    fs::write(repo.path().join("plain.txt"), "custom").unwrap();
    fs::write(repo.path().join("outside.txt"), "outside").unwrap();
    fs::write(
        scoped.join("inside.artf"),
        "artf inside : markdown = {\n  path = \"inside.txt\"\n  basis = true\n}\n",
    )
    .unwrap();
    fs::write(
        scoped.join("plain.artf"),
        "artf plain = { path = \"plain.txt\", basis = true }\n",
    )
    .unwrap();
    fs::write(
        definitions.join("outside.artf"),
        "artf outside = { path = \"outside.txt\", basis = true }\n",
    )
    .unwrap();
    fs::write(
        definitions.join("profiles.artfprofiles"),
        "profile fixture : agent = { backend = \"openai\", model = \"fixture\" }\n",
    )
    .unwrap();
    fs::write(
        definitions.join("design-definitions.artf"),
        r#"artf design-definitions : artifactize = {
  path = ".artifactize/design/"
  eval inspect : fixture = {
    title = "Inspect design declarations"
    instruction = "Inspect the declared graph."
  }
}
"#,
    )
    .unwrap();
    let config = crate::config::read_workspace_config(repo.path()).unwrap();
    let registry =
        crate::tools::Registry::new(&config, &"design-definitions/inspect".parse().unwrap())
            .unwrap();
    let tool = "artifactize_declarations_design-definitions";
    let result = registry
        .call(tool, json!({}), repo.path(), CancellationToken::new())
        .await;
    let [crate::tools::Content::Text { text }] = result.content() else {
        panic!("{result:?}")
    };
    assert!(
        text.contains("inside [File]") && !text.contains("outside"),
        "{text}"
    );
    let viewer = crate::viewer::Session::open(
        repo.path(),
        &"design-definitions".parse().unwrap(),
        &"artifactize.view".parse().unwrap(),
    )
    .unwrap();
    let graph = viewer.declarations().unwrap();
    assert_eq!(
        graph
            .artifacts
            .keys()
            .map(|name| name.as_str())
            .collect::<Vec<_>>(),
        ["inside", "plain"]
    );
    // What each composes is part of what a definitions review sees, here and graph-wide.
    let wire = serde_json::to_value(graph).unwrap();
    assert_eq!(wire["artifacts"]["inside"]["toolsets"], json!(["markdown"]));
    assert_eq!(wire["artifacts"]["plain"]["toolsets"], json!([]));
    let whole = serde_json::to_value(extract(&config)).unwrap();
    assert_eq!(
        whole["artifacts"]["inside"]["toolsets"],
        json!(["markdown"])
    );
}
