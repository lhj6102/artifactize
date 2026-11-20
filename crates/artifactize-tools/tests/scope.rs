use std::collections::BTreeMap;

use artifactize_tools::scope::{
    Artifact, ArtifactId, ArtifactKind, ChildPrefix, Scope, ScopedPath, Target,
};

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
fn nested_folders_resolve_to_their_target_ids() {
    let folder = |path: &str, nested: &[(&str, &str)]| Artifact {
        path: path.try_into().unwrap(),

        name: path.into(),
        covering_part: None,
        nested: nested
            .iter()
            .map(|(prefix, target)| (ChildPrefix::new(*prefix).unwrap(), id(target)))
            .collect(),
    };
    let scope = Scope {
        artifacts: BTreeMap::from([
            (id("review"), folder("review", &[("data", "input")])),
            (id("input"), folder("review/data", &[("deep/leaf", "leaf")])),
            (id("leaf"), folder("review/data/deep/leaf", &[])),
        ]),
    };
    assert_eq!(
        scope
            .resolve_path(&id("review"), "data/deep/leaf/file")
            .unwrap(),
        ScopedPath {
            artifact_id: id("leaf"),
            path: "file".parse().unwrap(),
        }
    );
    assert_eq!(
        scope.resolve_path(&id("review"), "data/deep").unwrap(),
        ScopedPath {
            artifact_id: id("input"),
            path: "deep".parse().unwrap(),
        }
    );
    assert_eq!(
        scope.resolve_path(&id("review"), "data").unwrap(),
        ScopedPath {
            artifact_id: id("input"),
            path: "".parse().unwrap(),
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

#[test]
fn stored_targets_accept_slash_paths_and_keep_live_addresses_opaque() {
    for text in [
        "",
        "crates/tools/note.md",
        "https://example.invalid/open?q=a:b",
    ] {
        let wire = serde_json::json!(text);
        let target: Target = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(target).unwrap(), wire);
    }
    for text in ["/root", "notes/../outside", "notes//file", r"notes\file"] {
        assert!(
            serde_json::from_value::<Target>(serde_json::json!(text)).is_err(),
            "{text:?}"
        );
    }
}

#[test]
fn the_project_root_cannot_be_a_file_target() {
    for root in ["", "."] {
        let target: Target = serde_json::from_value(serde_json::json!(root)).unwrap();
        assert!(target.with_kind(ArtifactKind::File, vec![]).is_err());
    }
    let target: Target = serde_json::from_value(serde_json::json!("src/note.md")).unwrap();
    let target = target.with_kind(ArtifactKind::File, vec![]).unwrap();
    assert_eq!(target.kind(), ArtifactKind::File);
    assert_eq!(target.folder(), std::path::Path::new("src"));
}

#[test]
fn saved_local_target_uses_canonical_slashes() {
    let target = Target::try_from("notes//draft.md").unwrap();
    let saved = serde_json::to_value(&target).unwrap();
    assert_eq!(saved, serde_json::json!("notes/draft.md"));
    assert_eq!(serde_json::from_value::<Target>(saved).unwrap(), target);
}
