use super::*;
use std::fs;

/// `(name, plan)` of every Artifact of `current`.
fn plans(current: &Current) -> Vec<(String, Plan)> {
    current
        .artifacts
        .iter()
        .map(|artifact| (artifact.name.to_string(), artifact.plan))
        .collect()
}

fn edges(current: &Current) -> Vec<(String, String)> {
    current
        .edges
        .iter()
        .map(|edge| (edge.from.to_string(), edge.to.to_string()))
        .collect()
}

#[tokio::test]
async fn a_scan_shows_the_graph_first_every_artifact_checking_then_what_verify_would_do() {
    let repo = crate::test_os::tempdir();
    let state = crate::test_os::tempdir();
    for (folder, declaration) in [
        (
            "notes",
            "name = 'notes'\nfingerprint = { files = ['.'] }\n[evals.read]\ntitle = 'Read the notes'\nprofile = { kind = 'human' }\npayload = { instruction = 'Read them.' }\n",
        ),
        (
            "app",
            "name = 'app'\nfingerprint = { files = ['.'] }\n[evals.review]\ntitle = 'Review the app'\nprofile = { kind = 'human' }\npayload = { instruction = 'Review it with {notes}.' }\n",
        ),
    ] {
        fs::create_dir(repo.path().join(folder)).unwrap();
        fs::write(repo.path().join(folder).join("index.artf"), declaration).unwrap();
    }
    let mut published = Vec::new();
    let current = scan(
        repo.path(),
        state.path(),
        CancellationToken::new(),
        |current| published.push(current),
    )
    .await
    .unwrap();
    let first = published.first().expect("the graph is published first");
    assert_eq!(
        plans(first),
        [("app".into(), Plan::Check), ("notes".into(), Plan::Check)]
    );
    assert_eq!(first.artifacts[0].text(), "checking");
    assert!(
        first.artifacts[0].evals[0]
            .reason
            .starts_with("Still being checked")
    );
    assert_eq!(edges(first), [("notes".into(), "app".into())]);
    assert_eq!(first.structure, current.structure);
    // Once every fingerprint is in, nothing is being checked.
    assert_eq!(
        plans(&current),
        [("app".into(), Plan::Run), ("notes".into(), Plan::Run)]
    );
    assert_eq!(edges(&current), edges(first));
}

#[tokio::test]
async fn a_whole_holds_its_parts_without_edges_to_them_and_does_what_they_do() {
    let repo = crate::test_os::tempdir();
    let state = crate::test_os::tempdir();
    for (folder, declaration) in [
        ("kit", "name = 'kit'\nparts = ['notes']\n"),
        (
            "kit/core",
            "name = 'core'\nfingerprint = { files = ['.'] }\n[evals.review]\ntitle = 'Review the core'\nprofile = { kind = 'human' }\npayload = { instruction = 'Review it.' }\n",
        ),
        ("notes", "name = 'notes'\nbasis = true\n"),
    ] {
        fs::create_dir_all(repo.path().join(folder)).unwrap();
        fs::write(repo.path().join(folder).join("index.artf"), declaration).unwrap();
    }
    let current = scan(repo.path(), state.path(), CancellationToken::new(), |_| {})
        .await
        .unwrap();
    let kit = current
        .artifacts
        .iter()
        .find(|artifact| artifact.name.as_str() == "kit")
        .unwrap();
    assert_eq!(
        kit.parts
            .iter()
            .map(|part| part.as_str())
            .collect::<Vec<_>>(),
        ["core", "notes"]
    );
    assert!(edges(&current).is_empty());
    // Without evals of its own, it would run what its part would run.
    assert_eq!(kit.plan, Plan::Run);
    assert_eq!(kit.text(), "would run");
}
