#[path = "support/os.rs"]
mod os;

use std::{collections::BTreeMap, fs};

use artifactize_tools::{
    Builtin, Content,
    builtin::{self, Input},
    scope::{Artifact, ArtifactId, ArtifactKind, MountAlias, Scope},
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[test]
fn typed_results_preserve_read_pagination_listing_and_search_json() {
    let directory = os::tempdir();
    let root = directory.path();
    fs::write(root.join("notes.md"), "alpha\r\nbeta\nlast").unwrap();
    fs::create_dir(root.join("mounted")).unwrap();
    fs::write(root.join("mounted/extra.md"), "alpha\n").unwrap();
    let id = |name| ArtifactId::new(name).unwrap();
    let owner = id("owner");
    let folder = |path| Artifact {
        path,
        kind: ArtifactKind::Folder,
        name: "fixture".into(),
        children: BTreeMap::new(),
        mounts: BTreeMap::new(),
    };
    let scope = Scope {
        artifacts: BTreeMap::from([
            (
                owner.clone(),
                Artifact {
                    mounts: BTreeMap::from([(MountAlias::new("docs").unwrap(), id("target"))]),
                    ..folder("".into())
                },
            ),
            (id("target"), folder("mounted".into())),
        ]),
    };
    let call = |tool, input| {
        let result = builtin::call(
            Input::parse(tool, input).unwrap(),
            root,
            &scope,
            &owner,
            &CancellationToken::new(),
        );
        assert!(!result.is_error(), "{result:?}");
        let [Content::Json { data }] = result.content() else {
            panic!("expected one JSON result")
        };
        data.clone()
    };
    assert_eq!(
        call(Builtin::Read, json!({"path":"notes.md", "limit":1})),
        json!({
            "artifactId":"owner", "resolvedArtifactId":"owner", "path":"notes.md",
            "lines":[{"number":1,"text":"alpha\r\n"}], "startLine":1,"endLine":1,
            "lineCount":1,"truncated":true,"nextOffset":2,
        })
    );
    assert_eq!(
        call(Builtin::Read, json!({"path":"notes.md", "offset":2})),
        json!({
            "artifactId":"owner", "resolvedArtifactId":"owner", "path":"notes.md",
            "lines":[{"number":2,"text":"beta\n"},{"number":3,"text":"last"}],
            "startLine":2,"endLine":3,"lineCount":2,"truncated":false,
            "nextOffset":null,"totalLines":3,
        })
    );
    let listing = call(Builtin::List, json!({"limit":1}));
    assert_eq!(
        listing,
        json!({
            "artifactId":"owner","path":"",
            "entries":[{"artifactId":"target","kind":"mount","name":"docs","path":"docs"}],
            "totalEntries":3,"truncated":true,"nextOffset":1,
        })
    );
    assert_eq!(
        call(Builtin::Glob, json!({"path":"docs", "pattern":"*.md"})),
        json!({"path":"docs","files":["docs/extra.md"],"truncated":false})
    );
    assert_eq!(
        call(Builtin::Grep, json!({"path":"notes.md", "pattern":"alpha"})),
        json!({"path":"notes.md","matches":[{"path":"notes.md","line":1,"text":"alpha"}],"truncated":false})
    );
    assert_eq!(
        call(Builtin::Read, json!({"path":"notes.md", "offset":4}))["endLine"],
        Value::Null
    );
}
