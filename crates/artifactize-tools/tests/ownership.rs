use std::{collections::BTreeMap, fs};

use artifactize_tools::{
    Builtin, Content,
    builtin::{self, Input},
    scope::{Artifact, ArtifactId, ArtifactKind, Scope},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn id(value: &str) -> ArtifactId {
    ArtifactId::new(value).unwrap()
}

#[test]
fn child_and_mount_names_cannot_be_bypassed_by_a_filesystem_alias() {
    let directory = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(directory.path()).unwrap();
    fs::create_dir(root.join("Secret")).unwrap();
    fs::write(root.join("Secret/x"), "excluded").unwrap();
    fs::write(root.join("public"), "visible").unwrap();
    let owner = Artifact {
        path: "".into(),
        kind: ArtifactKind::Folder,
        name: "parent".into(),
        children: BTreeMap::from([("Secret".into(), id("child"))]),
        mounts: BTreeMap::from([("Remote".into(), id("unavailable"))]),
    };
    // Excluded child and mounted Artifact data are intentionally absent from this scope.
    let scope = Scope {
        artifacts: BTreeMap::from([(id("parent"), owner)]),
    };
    let call = |builtin, args| {
        builtin::call(
            Input::parse(builtin, args).unwrap(),
            &root,
            &scope,
            &id("parent"),
            &CancellationToken::new(),
        )
    };
    for path in ["Secret/x", "secret/x", "SECRET/x", "Remote/x", "remote/x"] {
        let result = call(Builtin::Read, json!({"path":path}));
        assert!(result.is_error, "{path}: {result:?}");
        assert!(!format!("{result:?}").contains("excluded"));
    }
    let result = call(Builtin::Read, json!({"path":"public"}));
    assert!(!result.is_error, "{result:?}");
    assert!(
        matches!(&result.content[0], Content::Json { data } if data["lines"][0]["text"] == "visible")
    );

    // On a case-sensitive volume an independently created lowercase directory stays usable.
    if !root.join("secret").exists() {
        fs::create_dir(root.join("secret")).unwrap();
        fs::write(root.join("secret/x"), "distinct").unwrap();
        let result = call(Builtin::Read, json!({"path":"secret/x"}));
        assert!(!result.is_error, "{result:?}");
        assert!(
            matches!(&result.content[0], Content::Json { data } if data["lines"][0]["text"] == "distinct")
        );
    }
}
