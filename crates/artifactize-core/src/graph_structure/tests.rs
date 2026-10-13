use super::*;
use serde_json::json;
use std::fs;
use tokio_util::sync::CancellationToken;

const GRAPH: &str = "name = 'graph'\n[evals.review]\ntitle = 'Review graph'\nprofile = { kind = 'human' }\npayload = { instruction = 'Review the project structure.' }\n";
const APP: &str = "name = 'app'\ntags = ['code']\nmounts = { input = 'leaf' }\n[evals.review]\ntitle = 'Review app'\nprofile = { kind = 'agent', backend = 'openai', model = 'model-one', reasoning = 'high', timeout_ms = 10000, max_tool_calls = 10, max_tokens = 1000 }\npayload = { instruction = 'Review {input} and {app}.', context = 'criteria' }\npass_schema = { type = 'object' }\nfail_schema = { type = 'object' }\n[evals.ready]\ntitle = 'Inputs ready'\nprofile = { kind = 'dependency', depends_on = ['input'] }\n[evals.command]\ntitle = 'Runtime criteria'\nprofile = { kind = 'runtime', command = 'runtime-program', args = ['{input}/input.txt'], timeout_ms = 1234 }\npayload = { instruction = 'Check.' }\n";

fn fixture() -> tempfile::TempDir {
    let repo = crate::test_os::tempdir();
    fs::write(repo.path().join(".artf"), GRAPH).unwrap();
    fs::write(repo.path().join("index.artf"), APP).unwrap();
    fs::create_dir(repo.path().join("child")).unwrap();
    fs::write(repo.path().join("child/index.artf"), "name = 'leaf'\nbasis = true\nfingerprint = { script = { command = 'fingerprint-program', args = ['version'], files = ['input.txt'], timeout_ms = 1000 } }\n").unwrap();
    fs::write(repo.path().join("child/input.txt"), "data").unwrap();
    repo
}

#[test]
fn graph_fingerprint_tracks_relations_and_criteria_not_execution_or_location() {
    let repo = fixture();
    let fingerprint = || read(repo.path()).unwrap().fingerprint();
    let baseline = fingerprint();
    // Each independent change is meaningful even when no input bytes change.
    for changed in [
        APP.replace("input = 'leaf'", "input = 'graph'"),
        APP.replace("Review app", "A different title"),
        APP.replace("command = 'runtime-program'", "command = 'different-program'"),
        APP.replace("args = ['{input}/input.txt']", "args = ['{input}/input.txt', 'criterion']"),
        APP.replace("context = 'criteria'", "context = 'different criteria'"),
        APP.replace("Review {input} and {app}.", "Review {app}."),
        APP.replace("pass_schema = { type = 'object' }", "pass_schema = { type = 'object', required = ['summary'] }"),
        APP.replace("depends_on = ['input']", "depends_on = ['graph']"),
        APP.replace("kind = 'agent', backend = 'openai', model = 'model-one', reasoning = 'high', timeout_ms = 10000, max_tool_calls = 10, max_tokens = 1000", "kind = 'human'"),
    ] {
        fs::write(repo.path().join("index.artf"), changed).unwrap();
        assert_ne!(fingerprint(), baseline);
    }
    fs::write(repo.path().join("index.artf"), APP).unwrap();
    // Options, variants and views never enter extraction, including runtime variants.
    let excluded = APP
        .replace("['code']", "['changed-tag']")
        .replace("backend = 'openai'", "backend = 'codex'")
        .replace("model-one", "model-two")
        .replace("reasoning = 'high'", "reasoning = 'low'")
        .replace("timeout_ms = 10000", "timeout_ms = 20000")
        .replace("timeout_ms = 1234", "timeout_ms = 5678")
        .replace("max_tool_calls = 10", "max_tool_calls = 20")
        .replace("max_tokens = 1000", "max_tokens = 2000")
        + "\n[evals.review.profile_variants]\nalternate = { kind = 'agent', backend = 'openai', model = 'alternate' }\n[views.agent_tools.graph]\nbuiltin = 'graph'\nargs = []\n";
    fs::write(repo.path().join("index.artf"), excluded).unwrap();
    assert_eq!(fingerprint(), baseline);
    let leaf = fs::read_to_string(repo.path().join("child/index.artf")).unwrap();
    fs::write(
        repo.path().join("child/index.artf"),
        leaf.replace("timeout_ms = 1000", "timeout_ms = 9000"),
    )
    .unwrap();
    fs::write(repo.path().join("child/input.txt"), "changed contents").unwrap();
    assert_eq!(fingerprint(), baseline);
    fs::rename(repo.path().join("child"), repo.path().join("elsewhere")).unwrap();
    assert_eq!(fingerprint(), baseline);
    fs::write(
        repo.path().join("elsewhere/index.artf"),
        leaf.replace("['version']", "['new-version']"),
    )
    .unwrap();
    assert_ne!(fingerprint(), baseline);
    // Its own criteria belong to the graph too; no recursive fingerprint dependency.
    fs::write(repo.path().join("elsewhere/index.artf"), leaf).unwrap();
    fs::write(
        repo.path().join(".artf"),
        GRAPH.replace("Review graph", "New graph title"),
    )
    .unwrap();
    assert_ne!(fingerprint(), baseline);
    fs::write(
        repo.path().join(".artf"),
        GRAPH.replace(
            "title = 'Review graph'",
            "gate = 'blocking'\ntitle = 'Review graph'",
        ),
    )
    .unwrap();
    assert_ne!(fingerprint(), baseline);
}

#[test]
fn graph_declaration_is_root_only_and_rejects_non_subject_fields() {
    let repo = fixture();
    for field in ["fingerprint = false", "fingerprint = {}", "basis = false"] {
        fs::write(
            repo.path().join(".artf"),
            format!("name = 'graph'\n{field}\n"),
        )
        .unwrap();
        let error = read(repo.path()).unwrap_err();
        assert!(
            error.contains("The graph Artifact cannot declare"),
            "{error}"
        );
    }
    fs::write(
        repo.path().join(".artf"),
        "name = 'graph'\nreview_policy = {}\n",
    )
    .unwrap();
    assert!(
        read(repo.path())
            .unwrap_err()
            .contains("unknown field `review_policy`")
    );
    fs::write(repo.path().join(".artf"), "name = 'graph'\n").unwrap();
    let config = crate::config::read_workspace_config(repo.path()).unwrap();
    let graph = &config.artifacts["graph"];
    assert_eq!(graph.kind, ArtifactKind::Graph);
    assert!(graph.children.is_empty());
    assert!(graph.mounts.is_empty());
    assert!(!config.evals.iter().any(|eval| eval.target == "graph"));
    fs::write(repo.path().join("child/.artf"), "name = 'other-graph'\n").unwrap();
    assert!(
        read(repo.path())
            .unwrap_err()
            .contains("only at the repository root")
    );
}

#[tokio::test]
async fn graph_agent_tool_prints_current_extracted_structure_from_any_artifact() {
    let repo = fixture();
    fs::write(repo.path().join("index.artf"), format!("{APP}\n[views.agent_tools.structure]\nbuiltin = 'graph'\nargs = []\n[views.human_tools.viewer]\nbuiltin = 'graph'\nkind = 'launch'\nargs = []\n")).unwrap();
    let config = crate::config::read_workspace_config(repo.path()).unwrap();
    let registry = crate::tools::Registry::for_artifact(&config, "app").unwrap();
    // Change an unrelated definition after creating the registry: current, not saved state.
    fs::write(
        repo.path().join(".artf"),
        GRAPH.replace("Review graph", "Changed graph title"),
    )
    .unwrap();
    let structure = read(repo.path()).unwrap();
    let result = registry
        .call(
            "structure_app",
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
    assert_eq!(text, &structure.tree());
    assert!(text.contains("leaf → app"));
    assert!(text.contains("graph/review: Changed graph title"));
    let human = crate::tools::human::Registry::for_artifact(&config, "app").unwrap();
    human.preflight(&"viewer_app".parse().unwrap()).unwrap();
    assert_eq!(
        human.list().next().unwrap().kind,
        crate::config::HumanToolKind::Launch
    );
}
