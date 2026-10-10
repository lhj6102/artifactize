use std::{collections::BTreeMap, path::PathBuf};

use artifactize_tools::scope::{Artifact, ArtifactId, ArtifactKind, Scope, ScopedPath};

fn id(value: &str) -> ArtifactId {
    ArtifactId::new(value).unwrap()
}

#[test]
fn artifact_ids_accept_only_short_names_without_path_syntax() {
    for valid in ["a", "A9", "tools", "code-style", "x_y", &"a".repeat(64)] {
        assert_eq!(id(valid).as_str(), valid);
    }
    for invalid in [
        "",
        "_lead",
        "-lead",
        "a/b",
        "a.b",
        "..",
        "a b",
        "a\n",
        "é",
        &"a".repeat(65),
    ] {
        assert!(ArtifactId::new(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn mounts_and_children_resolve_to_their_target_ids() {
    let folder = |path: &str, mounts: &[(&str, &str)], children: &[(&str, &str)]| Artifact {
        path: PathBuf::from(path),
        kind: ArtifactKind::Folder,
        name: path.into(),
        mounts: mounts
            .iter()
            .map(|(alias, target)| ((*alias).into(), id(target)))
            .collect(),
        children: children
            .iter()
            .map(|(prefix, target)| ((*prefix).into(), id(target)))
            .collect(),
    };
    let scope = Scope {
        artifacts: BTreeMap::from([
            (id("review"), folder("review", &[("data", "input")], &[])),
            (id("input"), folder("input", &[], &[("deep/leaf", "leaf")])),
            (id("leaf"), folder("input/deep/leaf", &[], &[])),
        ]),
    };
    assert_eq!(
        scope
            .resolve_path(&id("review"), "data/deep/leaf/file")
            .unwrap(),
        ScopedPath {
            artifact_id: id("leaf"),
            path: "file".into(),
        }
    );
    assert_eq!(
        scope.resolve_path(&id("review"), "data/deep").unwrap(),
        ScopedPath {
            artifact_id: id("input"),
            path: "deep".into(),
        }
    );
    assert!(
        scope
            .resolve_path(&id("other"), "")
            .unwrap_err()
            .to_string()
            .contains("outside this review")
    );
}
