use std::fs;

use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::{config::read_workspace_config, test_os::symlink_file};

async fn prepare_all<'a>(
    repo: &Repo,
    config: &'a RepoConfig,
) -> BTreeMap<&'a ArtifactName, PreparedFingerprint> {
    prepare(
        config,
        [
            &config.artifacts["app"].name,
            &config.artifacts["basis"].name,
        ],
        repo.output.path(),
        &Parallelism::new(2),
        CancellationToken::new(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn default_artifactsum_matches_explicit_form_and_keys_a_basis_dependency() {
    let repo = Repo::new();
    repo.artifact("basis", json!({"name":"basis","basis":true}));
    repo.write("basis/rules.txt", "rules");
    repo.artifact(
        "app",
        json!({"name":"app","evals":[{
            "id":"check","title":"Check","profile":{"kind":"human"},
            "payload":{"instruction":"Check against {basis}."}
        }]}),
    );
    repo.write("app/input.txt", "input");
    let config = read_workspace_config(repo.root.path()).unwrap();
    let default = prepare_all(&repo, &config).await;
    let key = eval_key(&config, &config.evals[0], &default).unwrap();
    for id in ["app", "basis"] {
        assert!(
            default[&config.artifacts[id].name]
                .value
                .starts_with("artifactsum:")
        );
        assert_eq!(default[&config.artifacts[id].name].value.len(), 76);
        assert!(key.fingerprints.contains_key(id));
    }
    repo.artifact(
        "basis",
        json!({"name":"basis","basis":true,"fingerprint":{}}),
    );
    let explicit_config = read_workspace_config(repo.root.path()).unwrap();
    let explicit = prepare_all(&repo, &explicit_config).await;
    assert_eq!(
        default[&config.artifacts["basis"].name].value,
        explicit[&explicit_config.artifacts["basis"].name].value
    );
    assert_eq!(
        default[&config.artifacts["basis"].name].manifest,
        explicit[&explicit_config.artifacts["basis"].name].manifest
    );
    assert_eq!(
        key,
        eval_key(&explicit_config, &explicit_config.evals[0], &explicit).unwrap()
    );
}

#[tokio::test]
async fn fingerprint_false_is_not_prepared_or_rechecked_and_leaves_no_key() {
    let repo = Repo::new();
    repo.artifact(
        "",
        json!({"name":"app","fingerprint":false,"evals":[{
            "id":"check","title":"Check","profile":{"kind":"human"},
            "payload":{"instruction":"Check."}
        }]}),
    );
    // Even an invalid artifactsum input is never read for a disabled fingerprint.
    let _ = symlink_file("missing", repo.root.path().join("link"));
    let config = read_workspace_config(repo.root.path()).unwrap();
    let fingerprints = prepare(
        &config,
        [&config.artifacts["app"].name],
        repo.output.path(),
        &Parallelism::new(2),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(fingerprints.is_empty());
    assert_eq!(
        eval_key(&config, &config.evals[0], &fingerprints),
        Err(Unkeyed::Target)
    );
    assert!(
        recheck(
            &config,
            &config.evals[0],
            repo.output.path(),
            &Parallelism::new(2),
            CancellationToken::new()
        )
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(fs::read_dir(repo.output.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn tag_changes_leave_artifactsum_eval_definition_and_reuse_key_unchanged() {
    let repo = Repo::new();
    let mut app = json!({"name":"app","tags":["type:code"],"mounts":{"rules":"basis"},"evals":[{
        "id":"check","title":"Check","profile":{"kind":"human"},
        "payload":{"instruction":"Check."}
    }]});
    repo.artifact("app", app.clone());
    repo.write("app/input.txt", "input");
    repo.artifact(
        "basis",
        json!({"name":"basis","basis":true,"tags":["type:reference"]}),
    );
    let before = read_workspace_config(repo.root.path()).unwrap();
    let fingerprints = prepare_all(&repo, &before).await;
    let key = eval_key(&before, &before.evals[0], &fingerprints).unwrap();
    app["tags"] = json!(["scope:combat", "type:image"]);
    repo.artifact("app", app);
    repo.artifact("basis", json!({"name":"basis","basis":true,"tags":[]}));
    let after = read_workspace_config(repo.root.path()).unwrap();
    let fingerprints = prepare_all(&repo, &after).await;
    assert_eq!(
        key,
        eval_key(&after, &after.evals[0], &fingerprints).unwrap()
    );
}
