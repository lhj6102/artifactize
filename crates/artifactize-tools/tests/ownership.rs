#[path = "support/os.rs"]
mod os;

use std::{collections::BTreeMap, fs};

use artifactize_tools::{
    Builtin, Content,
    builtin::{self, Input},
    scope::{Artifact, ArtifactId, ArtifactKind, ChildPrefix, MountAlias, Scope},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn id(value: &str) -> ArtifactId {
    ArtifactId::new(value).unwrap()
}

#[test]
fn child_and_mount_names_cannot_be_bypassed_by_a_filesystem_alias() {
    let directory = os::tempdir();
    let root = fs::canonicalize(directory.path()).unwrap();
    fs::create_dir(root.join("Secret")).unwrap();
    fs::write(root.join("Secret/x"), "excluded").unwrap();
    fs::write(root.join("public"), "visible").unwrap();
    let owner = Artifact {
        path: "".into(),
        kind: ArtifactKind::Folder,
        name: "parent".into(),
        children: BTreeMap::from([(ChildPrefix::new("Secret").unwrap(), id("child"))]),
        mounts: BTreeMap::from([(MountAlias::new("Remote").unwrap(), id("unavailable"))]),
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

    // A physical entry added after the logical scope was built cannot shadow a mount by
    // changing its case on an insensitive volume; distinct names remain usable otherwise.
    fs::create_dir(root.join("remote")).unwrap();
    fs::write(root.join("remote/x"), "physical").unwrap();
    let insensitive = os::case_policy(&root) == os::CasePolicy::Insensitive;
    let result = call(Builtin::Read, json!({"path":"remote/x"}));
    assert_eq!(result.is_error, insensitive, "{result:?}");
    if !insensitive {
        assert!(
            matches!(&result.content[0], Content::Json { data } if data["lines"][0]["text"] == "physical")
        );
    }

    // A stale scope still reserves the excluded child after a case-only rename. Use an
    // intermediate name because Windows may otherwise leave the old spelling unchanged.
    if insensitive {
        fs::rename(root.join("Secret"), root.join("renamed")).unwrap();
        fs::rename(root.join("renamed"), root.join("secret")).unwrap();
        let result = call(Builtin::Read, json!({"path":"secret/x"}));
        assert!(result.is_error, "{result:?}");
    }

    // On a case-sensitive volume an independently created lowercase directory stays usable.
    if !insensitive {
        fs::create_dir(root.join("secret")).unwrap();
        fs::write(root.join("secret/x"), "distinct").unwrap();
        let result = call(Builtin::Read, json!({"path":"secret/x"}));
        assert!(!result.is_error, "{result:?}");
        assert!(
            matches!(&result.content[0], Content::Json { data } if data["lines"][0]["text"] == "distinct")
        );
    }
}
