//! Independent saved wire fixtures pin graph capture and legacy field preservation.
use super::*;
use crate::project::selection::Selection;
use serde_json::json;

#[test]
fn saved_graph_keeps_known_null_missing_and_extensible_unknown_fields() {
    let mut wire = json!({
        "version":1,
        "repoPath":"/saved/workspace",
        "selection":{"kind":"all"},
        "futureTop":{"v":7},
        "artifacts":{
            "page":{
                "path":"pages",
                "name":"page",
                "children":{},
                "basis":null,
                "fingerprint":{
                    "script":{
                        "command":crate::test_os::echo_program(),
                        "args":["v1"],
                        "files":[],
                        "timeoutMs":null,
                        "futureScript":true,
                    },
                },
                "reviewPolicy":{"dependencyGates":null,"futurePolicy":1},
                "views":{
                    "agentTools":{
                        "read":{"builtin":"read","futureTool":true},
                        "find":{
                            "description":"Find in {artifactName}",
                            "inputSchema":{"type":"object","properties":{"query":{"type":"string","maxLength":80}},"required":["query"],"additionalProperties":false},
                            "protocol":"json",
                            "command":crate::test_os::echo_program(),
                            "args":["find"],
                        },
                    },
                    "humanTools":{
                        "inspect":{
                            "description":"Inspect",
                            "kind":"output",
                            "command":crate::test_os::cat_program(),
                            "args":["file.md"],
                            "futureHumanTool":1,
                        },
                    },
                    "futureView":[],
                },
                "futureArtifact":"kept",
            },
        },
        "evals":[
            {
                "id":"page/check",
                "target":"page",
                "references":{},
                "deps":["input"],
                "declaration":{
                    "id":"check",
                    "title":"Inspect",
                    "profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[],"timeoutMs":null},
                    "profileVariants":{},
                    "payload":{"instruction":"Inspect {input}.","owner":[true,null,42]},
                    "passSchema":{"type":"object","properties":{"accepted":{"const":true}}},
                    "failSchema":null,
                    "futureDeclaration":true,
                },
                "futureEval":2,
            },
        ],
        "relations":[
            {
                "source":"input",
                "target":"page",
                "kind":"instruction",
                "evalId":"page/check",
                "name":"input",
                "cyclic":true,
                "futureRelation":false,
            },
        ],
        "components":[
            {
                "id":0,
                "artifacts":["input","page"],
                "dependencies":[],
                "gates":["input/check"],
                "cyclic":true,
                "futureComponent":3,
            },
        ],
    });
    wire["selection"]["futureSelection"] = json!({"v":true});
    wire["evals"][0]["declaration"]["profile"]["futureProfile"] = json!({"v":1});
    wire["evals"][0]["declaration"]["profileVariants"] =
        json!({"extra":{"kind":"human","futureVariant":[1,2]}});
    wire["relations"].as_array_mut().unwrap().push(
        json!({"source":"page","target":"input","kind":"future-edge","note":{"future":true}}),
    );
    let snapshot: Definitions = serde_json::from_value(wire.clone()).unwrap();
    let restored = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(restored, wire);
    // A reader decodes the saved text, and its tools' input schemas read back as saved.
    let read: Definitions = serde_json::from_str(&wire.to_string()).unwrap();
    assert_eq!(serde_json::to_value(&read).unwrap(), wire);
    let views = read
        .graph()
        .unwrap()
        .artifact("page")
        .unwrap()
        .views
        .value()
        .unwrap();
    let Some(AgentTool::Command(find)) = views.agent_tools.value().unwrap().get("find") else {
        panic!("the saved command tool reads back");
    };
    assert_eq!(
        find.input_schema.value(),
        wire["artifacts"]["page"]["views"]["agentTools"]["find"]["inputSchema"]
    );
    let graph = snapshot.graph().unwrap();
    assert_eq!(
        graph.repo_path.value().unwrap(),
        &PathBuf::from("/saved/workspace")
    );
    assert_eq!(
        graph.components()[0].artifacts.value().unwrap(),
        &["input", "page"]
    );
    assert!(matches!(graph.artifact("page").unwrap().basis, Field::Null));
    for wire in [
        Value::Null,
        json!({}),
        json!({"artifacts":null,"evals":null}),
        json!({"artifacts":{"old":{"path":"","basis":true}},"components":[{"artifacts":["old"]}]}),
    ] {
        let snapshot: Definitions = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(snapshot).unwrap(), wire);
    }
    let missing: crate::store::Run = serde_json::from_value(json!({
        "id":"run-old",
        "repoPath":"/repo",
        "stateDir":"/state",
        "status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z",
        "selection":{"kind":"all"},
        "validation":null,
    }))
    .unwrap();
    assert!(missing.definitions.graph().is_none());
}

#[test]
fn current_graph_capture_preserves_the_independently_pinned_saved_wire() {
    let root = crate::test_os::tempdir();
    crate::test_declaration::write(
        root.path().join("index.artf"),
        json!({
            "name":"app",
            "views":{"agent_tools":{"read":{"builtin":"read"}}},
            "fingerprint":{"script":{"command":crate::test_os::echo_program(),"args":["v1"]}},
            "evals":[
                {
                    "id":"check",
                    "title":"Check",
                    "profile":{"kind":"agent","backend":"openai","model":"fixture"},
                    "payload":{"instruction":"Check.","owner":{"nested":[1,2]}},
                    "pass_schema":{"type":"object"},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    let config = config::read_workspace_config(root.path()).unwrap();
    let selection = Selection::All;
    let view = crate::query::graph(&config, &selection).unwrap();
    let expected = json!({
        "version":1,
        "repoPath":crate::platform::path_text(&crate::platform::canonicalize(root.path()).unwrap()),
        "selection":{"kind":"all"},
        "artifacts":{
            "app":{
                "path":"",
                "kind":"folder",
                "name":"app",
                "tags":[],
                "children":{},
                "basis":null,
                "views":{"agentSessions":{},"agentTools":{"read":{"builtin":"read"}},"humanTools":{}},
                "fingerprint":{"script":{
                    "command":crate::test_os::echo_program(),
                    "args":["v1"],
                    "files":[],
                    "timeoutMs":null,
                }},
            },
        },
        "evals":[{
            "id":"app/check",
            "target":"app",
            "references":{},
            "deps":[],
            "declaration":{
                "id":"check",
                "title":"Check",
                "profile":{
                    "kind":"agent","backend":"openai","model":"fixture",
                    "reasoning":null,"timeoutMs":null,"maxToolCalls":null,"maxTokens":null,
                },
                "payload":{"instruction":"Check.","owner":{"nested":[1,2]}},
                "passSchema":{"type":"object"},
                "failSchema":null,
            },
        }],
        "relations":[],
        "components":[{"id":0,"artifacts":["app"],"dependencies":[],"gates":[],"cyclic":false}],
    });
    let snapshot = Definitions::from_view(&view).unwrap();
    assert_eq!(serde_json::to_value(snapshot).unwrap(), expected);
}

#[tokio::test]
async fn saved_definition_timeouts_reject_unrepresentable_writes() {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(&repo).unwrap();
    let receipts = crate::store::OwnedReceipts::open(&state, &repo)
        .await
        .unwrap();
    let mut run: crate::store::Run = serde_json::from_value(json!({
        "id":"run-definition",
        "repoPath":repo,
        "stateDir":state,
        "status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z",
        "selection":{"kind":"all"},
        "validation":null,
        "definitions":{
            "artifacts":{
                "app":{"fingerprint":{"script":{"command":crate::test_os::echo_program(),"args":[],"timeoutMs":1}}},
            },
        },
    }))
    .unwrap();
    receipts.create_run(&run, &[]).await.unwrap();
    let wire = json!({
        "artifacts":{"app":{"fingerprint":{"script":{"command":crate::test_os::echo_program(),"args":[],"timeoutMs":1}}}},
    });
    for duration in [
        std::time::Duration::ZERO,
        std::time::Duration::from_millis(2147483648),
        std::time::Duration::from_nanos(1),
    ] {
        let mut snapshot: Definitions = serde_json::from_value(wire.clone()).unwrap();
        let artifact = snapshot
            .0
            .as_mut()
            .unwrap()
            .artifacts
            .value()
            .unwrap()
            .get("app")
            .unwrap()
            .clone();
        let mut artifact = artifact;
        let mut fingerprint = artifact.fingerprint.value().unwrap().clone();
        let mut script = fingerprint.script.value().unwrap().clone();
        script.timeout_ms = Field::Value(duration);
        fingerprint.script = Field::Value(script);
        artifact.fingerprint = Field::Value(fingerprint);
        std::sync::Arc::make_mut(snapshot.0.as_mut().unwrap()).artifacts =
            Field::Value(BTreeMap::from([("app".parse().unwrap(), artifact)]));
        assert!(serde_json::to_value(&snapshot).is_err());
        run.definitions = snapshot;
        // Saving the Run writes only its state: its definitions were written with it.
        receipts.save_run(&run).await.unwrap();
        let saved = crate::store::read_run(&state, "run-definition")
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(saved.run.definitions).unwrap(), wire);
        run.id = "run-invalid-definition".parse().unwrap();
        assert!(receipts.create_run(&run, &[]).await.is_err());
        assert!(
            crate::store::read_run(&state, "run-invalid-definition")
                .await
                .is_err()
        );
        run.id = "run-definition".parse().unwrap();
    }
}

#[test]
fn saved_component_ids_and_known_declaration_keys_keep_their_wire_shapes() {
    let saved = r#"{"id":17,"artifacts":["app"],"dependencies":[0,10],"gates":["spec/check"],"cyclic":false}"#;
    let component: Component = serde_json::from_str(saved).unwrap();
    assert_eq!(serde_json::to_string(&component).unwrap(), saved);
    let saved = r#"{"children":{"nested/child":"child"}}"#;
    let artifact: Artifact = serde_json::from_str(saved).unwrap();
    assert_eq!(serde_json::to_string(&artifact).unwrap(), saved);
    let saved = r#"{"profileVariants":{"sol-max":{"kind":"human"}}}"#;
    let declaration: Declaration = serde_json::from_str(saved).unwrap();
    assert_eq!(serde_json::to_string(&declaration).unwrap(), saved);

    for invalid in ["-1", "1.5", "\"1\""] {
        assert!(serde_json::from_str::<crate::graph::ComponentId>(invalid).is_err());
    }
    for invalid in [
        json!({"artifacts":{"app":{"children":{"../outside":"child"}}}}),
        json!({"evals":[{"id":"app/check","target":"app","declaration":{"profileVariants":{"bad.name":{"kind":"human"}}}}]}),
        json!({"relations":[{"source":"input","target":"app","kind":"instruction","evalId":"app/check","name":"bad.name"}]}),
    ] {
        assert!(
            serde_json::from_value::<Definitions>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
}
