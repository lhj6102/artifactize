#[path = "support/os.rs"]
mod os;

use std::{collections::BTreeMap, fs};

use artifactize_tools::{
    Builtin, Content,
    builtin::{self, Input},
    scope::{Artifact, ArtifactId, ArtifactKind, ChildPrefix, Scope},
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[test]
fn typed_results_preserve_read_pagination_listing_and_search_json() {
    let directory = os::tempdir();
    let root = directory.path();
    fs::write(root.join("notes.md"), "alpha\r\nbeta\nlast").unwrap();
    fs::create_dir(root.join("docs")).unwrap();
    fs::write(root.join("docs/extra.md"), "alpha\n").unwrap();
    fs::create_dir(root.join("assets")).unwrap();
    let id = |name| ArtifactId::new(name).unwrap();
    let owner = id("owner");
    let folder = |path| Artifact {
        path,
        kind: ArtifactKind::Folder,
        name: "fixture".into(),
        children: BTreeMap::new(),
    };
    let scope = Scope {
        artifacts: BTreeMap::from([
            (
                owner.clone(),
                Artifact {
                    children: BTreeMap::from([(ChildPrefix::new("docs").unwrap(), id("target"))]),
                    ..folder("".into())
                },
            ),
            (id("target"), folder("docs".into())),
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
            "entries":[{"kind":"directory","name":"assets","path":"assets"}],
            "totalEntries":3,"truncated":true,"nextOffset":1,
        })
    );
    assert_eq!(
        call(Builtin::List, json!({})),
        json!({
            "artifactId":"owner", "path":"",
            "entries":[
                {"kind":"directory","name":"assets","path":"assets"},
                {"kind":"directory","name":"docs","path":"docs"},
                {"kind":"file","name":"notes.md","path":"notes.md"},
            ],
            "totalEntries":3,"truncated":false,"nextOffset":null,
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

fn folder_scope(owner: &ArtifactId) -> Scope {
    Scope {
        artifacts: BTreeMap::from([(
            owner.clone(),
            Artifact {
                path: "".into(),
                kind: ArtifactKind::Folder,
                name: "fixture".into(),
                children: BTreeMap::new(),
            },
        )]),
    }
}

#[tokio::test(start_paused = true)]
async fn help_maps_the_launcher_deadline_to_a_tool_error() {
    use artifactize_tools::launch::{Launch, LaunchError, Launcher, Running};
    use std::{sync::Mutex, time::Duration};

    struct Expiring {
        started: Mutex<Option<tokio::sync::oneshot::Sender<Launch>>>,
    }
    impl Launcher for Expiring {
        fn run<'a>(&'a self, launch: Launch, _: &'a CancellationToken) -> Running<'a> {
            Box::pin(async move {
                let timeout = launch.timeout;
                self.started
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send(launch)
                    .unwrap();
                tokio::time::sleep(timeout).await;
                Err(LaunchError::TimedOut)
            })
        }
    }

    let root = os::tempdir();
    let program = os::compile(root.path(), "help-stub", "fn main() {}\n");
    let owner = ArtifactId::new("owner").unwrap();
    let scope = folder_scope(&owner);
    let (started, received) = tokio::sync::oneshot::channel();
    let launcher = Expiring {
        started: Mutex::new(Some(started)),
    };
    let cancellation = CancellationToken::new();
    let args = [
        artifactize_tools::files::path_text(&program),
        "subcommand".into(),
    ];
    let mut running = Box::pin(builtin::call_fixed(
        Builtin::Help,
        &args,
        json!({}),
        builtin::Context {
            root: root.path(),
            scope: &scope,
            owner: &owner,
            launcher: &launcher,
        },
        &cancellation,
    ));
    let launch = tokio::select! {
        launch = received => launch.unwrap(),
        result = &mut running => panic!("help finished before its launcher deadline: {result:?}"),
    };
    // The documented help budget is independent of implementation constants.
    const HELP_BUDGET: Duration = Duration::from_secs(10);
    assert_eq!(launch.timeout, HELP_BUDGET);
    assert_eq!(
        artifactize_tools::files::path_text(&launch.program),
        artifactize_tools::files::path_text(&program)
    );
    assert_eq!(launch.cwd, root.path());
    assert_eq!(launch.args, ["subcommand", "--help"]);
    assert_eq!(launch.output_limit, 64 * 1024);
    tokio::time::advance(HELP_BUDGET).await;
    assert_eq!(
        serde_json::to_value(running.await).unwrap(),
        json!({"content":[{"type":"text","text":"Help timed out."}],"isError":true}),
    );
}

#[tokio::test]
async fn fixed_calls_reject_invalid_declarations_and_unexpected_model_fields() {
    let root = os::tempdir();
    fs::write(root.path().join("notes"), "# Title\nbody\n").unwrap();
    let owner = ArtifactId::new("owner").unwrap();
    let scope = folder_scope(&owner);
    for (tool, args, input) in [
        (Builtin::Read, vec![], json!({})),
        (Builtin::Read, vec!["notes".into()], json!({"path":"other"})),
        (Builtin::Help, vec!["program".into()], json!({"extra":true})),
        (Builtin::Section, vec!["notes".into()], json!({})),
        (
            Builtin::Section,
            vec!["notes".into(), "Title".into()],
            json!({"heading":"Other"}),
        ),
    ] {
        let result = builtin::call_fixed(
            tool,
            &args,
            input,
            builtin::Context {
                root: root.path(),
                scope: &scope,
                owner: &owner,
                launcher: &artifactize_tools::launch::Standalone,
            },
            &CancellationToken::new(),
        )
        .await;
        assert!(result.is_error(), "{result:?}");
    }
    let result = builtin::call_fixed(
        Builtin::Section,
        &["notes".into()],
        json!({"heading":"Title"}),
        builtin::Context {
            root: root.path(),
            scope: &scope,
            owner: &owner,
            launcher: &artifactize_tools::launch::Standalone,
        },
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        result.content(),
        &[Content::Text {
            text: "# Title\nbody\n".into()
        }]
    );
    assert!(!result.is_error());
}

#[tokio::test]
async fn sections_follow_markdown_headings_not_code_blocks() {
    let root = os::tempdir();
    fs::write(root.path().join("notes"), "# Top\nintro\n## Exact\nbody\n### Child\nchild\n```md\n## Fake\n```\n## Next\nnext\n\nSetext\n======\nlast\n").unwrap();
    let owner = ArtifactId::new("owner").unwrap();
    let scope = folder_scope(&owner);
    for (heading, expected) in [
        (
            "Exact",
            "## Exact\nbody\n### Child\nchild\n```md\n## Fake\n```\n",
        ),
        ("Setext", "Setext\n======\nlast\n"),
    ] {
        let result = builtin::call_fixed(
            Builtin::Section,
            &["notes".into()],
            json!({"heading":heading}),
            builtin::Context {
                root: root.path(),
                scope: &scope,
                owner: &owner,
                launcher: &artifactize_tools::launch::Standalone,
            },
            &CancellationToken::new(),
        )
        .await;
        assert!(!result.is_error(), "{result:?}");
        assert_eq!(
            result.content(),
            &[Content::Text {
                text: expected.into()
            }]
        );
    }
    let result = builtin::call_fixed(
        Builtin::Section,
        &["notes".into()],
        json!({"heading":"missing"}),
        builtin::Context {
            root: root.path(),
            scope: &scope,
            owner: &owner,
            launcher: &artifactize_tools::launch::Standalone,
        },
        &CancellationToken::new(),
    )
    .await;
    assert!(result.is_error());
    assert_eq!(
        result.content(),
        &[Content::Text {
            text:
                "No matching heading: missing. Available headings:\nTop\nExact\nChild\nNext\nSetext"
                    .into()
        }]
    );
}
